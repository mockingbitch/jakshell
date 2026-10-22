use anyhow::Result;
use serde::Deserialize;
use std::cell::RefCell;
use std::rc::Rc;

use crate::shell::Shell;

#[derive(Debug, Default, Deserialize)]
struct RcConfig {
    #[serde(default)]
    prompt: Option<String>,
    #[serde(default)]
    theme: Option<crate::theme::Theme>,
    #[serde(default)]
    timing: Option<crate::shell::TimingConfig>,
    #[serde(default)]
    greeting: Option<crate::shell::GreetingConfig>,
    #[serde(default)]
    ui: Option<crate::shell::UiConfig>,
    #[serde(default)]
    history: Option<crate::shell::HistoryConfig>,
    #[serde(default)]
    update: Option<crate::shell::UpdateConfig>,
    #[serde(default)]
    news: Option<crate::shell::NewsConfig>,
    #[serde(default)]
    aliases: std::collections::HashMap<String, String>,
    #[serde(default)]
    env: std::collections::HashMap<String, String>,
}

pub fn load(shell: &Rc<RefCell<Shell>>) -> Result<()> {
    let toml_path = shell.borrow().rc_toml_path();
    if toml_path.exists() {
        let content = std::fs::read_to_string(&toml_path)?;
        let cfg: RcConfig = toml::from_str(&content)?;
        if let Some(prompt) = cfg.prompt {
            shell.borrow_mut().prompt_template = prompt;
        }
        if let Some(theme) = cfg.theme {
            shell.borrow_mut().theme = theme;
        }
        if let Some(timing) = cfg.timing {
            shell.borrow_mut().timing = timing;
        }
        if let Some(greeting) = cfg.greeting {
            shell.borrow_mut().greeting = greeting;
        }
        if let Some(ui) = cfg.ui {
            shell.borrow_mut().ui = ui;
        }
        if let Some(history) = cfg.history {
            shell.borrow_mut().history_cfg = history;
        }
        if let Some(update) = cfg.update {
            shell.borrow_mut().update = update;
        }
        if let Some(news) = cfg.news {
            shell.borrow_mut().news = news;
        }
        for (k, v) in cfg.aliases {
            shell.borrow_mut().aliases.insert(k, v);
        }
        for (k, v) in cfg.env {
            // Mục [env] là biến môi trường → export cho lệnh con.
            if let Err(e) = shell.borrow_mut().export_var(&k, Some(&v)) {
                eprintln!("jaksh: [env] {e}");
            }
        }
    }

    let script_path = shell.borrow().rc_script_path();
    if script_path.exists() {
        let content = std::fs::read_to_string(&script_path)?;
        run_rc(shell, &content)?;
    }

    // Theme đã lưu qua `jak theme <name>` được áp CUỐI cùng (last-wins).
    if let Some(name) = crate::jak::saved_theme_name() {
        if let Some(t) = crate::theme::by_name(&name) {
            shell.borrow_mut().theme = t;
        }
    }

    // Ngôn ngữ đã chọn qua `jak lang <code>`.
    crate::i18n::load_from_disk();
    Ok(())
}

/// Chạy nội dung script (`source`, file script, stdin). Lỗi cú pháp: chạy các
/// khối lệnh hợp lệ TRƯỚC chỗ lỗi rồi dừng với exit 2 — như bash (chạy tiếp
/// phần sau có thể nguy hiểm, vd `if` bảo vệ `rm` bị hỏng).
pub fn run_script(shell: &Rc<RefCell<Shell>>, content: &str) -> Result<i32> {
    run_source(shell, content, false)
}

/// `.jakshrc`: khối lỗi được bỏ qua, các khối sau vẫn chạy — 1 dòng sai không
/// làm mất cả cấu hình.
fn run_rc(shell: &Rc<RefCell<Shell>>, content: &str) -> Result<i32> {
    run_source(shell, content, true)
}

fn run_source(shell: &Rc<RefCell<Shell>>, content: &str, tolerant: bool) -> Result<i32> {
    let content = crate::check::blank_shebang(content);
    // Đường nhanh: cả file hợp lệ → parse 1 lượt.
    if let Ok(ast) = crate::lexer::tokenize(&content).and_then(|t| crate::parser::parse(&t)) {
        return crate::executor::execute(shell, &ast);
    }
    let mut code = 0;
    for block in crate::check::blocks(&content) {
        match block {
            crate::check::Block::Code(line, text) => {
                shell.borrow_mut().env.insert("LINENO".into(), line.to_string());
                let ast = crate::lexer::tokenize(&text).and_then(|t| crate::parser::parse(&t))?;
                code = crate::executor::execute(shell, &ast)?;
                if shell.borrow().flow != crate::shell::Flow::Normal {
                    break;
                }
            }
            crate::check::Block::Error(line, msg) => {
                eprintln!("jaksh: dòng {}: lỗi cú pháp: {}", line, msg);
                if !tolerant {
                    return Ok(2);
                }
                code = 2;
            }
        }
    }
    shell.borrow_mut().env.remove("LINENO");
    Ok(code)
}
