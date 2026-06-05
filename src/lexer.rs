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
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        if c == '#' {
            // comment to end of line
            break;
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
            buf.push(bytes[i + 1]);
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
