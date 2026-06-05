use rustyline::completion::{Completer, Pair};
use rustyline::highlight::Highlighter;
use rustyline::hint::Hinter;
use rustyline::validate::Validator;
use rustyline::Context;
use rustyline_derive::Helper as DeriveHelper;
use std::borrow::Cow;
use std::cell::RefCell;
use std::rc::Rc;

use crate::shell::Shell;

#[derive(DeriveHelper)]
pub struct ShellHelper {
    shell: Rc<RefCell<Shell>>,
}

impl ShellHelper {
    pub fn new(shell: Rc<RefCell<Shell>>) -> Self {
        Self { shell }
    }
}

impl Completer for ShellHelper {
    type Candidate = Pair;
    fn complete(&self, line: &str, pos: usize, _ctx: &Context<'_>) -> rustyline::Result<(usize, Vec<Pair>)> {
        let (start, frag) = current_word(line, pos);
        let first_word = is_first_word(line, start);
        let mut results = Vec::new();

        if first_word {
            // Suggest builtins, aliases, and PATH binaries
            for b in crate::builtins::BUILTINS {
                if b.starts_with(frag) {
                    results.push(Pair {
                        display: format!("{} (tích hợp)", b),
                        replacement: b.to_string(),
                    });
                }
            }
            for k in self.shell.borrow().aliases.keys() {
                if k.starts_with(frag) {
                    results.push(Pair {
                        display: format!("{} (alias)", k),
                        replacement: k.clone(),
                    });
                }
            }
            for sub in ["clean", "backup", "update", "find", "open", "sysinfo", "theme", "weather", "ip", "help", "git"] {
                let full = format!("jak {}", sub);
                if full.starts_with(frag) {
                    results.push(Pair {
                        display: format!("{} (jak)", full),
                        replacement: full,
                    });
                }
            }
            for cmd in ["jak git save ", "jak git sync", "jak git wip", "jak git amend", "jak git undo", "jak git uncommit", "jak git publish", "jak git unstage ", "jak git clean-branches", "jak git help"] {
                if cmd.starts_with(frag) {
                    results.push(Pair {
                        display: format!("{} (git)", cmd.trim_end()),
                        replacement: cmd.to_string(),
                    });
                }
            }
            for cmd in ["explain ls", "explain ps", "explain df", "explain du", "explain free", "explain top", "explain chmod", "explain git status", "explain git log", "explain netstat", "explain ifconfig", "explain ip", "explain lsof"] {
                if cmd.starts_with(frag) {
                    results.push(Pair {
                        display: format!("{} (chú thích)", cmd),
                        replacement: cmd.to_string(),
                    });
                }
            }
            for cmd in ["ls -la --jak", "ls -lh --jak", "ps aux --jak", "df -h --jak", "du -sh --jak"] {
                if cmd.starts_with(frag) {
                    results.push(Pair {
                        display: format!("{} (pretty)", cmd),
                        replacement: cmd.to_string(),
                    });
                }
            }
            for cmd in ["jak find file ", "jak find dir ", "jak find text ", "jak find big", "jak find recent", "jak find empty", "jak find help"] {
                if cmd.starts_with(frag) {
                    results.push(Pair {
                        display: format!("{} (tìm)", cmd.trim_end()),
                        replacement: cmd.to_string(),
                    });
                }
            }
            // PATH binaries
            for p in path_binaries(frag, 50) {
                results.push(Pair {
                    display: p.clone(),
                    replacement: p,
                });
            }
        }

        // Path completion
        let path_results = path_complete(frag, &self.shell.borrow().cwd);
        results.extend(path_results);

        // Deduplicate by replacement
        let mut seen = std::collections::HashSet::new();
        results.retain(|p| seen.insert(p.replacement.clone()));

        Ok((start, results))
    }
}

impl Hinter for ShellHelper {
    type Hint = String;
}

impl Highlighter for ShellHelper {
    fn highlight_prompt<'b, 's: 'b, 'p: 'b>(&'s self, prompt: &'p str, _default: bool) -> Cow<'b, str> {
        Cow::Borrowed(prompt)
    }
    fn highlight_hint<'h>(&self, hint: &'h str) -> Cow<'h, str> {
        Cow::Owned(format!("\x1b[2m{}\x1b[0m", hint))
    }
}

impl Validator for ShellHelper {}

fn current_word(line: &str, pos: usize) -> (usize, &str) {
    let bytes = line.as_bytes();
    let mut start = pos;
    while start > 0 {
        let c = bytes[start - 1] as char;
        if c.is_whitespace() || matches!(c, '|' | '&' | ';' | '<' | '>') {
            break;
        }
        start -= 1;
    }
    (start, &line[start..pos])
}

fn is_first_word(line: &str, start: usize) -> bool {
    let before = &line[..start];
    let trimmed = before.trim_end();
    trimmed.is_empty()
        || trimmed.ends_with('|')
        || trimmed.ends_with('&')
        || trimmed.ends_with(';')
        || trimmed.ends_with("&&")
        || trimmed.ends_with("||")
}

fn path_complete(frag: &str, cwd: &std::path::Path) -> Vec<Pair> {
    let expanded = shellexpand::tilde(frag).to_string();
    let path = std::path::Path::new(&expanded);
    let (dir, prefix) = if expanded.ends_with('/') || expanded.is_empty() {
        (path.to_path_buf(), String::new())
    } else if let Some(parent) = path.parent() {
        let name = path.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        if parent.as_os_str().is_empty() {
            (std::path::PathBuf::from("."), name)
        } else {
            (parent.to_path_buf(), name)
        }
    } else {
        (std::path::PathBuf::from("."), expanded.clone())
    };

    let scan_dir = if dir.is_absolute() { dir.clone() } else { cwd.join(&dir) };
    let mut out = Vec::new();
    if let Ok(it) = std::fs::read_dir(&scan_dir) {
        for entry in it.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if !name.starts_with(&prefix) {
                continue;
            }
            if prefix.is_empty() && name.starts_with('.') {
                continue;
            }
            let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
            let display = if is_dir { format!("{}/", name) } else { name.clone() };
            let replacement = if dir.as_os_str().is_empty() || dir == std::path::Path::new(".") {
                if is_dir { format!("{}/", name) } else { name }
            } else {
                let base = dir.to_string_lossy().to_string();
                let sep = if base.ends_with('/') { "" } else { "/" };
                if is_dir {
                    format!("{}{}{}/", base, sep, display.trim_end_matches('/'))
                } else {
                    format!("{}{}{}", base, sep, display)
                }
            };
            out.push(Pair { display, replacement });
        }
    }
    out
}

fn path_binaries(prefix: &str, limit: usize) -> Vec<String> {
    if prefix.is_empty() || prefix.contains('/') {
        return Vec::new();
    }
    let mut out = Vec::new();
    if let Some(path) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path) {
            if let Ok(it) = std::fs::read_dir(&dir) {
                for entry in it.flatten() {
                    let name = entry.file_name().to_string_lossy().to_string();
                    if name.starts_with(prefix) {
                        out.push(name);
                        if out.len() >= limit {
                            return out;
                        }
                    }
                }
            }
        }
    }
    out
}
