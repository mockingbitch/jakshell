use anyhow::{anyhow, Result};
use std::cell::RefCell;
use std::fs::OpenOptions;
use std::io::{Read, Seek, SeekFrom};
use std::os::fd::AsRawFd;
use std::process::{Command, Stdio};
use std::rc::Rc;

use crate::builtins;
use crate::expand::expand_word;
use crate::lexer::WordPart;
use crate::parser::{Pipeline, Program, Redirect, RedirectKind, SeqOp, SimpleCommand};
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
        last_code = run_pipeline(shell, pipeline)?;
    }
    shell.borrow_mut().reap_jobs();
    Ok(last_code)
}

fn run_pipeline(shell: &Rc<RefCell<Shell>>, pipeline: &Pipeline) -> Result<i32> {
    // Resolve aliases on the head command (one-level, like bash default).
    let mut commands: Vec<SimpleCommand> = pipeline.commands.clone();
    // Command substitution `$(...)` / `` `...` `` — chạy lệnh con & thay bằng
    // stdout TRƯỚC khi giải alias và expand biến/glob (giống thứ tự của bash).
    for cmd in &mut commands {
        if let Err(e) = expand_command_subs(shell, cmd) {
            eprintln!("jaksh: {}", e);
            return Ok(1);
        }
    }
    expand_aliases(shell, &mut commands);

    if commands.len() == 1 {
        // Single command: may be a builtin (must run in our process)
        let cmd = &commands[0];
        let argv = build_argv(shell, cmd);
        // Phép gán đứng đầu (`VAR=val cmd`) — bash-style. GDM/Xsession khởi
        // động session qua login shell bằng dạng này nên bắt buộc phải hỗ trợ.
        let (assigns, argv) = split_leading_assignments(argv);
        if argv.is_empty() {
            // Chỉ có phép gán (`VAR=val`) → set biến shell, không chạy gì.
            for (k, v) in &assigns {
                shell.borrow_mut().set_var(k, v);
            }
            if cmd.redirects.is_empty() {
                return Ok(0);
            }
        }

        // Cờ --jak (do JakShell xử lý, không truyền xuống lệnh thật)
        if !pipeline.background {
            if let Some(head) = argv.first() {
                if argv.iter().any(|s| s == "--jak") && crate::pretty::supports(head) {
                    let cleaned: Vec<String> = argv.iter().filter(|s| *s != "--jak").cloned().collect();
                    return crate::pretty::run(shell, &cleaned, &cmd.redirects);
                }
                // curl tương tác: tự động capture + format response (separator
                // + pretty JSON). CHỈ khi stdout là TTY và không redirect —
                // pipe (`curl | jq`) hay `> file` vẫn nhận raw output như cũ.
                if head == "curl" && cmd.redirects.is_empty() && stdout_is_tty() {
                    return crate::pretty::run(shell, &argv, &cmd.redirects);
                }
            }
        }

        if let Some(head) = argv.first() {
            if builtins::is_builtin(head) && !pipeline.background {
                // `VAR=val builtin`: set tạm trong lúc chạy rồi khôi phục
                // (với `exec` thì không bao giờ quay lại — env đi theo process mới).
                let saved: Vec<(String, Option<String>)> = assigns
                    .iter()
                    .map(|(k, _)| (k.clone(), shell.borrow().get_var(k)))
                    .collect();
                for (k, v) in &assigns {
                    shell.borrow_mut().set_var(k, v);
                }
                let result = builtins::run(shell, &argv, &cmd.redirects);
                for (k, old) in saved {
                    match old {
                        Some(v) => shell.borrow_mut().set_var(&k, &v),
                        None => shell.borrow_mut().unset_var(&k),
                    }
                }
                return result;
            }
        }
        return spawn_external(shell, cmd, pipeline.background);
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

fn build_argv(shell: &Rc<RefCell<Shell>>, cmd: &SimpleCommand) -> Vec<String> {
    let mut argv = Vec::new();
    for (i, w) in cmd.words.iter().enumerate() {
        // Only glob expand for non-head words (head is the command itself).
        let do_glob = i > 0;
        let pieces = expand_word(&shell.borrow(), w, do_glob);
        for p in pieces {
            argv.push(p);
        }
    }
    argv
}

fn apply_redirects(builder: &mut Command, shell: &Rc<RefCell<Shell>>, redirects: &[Redirect]) -> Result<()> {
    for r in redirects {
        let target_pieces = expand_word(&shell.borrow(), &r.target, true);
        let target = target_pieces.into_iter().next().ok_or_else(|| anyhow!("đích redirect rỗng"))?;
        match r.kind {
            RedirectKind::In => {
                let f = std::fs::File::open(&target).map_err(|e| anyhow!("không mở được {}: {}", target, e))?;
                builder.stdin(Stdio::from(f));
            }
            RedirectKind::Out => {
                let f = OpenOptions::new().write(true).create(true).truncate(true).open(&target)
                    .map_err(|e| anyhow!("không tạo được {}: {}", target, e))?;
                builder.stdout(Stdio::from(f));
            }
            RedirectKind::Append => {
                let f = OpenOptions::new().write(true).create(true).append(true).open(&target)
                    .map_err(|e| anyhow!("không mở được {}: {}", target, e))?;
                builder.stdout(Stdio::from(f));
            }
            RedirectKind::ErrOut => {
                let f = OpenOptions::new().write(true).create(true).truncate(true).open(&target)
                    .map_err(|e| anyhow!("không tạo được {}: {}", target, e))?;
                builder.stderr(Stdio::from(f));
            }
            RedirectKind::ErrAppend => {
                let f = OpenOptions::new().write(true).create(true).append(true).open(&target)
                    .map_err(|e| anyhow!("không mở được {}: {}", target, e))?;
                builder.stderr(Stdio::from(f));
            }
            RedirectKind::AllOut => {
                let f = OpenOptions::new().write(true).create(true).truncate(true).open(&target)
                    .map_err(|e| anyhow!("không tạo được {}: {}", target, e))?;
                let f2 = f.try_clone()?;
                builder.stdout(Stdio::from(f));
                builder.stderr(Stdio::from(f2));
            }
        }
    }
    Ok(())
}

fn spawn_external(shell: &Rc<RefCell<Shell>>, cmd: &SimpleCommand, background: bool) -> Result<i32> {
    let argv = build_argv(shell, cmd);
    let (assigns, argv) = split_leading_assignments(argv);
    if argv.is_empty() {
        return Ok(0);
    }
    let prog = &argv[0];
    let args = &argv[1..];

    // Special "jak" subcommand router (runs in our process)
    if prog == "jak" {
        return crate::jak::run(shell, args);
    }

    let mut builder = Command::new(prog);
    builder.args(args);
    builder.envs(assigns);
    builder.current_dir(&shell.borrow().cwd);

    apply_redirects(&mut builder, shell, &cmd.redirects)?;

    match builder.spawn() {
        Ok(mut child) => {
            if background {
                let pid = child.id() as i32;
                let id = shell.borrow_mut().add_job(pid, argv.join(" "));
                println!("\x1b[2m[{}] {}\x1b[0m", id, pid);
                Ok(0)
            } else {
                let status = child.wait()?;
                Ok(status.code().unwrap_or_else(|| {
                    #[cfg(unix)]
                    {
                        use std::os::unix::process::ExitStatusExt;
                        128 + status.signal().unwrap_or(0)
                    }
                    #[cfg(not(unix))]
                    {
                        1
                    }
                }))
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

    for (i, cmd) in commands.iter().enumerate() {
        let argv = build_argv(shell, cmd);
        let (assigns, argv) = split_leading_assignments(argv);
        if argv.is_empty() {
            return Err(anyhow!("lệnh rỗng trong pipeline"));
        }
        let prog = &argv[0];
        let args = &argv[1..];

        // Builtins inside a pipeline: run them as external echo-like helper? For simplicity, we
        // fork via /usr/bin/env to re-exec ourselves with the builtin... but simpler: only handle
        // simple builtins like `echo` via the system command. Here, refuse builtins in pipelines
        // (rare to need cd|grep). Most useful "builtins" in pipelines are echo/printf which exist
        // as external commands anyway on macOS/Linux.
        let mut builder = Command::new(prog);
        builder.args(args);
        builder.envs(assigns);
        builder.current_dir(&shell.borrow().cwd);

        if let Some(stdin) = prev_stdout.take() {
            builder.stdin(stdin);
        }
        if i < n - 1 {
            builder.stdout(Stdio::piped());
        }
        apply_redirects(&mut builder, shell, &cmd.redirects)?;

        match builder.spawn() {
            Ok(mut child) => {
                if i < n - 1 {
                    if let Some(s) = child.stdout.take() {
                        prev_stdout = Some(Stdio::from(s));
                    }
                }
                children.push((child, argv.join(" ")));
            }
            Err(e) => {
                if e.kind() == std::io::ErrorKind::NotFound {
                    eprintln!("jaksh: {}: {}", crate::i18n::t("common.not_found"), prog);
                } else {
                    eprintln!("jaksh: không chạy được {}: {}", prog, e);
                }
                return Ok(127);
            }
        }
    }

    if background {
        for (child, cmd) in children {
            let pid = child.id() as i32;
            let id = shell.borrow_mut().add_job(pid, cmd);
            println!("\x1b[2m[{}] {}\x1b[0m", id, pid);
            // intentionally drop child to detach (will be reaped via job system)
            std::mem::forget(child);
        }
        return Ok(0);
    }

    let mut last_code = 0;
    for (mut child, _cmd) in children {
        let status = child.wait()?;
        last_code = status.code().unwrap_or(1);
    }
    Ok(last_code)
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

    // Hoán fd 1 → file trong một scope; drop sẽ khôi phục fd gốc.
    {
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

/// `NAME=value` với NAME là identifier hợp lệ ([A-Za-z_][A-Za-z0-9_]*).
fn parse_assignment(w: &str) -> Option<(String, String)> {
    let eq = w.find('=')?;
    if eq == 0 {
        return None;
    }
    let name = &w[..eq];
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
    }
}
