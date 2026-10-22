mod arith;
mod bookmark;
mod builtins;
mod check;
mod completion;
mod cond;
mod config;
mod executor;
mod expand;
mod explain;
mod findcmd;
mod highlight;
mod histstore;
mod fuzzy;
mod history;
mod i18n;
mod info;
mod jak;
mod jobctl;
mod lexer;
mod markdown;
mod news;
mod parser;
mod pretty;
mod prompt;
mod redirect;
mod safety;
mod shell;
mod suggest;
mod theme;
mod update;

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
    // ── CLI args parsing ──────────────────────────────────────────────────────
    // Hỗ trợ subset của bash/zsh để JakShell dùng được làm shell trong VSCode,
    // task runner, CI, login shell, v.v.
    //
    //   jaksh                          → REPL tương tác (mặc định)
    //   jaksh -c "cmd"                 → chạy 1 lệnh rồi exit (không banner/timing)
    //   jaksh script.sh [args...]      → chạy script từng dòng rồi exit
    //   jaksh -l / --login             → flag tương thích (no-op, accept không lỗi)
    //   jaksh -i / --interactive       → ép interactive (mặc định khi không có -c/script)
    //   jaksh -V / --version           → in version rồi exit
    //   jaksh -h / --help              → in help rồi exit
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mode = parse_cli(&args);
    match mode {
        CliMode::Help => {
            print_cli_help();
            return Ok(());
        }
        CliMode::Version => {
            println!("JakShell {}", env!("JAKSH_VERSION"));
            return Ok(());
        }
        CliMode::Command(cmd, rest) => {
            return run_oneshot(&cmd, &rest);
        }
        CliMode::Script(path, script_args) => {
            return run_script(&path, &script_args);
        }
        CliMode::Check(paths) => {
            std::process::exit(check::run(&paths));
        }
        CliMode::UpdateRefresh => {
            update::refresh_blocking();
            return Ok(());
        }
        CliMode::Interactive => {} // tiếp tục xuống REPL bên dưới
    }

    // stdin không phải terminal (`echo 'ls' | jaksh`, `jaksh < script`) → chạy
    // như script: không banner, không prompt, output sạch.
    if unsafe { libc::isatty(libc::STDIN_FILENO) } != 1 {
        let mut content = String::new();
        std::io::Read::read_to_string(&mut std::io::stdin(), &mut content)?;
        let shell = Rc::new(RefCell::new(Shell::new()?));
        if let Err(e) = config::load(&shell) {
            eprintln!("jaksh: lỗi khi nạp cấu hình: {e}");
        }
        let code = config::run_script(&shell, &content).unwrap_or(1);
        executor::run_exit_trap(&shell);
        std::process::exit(code);
    }

    // Job control: shell giữ terminal, miễn nhiễm Ctrl-C/Ctrl-Z; mỗi lệnh chạy
    // trong process group riêng. Phải bật trước mọi thứ có thể spawn lệnh con.
    jobctl::init();

    let shell = Rc::new(RefCell::new(Shell::new()?));

    if let Err(e) = config::load(&shell) {
        eprintln!("\x1b[33mjaksh: lỗi khi nạp cấu hình: {e}\x1b[0m");
    }

    let helper = ShellHelper::new(Rc::clone(&shell));
    // List-mode completion: Tab 1 → in danh sách candidates + complete common prefix.
    // Tab tiếp theo → cycle qua từng option.
    let history_size = shell.borrow().history_cfg.size.max(1);
    let config = Config::builder()
        .completion_type(CompletionType::List)
        .auto_add_history(false)
        .max_history_size(history_size)?
        .history_ignore_dups(true)?
        .history_ignore_space(true)
        .build();
    let mut rl: Editor<ShellHelper, rustyline::history::FileHistory> =
        Editor::with_config(config)?;
    rl.set_helper(Some(helper));
    // Ctrl-X Ctrl-E: sửa dòng đang gõ trong $EDITOR rồi chạy (như bash).
    rl.bind_sequence(
        rustyline::Event::KeySeq(vec![rustyline::KeyEvent::ctrl('X'), rustyline::KeyEvent::ctrl('E')]),
        rustyline::EventHandler::Conditional(Box::new(PromptAction::Edit)),
    );
    // Ctrl-R: tìm lịch sử kiểu fuzzy (thay tìm kiếm tuần tự của readline).
    rl.bind_sequence(
        rustyline::KeyEvent::ctrl('R'),
        rustyline::EventHandler::Conditional(Box::new(PromptAction::Search)),
    );
    if shell.borrow().ui.edit_mode == "vi" {
        use rustyline::config::Configurer;
        rl.set_edit_mode(rustyline::EditMode::Vi);
    }

    // Lịch sử: kho JSONL có metadata, chia sẻ giữa các phiên. rustyline chỉ giữ
    // bản trong bộ nhớ cho ↑ / ↓ — không tự ghi file nữa.
    let (mut store, loaded) = {
        let sh = shell.borrow();
        histstore::Store::open(sh.history_store_path(), &sh.history_path(), history_size)
    };
    for e in &loaded {
        let _ = rl.add_history_entry(e.cmd.as_str());
    }
    shell.borrow_mut().history = loaded;

    print_welcome(&shell);

    // Kiểm tra phiên bản mới: chạy nền cho lần sau + (nếu cache báo có bản mới)
    // thông báo & hỏi cập nhật. Đặt sau banner, trước prompt đầu tiên.
    update::startup_check(&shell);

    let mut first_prompt = true;
    // Lệnh chọn từ Ctrl-R — đặt sẵn lên prompt kế tiếp để sửa / Enter.
    let mut initial: Option<String> = None;
    loop {
        if first_prompt {
            first_prompt = false;
        } else if initial.is_none() {
            print_block_separator(&shell.borrow());
        }
        // Lệnh các phiên khác vừa chạy: luôn vào `history` / Ctrl-R; vào ↑ khi share.
        let share = shell.borrow().history_cfg.share;
        for e in store.poll() {
            if share {
                let _ = rl.add_history_entry(e.cmd.as_str());
            }
            shell.borrow_mut().history.push(e);
        }

        let prompt_str = prompt::render(&shell.borrow());
        let raw = match initial.take() {
            Some(text) => rl.readline_with_initial(&prompt_str, (&text, "")),
            None => rl.readline(&prompt_str),
        };
        let readline = match raw {
            Err(ReadlineError::Interrupted) => match take_prompt_action() {
                // Ctrl-X Ctrl-E: mở editor, chạy nội dung sau khi sửa.
                Some((PromptAction::Edit, text)) => match builtins::edit_in_editor(&shell, &text) {
                    Ok(Some(edited)) if !edited.trim().is_empty() => {
                        println!("{}", edited);
                        Ok(edited)
                    }
                    Ok(_) => continue,
                    Err(e) => {
                        eprintln!("\x1b[31mjaksh: {e}\x1b[0m");
                        continue;
                    }
                },
                // Ctrl-R: chọn lệnh rồi đặt lên prompt (chưa chạy).
                Some((PromptAction::Search, query)) => {
                    let (cwd, color) = {
                        let sh = shell.borrow();
                        (sh.cwd.display().to_string(), sh.theme.use_color)
                    };
                    let picked = fuzzy::pick(&shell.borrow().history, &query, &cwd, color);
                    initial = Some(picked.unwrap_or(query));
                    continue;
                }
                None => Err(ReadlineError::Interrupted),
            },
            other => other,
        };
        match readline {
            Ok(line) => {
                let line = line.trim_end_matches('\n').to_string();
                if line.trim().is_empty() {
                    continue;
                }
                // `!!`, `!$`, `!N`, … — mở rộng TRƯỚC khi lưu (như bash: lịch sử
                // giữ lệnh đã mở rộng) và in lại dòng thật sẽ chạy.
                let line = {
                    let past: Vec<String> = rl.history().iter().cloned().collect();
                    match history::expand(&line, &past) {
                        Ok(Some(expanded)) => {
                            println!("{}", expanded);
                            expanded
                        }
                        Ok(None) => line,
                        Err(e) => {
                            eprintln!("\x1b[31mjaksh: {e}\x1b[0m");
                            shell.borrow_mut().set_last_status(1);
                            continue;
                        }
                    }
                };
                // Paste nhiều dòng: mỗi lệnh 1 entry (gọn cho ↑ / Ctrl-R), còn
                // dòng nối bằng `\` / khối nhiều dòng gộp thành 1 entry.
                let cwd = shell.borrow().cwd.display().to_string();
                let filter = shell.borrow().history_cfg.secret_filter;
                let mut pending: Vec<histstore::Entry> = Vec::new();
                for entry in history::entries(&line) {
                    match history::secret_kind(&entry).filter(|_| filter) {
                        Some(kind) => eprintln!(
                            "\x1b[33m🔒 Lệnh chứa {kind} — không lưu vào lịch sử.\x1b[0m \x1b[2m(tắt: [history] secret_filter = false)\x1b[0m"
                        ),
                        // Dấu cách đầu dòng = không lưu (như HISTCONTROL=ignorespace).
                        None if line.starts_with(' ') => {}
                        None => {
                            rl.add_history_entry(entry.as_str()).ok();
                            pending.push(histstore::Entry::new(&entry, &cwd));
                        }
                    }
                }
                // Có ngay trong `history` khi lệnh đang chạy (như bash).
                shell.borrow_mut().history.extend(pending.iter().cloned());

                // Báo job nền đã xong / bị dừng trong lúc chờ ở prompt (bash in
                // trước khi chạy lệnh kế tiếp).
                shell.borrow_mut().reap_jobs();
                if line.trim() != "exit" && !line.trim().starts_with("exit ") {
                    jobctl::reset_exit_warning();
                }
                jobctl::take_interrupted();

                // Lệnh nguy hiểm (`rm -rf /`, `chmod -R 777 /`…): hỏi trước khi chạy.
                if shell.borrow().ui.confirm_dangerous {
                    if let Some(reason) = safety::danger(&line) {
                        eprintln!("\x1b[1;31m⚠ Lệnh nguy hiểm: {reason}\x1b[0m");
                        let answer = rl.readline("\x1b[33m  Vẫn chạy? [y/N] \x1b[0m").unwrap_or_default();
                        if !matches!(answer.trim().to_lowercase().as_str(), "y" | "yes" | "có" | "co") {
                            eprintln!("\x1b[2m  Đã huỷ. (tắt hỏi: [ui] confirm_dangerous = false)\x1b[0m");
                            shell.borrow_mut().set_last_status(1);
                            continue;
                        }
                    }
                }

                let started = Instant::now();
                let result = run_line(&shell, &line);
                let elapsed = started.elapsed();
                let code = match &result {
                    Ok(code) => *code,
                    Err(_) => 1,
                };

                // Ghi lịch sử kèm exit code + thời lượng (trừ khi lệnh vừa xoá lịch sử).
                let cleared = shell.borrow().history_clear_requested;
                if !cleared {
                    let ms = elapsed.as_millis() as u64;
                    let mut sh = shell.borrow_mut();
                    for mut e in pending {
                        e.code = Some(code);
                        e.ms = Some(ms);
                        store.append(&e);
                        if let Some(slot) =
                            sh.history.iter_mut().rev().find(|h| h.cmd == e.cmd && h.ts == e.ts && h.code.is_none())
                        {
                            *slot = e;
                        }
                    }
                }

                let mut rewrite = false;
                // `history --scrub`: lọc lại toàn bộ lịch sử, ghi đè file.
                if std::mem::take(&mut shell.borrow_mut().history_scrub_requested) {
                    let mut sh = shell.borrow_mut();
                    let before = sh.history.len();
                    sh.history.retain(|e| history::secret_kind(&e.cmd).is_none());
                    let removed = before - sh.history.len();
                    rewrite = true;
                    println!("🔒 Đã xoá {removed} lệnh chứa token/mật khẩu khỏi lịch sử.");
                    if removed > 0 {
                        println!("\x1b[33m   Lưu ý: token đã từng nằm trong file — nên thu hồi (revoke) chúng.\x1b[0m");
                    }
                }
                // `history -d N`. Lấy ra biến riêng: `if let` giữ RefMut tạm suốt
                // cả khối → borrow lồng panic.
                let delete_request = shell.borrow_mut().history_delete_request.take();
                if let Some(idx) = delete_request {
                    let mut sh = shell.borrow_mut();
                    if idx < sh.history.len() {
                        sh.history.remove(idx);
                    }
                    rewrite = true;
                }
                // `history -c`.
                if std::mem::take(&mut shell.borrow_mut().history_clear_requested) {
                    shell.borrow_mut().history.clear();
                    rewrite = true;
                }
                if rewrite {
                    store.rewrite(&shell.borrow().history);
                    let _ = rl.clear_history();
                    for e in shell.borrow().history.iter() {
                        let _ = rl.add_history_entry(e.cmd.as_str());
                    }
                }
                // Lệnh `fc` vừa chạy được ghi vào lịch sử.
                let added = std::mem::take(&mut shell.borrow_mut().history_add_request);
                for cmd in added {
                    if history::secret_kind(&cmd).is_some() && filter {
                        continue;
                    }
                    let _ = rl.add_history_entry(cmd.as_str());
                    let mut e = histstore::Entry::new(&cmd, &cwd);
                    e.code = Some(code);
                    store.append(&e);
                    shell.borrow_mut().history.push(e);
                }
                // `set -o vi` / `set -o emacs`.
                let mode_request = shell.borrow_mut().edit_mode_request.take();
                if let Some(mode) = mode_request {
                    use rustyline::config::Configurer;
                    rl.set_edit_mode(if mode == "vi" { rustyline::EditMode::Vi } else { rustyline::EditMode::Emacs });
                }

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
                // Ctrl-D: exit (còn job đang dừng → cảnh báo trước, như bash)
                if !jobctl::confirm_exit(&shell) {
                    continue;
                }
                println!("{} 👋", i18n::t("common.goodbye"));
                break;
            }
            Err(e) => {
                eprintln!("\x1b[31mjaksh: lỗi đọc dòng lệnh: {e}\x1b[0m");
                break;
            }
        }
    }

    executor::run_exit_trap(&shell);
    jobctl::shutdown(&shell);
    Ok(())
}

/// Phím đặc biệt ở prompt: lưu dòng đang gõ rồi thoát readline (Interrupt) để
/// vòng lặp chính làm tiếp — handler của rustyline phải Send + Sync.
#[derive(Clone, Copy)]
enum PromptAction {
    /// Ctrl-X Ctrl-E
    Edit,
    /// Ctrl-R
    Search,
}

static PROMPT_ACTION: std::sync::Mutex<Option<(u8, String)>> = std::sync::Mutex::new(None);

fn take_prompt_action() -> Option<(PromptAction, String)> {
    let (kind, text) = PROMPT_ACTION.lock().ok()?.take()?;
    Some((if kind == 0 { PromptAction::Edit } else { PromptAction::Search }, text))
}

impl rustyline::ConditionalEventHandler for PromptAction {
    fn handle(
        &self,
        _evt: &rustyline::Event,
        _n: rustyline::RepeatCount,
        _positive: bool,
        ctx: &rustyline::EventContext,
    ) -> Option<rustyline::Cmd> {
        if let Ok(mut slot) = PROMPT_ACTION.lock() {
            let kind = match self {
                PromptAction::Edit => 0,
                PromptAction::Search => 1,
            };
            *slot = Some((kind, ctx.line().to_string()));
        }
        Some(rustyline::Cmd::Interrupt)
    }
}

fn run_line(shell: &Rc<RefCell<Shell>>, line: &str) -> Result<i32> {
    let tokens = lexer::tokenize(line)?;
    let ast = parser::parse(&tokens)?;
    executor::execute(shell, &ast)
}

/// Tách các khối lệnh trong REPL cho dễ nhìn lại output cũ: in dòng trống
/// và/hoặc đường kẻ mờ TRƯỚC mỗi prompt (trừ prompt đầu tiên sau banner).
fn print_block_separator(shell: &Shell) {
    let ui = &shell.ui;
    if ui.blank_line {
        println!();
    }
    if ui.separator == "line" {
        let width = crossterm::terminal::size()
            .map(|(w, _)| w as usize)
            .unwrap_or(80);
        let dim = if shell.theme.use_color { "\x1b[2m" } else { "" };
        let reset = if shell.theme.use_color { "\x1b[0m" } else { "" };
        println!("{dim}{}{reset}", "─".repeat(width));
    }
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

// ─── CLI mode parsing ─────────────────────────────────────────────────────────

enum CliMode {
    /// REPL tương tác (mặc định).
    Interactive,
    /// `-c "cmd" [tên [args...]]` — chạy 1 lệnh rồi exit (`$0` = tên, như bash).
    Command(String, Vec<String>),
    /// Script file + args truyền vào.
    Script(String, Vec<String>),
    Help,
    Version,
    /// `--check` / `-n` — kiểm tra cú pháp script, không chạy.
    Check(Vec<String>),
    /// `--update-refresh` (nội bộ) — tiến trình nền: gọi mạng lấy tag mới nhất,
    /// ghi cache rồi thoát. Không banner, không REPL.
    UpdateRefresh,
}

fn parse_cli(args: &[String]) -> CliMode {
    let mut i = 0;
    while i < args.len() {
        let a = args[i].as_str();
        match a {
            "-c" => {
                let Some(cmd) = args.get(i + 1) else {
                    eprintln!("jaksh: `-c` cần argument: lệnh để chạy");
                    std::process::exit(2);
                };
                return CliMode::Command(cmd.clone(), args[i + 2..].to_vec());
            }
            // Flag tương thích bash/zsh — login / interactive đều no-op vì JakShell
            // luôn nạp .jakshrc khi mở.
            "-l" | "--login" | "-i" | "--interactive" => {}
            "-V" | "--version" => return CliMode::Version,
            "-h" | "--help" => return CliMode::Help,
            "--update-refresh" => return CliMode::UpdateRefresh,
            "--check" | "-n" => return CliMode::Check(args[i + 1..].to_vec()),
            "--" => {
                if let Some((path, rest)) = args[i + 1..].split_first() {
                    return CliMode::Script(path.clone(), rest.to_vec());
                }
                return CliMode::Interactive;
            }
            other if other.starts_with('-') => {
                eprintln!("jaksh: tham số không nhận: {other}");
                eprintln!("Chạy `jaksh --help` để xem help.");
                std::process::exit(2);
            }
            _ => {
                // Positional: script file + remaining args
                let path = args[i].clone();
                let rest: Vec<String> = args[i + 1..].to_vec();
                return CliMode::Script(path, rest);
            }
        }
        i += 1;
    }
    CliMode::Interactive
}

fn print_cli_help() {
    println!("JakShell {} — shell nhanh, gọn, thân thiện", env!("JAKSH_VERSION"));
    println!();
    println!("USAGE:");
    println!("  jaksh                          REPL tương tác (mặc định)");
    println!("  jaksh -c \"<lệnh>\"              chạy 1 lệnh rồi exit");
    println!("  jaksh <script.sh> [args...]    chạy script rồi exit");
    println!("  jaksh --check <script.sh>...   kiểm tra cú pháp (báo số dòng), không chạy  (= -n)");
    println!();
    println!("OPTIONS:");
    println!("  -c <cmd>           Chạy <cmd> qua lexer/parser rồi exit");
    println!("  -l, --login        Login shell mode (no-op, chấp nhận cho tương thích)");
    println!("  -i, --interactive  Ép interactive (mặc định nếu không có -c/script)");
    println!("  -V, --version      In phiên bản rồi exit");
    println!("  -h, --help         In help này rồi exit");
    println!();
    println!("Khi dùng trong VSCode / task runner / CI, dạng `jaksh -c \"...\"` là cách");
    println!("chuẩn để chạy 1 lệnh từ tiến trình khác — banner, greeting, timing đều bị");
    println!("tắt, output sạch.");
}

/// Chạy 1 dòng lệnh non-interactive: nạp config, parse + execute, exit với mã trả về.
/// KHÔNG in banner / greeting / timing — output phải sạch cho tool gọi (VSCode, CI, ...).
fn run_oneshot(line: &str, args: &[String]) -> Result<()> {
    let shell = Rc::new(RefCell::new(Shell::new()?));
    if let Err(e) = config::load(&shell) {
        eprintln!("\x1b[33mjaksh: lỗi khi nạp cấu hình: {e}\x1b[0m");
    }
    {
        let mut sh = shell.borrow_mut();
        if let Some((name, rest)) = args.split_first() {
            sh.arg0 = name.clone();
            sh.positional = rest.to_vec();
        }
    }
    let code = match run_line(&shell, line) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("\x1b[31mjaksh: {e}\x1b[0m");
            2
        }
    };
    executor::run_exit_trap(&shell);
    std::process::exit(code);
}

/// Chạy file script: `$0` = đường dẫn, `$1..` = đối số. Parse cả file một lượt
/// (chuỗi / here-doc nhiều dòng hoạt động) — xem `config::run_script`.
fn run_script(path: &str, args: &[String]) -> Result<()> {
    let shell = Rc::new(RefCell::new(Shell::new()?));
    if let Err(e) = config::load(&shell) {
        eprintln!("\x1b[33mjaksh: lỗi khi nạp cấu hình: {e}\x1b[0m");
    }
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("\x1b[31mjaksh: không đọc được {path}: {e}\x1b[0m");
            std::process::exit(127);
        }
    };
    {
        let mut sh = shell.borrow_mut();
        sh.arg0 = path.to_string();
        sh.positional = args.to_vec();
    }
    let code = config::run_script(&shell, &content).unwrap_or(1);
    executor::run_exit_trap(&shell);
    std::process::exit(code);
}
