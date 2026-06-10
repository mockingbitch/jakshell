use anyhow::{anyhow, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Token {
    Word(Vec<WordPart>),
    Pipe,
    AndIf,
    OrIf,
    Semicolon,
    Ampersand,
    Less,
    Great,
    DGreat,
    ErrGreat,        // 2>
    ErrDGreat,       // 2>>
    AndGreat,        // &>  (both stdout+stderr)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WordPart {
    Literal(String),
    Quoted(String), // nội dung từ trong nháy đơn/kép — không bao giờ bị glob
    Var(String),
    Tilde,
}

pub fn tokenize(input: &str) -> Result<Vec<Token>> {
    let mut tokens = Vec::new();
    let bytes: Vec<char> = input.chars().collect();
    let mut i = 0;

    while i < bytes.len() {
        let c = bytes[i];
        // Xuống dòng = dấu phân tách lệnh (như `;`). Quan trọng khi user paste
        // nhiều dòng cùng lúc: mỗi dòng là 1 lệnh riêng, không bị gộp làm một.
        // Newline NẰM TRONG nháy đơn/kép vẫn giữ nguyên (xử lý ở read_word).
        if c == '\n' || c == '\r' {
            tokens.push(Token::Semicolon);
            i += 1;
            continue;
        }
        if c.is_whitespace() {
            i += 1;
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
                    tokens.push(Token::AndGreat);
                    i += 2;
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
            '<' => {
                tokens.push(Token::Less);
                i += 1;
                continue;
            }
            '>' => {
                if bytes.get(i + 1) == Some(&'>') {
                    tokens.push(Token::DGreat);
                    i += 2;
                } else {
                    tokens.push(Token::Great);
                    i += 1;
                }
                continue;
            }
            '2' if bytes.get(i + 1) == Some(&'>') => {
                if bytes.get(i + 2) == Some(&'>') {
                    tokens.push(Token::ErrDGreat);
                    i += 3;
                } else {
                    tokens.push(Token::ErrGreat);
                    i += 2;
                }
                continue;
            }
            _ => {}
        }

        // Word (collect until whitespace or operator)
        let (parts, next) = read_word(&bytes, i)?;
        tokens.push(Token::Word(parts));
        i = next;
    }

    Ok(tokens)
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
        if c == '2' && bytes.get(i + 1) == Some(&'>') && buf.is_empty() && parts.is_empty() {
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
                    if !qbuf.is_empty() {
                        parts.push(WordPart::Quoted(std::mem::take(&mut qbuf)));
                    }
                    let (name, next) = read_var(bytes, i + 1);
                    parts.push(WordPart::Var(name));
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
            if !buf.is_empty() {
                parts.push(WordPart::Literal(std::mem::take(&mut buf)));
            }
            let (name, next) = read_var(bytes, i + 1);
            parts.push(WordPart::Var(name));
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
    // special single-char vars like $? $$ $!
    if let Some(&c) = bytes.get(i) {
        if matches!(c, '?' | '$' | '!' | '#') {
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
}
