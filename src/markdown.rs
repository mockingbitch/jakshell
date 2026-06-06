//! Markdown → ANSI renderer dùng cho output trong terminal (vd `jak version` in CHANGELOG).
//!
//! Hỗ trợ subset đủ dùng cho CHANGELOG / help text:
//!   - Headings: `# ` `## ` `### ` `#### `
//!   - Bullet list: `- ` `* ` `+ ` (giữ thụt lề)
//!   - Numbered list: `1. ` `2. ` ...
//!   - Horizontal rule: dòng chỉ chứa `---` / `***` / `___`
//!   - Blockquote: `> `
//!   - Code fence: ```` ``` ```` (giữ nguyên nội dung, in dim)
//!   - Inline: `` `code` ``, `**bold**`, `*italic*`, `[text](url)`
//!
//! Triết lý: best-effort, không cố làm CommonMark đầy đủ. Nếu không match, in nguyên dòng.
//!
//! API chính:
//!   - `render(md: &str) -> String` — trả về chuỗi đã ANSI hoá
//!   - `print(md: &str)` — in thẳng ra stdout

const RESET: &str = "\x1b[0m";
const BOLD: &str = "\x1b[1m";
const DIM: &str = "\x1b[2m";
const ITALIC: &str = "\x1b[3m";
const UNDERLINE: &str = "\x1b[4m";
const CYAN: &str = "\x1b[36m";
const BRIGHT_CYAN: &str = "\x1b[96m";
const BRIGHT_GREEN: &str = "\x1b[92m";
const BRIGHT_YELLOW: &str = "\x1b[93m";
const BRIGHT_MAGENTA: &str = "\x1b[95m";

pub fn print(md: &str) {
    print!("{}", render(md));
}

pub fn render(md: &str) -> String {
    let mut out = String::new();
    let mut in_fence = false;
    let mut fence_lang = String::new();

    for raw in md.lines() {
        // ── Code fence ───────────────────────────────────────────────────────
        let trimmed = raw.trim_start();
        if let Some(rest) = trimmed.strip_prefix("```") {
            if in_fence {
                in_fence = false;
                fence_lang.clear();
            } else {
                in_fence = true;
                fence_lang = rest.trim().to_string();
            }
            // Không in dòng fence — chỉ dùng làm delimiter.
            continue;
        }
        if in_fence {
            // Trong fence: in nguyên dòng, dim, thụt 2 space.
            out.push_str(DIM);
            out.push_str("  │ ");
            // Tô màu nhẹ nhãn ngôn ngữ chỉ ở dòng đầu tiên là tốn, bỏ qua.
            let _ = &fence_lang;
            out.push_str(raw);
            out.push_str(RESET);
            out.push('\n');
            continue;
        }

        // ── Horizontal rule ──────────────────────────────────────────────────
        let t = raw.trim();
        if t == "---" || t == "***" || t == "___" {
            out.push_str(DIM);
            out.push_str(&"─".repeat(56));
            out.push_str(RESET);
            out.push('\n');
            continue;
        }

        // ── Headings ─────────────────────────────────────────────────────────
        if let Some(rest) = t.strip_prefix("#### ") {
            out.push_str(BOLD);
            out.push_str(&render_inline(rest));
            out.push_str(RESET);
            out.push('\n');
            continue;
        }
        if let Some(rest) = t.strip_prefix("### ") {
            out.push_str(BOLD);
            out.push_str(BRIGHT_YELLOW);
            out.push_str(&render_inline(rest));
            out.push_str(RESET);
            out.push('\n');
            continue;
        }
        if let Some(rest) = t.strip_prefix("## ") {
            out.push_str(BOLD);
            out.push_str(BRIGHT_CYAN);
            out.push_str(&render_inline(rest));
            out.push_str(RESET);
            out.push('\n');
            continue;
        }
        if let Some(rest) = t.strip_prefix("# ") {
            out.push_str(BOLD);
            out.push_str(BRIGHT_MAGENTA);
            out.push_str(&render_inline(rest));
            out.push_str(RESET);
            out.push('\n');
            continue;
        }

        // ── Blockquote ───────────────────────────────────────────────────────
        if let Some(rest) = t.strip_prefix("> ") {
            out.push_str(DIM);
            out.push_str("│ ");
            out.push_str(&render_inline(rest));
            out.push_str(RESET);
            out.push('\n');
            continue;
        }

        // ── List items (giữ indent gốc) ──────────────────────────────────────
        let indent_len = raw.len() - trimmed.len();
        let indent = &raw[..indent_len];

        if let Some(rest) = trimmed
            .strip_prefix("- ")
            .or_else(|| trimmed.strip_prefix("* "))
            .or_else(|| trimmed.strip_prefix("+ "))
        {
            out.push_str(indent);
            out.push_str(BRIGHT_GREEN);
            out.push_str("• ");
            out.push_str(RESET);
            out.push_str(&render_inline(rest));
            out.push('\n');
            continue;
        }

        // Numbered list: tìm "N. " ở đầu (N chỉ chữ số).
        if let Some(dot_pos) = trimmed.find(". ") {
            if dot_pos > 0 && trimmed[..dot_pos].chars().all(|c| c.is_ascii_digit()) {
                let num = &trimmed[..dot_pos];
                let rest = &trimmed[dot_pos + 2..];
                out.push_str(indent);
                out.push_str(BRIGHT_GREEN);
                out.push_str(num);
                out.push_str(".");
                out.push_str(RESET);
                out.push(' ');
                out.push_str(&render_inline(rest));
                out.push('\n');
                continue;
            }
        }

        // ── Đoạn văn / dòng trống ────────────────────────────────────────────
        if raw.is_empty() {
            out.push('\n');
        } else {
            out.push_str(&render_inline(raw));
            out.push('\n');
        }
    }
    out
}

/// Xử lý inline markers trong một dòng: `code`, **bold**, *italic*, [text](url).
///
/// `i` luôn ở biên ký tự UTF-8 hợp lệ. Khi không match marker nào, ta advance theo độ dài
/// UTF-8 của ký tự hiện tại (chứ KHÔNG push byte đơn — sẽ phá Vietnamese diacritics).
fn render_inline(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = String::with_capacity(s.len() + 16);
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];

        // ── `code` ───────────────────────────────────────────────────────────
        if c == b'`' {
            if let Some(end) = s[i + 1..].find('`') {
                let inner = &s[i + 1..i + 1 + end];
                out.push_str(BRIGHT_CYAN);
                out.push_str(inner);
                out.push_str(RESET);
                i += 1 + end + 1;
                continue;
            }
        }

        // ── **bold** ─────────────────────────────────────────────────────────
        if c == b'*' && i + 1 < bytes.len() && bytes[i + 1] == b'*' {
            if let Some(end_rel) = s[i + 2..].find("**") {
                let inner = &s[i + 2..i + 2 + end_rel];
                out.push_str(BOLD);
                // Đệ quy vào để hỗ trợ `code` lồng trong **bold**.
                out.push_str(&render_inline(inner));
                out.push_str(RESET);
                i += 2 + end_rel + 2;
                continue;
            }
        }

        // ── *italic* (đơn dấu sao, không bị nhầm với **bold** đã handle ở trên)
        if c == b'*' {
            // Phải có ký tự không phải space ngay sau '*' và '*' đóng không liền trước space.
            if i + 1 < bytes.len() && bytes[i + 1] != b' ' && bytes[i + 1] != b'*' {
                if let Some(end_rel) = s[i + 1..].find('*') {
                    let inner = &s[i + 1..i + 1 + end_rel];
                    // Tránh match **... khi đếm dấu * đóng — đảm bảo ký tự kế tiếp không phải '*'.
                    let close_idx = i + 1 + end_rel;
                    let next_is_star = close_idx + 1 < bytes.len() && bytes[close_idx + 1] == b'*';
                    if !next_is_star && !inner.is_empty() {
                        out.push_str(ITALIC);
                        out.push_str(&render_inline(inner));
                        out.push_str(RESET);
                        i = close_idx + 1;
                        continue;
                    }
                }
            }
        }

        // ── [text](url) ──────────────────────────────────────────────────────
        if c == b'[' {
            if let Some(text_end) = s[i + 1..].find(']') {
                let after = i + 1 + text_end + 1;
                if after < bytes.len() && bytes[after] == b'(' {
                    if let Some(url_end) = s[after + 1..].find(')') {
                        let text = &s[i + 1..i + 1 + text_end];
                        let url = &s[after + 1..after + 1 + url_end];
                        out.push_str(UNDERLINE);
                        out.push_str(CYAN);
                        out.push_str(text);
                        out.push_str(RESET);
                        out.push_str(DIM);
                        out.push_str(" (");
                        out.push_str(url);
                        out.push(')');
                        out.push_str(RESET);
                        i = after + 1 + url_end + 1;
                        continue;
                    }
                }
            }
        }

        // Default: copy 1 ký tự UTF-8 đầy đủ (1–4 byte) — KHÔNG push từng byte.
        let ch_len = utf8_char_len(c);
        out.push_str(&s[i..i + ch_len]);
        i += ch_len;
    }
    out
}

/// Độ dài (byte) của ký tự UTF-8 bắt đầu bằng `b` (1..=4). Mặc định 1 nếu byte không hợp lệ.
fn utf8_char_len(b: u8) -> usize {
    match b {
        0x00..=0x7F => 1,
        0xC0..=0xDF => 2,
        0xE0..=0xEF => 3,
        0xF0..=0xF7 => 4,
        _ => 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_headings() {
        let r = render("# Title\n## Sub\n### Section\n");
        assert!(r.contains("Title"));
        assert!(r.contains("Sub"));
        assert!(r.contains("Section"));
    }

    #[test]
    fn renders_bullet() {
        let r = render("- one\n- two\n");
        // Có bullet char + nội dung; giữa chúng có ANSI reset nên không grep được literal "• one".
        assert!(r.contains("•"));
        assert!(r.contains("one"));
        assert!(r.contains("two"));
    }

    #[test]
    fn renders_inline_code_and_bold() {
        let r = render("Use **`jak self-update`** to upgrade.\n");
        assert!(r.contains("jak self-update"));
    }

    #[test]
    fn renders_link() {
        let r = render("[Keep a Changelog](https://keepachangelog.com)\n");
        assert!(r.contains("Keep a Changelog"));
        assert!(r.contains("keepachangelog.com"));
    }

    #[test]
    fn horizontal_rule() {
        let r = render("---\n");
        assert!(r.contains("─"));
    }

    #[test]
    fn code_fence() {
        let r = render("```bash\nls -la\n```\n");
        assert!(r.contains("ls -la"));
    }
}
