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
        "git" => git_shortcut(shell, &rest),
        "self-update" | "upgrade" | "selfupdate" => self_update(shell),
        "version" | "--version" | "-v" | "changelog" | "whatsnew" => version_info(&rest),
        _ => {
            // Thử bookmark
            if crate::bookmark::lookup(sub).is_some() {
                return crate::bookmark::execute(shell, sub, &rest);
            }
            eprintln!(
                "jak: tiểu lệnh / bookmark không rõ '{}'. Gõ `jak help` hoặc `bookmark` để xem.",
                sub
            );
            Ok(2)
        }
    }
}

fn help() -> Result<i32> {
    crate::info::print_banner();
    println!("\x1b[1mjak\x1b[0m \x1b[2m— bộ lệnh tiện ích\x1b[0m\n");
    let items = [
        ("clean", "Xoá file tạm và cache trong ~/.cache, /tmp do bạn sở hữu"),
        ("backup <thư_mục>", "Nén thư mục thành .tar.gz với tên-ngày-giờ"),
        ("update", "Tự dò brew/apt/dnf/pacman và chạy update + upgrade"),
        ("find <tên>", "Tìm file/thư mục (gõ `jak find help` để xem chế độ nâng cao: file/dir/text/big/recent/empty)"),
        ("open <app|path|url>", "Mở app (chrome/vscode/slack/zalo…), file, hay URL. Gõ `jak open list`."),
        ("sysinfo", "In thông tin máy: OS, CPU, RAM, đĩa"),
        ("theme <tên>", "Đổi giao diện (lưu lựa chọn) — gõ `jak theme list` để xem 17 theme"),
        ("ip", "In địa chỉ IP nội bộ và public"),
        ("weather [thành phố]", "Xem thời tiết (qua wttr.in)"),
        ("git <save|sync|undo|wip|...>", "Workflow git: gõ `jak git` để xem chi tiết"),
        ("self-update", "Cập nhật JakShell: git pull + ./install.sh tự động"),
        ("version [all]", "Thông tin phiên bản + CHANGELOG (thêm `all` để xem toàn bộ)"),
    ];
    for (cmd, desc) in items {
        println!("  \x1b[36m{:32}\x1b[0m {}", cmd, desc);
    }

    let bookmarks = crate::bookmark::list_all();
    if !bookmarks.is_empty() {
        println!();
        println!("\x1b[1m▸ Bookmark ({}):\x1b[0m", bookmarks.len());
        let w = bookmarks
            .iter()
            .map(|(n, _)| format!("jak {}", n).len())
            .max()
            .unwrap_or(0)
            .min(32);
        for (name, cmd) in &bookmarks {
            let label = format!("jak {}", name);
            // Cắt ngắn cmd hiển thị để không tràn dòng
            let preview: String = cmd.chars().take(60).collect();
            let suffix = if cmd.len() > 60 { "…" } else { "" };
            println!(
                "  \x1b[36m{:<w$}\x1b[0m \x1b[2m→\x1b[0m {}{}",
                label, preview, suffix, w = w
            );
        }
        println!(
            "\n\x1b[2mQuản lý: `bookmark`, `bookmark <name> <cmd ...>`, `bookmark del <name>`\x1b[0m"
        );
    } else {
        println!();
        println!(
            "\x1b[2m▸ Bookmark: chưa có. Tạo bằng: \x1b[36mbookmark <name> <command ...>\x1b[0m"
        );
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

// ─── jak open ─────────────────────────────────────────────────────────────────

/// Mapping: (alias chuẩn-hoá lowercase, tên app macOS, binary trên Linux)
/// Để rỗng nếu không có trên platform tương ứng.
const APP_ALIASES: &[(&str, &str, &str)] = &[
    // ── Browser ──
    ("chrome",       "Google Chrome",         "google-chrome"),
    ("googlechrome", "Google Chrome",         "google-chrome"),
    ("firefox",      "Firefox",               "firefox"),
    ("safari",       "Safari",                ""),
    ("edge",         "Microsoft Edge",        "microsoft-edge"),
    ("brave",        "Brave Browser",         "brave-browser"),
    ("opera",        "Opera",                 "opera"),
    ("arc",          "Arc",                   ""),
    ("zen",          "Zen Browser",           "zen-browser"),

    // ── Editor / IDE ──
    ("code",         "Visual Studio Code",    "code"),
    ("vscode",       "Visual Studio Code",    "code"),
    ("visualcode",   "Visual Studio Code",    "code"),
    ("visualstudiocode", "Visual Studio Code","code"),
    ("cursor",       "Cursor",                "cursor"),
    ("sublime",      "Sublime Text",          "subl"),
    ("subl",         "Sublime Text",          "subl"),
    ("atom",         "Atom",                  "atom"),
    ("intellij",     "IntelliJ IDEA",         "idea"),
    ("idea",         "IntelliJ IDEA",         "idea"),
    ("pycharm",      "PyCharm",               "pycharm"),
    ("webstorm",     "WebStorm",              "webstorm"),
    ("goland",       "GoLand",                "goland"),
    ("rider",        "Rider",                 "rider"),
    ("phpstorm",     "PhpStorm",              "phpstorm"),
    ("clion",        "CLion",                 "clion"),
    ("xcode",        "Xcode",                 ""),
    ("androidstudio","Android Studio",        "android-studio"),
    ("zed",          "Zed",                   "zed"),
    ("nova",         "Nova",                  ""),

    // ── Terminal ──
    ("terminal",     "Terminal",              "gnome-terminal"),
    ("iterm",        "iTerm",                 ""),
    ("iterm2",       "iTerm",                 ""),
    ("warp",         "Warp",                  "warp-terminal"),
    ("alacritty",    "Alacritty",             "alacritty"),
    ("kitty",        "Kitty",                 "kitty"),
    ("hyper",        "Hyper",                 "hyper"),
    ("wezterm",      "WezTerm",               "wezterm"),
    ("tabby",        "Tabby",                 "tabby"),

    // ── Chat / Communication ──
    ("slack",        "Slack",                 "slack"),
    ("discord",      "Discord",               "discord"),
    ("zoom",         "zoom.us",               "zoom"),
    ("teams",        "Microsoft Teams",       "teams"),
    ("telegram",     "Telegram",              "telegram-desktop"),
    ("whatsapp",     "WhatsApp",              "whatsapp"),
    ("zalo",         "Zalo",                  "zalo"),
    ("signal",       "Signal",                "signal-desktop"),
    ("messenger",    "Messenger",             ""),
    ("skype",        "Skype",                 "skype"),

    // ── Media / Entertainment ──
    ("spotify",      "Spotify",               "spotify"),
    ("vlc",          "VLC",                   "vlc"),

    // ── Productivity / Notes ──
    ("notion",       "Notion",                "notion-app"),
    ("obsidian",     "Obsidian",              "obsidian"),
    ("evernote",     "Evernote",              "evernote"),
    ("todoist",      "Todoist",               "todoist"),
    ("things",       "Things",                ""),

    // ── Design ──
    ("figma",        "Figma",                 "figma-linux"),
    ("sketch",       "Sketch",                ""),
    ("photoshop",    "Adobe Photoshop 2024",  ""),
    ("illustrator",  "Adobe Illustrator 2024",""),
    ("xd",           "Adobe XD",              ""),
    ("affinity",     "Affinity Designer",     ""),
    ("canva",        "Canva",                 "canva"),

    // ── Dev tools ──
    ("postman",      "Postman",               "postman"),
    ("insomnia",     "Insomnia",              "insomnia"),
    ("docker",       "Docker",                "docker-desktop"),
    ("dockerdesktop","Docker",                "docker-desktop"),
    ("githubdesktop","GitHub Desktop",        "github-desktop"),
    ("sourcetree",   "Sourcetree",            ""),
    ("gitkraken",    "GitKraken",             "gitkraken"),
    ("tableplus",    "TablePlus",             ""),
    ("sequel",       "Sequel Ace",            ""),
    ("dbeaver",      "DBeaver",               "dbeaver"),
    ("ngrok",        "ngrok",                 "ngrok"),

    // ── System / macOS apps ──
    ("finder",       "Finder",                ""),
    ("files",        "",                      "nautilus"),
    ("preview",      "Preview",               ""),
    ("calculator",   "Calculator",            "gnome-calculator"),
    ("calc",         "Calculator",            "gnome-calculator"),
    ("settings",     "System Settings",       "gnome-control-center"),
    ("preferences",  "System Settings",       "gnome-control-center"),
    ("activity",     "Activity Monitor",      "gnome-system-monitor"),
    ("monitor",      "Activity Monitor",      "gnome-system-monitor"),
    ("appstore",     "App Store",             ""),
    ("disk",         "Disk Utility",          "gnome-disks"),
    ("keychain",     "Keychain Access",       ""),

    // ── macOS built-ins ──
    ("mail",         "Mail",                  "thunderbird"),
    ("calendar",     "Calendar",              "gnome-calendar"),
    ("notes",        "Notes",                 ""),
    ("messages",     "Messages",              ""),
    ("music",        "Music",                 ""),
    ("photos",       "Photos",                "gnome-photos"),
    ("maps",         "Maps",                  ""),
    ("reminders",    "Reminders",             ""),

    // ── Cloud / Storage ──
    ("dropbox",      "Dropbox",               "dropbox"),
    ("drive",        "Google Drive",          ""),

    // ── Misc ──
    ("rectangle",    "Rectangle",             ""),
    ("raycast",      "Raycast",               ""),
    ("alfred",       "Alfred",                ""),
    ("magnet",       "Magnet",                ""),
    ("1password",    "1Password",             "1password"),
    ("bitwarden",    "Bitwarden",             "bitwarden"),
];

/// URL "shortcut" — gõ `jak open <key>` thì mở URL trong browser.
const URL_ALIASES: &[(&str, &str)] = &[
    ("youtube",   "https://www.youtube.com"),
    ("netflix",   "https://www.netflix.com"),
    ("gmail",     "https://mail.google.com"),
    ("github",    "https://github.com"),
    ("gitlab",    "https://gitlab.com"),
    ("stackoverflow", "https://stackoverflow.com"),
    ("chatgpt",   "https://chat.openai.com"),
    ("claude",    "https://claude.ai"),
    ("gemini",    "https://gemini.google.com"),
    ("translate", "https://translate.google.com"),
];

fn open(args: &[&str]) -> Result<i32> {
    let first = args.first().copied().unwrap_or(".");
    match first {
        "help" | "?" | "--help" | "-h" => { open_help(); return Ok(0); }
        "list" | "apps" => { open_list(); return Ok(0); }
        _ => {}
    }

    let key = first.to_lowercase().replace([' ', '-', '_'], "");
    let rest: Vec<&str> = args.iter().skip(1).copied().collect();

    // 1) Khớp app alias?
    if let Some((_, mac_app, linux_bin)) = APP_ALIASES.iter().find(|(k, _, _)| *k == key) {
        return open_app(mac_app, linux_bin, &rest, first);
    }
    // 2) Khớp URL alias?
    if let Some((_, url)) = URL_ALIASES.iter().find(|(k, _)| *k == key) {
        return open_url(url);
    }
    // 3) File / path / URL — đẩy cho open / xdg-open xử lý
    open_default(args)
}

fn open_help() {
    println!("\x1b[1mjak open — mở app, file, hoặc URL\x1b[0m\n");
    let items: &[(&str, &str)] = &[
        ("jak open <app>",     "mở app theo alias (vd: chrome, vscode, slack, zalo, figma)"),
        ("jak open <app> <file>", "mở file BẰNG app (macOS: open -a <App> <file>)"),
        ("jak open <url>",     "mở URL trong browser mặc định"),
        ("jak open <path>",    "mở file / thư mục bằng app mặc định của OS"),
        ("jak open list",      "liệt kê toàn bộ alias app có sẵn"),
        ("jak open .",         "mở thư mục hiện tại bằng Finder / file manager"),
    ];
    for (cmd, desc) in items {
        println!("  \x1b[36m{:24}\x1b[0m {}", cmd, desc);
    }
    println!("\n\x1b[2mKhông tìm thấy alias? → tự fallback sang `open` (macOS) / `xdg-open` (Linux).\x1b[0m");
}

fn open_list() {
    println!("\x1b[1mApp alias ({}):\x1b[0m", APP_ALIASES.len());
    let cols = 4usize;
    let w = APP_ALIASES.iter().map(|(k, _, _)| k.len()).max().unwrap_or(0);
    for chunk in APP_ALIASES.chunks(cols) {
        for (k, _, _) in chunk {
            print!("  \x1b[36m{:<w$}\x1b[0m", k, w = w);
        }
        println!();
    }
    println!("\n\x1b[1mURL alias ({}):\x1b[0m", URL_ALIASES.len());
    for (k, url) in URL_ALIASES {
        println!("  \x1b[36m{:<14}\x1b[0m \x1b[2m→\x1b[0m {}", k, url);
    }
    println!("\n\x1b[2mGõ: jak open <tên>\x1b[0m");
}

#[cfg(target_os = "macos")]
fn open_app(mac_app: &str, _linux_bin: &str, extra: &[&str], requested: &str) -> Result<i32> {
    if mac_app.is_empty() {
        eprintln!("'{}' không có trên macOS.", requested);
        return Ok(1);
    }
    let mut args: Vec<&str> = vec!["-a", mac_app];
    args.extend(extra);
    println!("\x1b[2m$ open -a \"{}\"{}\x1b[0m",
        mac_app,
        if extra.is_empty() { String::new() } else { format!(" {}", extra.join(" ")) }
    );
    Ok(run_or_warn("open", &args, ""))
}

#[cfg(not(target_os = "macos"))]
fn open_app(_mac_app: &str, linux_bin: &str, extra: &[&str], requested: &str) -> Result<i32> {
    if linux_bin.is_empty() {
        eprintln!("'{}' không có alias Linux. Thử gõ binary trực tiếp.", requested);
        return Ok(1);
    }
    if !has_cmd(linux_bin) {
        warn_missing(linux_bin, "App này chưa được cài hoặc không có trên PATH.");
        return Ok(127);
    }
    spawn_detached_ok(linux_bin, extra)
}

fn open_url(url: &str) -> Result<i32> {
    if cfg!(target_os = "macos") {
        println!("\x1b[2m$ open {}\x1b[0m", url);
        Ok(run_or_warn("open", &[url], ""))
    } else {
        println!("\x1b[2m$ xdg-open {}\x1b[0m", url);
        Ok(run_or_warn("xdg-open", &[url], "Cài: sudo apt install xdg-utils"))
    }
}

fn open_default(args: &[&str]) -> Result<i32> {
    let (cmd, hint) = if cfg!(target_os = "macos") {
        ("open", "")
    } else {
        ("xdg-open", "Cài: `sudo apt install xdg-utils` (Debian/Ubuntu) hoặc tương đương.")
    };
    Ok(run_or_warn(cmd, args, hint))
}

/// Spawn lệnh GUI Linux dạng detached — không block shell, đóng stdin/out/err.
#[cfg(not(target_os = "macos"))]
fn spawn_detached_ok(prog: &str, args: &[&str]) -> Result<i32> {
    use std::process::Stdio;
    println!("\x1b[2m$ {} {} &\x1b[0m", prog, args.join(" "));
    match Command::new(prog)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(child) => {
            let pid = child.id();
            std::mem::forget(child);
            println!("\x1b[2m✓ đã khởi động (pid {})\x1b[0m", pid);
            Ok(0)
        }
        Err(e) => {
            eprintln!("\x1b[31mkhông chạy được {}: {}\x1b[0m", prog, e);
            Ok(126)
        }
    }
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
    let sub = args.first().copied().unwrap_or("show");
    match sub {
        "list" | "ls" => theme_list(),
        "show" | "current" => theme_show(shell),
        "reset" | "default" => theme_reset(shell),
        "help" | "?" | "--help" | "-h" => {
            theme_help();
            Ok(0)
        }
        name => theme_apply(shell, name, true),
    }
}

fn theme_help() {
    println!("\x1b[1mjak theme — quản lý giao diện\x1b[0m\n");
    let items: &[(&str, &str)] = &[
        ("jak theme",              "= jak theme show — in theme hiện tại"),
        ("jak theme list",         "liệt kê tất cả theme có sẵn (kèm preview)"),
        ("jak theme <name>",       "đổi sang theme & LƯU lựa chọn cho lần mở sau"),
        ("jak theme reset",        "xoá lựa chọn lưu, dùng theme từ ~/.jakshrc.toml (hoặc default)"),
    ];
    for (cmd, desc) in items {
        println!("  \x1b[36m{:24}\x1b[0m {}", cmd, desc);
    }
    println!(
        "\n\x1b[2mLưu tại: ~/.config/jaksh/theme  (xoá file này = reset)\x1b[0m"
    );
}

fn theme_list() -> Result<i32> {
    let saved = saved_theme_name();
    println!("\x1b[1mTheme có sẵn ({}):\x1b[0m", crate::theme::BUILTIN_NAMES.len());
    let w = crate::theme::BUILTIN_NAMES.iter().map(|n| n.len()).max().unwrap_or(0);
    for name in crate::theme::BUILTIN_NAMES {
        let t = match crate::theme::by_name(name) {
            Some(t) => t,
            None => continue,
        };
        let accent = t.accent_ansi();
        let dim = t.dim_ansi();
        let reset = if t.use_color { "\x1b[0m" } else { "" };
        let marker = if saved.as_deref() == Some(*name) { "● " }
                     else { "  " };
        // Preview: tên theo accent + mũi tên + tên thư mục giả
        let preview = format!(
            "{accent}{name:<w$}{reset}  {dim}~/code{reset} {accent}{arrow}{reset}",
            accent = accent,
            name = name,
            reset = reset,
            dim = dim,
            arrow = t.arrow,
            w = w,
        );
        println!("{marker}{preview}  \x1b[2m{}\x1b[0m", crate::theme::describe(name));
    }
    println!();
    if let Some(s) = saved {
        println!("\x1b[2m● = đã lưu trong ~/.config/jaksh/theme  (hiện: {})\x1b[0m", s);
    } else {
        println!("\x1b[2m(chưa có theme lưu — dùng `jak theme <name>` để lưu)\x1b[0m");
    }
    Ok(0)
}

fn theme_show(shell: &Rc<RefCell<Shell>>) -> Result<i32> {
    let t = &shell.borrow().theme;
    let accent = t.accent_ansi();
    let reset = if t.use_color { "\x1b[0m" } else { "" };
    println!("Theme hiện tại (in-memory):");
    println!("  accent           = {accent}{}{reset}", t.accent);
    println!("  success          = {}", t.success);
    println!("  error            = {}", t.error);
    println!("  dim              = {}", t.dim);
    println!("  arrow            = {accent}{}{reset}", t.arrow);
    println!("  git_branch_icon  = '{}'", t.git_branch_icon);
    println!("  use_color        = {}", t.use_color);
    match saved_theme_name() {
        Some(name) => println!("\nLưu trên đĩa: \x1b[36m{}\x1b[0m  ({})", name, theme_file().display()),
        None => println!("\n\x1b[2m(không có lựa chọn lưu trên đĩa)\x1b[0m"),
    }
    Ok(0)
}

fn theme_apply(shell: &Rc<RefCell<Shell>>, name: &str, save: bool) -> Result<i32> {
    let t = match crate::theme::by_name(name) {
        Some(t) => t,
        None => {
            eprintln!(
                "theme không rõ: '{}'. Gõ `jak theme list` để xem danh sách.",
                name
            );
            return Ok(1);
        }
    };
    let accent = t.accent_ansi();
    let arrow = t.arrow.clone();
    let reset = if t.use_color { "\x1b[0m" } else { "" };
    shell.borrow_mut().theme = t;
    if save {
        match save_theme_name(name) {
            Ok(_) => println!(
                "\x1b[32m✓\x1b[0m đã đổi sang theme {accent}{name}{reset} {accent}{arrow}{reset}  \x1b[2m(đã lưu)\x1b[0m"
            ),
            Err(e) => println!(
                "\x1b[32m✓\x1b[0m đã đổi sang theme '{name}' \x1b[33m(không lưu được: {e})\x1b[0m"
            ),
        }
    } else {
        println!("\x1b[32m✓\x1b[0m đã đổi sang theme {accent}{name}{reset}");
    }
    Ok(0)
}

fn theme_reset(shell: &Rc<RefCell<Shell>>) -> Result<i32> {
    let path = theme_file();
    if path.exists() {
        if let Err(e) = std::fs::remove_file(&path) {
            eprintln!("\x1b[33m⚠ không xoá được {}: {}\x1b[0m", path.display(), e);
            return Ok(1);
        }
        println!("\x1b[32m✓\x1b[0m đã xoá theme đã lưu. Lần sau mở shell sẽ dùng theme từ ~/.jakshrc.toml hoặc default.");
    } else {
        println!("\x1b[2m(không có theme đã lưu — không cần xoá)\x1b[0m");
    }
    // Áp default ngay
    shell.borrow_mut().theme = crate::theme::Theme::default();
    println!("In-memory đã đổi về \x1b[36mdefault\x1b[0m.");
    Ok(0)
}

// ─── Lưu / đọc theme đã chọn ──────────────────────────────────────────────────

pub fn theme_file() -> std::path::PathBuf {
    dirs::home_dir()
        .map(|h| h.join(".config").join("jaksh").join("theme"))
        .unwrap_or_else(|| std::path::PathBuf::from("theme"))
}

pub fn saved_theme_name() -> Option<String> {
    let content = std::fs::read_to_string(theme_file()).ok()?;
    let name = content.trim().to_string();
    if name.is_empty() { None } else { Some(name) }
}

fn save_theme_name(name: &str) -> Result<()> {
    let path = theme_file();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    std::fs::write(&path, name)?;
    Ok(())
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

// ─── jak self-update ──────────────────────────────────────────────────────────

fn source_path_file() -> std::path::PathBuf {
    dirs::home_dir()
        .map(|h| h.join(".config").join("jaksh").join("source-path"))
        .unwrap_or_else(|| std::path::PathBuf::from("source-path"))
}

fn self_update(_shell: &Rc<RefCell<Shell>>) -> Result<i32> {
    let path_file = source_path_file();
    let source = match std::fs::read_to_string(&path_file) {
        Ok(s) => s.trim().to_string(),
        Err(_) => {
            print_manual_update_help(None);
            return Ok(1);
        }
    };
    if source.is_empty() {
        print_manual_update_help(None);
        return Ok(1);
    }
    let path = std::path::Path::new(&source);
    if !path.exists() {
        print_manual_update_help(Some(&format!(
            "thư mục source đã ghi không còn tồn tại: {}",
            source
        )));
        return Ok(1);
    }
    if !path.join(".git").exists() {
        print_manual_update_help(Some(&format!(
            "{} không phải git repo (đã bị xoá .git?)",
            source
        )));
        return Ok(1);
    }

    println!("\x1b[1m▸ Cập nhật JakShell\x1b[0m");
    println!("  \x1b[2msource:\x1b[0m {}", source);
    println!();

    // 1) git pull --rebase
    println!("\x1b[2m$ git -C {} fetch --tags\x1b[0m", source);
    if !has_cmd("git") {
        warn_missing("git", "Cần git để self-update.");
        return Ok(127);
    }
    let _ = Command::new("git").args(["-C", &source, "fetch", "--tags"]).status();

    println!("\x1b[2m$ git -C {} pull --rebase\x1b[0m", source);
    let s1 = Command::new("git")
        .args(["-C", &source, "pull", "--rebase"])
        .status()?;
    if !s1.success() {
        eprintln!(
            "\x1b[33m⚠ git pull thất bại. Fix conflict / commit local rồi thử lại.\x1b[0m"
        );
        return Ok(s1.code().unwrap_or(1));
    }

    // 2) Chạy install.sh
    let install = path.join("install.sh");
    if !install.exists() {
        eprintln!("\x1b[31m✗ không thấy install.sh trong source — cài bằng tay.\x1b[0m");
        return Ok(1);
    }
    println!("\n\x1b[2m$ ./install.sh --yes\x1b[0m");
    let s2 = Command::new("bash")
        .arg(&install)
        .arg("--yes")
        .current_dir(path)
        .status()?;
    if s2.success() {
        println!();
        let old_version = env!("JAKSH_VERSION");
        let new_version = read_new_version(&source);
        match new_version {
            Some(ref nv) if nv != old_version => {
                println!(
                    "\x1b[32m✓\x1b[0m Đã cập nhật: \x1b[2m{}\x1b[0m → \x1b[1m\x1b[32m{}\x1b[0m",
                    old_version, nv
                );
            }
            _ => println!("\x1b[32m✓\x1b[0m Đã cài lại JakShell (cùng version)."),
        }
        // In phần CHANGELOG của bản mới (đọc từ source repo vừa pull)
        let changelog_path = path.join("CHANGELOG.md");
        if let Ok(content) = std::fs::read_to_string(&changelog_path) {
            print_latest_changelog(&content);
        }
        println!(
            "\n\x1b[2mMở terminal mới để dùng bản mới.\x1b[0m"
        );
    }
    Ok(s2.code().unwrap_or(0))
}

/// Đọc version mới từ source repo (sau khi pull) bằng `git describe`.
fn read_new_version(source: &str) -> Option<String> {
    let out = Command::new("git")
        .args(["-C", source, "describe", "--tags", "--always", "--dirty=-dirty"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8(out.stdout).ok()?;
    let trimmed = s.trim();
    if trimmed.is_empty() { None } else { Some(trimmed.to_string()) }
}

/// In phần đầu (section mới nhất) của CHANGELOG.md.
fn print_latest_changelog(content: &str) {
    let lines: Vec<&str> = content.lines().collect();
    let mut start: Option<usize> = None;
    let mut end: usize = lines.len();
    for (i, l) in lines.iter().enumerate() {
        if l.starts_with("## ") {
            if start.is_none() {
                start = Some(i);
            } else {
                end = i;
                break;
            }
        }
    }
    let Some(s) = start else { return };
    println!();
    println!("\x1b[1m📝 Có gì mới trong bản này:\x1b[0m");
    println!();
    for line in &lines[s..end] {
        println!("{}", line);
    }
}

// ─── jak version / jak changelog ──────────────────────────────────────────────

/// CHANGELOG được nhúng vào binary lúc compile — không cần source repo để xem.
const EMBEDDED_CHANGELOG: &str = include_str!("../CHANGELOG.md");

fn version_info(args: &[&str]) -> Result<i32> {
    let bold = "\x1b[1m";
    let cyan = "\x1b[36m";
    let bright_cyan = "\x1b[96m";
    let yellow = "\x1b[33m";
    let dim = "\x1b[2m";
    let reset = "\x1b[0m";

    // ─── Khối thông tin version ───
    println!(
        "{bold}{bc}JakShell{reset}  {cyan}{ver}{reset}",
        bc = bright_cyan,
        ver = env!("JAKSH_VERSION")
    );
    println!();
    println!("  {dim}Commit:{reset}      {}", env!("JAKSH_COMMIT_HASH"));
    println!("  {dim}Commit date:{reset} {}", env!("JAKSH_COMMIT_DATE"));
    println!("  {dim}Built:{reset}       {}", env!("JAKSH_BUILD_DATE"));
    println!("  {dim}Rust:{reset}        {}", env!("JAKSH_RUSTC"));
    println!(
        "  {dim}Target:{reset}      {}-{}",
        std::env::consts::ARCH,
        std::env::consts::OS
    );
    println!();
    println!("  {dim}Author:{reset}      {bold}Jarvis Phong Tran{reset}");
    println!("  {dim}Repo:{reset}        https://github.com/mockingbitch/jakshell");
    println!(
        "  {dim}Update:{reset}      {y}jak self-update{reset}",
        y = yellow
    );

    // ─── Khối CHANGELOG ───
    let show_all = args.first().map(|s| *s == "all" || *s == "full").unwrap_or(false);
    println!();
    println!("{dim}{}{reset}", "─".repeat(56));
    if show_all {
        print!("{}", EMBEDDED_CHANGELOG);
    } else {
        print_latest_changelog(EMBEDDED_CHANGELOG);
        println!();
        println!("{dim}(gõ `jak version all` để xem toàn bộ CHANGELOG){reset}");
    }
    Ok(0)
}

fn print_manual_update_help(reason: Option<&str>) {
    eprintln!("\x1b[33m⚠ self-update không sẵn sàng.\x1b[0m");
    if let Some(r) = reason {
        eprintln!("  \x1b[2mLý do:\x1b[0m {}", r);
    } else {
        eprintln!(
            "  \x1b[2mLý do:\x1b[0m chưa có ~/.config/jaksh/source-path (cài bằng cách khác?)."
        );
    }
    eprintln!("\nCập nhật thủ công:");
    eprintln!("  \x1b[36mcd <thư-mục-jakshell-đã-clone>\x1b[0m");
    eprintln!("  \x1b[36mgit pull --rebase\x1b[0m");
    eprintln!("  \x1b[36m./install.sh\x1b[0m");
    eprintln!(
        "\nHoặc clone mới:"
    );
    eprintln!(
        "  \x1b[36mgit clone https://github.com/mockingbitch/jakshell.git\x1b[0m"
    );
    eprintln!("  \x1b[36mcd jakshell && ./install.sh\x1b[0m");
}

// ─── jak git shortcuts ────────────────────────────────────────────────────────

fn git_shortcut(shell: &Rc<RefCell<Shell>>, args: &[&str]) -> Result<i32> {
    if !has_cmd("git") {
        warn_missing("git", "Cài git rồi thử lại.");
        return Ok(127);
    }
    let sub = args.first().copied().unwrap_or("help");
    let rest: &[&str] = if args.is_empty() { &[] } else { &args[1..] };
    match sub {
        "help" | "?" | "--help" | "-h" => {
            git_help();
            Ok(0)
        }
        "save" => git_save(shell, rest),
        "sync" => git_sync(shell),
        "undo" => git_undo(shell),
        "wip" => git_wip(shell),
        "amend" => git_amend(shell),
        "unstage" => git_unstage(shell, rest),
        "clean-branches" | "prune-branches" => git_clean_branches(shell),
        "uncommit" => git_uncommit(shell),
        "publish" => git_publish(shell, rest),
        _ => {
            eprintln!("jak git: tiểu lệnh không rõ '{}'. Gõ `jak git help`.", sub);
            Ok(2)
        }
    }
}

fn git_help() {
    println!("\x1b[1mjak git — workflow tắt cho git\x1b[0m\n");
    let items: &[(&str, &str)] = &[
        ("jak git save \"<msg>\"",      "git add -A && git commit -m '<msg>' (lưu tất cả thay đổi)"),
        ("jak git wip",                "= save 'WIP' (commit tạm để chuyển nhánh)"),
        ("jak git sync",               "git pull --rebase + git push (đồng bộ với remote)"),
        ("jak git publish [<branch>]", "tạo upstream + push lần đầu cho branch hiện tại"),
        ("jak git amend",              "git commit --amend --no-edit (gộp staged vào commit cuối)"),
        ("jak git uncommit",           "git reset --soft HEAD~1 (huỷ commit cuối, giữ staged)"),
        ("jak git undo",               "git restore --staged . (huỷ stage; file vẫn còn)"),
        ("jak git unstage <file>",     "= git restore --staged <file>"),
        ("jak git clean-branches",     "xoá branch local đã merge vào main/master"),
    ];
    for (cmd, desc) in items {
        println!("  \x1b[36m{:32}\x1b[0m {}", cmd, desc);
    }
    println!("\n\x1b[2mLưu ý: mọi lệnh đều in `$ git ...` trước khi chạy để bạn biết nó làm gì.\x1b[0m");
}

/// In trước rồi chạy. Trả exit code.
fn git_step(args: &[&str], cwd: &std::path::Path) -> i32 {
    println!("\x1b[2m$ git {}\x1b[0m", args.join(" "));
    match Command::new("git").args(args).current_dir(cwd).status() {
        Ok(s) => s.code().unwrap_or(1),
        Err(e) => {
            eprintln!("\x1b[31mjak git: lỗi `git {}`: {}\x1b[0m", args[0], e);
            127
        }
    }
}

fn git_save(shell: &Rc<RefCell<Shell>>, rest: &[&str]) -> Result<i32> {
    if rest.is_empty() {
        eprintln!("dùng: jak git save \"<commit message>\"");
        return Ok(2);
    }
    let msg = rest.join(" ");
    let cwd = shell.borrow().cwd.clone();
    let code = git_step(&["add", "-A"], &cwd);
    if code != 0 {
        return Ok(code);
    }
    Ok(git_step(&["commit", "-m", &msg], &cwd))
}

fn git_wip(shell: &Rc<RefCell<Shell>>) -> Result<i32> {
    let cwd = shell.borrow().cwd.clone();
    let code = git_step(&["add", "-A"], &cwd);
    if code != 0 {
        return Ok(code);
    }
    Ok(git_step(&["commit", "-m", "WIP"], &cwd))
}

fn git_sync(shell: &Rc<RefCell<Shell>>) -> Result<i32> {
    let cwd = shell.borrow().cwd.clone();
    let code = git_step(&["pull", "--rebase"], &cwd);
    if code != 0 {
        eprintln!("\x1b[33m⚠ pull --rebase thất bại — fix conflict rồi `git rebase --continue` (hoặc `git rebase --abort`).\x1b[0m");
        return Ok(code);
    }
    Ok(git_step(&["push"], &cwd))
}

fn git_publish(shell: &Rc<RefCell<Shell>>, rest: &[&str]) -> Result<i32> {
    let cwd = shell.borrow().cwd.clone();
    let branch = if let Some(b) = rest.first() {
        b.to_string()
    } else {
        // Lấy branch hiện tại
        let out = Command::new("git")
            .args(["branch", "--show-current"])
            .current_dir(&cwd)
            .output()?;
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };
    if branch.is_empty() {
        eprintln!("không xác định được branch — đang detached?");
        return Ok(1);
    }
    Ok(git_step(&["push", "-u", "origin", &branch], &cwd))
}

fn git_amend(shell: &Rc<RefCell<Shell>>) -> Result<i32> {
    let cwd = shell.borrow().cwd.clone();
    Ok(git_step(&["commit", "--amend", "--no-edit"], &cwd))
}

fn git_uncommit(shell: &Rc<RefCell<Shell>>) -> Result<i32> {
    let cwd = shell.borrow().cwd.clone();
    println!("\x1b[2m(huỷ commit cuối, giữ nguyên thay đổi trong staging)\x1b[0m");
    Ok(git_step(&["reset", "--soft", "HEAD~1"], &cwd))
}

fn git_undo(shell: &Rc<RefCell<Shell>>) -> Result<i32> {
    let cwd = shell.borrow().cwd.clone();
    println!("\x1b[2m(huỷ stage cho tất cả file — nội dung file không đổi)\x1b[0m");
    Ok(git_step(&["restore", "--staged", "."], &cwd))
}

fn git_unstage(shell: &Rc<RefCell<Shell>>, rest: &[&str]) -> Result<i32> {
    if rest.is_empty() {
        eprintln!("dùng: jak git unstage <file ...>");
        return Ok(2);
    }
    let cwd = shell.borrow().cwd.clone();
    let mut args: Vec<&str> = vec!["restore", "--staged"];
    args.extend(rest);
    Ok(git_step(&args, &cwd))
}

fn git_clean_branches(shell: &Rc<RefCell<Shell>>) -> Result<i32> {
    let cwd = shell.borrow().cwd.clone();
    // Liệt kê branch đã merged (trừ branch hiện tại và main/master)
    let output = Command::new("git")
        .args(["branch", "--merged"])
        .current_dir(&cwd)
        .output()?;
    if !output.status.success() {
        return Ok(output.status.code().unwrap_or(1));
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let candidates: Vec<String> = text
        .lines()
        .map(|l| l.trim_start_matches('*').trim().to_string())
        .filter(|b| !b.is_empty()
            && b != "main"
            && b != "master"
            && b != "develop"
            && !b.starts_with('(')) // (HEAD detached at ...)
        .collect();

    // Bỏ branch hiện tại (có dấu '*' trong output)
    let current: String = text
        .lines()
        .find(|l| l.starts_with('*'))
        .map(|l| l.trim_start_matches('*').trim().to_string())
        .unwrap_or_default();
    let to_delete: Vec<&String> = candidates.iter().filter(|b| **b != current).collect();

    if to_delete.is_empty() {
        println!("\x1b[2m(không có branch nào đã merged sẵn sàng để xoá)\x1b[0m");
        return Ok(0);
    }
    println!("Sẽ xoá {} branch đã merged:", to_delete.len());
    for b in &to_delete {
        println!("  - {}", b);
    }
    print!("Xác nhận xoá? [y/N] ");
    use std::io::Write;
    std::io::stdout().flush().ok();
    let mut line = String::new();
    std::io::stdin().read_line(&mut line).ok();
    if !line.trim().eq_ignore_ascii_case("y") {
        println!("\x1b[2mđã huỷ\x1b[0m");
        return Ok(0);
    }
    for b in to_delete {
        let _ = git_step(&["branch", "-d", b], &cwd);
    }
    Ok(0)
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
