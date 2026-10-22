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
        run_script(shell, &content)?;
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

/// Chạy nội dung script (`.jakshrc`, `source`, file script). Parse CẢ FILE một
/// lượt để chuỗi / here-doc nhiều dòng hoạt động; nếu file có lỗi cú pháp thì
/// báo lỗi rồi rơi về chạy từng dòng — một dòng hỏng không chặn cả `.jakshrc`.
/// Trả về exit code của lệnh cuối.
pub fn run_script(shell: &Rc<RefCell<Shell>>, content: &str) -> Result<i32> {
    let content = content.strip_prefix("#!").map_or(content, |rest| rest.split_once('\n').map_or("", |(_, r)| r));
    let whole = crate::lexer::tokenize(content).and_then(|t| crate::parser::parse(&t));
    match whole {
        Ok(ast) => crate::executor::execute(shell, &ast),
        Err(first_err) => {
            eprintln!("jaksh: lỗi cú pháp: {first_err} — chạy từng dòng thay thế");
            let mut code = 0;
            for (n, raw) in content.lines().enumerate() {
                let line = raw.trim();
                if line.is_empty() || line.starts_with('#') {
                    continue;
                }
                match crate::lexer::tokenize(line).and_then(|t| crate::parser::parse(&t)) {
                    Ok(ast) => code = crate::executor::execute(shell, &ast).unwrap_or(1),
                    Err(e) => {
                        eprintln!("jaksh: dòng {}: {}", n + 1, e);
                        code = 2;
                    }
                }
            }
            Ok(code)
        }
    }
}
