mod bookmark;
mod builtins;
mod completion;
mod config;
mod executor;
mod expand;
mod explain;
mod findcmd;
mod jak;
mod lexer;
mod parser;
mod pretty;
mod prompt;
mod shell;
mod suggest;
mod theme;

use anyhow::Result;
use chrono::{Datelike, Timelike};
use rustyline::error::ReadlineError;
use rustyline::Editor;
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
    let mut rl: Editor<ShellHelper, rustyline::history::FileHistory> = Editor::new()?;
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
            }
            Err(ReadlineError::Interrupted) => {
                // Ctrl-C: clear current line, continue
                continue;
            }
            Err(ReadlineError::Eof) => {
                // Ctrl-D: exit
                println!("tạm biệt 👋");
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
        let (period, icon) = greeting_period(now.hour());
        let name = if !g.name.is_empty() {
            g.name.clone()
        } else {
            std::env::var("USER").unwrap_or_else(|_| "bạn".into())
        };
        let weekday = weekday_vi(now.weekday());
        let date = now.format("%d/%m/%Y");
        let time = now.format("%H:%M");
        println!(
            "{icon} Chào {period}, {accent}{name}{reset}!  {dim}{weekday}, {date} · {time}{reset}",
        );
    }

    if g.show_tip {
        let tip = pick_tip();
        println!("{dim}💡 {tip}{reset}");
    }

    println!("{dim}Gõ {reset}{accent}?{reset}{dim} hoặc {reset}{accent}help{reset}{dim} bất cứ lúc nào.{reset}");
}

fn greeting_period(hour: u32) -> (&'static str, &'static str) {
    match hour {
        5..=10 => ("buổi sáng", "🌅"),
        11..=12 => ("buổi trưa", "☀️ "),
        13..=17 => ("buổi chiều", "🌤 "),
        18..=21 => ("buổi tối", "🌆"),
        _ => ("đêm khuya", "🌙"),
    }
}

fn weekday_vi(d: chrono::Weekday) -> &'static str {
    use chrono::Weekday::*;
    match d {
        Mon => "Thứ Hai",
        Tue => "Thứ Ba",
        Wed => "Thứ Tư",
        Thu => "Thứ Năm",
        Fri => "Thứ Sáu",
        Sat => "Thứ Bảy",
        Sun => "Chủ Nhật",
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
        "Cấu hình tại `~/.jakshrc.toml` — prompt, theme, alias, env, timing, greeting.",
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
