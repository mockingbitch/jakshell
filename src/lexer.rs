use anyhow::{anyhow, Result};
use std::cell::Cell;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Token {
    Word(Vec<WordPart>),
    Pipe,
    AndIf,
    OrIf,
    Semicolon,
    Ampersand,
    LParen,
    RParen,
    /// `;;` / `;&` / `;;&` — kết thúc nhánh `case`.
    DSemi,
    SemiAnd,
    DSemiAnd,
    /// `(( biểu thức ))` — lệnh số học.
    DParen(String),
    /// Toán tử redirect, kèm số fd viết liền phía trước nếu có (`2>`, `3<`).
    Redir(Option<i32>, RedirOp),
    /// Here-doc đã đọc xong thân (`<<EOF` / `<<-EOF` / `<<'EOF'`). Thân có
    /// delimiter trong nháy → 1 `Quoted` nguyên văn; không → `DQ` được expand.
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
    /// Chữ không nằm trong nháy — chịu brace expansion và glob.
    Literal(String),
    /// Chữ trong nháy đơn, `$'...'`, hoặc ký tự escape `\x` — giữ nguyên văn.
    Quoted(String),
    /// Nội dung `"..."`: các phần bên trong không bị tách từ / glob.
    DQ(Vec<WordPart>),
    /// `$name` / `${name}`.
    Var(String),
    /// `${...}` có toán tử hoặc chỉ số mảng.
    Param(Box<ParamExpr>),
    /// `~` hoặc `~user` ở đầu từ.
    Tilde(String),
    /// Command substitution `$(...)` hoặc `` `...` `` — giữ nguyên phần lệnh bên
    /// trong (chưa chạy). Executor chạy nó rồi thay bằng stdout khi expand.
    CmdSub(String),
    /// `$(( biểu thức ))`.
    Arith(String),
    /// Process substitution: `<(cmd)` (true = đọc từ lệnh) / `>(cmd)`.
    ProcSub(String, bool),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParamExpr {
    /// Tên biến, số (tham số vị trí) hoặc ký tự đặc biệt (`@ * # ? $ ! -`).
    pub name: String,
    /// Chỉ số mảng thô: `@`, `*`, biểu thức số học hoặc khoá.
    pub index: Option<String>,
    pub op: ParamOp,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParamOp {
    Plain,
    /// `${#x}` / `${#a[@]}`
    Length,
    /// `${!x}`
    Indirect,
    /// `${!a[@]}`
    Keys,
    /// `${!prefix*}` / `${!prefix@}`
    NamesWithPrefix,
    /// `${x:-w}` / `${x-w}`
    Default { colon: bool, word: Vec<WordPart> },
    /// `${x:=w}`
    Assign { colon: bool, word: Vec<WordPart> },
    /// `${x:?w}`
    Error { colon: bool, word: Vec<WordPart> },
    /// `${x:+w}`
    Alt { colon: bool, word: Vec<WordPart> },
    /// `${x#p}` / `${x##p}`
    RemovePrefix { longest: bool, pat: Vec<WordPart> },
    /// `${x%p}` / `${x%%p}`
    RemoveSuffix { longest: bool, pat: Vec<WordPart> },
    /// `${x/p/r}` `${x//p/r}` `${x/#p/r}` `${x/%p/r}`
    Replace { mode: ReplaceMode, pat: Vec<WordPart>, rep: Vec<WordPart> },
    /// `${x:offset}` / `${x:offset:length}` (biểu thức số học thô).
    Substr { offset: String, length: Option<String> },
    /// `${x^}` `${x^^}` `${x,}` `${x,,}`
    Case { upper: bool, all: bool },
    /// `${x@Q}` `${x@U}` `${x@L}` `${x@E}` …
    Transform(char),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplaceMode {
    First,
    All,
    Prefix,
    Suffix,
}

/// Input chưa trọn (nháy chưa đóng, `$(` thiếu `)`, here-doc thiếu delimiter…).
/// REPL dựa vào lỗi này để chờ thêm dòng thay vì báo lỗi.
#[derive(Debug)]
pub struct Incomplete(pub String);

impl std::fmt::Display for Incomplete {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for Incomplete {}

pub fn incomplete(msg: &str) -> anyhow::Error {
    anyhow::Error::new(Incomplete(msg.to_string()))
}

thread_local! {
    /// Lần tokenize gần nhất có here-doc thiếu delimiter không (script vẫn chạy
    /// như bash; REPL dùng để chờ thêm dòng).
    static HEREDOC_OPEN: Cell<bool> = const { Cell::new(false) };
}

pub fn last_heredoc_unterminated() -> bool {
    HEREDOC_OPEN.with(|c| c.get())
}

fn is_meta(c: char) -> bool {
    c.is_whitespace() || matches!(c, '|' | '&' | ';' | '<' | '>' | '(' | ')')
}

pub fn tokenize(input: &str) -> Result<Vec<Token>> {
    let mut tokens = Vec::new();
    let bytes: Vec<char> = input.chars().collect();
    let mut i = 0;
    let mut pending_heredocs: Vec<PendingHereDoc> = Vec::new();
    HEREDOC_OPEN.with(|c| c.set(false));

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
            if matches!(bytes.get(j), Some('<') | Some('>')) && bytes.get(j + 1) != Some(&'(') {
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
                } else if bytes.get(i + 1) == Some(&'&') {
                    // `a |& b` = `a 2>&1 | b`
                    tokens.push(Token::Redir(Some(2), RedirOp::DupOut));
                    tokens.push(Token::Word(vec![WordPart::Literal("1".into())]));
                    tokens.push(Token::Pipe);
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
                match (bytes.get(i + 1), bytes.get(i + 2)) {
                    (Some(';'), Some('&')) => {
                        tokens.push(Token::DSemiAnd);
                        i += 3;
                    }
                    (Some(';'), _) => {
                        tokens.push(Token::DSemi);
                        i += 2;
                    }
                    (Some('&'), _) => {
                        tokens.push(Token::SemiAnd);
                        i += 2;
                    }
                    _ => {
                        tokens.push(Token::Semicolon);
                        i += 1;
                    }
                }
                continue;
            }
            '(' => {
                // `(( expr ))` — lệnh số học (nếu kết thúc bằng `))`).
                if bytes.get(i + 1) == Some(&'(') {
                    if let Some((expr, next)) = read_arith(&bytes, i + 2) {
                        tokens.push(Token::DParen(expr));
                        i = next;
                        continue;
                    }
                }
                tokens.push(Token::LParen);
                i += 1;
                continue;
            }
            ')' => {
                tokens.push(Token::RParen);
                i += 1;
                continue;
            }
            '<' | '>' => {
                // `<(cmd)` / `>(cmd)` — process substitution là 1 từ.
                if bytes.get(i + 1) == Some(&'(') {
                    let (parts, next) = read_word(&bytes, i, false)?;
                    tokens.push(Token::Word(parts));
                    i = next;
                    continue;
                }
                i = lex_redirect(&bytes, i, None, &mut tokens, &mut pending_heredocs)?;
                continue;
            }
            _ => {}
        }

        // Word (collect until whitespace or operator)
        let (parts, next) = read_word(&bytes, i, false)?;
        tokens.push(Token::Word(parts));
        i = next;
    }
    // Here-doc trên dòng cuối không có thân (vd gõ 1 dòng) → thân rỗng.
    if !pending_heredocs.is_empty() {
        HEREDOC_OPEN.with(|c| c.set(true));
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
            let (parts, next) = read_word(bytes, j, false)?;
            if parts.is_empty() {
                return Err(anyhow!("thiếu delimiter sau '<<'"));
            }
            // Bất kỳ dạng nháy / `\` nào trong delimiter → thân không expand (bash).
            let raw: String = bytes[j..next].iter().collect();
            let quoted = raw.contains('\'') || raw.contains('"') || raw.contains('\\');
            let delim = parts_to_source(&parts);
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

/// Ghép lại chữ của các phần (bỏ nháy) — dùng cho delimiter here-doc, từ khoá.
pub fn parts_to_source(parts: &[WordPart]) -> String {
    parts
        .iter()
        .map(|p| match p {
            WordPart::Literal(s) | WordPart::Quoted(s) => s.clone(),
            WordPart::DQ(inner) => parts_to_source(inner),
            WordPart::Var(v) => format!("${}", v),
            WordPart::Tilde(u) => format!("~{}", u),
            WordPart::CmdSub(c) => format!("$({})", c),
            WordPart::Arith(a) => format!("$(({}))", a),
            WordPart::Param(_) => "${...}".into(),
            WordPart::ProcSub(c, input) => format!("{}({})", if *input { '<' } else { '>' }, c),
        })
        .collect()
}

/// Đọc thân các here-doc đang chờ, bắt đầu từ đầu dòng tại `start`. Trả về vị
/// trí ngay sau dòng delimiter cuối cùng. Thiếu delimiter → lấy tới hết input
/// (bash cũng chỉ cảnh báo) và đánh dấu để REPL biết chờ thêm.
fn read_heredoc_bodies(bytes: &[char], start: usize, tokens: &mut [Token], pending: &mut Vec<PendingHereDoc>) -> usize {
    let mut i = start;
    for hd in pending.drain(..) {
        let mut body = String::new();
        let mut closed = false;
        while i < bytes.len() {
            let line_end = bytes[i..].iter().position(|&c| c == '\n').map_or(bytes.len(), |p| i + p);
            let mut line: String = bytes[i..line_end].iter().collect();
            if line.ends_with('\r') {
                line.pop();
            }
            i = if line_end < bytes.len() { line_end + 1 } else { bytes.len() };
            let content = if hd.strip_tabs { line.trim_start_matches('\t') } else { line.as_str() };
            if content == hd.delim {
                closed = true;
                break;
            }
            body.push_str(content);
            body.push('\n');
        }
        if !closed {
            HEREDOC_OPEN.with(|c| c.set(true));
        }
        let parts = if hd.quoted { vec![WordPart::Quoted(body)] } else { vec![WordPart::DQ(heredoc_parts(&body))] };
        if let Some(Token::HereDoc(_, slot)) = tokens.get_mut(hd.token_idx) {
            *slot = parts;
        }
    }
    i
}

/// Thân here-doc không nháy: `$VAR`, `${...}`, `$(...)`, `$((...))`, `` `...` ``
/// được thay; `\` chỉ escape `$` `` ` `` `\` và nối dòng — giống trong nháy kép
/// nhưng `"` là ký tự thường.
pub fn heredoc_parts(body: &str) -> Vec<WordPart> {
    let bytes: Vec<char> = body.chars().collect();
    let mut parts = Vec::new();
    let mut buf = String::new();
    let mut i = 0;
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
        if c == '$' || c == '`' {
            if let Ok(Some((part, next))) = read_dollar(&bytes, i) {
                flush(&mut buf, &mut parts, true);
                parts.push(part);
                i = next;
                continue;
            }
        }
        buf.push(c);
        i += 1;
    }
    flush(&mut buf, &mut parts, true);
    if parts.is_empty() {
        parts.push(WordPart::Quoted(String::new()));
    }
    parts
}

fn flush(buf: &mut String, parts: &mut Vec<WordPart>, quoted: bool) {
    if !buf.is_empty() {
        let s = std::mem::take(buf);
        parts.push(if quoted { WordPart::Quoted(s) } else { WordPart::Literal(s) });
    }
}

/// Đọc 1 từ bắt đầu tại `start`. `embedded` = đang đọc chuỗi con bên trong
/// `${x:-...}` / pattern: khoảng trắng và toán tử là chữ thường, không kết thúc từ.
fn read_word(bytes: &[char], start: usize, embedded: bool) -> Result<(Vec<WordPart>, usize)> {
    let mut parts = Vec::new();
    let mut buf = String::new();
    let mut i = start;

    while i < bytes.len() {
        let c = bytes[i];
        // `<(cmd)` / `>(cmd)` — process substitution.
        if !embedded && (c == '<' || c == '>') && bytes.get(i + 1) == Some(&'(') {
            let (inner, next) = read_cmd_sub_paren(bytes, i + 2)?;
            flush(&mut buf, &mut parts, false);
            parts.push(WordPart::ProcSub(inner, c == '<'));
            i = next;
            continue;
        }
        // operator/whitespace ends word
        if !embedded && is_meta(c) {
            break;
        }

        if c == '~' && i == start && !embedded {
            // `~` / `~user` ở đầu từ (tới `/` hoặc hết từ).
            let mut j = i + 1;
            let mut user = String::new();
            let mut valid = true;
            while j < bytes.len() && !is_meta(bytes[j]) && bytes[j] != '/' && bytes[j] != ':' {
                if !(bytes[j].is_ascii_alphanumeric() || matches!(bytes[j], '_' | '-' | '.')) {
                    valid = false;
                    break;
                }
                user.push(bytes[j]);
                j += 1;
            }
            if valid {
                parts.push(WordPart::Tilde(user));
                i = j;
                continue;
            }
        }

        if c == '\'' {
            // single-quoted: literal, no expansion
            flush(&mut buf, &mut parts, false);
            i += 1;
            let mut qbuf = String::new();
            while i < bytes.len() && bytes[i] != '\'' {
                qbuf.push(bytes[i]);
                i += 1;
            }
            if i >= bytes.len() {
                return Err(incomplete("nháy đơn không đóng"));
            }
            i += 1; // skip closing '
            parts.push(WordPart::Quoted(qbuf));
            continue;
        }

        if c == '"' {
            flush(&mut buf, &mut parts, false);
            let (inner, next) = read_double_quoted(bytes, i + 1)?;
            parts.push(WordPart::DQ(inner));
            i = next;
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
            // `\x` = ký tự x nguyên văn (không glob, không brace).
            flush(&mut buf, &mut parts, false);
            parts.push(WordPart::Quoted(n.to_string()));
            i += 2;
            continue;
        }

        if c == '$' || c == '`' {
            // `$'...'` — chuỗi kiểu C (\n \t \x41 …).
            if c == '$' && bytes.get(i + 1) == Some(&'\'') {
                let mut j = i + 2;
                let mut raw = String::new();
                while j < bytes.len() && bytes[j] != '\'' {
                    if bytes[j] == '\\' && j + 1 < bytes.len() {
                        raw.push(bytes[j]);
                        raw.push(bytes[j + 1]);
                        j += 2;
                        continue;
                    }
                    raw.push(bytes[j]);
                    j += 1;
                }
                if j >= bytes.len() {
                    return Err(incomplete("nháy $'...' không đóng"));
                }
                flush(&mut buf, &mut parts, false);
                parts.push(WordPart::Quoted(ansi_c_unescape(&raw)));
                i = j + 1;
                continue;
            }
            // `$"..."` (chuỗi dịch) = nháy kép thường.
            if c == '$' && bytes.get(i + 1) == Some(&'"') {
                flush(&mut buf, &mut parts, false);
                let (inner, next) = read_double_quoted(bytes, i + 2)?;
                parts.push(WordPart::DQ(inner));
                i = next;
                continue;
            }
            if let Some((part, next)) = read_dollar(bytes, i)? {
                flush(&mut buf, &mut parts, false);
                parts.push(part);
                i = next;
                continue;
            }
        }

        buf.push(c);
        i += 1;
    }

    flush(&mut buf, &mut parts, false);
    Ok((parts, i))
}

/// Nội dung nháy kép bắt đầu ngay sau `"`. Trả về các phần + vị trí sau `"` đóng.
fn read_double_quoted(bytes: &[char], start: usize) -> Result<(Vec<WordPart>, usize)> {
    let mut parts = Vec::new();
    let mut qbuf = String::new();
    let mut i = start;
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
        if ch == '$' || ch == '`' {
            if let Some((part, next)) = read_dollar(bytes, i)? {
                flush(&mut qbuf, &mut parts, true);
                parts.push(part);
                i = next;
                continue;
            }
        }
        qbuf.push(ch);
        i += 1;
    }
    if i >= bytes.len() {
        return Err(incomplete("nháy kép không đóng"));
    }
    flush(&mut qbuf, &mut parts, true);
    if parts.is_empty() {
        parts.push(WordPart::Quoted(String::new()));
    }
    Ok((parts, i + 1))
}

/// Đọc phần bắt đầu bằng `$` hoặc `` ` `` tại `i`. `None` nếu `$` chỉ là chữ
/// (vd `$` cuối từ, `$ `).
fn read_dollar(bytes: &[char], i: usize) -> Result<Option<(WordPart, usize)>> {
    if bytes[i] == '`' {
        let (inner, next) = read_backtick(bytes, i + 1)?;
        return Ok(Some((WordPart::CmdSub(inner), next)));
    }
    match bytes.get(i + 1) {
        Some('(') => {
            if bytes.get(i + 2) == Some(&'(') {
                if let Some((expr, next)) = read_arith(bytes, i + 3) {
                    return Ok(Some((WordPart::Arith(expr), next)));
                }
            }
            let (inner, next) = read_cmd_sub_paren(bytes, i + 2)?;
            Ok(Some((WordPart::CmdSub(inner), next)))
        }
        Some('{') => {
            let (content, next) = read_brace_param(bytes, i + 2)?;
            Ok(Some((parse_param(&content)?, next)))
        }
        // `$[ expr ]` — cú pháp số học cũ.
        Some('[') => {
            let end = bytes[i + 2..].iter().position(|&c| c == ']').ok_or_else(|| incomplete("$[ không đóng"))?;
            let expr: String = bytes[i + 2..i + 2 + end].iter().collect();
            Ok(Some((WordPart::Arith(expr), i + 3 + end)))
        }
        _ => {
            let (name, next) = read_var(bytes, i + 1);
            if name.is_empty() {
                return Ok(None);
            }
            Ok(Some((WordPart::Var(name), next)))
        }
    }
}

fn read_var(bytes: &[char], start: usize) -> (String, usize) {
    let mut name = String::new();
    let mut i = start;
    // Biến đặc biệt 1 ký tự: $? $$ $! $# $@ $* $- và tham số vị trí $0..$9
    // (`$10` = `$1` + "0", như POSIX — muốn tham số 10 phải viết `${10}`).
    if let Some(&c) = bytes.get(i) {
        if matches!(c, '?' | '$' | '!' | '#' | '@' | '*' | '-') || c.is_ascii_digit() {
            return (c.to_string(), i + 1);
        }
    }
    while i < bytes.len() {
        let c = bytes[i];
        if c.is_ascii_alphanumeric() || c == '_' {
            name.push(c);
            i += 1;
        } else {
            break;
        }
    }
    (name, i)
}

/// Đọc nội dung `${...}` (bắt đầu sau `${`) tới `}` khớp — bỏ qua nháy, `$(...)`
/// và `${...}` lồng nhau.
fn read_brace_param(bytes: &[char], start: usize) -> Result<(String, usize)> {
    let mut depth = 1;
    let mut i = start;
    let mut out = String::new();
    while i < bytes.len() {
        let c = bytes[i];
        match c {
            '\\' if i + 1 < bytes.len() => {
                out.push(c);
                out.push(bytes[i + 1]);
                i += 2;
                continue;
            }
            '\'' => {
                let end = bytes[i + 1..].iter().position(|&c| c == '\'').ok_or_else(|| incomplete("nháy đơn không đóng"))?;
                out.extend(&bytes[i..i + 2 + end]);
                i += end + 2;
                continue;
            }
            '"' => {
                let (_, next) = read_double_quoted(bytes, i + 1)?;
                out.extend(&bytes[i..next]);
                i = next;
                continue;
            }
            '$' if bytes.get(i + 1) == Some(&'(') => {
                let (_, next) = read_cmd_sub_paren(bytes, i + 2)?;
                out.extend(&bytes[i..next]);
                i = next;
                continue;
            }
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Ok((out, i + 1));
                }
            }
            _ => {}
        }
        out.push(c);
        i += 1;
    }
    Err(incomplete("${ không đóng"))
}

/// Phân tích nội dung `${...}` thành `WordPart` (Var thường hoặc Param có toán tử).
fn parse_param(content: &str) -> Result<WordPart> {
    let chars: Vec<char> = content.chars().collect();
    let bad = || anyhow!("${{{}}}: thay thế không hợp lệ (bad substitution)", content);
    if chars.is_empty() {
        return Err(bad());
    }
    let mut i = 0;
    let mut op = ParamOp::Plain;

    // Tiền tố `#` (độ dài) / `!` (gián tiếp) — trừ khi chính nó là tên ($#, $!).
    let mut prefix = None;
    if (chars[0] == '#' || chars[0] == '!') && chars.len() > 1 && !matches!(chars[1], ':' | '-' | '=' | '?' | '+' | '}') {
        prefix = Some(chars[0]);
        i = 1;
    }

    // Tên
    let name_start = i;
    if i < chars.len() && (matches!(chars[i], '@' | '*' | '#' | '?' | '$' | '!' | '-') || chars[i].is_ascii_digit()) {
        if chars[i].is_ascii_digit() {
            while i < chars.len() && chars[i].is_ascii_digit() {
                i += 1;
            }
        } else {
            i += 1;
        }
    } else {
        while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') {
            i += 1;
        }
    }
    let name: String = chars[name_start..i].iter().collect();
    if name.is_empty() {
        return Err(bad());
    }

    // Chỉ số mảng
    let mut index = None;
    if chars.get(i) == Some(&'[') {
        let mut depth = 0;
        let mut j = i;
        while j < chars.len() {
            match chars[j] {
                '[' => depth += 1,
                ']' => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
                _ => {}
            }
            j += 1;
        }
        if j >= chars.len() {
            return Err(bad());
        }
        index = Some(chars[i + 1..j].iter().collect::<String>());
        i = j + 1;
    }

    let rest: String = chars[i..].iter().collect();
    match prefix {
        Some('#') => {
            if !rest.is_empty() {
                return Err(bad());
            }
            op = ParamOp::Length;
        }
        Some('!') => {
            op = if matches!(index.as_deref(), Some("@") | Some("*")) {
                ParamOp::Keys
            } else if rest == "*" || rest == "@" {
                ParamOp::NamesWithPrefix
            } else if rest.is_empty() {
                ParamOp::Indirect
            } else {
                return Err(bad());
            };
            return Ok(WordPart::Param(Box::new(ParamExpr { name, index, op })));
        }
        _ => {}
    }

    if prefix.is_none() && !rest.is_empty() {
        let r: Vec<char> = rest.chars().collect();
        let after = |n: usize| -> String { r[n..].iter().collect() };
        let word = |n: usize| -> Result<Vec<WordPart>> { parse_embedded(&after(n)) };
        op = match (r[0], r.get(1).copied()) {
            (':', Some('-')) => ParamOp::Default { colon: true, word: word(2)? },
            (':', Some('=')) => ParamOp::Assign { colon: true, word: word(2)? },
            (':', Some('?')) => ParamOp::Error { colon: true, word: word(2)? },
            (':', Some('+')) => ParamOp::Alt { colon: true, word: word(2)? },
            ('-', _) => ParamOp::Default { colon: false, word: word(1)? },
            ('=', _) => ParamOp::Assign { colon: false, word: word(1)? },
            ('?', _) => ParamOp::Error { colon: false, word: word(1)? },
            ('+', _) => ParamOp::Alt { colon: false, word: word(1)? },
            ('#', Some('#')) => ParamOp::RemovePrefix { longest: true, pat: word(2)? },
            ('#', _) => ParamOp::RemovePrefix { longest: false, pat: word(1)? },
            ('%', Some('%')) => ParamOp::RemoveSuffix { longest: true, pat: word(2)? },
            ('%', _) => ParamOp::RemoveSuffix { longest: false, pat: word(1)? },
            ('/', next) => {
                let (mode, skip) = match next {
                    Some('/') => (ReplaceMode::All, 2),
                    Some('#') => (ReplaceMode::Prefix, 2),
                    Some('%') => (ReplaceMode::Suffix, 2),
                    _ => (ReplaceMode::First, 1),
                };
                let body = after(skip);
                let (pat, rep) = split_unquoted(&body, '/');
                ParamOp::Replace { mode, pat: parse_embedded(&pat)?, rep: parse_embedded(rep.as_deref().unwrap_or(""))? }
            }
            (':', _) => {
                let body = after(1);
                let (offset, length) = split_unquoted(&body, ':');
                ParamOp::Substr { offset, length }
            }
            ('^', Some('^')) => ParamOp::Case { upper: true, all: true },
            ('^', _) => ParamOp::Case { upper: true, all: false },
            (',', Some(',')) => ParamOp::Case { upper: false, all: true },
            (',', _) => ParamOp::Case { upper: false, all: false },
            ('@', Some(t)) if r.len() == 2 => ParamOp::Transform(t),
            _ => return Err(bad()),
        };
    }

    if op == ParamOp::Plain && index.is_none() {
        return Ok(WordPart::Var(name));
    }
    Ok(WordPart::Param(Box::new(ParamExpr { name, index, op })))
}

/// Tách `s` tại ký tự `sep` đầu tiên KHÔNG nằm trong nháy / sau `\`.
fn split_unquoted(s: &str, sep: char) -> (String, Option<String>) {
    let mut quote: Option<char> = None;
    let mut escaped = false;
    for (idx, c) in s.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => {}
            None if c == '\\' => escaped = true,
            None if c == '\'' || c == '"' => quote = Some(c),
            None if c == sep => return (s[..idx].to_string(), Some(s[idx + c.len_utf8()..].to_string())),
            None => {}
        }
    }
    (s.to_string(), None)
}

/// Lex chuỗi con (giá trị mặc định, pattern…) thành các phần của 1 từ; khoảng
/// trắng và toán tử là chữ thường.
pub fn parse_embedded(s: &str) -> Result<Vec<WordPart>> {
    let bytes: Vec<char> = s.chars().collect();
    let (parts, _) = read_word(&bytes, 0, true)?;
    Ok(parts)
}

/// `(( expr ))` / `$(( expr ))`: đọc từ `start` (ngay sau `((`) tới `))` khớp.
/// `None` nếu ngoặc không khép bằng `))` (khi đó là subshell / `$(` lồng).
fn read_arith(bytes: &[char], start: usize) -> Option<(String, usize)> {
    let mut depth = 0i32;
    let mut i = start;
    while i < bytes.len() {
        match bytes[i] {
            '(' => depth += 1,
            ')' if depth > 0 => depth -= 1,
            ')' => {
                if bytes.get(i + 1) == Some(&')') {
                    return Some((bytes[start..i].iter().collect(), i + 2));
                }
                return None;
            }
            '\'' | '"' => {
                // Nháy trong biểu thức số học (hiếm) — bỏ qua tới nháy đóng.
                let q = bytes[i];
                i += 1;
                while i < bytes.len() && bytes[i] != q {
                    i += 1;
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

/// Unescape kiểu `$'...'`: \n \t \\ \' \" \e \xHH \NNN (bát phân) …
fn ansi_c_unescape(raw: &str) -> String {
    let mut out = String::new();
    let mut chars = raw.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        let Some(n) = chars.next() else {
            out.push('\\');
            break;
        };
        match n {
            '\'' | '"' | '?' => out.push(n),
            'x' | '0'..='7' => {
                let (radix, max) = if n == 'x' { (16, 2) } else { (8, 3) };
                let mut digits = if n == 'x' { String::new() } else { n.to_string() };
                while digits.len() < max {
                    match chars.peek() {
                        Some(d) if d.is_digit(radix) => {
                            digits.push(*d);
                            chars.next();
                        }
                        _ => break,
                    }
                }
                match u32::from_str_radix(&digits, radix).ok().and_then(char::from_u32) {
                    Some(ch) if !digits.is_empty() => out.push(ch),
                    _ => out.push_str("\\x"),
                }
            }
            _ => out.push_str(&crate::builtins::interpret_escapes(&format!("\\{}", n)).0),
        }
    }
    out
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
            '\\' if i + 1 < bytes.len() => {
                inner.push(c);
                inner.push(bytes[i + 1]);
                i += 2;
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
    Err(incomplete("$(...) không đóng"))
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
    Err(incomplete("`...` không đóng"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(input: &str) -> Vec<String> {
        tokenize(input)
            .unwrap()
            .into_iter()
            .filter_map(|t| match t {
                Token::Word(parts) => Some(parts_to_source(&parts)),
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
        assert_eq!(word_parts("echo $(date +%s)", 1), vec![WordPart::CmdSub("date +%s".into())]);
    }

    #[test]
    fn backtick_is_command_substitution() {
        assert_eq!(word_parts("echo `uname -s`", 1), vec![WordPart::CmdSub("uname -s".into())]);
    }

    #[test]
    fn nested_command_substitution() {
        // ngoặc lồng nhau không kết thúc sớm
        assert_eq!(word_parts("echo $(a $(b) c)", 1), vec![WordPart::CmdSub("a $(b) c".into())]);
    }

    #[test]
    fn cmdsub_inside_double_quotes() {
        // $(...) trong "..." vẫn là cmdsub, nằm trong nhóm DQ
        assert_eq!(
            word_parts("x\"$(id -u)\"", 0),
            vec![WordPart::Literal("x".into()), WordPart::DQ(vec![WordPart::CmdSub("id -u".into())])]
        );
    }

    #[test]
    fn single_quotes_keep_cmdsub_literal() {
        // trong nháy đơn, $(...) là chuỗi thường, KHÔNG chạy
        assert_eq!(word_parts("'$(nope)'", 0), vec![WordPart::Quoted("$(nope)".into())]);
    }

    #[test]
    fn cmdsub_paren_inside_quotes_does_not_close_early() {
        // dấu ) nằm trong nháy bên trong $(...) không kết thúc cmdsub
        assert_eq!(word_parts("echo $(echo \")\")", 1), vec![WordPart::CmdSub("echo \")\"".into())]);
    }

    #[test]
    fn unclosed_is_incomplete() {
        for src in ["echo $(oops", "echo `oops", "echo 'a", "echo \"a", "echo ${x"] {
            let e = tokenize(src).unwrap_err();
            assert!(e.downcast_ref::<Incomplete>().is_some(), "{src}");
        }
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
            vec![WordPart::DQ(vec![
                WordPart::Quoted("hi ".into()),
                WordPart::Var("USER".into()),
                WordPart::Quoted("\n".into())
            ])]
        );
        // lệnh sau delimiter vẫn được tokenize
        assert!(words("cat <<EOF\nx\nEOF\necho after").ends_with(&["echo".to_string(), "after".to_string()]));
        assert!(!last_heredoc_unterminated());
        tokenize("cat <<EOF\nx").unwrap();
        assert!(last_heredoc_unterminated());
    }

    #[test]
    fn heredoc_quoted_and_dash() {
        assert_eq!(heredoc_body("cat <<'EOF'\nraw $HOME\nEOF"), vec![WordPart::Quoted("raw $HOME\n".into())]);
        assert_eq!(
            heredoc_body("cat <<-EOF\n\t\tindented\n\tEOF"),
            vec![WordPart::DQ(vec![WordPart::Quoted("indented\n".into())])]
        );
    }

    #[test]
    fn positional_vars() {
        assert_eq!(word_parts("$10", 0), vec![WordPart::Var("1".into()), WordPart::Literal("0".into())]);
        assert_eq!(word_parts("${10}", 0), vec![WordPart::Var("10".into())]);
        assert_eq!(word_parts("\"$@\"", 0), vec![WordPart::DQ(vec![WordPart::Var("@".into())])]);
    }

    fn param(src: &str) -> ParamExpr {
        match word_parts(src, 0).remove(0) {
            WordPart::Param(p) => *p,
            other => panic!("{src}: {other:?}"),
        }
    }

    #[test]
    fn param_operators() {
        assert_eq!(param("${#x}").op, ParamOp::Length);
        assert_eq!(param("${x:-a b}").op, ParamOp::Default { colon: true, word: vec![WordPart::Literal("a b".into())] });
        assert!(matches!(param("${p##*/}").op, ParamOp::RemovePrefix { longest: true, .. }));
        assert!(matches!(param("${p%.*}").op, ParamOp::RemoveSuffix { longest: false, .. }));
        assert!(matches!(param("${x//X/-}").op, ParamOp::Replace { mode: ReplaceMode::All, .. }));
        assert_eq!(param("${x:1:3}").op, ParamOp::Substr { offset: "1".into(), length: Some("3".into()) });
        assert_eq!(param("${x^^}").op, ParamOp::Case { upper: true, all: true });
        assert_eq!(param("${!a}").op, ParamOp::Indirect);
        let p = param("${a[@]}");
        assert_eq!((p.name.as_str(), p.index.as_deref()), ("a", Some("@")));
        assert_eq!(param("${!a[@]}").op, ParamOp::Keys);
        assert_eq!(param("${#a[@]}").op, ParamOp::Length);
        // $# và $! vẫn là biến
        assert_eq!(word_parts("${#}", 0), vec![WordPart::Var("#".into())]);
    }

    #[test]
    fn arith_and_parens() {
        assert_eq!(word_parts("$((1 + (2*3)))", 0), vec![WordPart::Arith("1 + (2*3)".into())]);
        let t = tokenize("((x++)); (cd /; pwd)").unwrap();
        assert_eq!(t[0], Token::DParen("x++".into()));
        assert!(t.contains(&Token::LParen) && t.contains(&Token::RParen));
        // `f()` = từ f + ( + )
        assert_eq!(tokenize("f() {").unwrap()[1], Token::LParen);
    }

    #[test]
    fn escapes_are_quoted() {
        assert_eq!(word_parts("\\*.txt", 0), vec![WordPart::Quoted("*".into()), WordPart::Literal(".txt".into())]);
        assert_eq!(word_parts("$'a\\tb\\x41'", 0), vec![WordPart::Quoted("a\tbA".into())]);
    }

    #[test]
    fn case_terminators_and_procsub() {
        let t = tokenize("a) x;; b) y;& c) z;;&").unwrap();
        assert!(t.contains(&Token::DSemi) && t.contains(&Token::SemiAnd) && t.contains(&Token::DSemiAnd));
        assert_eq!(word_parts("diff <(ls) x", 1), vec![WordPart::ProcSub("ls".into(), true)]);
    }

    #[test]
    fn tilde_user() {
        assert_eq!(word_parts("~/x", 0), vec![WordPart::Tilde(String::new()), WordPart::Literal("/x".into())]);
        assert_eq!(word_parts("~root/x", 0)[0], WordPart::Tilde("root".into()));
        assert_eq!(word_parts("a~b", 0), vec![WordPart::Literal("a~b".into())]);
    }
}
