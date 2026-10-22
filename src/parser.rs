use anyhow::{anyhow, Result};
use std::rc::Rc;

use crate::lexer::{incomplete, RedirOp, Token, WordPart};

pub type Word = Vec<WordPart>;

#[derive(Debug, Clone)]
pub struct Redirect {
    /// fd bị chuyển hướng (0 stdin, 1 stdout, 2 stderr, 3+ tuỳ ý).
    pub fd: i32,
    pub kind: RedirectKind,
    /// Đích: tên file; với `DupOut/DupIn` là số fd hoặc `-` (đóng); với
    /// `HereDoc/HereString` là nội dung.
    pub target: Word,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RedirectKind {
    In,
    Out,
    Append,
    /// `>|` — ghi đè kể cả khi `set -o noclobber`.
    Clobber,
    ReadWrite,
    DupOut,
    DupIn,
    /// `&>` / `&>>` — stdout + stderr cùng vào 1 file.
    AllOut,
    AllAppend,
    HereDoc,
    HereString,
}

/// Phép gán `NAME=v`, `NAME+=v`, `NAME[i]=v`, `NAME=(a b c)`.
#[derive(Debug, Clone)]
pub struct Assign {
    pub name: String,
    /// Chỉ số trong `NAME[...]=` (chưa expand / tính) — có thể chứa `$i`.
    pub index: Option<Word>,
    pub append: bool,
    pub value: AssignValue,
}

#[derive(Debug, Clone)]
pub enum AssignValue {
    Scalar(Word),
    /// `(a b [k]=v …)` — mỗi phần tử là 1 từ (có thể dạng `[k]=v`).
    Array(Vec<Word>),
}

#[derive(Debug, Clone, Default)]
pub struct SimpleCommand {
    /// Phép gán đứng trước tên lệnh.
    pub assigns: Vec<Assign>,
    pub words: Vec<Word>,
    pub redirects: Vec<Redirect>,
    /// `declare/local/… a=(x y)`: (vị trí trong `words`, phép gán mảng).
    pub array_args: Vec<(usize, Assign)>,
}

#[derive(Debug, Clone)]
pub enum Command {
    Simple(SimpleCommand),
    Compound(Box<Compound>, Vec<Redirect>),
    /// Định nghĩa function — thân là lệnh ghép (thường là `{ … }`).
    FuncDef(String, Rc<Command>),
}

#[derive(Debug, Clone)]
pub enum Compound {
    Group(Program),
    Subshell(Program),
    /// Các cặp (điều kiện, thân) của `if` / `elif`, rồi nhánh `else`.
    If(Vec<(Program, Program)>, Option<Program>),
    For { var: String, items: Option<Vec<Word>>, body: Program },
    ArithFor { init: String, cond: String, step: String, body: Program },
    While { cond: Program, body: Program, until: bool },
    Case { word: Word, arms: Vec<CaseArm> },
    Cond(CondExpr),
    Arith(String),
}

#[derive(Debug, Clone)]
pub struct CaseArm {
    pub patterns: Vec<Word>,
    pub body: Program,
    pub term: CaseTerm,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaseTerm {
    /// `;;` — dừng.
    Break,
    /// `;&` — chạy luôn thân nhánh sau.
    FallThrough,
    /// `;;&` — thử tiếp các pattern sau.
    Continue,
}

/// Biểu thức `[[ … ]]`.
#[derive(Debug, Clone)]
pub enum CondExpr {
    And(Box<CondExpr>, Box<CondExpr>),
    Or(Box<CondExpr>, Box<CondExpr>),
    Not(Box<CondExpr>),
    Unary(String, Word),
    Binary(Word, String, Word),
    Word(Word),
}

#[derive(Debug, Clone)]
pub struct Pipeline {
    pub commands: Vec<Command>,
    pub background: bool,
    /// `! cmd` — đảo exit code (0 ↔ 1).
    pub negate: bool,
    /// `time cmd` — in thời gian chạy ra stderr.
    pub timed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SeqOp {
    Always,
    AndIf,
    OrIf,
}

#[derive(Debug, Clone, Default)]
pub struct Program {
    pub items: Vec<(Pipeline, SeqOp)>,
}

pub fn parse(tokens: &[Token]) -> Result<Program> {
    let mut p = Parser { tokens, pos: 0 };
    let prog = p.parse_list(&[], false)?;
    if let Some(t) = p.peek() {
        return Err(anyhow!("lỗi cú pháp gần `{}`", describe(t)));
    }
    Ok(prog)
}

/// Input (REPL) chưa trọn — còn thiếu `fi`, `done`, nháy đóng, thân here-doc…?
pub fn needs_more(input: &str) -> bool {
    if ends_with_continuation(input) {
        return true;
    }
    match crate::lexer::tokenize(input) {
        Err(e) => e.downcast_ref::<crate::lexer::Incomplete>().is_some(),
        Ok(tokens) => {
            if crate::lexer::last_heredoc_unterminated() {
                return true;
            }
            matches!(parse(&tokens), Err(e) if e.downcast_ref::<crate::lexer::Incomplete>().is_some())
        }
    }
}

/// Dòng kết thúc bằng `\` (số lẻ dấu) = nối dòng.
fn ends_with_continuation(input: &str) -> bool {
    let trimmed = input.trim_end_matches(['\n', '\r']);
    trimmed.chars().rev().take_while(|&c| c == '\\').count() % 2 == 1
}

fn describe(t: &Token) -> String {
    match t {
        Token::Word(w) => crate::lexer::parts_to_source(w),
        Token::Pipe => "|".into(),
        Token::AndIf => "&&".into(),
        Token::OrIf => "||".into(),
        Token::Semicolon => ";".into(),
        Token::Ampersand => "&".into(),
        Token::LParen => "(".into(),
        Token::RParen => ")".into(),
        Token::DSemi => ";;".into(),
        Token::SemiAnd => ";&".into(),
        Token::DSemiAnd => ";;&".into(),
        Token::DParen(e) => format!("(({}))", e),
        Token::Redir(..) | Token::HereDoc(..) => "redirect".into(),
    }
}

/// Từ chỉ gồm 1 Literal (không nháy) — dạng duy nhất được coi là từ khoá.
fn plain(t: Option<&Token>) -> Option<&str> {
    match t {
        Some(Token::Word(parts)) => match parts.as_slice() {
            [WordPart::Literal(s)] => Some(s.as_str()),
            _ => None,
        },
        _ => None,
    }
}

/// `NAME=v`, `NAME+=v`, `NAME[i]=v` → (tên, chỉ số, append, phần giá trị).
/// Chỉ số có thể chứa expansion (`a[$i]=`, `m[${k}]=`) nên `]=` có thể nằm ở
/// một phần sau của từ.
fn split_assignment(word: &Word) -> Option<(String, Option<Word>, bool, Word)> {
    let WordPart::Literal(first) = word.first()? else { return None };
    // Tên chỉ gồm ASCII nên số ký tự = số byte.
    let name_len = first.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '_').count();
    let name = &first[..name_len];
    if name.is_empty() || name.starts_with(|c: char| c.is_ascii_digit()) {
        return None;
    }
    let rest = &first[name_len..];
    // Phần giá trị bắt đầu sau `=` tại (chỉ số part, vị trí byte trong Literal đó).
    let finish = |append: bool, index: Option<Word>, part: usize, at: usize| {
        let mut value: Word = Vec::new();
        if let WordPart::Literal(s) = &word[part] {
            if at < s.len() {
                value.push(WordPart::Literal(s[at..].to_string()));
            }
        }
        value.extend(word[part + 1..].iter().cloned());
        Some((name.to_string(), index, append, value))
    };
    if rest.starts_with("+=") {
        return finish(true, None, 0, name_len + 2);
    }
    if rest.starts_with('=') {
        return finish(false, None, 0, name_len + 1);
    }
    let inner_start = rest.strip_prefix('[')?;
    // Gom chỉ số tới `]=` / `]+=` (có thể vượt qua nhiều phần).
    let mut index: Word = Vec::new();
    let mut pending = inner_start.to_string();
    let mut part = 0;
    let mut offset = name_len + 1; // vị trí byte trong Literal hiện tại sau `[`
    loop {
        for marker in ["]+=", "]="] {
            if let Some(pos) = pending.find(marker) {
                if !pending[..pos].is_empty() {
                    index.push(WordPart::Literal(pending[..pos].to_string()));
                }
                let append = marker == "]+=";
                return finish(append, Some(index), part, offset + pos + marker.len());
            }
        }
        if !pending.is_empty() {
            index.push(WordPart::Literal(pending.clone()));
        }
        part += 1;
        match word.get(part)? {
            WordPart::Literal(s) => {
                pending = s.clone();
                offset = 0;
            }
            other => {
                index.push(other.clone());
                pending.clear();
                // phần không phải Literal: tiếp tục tìm `]=` ở phần sau
                offset = 0;
                if word.get(part + 1).is_none() {
                    return None;
                }
            }
        }
    }
}

/// Lệnh builtin nhận đối số dạng phép gán (`local x=$(cmd)` không bị tách từ).
pub fn is_decl_command(name: &str) -> bool {
    matches!(name, "declare" | "typeset" | "local" | "export" | "readonly")
}

struct Parser<'a> {
    tokens: &'a [Token],
    pos: usize,
}

impl<'a> Parser<'a> {
    fn peek(&self) -> Option<&'a Token> {
        self.tokens.get(self.pos)
    }

    fn advance(&mut self) -> Option<&'a Token> {
        let t = self.tokens.get(self.pos);
        if t.is_some() {
            self.pos += 1;
        }
        t
    }

    fn at_reserved(&self, words: &[&str]) -> bool {
        plain(self.peek()).is_some_and(|w| words.contains(&w))
    }

    fn skip_separators(&mut self) {
        while matches!(self.peek(), Some(Token::Semicolon)) {
            self.advance();
        }
    }

    /// Đòi từ khoá `kw` (bỏ qua xuống dòng trước nó).
    fn expect_reserved(&mut self, kw: &str) -> Result<()> {
        self.skip_separators();
        match self.peek() {
            None => Err(incomplete(&format!("thiếu `{}`", kw))),
            Some(_) if self.at_reserved(&[kw]) => {
                self.advance();
                Ok(())
            }
            Some(t) => Err(anyhow!("lỗi cú pháp: cần `{}` nhưng gặp `{}`", kw, describe(t))),
        }
    }

    /// Danh sách lệnh tới khi gặp 1 từ khoá trong `terms` (ở vị trí lệnh), hoặc
    /// `)` / `;;` khi `stop_at_close`. `terms` rỗng = tới hết input.
    fn parse_list(&mut self, terms: &[&str], stop_at_close: bool) -> Result<Program> {
        let mut items: Vec<(Pipeline, SeqOp)> = Vec::new();
        let mut next_op = SeqOp::Always;
        loop {
            // Bỏ qua các dấu `;` thừa (dòng trống khi paste nhiều dòng, `;`
            // đầu/cuối); sau `&&` / `||` cũng được phép xuống dòng.
            self.skip_separators();
            match self.peek() {
                None => {
                    if next_op != SeqOp::Always {
                        return Err(incomplete("lệnh còn dở sau `&&` / `||`"));
                    }
                    if !terms.is_empty() {
                        return Err(incomplete(&format!("thiếu `{}`", terms[0])));
                    }
                    break;
                }
                Some(Token::RParen | Token::DSemi | Token::SemiAnd | Token::DSemiAnd) if stop_at_close => break,
                Some(_) if self.at_reserved(terms) => break,
                _ => {}
            }
            let pipeline = self.parse_pipeline()?;
            let background = pipeline.background;
            items.push((pipeline, next_op));
            next_op = SeqOp::Always;
            match self.peek() {
                Some(Token::Semicolon) => {
                    self.advance();
                }
                Some(Token::AndIf) => {
                    self.advance();
                    next_op = SeqOp::AndIf;
                }
                Some(Token::OrIf) => {
                    self.advance();
                    next_op = SeqOp::OrIf;
                }
                None => break,
                Some(Token::RParen | Token::DSemi | Token::SemiAnd | Token::DSemiAnd) if stop_at_close => break,
                Some(_) if self.at_reserved(terms) => break,
                // `a & b` — `&` vừa đưa `a` xuống nền vừa là dấu phân tách.
                Some(_) if background => {}
                Some(t) => return Err(anyhow!("lỗi cú pháp gần `{}`", describe(t))),
            }
        }
        Ok(Program { items })
    }

    fn parse_pipeline(&mut self) -> Result<Pipeline> {
        let mut negate = false;
        let mut timed = false;
        loop {
            // `!` / `time` đứng riêng ở đầu pipeline (không phải `!=`, `!x`).
            if plain(self.peek()) == Some("!") && self.tokens.get(self.pos + 1).is_some() {
                self.advance();
                negate = !negate;
            } else if plain(self.peek()) == Some("time") && self.tokens.get(self.pos + 1).is_some() {
                self.advance();
                timed = true;
                if plain(self.peek()) == Some("-p") {
                    self.advance();
                }
            } else {
                break;
            }
        }
        let mut commands = vec![self.parse_command()?];
        while let Some(Token::Pipe) = self.peek() {
            self.advance();
            self.skip_separators();
            if self.peek().is_none() {
                return Err(incomplete("lệnh còn dở sau `|`"));
            }
            commands.push(self.parse_command()?);
        }
        let mut background = false;
        if let Some(Token::Ampersand) = self.peek() {
            self.advance();
            background = true;
        }
        Ok(Pipeline { commands, background, negate, timed })
    }

    fn parse_command(&mut self) -> Result<Command> {
        let compound = match self.peek() {
            None => return Err(incomplete("thiếu lệnh")),
            Some(Token::LParen) => {
                self.advance();
                let body = self.parse_list(&[], true)?;
                match self.advance() {
                    Some(Token::RParen) => Compound::Subshell(body),
                    None => return Err(incomplete("thiếu `)`")),
                    Some(t) => return Err(anyhow!("lỗi cú pháp: cần `)` nhưng gặp `{}`", describe(t))),
                }
            }
            Some(Token::DParen(expr)) => {
                self.advance();
                Compound::Arith(expr.clone())
            }
            Some(_) => match plain(self.peek()) {
                Some("{") => {
                    self.advance();
                    let body = self.parse_list(&["}"], false)?;
                    self.expect_reserved("}")?;
                    Compound::Group(body)
                }
                Some("if") => self.parse_if()?,
                Some("while") | Some("until") => {
                    let until = plain(self.advance()) == Some("until");
                    let cond = self.parse_list(&["do"], false)?;
                    self.expect_reserved("do")?;
                    let body = self.parse_list(&["done"], false)?;
                    self.expect_reserved("done")?;
                    Compound::While { cond, body, until }
                }
                Some("for") => self.parse_for()?,
                Some("case") => self.parse_case()?,
                Some("[[") => {
                    self.advance();
                    let expr = self.parse_cond_or()?;
                    if plain(self.peek()) != Some("]]") {
                        return match self.peek() {
                            None => Err(incomplete("thiếu `]]`")),
                            Some(t) => Err(anyhow!("lỗi cú pháp trong [[ ]] gần `{}`", describe(t))),
                        };
                    }
                    self.advance();
                    Compound::Cond(expr)
                }
                Some("function") => {
                    self.advance();
                    let name = match self.advance() {
                        Some(Token::Word(w)) => crate::lexer::parts_to_source(w),
                        None => return Err(incomplete("thiếu tên function")),
                        Some(t) => return Err(anyhow!("tên function không hợp lệ: `{}`", describe(t))),
                    };
                    if self.peek() == Some(&Token::LParen) && self.tokens.get(self.pos + 1) == Some(&Token::RParen) {
                        self.pos += 2;
                    }
                    return self.parse_func_body(name);
                }
                Some(w) if matches!(w, "then" | "else" | "elif" | "fi" | "do" | "done" | "esac" | "}" | "in" | "]]") => {
                    return Err(anyhow!("lỗi cú pháp gần từ khoá `{}`", w));
                }
                _ => {
                    // `name() { … }` — định nghĩa function (không phải `arr=()`).
                    if let Some(Token::Word(w)) = self.peek() {
                        if split_assignment(w).is_none()
                            && self.tokens.get(self.pos + 1) == Some(&Token::LParen)
                            && self.tokens.get(self.pos + 2) == Some(&Token::RParen)
                        {
                            let name = crate::lexer::parts_to_source(w);
                            self.pos += 3;
                            return self.parse_func_body(name);
                        }
                    }
                    return Ok(Command::Simple(self.parse_simple()?));
                }
            },
        };
        let redirects = self.parse_redirects()?;
        Ok(Command::Compound(Box::new(compound), redirects))
    }

    fn parse_func_body(&mut self, name: String) -> Result<Command> {
        self.skip_separators();
        if self.peek().is_none() {
            return Err(incomplete("thiếu thân function"));
        }
        let body = self.parse_command()?;
        if !matches!(body, Command::Compound(..)) {
            return Err(anyhow!("thân function `{}` phải là lệnh ghép (vd `{{ ...; }}`)", name));
        }
        Ok(Command::FuncDef(name, Rc::new(body)))
    }

    fn parse_redirects(&mut self) -> Result<Vec<Redirect>> {
        let mut out = Vec::new();
        while let Some(r) = self.try_redirect()? {
            out.push(r);
        }
        Ok(out)
    }

    fn try_redirect(&mut self) -> Result<Option<Redirect>> {
        match self.peek() {
            Some(Token::Redir(fd, op)) => {
                self.advance();
                let (default_fd, kind, sym) = match op {
                    RedirOp::In => (0, RedirectKind::In, "<"),
                    RedirOp::Out => (1, RedirectKind::Out, ">"),
                    RedirOp::Append => (1, RedirectKind::Append, ">>"),
                    RedirOp::Clobber => (1, RedirectKind::Clobber, ">|"),
                    RedirOp::ReadWrite => (0, RedirectKind::ReadWrite, "<>"),
                    RedirOp::DupOut => (1, RedirectKind::DupOut, ">&"),
                    RedirOp::DupIn => (0, RedirectKind::DupIn, "<&"),
                    RedirOp::AllOut => (1, RedirectKind::AllOut, "&>"),
                    RedirOp::AllAppend => (1, RedirectKind::AllAppend, "&>>"),
                    RedirOp::HereString => (0, RedirectKind::HereString, "<<<"),
                };
                let target = match self.advance() {
                    Some(Token::Word(w)) => w.clone(),
                    None => return Err(incomplete(&format!("thiếu đích cho '{}'", sym))),
                    Some(_) => {
                        return Err(anyhow!(
                            "thiếu đích cho '{}{}'",
                            fd.map(|n| n.to_string()).unwrap_or_default(),
                            sym
                        ))
                    }
                };
                Ok(Some(Redirect { fd: fd.unwrap_or(default_fd), kind, target }))
            }
            Some(Token::HereDoc(fd, body)) => {
                self.advance();
                Ok(Some(Redirect { fd: fd.unwrap_or(0), kind: RedirectKind::HereDoc, target: body.clone() }))
            }
            _ => Ok(None),
        }
    }

    /// `(a b [k]=v)` sau `NAME=` — đọc tới `)` (cho phép xuống dòng).
    fn parse_array_literal(&mut self) -> Result<Vec<Word>> {
        self.advance(); // (
        let mut items = Vec::new();
        loop {
            match self.advance() {
                Some(Token::Word(w)) => items.push(w.clone()),
                Some(Token::Semicolon) => {}
                Some(Token::RParen) => return Ok(items),
                None => return Err(incomplete("thiếu `)` của mảng")),
                Some(t) => return Err(anyhow!("lỗi cú pháp trong mảng gần `{}`", describe(t))),
            }
        }
    }

    fn parse_simple(&mut self) -> Result<SimpleCommand> {
        let mut cmd = SimpleCommand::default();
        loop {
            if let Some(r) = self.try_redirect()? {
                cmd.redirects.push(r);
                continue;
            }
            let Some(Token::Word(w)) = self.peek() else { break };
            // Phép gán chỉ được nhận diện trước tên lệnh (và làm đối số của
            // declare/local/export… để giá trị không bị tách từ).
            let decl_arg = cmd
                .words
                .first()
                .is_some_and(|f| matches!(f.as_slice(), [WordPart::Literal(s)] if is_decl_command(s)));
            if cmd.words.is_empty() || decl_arg {
                if let Some((name, index, append, value)) = split_assignment(w) {
                    self.advance();
                    let array = value.is_empty() && self.peek() == Some(&Token::LParen);
                    let value =
                        if array { AssignValue::Array(self.parse_array_literal()?) } else { AssignValue::Scalar(value) };
                    let assign = Assign { name: name.clone(), index, append, value };
                    if cmd.words.is_empty() {
                        cmd.assigns.push(assign);
                    } else if array {
                        // Đối số mảng của declare: builtin nhận tên, executor gán mảng.
                        cmd.array_args.push((cmd.words.len(), assign));
                        cmd.words.push(vec![WordPart::Literal(name)]);
                    } else {
                        cmd.words.push(w.clone());
                    }
                    continue;
                }
            }
            self.advance();
            cmd.words.push(w.clone());
        }
        if cmd.words.is_empty() && cmd.assigns.is_empty() && cmd.redirects.is_empty() {
            return match self.peek() {
                None => Err(incomplete("thiếu lệnh")),
                Some(t) => Err(anyhow!("lỗi cú pháp gần `{}`", describe(t))),
            };
        }
        Ok(cmd)
    }

    fn parse_if(&mut self) -> Result<Compound> {
        self.advance(); // if
        let mut arms = Vec::new();
        let mut else_body = None;
        loop {
            let cond = self.parse_list(&["then"], false)?;
            self.expect_reserved("then")?;
            let body = self.parse_list(&["elif", "else", "fi"], false)?;
            arms.push((cond, body));
            self.skip_separators();
            match plain(self.advance()) {
                Some("elif") => continue,
                Some("else") => {
                    else_body = Some(self.parse_list(&["fi"], false)?);
                    self.expect_reserved("fi")?;
                    break;
                }
                Some("fi") => break,
                _ => return Err(incomplete("thiếu `fi`")),
            }
        }
        Ok(Compound::If(arms, else_body))
    }

    fn parse_for(&mut self) -> Result<Compound> {
        self.advance(); // for
        if let Some(Token::DParen(expr)) = self.peek() {
            self.advance();
            let parts: Vec<&str> = expr.splitn(3, ';').collect();
            if parts.len() != 3 {
                return Err(anyhow!("for ((...)): cần dạng `for ((khởi tạo; điều kiện; bước))`"));
            }
            let body = self.parse_do_block()?;
            return Ok(Compound::ArithFor {
                init: parts[0].trim().to_string(),
                cond: parts[1].trim().to_string(),
                step: parts[2].trim().to_string(),
                body,
            });
        }
        let var = match self.advance() {
            Some(Token::Word(w)) => crate::lexer::parts_to_source(w),
            None => return Err(incomplete("thiếu tên biến sau `for`")),
            Some(t) => return Err(anyhow!("lỗi cú pháp sau `for`: `{}`", describe(t))),
        };
        if var.is_empty() || !var.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            return Err(anyhow!("for: `{}`: tên biến không hợp lệ", var));
        }
        self.skip_separators();
        let mut items = None;
        if self.at_reserved(&["in"]) {
            self.advance();
            let mut list = Vec::new();
            while let Some(Token::Word(w)) = self.peek() {
                self.advance();
                list.push(w.clone());
            }
            items = Some(list);
        }
        let body = self.parse_do_block()?;
        Ok(Compound::For { var, items, body })
    }

    /// `do … done` (cho phép `;` / xuống dòng trước `do`).
    fn parse_do_block(&mut self) -> Result<Program> {
        self.expect_reserved("do")?;
        let body = self.parse_list(&["done"], false)?;
        self.expect_reserved("done")?;
        Ok(body)
    }

    fn parse_case(&mut self) -> Result<Compound> {
        self.advance(); // case
        let word = match self.advance() {
            Some(Token::Word(w)) => w.clone(),
            None => return Err(incomplete("thiếu từ sau `case`")),
            Some(t) => return Err(anyhow!("lỗi cú pháp sau `case`: `{}`", describe(t))),
        };
        self.expect_reserved("in")?;
        let mut arms = Vec::new();
        loop {
            self.skip_separators();
            match self.peek() {
                None => return Err(incomplete("thiếu `esac`")),
                Some(_) if self.at_reserved(&["esac"]) => {
                    self.advance();
                    break;
                }
                _ => {}
            }
            if self.peek() == Some(&Token::LParen) {
                self.advance();
            }
            let mut patterns = Vec::new();
            loop {
                match self.advance() {
                    Some(Token::Word(w)) => patterns.push(w.clone()),
                    None => return Err(incomplete("thiếu `)` sau pattern")),
                    Some(t) => return Err(anyhow!("lỗi cú pháp trong pattern case gần `{}`", describe(t))),
                }
                match self.advance() {
                    Some(Token::Pipe) => continue,
                    Some(Token::RParen) => break,
                    None => return Err(incomplete("thiếu `)` sau pattern")),
                    Some(t) => return Err(anyhow!("lỗi cú pháp trong pattern case gần `{}`", describe(t))),
                }
            }
            let body = self.parse_list(&["esac"], true)?;
            let term = match self.peek() {
                Some(Token::DSemi) => CaseTerm::Break,
                Some(Token::SemiAnd) => CaseTerm::FallThrough,
                Some(Token::DSemiAnd) => CaseTerm::Continue,
                _ => {
                    // Nhánh cuối có thể bỏ `;;` trước `esac`.
                    arms.push(CaseArm { patterns, body, term: CaseTerm::Break });
                    self.expect_reserved("esac")?;
                    break;
                }
            };
            self.advance();
            arms.push(CaseArm { patterns, body, term });
        }
        Ok(Compound::Case { word, arms })
    }

    // ── [[ … ]] ────────────────────────────────────────────────────────────

    fn parse_cond_or(&mut self) -> Result<CondExpr> {
        let mut lhs = self.parse_cond_and()?;
        while self.peek() == Some(&Token::OrIf) {
            self.advance();
            self.skip_separators();
            let rhs = self.parse_cond_and()?;
            lhs = CondExpr::Or(Box::new(lhs), Box::new(rhs));
        }
        Ok(lhs)
    }

    fn parse_cond_and(&mut self) -> Result<CondExpr> {
        let mut lhs = self.parse_cond_not()?;
        while self.peek() == Some(&Token::AndIf) {
            self.advance();
            self.skip_separators();
            let rhs = self.parse_cond_not()?;
            lhs = CondExpr::And(Box::new(lhs), Box::new(rhs));
        }
        Ok(lhs)
    }

    fn parse_cond_not(&mut self) -> Result<CondExpr> {
        if plain(self.peek()) == Some("!") {
            self.advance();
            return Ok(CondExpr::Not(Box::new(self.parse_cond_not()?)));
        }
        self.parse_cond_primary()
    }

    fn cond_word(&mut self) -> Result<Word> {
        match self.advance() {
            Some(Token::Word(w)) => Ok(w.clone()),
            None => Err(incomplete("thiếu `]]`")),
            Some(t) => Err(anyhow!("lỗi cú pháp trong [[ ]] gần `{}`", describe(t))),
        }
    }

    fn parse_cond_primary(&mut self) -> Result<CondExpr> {
        if self.peek() == Some(&Token::LParen) {
            self.advance();
            let e = self.parse_cond_or()?;
            if self.advance() != Some(&Token::RParen) {
                return Err(anyhow!("[[ ]]: thiếu `)`"));
            }
            return Ok(e);
        }
        // Toán tử một ngôi: -f file, -z str, …
        if let Some(op) = plain(self.peek()) {
            if crate::cond::is_unary_op(op) && !matches!(plain(self.tokens.get(self.pos + 1)), Some("]]")) {
                let op = op.to_string();
                self.advance();
                let w = self.cond_word()?;
                return Ok(CondExpr::Unary(op, w));
            }
        }
        let lhs = self.cond_word()?;
        let op = match self.peek() {
            Some(Token::Redir(None, RedirOp::In)) => Some("<".to_string()),
            Some(Token::Redir(None, RedirOp::Out)) => Some(">".to_string()),
            _ => plain(self.peek()).filter(|o| crate::cond::is_binary_op(o)).map(str::to_string),
        };
        let Some(op) = op else {
            return Ok(CondExpr::Word(lhs));
        };
        self.advance();
        if op == "=~" {
            return Ok(CondExpr::Binary(lhs, op, self.parse_regex_word()?));
        }
        let rhs = self.cond_word()?;
        Ok(CondExpr::Binary(lhs, op, rhs))
    }

    /// Vế phải của `=~`: ghép các token liền nhau (kể cả `(`, `)`, `|`) tới
    /// `&&` / `||` / `]]` — regex không chứa dấu cách không nháy.
    fn parse_regex_word(&mut self) -> Result<Word> {
        let mut word: Word = Vec::new();
        let mut depth = 0;
        loop {
            match self.peek() {
                Some(Token::Word(_)) if plain(self.peek()) == Some("]]") && depth == 0 => break,
                Some(Token::Word(w)) => word.extend(w.iter().cloned()),
                Some(Token::LParen) => {
                    depth += 1;
                    word.push(WordPart::Literal("(".into()));
                }
                Some(Token::RParen) if depth > 0 => {
                    depth -= 1;
                    word.push(WordPart::Literal(")".into()));
                }
                Some(Token::Pipe) => word.push(WordPart::Literal("|".into())),
                None => return Err(incomplete("thiếu `]]`")),
                _ => break,
            }
            self.advance();
        }
        if word.is_empty() {
            return Err(anyhow!("[[ ]]: thiếu regex sau `=~`"));
        }
        Ok(word)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prog(src: &str) -> Program {
        parse(&crate::lexer::tokenize(src).unwrap()).unwrap()
    }

    fn first(src: &str) -> Command {
        prog(src).items.remove(0).0.commands.remove(0)
    }

    fn compound(src: &str) -> Compound {
        match first(src) {
            Command::Compound(c, _) => *c,
            other => panic!("{src}: {other:?}"),
        }
    }

    #[test]
    fn ampersand_separates_commands() {
        let p = prog("sleep 1 & echo hi");
        assert_eq!(p.items.len(), 2);
        assert!(p.items[0].0.background);
        assert!(!p.items[1].0.background);
    }

    #[test]
    fn bang_negates_pipeline() {
        let p = prog("! false && echo x");
        assert!(p.items[0].0.negate);
        // `!` một mình là lệnh, không phải phủ định
        assert!(!prog("!").items[0].0.negate);
    }

    #[test]
    fn redirect_default_fds() {
        let Command::Simple(c) = first("cmd < a > b 2>&1 <<< s") else { panic!() };
        let r: Vec<(i32, RedirectKind)> = c.redirects.iter().map(|r| (r.fd, r.kind)).collect();
        assert_eq!(
            r,
            vec![(0, RedirectKind::In), (1, RedirectKind::Out), (2, RedirectKind::DupOut), (0, RedirectKind::HereString)]
        );
    }

    #[test]
    fn compound_commands_parse() {
        assert!(matches!(compound("if a; then b; elif c; then d; else e; fi"), Compound::If(arms, Some(_)) if arms.len() == 2));
        assert!(matches!(compound("for x in 1 2; do echo $x; done"), Compound::For { items: Some(i), .. } if i.len() == 2));
        assert!(matches!(compound("for ((i=0;i<3;i++)); do :; done"), Compound::ArithFor { .. }));
        assert!(matches!(first("while read l\ndo\n echo $l\ndone < f"), Command::Compound(c, r) if matches!(*c, Compound::While { until: false, .. }) && r.len() == 1));
        assert!(matches!(compound("case $x in a|b) echo ab;; *) echo other;; esac"), Compound::Case { arms, .. } if arms.len() == 2 && arms[0].patterns.len() == 2));
        assert!(matches!(compound("case x in\n  a) echo a\nesac"), Compound::Case { arms, .. } if arms.len() == 1));
        assert!(matches!(compound("{ a; b; }"), Compound::Group(_)));
        assert!(matches!(compound("(cd /; pwd)"), Compound::Subshell(_)));
        assert!(matches!(first("f() { echo hi; }"), Command::FuncDef(n, _) if n == "f"));
        assert!(matches!(first("function g { :; }"), Command::FuncDef(n, _) if n == "g"));
        assert!(matches!(compound("[[ -f a && $x == y* ]]"), Compound::Cond(CondExpr::And(..))));
        assert!(matches!(compound("[[ v1.2 =~ ^v([0-9]+)\\.(.*)$ ]]"), Compound::Cond(CondExpr::Binary(_, op, _)) if op == "=~"));
        assert!(matches!(compound("((x++))"), Compound::Arith(e) if e == "x++"));
    }

    #[test]
    fn assignments_parse() {
        let Command::Simple(c) = first("A=1 B+=x arr=(a b c) cmd arg") else { panic!() };
        assert_eq!(c.assigns.len(), 3);
        assert!(c.assigns[1].append);
        assert!(matches!(c.assigns[2].value, AssignValue::Array(ref v) if v.len() == 3));
        assert_eq!(c.words.len(), 2);
        // sau tên lệnh, `X=1` là đối số thường
        let Command::Simple(c) = first("ls A=1") else { panic!() };
        assert!(c.assigns.is_empty() && c.words.len() == 2);
        // declare a=(…) → array_args
        let Command::Simple(c) = first("local -a a=(1 2)") else { panic!() };
        assert_eq!(c.array_args.len(), 1);
        // `arr=()` là mảng rỗng, không phải định nghĩa function
        // chỉ số có expansion
        let Command::Simple(c) = first("counts[$w]=$(( x + 1 )) m[${k}x]+=v") else { panic!() };
        assert_eq!(c.assigns.len(), 2);
        assert!(c.assigns[1].append && c.assigns[1].index.is_some());
        let Command::Simple(c) = first("arr=()") else { panic!() };
        assert!(matches!(c.assigns[0].value, AssignValue::Array(ref v) if v.is_empty()));
    }

    #[test]
    fn incomplete_input_detected() {
        for src in [
            "if true; then", "for x in a b; do echo", "while true", "case x in", "{ echo", "f() {", "echo 'a", "a |",
            "a &&", "echo \\", "cat <<EOF\nx", "[[ -f x",
        ] {
            assert!(needs_more(src), "phải chờ thêm: {src:?}");
        }
        for src in ["echo a", "if true; then echo; fi", "cat <<EOF\nx\nEOF", "echo a; fi"] {
            assert!(!needs_more(src), "đã đủ: {src:?}");
        }
    }
}
