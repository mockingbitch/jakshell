use crate::lexer::WordPart;
use crate::shell::Shell;

/// Lỗi `set -u`: dùng biến chưa gán.
#[derive(Debug)]
pub struct UnboundVar(pub String);

impl std::fmt::Display for UnboundVar {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: biến chưa được gán (set -u)", self.0)
    }
}

impl std::error::Error for UnboundVar {}

/// Expand a word (list of parts) into one or more strings (after glob expansion).
///
/// Phần `WordPart::Quoted` (đến từ nháy đơn/kép) sẽ chặn glob expansion — đúng
/// theo chuẩn POSIX: `"*.toml"` là literal, không bung ra danh sách file.
/// Bản dễ dãi (biến chưa gán = rỗng) — cho completion, prompt, alias…
pub fn expand_word(shell: &Shell, parts: &[WordPart], do_glob: bool) -> Vec<String> {
    expand_impl(shell, parts, do_glob, false).unwrap_or_default()
}

/// Như `expand_word` nhưng tôn trọng `set -u` — dùng khi chạy lệnh thật.
pub fn expand_word_checked(shell: &Shell, parts: &[WordPart], do_glob: bool) -> Result<Vec<String>, UnboundVar> {
    expand_impl(shell, parts, do_glob, shell.opts.nounset)
}

fn expand_impl(shell: &Shell, parts: &[WordPart], do_glob: bool, strict: bool) -> Result<Vec<String>, UnboundVar> {
    // `$@` bung thành MỖI tham số một từ (`"$@"` giữ nguyên tham số có dấu cách);
    // phần chữ dính trước/sau gắn vào từ đầu/cuối, như bash.
    if let Some(at) = parts.iter().position(|p| matches!(p, WordPart::Var(n) if n == "@")) {
        let prefix = expand_impl(shell, &parts[..at], false, strict)?.concat();
        let suffix = expand_impl(shell, &parts[at + 1..], false, strict)?.concat();
        let args = &shell.positional;
        if args.is_empty() {
            let joined = prefix + &suffix;
            return Ok(if joined.is_empty() && at == 0 && parts.len() == 1 { vec![] } else { vec![joined] });
        }
        let last = args.len() - 1;
        return Ok(args
            .iter()
            .enumerate()
            .map(|(i, a)| {
                let mut w = String::new();
                if i == 0 {
                    w.push_str(&prefix);
                }
                w.push_str(a);
                if i == last {
                    w.push_str(&suffix);
                }
                w
            })
            .collect());
    }

    let mut s = String::new();
    let mut had_quoted = false;
    for p in parts {
        match p {
            WordPart::Literal(lit) => s.push_str(lit),
            WordPart::Quoted(lit) => {
                s.push_str(lit);
                had_quoted = true;
            }
            WordPart::Var(name) => match shell.get_var(name) {
                Some(v) => s.push_str(&v),
                None if strict && name != "*" && name != "!" => return Err(UnboundVar(name.clone())),
                None => {}
            },
            WordPart::Tilde => {
                if let Some(h) = dirs::home_dir() {
                    s.push_str(&h.display().to_string());
                }
            }
            // Command substitution đã được executor chạy & thay bằng Quoted
            // TRƯỚC khi tới đây (xem executor::expand_command_subs). Nếu vẫn còn
            // CmdSub thì coi như rỗng — an toàn cho các nơi expand không qua
            // executor (completion, prompt).
            WordPart::CmdSub(_) => {}
        }
    }

    if do_glob && !had_quoted && contains_glob(&s) {
        match glob::glob(&s) {
            Ok(paths) => {
                let mut out: Vec<String> = paths.filter_map(|r| r.ok()).map(|p| p.display().to_string()).collect();
                if out.is_empty() {
                    out.push(s);
                }
                return Ok(out);
            }
            Err(_) => return Ok(vec![s]),
        }
    }
    Ok(vec![s])
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
