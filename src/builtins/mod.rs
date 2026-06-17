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
];

pub fn is_builtin(name: &str) -> bool {
    BUILTINS.contains(&name)
}

pub fn run(shell: &Rc<RefCell<Shell>>, argv: &[String], redirects: &[Redirect]) -> Result<i32> {
    let cmd = argv[0].as_str();
    let args: Vec<&str> = argv.iter().skip(1).map(|s| s.as_str()).collect();
    let _guard = RedirectGuard::install(shell, redirects)?;
    flush_std();
    let result = match cmd {
        "cd" => cd(shell, &args),
        "pwd" => pwd(shell),
        "exit" => exit(&args),
        "export" => export(shell, &args),
        "unset" => unset(shell, &args),
        "alias" => alias(shell, &args),
        "unalias" => unalias(shell, &args),
        "echo" => echo(&args),
        "source" | "." => source(shell, &args),
        "history" => history(shell),
        "jobs" => jobs(shell),
        "fg" => fg(shell, &args),
        "bg" => bg(shell, &args),
        "kill" => kill(&args),
        "help" | "?" => help(),
        "which" => which(shell, &args),
        "true" => Ok(0),
        "false" => Ok(1),
        "set" => Ok(0),
        "explain" => crate::explain::run(shell, &argv[1..].to_vec()),
        "bookmark" => crate::bookmark::run(shell, argv),
        "exec" => exec_replace(shell, &args),
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

fn exit(args: &[&str]) -> Result<i32> {
    let code = args.first().and_then(|a| a.parse::<i32>().ok()).unwrap_or(0);
    println!("{} 👋", crate::i18n::t("common.goodbye"));
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

fn history(shell: &Rc<RefCell<Shell>>) -> Result<i32> {
    // Đọc từ lịch sử trong bộ nhớ của phiên (cập nhật theo thời gian thực),
    // KHÔNG đọc file: file chỉ phản ánh lần thoát trước nên sẽ cũ, và còn ở
    // định dạng `#V2` (có header + escape `\n`) không hợp để in trực tiếp.
    let sh = shell.borrow();
    for (i, line) in sh.history.iter().enumerate() {
        println!("{:5}  {}", i + 1, line);
    }
    Ok(0)
}

fn jobs(shell: &Rc<RefCell<Shell>>) -> Result<i32> {
    shell.borrow_mut().reap_jobs();
    let list = shell.borrow().jobs.clone();
    if list.is_empty() {
        println!("(không có job đang chạy)");
        return Ok(0);
    }
    for j in list {
        let state = match j.state {
            crate::shell::JobState::Running => "Running",
            crate::shell::JobState::Stopped => "Stopped",
            crate::shell::JobState::Done => "Done",
        };
        println!("[{}] {} {} {}", j.id, j.pid, state, j.cmd);
    }
    Ok(0)
}

fn fg(shell: &Rc<RefCell<Shell>>, args: &[&str]) -> Result<i32> {
    use nix::sys::wait::{waitpid, WaitStatus};
    use nix::unistd::Pid;
    let id: u32 = args.first().and_then(|s| s.trim_start_matches('%').parse().ok())
        .unwrap_or_else(|| shell.borrow().jobs.last().map(|j| j.id).unwrap_or(0));
    let job = shell.borrow().jobs.iter().find(|j| j.id == id).cloned();
    let job = match job {
        Some(j) => j,
        None => { eprintln!("fg: không có job {}", id); return Ok(1); }
    };
    println!("{}", job.cmd);
    let pid = Pid::from_raw(job.pid);
    // Send SIGCONT and wait.
    let _ = nix::sys::signal::kill(pid, nix::sys::signal::Signal::SIGCONT);
    match waitpid(pid, None) {
        Ok(WaitStatus::Exited(_, code)) => {
            shell.borrow_mut().jobs.retain(|j| j.id != id);
            Ok(code)
        }
        Ok(_) => {
            shell.borrow_mut().jobs.retain(|j| j.id != id);
            Ok(0)
        }
        Err(_) => Ok(1),
    }
}

fn bg(shell: &Rc<RefCell<Shell>>, args: &[&str]) -> Result<i32> {
    let id: u32 = args.first().and_then(|s| s.trim_start_matches('%').parse().ok())
        .unwrap_or_else(|| shell.borrow().jobs.last().map(|j| j.id).unwrap_or(0));
    let pid_opt = shell.borrow().jobs.iter().find(|j| j.id == id).map(|j| j.pid);
    match pid_opt {
        Some(pid) => {
            let _ = nix::sys::signal::kill(nix::unistd::Pid::from_raw(pid), nix::sys::signal::Signal::SIGCONT);
            Ok(0)
        }
        None => { eprintln!("bg: không có job {}", id); Ok(1) }
    }
}

fn kill(args: &[&str]) -> Result<i32> {
    use nix::sys::signal::{kill as nkill, Signal};
    use nix::unistd::Pid;
    if args.is_empty() {
        eprintln!("dùng: kill [-SIGNAL] <pid>");
        return Ok(2);
    }
    let mut sig = Signal::SIGTERM;
    let mut i = 0;
    if args[0].starts_with('-') {
        let name = args[0].trim_start_matches('-');
        sig = match name {
            "9" | "KILL" => Signal::SIGKILL,
            "15" | "TERM" => Signal::SIGTERM,
            "2" | "INT" => Signal::SIGINT,
            "1" | "HUP" => Signal::SIGHUP,
            _ => Signal::SIGTERM,
        };
        i = 1;
    }
    for s in &args[i..] {
        let pid: i32 = s.parse().unwrap_or(0);
        let _ = nkill(Pid::from_raw(pid), sig);
    }
    Ok(0)
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
