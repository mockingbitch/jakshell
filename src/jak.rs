use anyhow::Result;
use std::cell::RefCell;
use std::process::Command;
use std::rc::Rc;

use crate::shell::Shell;

// ─── helpers cho lệnh ngoài ──────────────────────────────────────────────────

/// `name` có trên PATH không?
fn has_cmd(name: &str) -> bool {
    which::which(name).is_ok()
}

/// In cảnh báo lệnh thiếu (vàng), kèm gợi ý cài (nếu có).
fn warn_missing(prog: &str, hint: &str) {
    if hint.is_empty() {
        eprintln!("\x1b[33m⚠ `{}` chưa cài trên hệ thống.\x1b[0m", prog);
    } else {
        eprintln!(
            "\x1b[33m⚠ `{}` chưa cài trên hệ thống.\x1b[0m \x1b[2m{}\x1b[0m",
            prog, hint
        );
    }
}

/// Chạy lệnh sau khi check `has_cmd`; trả code (127 nếu thiếu).
fn run_or_warn(prog: &str, args: &[&str], hint: &str) -> i32 {
    if !has_cmd(prog) {
        warn_missing(prog, hint);
        return 127;
    }
    match Command::new(prog).args(args).status() {
        Ok(s) => s.code().unwrap_or(1),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            warn_missing(prog, hint);
            127
        }
        Err(e) => {
            eprintln!("\x1b[31m`{}`: {}\x1b[0m", prog, e);
            126
        }
    }
}

/// Best-effort run cho mục đích "thêm thông tin" — bỏ qua êm nếu lệnh thiếu.
fn try_run(prog: &str, args: &[&str]) {
    if !has_cmd(prog) {
        eprintln!("\x1b[2m  ↳ bỏ qua: `{}` không có trên hệ thống\x1b[0m", prog);
        return;
    }
    let _ = Command::new(prog).args(args).status();
}

pub fn run(shell: &Rc<RefCell<Shell>>, args: &[String]) -> Result<i32> {
    let sub = args.first().map(|s| s.as_str()).unwrap_or("help");
    let rest: Vec<&str> = args.iter().skip(1).map(|s| s.as_str()).collect();
    match sub {
        "help" | "?" => help(),
        "clean" => clean(shell, &rest),
        "backup" => backup(shell, &rest),
        "update" => update(),
        "find" => {
            // Reconstruct argv để khớp signature findcmd::run (skip phần tử [0])
            let mut argv: Vec<String> = vec!["find".to_string()];
            argv.extend(rest.iter().map(|s| s.to_string()));
            crate::findcmd::run(shell, &argv)
        }
        "open" => open(&rest),
        "sysinfo" => sysinfo(),
        "theme" => theme(shell, &rest),
        "weather" => weather(&rest),
        "ip" => ip(),
        _ => {
            eprintln!("jak: tiểu lệnh không rõ '{}'. Gõ `jak help`.", sub);
            Ok(2)
        }
    }
}

fn help() -> Result<i32> {
    println!("\x1b[1mjak — bộ lệnh tiện ích JakShell\x1b[0m\n");
    let items = [
        ("clean", "Xoá file tạm và cache trong ~/.cache, /tmp do bạn sở hữu"),
        ("backup <thư_mục>", "Nén thư mục thành .tar.gz với tên-ngày-giờ"),
        ("update", "Tự dò brew/apt/dnf/pacman và chạy update + upgrade"),
        ("find <tên>", "Tìm file/thư mục (gõ `jak find help` để xem chế độ nâng cao: file/dir/text/big/recent/empty)"),
        ("open <đường_dẫn>", "Mở bằng ứng dụng mặc định (open/xdg-open)"),
        ("sysinfo", "In thông tin máy: OS, CPU, RAM, đĩa"),
        ("theme <tên>", "Đổi giao diện: ocean | forest | sunset | mono | default"),
        ("ip", "In địa chỉ IP nội bộ và public"),
        ("weather [thành phố]", "Xem thời tiết (qua wttr.in)"),
    ];
    for (cmd, desc) in items {
        println!("  \x1b[36m{:24}\x1b[0m {}", cmd, desc);
    }
    Ok(0)
}

fn clean(_shell: &Rc<RefCell<Shell>>, args: &[&str]) -> Result<i32> {
    let dry = args.contains(&"--dry") || args.contains(&"-n");
    let mut total: u64 = 0;
    let mut count: u64 = 0;
    let targets: Vec<std::path::PathBuf> = {
        let mut v = Vec::new();
        if let Some(h) = dirs::home_dir() {
            v.push(h.join(".cache").join("jaksh-tmp"));
            v.push(h.join("Library/Caches")); // mac
            v.push(h.join(".cache"));         // linux
        }
        v.push(std::path::PathBuf::from("/tmp"));
        v
    };
    println!("{}{}", if dry { "[dry-run] " } else { "" }, "đang quét cache…");
    for t in &targets {
        if !t.exists() {
            continue;
        }
        let walk = match std::fs::read_dir(t) {
            Ok(it) => it,
            Err(_) => continue,
        };
        for entry in walk.flatten() {
            let p = entry.path();
            // Only delete entries we own and that look temporary
            let meta = match entry.metadata() {
                Ok(m) => m,
                Err(_) => continue,
            };
            #[cfg(unix)]
            {
                use std::os::unix::fs::MetadataExt;
                if meta.uid() != users_uid() {
                    continue;
                }
            }
            let size = dir_size(&p).unwrap_or(0);
            total += size;
            count += 1;
            if !dry {
                if meta.is_dir() {
                    let _ = std::fs::remove_dir_all(&p);
                } else {
                    let _ = std::fs::remove_file(&p);
                }
            }
        }
    }
    println!(
        "{} {} mục, ~{} MB.",
        if dry { "Sẽ xoá" } else { "Đã xử lý" },
        count,
        total / (1024 * 1024)
    );
    if dry {
        println!("\x1b[2m(thực thi bằng `jak clean` không có cờ --dry)\x1b[0m");
    }
    Ok(0)
}

#[cfg(unix)]
fn users_uid() -> u32 {
    unsafe { libc::getuid() }
}

fn dir_size(p: &std::path::Path) -> Result<u64> {
    let meta = std::fs::metadata(p)?;
    if meta.is_file() {
        return Ok(meta.len());
    }
    let mut total = 0;
    if let Ok(it) = std::fs::read_dir(p) {
        for e in it.flatten() {
            total += dir_size(&e.path()).unwrap_or(0);
        }
    }
    Ok(total)
}

fn backup(shell: &Rc<RefCell<Shell>>, args: &[&str]) -> Result<i32> {
    let src = match args.first() {
        Some(p) => p.to_string(),
        None => {
            eprintln!("dùng: jak backup <thư_mục>");
            return Ok(2);
        }
    };
    let src_path = shell.borrow().cwd.join(&src);
    if !src_path.exists() {
        eprintln!("không tồn tại: {}", src_path.display());
        return Ok(1);
    }
    if !has_cmd("tar") {
        warn_missing("tar", "Cài qua hệ thống package manager rồi thử lại.");
        return Ok(127);
    }
    let now = chrono::Local::now().format("%Y%m%d-%H%M%S");
    let name = src_path.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "backup".into());
    let archive = shell.borrow().cwd.join(format!("{}-{}.tar.gz", name, now));
    println!("đang nén {} → {}", src_path.display(), archive.display());
    let status = Command::new("tar")
        .arg("-czf")
        .arg(&archive)
        .arg("-C")
        .arg(src_path.parent().unwrap_or(std::path::Path::new(".")))
        .arg(src_path.file_name().unwrap())
        .status()?;
    if status.success() {
        println!("\x1b[32m✓ xong:\x1b[0m {}", archive.display());
        Ok(0)
    } else {
        Ok(status.code().unwrap_or(1))
    }
}

fn update() -> Result<i32> {
    // Chỉ liệt kê các package manager phù hợp với OS hiện tại để không phát hiện nhầm.
    let candidates: Vec<(&str, Vec<Vec<&str>>)> = if cfg!(target_os = "macos") {
        vec![
            ("brew", vec![
                vec!["brew", "update"],
                vec!["brew", "upgrade"],
                vec!["brew", "cleanup"],
            ]),
            ("port", vec![
                vec!["sudo", "port", "selfupdate"],
                vec!["sudo", "port", "upgrade", "outdated"],
            ]),
        ]
    } else {
        vec![
            ("apt", vec![
                vec!["sudo", "apt", "update"],
                vec!["sudo", "apt", "upgrade", "-y"],
            ]),
            ("dnf", vec![
                vec!["sudo", "dnf", "upgrade", "--refresh", "-y"],
            ]),
            ("pacman", vec![
                vec!["sudo", "pacman", "-Syu", "--noconfirm"],
            ]),
            ("zypper", vec![
                vec!["sudo", "zypper", "--non-interactive", "update"],
            ]),
            ("apk", vec![
                vec!["sudo", "apk", "update"],
                vec!["sudo", "apk", "upgrade"],
            ]),
            // brew cũng có trên Linux (Linuxbrew)
            ("brew", vec![
                vec!["brew", "update"],
                vec!["brew", "upgrade"],
                vec!["brew", "cleanup"],
            ]),
        ]
    };

    for (mgr, steps) in &candidates {
        if !has_cmd(mgr) {
            continue;
        }
        println!("\x1b[36m▸ phát hiện trình quản lý: {}\x1b[0m", mgr);
        for step in steps {
            println!("\x1b[2m$ {}\x1b[0m", step.join(" "));
            if !has_cmd(step[0]) {
                warn_missing(step[0], "Cần cho lệnh trên — dừng tại đây.");
                return Ok(127);
            }
            let status = Command::new(step[0]).args(&step[1..]).status()?;
            if !status.success() {
                return Ok(status.code().unwrap_or(1));
            }
        }
        return Ok(0);
    }

    let names: Vec<&str> = candidates.iter().map(|(m, _)| *m).collect();
    eprintln!(
        "\x1b[33m⚠ không tìm thấy trình quản lý gói nào trong: {}.\x1b[0m",
        names.join(", ")
    );
    eprintln!("\x1b[2m  Nếu bạn dùng hệ khác (nix, scoop, snap, flatpak…) hãy gõ trực tiếp.\x1b[0m");
    Ok(127)
}

fn open(args: &[&str]) -> Result<i32> {
    let target = args.first().copied().unwrap_or(".");
    let (cmd, hint) = if cfg!(target_os = "macos") {
        ("open", "")
    } else {
        ("xdg-open", "Cài: `sudo apt install xdg-utils` (Debian/Ubuntu) hoặc tương đương.")
    };
    Ok(run_or_warn(cmd, &[target], hint))
}

fn sysinfo() -> Result<i32> {
    let os = std::env::consts::OS;
    let arch = std::env::consts::ARCH;
    println!("\x1b[1mHệ điều hành:\x1b[0m {} ({})", os, arch);
    if cfg!(target_os = "macos") {
        try_run("sw_vers", &[]);
        try_run("sysctl", &["-n", "machdep.cpu.brand_string"]);
        try_run("sysctl", &["-n", "hw.memsize"]);
    } else {
        try_run("uname", &["-a"]);
        try_run("lscpu", &[]);
        try_run("free", &["-h"]);
    }
    println!();
    println!("\x1b[1mDung lượng đĩa:\x1b[0m");
    try_run("df", &["-h"]);
    Ok(0)
}

fn theme(shell: &Rc<RefCell<Shell>>, args: &[&str]) -> Result<i32> {
    let name = args.first().copied().unwrap_or("default");
    let mut t = crate::theme::Theme::default();
    match name {
        "ocean" => { t.accent = "bright_cyan".into(); t.arrow = "❯".into(); }
        "forest" => { t.accent = "bright_green".into(); t.arrow = "→".into(); }
        "sunset" => { t.accent = "bright_magenta".into(); t.arrow = "✦".into(); }
        "mono" => { t.accent = "white".into(); t.use_color = false; t.arrow = ">".into(); }
        "default" => {}
        _ => {
            eprintln!("theme không rõ: {}. có sẵn: ocean | forest | sunset | mono | default", name);
            return Ok(1);
        }
    }
    shell.borrow_mut().theme = t;
    println!("✓ đã đổi sang theme '{}'.", name);
    Ok(0)
}

fn weather(args: &[&str]) -> Result<i32> {
    let city = args.join("+");
    let url = if city.is_empty() {
        "https://wttr.in/?Q&format=3".to_string()
    } else {
        format!("https://wttr.in/{}?Q&format=3", city)
    };
    let hint = "Cần `curl` để lấy thời tiết từ wttr.in.";
    Ok(run_or_warn("curl", &["-s", &url], hint))
}

fn ip() -> Result<i32> {
    println!("\x1b[1mIP nội bộ:\x1b[0m");
    if cfg!(target_os = "macos") {
        // Thử en0 trước, sau đó en1 (laptop có Wi-Fi/Ethernet khác nhau)
        if has_cmd("ipconfig") {
            let _ = Command::new("ipconfig").args(["getifaddr", "en0"]).status();
            let _ = Command::new("ipconfig").args(["getifaddr", "en1"]).status();
        } else {
            warn_missing("ipconfig", "Bất thường trên macOS — `ipconfig` thường có sẵn.");
        }
    } else if has_cmd("hostname") {
        let _ = Command::new("hostname").arg("-I").status();
    } else if has_cmd("ip") {
        // fallback: parse `ip addr` cho dòng inet
        let _ = Command::new("ip").args(["-4", "-o", "addr"]).status();
    } else {
        warn_missing("hostname", "Hoặc cài `iproute2` để có lệnh `ip`.");
    }
    println!("\x1b[1mIP public:\x1b[0m");
    if has_cmd("curl") {
        let _ = Command::new("curl").args(["-s", "https://api.ipify.org"]).status();
        println!();
    } else if has_cmd("wget") {
        let _ = Command::new("wget").args(["-qO-", "https://api.ipify.org"]).status();
        println!();
    } else {
        warn_missing("curl", "Hoặc cài `wget` để lấy IP public.");
    }
    Ok(0)
}
