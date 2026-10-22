use anyhow::{anyhow, Result};
use std::cell::RefCell;
use std::fs::OpenOptions;
use std::io::{Read, Seek, SeekFrom};
use std::os::fd::AsRawFd;
use std::process::{Command, Stdio};
use std::rc::Rc;

use crate::builtins;
use crate::expand::{expand_word, expand_word_checked, UnboundVar};
use crate::lexer::WordPart;
use crate::parser::{Pipeline, Program, Redirect, SeqOp, SimpleCommand};
use crate::shell::Shell;

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
        // Lỗi của 1 lệnh (file redirect không mở được, biến chưa gán…) chỉ làm
        // hỏng lệnh đó — script chạy tiếp như bash.
        let code = match run_pipeline(shell, pipeline) {
            Ok(code) => code,
            Err(e) => {
                report_error(&e);
                if e.downcast_ref::<UnboundVar>().is_some() {
                    // `set -u`: script dừng hẳn; phiên tương tác bỏ phần còn lại của dòng.
                    if !crate::jobctl::interactive() && !crate::jobctl::in_capture() {
                        exit_shell(shell, 127);
                    }
                    last_code = 127;
                    shell.borrow_mut().set_last_status(last_code);
                    break;
                }
                1
            }
        };
        if pipeline.commands.len() == 1 && !pipeline.background {
            shell.borrow_mut().pipestatus = vec![code];
        }
        last_code = if pipeline.negate { (code == 0) as i32 } else { code };
        shell.borrow_mut().set_last_status(last_code);
        // Ctrl-C giữa chừng → bỏ phần còn lại của dòng (`sleep 9; rm x` mà
        // ^C thì KHÔNG chạy `rm x`), như bash.
        if crate::jobctl::take_interrupted() {
            last_code = 130;
            break;
        }
        // `set -e`: lệnh lỗi làm thoát shell — trừ vế trái của `&&`/`||` và `!`.
        let left_of_list = matches!(prog.items.get(i + 1), Some((_, SeqOp::AndIf | SeqOp::OrIf)));
        if last_code != 0 && shell.borrow().opts.errexit && !pipeline.negate && !left_of_list {
            if crate::jobctl::in_capture() {
                break;
            }
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

/// Thoát shell (dùng cho `set -e` / `set -u`) — dọn job control như `exit`.
fn exit_shell(shell: &Rc<RefCell<Shell>>, code: i32) -> ! {
    if crate::jobctl::interactive() {
        crate::jobctl::shutdown(shell);
    }
    flush_std();
    std::process::exit(code);
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
    // Resolve aliases on the head command (one-level, like bash default).
    let mut commands: Vec<SimpleCommand> = pipeline.commands.clone();
    // Command substitution `$(...)` / `` `...` `` — chạy lệnh con & thay bằng
    // stdout TRƯỚC khi giải alias và expand biến/glob (giống thứ tự của bash).
    for cmd in &mut commands {
        expand_command_subs(shell, cmd)?;
    }
    expand_aliases(shell, &mut commands);

    if commands.len() == 1 {
        // Single command: may be a builtin (must run in our process)
        let cmd = &commands[0];
        let argv = build_argv(shell, cmd)?;
        // Phép gán đứng đầu (`VAR=val cmd`) — bash-style. GDM/Xsession khởi
        // động session qua login shell bằng dạng này nên bắt buộc phải hỗ trợ.
        let (assigns, mut argv) = split_leading_assignments(argv);
        let assigns = resolve_appends(shell, assigns);
        if argv.is_empty() {
            // Chỉ có phép gán (`VAR=val`) → set biến shell, không chạy gì.
            xtrace(shell, &assigns, &argv);
            for (k, v) in &assigns {
                if let Err(e) = shell.borrow_mut().set_var(k, v) {
                    // Gán vào biến readonly: script dừng hẳn (POSIX), REPL chỉ báo lỗi.
                    if !crate::jobctl::interactive() && !crate::jobctl::in_capture() {
                        report_error(&e);
                        exit_shell(shell, 1);
                    }
                    return Err(e);
                }
            }
            // `> file` không có lệnh: vẫn tạo / cắt rỗng file như bash.
            if !cmd.redirects.is_empty() {
                drop(crate::redirect::plan(shell, &cmd.redirects)?);
            }
            return Ok(0);
        }
        xtrace(shell, &assigns, &argv);

        // `command x …` bỏ qua alias (chỉ từ đầu — `command` — được giải alias);
        // `builtin x …` bắt buộc chạy builtin. `command -v/-V` là builtin riêng.
        let mut only_builtin = false;
        match argv[0].as_str() {
            "command" if argv.len() > 1 && !argv[1].starts_with('-') => {
                argv.remove(0);
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

        // Cờ --jak (do JakShell xử lý, không truyền xuống lệnh thật)
        if !pipeline.background && !only_builtin {
            if let Some(head) = argv.first() {
                if argv.iter().any(|s| s == "--jak") && crate::pretty::supports(head) {
                    let cleaned: Vec<String> = argv.iter().filter(|s| *s != "--jak").cloned().collect();
                    return crate::pretty::run(shell, &cleaned, &cmd.redirects);
                }
                // curl tương tác: tự động capture + format response (separator
                // + pretty JSON). CHỈ khi stdout là TTY và không redirect —
                // pipe (`curl | jq`) hay `> file` vẫn nhận raw output như cũ.
                // Tải file / stream / -v / -w tự chọn → chạy nguyên bản.
                if head == "curl"
                    && cmd.redirects.is_empty()
                    && shell.borrow().ui.pretty_curl
                    && crate::pretty::curl_wants_pretty(&argv[1..])
                    && stdout_is_tty()
                {
                    return crate::pretty::run(shell, &argv, &cmd.redirects);
                }
            }
        }

        if builtins::is_builtin(&argv[0]) && !pipeline.background {
            // `VAR=val builtin`: set tạm trong lúc chạy rồi khôi phục
            // (với `exec` thì không bao giờ quay lại — env đi theo process mới).
            let saved: Vec<(String, Option<String>)> = assigns
                .iter()
                .map(|(k, _)| (k.clone(), shell.borrow().get_var(k)))
                .collect();
            for (k, v) in &assigns {
                shell.borrow_mut().set_var(k, v)?;
            }
            let result = builtins::run(shell, &argv, &cmd.redirects);
            for (k, old) in saved {
                let _ = match old {
                    Some(v) => shell.borrow_mut().set_var(&k, &v),
                    None => shell.borrow_mut().unset_var(&k),
                };
            }
            return result;
        }
        return spawn_external(shell, &argv, &assigns, &cmd.redirects, pipeline.background);
    }

    // Multi-command pipeline. Connect with pipes.
    spawn_pipeline(shell, &commands, pipeline.background)
}

fn expand_aliases(shell: &Rc<RefCell<Shell>>, commands: &mut [SimpleCommand]) {
    // Only expand the first command head, repeatedly, to avoid loops.
    if let Some(first) = commands.first_mut() {
        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
        loop {
            let head = match first.words.first() {
                Some(parts) => {
                    let strs = expand_word(&shell.borrow(), parts, false);
                    strs.into_iter().next().unwrap_or_default()
                }
                None => return,
            };
            if seen.contains(&head) {
                break;
            }
            let alias_val = shell.borrow().aliases.get(&head).cloned();
            if let Some(val) = alias_val {
                seen.insert(head);
                // Tokenize+parse the alias value to replace the head and prepend its args
                if let Ok(tokens) = crate::lexer::tokenize(&val) {
                    // Collect Word tokens only (alias bodies generally are commands).
                    let mut new_words: Vec<Vec<crate::lexer::WordPart>> = Vec::new();
                    for t in tokens {
                        if let crate::lexer::Token::Word(parts) = t {
                            new_words.push(parts);
                        }
                    }
                    if !new_words.is_empty() {
                        let rest: Vec<Vec<crate::lexer::WordPart>> = first.words.iter().skip(1).cloned().collect();
                        new_words.extend(rest);
                        first.words = new_words;
                        continue;
                    }
                }
                break;
            } else {
                break;
            }
        }
    }
}

fn build_argv(shell: &Rc<RefCell<Shell>>, cmd: &SimpleCommand) -> Result<Vec<String>> {
    let mut argv = Vec::new();
    for (i, w) in cmd.words.iter().enumerate() {
        // Only glob expand for non-head words (head is the command itself).
        let do_glob = i > 0;
        argv.extend(expand_word_checked(&shell.borrow(), w, do_glob)?);
    }
    Ok(argv)
}

fn spawn_external(
    shell: &Rc<RefCell<Shell>>,
    argv: &[String],
    assigns: &[(String, String)],
    redirects: &[Redirect],
    background: bool,
) -> Result<i32> {
    let prog = &argv[0];
    let args = &argv[1..];

    // Special "jak" subcommand router (runs in our process)
    if prog == "jak" {
        let _guard = crate::redirect::apply_in_process(crate::redirect::plan(shell, redirects)?)?;
        return crate::jak::run(shell, args);
    }

    let mut builder = Command::new(prog);
    builder.args(args);
    builder.envs(assigns.iter().map(|(k, v)| (k, v)));
    builder.current_dir(&shell.borrow().cwd);

    crate::redirect::apply_to_command(&mut builder, crate::redirect::plan(shell, redirects)?);
    crate::jobctl::prepare(&mut builder, 0, !background);

    match builder.spawn() {
        Ok(child) => {
            let pid = child.id() as i32;
            let pgid = crate::jobctl::after_spawn(pid, 0, !background);
            let cmdline = argv.join(" ");
            if background {
                shell.borrow_mut().last_bg_pid = Some(pid);
                let id = shell.borrow_mut().add_job(pgid, vec![pid], cmdline);
                if crate::jobctl::interactive() {
                    println!("\x1b[2m[{}] {}\x1b[0m", id, pid);
                }
                Ok(0)
            } else {
                Ok(crate::jobctl::wait_foreground(shell, pgid, &[pid], &cmdline, None))
            }
        }
        Err(e) => {
            if e.kind() == std::io::ErrorKind::NotFound {
                eprintln!("jaksh: {}: {}", crate::i18n::t("common.not_found"), prog);
                Ok(127)
            } else {
                eprintln!("jaksh: không chạy được {}: {}", prog, e);
                Ok(126)
            }
        }
    }
}

fn spawn_pipeline(shell: &Rc<RefCell<Shell>>, commands: &[SimpleCommand], background: bool) -> Result<i32> {
    let n = commands.len();
    let mut children = Vec::with_capacity(n);
    let mut prev_stdout: Option<Stdio> = None;
    // Output đã bắt của builtin đứng trước — ghi vào stdin lệnh kế tiếp.
    let mut pending_input: Option<Vec<u8>> = None;
    // Exit code từng vị trí trong pipeline: builtin biết ngay (Some), lệnh ngoài
    // điền sau khi chờ (None) — cho `$PIPESTATUS` và `set -o pipefail`.
    let mut slots: Vec<Option<i32>> = Vec::with_capacity(n);
    let mut pgid = 0;
    // Lỗi giữa chừng: vẫn phải chờ các lệnh đã spawn (và lấy lại terminal từ
    // chúng) trước khi trả lỗi — không return sớm.
    let mut failure: Option<Result<i32>> = None;

    for (i, cmd) in commands.iter().enumerate() {
        let argv = match build_argv(shell, cmd) {
            Ok(a) => a,
            Err(e) => {
                failure = Some(Err(e));
                break;
            }
        };
        let (assigns, mut argv) = split_leading_assignments(argv);
        let assigns = resolve_appends(shell, assigns);
        if argv.len() > 1 && argv[0] == "command" && !argv[1].starts_with('-') {
            argv.remove(0);
        }
        if argv.is_empty() {
            failure = Some(Err(anyhow!("lệnh rỗng trong pipeline")));
            break;
        }
        xtrace(shell, &assigns, &argv);
        let prog = &argv[0];
        let args = &argv[1..];

        // Builtin chỉ-in (`history`, `alias`, …) chạy ngay trong shell: output
        // được bắt lại rồi nạp làm stdin cho lệnh sau. Builtin khác rơi về lệnh
        // ngoài cùng tên (vd /bin/echo) như trước.
        if builtins::is_pipe_safe(prog) {
            prev_stdout = None;
            if i < n - 1 {
                match builtins::run_captured(shell, &argv, &cmd.redirects) {
                    Ok((code, out)) => {
                        pending_input = Some(out);
                        slots.push(Some(code));
                    }
                    Err(e) => {
                        failure = Some(Err(e));
                        break;
                    }
                }
            } else {
                match builtins::run(shell, &argv, &cmd.redirects) {
                    Ok(code) => slots.push(Some(code)),
                    Err(e) => {
                        failure = Some(Err(e));
                        break;
                    }
                }
            }
            continue;
        }

        let mut builder = Command::new(prog);
        builder.args(args);
        builder.envs(assigns);
        builder.current_dir(&shell.borrow().cwd);

        if let Some(stdin) = prev_stdout.take() {
            builder.stdin(stdin);
        } else if pending_input.is_some() {
            builder.stdin(Stdio::piped());
        }
        if i < n - 1 {
            builder.stdout(Stdio::piped());
        }
        match crate::redirect::plan(shell, &cmd.redirects) {
            Ok(plan) => crate::redirect::apply_to_command(&mut builder, plan),
            Err(e) => {
                failure = Some(Err(e));
                break;
            }
        }
        // Cả pipeline chung 1 process group (trưởng = lệnh đầu tiên được spawn).
        crate::jobctl::prepare(&mut builder, pgid, !background);

        match builder.spawn() {
            Ok(mut child) => {
                pgid = crate::jobctl::after_spawn(child.id() as i32, pgid, !background);
                if let (Some(data), Some(mut stdin)) = (pending_input.take(), child.stdin.take()) {
                    std::thread::spawn(move || {
                        let _ = std::io::Write::write_all(&mut stdin, &data);
                    });
                }
                if i < n - 1 {
                    if let Some(s) = child.stdout.take() {
                        prev_stdout = Some(Stdio::from(s));
                    }
                }
                children.push((child, argv.join(" ")));
                slots.push(None);
            }
            Err(e) => {
                if e.kind() == std::io::ErrorKind::NotFound {
                    eprintln!("jaksh: {}: {}", crate::i18n::t("common.not_found"), prog);
                } else {
                    eprintln!("jaksh: không chạy được {}: {}", prog, e);
                }
                failure = Some(Ok(127));
                break;
            }
        }
    }
    // Đóng đầu đọc còn treo để lệnh trước nhận EOF/SIGPIPE thay vì chờ mãi.
    drop(prev_stdout);

    let pids: Vec<i32> = children.iter().map(|(c, _)| c.id() as i32).collect();
    let cmdline = children.iter().map(|(_, c)| c.as_str()).collect::<Vec<_>>().join(" | ");
    // Child của std không tự wait khi drop — việc thu dọn do jobctl/reap_jobs.
    drop(children);
    if pids.is_empty() {
        if let Some(f) = failure {
            return f;
        }
        return Ok(finish_pipestatus(shell, slots));
    }
    if let Some(f) = failure {
        if background {
            shell.borrow_mut().add_job(pgid, pids, cmdline);
        } else {
            crate::jobctl::wait_foreground(shell, pgid, &pids, &cmdline, None);
        }
        return f;
    }
    if background {
        shell.borrow_mut().last_bg_pid = pids.last().copied();
        let id = shell.borrow_mut().add_job(pgid, pids.clone(), cmdline);
        if crate::jobctl::interactive() {
            println!("\x1b[2m[{}] {}\x1b[0m", id, pids.last().copied().unwrap_or(0));
        }
        return Ok(0);
    }
    let code = crate::jobctl::wait_foreground(shell, pgid, &pids, &cmdline, None);
    if code == 128 + libc::SIGTSTP && shell.borrow().jobs.iter().any(|j| j.pgid == pgid) {
        return Ok(code); // bị Ctrl-Z — pipeline chưa xong
    }
    // Ghép exit code lệnh ngoài (theo thứ tự) vào các ô còn trống.
    let mut external = std::mem::take(&mut shell.borrow_mut().pipestatus).into_iter();
    let slots = slots.into_iter().map(|s| s.or_else(|| external.next())).collect();
    Ok(finish_pipestatus(shell, slots))
}

/// Lưu `$PIPESTATUS` và tính exit code của pipeline (lệnh cuối, hoặc lệnh lỗi
/// cuối cùng khi `set -o pipefail`).
fn finish_pipestatus(shell: &Rc<RefCell<Shell>>, slots: Vec<Option<i32>>) -> i32 {
    let codes: Vec<i32> = slots.into_iter().map(|c| c.unwrap_or(0)).collect();
    let code = if shell.borrow().opts.pipefail {
        codes.iter().rev().find(|&&c| c != 0).copied().unwrap_or(0)
    } else {
        codes.last().copied().unwrap_or(0)
    };
    shell.borrow_mut().pipestatus = codes;
    code
}

/// stdout có phải terminal không — quyết định có bật auto-pretty (curl, …).
fn stdout_is_tty() -> bool {
    unsafe { libc::isatty(libc::STDOUT_FILENO) == 1 }
}

/// Thay mọi `WordPart::CmdSub` trong 1 lệnh (cả words lẫn đích redirect) bằng
/// kết quả chạy lệnh con. Gọi ở đầu `run_pipeline`, khi CHƯA giữ borrow nào của
/// shell — nên chạy lệnh con (mượn shell mutably) an toàn.
fn expand_command_subs(shell: &Rc<RefCell<Shell>>, cmd: &mut SimpleCommand) -> Result<()> {
    for w in &mut cmd.words {
        expand_parts_cmdsub(shell, w)?;
    }
    for r in &mut cmd.redirects {
        expand_parts_cmdsub(shell, &mut r.target)?;
    }
    Ok(())
}

fn expand_parts_cmdsub(shell: &Rc<RefCell<Shell>>, parts: &mut Vec<WordPart>) -> Result<()> {
    if !parts.iter().any(|p| matches!(p, WordPart::CmdSub(_))) {
        return Ok(());
    }
    let mut out = Vec::with_capacity(parts.len());
    for p in parts.drain(..) {
        match p {
            WordPart::CmdSub(src) => {
                let captured = capture_command_output(shell, &src)?;
                // Quoted → không glob & không tách từ; đúng nhu cầu phổ biến
                // nhất (`eval "$(brew shellenv)"`, `x="$(cmd)"`, `$(git ...)`),
                // và khớp cách shell này vốn không tách từ khi expand biến.
                out.push(WordPart::Quoted(captured));
            }
            other => out.push(other),
        }
    }
    *parts = out;
    Ok(())
}

/// Chạy `src` như một chương trình con và thu lại stdout của nó — phần lõi của
/// command substitution. Chạy IN-PROCESS trong shell hiện tại (không fork một
/// subshell thật), nên side effect như `cd` / gán biến bên trong `$(...)` sẽ
/// ảnh hưởng shell cha; đủ dùng cho các lệnh chỉ đọc/in dữ liệu (brew shellenv,
/// git rev-parse, date, pwd, ...). Thu bằng cách hoán fd 1 sang FILE TẠM (không
/// dùng pipe) để tránh kẹt khi output lớn hơn buffer pipe.
fn capture_command_output(shell: &Rc<RefCell<Shell>>, src: &str) -> Result<String> {
    let tokens = crate::lexer::tokenize(src)?;
    let prog = crate::parser::parse(&tokens)?;

    let mut path = std::env::temp_dir();
    path.push(format!(
        "jaksh-cmdsub-{}-{}",
        std::process::id(),
        next_capture_id()
    ));
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(true)
        .open(&path)
        .map_err(|e| anyhow!("$(...): không tạo được file tạm: {}", e))?;

    // Hoán fd 1 → file trong một scope; drop sẽ khôi phục fd gốc. `$(...)`
    // không có job control (như bash): lệnh con ở chung group với shell.
    {
        let _nojobs = crate::jobctl::Suspend::new();
        let _swap = FdSwap::new(libc::STDOUT_FILENO, file.as_raw_fd())?;
        flush_std();
        let _ = execute(shell, &prog);
        flush_std();
    }

    let mut file = file;
    file.seek(SeekFrom::Start(0)).ok();
    let mut s = String::new();
    file.read_to_string(&mut s).ok();
    let _ = std::fs::remove_file(&path);

    // Bỏ mọi newline ở cuối (đúng như POSIX command substitution).
    let end = s.trim_end_matches('\n').len();
    s.truncate(end);
    Ok(s)
}

fn flush_std() {
    use std::io::Write;
    let _ = std::io::stdout().flush();
    let _ = std::io::stderr().flush();
}

/// Bộ đếm để đặt tên file tạm duy nhất trong 1 tiến trình (tránh đụng khi có
/// nhiều `$(...)` lồng/nối tiếp).
fn next_capture_id() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    N.fetch_add(1, Ordering::Relaxed)
}

/// Hoán đổi tạm một file descriptor (thường là stdout=1) sang `new_fd`; khi drop
/// sẽ khôi phục nguyên trạng. Dùng cho command substitution.
struct FdSwap {
    target: i32,
    saved: i32,
}

impl FdSwap {
    fn new(target: i32, new_fd: i32) -> Result<Self> {
        unsafe {
            let saved = libc::dup(target);
            if saved < 0 {
                return Err(anyhow!("dup({}) thất bại", target));
            }
            if libc::dup2(new_fd, target) < 0 {
                libc::close(saved);
                return Err(anyhow!("dup2 thất bại"));
            }
            Ok(FdSwap { target, saved })
        }
    }
}

impl Drop for FdSwap {
    fn drop(&mut self) {
        flush_std();
        unsafe {
            libc::dup2(self.saved, self.target);
            libc::close(self.saved);
        }
    }
}

/// Tách các phép gán `VAR=val` đứng đầu lệnh (bash-style):
/// `A=1 B=2 cmd args` → ([("A","1"),("B","2")], ["cmd","args"]).
/// Dừng peel ở word đầu tiên không phải dạng gán hợp lệ.
fn split_leading_assignments(argv: Vec<String>) -> (Vec<(String, String)>, Vec<String>) {
    let mut assigns = Vec::new();
    let mut rest = Vec::new();
    let mut done = false;
    for w in argv {
        if !done {
            if let Some(kv) = parse_assignment(&w) {
                assigns.push(kv);
                continue;
            }
            done = true;
        }
        rest.push(w);
    }
    (assigns, rest)
}

/// `NAME+=value` → giá trị đầy đủ (nối vào giá trị hiện có).
fn resolve_appends(shell: &Rc<RefCell<Shell>>, assigns: Vec<(String, String)>) -> Vec<(String, String)> {
    assigns
        .into_iter()
        .map(|(k, v)| match k.strip_suffix('+') {
            Some(base) => {
                let cur = shell.borrow().get_var(base).unwrap_or_default();
                (base.to_string(), cur + &v)
            }
            None => (k, v),
        })
        .collect()
}

/// `NAME=value` với NAME là identifier hợp lệ ([A-Za-z_][A-Za-z0-9_]*).
/// `NAME+=value` trả về tên kèm `+` — `resolve_appends` xử lý tiếp.
fn parse_assignment(w: &str) -> Option<(String, String)> {
    let eq = w.find('=')?;
    if eq == 0 {
        return None;
    }
    let name = &w[..eq];
    let name = match name.strip_suffix('+') {
        Some(base) if !base.is_empty() => {
            let (k, v) = parse_assignment(&format!("{}={}", base, &w[eq + 1..]))?;
            return Some((format!("{}+", k), v));
        }
        _ => name,
    };
    let mut chars = name.chars();
    let first = chars.next()?;
    if !(first.is_ascii_alphabetic() || first == '_') {
        return None;
    }
    if !chars.all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return None;
    }
    Some((name.to_string(), w[eq + 1..].to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn assignments_are_peeled() {
        let (a, rest) = split_leading_assignments(vec![
            "GNOME_SHELL_SESSION_MODE=ubuntu".into(),
            "exec".into(),
            "gnome-session".into(),
        ]);
        assert_eq!(a, vec![("GNOME_SHELL_SESSION_MODE".to_string(), "ubuntu".to_string())]);
        assert_eq!(rest, vec!["exec", "gnome-session"]);
    }

    #[test]
    fn non_assignment_stops_peeling() {
        // `ls A=1` — A=1 là argument, không phải phép gán
        let (a, rest) = split_leading_assignments(vec!["ls".into(), "A=1".into()]);
        assert!(a.is_empty());
        assert_eq!(rest, vec!["ls", "A=1"]);
    }

    #[test]
    fn invalid_names_are_not_assignments() {
        assert!(parse_assignment("=foo").is_none());
        assert!(parse_assignment("1AB=x").is_none());
        assert!(parse_assignment("A-B=x").is_none());
        assert_eq!(parse_assignment("_OK=1"), Some(("_OK".into(), "1".into())));
        assert_eq!(parse_assignment("x+=b"), Some(("x+".into(), "b".into())));
        assert!(parse_assignment("+=b").is_none());
    }
}
