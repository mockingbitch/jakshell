use anyhow::{anyhow, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Token {
    Word(Vec<WordPart>),
    Pipe,
    AndIf,
    OrIf,
    Semicolon,
    Ampersand,
    /// Toán tử redirect, kèm số fd viết liền phía trước nếu có (`2>`, `3<`).
    Redir(Option<i32>, RedirOp),
    /// Here-doc đã đọc xong thân (`<<EOF` / `<<-EOF` / `<<'EOF'`). Thân có
    /// delimiter trong nháy → 1 `Quoted` nguyên văn; không → được expand.
    HereDoc(Option<i32>, Vec<WordPart>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RedirOp {
    In,         // <
    Out,        // >
    Append,     // >>
    Clobber,    // >|
    ReadWrite,  // <>
    DupOut,     // >&
    DupIn,      // <&
    AllOut,     // &>
    AllAppend,  // &>>
    HereString, // <<<
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WordPart {
    Literal(String),
    Quoted(String), // nội dung từ trong nháy đơn/kép — không bao giờ bị glob
    Var(String),
    Tilde,
    /// Command substitution `$(...)` hoặc `` `...` `` — giữ nguyên phần lệnh bên
    /// trong (chưa chạy). Executor chạy nó rồi thay bằng stdout trước khi expand.
    CmdSub(String),
}

pub fn tokenize(input: &str) -> Result<Vec<Token>> {
    let mut tokens = Vec::new();
    let bytes: Vec<char> = input.chars().collect();
    let mut i = 0;
    let mut pending_heredocs: Vec<PendingHereDoc> = Vec::new();

    while i < bytes.len() {
        let c = bytes[i];
        // Xuống dòng = dấu phân tách lệnh (như `;`). Quan trọng khi user paste
        // nhiều dòng cùng lúc: mỗi dòng là 1 lệnh riêng, không bị gộp làm một.
        // Newline NẰM TRONG nháy đơn/kép vẫn giữ nguyên (xử lý ở read_word).
        if c == '\n' || c == '\r' {
            tokens.push(Token::Semicolon);
            i += 1;
            if c == '\r' && bytes.get(i) == Some(&'\n') {
                i += 1;
            }
            // Here-doc mở trên dòng vừa hết: thân nằm ở các dòng tiếp theo.
            if !pending_heredocs.is_empty() {
                i = read_heredoc_bodies(&bytes, i, &mut tokens, &mut pending_heredocs);
            }
            continue;
        }
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        // `\` + xuống dòng giữa hai từ (`... 'url' \` rồi dòng sau thụt lề) =
        // khoảng trắng. Để read_word xử lý sẽ sinh ra 1 word rỗng → curl báo
        // "blank argument".
        if c == '\\' && bytes.get(i + 1) == Some(&'\n') {
            i += 2;
            continue;
        }
        if c == '\\' && bytes.get(i + 1) == Some(&'\r') {
            i += if bytes.get(i + 2) == Some(&'\n') { 3 } else { 2 };
            continue;
        }
        if c == '#' {
            // comment: bỏ qua tới hết dòng (không kết thúc toàn bộ input —
            // các dòng sau khi paste nhiều dòng vẫn phải được xử lý).
            while i < bytes.len() && bytes[i] != '\n' && bytes[i] != '\r' {
                i += 1;
            }
            continue;
        }
        // `N>` / `N<` — số fd viết liền ngay trước toán tử redirect.
        if c.is_ascii_digit() {
            let mut j = i;
            while j < bytes.len() && bytes[j].is_ascii_digit() {
                j += 1;
            }
            if matches!(bytes.get(j), Some('<') | Some('>')) {
                let fd: i32 = bytes[i..j].iter().collect::<String>().parse().unwrap_or(-1);
                i = lex_redirect(&bytes, j, Some(fd), &mut tokens, &mut pending_heredocs)?;
                continue;
            }
        }
        // Operators
        match c {
            '|' => {
                if bytes.get(i + 1) == Some(&'|') {
                    tokens.push(Token::OrIf);
                    i += 2;
                } else {
                    tokens.push(Token::Pipe);
                    i += 1;
                }
                continue;
            }
            '&' => {
                if bytes.get(i + 1) == Some(&'&') {
                    tokens.push(Token::AndIf);
                    i += 2;
                } else if bytes.get(i + 1) == Some(&'>') {
                    if bytes.get(i + 2) == Some(&'>') {
                        tokens.push(Token::Redir(None, RedirOp::AllAppend));
                        i += 3;
                    } else {
                        tokens.push(Token::Redir(None, RedirOp::AllOut));
                        i += 2;
                    }
                } else {
                    tokens.push(Token::Ampersand);
                    i += 1;
                }
                continue;
            }
            ';' => {
                tokens.push(Token::Semicolon);
                i += 1;
                continue;
            }
            '<' | '>' => {
                i = lex_redirect(&bytes, i, None, &mut tokens, &mut pending_heredocs)?;
                continue;
            }
            _ => {}
        }

        // Word (collect until whitespace or operator)
        let (parts, next) = read_word(&bytes, i)?;
        tokens.push(Token::Word(parts));
        i = next;
    }
    // Here-doc trên dòng cuối không có thân (vd gõ 1 dòng) → thân rỗng.
    if !pending_heredocs.is_empty() {
        read_heredoc_bodies(&bytes, bytes.len(), &mut tokens, &mut pending_heredocs);
    }

    Ok(tokens)
}

/// Here-doc đã gặp `<<DELIM` nhưng chưa đọc thân (thân bắt đầu ở dòng sau).
struct PendingHereDoc {
    token_idx: usize,
    delim: String,
    strip_tabs: bool,
    quoted: bool,
}

/// Đọc 1 toán tử redirect bắt đầu tại `i` (ký tự `<` hoặc `>`), đẩy token, trả
/// về vị trí kế tiếp. `<<` / `<<-` đọc luôn delimiter và đăng ký here-doc.
fn lex_redirect(
    bytes: &[char],
    i: usize,
    fd: Option<i32>,
    tokens: &mut Vec<Token>,
    pending: &mut Vec<PendingHereDoc>,
) -> Result<usize> {
    let at = |k: usize| bytes.get(i + k).copied();
    let (op, len) = match (at(0), at(1), at(2)) {
        (Some('<'), Some('<'), Some('<')) => (RedirOp::HereString, 3),
        (Some('<'), Some('<'), _) => {
            let strip_tabs = at(2) == Some('-');
            let mut j = i + if strip_tabs { 3 } else { 2 };
            while j < bytes.len() && (bytes[j] == ' ' || bytes[j] == '\t') {
                j += 1;
            }
            let (parts, next) = read_word(bytes, j)?;
            if parts.is_empty() {
                return Err(anyhow!("thiếu delimiter sau '<<'"));
            }
            // Bất kỳ dạng nháy / `\` nào trong delimiter → thân không expand (bash).
            let raw: String = bytes[j..next].iter().collect();
            let quoted = raw.contains('\'') || raw.contains('"') || raw.contains('\\');
            let delim: String = parts
                .iter()
                .map(|p| match p {
                    WordPart::Literal(s) | WordPart::Quoted(s) => s.clone(),
                    WordPart::Var(v) => format!("${}", v),
                    WordPart::Tilde => "~".into(),
                    WordPart::CmdSub(c) => format!("$({})", c),
                })
                .collect();
            pending.push(PendingHereDoc { token_idx: tokens.len(), delim, strip_tabs, quoted });
            tokens.push(Token::HereDoc(fd, Vec::new()));
            return Ok(next);
        }
        (Some('<'), Some('&'), _) => (RedirOp::DupIn, 2),
        (Some('<'), Some('>'), _) => (RedirOp::ReadWrite, 2),
        (Some('<'), _, _) => (RedirOp::In, 1),
        (Some('>'), Some('>'), _) => (RedirOp::Append, 2),
        (Some('>'), Some('&'), _) => (RedirOp::DupOut, 2),
        (Some('>'), Some('|'), _) => (RedirOp::Clobber, 2),
        _ => (RedirOp::Out, 1),
    };
    tokens.push(Token::Redir(fd, op));
    Ok(i + len)
}

/// Đọc thân các here-doc đang chờ, bắt đầu từ đầu dòng tại `start`. Trả về vị
/// trí ngay sau dòng delimiter cuối cùng. Thiếu delimiter → lấy tới hết input
/// (bash cũng chỉ cảnh báo).
fn read_heredoc_bodies(bytes: &[char], start: usize, tokens: &mut [Token], pending: &mut Vec<PendingHereDoc>) -> usize {
    let mut i = start;
    for hd in pending.drain(..) {
        let mut body = String::new();
        while i < bytes.len() {
            let line_end = bytes[i..].iter().position(|&c| c == '\n').map_or(bytes.len(), |p| i + p);
            let mut line: String = bytes[i..line_end].iter().collect();
            if line.ends_with('\r') {
                line.pop();
            }
            i = if line_end < bytes.len() { line_end + 1 } else { bytes.len() };
            let content = if hd.strip_tabs { line.trim_start_matches('\t') } else { line.as_str() };
            if content == hd.delim {
                break;
            }
            body.push_str(content);
            body.push('\n');
        }
        let parts = if hd.quoted { vec![WordPart::Quoted(body)] } else { heredoc_parts(&body) };
        if let Some(Token::HereDoc(_, slot)) = tokens.get_mut(hd.token_idx) {
            *slot = parts;
        }
    }
    i
}

/// Thân here-doc không nháy: `$VAR`, `${...}`, `$(...)`, `` `...` `` được thay;
/// `\` chỉ escape `$` `` ` `` `\` và nối dòng — giống trong nháy kép nhưng `"`
/// là ký tự thường. Chữ thường thành `Quoted` (không glob).
pub fn heredoc_parts(body: &str) -> Vec<WordPart> {
    let bytes: Vec<char> = body.chars().collect();
    let mut parts = Vec::new();
    let mut buf = String::new();
    let mut i = 0;
    let flush = |buf: &mut String, parts: &mut Vec<WordPart>| {
        if !buf.is_empty() {
            parts.push(WordPart::Quoted(std::mem::take(buf)));
        }
    };
    while i < bytes.len() {
        let c = bytes[i];
        if c == '\\' && i + 1 < bytes.len() {
            let n = bytes[i + 1];
            if n == '\n' {
                i += 2;
                continue;
            }
            if matches!(n, '$' | '`' | '\\') {
                buf.push(n);
                i += 2;
                continue;
            }
        }
        if c == '$' {
            if bytes.get(i + 1) == Some(&'(') {
                if let Ok((inner, next)) = read_cmd_sub_paren(&bytes, i + 2) {
                    flush(&mut buf, &mut parts);
                    parts.push(WordPart::CmdSub(inner));
                    i = next;
                    continue;
                }
            }
            let (name, next) = read_var(&bytes, i + 1);
            if !name.is_empty() {
                flush(&mut buf, &mut parts);
                parts.push(WordPart::Var(name));
                i = next;
                continue;
            }
        }
        if c == '`' {
            if let Ok((inner, next)) = read_backtick(&bytes, i + 1) {
                flush(&mut buf, &mut parts);
                parts.push(WordPart::CmdSub(inner));
                i = next;
                continue;
            }
        }
        buf.push(c);
        i += 1;
    }
    flush(&mut buf, &mut parts);
    if parts.is_empty() {
        parts.push(WordPart::Quoted(String::new()));
    }
    parts
}

fn read_word(bytes: &[char], start: usize) -> Result<(Vec<WordPart>, usize)> {
    let mut parts = Vec::new();
    let mut buf = String::new();
    let mut i = start;
    let mut first = true;

    while i < bytes.len() {
        let c = bytes[i];
        // operator/whitespace ends word
        if c.is_whitespace() || matches!(c, '|' | '&' | ';' | '<' | '>') {
            break;
        }

        if c == '~' && first && (bytes.get(i + 1).is_none() || bytes[i + 1] == '/' || bytes[i + 1].is_whitespace() || matches!(bytes[i + 1], '|' | '&' | ';' | '<' | '>')) {
            if !buf.is_empty() {
                parts.push(WordPart::Literal(std::mem::take(&mut buf)));
            }
            parts.push(WordPart::Tilde);
            i += 1;
            first = false;
            continue;
        }
        first = false;

        if c == '\'' {
            // single-quoted: literal, no expansion
            if !buf.is_empty() {
                parts.push(WordPart::Literal(std::mem::take(&mut buf)));
            }
            i += 1;
            let mut qbuf = String::new();
            while i < bytes.len() && bytes[i] != '\'' {
                qbuf.push(bytes[i]);
                i += 1;
            }
            if i >= bytes.len() {
                return Err(anyhow!("nháy đơn không đóng"));
            }
            i += 1; // skip closing '
            parts.push(WordPart::Quoted(qbuf));
            continue;
        }

        if c == '"' {
            // double-quoted: $VAR expands, \ escapes some chars
            if !buf.is_empty() {
                parts.push(WordPart::Literal(std::mem::take(&mut buf)));
            }
            i += 1;
            let mut qbuf = String::new();
            while i < bytes.len() && bytes[i] != '"' {
                let ch = bytes[i];
                if ch == '\\' && i + 1 < bytes.len() {
                    let n = bytes[i + 1];
                    // Nối dòng trong nháy kép: `\` trước xuống dòng → bỏ cả hai.
                    if n == '\n' {
                        i += 2;
                        continue;
                    }
                    if n == '\r' {
                        i += if bytes.get(i + 2) == Some(&'\n') { 3 } else { 2 };
                        continue;
                    }
                    if matches!(n, '"' | '\\' | '$' | '`') {
                        qbuf.push(n);
                        i += 2;
                        continue;
                    }
                }
                if ch == '$' {
                    // `$(...)` bên trong nháy kép vẫn là command substitution
                    // (chỉ chặn glob/word-split, không chặn thay thế lệnh).
                    if bytes.get(i + 1) == Some(&'(') {
                        if !qbuf.is_empty() {
                            parts.push(WordPart::Quoted(std::mem::take(&mut qbuf)));
                        }
                        let (inner, next) = read_cmd_sub_paren(bytes, i + 2)?;
                        parts.push(WordPart::CmdSub(inner));
                        i = next;
                        continue;
                    }
                    if !qbuf.is_empty() {
                        parts.push(WordPart::Quoted(std::mem::take(&mut qbuf)));
                    }
                    let (name, next) = read_var(bytes, i + 1);
                    parts.push(WordPart::Var(name));
                    i = next;
                    continue;
                }
                if ch == '`' {
                    if !qbuf.is_empty() {
                        parts.push(WordPart::Quoted(std::mem::take(&mut qbuf)));
                    }
                    let (inner, next) = read_backtick(bytes, i + 1)?;
                    parts.push(WordPart::CmdSub(inner));
                    i = next;
                    continue;
                }
                qbuf.push(ch);
                i += 1;
            }
            if i >= bytes.len() {
                return Err(anyhow!("nháy kép không đóng"));
            }
            i += 1;
            if !qbuf.is_empty() {
                parts.push(WordPart::Quoted(qbuf));
            }
            continue;
        }

        if c == '\\' && i + 1 < bytes.len() {
            let n = bytes[i + 1];
            // `\` ngay trước xuống dòng = nối dòng (line continuation): bỏ cả
            // hai. Quan trọng khi paste lệnh nhiều dòng dạng `curl ... \`.
            if n == '\n' {
                i += 2;
                continue;
            }
            if n == '\r' {
                i += if bytes.get(i + 2) == Some(&'\n') { 3 } else { 2 };
                continue;
            }
            buf.push(n);
            i += 2;
            continue;
        }

        if c == '$' {
            // `$(...)` = command substitution.
            if bytes.get(i + 1) == Some(&'(') {
                if !buf.is_empty() {
                    parts.push(WordPart::Literal(std::mem::take(&mut buf)));
                }
                let (inner, next) = read_cmd_sub_paren(bytes, i + 2)?;
                parts.push(WordPart::CmdSub(inner));
                i = next;
                continue;
            }
            if !buf.is_empty() {
                parts.push(WordPart::Literal(std::mem::take(&mut buf)));
            }
            let (name, next) = read_var(bytes, i + 1);
            parts.push(WordPart::Var(name));
            i = next;
            continue;
        }

        if c == '`' {
            // Backtick command substitution `` `...` `` (dạng cũ, vẫn phổ biến).
            if !buf.is_empty() {
                parts.push(WordPart::Literal(std::mem::take(&mut buf)));
            }
            let (inner, next) = read_backtick(bytes, i + 1)?;
            parts.push(WordPart::CmdSub(inner));
            i = next;
            continue;
        }

        buf.push(c);
        i += 1;
    }

    if !buf.is_empty() {
        parts.push(WordPart::Literal(buf));
    }
    Ok((parts, i))
}

fn read_var(bytes: &[char], start: usize) -> (String, usize) {
    let mut name = String::new();
    let mut i = start;
    if bytes.get(i) == Some(&'{') {
        i += 1;
        while i < bytes.len() && bytes[i] != '}' {
            name.push(bytes[i]);
            i += 1;
        }
        if i < bytes.len() {
            i += 1;
        }
        return (name, i);
    }
    // Biến đặc biệt 1 ký tự: $? $$ $! $# $@ $* $- và tham số vị trí $0..$9
    // (`$10` = `$1` + "0", như POSIX — muốn tham số 10 phải viết `${10}`).
    if let Some(&c) = bytes.get(i) {
        if matches!(c, '?' | '$' | '!' | '#' | '@' | '*' | '-') || c.is_ascii_digit() {
            return (c.to_string(), i + 1);
        }
    }
    while i < bytes.len() {
        let c = bytes[i];
        if c.is_alphanumeric() || c == '_' {
            name.push(c);
            i += 1;
        } else {
            break;
        }
    }
    (name, i)
}

/// Đọc nội dung `$(...)` bắt đầu ngay SAU `$(` (tại `start`), trả về (nội_dung,
/// vị_trí_sau_dấu_`)`). Đếm ngoặc lồng nhau và bỏ qua ngoặc nằm trong nháy
/// đơn/kép để `$(echo ")")` hay `$(a $(b) c)` không kết thúc sớm.
fn read_cmd_sub_paren(bytes: &[char], start: usize) -> Result<(String, usize)> {
    let mut depth = 1;
    let mut i = start;
    let mut inner = String::new();
    while i < bytes.len() {
        let c = bytes[i];
        match c {
            '(' => {
                depth += 1;
                inner.push(c);
                i += 1;
            }
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return Ok((inner, i + 1));
                }
                inner.push(c);
                i += 1;
            }
            '\'' => {
                inner.push(c);
                i += 1;
                while i < bytes.len() && bytes[i] != '\'' {
                    inner.push(bytes[i]);
                    i += 1;
                }
                if i < bytes.len() {
                    inner.push(bytes[i]);
                    i += 1;
                }
            }
            '"' => {
                inner.push(c);
                i += 1;
                while i < bytes.len() && bytes[i] != '"' {
                    if bytes[i] == '\\' && i + 1 < bytes.len() {
                        inner.push(bytes[i]);
                        inner.push(bytes[i + 1]);
                        i += 2;
                        continue;
                    }
                    inner.push(bytes[i]);
                    i += 1;
                }
                if i < bytes.len() {
                    inner.push(bytes[i]);
                    i += 1;
                }
            }
            _ => {
                inner.push(c);
                i += 1;
            }
        }
    }
    Err(anyhow!("$(...) không đóng"))
}

/// Đọc nội dung `` `...` `` bắt đầu ngay SAU dấu backtick mở (tại `start`).
/// Trong backtick, `\`` `\\` `\$` được escape (POSIX).
fn read_backtick(bytes: &[char], start: usize) -> Result<(String, usize)> {
    let mut i = start;
    let mut inner = String::new();
    while i < bytes.len() {
        let c = bytes[i];
        if c == '\\' && i + 1 < bytes.len() && matches!(bytes[i + 1], '`' | '\\' | '$') {
            inner.push(bytes[i + 1]);
            i += 2;
            continue;
        }
        if c == '`' {
            return Ok((inner, i + 1));
        }
        inner.push(c);
        i += 1;
    }
    Err(anyhow!("`...` không đóng"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(input: &str) -> Vec<String> {
        tokenize(input)
            .unwrap()
            .into_iter()
            .filter_map(|t| match t {
                Token::Word(parts) => Some(
                    parts
                        .iter()
                        .map(|p| match p {
                            WordPart::Literal(s) | WordPart::Quoted(s) => s.clone(),
                            WordPart::Var(s) => format!("${}", s),
                            WordPart::Tilde => "~".to_string(),
                            WordPart::CmdSub(s) => format!("$({})", s),
                        })
                        .collect::<String>(),
                ),
                _ => None,
            })
            .collect()
    }

    fn count_separators(input: &str) -> usize {
        tokenize(input)
            .unwrap()
            .iter()
            .filter(|t| matches!(t, Token::Semicolon))
            .count()
    }

    #[test]
    fn newline_is_command_separator() {
        // hai dòng → 1 dấu phân tách ở giữa
        assert_eq!(count_separators("echo a\necho b"), 1);
    }

    #[test]
    fn backslash_newline_is_line_continuation() {
        // `\` cuối dòng nối dòng: KHÔNG có dấu phân tách, các word nối liền mạch
        let w = words("curl --location 'https://x/v' \\\n--header 'Content-Type: application/json'");
        assert_eq!(
            w,
            vec![
                "curl",
                "--location",
                "https://x/v",
                "--header",
                "Content-Type: application/json",
            ]
        );
        assert_eq!(count_separators("foo \\\nbar"), 0, "nối dòng không tạo separator");
        // dòng sau thụt lề (kiểu paste từ Postman/docs) — không sinh word rỗng
        let w = words("curl 'https://x/v' \\\n  --header 'A: b' \\\r\n  -s");
        assert_eq!(w, vec!["curl", "https://x/v", "--header", "A: b", "-s"]);
    }

    #[test]
    fn single_quote_preserves_newlines() {
        // JSON nhiều dòng trong nháy đơn giữ nguyên xuống dòng
        let w = words("--data '{\n  \"a\": 1\n}'");
        assert_eq!(w, vec!["--data", "{\n  \"a\": 1\n}"]);
    }

    #[test]
    fn comment_only_to_end_of_line() {
        // comment ở dòng 1 không nuốt dòng 2
        let w = words("echo a # ghi chú\necho b");
        assert_eq!(w, vec!["echo", "a", "echo", "b"]);
    }

    /// Lấy các WordPart của word thứ `idx` (0-based) trong input.
    fn word_parts(input: &str, idx: usize) -> Vec<WordPart> {
        tokenize(input)
            .unwrap()
            .into_iter()
            .filter_map(|t| match t {
                Token::Word(parts) => Some(parts),
                _ => None,
            })
            .nth(idx)
            .unwrap()
    }

    #[test]
    fn dollar_paren_is_command_substitution() {
        assert_eq!(
            word_parts("echo $(date +%s)", 1),
            vec![WordPart::CmdSub("date +%s".into())]
        );
    }

    #[test]
    fn backtick_is_command_substitution() {
        assert_eq!(
            word_parts("echo `uname -s`", 1),
            vec![WordPart::CmdSub("uname -s".into())]
        );
    }

    #[test]
    fn nested_command_substitution() {
        // ngoặc lồng nhau không kết thúc sớm
        assert_eq!(
            word_parts("echo $(a $(b) c)", 1),
            vec![WordPart::CmdSub("a $(b) c".into())]
        );
    }

    #[test]
    fn cmdsub_inside_double_quotes() {
        // $(...) trong "..." vẫn là cmdsub; phần chữ quanh nó thành Quoted
        assert_eq!(
            word_parts("x\"$(id -u)\"", 0),
            vec![WordPart::Literal("x".into()), WordPart::CmdSub("id -u".into())]
        );
    }

    #[test]
    fn single_quotes_keep_cmdsub_literal() {
        // trong nháy đơn, $(...) là chuỗi thường, KHÔNG chạy
        assert_eq!(
            word_parts("'$(nope)'", 0),
            vec![WordPart::Quoted("$(nope)".into())]
        );
    }

    #[test]
    fn cmdsub_paren_inside_quotes_does_not_close_early() {
        // dấu ) nằm trong nháy bên trong $(...) không kết thúc cmdsub
        assert_eq!(
            word_parts("echo $(echo \")\")", 1),
            vec![WordPart::CmdSub("echo \")\"".into())]
        );
    }

    #[test]
    fn fd_redirect_tokens() {
        let t = tokenize("cmd >f 2>&1 3<in 2>&- &>>log >|x <<<s").unwrap();
        let redirs: Vec<(Option<i32>, RedirOp)> = t
            .iter()
            .filter_map(|t| match t {
                Token::Redir(fd, op) => Some((*fd, *op)),
                _ => None,
            })
            .collect();
        assert_eq!(
            redirs,
            vec![
                (None, RedirOp::Out),
                (Some(2), RedirOp::DupOut),
                (Some(3), RedirOp::In),
                (Some(2), RedirOp::DupOut),
                (None, RedirOp::AllAppend),
                (None, RedirOp::Clobber),
                (None, RedirOp::HereString),
            ]
        );
        // chữ số dính trong từ không phải fd: `a2>f` = từ `a2` + `>f`
        assert_eq!(words("echo a2>f"), vec!["echo", "a2", "f"]);
    }

    fn heredoc_body(input: &str) -> Vec<WordPart> {
        tokenize(input)
            .unwrap()
            .into_iter()
            .find_map(|t| match t {
                Token::HereDoc(_, body) => Some(body),
                _ => None,
            })
            .unwrap()
    }

    #[test]
    fn heredoc_unquoted_expands() {
        assert_eq!(
            heredoc_body("cat <<EOF\nhi $USER\nEOF\necho after"),
            vec![WordPart::Quoted("hi ".into()), WordPart::Var("USER".into()), WordPart::Quoted("\n".into())]
        );
        // lệnh sau delimiter vẫn được tokenize
        assert!(words("cat <<EOF\nx\nEOF\necho after").ends_with(&["echo".to_string(), "after".to_string()]));
    }

    #[test]
    fn heredoc_quoted_and_dash() {
        assert_eq!(heredoc_body("cat <<'EOF'\nraw $HOME\nEOF"), vec![WordPart::Quoted("raw $HOME\n".into())]);
        assert_eq!(heredoc_body("cat <<-EOF\n\t\tindented\n\tEOF"), vec![WordPart::Quoted("indented\n".into())]);
    }

    #[test]
    fn positional_vars() {
        assert_eq!(word_parts("$10", 0), vec![WordPart::Var("1".into()), WordPart::Literal("0".into())]);
        assert_eq!(word_parts("${10}", 0), vec![WordPart::Var("10".into())]);
        assert_eq!(word_parts("\"$@\"", 0), vec![WordPart::Var("@".into())]);
    }

    #[test]
    fn unclosed_cmdsub_errors() {
        assert!(tokenize("echo $(oops").is_err());
        assert!(tokenize("echo `oops").is_err());
    }
}
