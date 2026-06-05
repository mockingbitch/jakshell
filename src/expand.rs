use crate::lexer::WordPart;
use crate::shell::Shell;

/// Expand a word (list of parts) into one or more strings (after glob expansion).
///
/// Phần `WordPart::Quoted` (đến từ nháy đơn/kép) sẽ chặn glob expansion — đúng
/// theo chuẩn POSIX: `"*.toml"` là literal, không bung ra danh sách file.
pub fn expand_word(shell: &Shell, parts: &[WordPart], do_glob: bool) -> Vec<String> {
    let mut s = String::new();
    let mut had_quoted = false;
    for p in parts {
        match p {
            WordPart::Literal(lit) => s.push_str(lit),
            WordPart::Quoted(lit) => {
                s.push_str(lit);
                had_quoted = true;
            }
            WordPart::Var(name) => {
                if let Some(v) = shell.get_var(name) {
                    s.push_str(&v);
                }
            }
            WordPart::Tilde => {
                if let Some(h) = dirs::home_dir() {
                    s.push_str(&h.display().to_string());
                }
            }
        }
    }

    if do_glob && !had_quoted && contains_glob(&s) {
        match glob::glob(&s) {
            Ok(paths) => {
                let mut out: Vec<String> = paths.filter_map(|r| r.ok()).map(|p| p.display().to_string()).collect();
                if out.is_empty() {
                    out.push(s);
                }
                return out;
            }
            Err(_) => return vec![s],
        }
    }
    vec![s]
}

fn contains_glob(s: &str) -> bool {
    let mut in_bracket = false;
    for c in s.chars() {
        match c {
            '*' | '?' if !in_bracket => return true,
            '[' => in_bracket = true,
            ']' => return true,
            _ => {}
        }
    }
    false
}
