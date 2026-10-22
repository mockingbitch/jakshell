use anyhow::{anyhow, Result};
use std::cell::RefCell;
use std::rc::Rc;

use crate::parser::Redirect;
use crate::shell::Shell;

pub const BUILTINS: &[&str] = &[
    "cd", "pwd", "exit", "export", "unset", "alias", "unalias", "set",
    "echo", "source", ".", "history", "jobs", "fg", "bg", "kill",
    "help", "?", "which", "true", "false", "explain", "bookmark", "exec",
    "eval", "wait", "disown", "readonly", "declare", "typeset", "shift",
    "read", "command", "type", "hash", "builtin",
];

pub fn is_builtin(name: &str) -> bool {
    BUILTINS.contains(&name)
}

/// Builtin chỉ in ra stdout, không đổi trạng thái shell — chạy được ngay trong
/// pipeline (`history | grep git`). Builtin còn lại (`cd`, `exit`, `export`…)
/// trong bash chạy ở subshell nên vô tác dụng; ở đây vẫn rơi về lệnh ngoài.
pub fn is_pipe_safe(name: &str) -> bool {
    matches!(
        name,
        "history" | "alias" | "jobs" | "which" | "help" | "?" | "explain" | "echo" | "pwd" | "true" | "false"
            | "type" | "command"
    )
}

/// Chạy builtin với stdout chuyển vào pipe, trả về (exit code, output) — để
/// nạp làm stdin cho lệnh kế tiếp trong pipeline. Một thread đọc song song nên
/// output lớn hơn buffer pipe cũng không treo.
pub fn run_captured(shell: &Rc<RefCell<Shell>>, argv: &[String], redirects: &[Redirect]) -> Result<(i32, Vec<u8>)> {
    let (mut reader, writer) = std::io::pipe()?;
    let drain = std::thread::spawn(move || {
        let mut buf = Vec::new();
        std::io::Read::read_to_end(&mut reader, &mut buf).ok();
        buf
    });
    let code = {
        let mut plan = crate::redirect::Plan::default();
        plan.push(1, crate::redirect::FdAction::Open(writer.into()));
        let guard = crate::redirect::apply_in_process(plan)?;
        let code = run(shell, argv, redirects);
        drop(guard);
        code
    };
    let out = drain.join().unwrap_or_default();
    Ok((code?, out))
}

pub fn run(shell: &Rc<RefCell<Shell>>, argv: &[String], redirects: &[Redirect]) -> Result<i32> {
    let cmd = argv[0].as_str();
    let args: Vec<&str> = argv.iter().skip(1).map(|s| s.as_str()).collect();
    let guard = crate::redirect::apply_in_process(crate::redirect::plan(shell, redirects)?)?;
    // `exec 3>file` / `exec >log` không kèm lệnh: redirect áp cho chính shell, giữ luôn.
    if cmd == "exec" && args.is_empty() {
        guard.persist();
        return Ok(0);
    }
    flush_std();
    let result = match cmd {
        "cd" => cd(shell, &args),
        "pwd" => pwd(shell),
        "exit" => exit(shell, &args),
        "export" => export(shell, &args),
        "unset" => unset(shell, &args),
        "readonly" => readonly(shell, &args),
        "declare" | "typeset" => declare(shell, &args),
        "shift" => shift(shell, &args),
        "alias" => alias(shell, &args),
        "unalias" => unalias(shell, &args),
        "echo" => echo(&args),
        "read" => read(shell, &args),
        "command" => command(shell, &args),
        "type" => type_(shell, &args),
        // Lệnh luôn được tìm lại trong $PATH mỗi lần chạy → không có bảng hash.
        "hash" => Ok(0),
        "builtin" => Ok(0),
        "source" | "." => source(shell, &args),
        "history" => history(shell, &args),
        "jobs" => jobs(shell, &args),
        "wait" => wait(shell, &args),
        "disown" => disown(shell, &args),
        "fg" => fg(shell, &args),
        "bg" => bg(shell, &args),
        "kill" => kill(shell, &args),
        "help" | "?" => help(),
        "which" => which(shell, &args),
        "true" => Ok(0),
        "false" => Ok(1),
        "set" => set(shell, &args),
        "explain" => crate::explain::run(shell, &argv[1..].to_vec()),
        "bookmark" => crate::bookmark::run(shell, argv),
        "exec" => exec_replace(shell, &args),
        "eval" => eval(shell, &args),
        _ => Err(anyhow!("builtin chưa hỗ trợ: {}", cmd)),
    };
    flush_std();
    result
}

fn flush_std() {
    use std::io::Write;
    let _ = std::io::stdout().flush();
    let _ = std::io::stderr().flush();
}

/// Save the original stdin/stdout/stderr file descriptors and apply the
/// redirects. When dropped, restores the originals.
fn cd(shell: &Rc<RefCell<Shell>>, args: &[&str]) -> Result<i32> {
    let target = if args.is_empty() {
        dirs::home_dir().ok_or_else(|| anyhow!("không tìm được HOME"))?
    } else if args[0] == "-" {
        let prev = shell.borrow().env.get("OLDPWD").cloned();
        match prev {
            Some(p) => std::path::PathBuf::from(p),
            None => return Err(anyhow!("OLDPWD chưa được đặt")),
        }
    } else {
        let expanded = shellexpand::tilde(args[0]).to_string();
        std::path::PathBuf::from(expanded)
    };
    let abs = if target.is_absolute() {
        target.clone()
    } else {
        shell.borrow().cwd.join(&target)
    };
    let canon = std::fs::canonicalize(&abs).map_err(|e| anyhow!("cd {}: {}", abs.display(), e))?;
    let old = shell.borrow().cwd.clone();
    shell.borrow_mut().export_var("OLDPWD", Some(&old.display().to_string()))?;
    std::env::set_current_dir(&canon)?;
    shell.borrow_mut().cwd = canon.clone();
    shell.borrow_mut().export_var("PWD", Some(&canon.display().to_string()))?;
    Ok(0)
}

fn pwd(shell: &Rc<RefCell<Shell>>) -> Result<i32> {
    println!("{}", shell.borrow().cwd.display());
    Ok(0)
}

fn exit(shell: &Rc<RefCell<Shell>>, args: &[&str]) -> Result<i32> {
    let code = match args.first() {
        Some(a) => a.parse::<i32>().map_err(|_| anyhow!("exit: {}: cần số", a))?,
        None => shell.borrow().last_status(),
    };
    if !crate::jobctl::confirm_exit(shell) {
        return Ok(1);
    }
    // Lời chào chỉ cho phiên tương tác — `-c` / script không được in rác ra stdout.
    if crate::jobctl::interactive() {
        println!("{} 👋", crate::i18n::t("common.goodbye"));
        crate::jobctl::shutdown(shell);
    }
    flush_std();
    std::process::exit(code);
}

/// Bọc giá trị trong nháy kép kiểu bash (`declare -p`, `export -p`).
fn dq(v: &str) -> String {
    let mut out = String::with_capacity(v.len() + 2);
    out.push('"');
    for c in v.chars() {
        if matches!(c, '"' | '\\' | '$' | '`') {
            out.push('\\');
        }
        out.push(c);
    }
    out.push('"');
    out
}

/// In biến dạng `declare -x NAME="v"` (sắp theo tên), lọc bằng `pick`.
fn print_decls(shell: &Shell, pick: impl Fn(&str) -> bool) {
    let mut names: Vec<&String> = shell.env.keys().filter(|k| pick(k) && *k != "?").collect();
    names.sort();
    for k in names {
        let mut flags = String::new();
        if shell.readonly.contains(k) {
            flags.push('r');
        }
        if shell.exported.contains(k) {
            flags.push('x');
        }
        let flags = if flags.is_empty() { "--".to_string() } else { format!("-{}", flags) };
        println!("declare {} {}={}", flags, k, dq(&shell.env[k]));
    }
}

fn export(shell: &Rc<RefCell<Shell>>, args: &[&str]) -> Result<i32> {
    let mut unexport = false;
    let mut names = Vec::new();
    for a in args {
        match *a {
            "-n" => unexport = true,
            "-p" => {}
            "-f" => return Err(anyhow!("export: -f: chưa hỗ trợ function")),
            _ => names.push(*a),
        }
    }
    if names.is_empty() {
        let sh = shell.borrow();
        print_decls(&sh, |k| sh.exported.contains(k));
        return Ok(0);
    }
    let mut code = 0;
    for a in names {
        let (k, v) = match a.split_once('=') {
            Some((k, v)) => (k, Some(v)),
            None => (a, None),
        };
        if !valid_name(k) {
            eprintln!("jaksh: export: `{}`: tên biến không hợp lệ", a);
            code = 1;
            continue;
        }
        let mut sh = shell.borrow_mut();
        let res = if unexport {
            if let Some(v) = v {
                sh.set_var(k, v)?;
            }
            sh.unexport_var(k);
            Ok(())
        } else {
            sh.export_var(k, v)
        };
        if let Err(e) = res {
            eprintln!("jaksh: export: {e}");
            code = 1;
        }
    }
    Ok(code)
}

fn valid_name(k: &str) -> bool {
    let mut c = k.chars();
    matches!(c.next(), Some(f) if f.is_ascii_alphabetic() || f == '_') && c.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn unset(shell: &Rc<RefCell<Shell>>, args: &[&str]) -> Result<i32> {
    let mut code = 0;
    for a in args.iter().filter(|a| !matches!(**a, "-v" | "-f")) {
        if let Err(e) = shell.borrow_mut().unset_var(a) {
            eprintln!("jaksh: unset: {e}");
            code = 1;
        }
    }
    Ok(code)
}

/// `readonly NAME[=v]...` / `readonly -p`.
fn readonly(shell: &Rc<RefCell<Shell>>, args: &[&str]) -> Result<i32> {
    let names: Vec<&str> = args.iter().copied().filter(|a| *a != "-p").collect();
    if names.is_empty() {
        let sh = shell.borrow();
        print_decls(&sh, |k| sh.readonly.contains(k));
        return Ok(0);
    }
    for a in names {
        let (k, v) = a.split_once('=').map_or((a, None), |(k, v)| (k, Some(v)));
        if !valid_name(k) {
            return Err(anyhow!("readonly: `{}`: tên biến không hợp lệ", a));
        }
        let mut sh = shell.borrow_mut();
        if let Some(v) = v {
            sh.set_var(k, v)?;
        }
        sh.readonly.insert(k.to_string());
    }
    Ok(0)
}

/// `declare` / `typeset` — phần không cần mảng: `-x` export, `+x` bỏ export,
/// `-r` readonly, `-p` in, `-i` (số nguyên — giữ chuỗi, chưa có số học).
fn declare(shell: &Rc<RefCell<Shell>>, args: &[&str]) -> Result<i32> {
    let mut export = None;
    let mut ro = false;
    let mut names = Vec::new();
    for a in args {
        if let Some(flags) = a.strip_prefix('-').filter(|f| !f.is_empty() && !f.contains('=')) {
            for f in flags.chars() {
                match f {
                    'x' => export = Some(true),
                    'r' => ro = true,
                    'p' | 'i' | 'g' | 'l' | 'u' => {}
                    'a' | 'A' => return Err(anyhow!("declare: -{}: chưa hỗ trợ mảng", f)),
                    'f' | 'F' => return Err(anyhow!("declare: -{}: chưa hỗ trợ function", f)),
                    _ => return Err(anyhow!("declare: -{}: tuỳ chọn không hợp lệ", f)),
                }
            }
        } else if a.strip_prefix('+') == Some("x") {
            export = Some(false);
        } else {
            names.push(*a);
        }
    }
    if names.is_empty() {
        let sh = shell.borrow();
        print_decls(&sh, |k| (!ro || sh.readonly.contains(k)) && (export != Some(true) || sh.exported.contains(k)));
        return Ok(0);
    }
    for a in names {
        let (k, v) = a.split_once('=').map_or((a, None), |(k, v)| (k, Some(v)));
        if !valid_name(k) {
            return Err(anyhow!("declare: `{}`: tên biến không hợp lệ", a));
        }
        let mut sh = shell.borrow_mut();
        match (export, v) {
            (Some(true), v) => sh.export_var(k, v)?,
            (Some(false), v) => {
                if let Some(v) = v {
                    sh.set_var(k, v)?;
                }
                sh.unexport_var(k);
            }
            (None, Some(v)) => sh.set_var(k, v)?,
            (None, None) => {
                if !sh.env.contains_key(k) {
                    sh.set_var(k, "")?;
                }
            }
        }
        if ro {
            sh.readonly.insert(k.to_string());
        }
    }
    Ok(0)
}

/// `shift [n]` — bỏ n tham số vị trí đầu.
fn shift(shell: &Rc<RefCell<Shell>>, args: &[&str]) -> Result<i32> {
    let n: usize = match args.first() {
        Some(a) => a.parse().map_err(|_| anyhow!("shift: {}: cần số", a))?,
        None => 1,
    };
    let mut sh = shell.borrow_mut();
    if n > sh.positional.len() {
        return Ok(1);
    }
    sh.positional.drain(..n);
    Ok(0)
}

/// `set` — tuỳ chọn shell (`-e -u -x -a -C`, `-o tên`, `+…` để tắt), `set --
/// args` đặt tham số vị trí, `set -o` / `set +o` liệt kê, `set` không đối số
/// in mọi biến.
fn set(shell: &Rc<RefCell<Shell>>, args: &[&str]) -> Result<i32> {
    use crate::shell::ShellOpts;
    if args.is_empty() {
        let sh = shell.borrow();
        let mut names: Vec<&String> = sh.env.keys().filter(|k| *k != "?").collect();
        names.sort();
        for k in names {
            println!("{}={}", k, crate::builtins::sq(&sh.env[k]));
        }
        return Ok(0);
    }
    let mut i = 0;
    while i < args.len() {
        let a = args[i];
        if a == "--" {
            shell.borrow_mut().positional = args[i + 1..].iter().map(|s| s.to_string()).collect();
            return Ok(0);
        }
        let on = match a.chars().next() {
            Some('-') => true,
            Some('+') => false,
            _ => {
                // `set a b c` (không cờ) cũng đặt tham số vị trí.
                shell.borrow_mut().positional = args[i..].iter().map(|s| s.to_string()).collect();
                return Ok(0);
            }
        };
        let body = &a[1..];
        if body == "o" {
            match args.get(i + 1) {
                Some(name) => {
                    if !shell.borrow_mut().opts.set(name, on) {
                        return Err(anyhow!("set: {}: tên tuỳ chọn không hợp lệ", name));
                    }
                    i += 2;
                    continue;
                }
                None => {
                    let sh = shell.borrow();
                    for (name, _) in ShellOpts::NAMES {
                        let v = sh.opts.get(name).unwrap_or(false);
                        if on {
                            println!("{:<15} {}", name, if v { "on" } else { "off" });
                        } else {
                            println!("set {}o {}", if v { '-' } else { '+' }, name);
                        }
                    }
                    return Ok(0);
                }
            }
        }
        for c in body.chars() {
            match ShellOpts::name_of_flag(c) {
                Some(name) => {
                    shell.borrow_mut().opts.set(name, on);
                }
                // Cờ bash chưa hỗ trợ nhưng vô hại → bỏ qua thay vì làm hỏng script.
                None if matches!(c, 'h' | 'B' | 'H' | 'm' | 'b' | 'f' | 'v' | 'T' | 'E' | 'P') => {}
                None => return Err(anyhow!("set: -{}: tuỳ chọn không hợp lệ", c)),
            }
        }
        i += 1;
    }
    Ok(0)
}

/// Bọc nháy đơn nếu cần (an toàn để dán lại vào shell) — dùng cho `set`, `set -x`.
pub fn sq(v: &str) -> String {
    if !v.is_empty() && v.chars().all(|c| c.is_ascii_alphanumeric() || "_-./:=@%+,".contains(c)) {
        return v.to_string();
    }
    format!("'{}'", v.replace('\'', "'\\''"))
}

fn alias(shell: &Rc<RefCell<Shell>>, args: &[&str]) -> Result<i32> {
    if args.is_empty() {
        let mut entries: Vec<(String, String)> = shell.borrow().aliases.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        entries.sort_by(|a, b| a.0.cmp(&b.0));
        for (k, v) in entries {
            println!("alias {}='{}'", k, v);
        }
        return Ok(0);
    }
    for a in args {
        if let Some((k, v)) = a.split_once('=') {
            let v = v.trim_matches(|c| c == '\'' || c == '"');
            shell.borrow_mut().aliases.insert(k.to_string(), v.to_string());
        } else if let Some(v) = shell.borrow().aliases.get(*a) {
            println!("alias {}='{}'", a, v);
        }
    }
    Ok(0)
}

fn unalias(shell: &Rc<RefCell<Shell>>, args: &[&str]) -> Result<i32> {
    for a in args {
        shell.borrow_mut().aliases.remove(*a);
    }
    Ok(0)
}

fn echo(args: &[&str]) -> Result<i32> {
    use std::io::Write;
    // Cờ chỉ nhận khi toàn bộ là -n/-e/-E (`-ne`, `-en`…); `-x` là chữ thường.
    let mut newline = true;
    let mut escapes = false;
    let mut start = 0;
    for a in args {
        match a.strip_prefix('-') {
            Some(flags) if !flags.is_empty() && flags.chars().all(|c| matches!(c, 'n' | 'e' | 'E')) => {
                for c in flags.chars() {
                    match c {
                        'n' => newline = false,
                        'e' => escapes = true,
                        _ => escapes = false,
                    }
                }
                start += 1;
            }
            _ => break,
        }
    }
    let mut line = args[start..].join(" ");
    if escapes {
        let (text, stop) = interpret_escapes(&line);
        line = text;
        if stop {
            newline = false;
        }
    }
    // Rust bỏ qua im lặng lỗi EBADF trên stdout → tự kiểm tra fd 1 còn mở.
    if unsafe { libc::fcntl(libc::STDOUT_FILENO, libc::F_GETFD) } < 0 {
        return Err(anyhow!("echo: lỗi ghi: Bad file descriptor"));
    }
    let mut out = std::io::stdout().lock();
    let res = if newline { writeln!(out, "{}", line) } else { write!(out, "{}", line) };
    // stdout đã đóng (`echo x >&-`) → lỗi ghi, exit 1 như bash.
    if let Err(e) = res.and_then(|_| out.flush()) {
        return Err(anyhow!("echo: lỗi ghi: {}", e));
    }
    Ok(0)
}

/// Escape kiểu `echo -e` / `$'...'`: \n \t \\ \a \b \e \f \r \v \0NNN \xHH.
/// Trả cờ `true` khi gặp `\c` (ngừng in, kể cả xuống dòng).
pub fn interpret_escapes(s: &str) -> (String, bool) {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('\\') => out.push('\\'),
            Some('a') => out.push('\x07'),
            Some('b') => out.push('\x08'),
            Some('e') | Some('E') => out.push('\x1b'),
            Some('f') => out.push('\x0c'),
            Some('r') => out.push('\r'),
            Some('v') => out.push('\x0b'),
            Some('c') => return (out, true),
            Some('0') => {
                let mut n = 0u32;
                for _ in 0..3 {
                    match chars.peek().and_then(|c| c.to_digit(8)) {
                        Some(d) => {
                            n = n * 8 + d;
                            chars.next();
                        }
                        None => break,
                    }
                }
                out.push(char::from_u32(n).unwrap_or('\0'));
            }
            Some('x') => {
                let mut n = 0u32;
                let mut got = 0;
                while got < 2 {
                    match chars.peek().and_then(|c| c.to_digit(16)) {
                        Some(d) => {
                            n = n * 16 + d;
                            chars.next();
                            got += 1;
                        }
                        None => break,
                    }
                }
                if got == 0 {
                    out.push_str("\\x");
                } else {
                    out.push(char::from_u32(n).unwrap_or('?'));
                }
            }
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    (out, false)
}

/// `read [-r] [-s] [-p prompt] [-t giây] [-n N] [-d delim] [tên...]` — đọc 1
/// dòng từ stdin, tách theo `$IFS` vào các biến (biến cuối nhận phần còn lại;
/// không tên → `$REPLY`). Đọc từng byte (không buffer) để phần input còn lại
/// vẫn dành cho lệnh sau; Ctrl-C ngắt được.
fn read(shell: &Rc<RefCell<Shell>>, args: &[&str]) -> Result<i32> {
    let mut raw = false;
    let mut silent = false;
    let mut prompt: Option<String> = None;
    let mut timeout: Option<f64> = None;
    let mut nchars: Option<usize> = None;
    let mut delim = b'\n';
    let mut names: Vec<String> = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let a = args[i];
        if names.is_empty() && a.len() > 1 && a.starts_with('-') {
            let flags: Vec<char> = a[1..].chars().collect();
            let mut k = 0;
            while k < flags.len() {
                let f = flags[k];
                if matches!(f, 'p' | 't' | 'n' | 'N' | 'd' | 'a' | 'u') {
                    let rest: String = flags[k + 1..].iter().collect();
                    let val = if !rest.is_empty() {
                        rest
                    } else {
                        i += 1;
                        args.get(i).ok_or_else(|| anyhow!("read: -{}: thiếu giá trị", f))?.to_string()
                    };
                    match f {
                        'p' => prompt = Some(val),
                        't' => timeout = Some(val.parse().map_err(|_| anyhow!("read: {}: thời gian không hợp lệ", val))?),
                        'n' | 'N' => nchars = Some(val.parse().map_err(|_| anyhow!("read: {}: số không hợp lệ", val))?),
                        'd' => delim = val.bytes().next().unwrap_or(0),
                        'a' => return Err(anyhow!("read: -a: chưa hỗ trợ mảng")),
                        _ => return Err(anyhow!("read: -u: chưa hỗ trợ")),
                    }
                    break;
                }
                match f {
                    'r' => raw = true,
                    's' => silent = true,
                    'e' | 'i' => {}
                    _ => return Err(anyhow!("read: -{}: tuỳ chọn không hợp lệ", f)),
                }
                k += 1;
            }
        } else {
            names.push(a.to_string());
        }
        i += 1;
    }

    let tty = unsafe { libc::isatty(0) } == 1;
    if let Some(p) = &prompt {
        if tty {
            eprint!("{}", p);
        }
    }
    // -s: tắt echo của terminal trong lúc gõ (mật khẩu).
    let saved_termios = if silent && tty {
        unsafe {
            let mut t: libc::termios = std::mem::zeroed();
            (libc::tcgetattr(0, &mut t) == 0).then(|| {
                let mut quiet = t;
                quiet.c_lflag &= !libc::ECHO;
                libc::tcsetattr(0, libc::TCSANOW, &quiet);
                t
            })
        }
    } else {
        None
    };

    let deadline = timeout.map(|t| std::time::Instant::now() + std::time::Duration::from_secs_f64(t));
    let mut bytes: Vec<u8> = Vec::new();
    let mut eof = false;
    let mut timed_out = false;
    let mut interrupted = false;
    loop {
        if nchars.is_some_and(|n| String::from_utf8_lossy(&bytes).chars().count() >= n) {
            break;
        }
        // poll theo lát 100ms: kiểm tra timeout và Ctrl-C (handler đặt cờ).
        let mut pfd = libc::pollfd { fd: 0, events: libc::POLLIN, revents: 0 };
        let wait_ms = match deadline {
            Some(d) => {
                let left = d.saturating_duration_since(std::time::Instant::now());
                if left.is_zero() {
                    timed_out = true;
                    break;
                }
                left.as_millis().min(100) as i32
            }
            None => 100,
        };
        let r = unsafe { libc::poll(&mut pfd, 1, wait_ms) };
        if crate::jobctl::interrupted() {
            interrupted = true;
            break;
        }
        if r <= 0 {
            continue;
        }
        let mut b = [0u8; 1];
        let n = unsafe { libc::read(0, b.as_mut_ptr() as *mut libc::c_void, 1) };
        if n <= 0 {
            if n < 0 && std::io::Error::last_os_error().raw_os_error() == Some(libc::EINTR) {
                continue;
            }
            eof = true;
            break;
        }
        let c = b[0];
        if !raw && c == b'\\' {
            // `\` + xuống dòng = nối dòng; `\x` = ký tự x giữ nguyên.
            let mut nb = [0u8; 1];
            if unsafe { libc::read(0, nb.as_mut_ptr() as *mut libc::c_void, 1) } == 1 {
                if nb[0] != b'\n' {
                    bytes.push(nb[0]);
                }
            }
            continue;
        }
        if c == delim && nchars.is_none() {
            break;
        }
        bytes.push(c);
    }
    if let Some(t) = saved_termios {
        unsafe { libc::tcsetattr(0, libc::TCSANOW, &t) };
        if tty {
            eprintln!();
        }
    }
    if interrupted {
        return Ok(130);
    }

    let line = String::from_utf8_lossy(&bytes).into_owned();
    let ifs = shell.borrow().get_var("IFS").unwrap_or_else(|| " \t\n".into());
    let is_ifs = |c: char| ifs.contains(c);
    let mut sh = shell.borrow_mut();
    if names.is_empty() {
        sh.set_var("REPLY", &line)?;
    } else {
        let mut rest = line.trim_matches(|c: char| is_ifs(c) && c.is_whitespace());
        let last = names.len() - 1;
        for (k, name) in names.iter().enumerate() {
            if k == last {
                sh.set_var(name, rest.trim_end_matches(|c: char| is_ifs(c) && c.is_whitespace()))?;
                break;
            }
            let end = rest.find(is_ifs).unwrap_or(rest.len());
            sh.set_var(name, &rest[..end])?;
            rest = rest[end..].trim_start_matches(|c: char| is_ifs(c) && c.is_whitespace());
            // Ký tự IFS không phải khoảng trắng (vd `:`) chỉ ăn đúng 1 lần.
            if let Some(c) = rest.chars().next().filter(|c| is_ifs(*c) && !c.is_whitespace()) {
                rest = rest[c.len_utf8()..].trim_start_matches(|c: char| is_ifs(c) && c.is_whitespace());
            }
        }
    }
    if timed_out {
        return Ok(142);
    }
    // Gặp EOF trước delimiter → 1 (biến vẫn được gán), như bash.
    Ok(eof as i32)
}

/// Tìm `name` trong `$PATH` của shell (không phải của tiến trình — biến PATH
/// có thể vừa được gán mà chưa export).
fn path_lookup(shell: &Shell, name: &str) -> Option<std::path::PathBuf> {
    let path = shell.get_var("PATH").unwrap_or_default();
    which::which_in(name, Some(path), &shell.cwd).ok()
}

/// Cách shell sẽ hiểu `name`: alias / builtin / file.
enum Kind {
    Alias(String),
    Builtin,
    File(std::path::PathBuf),
}

fn classify(shell: &Shell, name: &str, all: bool) -> Vec<Kind> {
    let mut out = Vec::new();
    if let Some(v) = shell.aliases.get(name) {
        out.push(Kind::Alias(v.clone()));
    }
    if is_builtin(name) || name == "jak" || name == "command" || name == "builtin" {
        out.push(Kind::Builtin);
    }
    if all {
        let path = shell.get_var("PATH").unwrap_or_default();
        if let Ok(paths) = which::which_in_all(name, Some(path), &shell.cwd) {
            out.extend(paths.map(Kind::File));
        }
    } else if out.is_empty() {
        if let Some(p) = path_lookup(shell, name) {
            out.push(Kind::File(p));
        }
    }
    out
}

/// `command -v|-V name...` (dạng `command cmd args` do executor xử lý).
fn command(shell: &Rc<RefCell<Shell>>, args: &[&str]) -> Result<i32> {
    let verbose = args.first() == Some(&"-V");
    let names: Vec<&str> = args.iter().copied().filter(|a| !a.starts_with('-')).collect();
    if names.is_empty() {
        return Ok(0);
    }
    let sh = shell.borrow();
    let mut code = 0;
    for name in names {
        match classify(&sh, name, false).into_iter().next() {
            Some(Kind::Alias(v)) if verbose => println!("{} is aliased to `{}'", name, v),
            Some(Kind::Alias(v)) => println!("alias {}={}", name, sq(&v)),
            Some(Kind::Builtin) if verbose => println!("{} is a shell builtin", name),
            Some(Kind::Builtin) => println!("{}", name),
            Some(Kind::File(p)) if verbose => println!("{} is {}", name, p.display()),
            Some(Kind::File(p)) => println!("{}", p.display()),
            None => {
                if verbose {
                    eprintln!("jaksh: command: {}: không tìm thấy", name);
                }
                code = 1;
            }
        }
    }
    Ok(code)
}

/// `type [-t|-p|-P|-a] name...`
fn type_(shell: &Rc<RefCell<Shell>>, args: &[&str]) -> Result<i32> {
    let mut only_type = false;
    let mut only_path = false;
    let mut all = false;
    let mut names = Vec::new();
    for a in args {
        match *a {
            "-t" => only_type = true,
            "-p" | "-P" => only_path = true,
            "-a" => all = true,
            _ => names.push(*a),
        }
    }
    let sh = shell.borrow();
    let mut code = 0;
    for name in names {
        let kinds = classify(&sh, name, all);
        if kinds.is_empty() {
            if !only_type && !only_path {
                eprintln!("jaksh: type: {}: không tìm thấy", name);
            }
            code = 1;
            continue;
        }
        for k in kinds {
            match k {
                Kind::Alias(_) if only_path => {}
                Kind::Builtin if only_path => {}
                Kind::File(p) if only_path => println!("{}", p.display()),
                Kind::Alias(_) if only_type => println!("alias"),
                Kind::Builtin if only_type => println!("builtin"),
                Kind::File(_) if only_type => println!("file"),
                Kind::Alias(v) => println!("{} is aliased to `{}'", name, v),
                Kind::Builtin => println!("{} is a shell builtin", name),
                Kind::File(p) => println!("{} is {}", name, p.display()),
            }
            if !all {
                break;
            }
        }
    }
    Ok(code)
}

fn source(shell: &Rc<RefCell<Shell>>, args: &[&str]) -> Result<i32> {
    let path = args.first().ok_or_else(|| anyhow!("dùng: source <tệp> [đối số...]"))?;
    let content = std::fs::read_to_string(path).map_err(|e| anyhow!("source: {}: {}", path, e))?;
    // Có đối số → tạm thay tham số vị trí trong lúc chạy file (như bash).
    let saved = (args.len() > 1).then(|| {
        std::mem::replace(&mut shell.borrow_mut().positional, args[1..].iter().map(|s| s.to_string()).collect())
    });
    let code = crate::config::run_script(shell, &content);
    if let Some(p) = saved {
        shell.borrow_mut().positional = p;
    }
    code
}

fn history(shell: &Rc<RefCell<Shell>>, args: &[&str]) -> Result<i32> {
    // Đọc từ lịch sử trong bộ nhớ của phiên (cập nhật theo thời gian thực),
    // KHÔNG đọc file: file chỉ phản ánh lần thoát trước nên sẽ cũ, và còn ở
    // định dạng `#V2` (có header + escape `\n`) không hợp để in trực tiếp.
    match args {
        [] => {}
        ["-c"] => {
            let mut sh = shell.borrow_mut();
            sh.history.clear();
            sh.history_clear_requested = true;
            return Ok(0);
        }
        ["--scrub"] => {
            shell.borrow_mut().history_scrub_requested = true;
            return Ok(0);
        }
        [n] if n.parse::<usize>().is_ok() => {}
        _ => return Err(anyhow!("history: cách dùng: history [N] | history -c | history --scrub")),
    }
    let sh = shell.borrow();
    let total = sh.history.len();
    let skip = args.first().and_then(|n| n.parse::<usize>().ok()).map_or(0, |n| total.saturating_sub(n));
    for (i, line) in sh.history.iter().enumerate().skip(skip) {
        println!("{:5}  {}", i + 1, line);
    }
    Ok(0)
}

fn jobs(shell: &Rc<RefCell<Shell>>, args: &[&str]) -> Result<i32> {
    shell.borrow_mut().reap_jobs();
    let long = args.contains(&"-l");
    let only_pids = args.contains(&"-p");
    let sh = shell.borrow();
    if sh.jobs.is_empty() {
        if unsafe { libc::isatty(libc::STDOUT_FILENO) } == 1 {
            println!("(không có job nào)");
        }
        return Ok(0);
    }
    let current = sh.jobs.last().map(|j| j.id);
    let previous = sh.jobs.iter().rev().nth(1).map(|j| j.id);
    for j in &sh.jobs {
        if only_pids {
            println!("{}", j.pgid);
            continue;
        }
        let mark = if Some(j.id) == current { '+' } else if Some(j.id) == previous { '-' } else { ' ' };
        let state = match j.state {
            crate::shell::JobState::Running => "Running",
            crate::shell::JobState::Stopped => "Stopped",
            crate::shell::JobState::Done => "Done",
        };
        if long {
            println!("[{}]{} {:>7} {:<9} {}", j.id, mark, j.pgid, state, j.cmd);
        } else {
            println!("[{}]{}  {:<9} {}", j.id, mark, state, j.cmd);
        }
    }
    Ok(0)
}

fn fg(shell: &Rc<RefCell<Shell>>, args: &[&str]) -> Result<i32> {
    if !crate::jobctl::interactive() {
        return Err(anyhow!("fg: không có job control (shell không tương tác)"));
    }
    shell.borrow_mut().reap_jobs();
    let Some(id) = shell.borrow().find_job(args.first().copied()) else {
        return Err(anyhow!("fg: {}: không có job này", args.first().unwrap_or(&"%%")));
    };
    shell.borrow_mut().touch_job(id);
    Ok(crate::jobctl::foreground(shell, id))
}

fn bg(shell: &Rc<RefCell<Shell>>, args: &[&str]) -> Result<i32> {
    shell.borrow_mut().reap_jobs();
    let Some(id) = shell.borrow().find_job(args.first().copied()) else {
        return Err(anyhow!("bg: {}: không có job này", args.first().unwrap_or(&"%%")));
    };
    let mut sh = shell.borrow_mut();
    let job = sh.jobs.iter_mut().find(|j| j.id == id).expect("find_job trả id hợp lệ");
    unsafe { libc::kill(-job.pgid, libc::SIGCONT) };
    job.state = crate::shell::JobState::Running;
    println!("[{}]+ {} &", job.id, job.cmd);
    Ok(0)
}

/// `wait [%N|pid ...]` — chờ job nền (không đối số = tất cả) xong.
fn wait(shell: &Rc<RefCell<Shell>>, args: &[&str]) -> Result<i32> {
    let targets: Vec<i32> = if args.is_empty() {
        shell.borrow().jobs.iter().flat_map(|j| j.pids.clone()).collect()
    } else {
        let sh = shell.borrow();
        let mut v = Vec::new();
        for a in args {
            if a.starts_with('%') {
                let id = sh.find_job(Some(a)).ok_or_else(|| anyhow!("wait: {}: không có job này", a))?;
                v.extend(sh.jobs.iter().find(|j| j.id == id).map(|j| j.pids.clone()).unwrap_or_default());
            } else {
                v.push(a.parse().map_err(|_| anyhow!("wait: `{}`: không phải pid hay job", a))?);
            }
        }
        v
    };
    let mut code = 0;
    for pid in targets {
        let mut st = 0;
        loop {
            let r = unsafe { libc::waitpid(pid, &mut st, 0) };
            if r < 0 && std::io::Error::last_os_error().raw_os_error() == Some(libc::EINTR) {
                if crate::jobctl::take_interrupted() {
                    return Ok(130);
                }
                continue;
            }
            code = if r < 0 { 127 } else if libc::WIFSIGNALED(st) { 128 + libc::WTERMSIG(st) } else { libc::WEXITSTATUS(st) };
            break;
        }
        for j in shell.borrow_mut().jobs.iter_mut() {
            j.pids.retain(|&p| p != pid);
        }
    }
    shell.borrow_mut().jobs.retain(|j| !j.pids.is_empty());
    Ok(code)
}

/// `disown [%N]` — bỏ job khỏi danh sách (shell không theo dõi / HUP nữa).
fn disown(shell: &Rc<RefCell<Shell>>, args: &[&str]) -> Result<i32> {
    if args == ["-a"] {
        shell.borrow_mut().jobs.clear();
        return Ok(0);
    }
    let Some(id) = shell.borrow().find_job(args.first().copied()) else {
        return Err(anyhow!("disown: {}: không có job này", args.first().unwrap_or(&"%%")));
    };
    shell.borrow_mut().jobs.retain(|j| j.id != id);
    Ok(0)
}

fn kill(shell: &Rc<RefCell<Shell>>, args: &[&str]) -> Result<i32> {
    use nix::sys::signal::Signal;
    use std::str::FromStr;
    if args.is_empty() {
        eprintln!("dùng: kill [-SIGNAL | -s SIGNAL] <pid | %job> ...  ·  kill -l");
        return Ok(2);
    }
    let parse_sig = |name: &str| -> Option<Signal> {
        if let Ok(n) = name.parse::<i32>() {
            return Signal::try_from(n).ok();
        }
        let up = name.to_ascii_uppercase();
        let full = if up.starts_with("SIG") { up } else { format!("SIG{}", up) };
        Signal::from_str(&full).ok()
    };
    if args[0] == "-l" || args[0] == "-L" {
        // `kill -l 9` → KILL, `kill -l 130` → INT (mã thoát 128+N), `kill -l` → tất cả.
        if let Some(n) = args.get(1).and_then(|a| a.parse::<i32>().ok()) {
            let n = if n > 128 { n - 128 } else { n };
            let sig = Signal::try_from(n).map_err(|_| anyhow!("kill: {}: số tín hiệu không hợp lệ", n))?;
            println!("{}", sig.as_str().trim_start_matches("SIG"));
        } else {
            let all: Vec<String> = Signal::iterator().map(|s| format!("{:>2}) {}", s as i32, s.as_str())).collect();
            for chunk in all.chunks(5) {
                println!("{}", chunk.join("\t"));
            }
        }
        return Ok(0);
    }
    let mut sig = Signal::SIGTERM;
    let mut i = 0;
    if args[0] == "-s" || args[0] == "-n" {
        let name = args.get(1).ok_or_else(|| anyhow!("kill: thiếu tên tín hiệu sau {}", args[0]))?;
        sig = parse_sig(name).ok_or_else(|| anyhow!("kill: {}: tín hiệu không hợp lệ", name))?;
        i = 2;
    } else if let Some(name) = args[0].strip_prefix('-') {
        sig = parse_sig(name).ok_or_else(|| anyhow!("kill: {}: tín hiệu không hợp lệ", name))?;
        i = 1;
    }
    let mut code = 0;
    for target in &args[i..] {
        // `%N` → cả process group của job (pid âm).
        let pid = if target.starts_with('%') {
            let sh = shell.borrow();
            match sh.find_job(Some(target)).and_then(|id| sh.jobs.iter().find(|j| j.id == id)) {
                Some(j) => -j.pgid,
                None => {
                    eprintln!("jaksh: kill: {}: không có job này", target);
                    code = 1;
                    continue;
                }
            }
        } else {
            match target.parse::<i32>() {
                Ok(p) => p,
                Err(_) => {
                    eprintln!("jaksh: kill: {}: phải là pid hoặc %job", target);
                    code = 1;
                    continue;
                }
            }
        };
        if let Err(e) = nix::sys::signal::kill(nix::unistd::Pid::from_raw(pid), sig) {
            eprintln!("jaksh: kill: ({}) - {}", target, e.desc());
            code = 1;
        } else if pid < 0 && sig != Signal::SIGCONT {
            // Job đang dừng không xử lý được tín hiệu tới khi chạy lại.
            let _ = nix::sys::signal::kill(nix::unistd::Pid::from_raw(pid), Signal::SIGCONT);
        }
    }
    Ok(code)
}

fn help() -> Result<i32> {
    crate::info::print_banner();
    println!("\x1b[1m{}\x1b[0m", crate::i18n::t("help.title"));
    println!();
    println!("\x1b[36m▸ {}\x1b[0m", crate::i18n::t("help.utilities"));
    println!("  jak clean             dọn cache & file tạm");
    println!("  jak backup <thư_mục>  nén & đặt tên theo ngày");
    println!("  jak update            cập nhật hệ thống (brew/apt/dnf)");
    println!("  jak find <tên>        tìm file/thư mục (file/dir/text/big/recent)");
    println!("  jak open <đường_dẫn>  mở bằng app mặc định");
    println!("  jak sysinfo           thông tin máy");
    println!("  jak theme <tên>       đổi giao diện");
    println!("  jak ip                IP nội bộ + public");
    println!("  jak weather [tp]      thời tiết");
    println!("  jak git <sub>         workflow git (save/sync/wip/amend/...)");
    println!("  jak <bookmark>        chạy lệnh đã bookmark (xem `bookmark`)");
    println!();
    println!("\x1b[36m▸ {}\x1b[0m", crate::i18n::t("help.bookmark_section"));
    println!("  bookmark <name> <cmd ...>   tạo / cập nhật");
    println!("  bookmark                    liệt kê");
    println!("  bookmark del <name>         xoá");
    println!();
    println!("\x1b[36m▸ {}\x1b[0m", crate::i18n::t("help.explain_section"));
    println!("  explain                liệt kê các lệnh đã có chú thích");
    println!("  explain <lệnh>         xem usage / tham số / ví dụ");
    println!("  explain ls -la         live annotate giá trị thật trên output");
    println!();
    println!("\x1b[36m▸ {}\x1b[0m", crate::i18n::t("help.jak_color_section"));
    println!("  ls -la --jak           tô màu permissions, icon thư mục/exec");
    println!("  ps aux --jak           PID, %CPU, %MEM tô theo ngưỡng");
    println!("  df -h --jak            Use% xanh→vàng→đỏ; size theo đơn vị");
    println!("  du -sh --jak           căn cột size");
    println!("  git status --jak       bố cục theo section + icon");
    println!("  git branch --jak       ● cho branch hiện tại");
    println!();
    println!("\x1b[2m{}\x1b[0m", crate::i18n::t("help.config_at"));
    Ok(0)
}

fn which(shell: &Rc<RefCell<Shell>>, args: &[&str]) -> Result<i32> {
    let mut code = 0;
    for a in args {
        if is_builtin(a) {
            println!("{}: lệnh tích hợp jaksh", a);
            continue;
        }
        if shell.borrow().aliases.contains_key(*a) {
            println!("{}: alias='{}'", a, shell.borrow().aliases[*a]);
            continue;
        }
        match which::which(a) {
            Ok(p) => println!("{}", p.display()),
            Err(_) => { eprintln!("không tìm thấy: {}", a); code = 1; }
        }
    }
    Ok(code)
}

/// `exec <cmd> [args...]` — thay thế tiến trình shell bằng <cmd> (POSIX).
/// BẮT BUỘC cho login shell: GDM/Xsession khởi động session qua
/// `$SHELL -c "exec <session>..."` — thiếu nó session chết ngay sau xác thực.
/// Redirect đã được áp vào fd của shell trước khi gọi — process mới kế
/// thừa nguyên (và không bao giờ restore vì exec không quay lại).
/// `exec` không args: chỉ áp redirect vĩnh viễn (xử lý ở `run`).
fn exec_replace(shell: &Rc<RefCell<Shell>>, args: &[&str]) -> Result<i32> {
    if args.is_empty() {
        return Ok(0);
    }
    use std::os::unix::process::CommandExt;
    let mut c = std::process::Command::new(args[0]);
    c.args(&args[1..]);
    c.current_dir(&shell.borrow().cwd);
    crate::jobctl::reset_for_exec(&mut c);
    // exec() chỉ trả về khi THẤT BẠI — thành công thì process này biến mất.
    let err = c.exec();
    if err.kind() == std::io::ErrorKind::NotFound {
        eprintln!("jaksh: exec: {}: {}", crate::i18n::t("common.not_found"), args[0]);
        Ok(127)
    } else {
        eprintln!("jaksh: exec: không chạy được {}: {}", args[0], err);
        Ok(126)
    }
}

/// `eval <lời...>` — nối các tham số bằng dấu cách rồi phân tích + thực thi như
/// một dòng lệnh trong chính shell hiện tại (POSIX). Nhờ đó các init script phổ
/// biến chạy được, ví dụ `eval "$(brew shellenv)"`, `eval "$(zoxide init ...)"`.
/// Chạy in-process nên phép gán / `cd` bên trong có hiệu lực với shell cha.
fn eval(shell: &Rc<RefCell<Shell>>, args: &[&str]) -> Result<i32> {
    if args.is_empty() {
        return Ok(0);
    }
    let src = args.join(" ");
    let tokens = crate::lexer::tokenize(&src)?;
    let ast = crate::parser::parse(&tokens)?;
    crate::executor::execute(shell, &ast)
}
