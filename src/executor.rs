use anyhow::{anyhow, Result};
use std::cell::RefCell;
use std::fs::OpenOptions;
use std::process::{Command, Stdio};
use std::rc::Rc;

use crate::builtins;
use crate::expand::expand_word;
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
    expand_aliases(shell, &mut commands);

    if commands.len() == 1 {
        // Single command: may be a builtin (must run in our process)
        let cmd = &commands[0];
        let argv = build_argv(shell, cmd);
        if argv.is_empty() && cmd.redirects.is_empty() {
            return Ok(0);
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
                return builtins::run(shell, &argv, &cmd.redirects);
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
