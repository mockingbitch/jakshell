//! Bookmark — đặt tên cho lệnh dài.
//!
//! ```
//! bookmark docker_app docker exec -it payin_app sh
//! jak docker_app                       # chạy bookmark
//! jak docker_app -e KEY=val            # ghép thêm tham số
//! bookmark                             # liệt kê
//! bookmark del docker_app              # xoá
//! ```
//!
//! Lưu tại `~/.config/jaksh/bookmarks.toml`:
//! ```toml
//! docker_app = "docker exec -it payin_app sh"
//! ```

use anyhow::{anyhow, Result};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::rc::Rc;

use crate::shell::Shell;

/// Các tên bị cấm vì trùng với jak subcommand sẵn có.
const RESERVED: &[&str] = &[
    "help", "?", "list", "ls", "add", "set", "del", "rm", "remove", "show",
    "clean", "backup", "update", "find", "open", "sysinfo", "theme",
    "weather", "ip", "git",
];

pub fn store_path() -> PathBuf {
    dirs::home_dir()
        .map(|h| h.join(".config").join("jaksh").join("bookmarks.toml"))
        .unwrap_or_else(|| PathBuf::from("bookmarks.toml"))
}

pub fn load() -> BTreeMap<String, String> {
    let path = store_path();
    let content = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(_) => return BTreeMap::new(),
    };
    toml::from_str(&content).unwrap_or_default()
}

fn save(map: &BTreeMap<String, String>) -> Result<()> {
    let path = store_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    let s = toml::to_string_pretty(map)?;
    std::fs::write(&path, s)?;
    Ok(())
}

/// Tra cứu 1 bookmark theo tên.
pub fn lookup(name: &str) -> Option<String> {
    load().get(name).cloned()
}

/// Iterator các bookmark (đã sort theo tên).
pub fn list_all() -> Vec<(String, String)> {
    load().into_iter().collect()
}

// ─── Builtin entry point ──────────────────────────────────────────────────────

pub fn run(_shell: &Rc<RefCell<Shell>>, argv: &[String]) -> Result<i32> {
    let args: Vec<&str> = argv.iter().skip(1).map(|s| s.as_str()).collect();
    let sub = args.first().copied().unwrap_or("list");
    match sub {
        "list" | "ls" => list_cmd(),
        "show" => show_cmd(&args[1..]),
        "del" | "rm" | "remove" => del_cmd(&args[1..]),
        "add" | "set" => add_cmd(&args[1..]),
        "help" | "?" | "--help" | "-h" => {
            print_help();
            Ok(0)
        }
        _ => {
            // Form rút gọn: `bookmark <name> <cmd ...>` (không cần `add`)
            // Áp dụng khi sub không phải reserved keyword.
            add_cmd(&args)
        }
    }
}

fn print_help() {
    println!("\x1b[1mbookmark — đặt tên cho lệnh dài\x1b[0m\n");
    let items: &[(&str, &str)] = &[
        ("bookmark <name> <cmd ...>",   "tạo / cập nhật bookmark"),
        ("bookmark add <name> <cmd ...>", "(rõ ràng hơn) tạo / cập nhật"),
        ("bookmark",                    "liệt kê tất cả (= bookmark list)"),
        ("bookmark show <name>",        "in lệnh được gắn cho <name>"),
        ("bookmark del <name>",         "xoá bookmark (= rm / remove)"),
    ];
    for (cmd, desc) in items {
        println!("  \x1b[36m{:32}\x1b[0m {}", cmd, desc);
    }
    println!("\nChạy bookmark: \x1b[36mjak <name>\x1b[0m  (có thể thêm tham số phía sau)\n");
    println!("\x1b[2mLưu tại: ~/.config/jaksh/bookmarks.toml\x1b[0m");
}

fn list_cmd() -> Result<i32> {
    let entries = list_all();
    if entries.is_empty() {
        println!("\x1b[2m(chưa có bookmark nào)\x1b[0m");
        println!("Tạo: \x1b[36mbookmark <name> <command ...>\x1b[0m");
        return Ok(0);
    }
    let w = entries.iter().map(|(n, _)| n.len()).max().unwrap_or(0).min(24);
    println!("\x1b[1m{} bookmark:\x1b[0m", entries.len());
    for (name, cmd) in entries {
        println!("  \x1b[36m{:<w$}\x1b[0m \x1b[2m→\x1b[0m {}", name, cmd, w = w);
    }
    Ok(0)
}

fn show_cmd(args: &[&str]) -> Result<i32> {
    let name = match args.first() {
        Some(n) => *n,
        None => {
            eprintln!("dùng: bookmark show <name>");
            return Ok(2);
        }
    };
    match lookup(name) {
        Some(cmd) => {
            println!("{}", cmd);
            Ok(0)
        }
        None => {
            eprintln!("không có bookmark '{}'", name);
            Ok(1)
        }
    }
}

fn del_cmd(args: &[&str]) -> Result<i32> {
    if args.is_empty() {
        eprintln!("dùng: bookmark del <name ...>");
        return Ok(2);
    }
    let mut map = load();
    let mut removed = 0;
    for name in args {
        if map.remove(*name).is_some() {
            println!("\x1b[2m- xoá:\x1b[0m {}", name);
            removed += 1;
        } else {
            eprintln!("không có bookmark '{}'", name);
        }
    }
    if removed > 0 {
        save(&map)?;
    }
    Ok(if removed == args.len() { 0 } else { 1 })
}

fn add_cmd(args: &[&str]) -> Result<i32> {
    if args.len() < 2 {
        eprintln!("dùng: bookmark <name> <lệnh ...>");
        return Ok(2);
    }
    let name = args[0].to_string();
    validate_name(&name)?;
    if RESERVED.contains(&name.as_str()) {
        return Err(anyhow!(
            "'{}' là tên dành riêng cho jak subcommand — chọn tên khác.",
            name
        ));
    }
    let cmd = rejoin_args(&args[1..]);
    let mut map = load();
    let action = if map.contains_key(&name) { "cập nhật" } else { "tạo" };
    map.insert(name.clone(), cmd.clone());
    save(&map)?;
    println!("\x1b[32m✓ {}:\x1b[0m {} \x1b[2m→\x1b[0m {}", action, name, cmd);
    println!("\x1b[2m  Chạy: jak {}\x1b[0m", name);
    Ok(0)
}

fn validate_name(name: &str) -> Result<()> {
    if name.is_empty() {
        return Err(anyhow!("tên bookmark rỗng"));
    }
    if name.len() > 64 {
        return Err(anyhow!("tên bookmark quá dài (> 64 ký tự)"));
    }
    for ch in name.chars() {
        if !(ch.is_ascii_alphanumeric() || ch == '_' || ch == '-') {
            return Err(anyhow!(
                "tên bookmark chỉ chứa chữ/số/dấu _ -, không hợp lệ: '{}'",
                ch
            ));
        }
    }
    Ok(())
}

/// Ghép lại args đã bị shell tách. Re-quote phần có khoảng trắng hoặc ký tự đặc biệt
/// để lưu được lệnh phức tạp.
fn rejoin_args(args: &[&str]) -> String {
    args.iter()
        .map(|a| shell_quote(a))
        .collect::<Vec<_>>()
        .join(" ")
}

fn shell_quote(s: &str) -> String {
    if s.is_empty() {
        return "\"\"".to_string();
    }
    let needs = s
        .chars()
        .any(|c| c.is_whitespace() || matches!(c, '|' | '&' | ';' | '<' | '>' | '"' | '\'' | '$' | '`' | '\\' | '*' | '?'));
    if !needs {
        return s.to_string();
    }
    // Dùng nháy đơn nếu không chứa nháy đơn — đơn giản nhất, không cần escape gì.
    if !s.contains('\'') {
        return format!("'{}'", s);
    }
    // Có nháy đơn — fall back nháy kép, escape \ $ " `.
    let escaped: String = s
        .chars()
        .map(|c| match c {
            '\\' | '"' | '$' | '`' => format!("\\{}", c),
            _ => c.to_string(),
        })
        .collect();
    format!("\"{}\"", escaped)
}

// ─── Run bookmark từ jak ──────────────────────────────────────────────────────

/// Chạy bookmark `name` với tham số phụ thêm. Đưa qua tokenizer + parser + executor
/// nên hỗ trợ pipe / redirect / quote y như gõ trực tiếp.
pub fn execute(
    shell: &Rc<RefCell<Shell>>,
    name: &str,
    extra: &[&str],
) -> Result<i32> {
    let cmd = match lookup(name) {
        Some(c) => c,
        None => return Err(anyhow!("không có bookmark '{}'", name)),
    };
    let full = if extra.is_empty() {
        cmd
    } else {
        let extras_quoted: Vec<String> = extra.iter().map(|s| shell_quote(s)).collect();
        format!("{} {}", cmd, extras_quoted.join(" "))
    };
    let tokens = crate::lexer::tokenize(&full)?;
    let ast = crate::parser::parse(&tokens)?;
    crate::executor::execute(shell, &ast)
}
