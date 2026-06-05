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
        for (k, v) in cfg.aliases {
            shell.borrow_mut().aliases.insert(k, v);
        }
        for (k, v) in cfg.env {
            shell.borrow_mut().set_var(&k, &v);
        }
    }

    let script_path = shell.borrow().rc_script_path();
    if script_path.exists() {
        let content = std::fs::read_to_string(&script_path)?;
        run_script(shell, &content)?;
    }
    Ok(())
}

pub fn run_script(shell: &Rc<RefCell<Shell>>, content: &str) -> Result<()> {
    for raw in content.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        match crate::lexer::tokenize(line) {
            Ok(tokens) => match crate::parser::parse(&tokens) {
                Ok(ast) => {
                    let _ = crate::executor::execute(shell, &ast);
                }
                Err(e) => eprintln!("jakshrc: lỗi cú pháp '{}': {}", line, e),
            },
            Err(e) => eprintln!("jakshrc: lỗi token '{}': {}", line, e),
        }
    }
    Ok(())
}
