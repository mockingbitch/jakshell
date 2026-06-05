//! Module thực thi cho `jak find`. Cú pháp:
//!   jak find                           — in help
//!   jak find <tên>                     — viết tắt của `jak find file <tên>`
//!   jak find file <pattern> [in <p>]   — tìm file theo tên
//!   jak find dir  <pattern> [in <p>]   — tìm thư mục
//!   jak find text "<chuỗi>" [in <p>]   — tìm trong nội dung file (grep)
//!   jak find big  [in <p>]             — 20 file lớn nhất
//!   jak find recent [in <p>]           — file sửa trong 24h
//!   jak find empty [in <p>]            — file rỗng
//!
//! `<pattern>` hỗ trợ glob (* ? [abc]) hoặc substring case-insensitive.
//! `in` có thể viết là `trong`. Đường dẫn hỗ trợ `~`.

use anyhow::{anyhow, Result};
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::rc::Rc;

use crate::shell::Shell;

#[allow(dead_code)]
const _MODULE_DOC: &str = "implementation backend for `jak find`";

const DIM: &str = "\x1b[2m";
const RESET: &str = "\x1b[0m";
const BLUE: &str = "\x1b[94m";
const CYAN: &str = "\x1b[36m";
const YELLOW: &str = "\x1b[33m";
const GREEN: &str = "\x1b[32m";

/// Các tên thư mục bỏ qua khi đi đệ quy (vì lớn / tự sinh).
const IGNORE_DIRS: &[&str] = &[
    ".git", "node_modules", "target", "venv", ".venv", "__pycache__",
    ".next", ".nuxt", "dist", "build", ".gradle", ".idea", ".vscode",
];

pub fn run(shell: &Rc<RefCell<Shell>>, argv: &[String]) -> Result<i32> {
    let args: Vec<&str> = argv.iter().skip(1).map(|s| s.as_str()).collect();
    if args.is_empty() {
        print_help();
        return Ok(0);
    }
    match args[0] {
        "file" | "files" | "f" => find_by_name(shell, &args, Kind::File),
        "dir" | "dirs" | "folder" | "folders" | "directory" | "d" => find_by_name(shell, &args, Kind::Dir),
        "text" | "string" | "content" | "contents" | "grep" => find_text(shell, &args),
        "big" | "large" | "largest" => find_big(shell, &args),
        "recent" | "new" | "latest" | "moi" => find_recent(shell, &args),
        "empty" | "rong" => find_empty(shell, &args),
        "help" | "?" | "--help" | "-h" => {
            print_help();
            Ok(0)
        }
        // Tương thích ngược: `jak find <tên>` → giống `jak find file <tên>`
        _ => {
            let mut shifted: Vec<&str> = vec!["file"];
            shifted.extend(args.iter().copied());
            find_by_name(shell, &shifted, Kind::File)
        }
    }
}

fn print_help() {
    println!("\x1b[1mjak find — tìm kiếm thân thiện\x1b[0m\n");
    let items: &[(&str, &str)] = &[
        ("jak find <tên>",                   "viết tắt của `jak find file <tên>`"),
        ("jak find file <tên> [in <path>]",  "tìm file (glob hoặc substring)"),
        ("jak find dir  <tên> [in <path>]",  "tìm thư mục"),
        ("jak find text \"<chuỗi>\" [in <p>]", "tìm chuỗi trong nội dung file (grep)"),
        ("jak find big  [in <path>]",        "20 file lớn nhất"),
        ("jak find recent [in <path>]",      "file sửa trong 24h gần đây"),
        ("jak find empty [in <path>]",       "file rỗng"),
    ];
    for (cmd, desc) in items {
        println!("  \x1b[36m{:38}\x1b[0m {}", cmd, desc);
    }
    println!("\n\x1b[2mGhi chú:\x1b[0m");
    println!("  • Glob: \x1b[33m*.pdf\x1b[0m, \x1b[33mreport-*\x1b[0m, \x1b[33m[Tt]est\x1b[0m");
    println!("  • Không glob → so khớp \x1b[1msubstring không phân biệt hoa thường\x1b[0m");
    println!("  • Tự bỏ qua: {}", IGNORE_DIRS.join(", "));
    println!("  • Từ khoá `in` có thể viết là `trong`");
    println!("  • Cú pháp `find` POSIX gốc (\x1b[36mfind . -name ...\x1b[0m) vẫn dùng bình thường (không cần `jak`).");
}

#[derive(Clone, Copy)]
enum Kind {
    File,
    Dir,
}

fn parse_target(args: &[&str], min_args: usize) -> Result<(String, PathBuf)> {
    if args.len() < min_args {
        return Err(anyhow!(
            "thiếu tham số. Gõ `find` để xem hướng dẫn."
        ));
    }
    let pattern = args[1].to_string();
    // [in|trong] <path>
    let path = if args.len() >= 4 && matches!(args[2], "in" | "trong") {
        let expanded = shellexpand::tilde(args[3]).to_string();
        PathBuf::from(expanded)
    } else if args.len() == 3 {
        return Err(anyhow!(
            "không nhận ra tham số `{}`. Bạn có thể quên từ khoá `in`?",
            args[2]
        ));
    } else {
        std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
    };
    Ok((pattern, path))
}

fn parse_optional_path(args: &[&str]) -> Result<PathBuf> {
    // `find big [in <path>]` — args[0] là keyword
    if args.len() >= 3 && matches!(args[1], "in" | "trong") {
        let expanded = shellexpand::tilde(args[2]).to_string();
        Ok(PathBuf::from(expanded))
    } else if args.len() == 1 {
        Ok(std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
    } else {
        Err(anyhow!("cú pháp: find {} [in <path>]", args[0]))
    }
}

fn resolve_root(shell: &Rc<RefCell<Shell>>, p: PathBuf) -> PathBuf {
    if p.is_absolute() {
        p
    } else {
        shell.borrow().cwd.join(p)
    }
}

// ─── find file / find dir ─────────────────────────────────────────────────────

fn find_by_name(shell: &Rc<RefCell<Shell>>, args: &[&str], kind: Kind) -> Result<i32> {
    let (pattern, root) = parse_target(args, 2)?;
    let root = resolve_root(shell, root);

    if !root.exists() {
        return Err(anyhow!("không tồn tại: {}", root.display()));
    }

    let matcher = Matcher::new(&pattern);
    let what = match kind {
        Kind::File => "file",
        Kind::Dir => "thư mục",
    };
    println!(
        "{DIM}tìm {what} khớp '{p}' trong {root}…{RESET}",
        what = what,
        p = pattern,
        root = root.display()
    );

    let mut count = 0u32;
    let cap = 1000u32;
    walk(&root, &root, &mut |entry, is_dir| {
        let name = match entry.file_name().and_then(|s| s.to_str()) {
            Some(n) => n,
            None => return WalkAction::Continue,
        };
        let matches_kind = match kind {
            Kind::File => !is_dir,
            Kind::Dir => is_dir,
        };
        if matches_kind && matcher.matches(name) {
            print_hit(entry, is_dir, &pattern);
            count += 1;
            if count >= cap {
                return WalkAction::Stop;
            }
        }
        WalkAction::Continue
    });

    println!(
        "{DIM}({n} kết quả{cap_note}){RESET}",
        n = count,
        cap_note = if count >= cap { ", đã đạt giới hạn" } else { "" }
    );
    Ok(if count > 0 { 0 } else { 1 })
}

fn print_hit(path: &Path, is_dir: bool, pattern: &str) {
    let display = path.display().to_string();
    // Highlight phần khớp trong tên file
    let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
    let parent = path.parent().map(|p| p.display().to_string()).unwrap_or_default();
    let highlighted = highlight_substring(name, pattern);
    let separator = if parent.is_empty() || parent == "." { "" } else { "/" };
    let parent_disp = if parent.is_empty() { String::new() } else { format!("{}{}{}", DIM, parent, RESET) };
    if is_dir {
        println!("{parent_disp}{separator}{BLUE}{highlighted}{RESET}/");
    } else {
        println!("{parent_disp}{separator}{highlighted}");
    }
    let _ = display;
}

fn highlight_substring(name: &str, pattern: &str) -> String {
    // Bỏ qua khi pattern là glob
    if pattern.contains('*') || pattern.contains('?') || pattern.contains('[') {
        return name.to_string();
    }
    let lower_name = name.to_lowercase();
    let lower_pat = pattern.to_lowercase();
    if let Some(idx) = lower_name.find(&lower_pat) {
        let end = idx + lower_pat.len();
        format!(
            "{}{}{}{}{}{}",
            &name[..idx],
            YELLOW,
            &name[idx..end],
            RESET,
            "",
            &name[end..],
        )
    } else {
        name.to_string()
    }
}

// ─── find text (grep wrapper) ─────────────────────────────────────────────────

fn find_text(shell: &Rc<RefCell<Shell>>, args: &[&str]) -> Result<i32> {
    let (pattern, root) = parse_target(args, 2)?;
    let root = resolve_root(shell, root);

    println!(
        "{DIM}tìm chuỗi '{p}' trong {root}…{RESET}",
        p = pattern,
        root = root.display()
    );

    // Ưu tiên ripgrep nếu có, sau đến grep -rIn
    let (prog, extra_args): (&str, Vec<&str>) = if which::which("rg").is_ok() {
        ("rg", vec!["--color=always", "-n", "-S"])
    } else if which::which("grep").is_ok() {
        ("grep", vec!["-rnI", "--color=always",
            "--exclude-dir=.git", "--exclude-dir=node_modules",
            "--exclude-dir=target", "--exclude-dir=venv",
            "--exclude-dir=__pycache__"])
    } else {
        eprintln!(
            "\x1b[33m⚠ không có `rg` hoặc `grep` trên hệ thống.\x1b[0m \
             \x1b[2mCài một trong hai để dùng `jak find text`.\x1b[0m"
        );
        return Ok(127);
    };

    let status = Command::new(prog)
        .args(&extra_args)
        .arg(&pattern)
        .arg(&root)
        .status();

    match status {
        Ok(s) => Ok(s.code().unwrap_or(1)),
        Err(e) => Err(anyhow!("không chạy được {}: {}", prog, e)),
    }
}

// ─── find big / recent / empty ────────────────────────────────────────────────

fn find_big(shell: &Rc<RefCell<Shell>>, args: &[&str]) -> Result<i32> {
    let root = resolve_root(shell, parse_optional_path(args)?);
    println!(
        "{DIM}quét file lớn trong {root}…{RESET}",
        root = root.display()
    );

    let mut sizes: Vec<(u64, PathBuf)> = Vec::new();
    walk(&root, &root, &mut |entry, is_dir| {
        if !is_dir {
            if let Ok(m) = entry.metadata() {
                sizes.push((m.len(), entry.to_path_buf()));
            }
        }
        WalkAction::Continue
    });

    sizes.sort_by(|a, b| b.0.cmp(&a.0));
    let top = sizes.iter().take(20);
    for (size, path) in top {
        let s = humanize(*size);
        println!("  {CYAN}{:>8}{RESET}  {}", s, path.display());
    }
    println!("{DIM}(tổng quét: {} file){RESET}", sizes.len());
    Ok(0)
}

fn find_recent(shell: &Rc<RefCell<Shell>>, args: &[&str]) -> Result<i32> {
    let root = resolve_root(shell, parse_optional_path(args)?);
    let cutoff = std::time::SystemTime::now() - std::time::Duration::from_secs(60 * 60 * 24);
    println!(
        "{DIM}file sửa trong 24h gần đây — {root}…{RESET}",
        root = root.display()
    );

    let mut hits: Vec<(std::time::SystemTime, PathBuf)> = Vec::new();
    walk(&root, &root, &mut |entry, is_dir| {
        if is_dir {
            return WalkAction::Continue;
        }
        if let Ok(m) = entry.metadata() {
            if let Ok(mtime) = m.modified() {
                if mtime >= cutoff {
                    hits.push((mtime, entry.to_path_buf()));
                }
            }
        }
        WalkAction::Continue
    });
    hits.sort_by(|a, b| b.0.cmp(&a.0));
    for (mtime, path) in &hits {
        let ago = humanize_ago(*mtime);
        println!("  {GREEN}{:>10}{RESET}  {}", ago, path.display());
    }
    println!("{DIM}({} file){RESET}", hits.len());
    Ok(0)
}

fn find_empty(shell: &Rc<RefCell<Shell>>, args: &[&str]) -> Result<i32> {
    let root = resolve_root(shell, parse_optional_path(args)?);
    println!(
        "{DIM}file rỗng (0 byte) trong {root}…{RESET}",
        root = root.display()
    );
    let mut n = 0u32;
    walk(&root, &root, &mut |entry, is_dir| {
        if is_dir {
            return WalkAction::Continue;
        }
        if let Ok(m) = entry.metadata() {
            if m.len() == 0 {
                println!("  {}", entry.display());
                n += 1;
            }
        }
        WalkAction::Continue
    });
    println!("{DIM}({n} file){RESET}");
    Ok(0)
}

// ─── walk + matcher ───────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq)]
enum WalkAction {
    Continue,
    Stop,
}

fn walk<F: FnMut(&Path, bool) -> WalkAction>(root: &Path, current: &Path, cb: &mut F) -> WalkAction {
    let is_dir = current.is_dir();
    // Gọi callback cho TẤT CẢ entry trừ root (root chỉ là điểm bắt đầu)
    if current != root {
        if cb(current, is_dir) == WalkAction::Stop {
            return WalkAction::Stop;
        }
    }
    if is_dir {
        // Skip ignore-dirs (trừ khi đó là root)
        if current != root {
            if let Some(name) = current.file_name().and_then(|s| s.to_str()) {
                if IGNORE_DIRS.contains(&name) {
                    return WalkAction::Continue;
                }
            }
        }
        if let Ok(it) = std::fs::read_dir(current) {
            let mut entries: Vec<_> = it.flatten().collect();
            entries.sort_by_key(|e| e.file_name());
            for e in entries {
                if walk(root, &e.path(), cb) == WalkAction::Stop {
                    return WalkAction::Stop;
                }
            }
        }
    }
    WalkAction::Continue
}

struct Matcher {
    is_glob: bool,
    glob_pat: Option<glob::Pattern>,
    needle: String,
}

impl Matcher {
    fn new(pat: &str) -> Self {
        let is_glob = pat.contains('*') || pat.contains('?') || pat.contains('[');
        let glob_pat = if is_glob { glob::Pattern::new(pat).ok() } else { None };
        Self {
            is_glob,
            glob_pat,
            needle: pat.to_lowercase(),
        }
    }
    fn matches(&self, name: &str) -> bool {
        if self.is_glob {
            self.glob_pat.as_ref().map(|p| p.matches(name)).unwrap_or(false)
        } else {
            name.to_lowercase().contains(&self.needle)
        }
    }
}

// ─── format helpers ───────────────────────────────────────────────────────────

fn humanize(bytes: u64) -> String {
    const UNITS: [&str; 6] = ["B", "K", "M", "G", "T", "P"];
    let mut v = bytes as f64;
    let mut i = 0;
    while v >= 1024.0 && i < UNITS.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 {
        format!("{}{}", bytes, UNITS[i])
    } else {
        format!("{:.1}{}", v, UNITS[i])
    }
}

fn humanize_ago(t: std::time::SystemTime) -> String {
    let dur = std::time::SystemTime::now().duration_since(t).unwrap_or_default();
    let secs = dur.as_secs();
    if secs < 60 {
        format!("{}s", secs)
    } else if secs < 3600 {
        format!("{}m", secs / 60)
    } else if secs < 86400 {
        format!("{}h{:02}", secs / 3600, (secs % 3600) / 60)
    } else {
        format!("{}d", secs / 86400)
    }
}
