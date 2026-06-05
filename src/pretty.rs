//! `--jak` flag: bắt output của lệnh phổ biến, in lại đẹp hơn.
//!
//! Quy tắc:
//!   • Cờ `--jak` được JakShell tách ra TRƯỚC khi gọi lệnh thật.
//!   • Output gốc được capture; stderr in nguyên si; stdout được re-render.
//!   • Giữ nguyên cấu trúc cột, chỉ thêm 1 header gợi nhớ + tô màu giá trị.
//!   • Pipe/redirect không nên dùng với `--jak` (output đã chứa ANSI).

use anyhow::Result;
use std::cell::RefCell;
use std::io::Write;
use std::process::Command;
use std::rc::Rc;

use crate::parser::Redirect;
use crate::shell::Shell;

/// Có nên chạy ở chế độ pretty cho lệnh `cmd` này không?
pub fn supports(cmd: &str) -> bool {
    matches!(cmd, "ls" | "ps" | "df" | "du" | "git")
}

pub fn run(
    shell: &Rc<RefCell<Shell>>,
    argv: &[String],
    _redirects: &[Redirect],
) -> Result<i32> {
    let cmd = argv[0].as_str();
    let args: Vec<&str> = argv.iter().skip(1).map(|s| s.as_str()).collect();
    let use_color = shell.borrow().theme.use_color;

    let mut builder = Command::new(cmd);
    // Với git: ép màu ngay cả khi pipe (vì ta capture stdout)
    if cmd == "git" {
        builder.arg("-c").arg("color.ui=always");
    }
    builder.args(&args);
    builder.current_dir(&shell.borrow().cwd);

    let output = match builder.output() {
        Ok(o) => o,
        Err(e) => {
            if e.kind() == std::io::ErrorKind::NotFound {
                eprintln!("--jak: không tìm thấy lệnh: {}", cmd);
                return Ok(127);
            }
            eprintln!("--jak: không chạy được {}: {}", cmd, e);
            return Ok(126);
        }
    };

    std::io::stderr().write_all(&output.stderr).ok();
    let text = String::from_utf8_lossy(&output.stdout);

    match cmd {
        "ls" => pretty_ls(&text, &args, use_color),
        "ps" => pretty_ps(&text, use_color),
        "df" => pretty_df(&text, use_color),
        "du" => pretty_du(&text, use_color),
        "git" => pretty_git(&text, &args, use_color),
        _ => {
            // Không có prettifier — in raw
            print!("{}", text);
        }
    }

    Ok(output.status.code().unwrap_or(0))
}

// ─── helpers ──────────────────────────────────────────────────────────────────

fn c(use_color: bool, code: &str) -> &str {
    if use_color { code } else { "" }
}

const RESET: &str = "\x1b[0m";
const BOLD: &str = "\x1b[1m";
const DIM: &str = "\x1b[2m";
const RED: &str = "\x1b[31m";
const GREEN: &str = "\x1b[32m";
const YELLOW: &str = "\x1b[33m";
const MAGENTA: &str = "\x1b[35m";
const CYAN: &str = "\x1b[36m";
const BRIGHT_BLUE: &str = "\x1b[94m";
const BRIGHT_GREEN: &str = "\x1b[92m";
const BRIGHT_CYAN: &str = "\x1b[96m";

// ─── ls ───────────────────────────────────────────────────────────────────────

fn pretty_ls(out: &str, args: &[&str], color: bool) {
    let flags: String = args.iter().filter(|a| a.starts_with('-')).copied().collect::<Vec<_>>().join("");
    let long = flags.contains('l');

    if !long {
        // Output ngắn (không có -l): chỉ tô màu tên theo loại
        pretty_ls_short(out, color);
        return;
    }

    let mut lines = out.lines().peekable();
    // Dòng "total N"
    if let Some(first) = lines.peek() {
        if first.starts_with("total ") {
            let line = lines.next().unwrap();
            println!("{dim}{line}{reset}", dim = c(color, DIM), line = line, reset = c(color, RESET));
        }
    }

    // Parse dữ liệu để biết độ rộng từng cột (căn đẹp)
    let rows: Vec<Vec<String>> = lines
        .map(|l| parse_ls_long_line(l))
        .filter(|v| v.len() == 7)
        .collect();

    if rows.is_empty() {
        // Không parse được — in nguyên
        for l in out.lines() {
            println!("{}", l);
        }
        return;
    }

    // Tính độ rộng các cột (perms, links, owner, group, size, date, name)
    let mut w = [0usize; 7];
    for r in &rows {
        w[0] = w[0].max(r[0].len()); // perms
        w[1] = w[1].max(r[1].len()); // links
        w[2] = w[2].max(r[2].len()); // owner
        w[3] = w[3].max(r[3].len()); // group
        w[4] = w[4].max(r[4].len()); // size
        w[5] = w[5].max(r[5].len()); // date
        w[6] = w[6].max(r[6].len()); // name
    }

    // Header gợi nhớ
    let arrow = if color { "\x1b[2m→\x1b[0m" } else { "→" };
    println!(
        "{arrow} {bold}{:<wp$}  {:>wl$}  {:<wo$}  {:<wg$}  {:>ws$}  {:<wd$}  {:<wn$}{reset}",
        "perms", "links", "owner", "group", "size", "date", "name",
        wp = w[0], wl = w[1], wo = w[2], wg = w[3], ws = w[4], wd = w[5], wn = w[6],
        bold = c(color, DIM),
        reset = c(color, RESET),
    );

    for r in rows {
        let perms = &r[0];
        let links = &r[1];
        let owner = &r[2];
        let group = &r[3];
        let size = &r[4];
        let date = &r[5];
        let name = &r[6];

        let (name_colored, name_decor) = colorize_ls_name(perms, name, color);
        let perms_colored = colorize_perms(perms, color);
        let size_colored = colorize_size(size, color);

        // perms cần độ rộng theo TEXT (không tính ANSI). Pad bằng spaces sau khi tô.
        let pad_perms = " ".repeat(w[0].saturating_sub(visible_len(perms)));
        let pad_size = " ".repeat(w[4].saturating_sub(visible_len(size)));

        println!(
            "  {perms}{pp}  {links:>wl$}  {owner:<wo$}  {group:<wg$}  {size}{ps}  {date:<wd$}  {name}{decor}",
            perms = perms_colored,
            pp = pad_perms,
            links = links,
            owner = owner,
            group = group,
            size = size_colored,
            ps = pad_size,
            date = date,
            name = name_colored,
            decor = name_decor,
            wl = w[1], wo = w[2], wg = w[3], wd = w[5],
        );
    }
}

fn pretty_ls_short(out: &str, color: bool) {
    // Output ngắn không có metadata để biết loại — chỉ in lại, thêm '/' nếu kiểm tra được path.
    for line in out.lines() {
        for token in line.split_whitespace() {
            let p = std::path::Path::new(token);
            let meta = p.symlink_metadata().ok();
            let label = match meta {
                Some(m) if m.file_type().is_dir() => format!("{}{}{}/", c(color, BRIGHT_BLUE), token, c(color, RESET)),
                Some(m) if m.file_type().is_symlink() => format!("{}{}{}@", c(color, CYAN), token, c(color, RESET)),
                Some(m) => {
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::PermissionsExt;
                        if m.permissions().mode() & 0o111 != 0 {
                            format!("{}{}{}*", c(color, BRIGHT_GREEN), token, c(color, RESET))
                        } else {
                            token.to_string()
                        }
                    }
                    #[cfg(not(unix))]
                    { token.to_string() }
                }
                None => token.to_string(),
            };
            print!("{}  ", label);
        }
        println!();
    }
}

fn parse_ls_long_line(line: &str) -> Vec<String> {
    // PERMS LINKS OWNER GROUP SIZE MON DAY (TIME|YEAR) NAME...
    let toks: Vec<&str> = line.split_whitespace().collect();
    if toks.len() < 8 {
        return Vec::new();
    }
    let date = format!("{} {} {}", toks[5], toks[6], toks[7]);
    let name = if toks.len() > 8 { toks[8..].join(" ") } else { String::new() };
    vec![
        toks[0].to_string(),
        toks[1].to_string(),
        toks[2].to_string(),
        toks[3].to_string(),
        toks[4].to_string(),
        date,
        name,
    ]
}

fn colorize_perms(perms: &str, color: bool) -> String {
    if !color || perms.len() < 10 {
        return perms.to_string();
    }
    let mut out = String::new();
    let bytes: Vec<char> = perms.chars().collect();
    // Loại
    let typ_col = match bytes[0] {
        'd' => BRIGHT_BLUE,
        'l' => CYAN,
        'c' | 'b' => MAGENTA,
        's' | 'p' => YELLOW,
        _ => "",
    };
    out.push_str(typ_col);
    out.push(bytes[0]);
    out.push_str(RESET);
    // 9 ký tự rwx
    for (i, ch) in bytes.iter().skip(1).take(9).enumerate() {
        let col = match *ch {
            'r' => YELLOW,
            'w' => RED,
            'x' | 's' | 't' => GREEN,
            'S' | 'T' => MAGENTA,
            '-' => DIM,
            _ => "",
        };
        let _ = i;
        out.push_str(col);
        out.push(*ch);
        out.push_str(RESET);
    }
    // Phần đuôi (@/+/.) nếu có
    if bytes.len() > 10 {
        out.push_str(DIM);
        out.extend(bytes.iter().skip(10));
        out.push_str(RESET);
    }
    out
}

fn colorize_size(size: &str, color: bool) -> String {
    if !color {
        return size.to_string();
    }
    // Phân biệt số và suffix (B/K/M/G/T/Ki/Mi/Gi…)
    let mut idx = 0;
    for (i, ch) in size.char_indices() {
        if ch.is_ascii_digit() || ch == '.' {
            idx = i + ch.len_utf8();
        } else {
            break;
        }
    }
    if idx == 0 {
        return size.to_string();
    }
    let (num, suf) = size.split_at(idx);
    let suf_col = match suf.chars().next() {
        Some('K' | 'k') => CYAN,
        Some('M' | 'm') => GREEN,
        Some('G' | 'g') => YELLOW,
        Some('T' | 't') => RED,
        _ => DIM,
    };
    if suf.is_empty() {
        // chỉ số bytes — gray
        format!("{}{}{}", DIM, num, RESET)
    } else {
        format!("{}{}{}{}{}", DIM, num, suf_col, suf, RESET)
    }
}

fn colorize_ls_name(perms: &str, name: &str, color: bool) -> (String, &'static str) {
    if !color {
        let decor = if perms.starts_with('d') { "/" }
                    else if perms.starts_with('l') { "@" }
                    else if is_exec(perms) { "*" }
                    else { "" };
        return (name.to_string(), decor);
    }
    if perms.starts_with('d') {
        (format!("{}{}{}{}", BOLD, BRIGHT_BLUE, name, RESET), "/")
    } else if perms.starts_with('l') {
        (format!("{}{}{}", CYAN, name, RESET), "@")
    } else if is_exec(perms) {
        (format!("{}{}{}{}", BOLD, BRIGHT_GREEN, name, RESET), "*")
    } else {
        // Phân loại nhẹ theo đuôi
        let ext_col = match std::path::Path::new(name).extension().and_then(|s| s.to_str()) {
            Some("md" | "txt" | "rst" | "log") => BRIGHT_CYAN,
            Some("rs" | "go" | "py" | "js" | "ts" | "rb" | "java" | "c" | "cpp" | "h" | "hpp") => YELLOW,
            Some("png" | "jpg" | "jpeg" | "gif" | "svg" | "webp") => MAGENTA,
            Some("zip" | "tar" | "gz" | "bz2" | "xz" | "7z" | "rar") => RED,
            Some("toml" | "yaml" | "yml" | "json" | "ini" | "conf") => BRIGHT_CYAN,
            _ => "",
        };
        if ext_col.is_empty() {
            (name.to_string(), "")
        } else {
            (format!("{}{}{}", ext_col, name, RESET), "")
        }
    }
}

fn is_exec(perms: &str) -> bool {
    let b = perms.as_bytes();
    b.len() >= 10 && (b[3] == b'x' || b[6] == b'x' || b[9] == b'x' || b[3] == b's' || b[6] == b's')
}

/// Đếm số ký tự "nhìn thấy" (bỏ qua escape ANSI \x1b[...m).
fn visible_len(s: &str) -> usize {
    let mut n = 0;
    let mut in_esc = false;
    for ch in s.chars() {
        if in_esc {
            if ch == 'm' { in_esc = false; }
            continue;
        }
        if ch == '\x1b' {
            in_esc = true;
            continue;
        }
        n += 1;
    }
    n
}

// ─── ps ───────────────────────────────────────────────────────────────────────

fn pretty_ps(out: &str, color: bool) {
    let mut lines = out.lines();
    let header = match lines.next() {
        Some(h) => h,
        None => return,
    };
    let cols: Vec<&str> = header.split_whitespace().collect();
    let n = cols.len();

    // Tô đậm header
    let header_line = if color {
        format!("{}{}{}", BOLD, header, RESET)
    } else {
        header.to_string()
    };
    println!("{}", header_line);

    for row in lines {
        if row.trim().is_empty() {
            println!();
            continue;
        }
        let toks: Vec<&str> = row.split_whitespace().collect();
        if toks.len() < n {
            println!("{}", row);
            continue;
        }
        // Ghép phần dư vào cột cuối (COMMAND thường có spaces)
        let vals: Vec<String> = if toks.len() == n {
            toks.iter().map(|s| s.to_string()).collect()
        } else {
            let mut v: Vec<String> = toks.iter().take(n - 1).map(|s| s.to_string()).collect();
            v.push(toks[(n - 1)..].join(" "));
            v
        };
        // In với màu theo tên cột
        let parts: Vec<String> = cols.iter().enumerate().map(|(i, col)| {
            let v = &vals[i];
            colorize_ps_value(col, v, color)
        }).collect();
        // Cố căn cột theo header gốc (đơn giản hoá: ghép bằng 2 spaces, COMMAND không thụt)
        println!("{}", parts.join("  "));
    }
}

fn colorize_ps_value(col: &str, val: &str, color: bool) -> String {
    if !color {
        return val.to_string();
    }
    match col {
        "USER" | "UID" => {
            let c = if val == "root" || val == "_root" { RED } else { CYAN };
            format!("{}{}{}", c, val, RESET)
        }
        "PID" | "PPID" => format!("{}{}{}", BRIGHT_CYAN, val, RESET),
        "%CPU" | "%MEM" => {
            let n: f32 = val.parse().unwrap_or(0.0);
            let c = if n >= 50.0 { RED }
                    else if n >= 10.0 { YELLOW }
                    else if n > 0.0 { GREEN }
                    else { DIM };
            format!("{}{}{}", c, val, RESET)
        }
        "STAT" | "S" | "STATE" => {
            let c = match val.chars().next() {
                Some('R') => GREEN,
                Some('S' | 'I') => DIM,
                Some('D') => YELLOW,
                Some('T') => MAGENTA,
                Some('Z') => RED,
                _ => "",
            };
            format!("{}{}{}", c, val, RESET)
        }
        "COMMAND" | "CMD" | "ARGS" => format!("{}{}{}", DIM, val, RESET),
        "TIME" | "STARTED" | "STIME" | "START" => format!("{}{}{}", DIM, val, RESET),
        _ => val.to_string(),
    }
}

// ─── df ───────────────────────────────────────────────────────────────────────

fn pretty_df(out: &str, color: bool) {
    let mut lines = out.lines();
    let header = match lines.next() {
        Some(h) => h,
        None => return,
    };
    // Bold header
    if color {
        println!("{}{}{}", BOLD, header, RESET);
    } else {
        println!("{}", header);
    }

    for line in lines {
        if line.trim().is_empty() {
            println!();
            continue;
        }
        let toks: Vec<&str> = line.split_whitespace().collect();
        // Tìm cột "Use%" hoặc "Capacity" (xx%) để tô màu theo mức
        let mut out_line = String::new();
        for (i, t) in toks.iter().enumerate() {
            if t.ends_with('%') {
                let pct: u32 = t.trim_end_matches('%').parse().unwrap_or(0);
                let col = if !color { "" }
                          else if pct >= 90 { RED }
                          else if pct >= 75 { YELLOW }
                          else if pct >= 50 { GREEN }
                          else { DIM };
                out_line.push_str(&format!("{}{}{}", col, t, c(color, RESET)));
            } else if is_size_token(t) {
                out_line.push_str(&colorize_size(t, color));
            } else if t.starts_with('/') {
                // path
                out_line.push_str(&format!("{}{}{}", c(color, BRIGHT_BLUE), t, c(color, RESET)));
            } else {
                out_line.push_str(t);
            }
            if i < toks.len() - 1 {
                out_line.push(' ');
            }
        }
        println!("{}", out_line);
    }
}

fn is_size_token(t: &str) -> bool {
    // 228Gi, 10K, 1.5M, 482Mi…
    let mut digits = false;
    for ch in t.chars() {
        if ch.is_ascii_digit() || ch == '.' {
            digits = true;
        } else if digits && matches!(ch, 'K' | 'M' | 'G' | 'T' | 'P' | 'k' | 'm' | 'g' | 't' | 'i' | 'B' | 'b') {
            continue;
        } else {
            return false;
        }
    }
    digits
}

// ─── du ───────────────────────────────────────────────────────────────────────

// ─── git ──────────────────────────────────────────────────────────────────────

fn pretty_git(out: &str, args: &[&str], color: bool) {
    let sub = args.iter().find(|a| !a.starts_with('-')).copied().unwrap_or("");
    match sub {
        "status" => pretty_git_status(out, color),
        "log" => {
            // git -c color.ui=always log đã có màu — chỉ in lại
            print!("{}", out);
        }
        "diff" => {
            // git diff đã đẹp — in lại
            print!("{}", out);
        }
        "branch" => pretty_git_branch(out, color),
        _ => print!("{}", out),
    }
}

fn pretty_git_status(out: &str, color: bool) {
    // Output `git status` thường có dạng:
    //   On branch main
    //   Your branch is up to date with 'origin/main'.
    //
    //   Changes to be committed:
    //     (use "git restore --staged <file>..." to unstage)
    //         modified:   src/main.rs
    //         new file:   src/new.rs
    //
    //   Changes not staged for commit:
    //     (use "git add <file>..." to update what will be committed)
    //         modified:   README.md
    //         deleted:    old.txt
    //
    //   Untracked files:
    //     (use "git add <file>..." to include in what will be committed)
    //         temp.log
    //
    //   no changes added to commit (...)
    let mut section: Section = Section::None;
    for raw in out.lines() {
        let line = strip_ansi(raw);
        let trimmed = line.trim();

        if trimmed.starts_with("On branch ") {
            let br = &trimmed["On branch ".len()..];
            print_kv(color, "branch", br, CYAN);
            continue;
        }
        if trimmed.starts_with("HEAD detached at ") {
            let h = &trimmed["HEAD detached at ".len()..];
            print_kv(color, "HEAD", &format!("(detached) {}", h), MAGENTA);
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("Your branch is ahead of ") {
            print_kv(color, "ahead", rest, GREEN);
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("Your branch is behind ") {
            print_kv(color, "behind", rest, RED);
            continue;
        }
        if trimmed.starts_with("Your branch is up to date") {
            print_kv(color, "remote", "up to date", DIM);
            continue;
        }
        if trimmed.starts_with("Your branch and ") && trimmed.contains("diverged") {
            print_kv(color, "remote", "diverged — cần merge/rebase", YELLOW);
            continue;
        }

        if trimmed == "Changes to be committed:" {
            println!();
            println!("{bold}{green}● Staged — sẵn sàng commit:{reset}",
                bold = c(color, BOLD), green = c(color, GREEN), reset = c(color, RESET));
            section = Section::Staged;
            continue;
        }
        if trimmed == "Changes not staged for commit:" || trimmed.starts_with("Changes not staged") {
            println!();
            println!("{bold}{yellow}● Modified — đã sửa, chưa stage:{reset}",
                bold = c(color, BOLD), yellow = c(color, YELLOW), reset = c(color, RESET));
            section = Section::Modified;
            continue;
        }
        if trimmed == "Untracked files:" {
            println!();
            println!("{bold}{red}● Untracked — git chưa biết:{reset}",
                bold = c(color, BOLD), red = c(color, RED), reset = c(color, RESET));
            section = Section::Untracked;
            continue;
        }
        if trimmed == "Unmerged paths:" {
            println!();
            println!("{bold}{red}● Conflict — chưa giải quyết:{reset}",
                bold = c(color, BOLD), red = c(color, RED), reset = c(color, RESET));
            section = Section::Conflict;
            continue;
        }
        if trimmed == "nothing to commit, working tree clean" {
            println!();
            println!("  {green}✓ working tree sạch — không có gì để commit.{reset}",
                green = c(color, GREEN), reset = c(color, RESET));
            section = Section::None;
            continue;
        }
        if trimmed.starts_with("no changes added to commit") {
            // bỏ qua câu nhắc dài
            continue;
        }
        // Bỏ các dòng "  (use \"git ...\" ...)"
        if trimmed.starts_with("(use ") && trimmed.ends_with(")") {
            continue;
        }

        // Dòng file của từng section
        if matches!(section, Section::Staged | Section::Modified) {
            // Dòng dạng "  modified: path" / "  new file: path" / "  deleted: path"
            if let Some((label, path)) = split_status_line(&line) {
                let (icon, col) = icon_for_status(&label, &section, color);
                let path_str = path.trim();
                if matches!(section, Section::Staged) {
                    println!("  {col}{icon}{reset}  {green}{path}{reset}  {dim}({label}){reset}",
                        col = col, icon = icon, reset = c(color, RESET),
                        green = c(color, GREEN), path = path_str,
                        dim = c(color, DIM), label = label);
                } else {
                    println!("  {col}{icon}{reset}  {yellow}{path}{reset}  {dim}({label}){reset}",
                        col = col, icon = icon, reset = c(color, RESET),
                        yellow = c(color, YELLOW), path = path_str,
                        dim = c(color, DIM), label = label);
                }
                continue;
            }
        }
        if matches!(section, Section::Untracked) {
            if !trimmed.is_empty() {
                println!("  {red}+{reset}  {path}",
                    red = c(color, RED), reset = c(color, RESET), path = trimmed);
                continue;
            }
        }
        if matches!(section, Section::Conflict) {
            if let Some((label, path)) = split_status_line(&line) {
                println!("  {red}⚠{reset}  {red}{path}{reset}  {dim}({label}){reset}",
                    red = c(color, RED), reset = c(color, RESET),
                    path = path.trim(), dim = c(color, DIM), label = label);
                continue;
            }
        }
        // dòng trống / khác — bỏ qua để output gọn
    }
}

#[derive(PartialEq, Eq, Clone, Copy)]
enum Section { None, Staged, Modified, Untracked, Conflict }

fn split_status_line(line: &str) -> Option<(String, String)> {
    // line dạng "\tmodified:   path" hoặc "        modified:   path"
    let l = line.trim_start();
    let colon = l.find(':')?;
    let label = l[..colon].to_string();
    if label.contains(' ') && !label.starts_with("new file") {
        // không phải status label
        return None;
    }
    let path = l[colon + 1..].trim().to_string();
    if path.is_empty() {
        return None;
    }
    Some((label, path))
}

fn icon_for_status(label: &str, _section: &Section, _color: bool) -> (&'static str, &'static str) {
    match label {
        "modified" => ("✎", YELLOW),
        "new file" => ("+", GREEN),
        "deleted" => ("✗", RED),
        "renamed" => ("→", CYAN),
        "copied" => ("⇒", CYAN),
        "typechange" => ("⇄", MAGENTA),
        "both modified" | "both added" | "both deleted" => ("⚠", RED),
        _ => ("•", DIM),
    }
}

fn print_kv(color: bool, label: &str, value: &str, val_color: &str) {
    println!("  {dim}{label:>8}{reset}  {col}{value}{reset}",
        dim = c(color, DIM), label = label, reset = c(color, RESET),
        col = c(color, val_color), value = value);
}

/// Loại bỏ ANSI escape (vì git -c color.ui=always có thể đã thêm).
fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_esc = false;
    for ch in s.chars() {
        if in_esc {
            if ch == 'm' { in_esc = false; }
            continue;
        }
        if ch == '\x1b' {
            in_esc = true;
            continue;
        }
        out.push(ch);
    }
    out
}

fn pretty_git_branch(out: &str, color: bool) {
    // Output `git branch`:
    //   * main
    //     feature/x
    //     bugfix/y
    // Với -a:
    //     remotes/origin/main
    // Với -vv kèm SHA và tracking
    for raw in out.lines() {
        let line = strip_ansi(raw);
        let is_current = line.starts_with('*');
        let trimmed = line.trim_start_matches('*').trim_start();
        if trimmed.is_empty() { continue; }
        // Tách phần đầu (branch name) và phần đuôi (SHA + tracking)
        let mut parts_iter = trimmed.splitn(2, char::is_whitespace);
        let name = parts_iter.next().unwrap_or("");
        let extra = parts_iter.next().unwrap_or("").trim();
        let is_remote = name.starts_with("remotes/") || name.contains("HEAD ->");
        let prefix = if is_current { "●" } else { " " };
        let name_color = if is_current { BRIGHT_GREEN }
                         else if is_remote { MAGENTA }
                         else { CYAN };
        let pad_name = if is_current { BOLD } else { "" };
        if extra.is_empty() {
            println!("  {col_p}{prefix}{reset}  {bold}{col}{name}{reset}",
                col_p = c(color, GREEN), prefix = prefix, reset = c(color, RESET),
                bold = c(color, pad_name), col = c(color, name_color), name = name);
        } else {
            println!("  {col_p}{prefix}{reset}  {bold}{col}{name}{reset}  {dim}{extra}{reset}",
                col_p = c(color, GREEN), prefix = prefix, reset = c(color, RESET),
                bold = c(color, pad_name), col = c(color, name_color), name = name,
                dim = c(color, DIM), extra = extra);
        }
    }
}

fn pretty_du(out: &str, color: bool) {
    let lines: Vec<&str> = out.lines().filter(|l| !l.trim().is_empty()).collect();
    if lines.is_empty() {
        return;
    }
    // Tính max độ rộng cột size
    let parsed: Vec<(String, String)> = lines.iter().map(|l| {
        let mut it = l.splitn(2, char::is_whitespace);
        let size = it.next().unwrap_or("").to_string();
        let path = it.next().unwrap_or("").trim().to_string();
        (size, path)
    }).collect();

    let wsize = parsed.iter().map(|(s, _)| s.len()).max().unwrap_or(0);
    for (size, path) in parsed {
        let size_c = colorize_size(&size, color);
        let pad = " ".repeat(wsize.saturating_sub(visible_len(&size)));
        let path_c = if color {
            // Phân biệt thư mục/file
            let p = std::path::Path::new(&path);
            if p.is_dir() {
                format!("{}{}{}", BRIGHT_BLUE, path, RESET)
            } else {
                path.clone()
            }
        } else {
            path.clone()
        };
        println!("{}{}  {}", size_c, pad, path_c);
    }
}
