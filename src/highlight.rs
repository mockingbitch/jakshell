//! Tô màu cú pháp khi gõ (rustyline `Highlighter`).
//!
//! Bộ quét riêng, dễ dãi với dòng đang gõ dở (nháy / `$(` chưa đóng): lỗi cú
//! pháp không bao giờ làm mất màu cả dòng. Chỉ chèn mã ANSI — không đổi độ
//! rộng hiển thị.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::Path;

use crate::shell::Shell;

const RESET: &str = "\x1b[0m";
const CMD_OK: &str = "\x1b[32m";
const CMD_BAD: &str = "\x1b[31m";
const KEYWORD: &str = "\x1b[1;35m";
const STRING: &str = "\x1b[33m";
const VAR: &str = "\x1b[36m";
const OPERATOR: &str = "\x1b[35m";
const COMMENT: &str = "\x1b[2m";
const PATH_OK: &str = "\x1b[4m";
const OPTION: &str = "\x1b[34m";

const KEYWORDS: &[&str] = &[
    "if", "then", "else", "elif", "fi", "for", "in", "do", "done", "while", "until", "case", "esac", "{", "}",
    "[[", "]]", "function", "!", "time",
];

/// Từ khoá mà sau nó lại là vị trí tên lệnh.
const CMD_AFTER: &[&str] = &["if", "then", "else", "elif", "do", "while", "until", "{", "!", "time"];

/// Cache "tên lệnh có tồn tại không" theo `$PATH` hiện tại (gõ phím nào cũng
/// tô lại cả dòng — không thể stat PATH mỗi lần).
#[derive(Default)]
pub struct CommandCache {
    path: String,
    known: HashMap<String, bool>,
}

impl CommandCache {
    fn exists(&mut self, shell: &Shell, name: &str) -> bool {
        if crate::builtins::is_builtin(name)
            || shell.aliases.contains_key(name)
            || shell.functions.contains_key(name)
            || name == "jak"
            || KEYWORDS.contains(&name)
        {
            return true;
        }
        if name.contains('/') {
            let p = if Path::new(name).is_absolute() { Path::new(name).to_path_buf() } else { shell.cwd.join(name) };
            return p.is_file();
        }
        let path = shell.get_var("PATH").unwrap_or_default();
        if path != self.path {
            self.path = path.clone();
            self.known.clear();
        }
        if let Some(&k) = self.known.get(name) {
            return k;
        }
        let found = which::which_in(name, Some(&path), &shell.cwd).is_ok();
        self.known.insert(name.to_string(), found);
        found
    }
}

/// Tô màu `line`. `cache` dùng chung giữa các lần gõ phím.
pub fn highlight(line: &str, shell: &Shell, cache: &RefCell<CommandCache>) -> String {
    let chars: Vec<char> = line.chars().collect();
    let mut out = String::with_capacity(line.len() * 2);
    let mut i = 0;
    let mut cmd_pos = true;
    let mut after_for = false;
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            out.push(c);
            i += 1;
            if c == '\n' {
                cmd_pos = true;
            }
            continue;
        }
        if c == '#' {
            let rest: String = chars[i..].iter().take_while(|&&c| c != '\n').collect();
            i += rest.chars().count();
            out.push_str(COMMENT);
            out.push_str(&rest);
            out.push_str(RESET);
            continue;
        }
        if matches!(c, '|' | '&' | ';' | '(' | ')' | '<' | '>') {
            let mut op = String::new();
            while i < chars.len() && matches!(chars[i], '|' | '&' | ';' | '(' | ')' | '<' | '>') {
                op.push(chars[i]);
                i += 1;
            }
            // Sau `|`, `&&`, `;`, `(`… lại là vị trí tên lệnh; sau redirect thì không.
            cmd_pos = !op.ends_with('<') && !op.ends_with('>');
            out.push_str(OPERATOR);
            out.push_str(&op);
            out.push_str(RESET);
            continue;
        }
        // 1 từ (tới khoảng trắng / toán tử ngoài nháy).
        let start = i;
        let mut word_out = String::new();
        let mut plain = String::new();
        let mut has_expansion = false;
        while i < chars.len() {
            let c = chars[i];
            if c.is_whitespace() || matches!(c, '|' | '&' | ';' | '(' | ')' | '<' | '>') {
                break;
            }
            match c {
                '\'' => {
                    let end = chars[i + 1..].iter().position(|&c| c == '\'').map_or(chars.len(), |p| i + 1 + p + 1);
                    let s: String = chars[i..end].iter().collect();
                    plain.push_str(s.trim_matches('\''));
                    word_out.push_str(STRING);
                    word_out.push_str(&s);
                    word_out.push_str(RESET);
                    i = end;
                }
                '"' => {
                    let (s, end) = scan_double(&chars, i);
                    plain.push_str(&s.replace('"', ""));
                    word_out.push_str(&s);
                    i = end;
                    has_expansion |= s.contains('$');
                }
                '$' | '`' => {
                    let end = scan_dollar(&chars, i);
                    let s: String = chars[i..end].iter().collect();
                    word_out.push_str(VAR);
                    word_out.push_str(&s);
                    word_out.push_str(RESET);
                    i = end;
                    has_expansion = true;
                }
                '\\' => {
                    let end = (i + 2).min(chars.len());
                    word_out.extend(&chars[i..end]);
                    if end > i + 1 {
                        plain.push(chars[i + 1]);
                    }
                    i = end;
                }
                _ => {
                    word_out.push(c);
                    plain.push(c);
                    i += 1;
                }
            }
        }
        let raw: String = chars[start..i].iter().collect();

        if cmd_pos {
            if KEYWORDS.contains(&raw.as_str()) {
                out.push_str(KEYWORD);
                out.push_str(&raw);
                out.push_str(RESET);
                cmd_pos = CMD_AFTER.contains(&raw.as_str());
                after_for = raw == "for" || raw == "case";
                continue;
            }
            // `NAME=giá trị` đứng trước lệnh: vẫn ở vị trí tên lệnh.
            if let Some(eq) = raw.find('=') {
                let name = &raw[..eq];
                if !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '+' || c == '[' || c == ']') {
                    out.push_str(VAR);
                    out.push_str(name);
                    out.push_str(RESET);
                    // phần giá trị giữ màu đã tô (bỏ phần tên)
                    let prefix_chars = name.chars().count();
                    let colored_value = strip_prefix_visible(&word_out, prefix_chars);
                    out.push_str(&colored_value);
                    continue;
                }
            }
            cmd_pos = false;
            if after_for {
                // tên biến của `for` / từ của `case`
                out.push_str(&word_out);
                after_for = false;
                continue;
            }
            let ok = has_expansion || cache.borrow_mut().exists(shell, &plain);
            out.push_str(if ok { CMD_OK } else { CMD_BAD });
            out.push_str(&word_out);
            out.push_str(RESET);
            continue;
        }
        if KEYWORDS.contains(&raw.as_str()) && matches!(raw.as_str(), "in" | "do" | "then" | "done" | "fi" | "esac" | "}" | "]]") {
            out.push_str(KEYWORD);
            out.push_str(&raw);
            out.push_str(RESET);
            cmd_pos = matches!(raw.as_str(), "do" | "then");
            continue;
        }
        if raw.starts_with('-') && !has_expansion && word_out == raw {
            out.push_str(OPTION);
            out.push_str(&raw);
            out.push_str(RESET);
            continue;
        }
        // Đối số là đường dẫn có thật → gạch chân (kiểu fish).
        if !has_expansion && plain.len() > 1 && looks_like_path(&plain) && path_exists(shell, &plain) {
            out.push_str(PATH_OK);
            out.push_str(&word_out);
            out.push_str(RESET);
            continue;
        }
        out.push_str(&word_out);
    }
    out
}

fn looks_like_path(s: &str) -> bool {
    s.contains('/') || s.starts_with('.') || s.starts_with('~')
}

fn path_exists(shell: &Shell, s: &str) -> bool {
    let expanded = if let Some(rest) = s.strip_prefix('~') {
        match shell.get_var("HOME") {
            Some(h) => format!("{}{}", h, rest),
            None => return false,
        }
    } else {
        s.to_string()
    };
    let p = Path::new(&expanded);
    if p.is_absolute() { p.exists() } else { shell.cwd.join(p).exists() }
}

/// Chuỗi nháy kép tại `i` (tô vàng, biến bên trong tô cyan). Dễ dãi khi chưa đóng.
fn scan_double(chars: &[char], i: usize) -> (String, usize) {
    let mut s = String::from(STRING);
    s.push('"');
    let mut j = i + 1;
    while j < chars.len() && chars[j] != '"' {
        if chars[j] == '\\' && j + 1 < chars.len() {
            s.push(chars[j]);
            s.push(chars[j + 1]);
            j += 2;
            continue;
        }
        if chars[j] == '$' || chars[j] == '`' {
            let end = scan_dollar(chars, j);
            s.push_str(VAR);
            s.extend(&chars[j..end]);
            s.push_str(STRING);
            j = end;
            continue;
        }
        s.push(chars[j]);
        j += 1;
    }
    if j < chars.len() {
        s.push('"');
        j += 1;
    }
    s.push_str(RESET);
    (s, j)
}

/// Phần expansion bắt đầu tại `i` (`$x`, `${…}`, `$(…)`, `$((…))`, `` `…` ``).
fn scan_dollar(chars: &[char], i: usize) -> usize {
    let close_of = |open: char, close: char, from: usize| -> usize {
        let mut depth = 0;
        let mut j = from;
        while j < chars.len() {
            if chars[j] == open {
                depth += 1;
            } else if chars[j] == close {
                depth -= 1;
                if depth == 0 {
                    return j + 1;
                }
            }
            j += 1;
        }
        chars.len()
    };
    if chars[i] == '`' {
        return chars[i + 1..].iter().position(|&c| c == '`').map_or(chars.len(), |p| i + p + 2);
    }
    match chars.get(i + 1) {
        Some('(') => close_of('(', ')', i + 1),
        Some('{') => close_of('{', '}', i + 1),
        Some(c) if matches!(c, '?' | '$' | '!' | '#' | '@' | '*' | '-') || c.is_ascii_digit() => i + 2,
        Some(c) if c.is_ascii_alphabetic() || *c == '_' => {
            let mut j = i + 1;
            while j < chars.len() && (chars[j].is_ascii_alphanumeric() || chars[j] == '_') {
                j += 1;
            }
            j
        }
        _ => i + 1,
    }
}

/// Bỏ `n` ký tự HIỂN THỊ đầu của chuỗi có mã ANSI (giữ nguyên mã màu).
fn strip_prefix_visible(s: &str, n: usize) -> String {
    let mut out = String::new();
    let mut skipped = 0;
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            out.push(c);
            for d in chars.by_ref() {
                out.push(d);
                if d.is_ascii_alphabetic() {
                    break;
                }
            }
            continue;
        }
        if skipped < n {
            skipped += 1;
            continue;
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strip(s: &str) -> String {
        let mut out = String::new();
        let mut in_esc = false;
        for c in s.chars() {
            if c == '\x1b' {
                in_esc = true;
            } else if in_esc {
                if c.is_ascii_alphabetic() {
                    in_esc = false;
                }
            } else {
                out.push(c);
            }
        }
        out
    }

    fn hl(line: &str) -> String {
        let shell = Shell::new().unwrap();
        highlight(line, &shell, &RefCell::new(CommandCache::default()))
    }

    #[test]
    fn highlighting_never_changes_text() {
        for line in [
            "echo hello | grep h && ls -la /tmp",
            "for f in *.txt; do echo \"$f ${x:-y}\"; done",
            "X=1 nosuchcmd_zz 'a b' # comment",
            "echo \"unclosed $(date",
            "echo $(( 1 + 2 )) `uname` \\$HOME",
            "if [[ -f x ]]; then cat < in > out; fi",
            "tiếng Việt có dấu",
        ] {
            assert_eq!(strip(&hl(line)), line);
        }
    }

    #[test]
    fn command_colors() {
        assert!(hl("echo hi").starts_with(CMD_OK), "builtin → xanh");
        assert!(hl("nosuchcmd_zz").starts_with(CMD_BAD), "lệnh không có → đỏ");
        assert!(hl("if true").starts_with(KEYWORD));
        // sau `|` là lệnh mới
        assert!(hl("echo a | nosuchcmd_zz").contains(&format!("{}nosuchcmd_zz", CMD_BAD)));
    }
}
