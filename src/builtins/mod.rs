use anyhow::{anyhow, Result};
use std::cell::RefCell;
use std::os::fd::AsRawFd;
use std::rc::Rc;

use crate::parser::{Redirect, RedirectKind};
use crate::shell::Shell;

pub const BUILTINS: &[&str] = &[
    "cd", "pwd", "exit", "export", "unset", "alias", "unalias", "set",
    "echo", "source", ".", "history", "jobs", "fg", "bg", "kill",
    "help", "?", "which", "true", "false", "explain", "bookmark", "exec",
    "eval", "wait", "disown",
];

pub fn is_builtin(name: &str) -> bool {
    BUILTINS.contains(&name)
}

/// Builtin chỉ in ra stdout, không đổi trạng thái shell — chạy được ngay trong
/// pipeline (`history | grep git`). Builtin còn lại (`cd`, `exit`, `export`…)
/// trong bash chạy ở subshell nên vô tác dụng; ở đây vẫn rơi về lệnh ngoài.
pub fn is_pipe_safe(name: &str) -> bool {
    matches!(name, "history" | "alias" | "jobs" | "which" | "help" | "?" | "explain" | "echo" | "pwd" | "true" | "false")
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
        flush_std();
        let mut guard = RedirectGuard { saved: Vec::new(), open_files: Vec::new() };
        guard.replace_fd(1, writer.as_raw_fd())?;
        drop(writer);
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
    let _guard = RedirectGuard::install(shell, redirects)?;
    flush_std();
    let result = match cmd {
        "cd" => cd(shell, &args),
        "pwd" => pwd(shell),
        "exit" => exit(shell, &args),
        "export" => export(shell, &args),
        "unset" => unset(shell, &args),
        "alias" => alias(shell, &args),
        "unalias" => unalias(shell, &args),
        "echo" => echo(&args),
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
        "set" => Ok(0),
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
struct RedirectGuard {
    saved: Vec<(i32, i32)>, // (target_fd, saved_dup_fd)
    open_files: Vec<std::fs::File>,
}

impl RedirectGuard {
    fn install(shell: &Rc<RefCell<Shell>>, redirects: &[Redirect]) -> Result<Self> {
        let mut guard = RedirectGuard { saved: Vec::new(), open_files: Vec::new() };
        for r in redirects {
            let target_pieces = crate::expand::expand_word(&shell.borrow(), &r.target, true);
            let target_path = target_pieces.into_iter().next().ok_or_else(|| anyhow!("đích redirect rỗng"))?;
            match r.kind {
                RedirectKind::In => {
                    let f = std::fs::File::open(&target_path)?;
                    guard.replace_fd(0, f.as_raw_fd())?;
                    guard.open_files.push(f);
                }
                RedirectKind::Out => {
                    let f = std::fs::OpenOptions::new().write(true).create(true).truncate(true).open(&target_path)?;
                    guard.replace_fd(1, f.as_raw_fd())?;
                    guard.open_files.push(f);
                }
                RedirectKind::Append => {
                    let f = std::fs::OpenOptions::new().write(true).create(true).append(true).open(&target_path)?;
                    guard.replace_fd(1, f.as_raw_fd())?;
                    guard.open_files.push(f);
                }
                RedirectKind::ErrOut => {
                    let f = std::fs::OpenOptions::new().write(true).create(true).truncate(true).open(&target_path)?;
                    guard.replace_fd(2, f.as_raw_fd())?;
                    guard.open_files.push(f);
                }
                RedirectKind::ErrAppend => {
                    let f = std::fs::OpenOptions::new().write(true).create(true).append(true).open(&target_path)?;
                    guard.replace_fd(2, f.as_raw_fd())?;
                    guard.open_files.push(f);
                }
                RedirectKind::AllOut => {
                    let f = std::fs::OpenOptions::new().write(true).create(true).truncate(true).open(&target_path)?;
                    guard.replace_fd(1, f.as_raw_fd())?;
                    guard.replace_fd(2, f.as_raw_fd())?;
                    guard.open_files.push(f);
                }
            }
        }
        Ok(guard)
    }

    fn replace_fd(&mut self, target: i32, new_fd: i32) -> Result<()> {
        unsafe {
            let saved = libc::dup(target);
            if saved < 0 {
                return Err(anyhow!("dup({}) thất bại", target));
            }
            if libc::dup2(new_fd, target) < 0 {
                libc::close(saved);
                return Err(anyhow!("dup2({}, {}) thất bại", new_fd, target));
            }
            self.saved.push((target, saved));
        }
        Ok(())
    }
}

impl Drop for RedirectGuard {
    fn drop(&mut self) {
        flush_std();
        unsafe {
            for (target, saved) in self.saved.drain(..).rev() {
                libc::dup2(saved, target);
                libc::close(saved);
            }
        }
    }
}

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
    shell.borrow_mut().env.insert("OLDPWD".into(), old.display().to_string());
    std::env::set_current_dir(&canon)?;
    shell.borrow_mut().cwd = canon.clone();
    shell.borrow_mut().env.insert("PWD".into(), canon.display().to_string());
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

fn export(shell: &Rc<RefCell<Shell>>, args: &[&str]) -> Result<i32> {
    if args.is_empty() {
        let env: Vec<(String, String)> = shell.borrow().env.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        let mut entries = env;
        entries.sort_by(|a, b| a.0.cmp(&b.0));
        for (k, v) in entries {
            println!("export {}={}", k, v);
        }
        return Ok(0);
    }
    for a in args {
        if let Some((k, v)) = a.split_once('=') {
            shell.borrow_mut().set_var(k, v);
        } else {
            // export NAME (promote existing local var to env — we treat all as env already)
            let cur = shell.borrow().env.get(*a).cloned();
            if let Some(v) = cur {
                std::env::set_var(a, v);
            }
        }
    }
    Ok(0)
}

fn unset(shell: &Rc<RefCell<Shell>>, args: &[&str]) -> Result<i32> {
    for a in args {
        shell.borrow_mut().unset_var(a);
    }
    Ok(0)
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
    let mut newline = true;
    let mut start = 0;
    if args.first() == Some(&"-n") {
        newline = false;
        start = 1;
    }
    let line = args[start..].join(" ");
    if newline {
        println!("{}", line);
    } else {
        print!("{}", line);
        use std::io::Write;
        std::io::stdout().flush().ok();
    }
    Ok(0)
}

fn source(shell: &Rc<RefCell<Shell>>, args: &[&str]) -> Result<i32> {
    let path = args.first().ok_or_else(|| anyhow!("dùng: source <tệp>"))?;
    let content = std::fs::read_to_string(path).map_err(|e| anyhow!("source: {}", e))?;
    crate::config::run_script(shell, &content)?;
    Ok(0)
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
        [n] if n.parse::<usize>().is_ok() => {}
        _ => return Err(anyhow!("history: cách dùng: history [N] | history -c")),
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
/// Redirect đã được RedirectGuard áp vào fd trước khi gọi — process mới kế
/// thừa nguyên (và không bao giờ restore vì exec không quay lại).
/// `exec` không args: no-op (bash giữ shell chạy tiếp).
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
