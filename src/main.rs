mod bookmark;
mod builtins;
mod completion;
mod config;
mod executor;
mod expand;
mod explain;
mod findcmd;
mod i18n;
mod info;
mod jak;
mod lexer;
mod markdown;
mod parser;
mod pretty;
mod prompt;
mod shell;
mod suggest;
mod theme;

use anyhow::Result;
use chrono::{Datelike, Timelike};
use rustyline::error::ReadlineError;
use rustyline::{CompletionType, Config, Editor};
use std::cell::RefCell;
use std::rc::Rc;
use std::time::Instant;

use crate::completion::ShellHelper;
use crate::shell::Shell;

fn main() -> Result<()> {
    let shell = Rc::new(RefCell::new(Shell::new()?));

    if let Err(e) = config::load(&shell) {
        eprintln!("\x1b[33mjaksh: lỗi khi nạp cấu hình: {e}\x1b[0m");
    }

    let helper = ShellHelper::new(Rc::clone(&shell));
    // List-mode completion: Tab 1 → in danh sách candidates + complete common prefix.
    // Tab tiếp theo → cycle qua từng option.
    let config = Config::builder()
        .completion_type(CompletionType::List)
        .auto_add_history(false)
        .history_ignore_dups(true)?
        .history_ignore_space(true)
        .build();
    let mut rl: Editor<ShellHelper, rustyline::history::FileHistory> =
        Editor::with_config(config)?;
    rl.set_helper(Some(helper));

    let history_path = shell.borrow().history_path();
    let _ = rl.load_history(&history_path);

    print_welcome(&shell);

    loop {
        let prompt_str = prompt::render(&shell.borrow());
        match rl.readline(&prompt_str) {
            Ok(line) => {
                let line = line.trim_end_matches('\n').to_string();
                if line.trim().is_empty() {
                    continue;
                }
                rl.add_history_entry(line.as_str()).ok();

                let started = Instant::now();
                let result = run_line(&shell, &line);
                let elapsed = started.elapsed();

                let code = match result {
                    Ok(code) => {
                        if code == 127 {
                            suggest::maybe_suggest(&shell.borrow(), &line);
                        }
                        shell.borrow_mut().set_last_status(code);
                        code
                    }
                    Err(e) => {
                        eprintln!("\x1b[31mjaksh: {e}\x1b[0m");
                        suggest::maybe_suggest(&shell.borrow(), &line);
                        shell.borrow_mut().set_last_status(1);
                        1
                    }
                };
                print_timing(&shell.borrow(), elapsed, code);
                print_failure_hint(&shell.borrow(), &line, code);
            }
            Err(ReadlineError::Interrupted) => {
                // Ctrl-C: clear current line, continue
                continue;
            }
            Err(ReadlineError::Eof) => {
                // Ctrl-D: exit
                println!("{} 👋", i18n::t("common.goodbye"));
                break;
            }
            Err(e) => {
                eprintln!("\x1b[31mjaksh: lỗi đọc dòng lệnh: {e}\x1b[0m");
                break;
            }
        }
    }

    let _ = rl.save_history(&history_path);
    Ok(())
}

fn run_line(shell: &Rc<RefCell<Shell>>, line: &str) -> Result<i32> {
    let tokens = lexer::tokenize(line)?;
    let ast = parser::parse(&tokens)?;
    executor::execute(shell, &ast)
}

fn print_timing(shell: &Shell, elapsed: std::time::Duration, code: i32) {
    let cfg = &shell.timing;
    if !cfg.enabled {
        return;
    }
    let ms = elapsed.as_millis() as u64;
    if ms < cfg.threshold_ms {
        return;
    }
    let duration = format_duration(elapsed);
    let dim = if shell.theme.use_color { "\x1b[2m" } else { "" };
    let red = if shell.theme.use_color { "\x1b[31m" } else { "" };
    let reset = if shell.theme.use_color { "\x1b[0m" } else { "" };
    if cfg.show_status && code != 0 {
        eprintln!("{dim}⏱  {duration}{reset}  {red}✗ exit {code}{reset}");
    } else {
        eprintln!("{dim}⏱  {duration}{reset}");
    }
}

/// Lệnh "fail mềm" — exit non-zero là chuyện thường (no match / no diff / …),
/// KHÔNG phải lỗi thực sự → không cần in hint.
const BENIGN_NONZERO_COMMANDS: &[&str] = &[
    "grep", "egrep", "fgrep", "rg", "ripgrep", "ag",
    "diff", "cmp",
    "test", "[", "[[",
    "false",  // false luôn return 1 cố ý
    "true",
];

fn print_failure_hint(shell: &Shell, line: &str, code: i32) {
    if !shell.timing.show_hint || code == 0 {
        return;
    }
    // 127 đã được suggest module xử lý — bỏ qua để không trùng.
    if code == 127 {
        return;
    }
    // Lệnh "non-zero by design" — skip.
    let cmd = first_command(line);
    if BENIGN_NONZERO_COMMANDS.contains(&cmd.as_str()) {
        return;
    }

    let dim = if shell.theme.use_color { "\x1b[2m" } else { "" };
    let yellow = if shell.theme.use_color { "\x1b[33m" } else { "" };
    let reset = if shell.theme.use_color { "\x1b[0m" } else { "" };
    let bold = if shell.theme.use_color { "\x1b[1m" } else { "" };

    let reason = explain_exit_code(code);
    eprintln!("{yellow}💡{reset} {dim}{reason}{reset}");

    // Gợi ý xem help — chỉ cho mã 1, 2, 64-78 (sysexits).
    if matches!(code, 1 | 2 | 64..=78) && !cmd.is_empty() {
        eprintln!(
            "   {dim}{}{reset} {bold}{cmd} --help{reset}{dim}  hoặc  {reset}{bold}man {cmd}{reset}",
            i18n::t("hint.try"),
        );
    }
    // Gợi ý chmod cho 126
    if code == 126 && !cmd.is_empty() {
        eprintln!(
            "   {dim}{}{reset} {bold}chmod +x <file>{reset}",
            i18n::t("hint.fix"),
        );
    }
}

fn first_command(line: &str) -> String {
    let trimmed = line.trim_start();
    // Bỏ qua biến môi trường prefix (VAR=val cmd ...)
    let mut iter = trimmed.split_whitespace().peekable();
    while let Some(tok) = iter.peek() {
        if tok.contains('=') && !tok.starts_with('-') && !tok.starts_with('=') {
            iter.next();
        } else {
            break;
        }
    }
    iter.next().unwrap_or("").to_string()
}

fn explain_exit_code(code: i32) -> String {
    let key = match code {
        1 => "hint.exit.1",
        2 => "hint.exit.2",
        126 => "hint.exit.126",
        128 => "hint.exit.128",
        130 => "hint.exit.130",
        137 => "hint.exit.137",
        139 => "hint.exit.139",
        143 => "hint.exit.143",
        n if (129..192).contains(&n) => {
            let sig = n - 128;
            return i18n::t("hint.exit.signal")
                .replace("{N}", &code.to_string())
                .replace("{SIG}", &sig.to_string());
        }
        _ => "hint.exit.other",
    };
    i18n::t(key).replace("{N}", &code.to_string())
}

fn format_duration(d: std::time::Duration) -> String {
    let total_ms = d.as_millis() as u64;
    if total_ms < 1 {
        let us = d.as_micros() as u64;
        return format!("{us} µs");
    }
    if total_ms < 1000 {
        return format!("{total_ms} ms");
    }
    let total_s = d.as_secs_f64();
    if total_s < 60.0 {
        return format!("{:.2} s", total_s);
    }
    let mins = (total_s as u64) / 60;
    let secs = total_s - (mins * 60) as f64;
    if mins < 60 {
        return format!("{mins}m {:.1}s", secs);
    }
    let hrs = mins / 60;
    let mins_rem = mins % 60;
    format!("{hrs}h {mins_rem}m {:.0}s", secs)
}

fn print_welcome(shell: &Rc<RefCell<Shell>>) {
    let s = shell.borrow();
    if !s.greeting.enabled {
        return;
    }
    let theme = &s.theme;
    let g = &s.greeting;

    let bold = "\x1b[1m";
    let dim = theme.dim_ansi();
    let accent = theme.accent_ansi();
    let reset = "\x1b[0m";

    println!(
        "{bold}{accent}JakShell{reset} {dim}{ver}{reset}  {dim}—{reset} shell nhanh, gọn, thân thiện",
        ver = env!("JAKSH_VERSION"),
    );

    if g.show_greeting {
        let now = chrono::Local::now();
        let (period_key, icon) = greeting_period(now.hour());
        let name = if !g.name.is_empty() {
            g.name.clone()
        } else {
            std::env::var("USER").unwrap_or_else(|_| "you".into())
        };
        let weekday = i18n::t(weekday_key(now.weekday()));
        let date = now.format("%d/%m/%Y");
        let time = now.format("%H:%M");
        let greeting = i18n::t(period_key);
        println!(
            "{icon} {greeting}, {accent}{name}{reset}!  {dim}{weekday}, {date} · {time}{reset}",
        );
    }

    if g.show_tip {
        let tip = pick_tip();
        println!("{dim}💡 {tip}{reset}");
    }

    println!("{dim}{}{reset}", i18n::t("banner.help_hint"));
}

fn greeting_period(hour: u32) -> (&'static str, &'static str) {
    match hour {
        5..=10 => ("greeting.morning", "🌅"),
        11..=12 => ("greeting.noon", "☀️ "),
        13..=17 => ("greeting.afternoon", "🌤 "),
        18..=21 => ("greeting.evening", "🌆"),
        _ => ("greeting.midnight", "🌙"),
    }
}

fn weekday_key(d: chrono::Weekday) -> &'static str {
    use chrono::Weekday::*;
    match d {
        Mon => "weekday.mon",
        Tue => "weekday.tue",
        Wed => "weekday.wed",
        Thu => "weekday.thu",
        Fri => "weekday.fri",
        Sat => "weekday.sat",
        Sun => "weekday.sun",
    }
}

fn pick_tip() -> &'static str {
    const TIPS: &[&str] = &[
        "Gõ `explain <lệnh>` để xem usage / tham số / ví dụ.",
        "Thử `explain ls -la` — JakShell sẽ chú thích từng cột trên output thật.",
        "Thêm `--jak` vào `ls`, `ps`, `df`, `git status` để tô màu & format đẹp.",
        "Tạo bookmark: `bookmark <tên> <lệnh>` · chạy: `jak <tên>`.",
        "Tìm file nhanh: `jak find <tên>` · tìm trong nội dung: `jak find text \"chuỗi\"`.",
        "Lưu commit nhanh: `jak git save \"msg\"` = `git add -A && git commit -m \"msg\"`.",
        "`jak git sync` = pull --rebase + push. `jak git uncommit` = huỷ commit cuối, giữ stage.",
        "Đổi giao diện: `jak theme ocean | forest | sunset | mono`.",
        "Xem thông tin máy: `jak sysinfo`. Xem IP: `jak ip`. Thời tiết: `jak weather`.",
        "Gõ sai lệnh? JakShell sẽ gợi ý 'có phải bạn muốn …?'.",
        "Quên lệnh? `jak help` liệt kê toàn bộ tiện ích · `explain` liệt kê 70+ lệnh đã có chú thích.",
        "Sau mỗi lệnh có dòng `⏱ X ms` — thời gian thực thi.",
        "Trong git repo: prompt hiện branch, *=dirty, ↑↓=ahead/behind, ⚑=stash.",
        "`jak find big` — tìm 20 file lớn nhất. `jak find recent` — sửa trong 24h qua.",
        "`Ctrl-R` để search lịch sử lệnh ngược (như bash/zsh).",
    ];
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos() as usize)
        .unwrap_or(0);
    TIPS[nanos % TIPS.len()]
}
