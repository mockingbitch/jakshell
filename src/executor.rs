use anyhow::{anyhow, Result};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::io::Read;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::process::Command as Process;
use std::rc::Rc;

use crate::builtins;
use crate::expand::{eval_arith, eval_index_word, expand_pattern, expand_string, expand_words, fnmatch, ParamError, UnboundVar};
use crate::lexer::WordPart;
use crate::parser::{
    Assign, AssignValue, CaseTerm, Command, Compound, Pipeline, Program, Redirect, SeqOp, SimpleCommand, Word,
};
use crate::shell::{Flow, Shell};

pub fn execute(shell: &Rc<RefCell<Shell>>, prog: &Program) -> Result<i32> {
    let mut last_code = 0;
    for (i, (pipeline, op)) in prog.items.iter().enumerate() {
        if i > 0 {
            match op {
                SeqOp::Always => {}
                SeqOp::AndIf => {
                    if last_code != 0 {
                        continue;
                    }
                }
                SeqOp::OrIf => {
                    if last_code == 0 {
                        continue;
                    }
                }
            }
        }
        // Vế trái của `&&` / `||` và lệnh bị `!` không kích hoạt `set -e`.
        let left_of_list = matches!(prog.items.get(i + 1), Some((_, SeqOp::AndIf | SeqOp::OrIf)));
        let suppress = left_of_list || pipeline.negate;
        if suppress {
            shell.borrow_mut().errexit_suppressed += 1;
        }
        let result = run_pipeline(shell, pipeline);
        if suppress {
            shell.borrow_mut().errexit_suppressed -= 1;
        }
        // Lỗi của 1 lệnh (file redirect không mở được, biến chưa gán…) chỉ làm
        // hỏng lệnh đó — script chạy tiếp như bash.
        let code = match result {
            Ok(code) => code,
            Err(e) => {
                report_error(&e);
                let fatal = if e.downcast_ref::<UnboundVar>().is_some() {
                    Some(127)
                } else if e.downcast_ref::<ParamError>().is_some() {
                    Some(127)
                } else {
                    None
                };
                if let Some(code) = fatal {
                    // `set -u` / `${x:?}`: script dừng hẳn; phiên tương tác bỏ phần còn lại của dòng.
                    if !crate::jobctl::interactive() {
                        exit_shell(shell, code);
                    }
                    shell.borrow_mut().set_last_status(code);
                    return Ok(code);
                }
                1
            }
        };
        last_code = if pipeline.negate { (code == 0) as i32 } else { code };
        shell.borrow_mut().set_last_status(last_code);
        // trap ERR: cùng điều kiện với `set -e` (không trong điều kiện / `&&`…).
        if last_code != 0 && !suppress {
            let run_err = {
                let sh = shell.borrow();
                sh.errexit_suppressed == 0 && !sh.in_trap && sh.traps.get("ERR").is_some_and(|a| !a.is_empty())
            };
            if run_err {
                run_trap(shell, "ERR");
            }
        }
        run_pending_traps(shell);
        // Ctrl-C giữa chừng → bỏ phần còn lại (kể cả vòng lặp bao ngoài):
        // `sleep 9; rm x` mà ^C thì KHÔNG chạy `rm x`, như bash.
        if crate::jobctl::interrupted() {
            last_code = 130;
            break;
        }
        if shell.borrow().flow != Flow::Normal {
            break;
        }
        // `set -e`: lệnh lỗi làm thoát shell (trừ trong điều kiện if/while, `&&`…).
        let errexit = {
            let sh = shell.borrow();
            sh.opts.errexit && sh.errexit_suppressed == 0
        };
        if last_code != 0 && errexit && !suppress {
            exit_shell(shell, last_code);
        }
    }
    shell.borrow_mut().reap_jobs();
    Ok(last_code)
}

fn report_error(e: &anyhow::Error) {
    if unsafe { libc::isatty(libc::STDERR_FILENO) } == 1 {
        eprintln!("\x1b[31mjaksh: {e}\x1b[0m");
    } else {
        eprintln!("jaksh: {e}");
    }
}

/// Chạy lệnh của trap `name`. `$?` được giữ nguyên sau khi trap chạy xong.
pub fn run_trap(shell: &Rc<RefCell<Shell>>, name: &str) {
    let Some(action) = shell.borrow().traps.get(name).cloned().filter(|a| !a.is_empty()) else {
        return;
    };
    let (saved_status, was_in_trap) = {
        let mut sh = shell.borrow_mut();
        (sh.last_status(), std::mem::replace(&mut sh.in_trap, true))
    };
    match crate::lexer::tokenize(&action).and_then(|t| crate::parser::parse(&t)) {
        Ok(prog) => {
            let _ = execute(shell, &prog);
        }
        Err(e) => eprintln!("jaksh: trap {}: {}", name, e),
    }
    let mut sh = shell.borrow_mut();
    sh.in_trap = was_in_trap;
    sh.set_last_status(saved_status);
}

/// Chạy trap cho các tín hiệu đã tới (sau khi lệnh foreground xong, như bash).
/// Có trap INT → Ctrl-C không huỷ phần còn lại của script nữa.
pub fn run_pending_traps(shell: &Rc<RefCell<Shell>>) {
    for sig in crate::jobctl::take_pending_signals() {
        let Some((name, _)) = crate::builtins::trap_name(&sig.to_string()) else { continue };
        if shell.borrow().traps.get(&name).is_some_and(|a| !a.is_empty()) {
            run_trap(shell, &name);
            if sig == libc::SIGINT {
                crate::jobctl::take_interrupted();
            }
        }
    }
}

/// trap EXIT — chạy đúng 1 lần khi shell thoát (mọi đường thoát).
pub fn run_exit_trap(shell: &Rc<RefCell<Shell>>) {
    if shell.borrow().traps.contains_key("EXIT") {
        run_trap(shell, "EXIT");
        shell.borrow_mut().traps.remove("EXIT");
    }
}

/// Thoát shell (dùng cho `set -e` / `set -u`) — dọn job control như `exit`.
fn exit_shell(shell: &Rc<RefCell<Shell>>, code: i32) -> ! {
    shell.borrow_mut().set_last_status(code);
    run_exit_trap(shell);
    if crate::jobctl::interactive() {
        crate::jobctl::shutdown(shell);
    }
    flush_std();
    std::process::exit(code);
}

fn flush_std() {
    use std::io::Write;
    let _ = std::io::stdout().flush();
    let _ = std::io::stderr().flush();
}

/// `set -x`: in lệnh đã expand ra stderr, tiền tố `$PS4` (mặc định `+ `).
fn xtrace(shell: &Rc<RefCell<Shell>>, assigns: &[(String, String)], argv: &[String]) {
    if !shell.borrow().opts.xtrace {
        return;
    }
    let ps4 = shell.borrow().get_var("PS4").unwrap_or_else(|| "+ ".into());
    let words: Vec<String> = assigns
        .iter()
        .map(|(k, v)| format!("{}={}", k, builtins::sq(v)))
        .chain(argv.iter().map(|a| builtins::sq(a)))
        .collect();
    eprintln!("{}{}", ps4, words.join(" "));
}

fn run_pipeline(shell: &Rc<RefCell<Shell>>, pipeline: &Pipeline) -> Result<i32> {
    let timer = pipeline.timed.then(TimeReport::start);
    let code = if pipeline.background {
        run_background(shell, pipeline)
    } else if pipeline.commands.len() == 1 {
        let code = run_command(shell, &pipeline.commands[0], false);
        if let Ok(c) = &code {
            shell.borrow_mut().set_pipestatus(&[*c]);
        }
        code
    } else {
        spawn_pipeline(shell, &pipeline.commands, false)
    };
    if let Some(t) = timer {
        t.finish();
    }
    code
}

/// `cmd &` / `{ …; } &` / `a | b &` — chạy nền trong subshell (fork).
fn run_background(shell: &Rc<RefCell<Shell>>, pipeline: &Pipeline) -> Result<i32> {
    if pipeline.commands.len() > 1 {
        return spawn_pipeline(shell, &pipeline.commands, true);
    }
    let cmd = &pipeline.commands[0];
    let pid = fork_child(shell, 0, false, ChildKind::Job, || {}, |sh| run_command(sh, cmd, true).unwrap_or(1))?;
    let mut sh = shell.borrow_mut();
    sh.last_bg_pid = Some(pid);
    let id = sh.add_job(pid, vec![pid], command_label(cmd));
    if crate::jobctl::interactive() {
        println!("\x1b[2m[{}] {}\x1b[0m", id, pid);
    }
    Ok(0)
}

/// Nhãn ngắn của lệnh cho `jobs`.
fn command_label(cmd: &Command) -> String {
    match cmd {
        Command::Simple(sc) => sc.words.iter().map(|w| crate::lexer::parts_to_source(w)).collect::<Vec<_>>().join(" "),
        Command::Compound(c, _) => match **c {
            Compound::Group(_) => "{ … }".into(),
            Compound::Subshell(_) => "( … )".into(),
            Compound::If(..) => "if …".into(),
            Compound::For { .. } | Compound::ArithFor { .. } => "for …".into(),
            Compound::While { until, .. } => if until { "until …" } else { "while …" }.into(),
            Compound::Case { .. } => "case …".into(),
            Compound::Cond(_) => "[[ … ]]".into(),
            Compound::Arith(ref e) => format!("(({}))", e),
        },
        Command::FuncDef(n, _) => format!("{}()", n),
    }
}

/// Chạy 1 lệnh trong shell hiện tại. `exec_last`: đang ở tiến trình con của
/// pipeline — lệnh ngoài được `exec` thẳng (thay tiến trình) thay vì spawn.
pub fn run_command(shell: &Rc<RefCell<Shell>>, cmd: &Command, exec_last: bool) -> Result<i32> {
    match cmd {
        Command::Simple(sc) => {
            let r = run_simple(shell, sc, exec_last);
            finish_procsubs();
            r
        }
        Command::Compound(c, redirects) => {
            let _guard = crate::redirect::apply_in_process(crate::redirect::plan(shell, redirects)?)?;
            run_compound(shell, c)
        }
        Command::FuncDef(name, body) => {
            shell.borrow_mut().functions.insert(name.clone(), Rc::clone(body));
            Ok(0)
        }
    }
}

// ── Lệnh ghép ───────────────────────────────────────────────────────────────

/// Chạy điều kiện (`if`, `while`) — `set -e` không áp bên trong.
fn run_condition(shell: &Rc<RefCell<Shell>>, cond: &Program) -> Result<i32> {
    shell.borrow_mut().errexit_suppressed += 1;
    let r = execute(shell, cond);
    shell.borrow_mut().errexit_suppressed -= 1;
    r
}

/// Sau thân vòng lặp: xử lý `break` / `continue` / `return` / Ctrl-C.
/// Trả về true nếu phải thoát vòng lặp.
fn loop_should_exit(shell: &Rc<RefCell<Shell>>) -> bool {
    if crate::jobctl::interrupted() {
        return true;
    }
    let mut sh = shell.borrow_mut();
    match sh.flow {
        Flow::Normal => false,
        Flow::Break(n) => {
            sh.flow = if n > 1 { Flow::Break(n - 1) } else { Flow::Normal };
            true
        }
        Flow::Continue(n) => {
            if n > 1 {
                sh.flow = Flow::Continue(n - 1);
                true
            } else {
                sh.flow = Flow::Normal;
                false
            }
        }
        Flow::Return => true,
    }
}

fn run_loop_body(shell: &Rc<RefCell<Shell>>, body: &Program) -> Result<(i32, bool)> {
    shell.borrow_mut().loop_depth += 1;
    let r = execute(shell, body);
    shell.borrow_mut().loop_depth -= 1;
    let code = r?;
    Ok((code, loop_should_exit(shell)))
}

fn run_compound(shell: &Rc<RefCell<Shell>>, c: &Compound) -> Result<i32> {
    match c {
        Compound::Group(p) => execute(shell, p),
        Compound::Subshell(p) => {
            let pid = fork_child(shell, 0, true, ChildKind::Job, || {}, |sh| execute(sh, p).unwrap_or(1))?;
            Ok(crate::jobctl::wait_foreground(shell, pid, &[pid], "( … )", None))
        }
        Compound::If(arms, else_body) => {
            for (cond, body) in arms {
                let c = run_condition(shell, cond)?;
                if shell.borrow().flow != Flow::Normal || crate::jobctl::interrupted() {
                    return Ok(c);
                }
                if c == 0 {
                    return execute(shell, body);
                }
            }
            match else_body {
                Some(b) => execute(shell, b),
                None => Ok(0),
            }
        }
        Compound::While { cond, body, until } => {
            let mut code = 0;
            loop {
                let c = run_condition(shell, cond)?;
                if shell.borrow().flow != Flow::Normal || crate::jobctl::interrupted() {
                    break;
                }
                if (c == 0) == *until {
                    break;
                }
                let (bc, exit) = run_loop_body(shell, body)?;
                code = bc;
                if exit {
                    break;
                }
            }
            Ok(code)
        }
        Compound::For { var, items, body } => {
            let values = match items {
                Some(words) => expand_words(shell, words)?,
                None => shell.borrow().positional.clone(),
            };
            let mut code = 0;
            for v in values {
                shell.borrow_mut().set_var(var, &v)?;
                let (bc, exit) = run_loop_body(shell, body)?;
                code = bc;
                if exit {
                    break;
                }
            }
            Ok(code)
        }
        Compound::ArithFor { init, cond, step, body } => {
            if !init.is_empty() {
                eval_arith(shell, init)?;
            }
            let mut code = 0;
            loop {
                if !cond.is_empty() && eval_arith(shell, cond)? == 0 {
                    break;
                }
                let (bc, exit) = run_loop_body(shell, body)?;
                code = bc;
                if exit {
                    break;
                }
                if !step.is_empty() {
                    eval_arith(shell, step)?;
                }
            }
            Ok(code)
        }
        Compound::Case { word, arms } => {
            let subject = expand_string(shell, word)?;
            let nocase = shell.borrow().shopts.nocasematch;
            let mut code = 0;
            let mut fall_through = false;
            for arm in arms {
                let mut matched = fall_through;
                if !matched {
                    for p in &arm.patterns {
                        if fnmatch(&expand_pattern(shell, p)?, &subject, nocase) {
                            matched = true;
                            break;
                        }
                    }
                }
                if !matched {
                    continue;
                }
                code = execute(shell, &arm.body)?;
                if shell.borrow().flow != Flow::Normal {
                    return Ok(code);
                }
                match arm.term {
                    CaseTerm::Break => return Ok(code),
                    CaseTerm::FallThrough => fall_through = true,
                    CaseTerm::Continue => fall_through = false,
                }
            }
            Ok(code)
        }
        Compound::Cond(e) => {
            if shell.borrow().opts.xtrace {
                eprintln!("{}[[ … ]]", shell.borrow().get_var("PS4").unwrap_or_else(|| "+ ".into()));
            }
            match crate::cond::eval(shell, e) {
                Ok(true) => Ok(0),
                Ok(false) => Ok(1),
                Err(err) => {
                    report_error(&err);
                    Ok(2)
                }
            }
        }
        Compound::Arith(expr) => match eval_arith(shell, expr) {
            Ok(v) => Ok((v == 0) as i32),
            Err(err) => {
                report_error(&err);
                Ok(1)
            }
        },
    }
}

// ── Lệnh đơn ────────────────────────────────────────────────────────────────

/// Alias: chỉ áp cho từ đầu là chữ thường không nháy (`\ls`, `'ls'` bỏ qua
/// alias như bash). Lặp được (alias trỏ alias) nhưng không vòng lại chính nó.
fn expand_aliases(shell: &Rc<RefCell<Shell>>, words: &[Word]) -> Vec<Word> {
    let mut words = words.to_vec();
    if !shell.borrow().shopts.expand_aliases {
        return words;
    }
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    loop {
        let head = match words.first().map(|w| w.as_slice()) {
            Some([WordPart::Literal(s)]) => s.clone(),
            _ => return words,
        };
        if !seen.insert(head.clone()) {
            return words;
        }
        let Some(val) = shell.borrow().aliases.get(&head).cloned() else {
            return words;
        };
        let Ok(tokens) = crate::lexer::tokenize(&val) else {
            return words;
        };
        let mut new_words: Vec<Word> = tokens
            .into_iter()
            .filter_map(|t| match t {
                crate::lexer::Token::Word(parts) => Some(parts),
                _ => None,
            })
            .collect();
        if new_words.is_empty() {
            return words;
        }
        new_words.extend(words.into_iter().skip(1));
        words = new_words;
    }
}

/// Mở rộng argv. Với declare/local/export/readonly, đối số dạng `x=...` không
/// bị tách từ / glob (như bash).
fn build_argv(shell: &Rc<RefCell<Shell>>, words: &[Word]) -> Result<Vec<String>> {
    let decl = matches!(words.first().map(|w| w.as_slice()), Some([WordPart::Literal(s)]) if crate::parser::is_decl_command(s));
    if !decl {
        return expand_words(shell, words);
    }
    let mut argv = Vec::new();
    for w in words {
        let assignment_like = matches!(w.first(), Some(WordPart::Literal(s)) if s.contains('=') && !s.starts_with('-'));
        if assignment_like {
            argv.push(expand_string(shell, w)?);
        } else {
            argv.extend(expand_words(shell, std::slice::from_ref(w))?);
        }
    }
    Ok(argv)
}

/// Giá trị phép gán: không tách từ / glob; `~` đầu giá trị được thay.
fn assign_value(shell: &Rc<RefCell<Shell>>, value: &Word) -> Result<String> {
    if let Some(WordPart::Literal(s)) = value.first() {
        if s == "~" || s.starts_with("~/") || s.starts_with("~:") {
            let home = shell.borrow().get_var("HOME").unwrap_or_default();
            let mut w = value.clone();
            w[0] = WordPart::Literal(format!("{}{}", home, &s[1..]));
            return expand_string(shell, &w);
        }
    }
    expand_string(shell, value)
}

/// Thực hiện 1 phép gán (bền vững) — scalar, phần tử mảng, hoặc cả mảng.
pub fn perform_assign(shell: &Rc<RefCell<Shell>>, a: &Assign) -> Result<()> {
    match &a.value {
        AssignValue::Scalar(w) => {
            let v = assign_value(shell, w)?;
            match &a.index {
                Some(raw) => {
                    let idx = eval_index_word(shell, &a.name, raw)?;
                    let v = if a.append { shell.borrow().get_index(&a.name, &idx).unwrap_or_default() + &v } else { v };
                    shell.borrow_mut().set_index(&a.name, idx, &v)
                }
                None => {
                    let v = if shell.borrow().integers.contains(&a.name) {
                        // `declare -i`: giá trị được tính như biểu thức số học.
                        let n = eval_arith(shell, &v)?;
                        let base = if a.append { eval_arith(shell, &a.name)? } else { 0 };
                        (base + n).to_string()
                    } else if a.append {
                        shell.borrow().get_var(&a.name).unwrap_or_default() + &v
                    } else {
                        v
                    };
                    shell.borrow_mut().set_var(&a.name, &v)
                }
            }
        }
        AssignValue::Array(items) => {
            if shell.borrow().is_assoc(&a.name) {
                let mut map = if a.append { shell.borrow().assocs.get(&a.name).cloned().unwrap_or_default() } else { BTreeMap::new() };
                for item in items {
                    let (key, val) = split_keyed(shell, item)?;
                    let key = key.ok_or_else(|| anyhow!("{}: mảng kết hợp cần dạng [khoá]=giá_trị", a.name))?;
                    map.insert(key, val);
                }
                return shell.borrow_mut().set_assoc(&a.name, map);
            }
            let mut arr = if a.append { shell.borrow().arrays.get(&a.name).cloned().unwrap_or_default() } else { BTreeMap::new() };
            let mut next = arr.keys().next_back().map_or(0, |k| k + 1);
            for item in items {
                match split_keyed(shell, item)? {
                    (Some(key), val) => {
                        let i = eval_arith(shell, &key)?;
                        arr.insert(i, val);
                        next = i + 1;
                    }
                    (None, _) => {
                        // Phần tử thường: expand đầy đủ (tách từ, glob) như bash.
                        for v in expand_words(shell, std::slice::from_ref(item))? {
                            arr.insert(next, v);
                            next += 1;
                        }
                    }
                }
            }
            shell.borrow_mut().set_array(&a.name, arr)
        }
    }
}

/// Phần tử `[k]=v` trong `(…)` → (Some(k), v); phần tử thường → (None, "").
fn split_keyed(shell: &Rc<RefCell<Shell>>, item: &Word) -> Result<(Option<String>, String)> {
    if let Some(WordPart::Literal(s)) = item.first() {
        if let Some(rest) = s.strip_prefix('[') {
            if let Some(close) = rest.find("]=") {
                let key = rest[..close].to_string();
                let mut value: Word = Vec::new();
                let tail = &rest[close + 2..];
                if !tail.is_empty() {
                    value.push(WordPart::Literal(tail.to_string()));
                }
                value.extend(item[1..].iter().cloned());
                let key = expand_string(shell, &crate::lexer::parse_embedded(&key)?)?;
                return Ok((Some(key), expand_string(shell, &value)?));
            }
        }
    }
    Ok((None, String::new()))
}

fn run_simple(shell: &Rc<RefCell<Shell>>, sc: &SimpleCommand, exec_last: bool) -> Result<i32> {
    let words = expand_aliases(shell, &sc.words);
    shell.borrow_mut().last_cmdsub_status = None;
    let mut argv = build_argv(shell, &words)?;

    if argv.is_empty() {
        // Chỉ có phép gán / redirect: gán bền vững, tạo file redirect.
        for a in &sc.assigns {
            if let Err(e) = perform_assign(shell, a) {
                // Gán vào biến readonly: script dừng hẳn (POSIX), REPL chỉ báo lỗi.
                if !crate::jobctl::interactive() {
                    report_error(&e);
                    exit_shell(shell, 1);
                }
                return Err(e);
            }
        }
        if shell.borrow().opts.xtrace {
            let shown: Vec<(String, String)> = sc
                .assigns
                .iter()
                .map(|a| (a.name.clone(), shell.borrow().get_var(&a.name).unwrap_or_default()))
                .collect();
            xtrace(shell, &shown, &[]);
        }
        if !sc.redirects.is_empty() {
            drop(crate::redirect::plan(shell, &sc.redirects)?);
        }
        // `x=$(false)` → `$?` = exit code của lệnh con.
        return Ok(shell.borrow_mut().last_cmdsub_status.take().unwrap_or(0));
    }

    // Phép gán tạm cho riêng lệnh này (`VAR=v cmd`).
    let mut temp: Vec<(String, String)> = Vec::new();
    for a in &sc.assigns {
        let AssignValue::Scalar(w) = &a.value else {
            return Err(anyhow!("{}: không gán mảng tạm cho lệnh được", a.name));
        };
        let mut v = assign_value(shell, w)?;
        if a.append {
            v = shell.borrow().get_var(&a.name).unwrap_or_default() + &v;
        }
        temp.push((a.name.clone(), v));
    }
    xtrace(shell, &temp, &argv);

    // `command x …` bỏ qua function/alias; `builtin x …` bắt buộc builtin.
    let mut skip_functions = false;
    let mut only_builtin = false;
    match argv[0].as_str() {
        "command" if argv.len() > 1 && !argv[1].starts_with('-') => {
            argv.remove(0);
            skip_functions = true;
        }
        "builtin" if argv.len() > 1 => {
            argv.remove(0);
            if !builtins::is_builtin(&argv[0]) {
                return Err(anyhow!("builtin: {}: không phải lệnh builtin", argv[0]));
            }
            only_builtin = true;
        }
        _ => {}
    }

    // Function (ưu tiên hơn builtin và lệnh ngoài, như bash).
    if !skip_functions && !only_builtin {
        let func = shell.borrow().functions.get(&argv[0]).cloned();
        if let Some(body) = func {
            let _guard = crate::redirect::apply_in_process(crate::redirect::plan(shell, &sc.redirects)?)?;
            return with_temp_vars(shell, &temp, true, |sh| call_function(sh, &argv[0], &body, &argv[1..]));
        }
    }

    if !only_builtin {
        // Cờ --jak (do JakShell xử lý, không truyền xuống lệnh thật)
        if argv.iter().any(|s| s == "--jak") && crate::pretty::supports(&argv[0]) {
            let cleaned: Vec<String> = argv.iter().filter(|s| *s != "--jak").cloned().collect();
            return crate::pretty::run(shell, &cleaned, &sc.redirects);
        }
        // curl tương tác: tự động capture + format response (separator
        // + pretty JSON). CHỈ khi stdout là TTY và không redirect —
        // pipe (`curl | jq`) hay `> file` vẫn nhận raw output như cũ.
        // Tải file / stream / -v / -w tự chọn → chạy nguyên bản.
        if argv[0] == "curl"
            && sc.redirects.is_empty()
            && shell.borrow().ui.pretty_curl
            && crate::pretty::curl_wants_pretty(&argv[1..])
            && stdout_is_tty()
        {
            return crate::pretty::run(shell, &argv, &sc.redirects);
        }
    }

    if builtins::is_builtin(&argv[0]) {
        let result = with_temp_vars(shell, &temp, false, |sh| builtins::run(sh, &argv, &sc.redirects));
        // `declare -a a=(…)` / `local a=(…)`: builtin đã khai báo tên, giờ gán mảng.
        if result.is_ok() {
            for (_, a) in &sc.array_args {
                perform_assign(shell, a)?;
            }
        }
        return result;
    }

    // Special "jak" subcommand router (runs in our process)
    if argv[0] == "jak" {
        let _guard = crate::redirect::apply_in_process(crate::redirect::plan(shell, &sc.redirects)?)?;
        return crate::jak::run(shell, &argv[1..]);
    }

    spawn_external(shell, &argv, &temp, &sc.redirects, exec_last)
}

/// Gán tạm `VAR=v` trong lúc chạy builtin / function rồi khôi phục. Với
/// function, biến tạm còn được export (lệnh con bên trong thấy), như bash.
fn with_temp_vars<T>(
    shell: &Rc<RefCell<Shell>>,
    temp: &[(String, String)],
    export: bool,
    f: impl FnOnce(&Rc<RefCell<Shell>>) -> Result<T>,
) -> Result<T> {
    let saved: Vec<(String, Option<String>, bool)> = temp
        .iter()
        .map(|(k, _)| {
            let sh = shell.borrow();
            (k.clone(), sh.get_var(k), sh.exported.contains(k))
        })
        .collect();
    for (k, v) in temp {
        let mut sh = shell.borrow_mut();
        if export {
            sh.export_var(k, Some(v))?;
        } else {
            sh.set_var(k, v)?;
        }
    }
    let result = f(shell);
    for (k, old, was_exported) in saved {
        let mut sh = shell.borrow_mut();
        let _ = match old {
            Some(v) => sh.set_var(&k, &v),
            None => sh.unset_var(&k),
        };
        if export && !was_exported {
            sh.unexport_var(&k);
        }
    }
    result
}

fn call_function(shell: &Rc<RefCell<Shell>>, name: &str, body: &Rc<Command>, args: &[String]) -> Result<i32> {
    if shell.borrow().funcnames.len() >= 1000 {
        return Err(anyhow!("{}: gọi đệ quy quá sâu (> 1000 tầng)", name));
    }
    let saved_positional = {
        let mut sh = shell.borrow_mut();
        sh.funcnames.push(name.to_string());
        sh.local_frames.push(Vec::new());
        std::mem::replace(&mut sh.positional, args.to_vec())
    };
    // Vòng lặp bên ngoài function không bị `break` trong function chạm tới.
    let saved_loop = std::mem::replace(&mut shell.borrow_mut().loop_depth, 0);
    let result = run_command(shell, body, false);
    let mut sh = shell.borrow_mut();
    sh.loop_depth = saved_loop;
    sh.pop_locals();
    sh.funcnames.pop();
    sh.positional = saved_positional;
    if sh.flow == Flow::Return {
        sh.flow = Flow::Normal;
        // `return` không đối số → exit code lệnh cuối (đã nằm trong $?).
        return Ok(sh.last_status());
    }
    if matches!(sh.flow, Flow::Break(_) | Flow::Continue(_)) {
        sh.flow = Flow::Normal;
    }
    result
}

fn spawn_external(
    shell: &Rc<RefCell<Shell>>,
    argv: &[String],
    assigns: &[(String, String)],
    redirects: &[Redirect],
    exec_last: bool,
) -> Result<i32> {
    let prog = &argv[0];
    let args = &argv[1..];

    let mut builder = Process::new(prog);
    builder.args(args);
    builder.envs(assigns.iter().map(|(k, v)| (k, v)));
    builder.current_dir(&shell.borrow().cwd);
    crate::redirect::apply_to_command(&mut builder, crate::redirect::plan(shell, redirects)?);

    if exec_last {
        // Tiến trình con của pipeline: thay luôn bằng lệnh thật.
        use std::os::unix::process::CommandExt;
        crate::jobctl::reset_for_exec(&mut builder);
        flush_std();
        let err = builder.exec();
        return Ok(not_found_or_failed(prog, &err));
    }

    crate::jobctl::prepare(&mut builder, 0, true);
    match builder.spawn() {
        Ok(child) => {
            let pid = child.id() as i32;
            let pgid = crate::jobctl::after_spawn(pid, 0, true);
            Ok(crate::jobctl::wait_foreground(shell, pgid, &[pid], &argv.join(" "), None))
        }
        Err(e) => {
            crate::jobctl::reclaim_terminal();
            Ok(not_found_or_failed(prog, &e))
        }
    }
}

fn not_found_or_failed(prog: &str, e: &std::io::Error) -> i32 {
    if e.kind() == std::io::ErrorKind::NotFound {
        eprintln!("jaksh: {}: {}", crate::i18n::t("common.not_found"), prog);
        127
    } else {
        eprintln!("jaksh: không chạy được {}: {}", prog, e);
        126
    }
}

/// stdout có phải terminal không — quyết định có bật auto-pretty (curl, …).
fn stdout_is_tty() -> bool {
    unsafe { libc::isatty(libc::STDOUT_FILENO) == 1 }
}

// ── Fork: pipeline, subshell, nền, $(...), <(...) ───────────────────────────

#[derive(Clone, Copy, PartialEq, Eq)]
enum ChildKind {
    /// Thành viên của 1 job (pipeline / subshell / nền) — có job control.
    Job,
    /// `$(...)` / `<(...)` — ở chung group với shell, không job control.
    Capture,
}

/// Fork một subshell: con chạy `setup` (nối fd) rồi `body`, thoát bằng exit
/// code của body; cha nhận pid. Biến / thư mục / function của con là bản sao —
/// thay đổi không ảnh hưởng shell cha (đúng ngữ nghĩa subshell của bash).
fn fork_child(
    shell: &Rc<RefCell<Shell>>,
    pgid: i32,
    foreground: bool,
    kind: ChildKind,
    setup: impl FnOnce(),
    body: impl FnOnce(&Rc<RefCell<Shell>>) -> i32,
) -> Result<i32> {
    flush_std();
    let pid = unsafe { libc::fork() };
    if pid < 0 {
        return Err(anyhow!("fork thất bại: {}", std::io::Error::last_os_error()));
    }
    if pid == 0 {
        match kind {
            ChildKind::Job => crate::jobctl::child_setup(pgid, foreground),
            ChildKind::Capture => crate::jobctl::child_setup_capture(),
        }
        // Subshell không kế thừa trap (trừ tín hiệu đang bị bỏ qua `trap ''`).
        {
            let mut sh = shell.borrow_mut();
            let active: Vec<String> = sh.traps.iter().filter(|(_, a)| !a.is_empty()).map(|(n, _)| n.clone()).collect();
            for name in active {
                sh.traps.remove(&name);
                if let Some((_, Some(sig))) = crate::builtins::trap_name(&name) {
                    crate::jobctl::set_signal_disposition(sig, crate::jobctl::Disposition::Default);
                }
            }
        }
        setup();
        let code = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| body(shell))).unwrap_or(1);
        // trap EXIT đặt bên trong subshell chạy khi subshell kết thúc.
        shell.borrow_mut().set_last_status(code);
        run_exit_trap(shell);
        flush_std();
        unsafe { libc::_exit(code) };
    }
    if kind == ChildKind::Job {
        crate::jobctl::after_spawn(pid, pgid, foreground);
    }
    Ok(pid)
}

fn os_pipe() -> Result<(OwnedFd, OwnedFd)> {
    let mut fds = [0; 2];
    #[cfg(any(target_os = "linux", target_os = "android"))]
    let rc = unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC) };
    // macOS has no pipe2: set FD_CLOEXEC right after creating the pipe.
    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    let rc = unsafe { libc::pipe(fds.as_mut_ptr()) };
    if rc < 0 {
        return Err(anyhow!("pipe: {}", std::io::Error::last_os_error()));
    }
    let pair = unsafe { (OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1])) };
    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    for fd in fds {
        if unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) } < 0 {
            return Err(anyhow!("fcntl: {}", std::io::Error::last_os_error()));
        }
    }
    Ok(pair)
}

fn spawn_pipeline(shell: &Rc<RefCell<Shell>>, commands: &[Command], background: bool) -> Result<i32> {
    let n = commands.len();
    let mut pids = Vec::with_capacity(n);
    let mut pgid = 0;
    let mut prev_read: Option<OwnedFd> = None;
    let mut failure = None;

    for (i, cmd) in commands.iter().enumerate() {
        let next = if i + 1 < n {
            match os_pipe() {
                Ok(p) => Some(p),
                Err(e) => {
                    failure = Some(e);
                    break;
                }
            }
        } else {
            None
        };
        let stdin_fd = prev_read.as_ref().map(|f| f.as_raw_fd());
        let stdout_fd = next.as_ref().map(|(_, w)| w.as_raw_fd());
        let next_read_fd = next.as_ref().map(|(r, _)| r.as_raw_fd());
        let setup = move || unsafe {
            if let Some(fd) = stdin_fd {
                libc::dup2(fd, 0);
                libc::close(fd);
            }
            if let Some(fd) = stdout_fd {
                libc::dup2(fd, 1);
                libc::close(fd);
            }
            if let Some(fd) = next_read_fd {
                libc::close(fd);
            }
        };
        // Mỗi thành viên là 1 subshell; lệnh ngoài trong đó được exec thẳng.
        match fork_child(shell, pgid, !background, ChildKind::Job, setup, |sh| run_command(sh, cmd, true).unwrap_or_else(|e| {
            report_error(&e);
            1
        })) {
            Ok(pid) => {
                if pgid == 0 {
                    pgid = pid;
                }
                pids.push(pid);
            }
            Err(e) => {
                failure = Some(e);
                break;
            }
        }
        // Cha đóng đầu ghi + đầu đọc cũ; giữ đầu đọc mới cho lệnh sau.
        prev_read = next.map(|(r, _w)| r);
    }
    drop(prev_read);

    let label = commands.iter().map(command_label).collect::<Vec<_>>().join(" | ");
    if background {
        let mut sh = shell.borrow_mut();
        sh.last_bg_pid = pids.last().copied();
        let id = sh.add_job(pgid, pids.clone(), label);
        if crate::jobctl::interactive() {
            println!("\x1b[2m[{}] {}\x1b[0m", id, pids.last().copied().unwrap_or(0));
        }
        return failure.map_or(Ok(0), Err);
    }
    if pids.is_empty() {
        return Err(failure.unwrap_or_else(|| anyhow!("pipeline rỗng")));
    }
    let code = crate::jobctl::wait_foreground(shell, pgid, &pids, &label, None);
    failure.map_or(Ok(code), Err)
}

/// `$(...)`: chạy trong subshell (fork), trả stdout đã bỏ newline cuối. Thay
/// đổi biến / `cd` bên trong không rò ra ngoài, như bash.
pub fn command_substitution(shell: &Rc<RefCell<Shell>>, src: &str) -> Result<String> {
    let tokens = crate::lexer::tokenize(src)?;
    let prog = crate::parser::parse(&tokens)?;
    let (read_end, write_end) = os_pipe()?;
    let wfd = write_end.as_raw_fd();
    let rfd = read_end.as_raw_fd();
    let pid = fork_child(
        shell,
        0,
        false,
        ChildKind::Capture,
        move || unsafe {
            libc::dup2(wfd, 1);
            libc::close(wfd);
            libc::close(rfd);
        },
        |sh| {
            let code = execute(sh, &prog).unwrap_or(1);
            // `exit` bên trong đã thoát; còn lại trả exit code lệnh cuối.
            code
        },
    )?;
    drop(write_end);
    let mut out = Vec::new();
    let mut file = std::fs::File::from(read_end);
    let _ = file.read_to_end(&mut out);
    let status = wait_pid(pid);
    shell.borrow_mut().last_cmdsub_status = Some(status);
    let mut s = String::from_utf8_lossy(&out).into_owned();
    // Bỏ mọi newline ở cuối (đúng như POSIX command substitution).
    let end = s.trim_end_matches('\n').len();
    s.truncate(end);
    Ok(s)
}

/// Chờ 1 tiến trình con, trả exit code kiểu bash.
fn wait_pid(pid: i32) -> i32 {
    loop {
        let mut st = 0;
        let r = unsafe { libc::waitpid(pid, &mut st, 0) };
        if r < 0 {
            if std::io::Error::last_os_error().raw_os_error() == Some(libc::EINTR) {
                continue;
            }
            return 0;
        }
        if libc::WIFSIGNALED(st) {
            return 128 + libc::WTERMSIG(st);
        }
        return libc::WEXITSTATUS(st);
    }
}

thread_local! {
    /// fd + pid của các `<(...)` / `>(...)` đang mở cho lệnh hiện tại.
    static PROCSUBS: RefCell<Vec<(OwnedFd, i32)>> = const { RefCell::new(Vec::new()) };
}

/// `<(cmd)` / `>(cmd)`: chạy cmd nối với 1 pipe, trả về `/dev/fd/N`.
pub fn process_substitution(shell: &Rc<RefCell<Shell>>, src: &str, input: bool) -> Result<String> {
    let tokens = crate::lexer::tokenize(src)?;
    let prog = crate::parser::parse(&tokens)?;
    let (read_end, write_end) = os_pipe()?;
    let (child_end, parent_end) = if input { (write_end, read_end) } else { (read_end, write_end) };
    let cfd = child_end.as_raw_fd();
    let pfd = parent_end.as_raw_fd();
    let target = if input { 1 } else { 0 };
    let pid = fork_child(
        shell,
        0,
        false,
        ChildKind::Capture,
        move || unsafe {
            libc::dup2(cfd, target);
            libc::close(cfd);
            libc::close(pfd);
        },
        |sh| execute(sh, &prog).unwrap_or(1),
    )?;
    drop(child_end);
    // Lệnh chính (tiến trình con) phải thừa kế được fd này → bỏ CLOEXEC.
    unsafe { libc::fcntl(pfd, libc::F_SETFD, 0) };
    PROCSUBS.with(|p| p.borrow_mut().push((parent_end, pid)));
    Ok(format!("/dev/fd/{}", pfd))
}

/// Sau mỗi lệnh đơn: đóng fd của `<(...)` và thu dọn tiến trình của chúng.
fn finish_procsubs() {
    let list = PROCSUBS.with(|p| std::mem::take(&mut *p.borrow_mut()));
    for (fd, pid) in list {
        drop(fd);
        wait_pid(pid);
    }
}

/// `time pipeline`: in real / user / sys ra stderr.
struct TimeReport {
    start: std::time::Instant,
    user: f64,
    sys: f64,
}

fn child_times() -> (f64, f64) {
    let mut ru: libc::rusage = unsafe { std::mem::zeroed() };
    unsafe { libc::getrusage(libc::RUSAGE_CHILDREN, &mut ru) };
    let t = |tv: libc::timeval| tv.tv_sec as f64 + tv.tv_usec as f64 / 1e6;
    (t(ru.ru_utime), t(ru.ru_stime))
}

impl TimeReport {
    fn start() -> Self {
        let (user, sys) = child_times();
        TimeReport { start: std::time::Instant::now(), user, sys }
    }

    fn finish(self) {
        let real = self.start.elapsed().as_secs_f64();
        let (user, sys) = child_times();
        let fmt = |s: f64| format!("{}m{:.3}s", (s / 60.0) as u64, s % 60.0);
        eprintln!("\nreal\t{}\nuser\t{}\nsys\t{}", fmt(real), fmt(user - self.user), fmt(sys - self.sys));
    }
}
