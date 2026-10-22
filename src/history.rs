//! Xử lý lịch sử lệnh phía JakShell (phần rustyline không làm):
//!   • gộp dòng nối bằng `\` thành 1 entry khi paste lệnh nhiều dòng;
//!   • history expansion kiểu bash: `!!`, `!$`, `!*`, `!N`, `!-N`, `!prefix`.

use anyhow::{anyhow, Result};

/// Tách 1 lần nhập (có thể là khối paste nhiều dòng) thành các entry lịch sử.
/// Dòng kết thúc bằng `\` được nối với dòng sau (bỏ `\`, ghép bằng 1 dấu cách)
/// — nhờ đó `curl ... \` paste nhiều dòng gọi lại bằng ↑ thành 1 lệnh chạy được
/// thay vì từng mẩu `--header ...` rời rạc.
pub fn entries(input: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for raw in input.split('\n') {
        let line = raw.strip_suffix('\r').unwrap_or(raw);
        let piece = if cur.is_empty() { line } else { line.trim_start() };
        match piece.strip_suffix('\\') {
            Some(head) => {
                cur.push_str(head.trim_end());
                cur.push(' ');
            }
            None => {
                cur.push_str(piece);
                let entry = cur.trim().to_string();
                if !entry.is_empty() {
                    out.push(entry);
                }
                cur.clear();
            }
        }
    }
    let entry = cur.trim().to_string();
    if !entry.is_empty() {
        out.push(entry);
    }
    out
}

/// History expansion. `hist` = lịch sử TRƯỚC lệnh hiện tại (cũ → mới).
/// Trả `Ok(None)` nếu dòng không có gì để mở rộng, `Ok(Some(dòng mới))` nếu có,
/// `Err` khi không tìm thấy lệnh (bash: "event not found").
///
/// Như bash: không mở rộng trong nháy đơn, sau `\`, hay khi `!` đứng trước
/// khoảng trắng / `=` / `(` / cuối dòng (vd `[ ! -f x ]`, `!=`). Khác bash:
/// `!prefix` chỉ mở rộng khi `!` ở ĐẦU một từ — `echo hi!there` giữ nguyên
/// thay vì báo lỗi.
pub fn expand(line: &str, hist: &[String]) -> Result<Option<String>> {
    if !line.contains('!') {
        return Ok(None);
    }
    let chars: Vec<char> = line.chars().collect();
    let mut out = String::with_capacity(line.len());
    let mut changed = false;
    let mut in_single = false;
    let mut in_double = false;
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '\\' && !in_single {
            out.push(c);
            if let Some(&n) = chars.get(i + 1) {
                out.push(n);
            }
            i += 2;
            continue;
        }
        if c == '\'' && !in_double {
            in_single = !in_single;
        } else if c == '"' && !in_single {
            in_double = !in_double;
        }
        if c != '!' || in_single || (i > 0 && chars[i - 1] == '$') {
            out.push(c);
            i += 1;
            continue;
        }
        let next = chars.get(i + 1).copied();
        let at_word_start = i == 0 || matches!(chars[i - 1], ' ' | '\t' | ';' | '|' | '&' | '(');
        let last = || hist.last().ok_or_else(|| anyhow!("!!: event not found"));
        match next {
            Some('!') => {
                out.push_str(last()?);
                i += 2;
            }
            Some('$') => {
                let entry = last()?;
                out.push_str(words(entry).last().copied().unwrap_or(""));
                i += 2;
            }
            Some('*') => {
                let entry = last()?;
                out.push_str(&words(entry).get(1..).unwrap_or(&[]).join(" "));
                i += 2;
            }
            Some(d) if d.is_ascii_digit() || (d == '-' && chars.get(i + 2).is_some_and(|c| c.is_ascii_digit())) => {
                let start = i + 1;
                let mut j = if d == '-' { start + 1 } else { start };
                while j < chars.len() && chars[j].is_ascii_digit() {
                    j += 1;
                }
                let spec: String = chars[start..j].iter().collect();
                let n: i64 = spec.parse().unwrap_or(0);
                let idx = if n < 0 { hist.len() as i64 + n } else { n - 1 };
                let entry = usize::try_from(idx)
                    .ok()
                    .and_then(|k| hist.get(k))
                    .ok_or_else(|| anyhow!("!{}: event not found", spec))?;
                out.push_str(entry);
                i = j;
            }
            Some(n) if at_word_start && !is_word_end(n) && n != '=' && n != '"' => {
                let start = i + 1;
                let mut j = start;
                while j < chars.len() && !is_word_end(chars[j]) && chars[j] != '"' && chars[j] != '\'' {
                    j += 1;
                }
                let prefix: String = chars[start..j].iter().collect();
                let entry = hist
                    .iter()
                    .rev()
                    .find(|e| e.starts_with(&prefix))
                    .ok_or_else(|| anyhow!("!{}: event not found", prefix))?;
                out.push_str(entry);
                i = j;
            }
            _ => {
                out.push(c);
                i += 1;
                continue;
            }
        }
        changed = true;
    }
    Ok(changed.then_some(out))
}

fn is_word_end(c: char) -> bool {
    c.is_whitespace() || matches!(c, ';' | '|' | '&' | '(' | ')' | '<' | '>')
}

/// Tách lệnh thành từ (giữ nguyên nháy) — đủ cho `!$` / `!*`.
fn words(line: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = None;
    let mut quote: Option<char> = None;
    for (idx, c) in line.char_indices() {
        match quote {
            Some(q) => {
                if c == q {
                    quote = None;
                }
            }
            None if c.is_whitespace() => {
                if let Some(s) = start.take() {
                    out.push(&line[s..idx]);
                }
            }
            None => {
                if start.is_none() {
                    start = Some(idx);
                }
                if c == '\'' || c == '"' {
                    quote = Some(c);
                }
            }
        }
    }
    if let Some(s) = start {
        out.push(&line[s..]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn h() -> Vec<String> {
        vec!["git status".into(), "ls -la /tmp".into(), "echo 'a b' c".into()]
    }

    #[test]
    fn joins_continuation_lines() {
        let paste = "curl --location 'https://x/v' \\\n  --header 'A: b' \\\n  --data '{}'\nls";
        assert_eq!(
            entries(paste),
            vec!["curl --location 'https://x/v' --header 'A: b' --data '{}'", "ls"]
        );
        assert_eq!(entries("a\n\nb"), vec!["a", "b"]);
    }

    #[test]
    fn bang_forms() {
        let h = h();
        assert_eq!(expand("sudo !!", &h).unwrap().unwrap(), "sudo echo 'a b' c");
        assert_eq!(expand("cat !$", &h).unwrap().unwrap(), "cat c");
        assert_eq!(expand("echo !*", &h).unwrap().unwrap(), "echo 'a b' c");
        assert_eq!(expand("!1", &h).unwrap().unwrap(), "git status");
        assert_eq!(expand("!-2", &h).unwrap().unwrap(), "ls -la /tmp");
        assert_eq!(expand("!git", &h).unwrap().unwrap(), "git status");
    }

    #[test]
    fn bang_left_alone() {
        let h = h();
        for s in ["[ ! -f x ]", "a != b", "echo '!!'", "echo \\!!", "echo $!", "echo hi!there", "echo \"wow!\"", "x!"] {
            assert_eq!(expand(s, &h).unwrap(), None, "{s}");
        }
    }

    #[test]
    fn bang_not_found() {
        assert!(expand("!nope", &h()).is_err());
        assert!(expand("!99", &h()).is_err());
        assert!(expand("!!", &[]).is_err());
    }
}
