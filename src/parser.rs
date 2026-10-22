use anyhow::{anyhow, Result};

use crate::lexer::{RedirOp, Token, WordPart};

#[derive(Debug, Clone)]
pub struct SimpleCommand {
    pub words: Vec<Vec<WordPart>>,
    pub redirects: Vec<Redirect>,
}

#[derive(Debug, Clone)]
pub struct Redirect {
    /// fd bị chuyển hướng (0 stdin, 1 stdout, 2 stderr, 3+ tuỳ ý).
    pub fd: i32,
    pub kind: RedirectKind,
    /// Đích: tên file; với `DupOut/DupIn` là số fd hoặc `-` (đóng); với
    /// `HereDoc/HereString` là nội dung.
    pub target: Vec<WordPart>,
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

#[derive(Debug, Clone)]
pub struct Pipeline {
    pub commands: Vec<SimpleCommand>,
    pub background: bool,
    /// `! cmd` — đảo exit code (0 ↔ 1).
    pub negate: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SeqOp {
    Always,
    AndIf,
    OrIf,
}

#[derive(Debug, Clone)]
pub struct Program {
    pub items: Vec<(Pipeline, SeqOp)>,
}

pub fn parse(tokens: &[Token]) -> Result<Program> {
    let mut p = Parser { tokens, pos: 0 };
    p.parse_program()
}

struct Parser<'a> {
    tokens: &'a [Token],
    pos: usize,
}

impl<'a> Parser<'a> {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos)
    }
    fn advance(&mut self) -> Option<&Token> {
        let t = self.tokens.get(self.pos);
        if t.is_some() {
            self.pos += 1;
        }
        t
    }

    fn parse_program(&mut self) -> Result<Program> {
        let mut items: Vec<(Pipeline, SeqOp)> = Vec::new();
        // The first pipeline's "op" describes how it connects to the previous one.
        // Since there's no previous, we mark it as Always.
        let mut next_op = SeqOp::Always;
        loop {
            // Bỏ qua các dấu `;` thừa (dòng trống khi paste nhiều dòng, `;`
            // đầu/cuối, `;;`). Mỗi cái reset op về Always.
            while matches!(self.peek(), Some(Token::Semicolon)) {
                self.advance();
                next_op = SeqOp::Always;
            }
            if self.peek().is_none() {
                break;
            }
            let pipeline = self.parse_pipeline()?;
            let background = pipeline.background;
            items.push((pipeline, next_op));
            match self.peek() {
                // `a & b` — `&` vừa đưa `a` xuống nền vừa là dấu phân tách.
                Some(Token::Word(_)) if background => {
                    next_op = SeqOp::Always;
                }
                Some(Token::Semicolon) => {
                    self.advance();
                    next_op = SeqOp::Always;
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
                Some(t) => return Err(anyhow!("token không mong đợi: {:?}", t)),
            }
        }
        Ok(Program { items })
    }

    fn parse_pipeline(&mut self) -> Result<Pipeline> {
        // `!` đứng riêng ở đầu pipeline = phủ định (không phải `!=`, `!x`).
        let mut negate = false;
        while matches!(self.peek(), Some(Token::Word(p)) if p == &[WordPart::Literal("!".into())])
            && self.tokens.get(self.pos + 1).is_some()
        {
            self.advance();
            negate = !negate;
        }
        let mut commands = vec![self.parse_command()?];
        while let Some(Token::Pipe) = self.peek() {
            self.advance();
            commands.push(self.parse_command()?);
        }
        let mut background = false;
        if let Some(Token::Ampersand) = self.peek() {
            self.advance();
            background = true;
        }
        Ok(Pipeline { commands, background, negate })
    }

    fn parse_command(&mut self) -> Result<SimpleCommand> {
        let mut words: Vec<Vec<WordPart>> = Vec::new();
        let mut redirects = Vec::new();
        loop {
            match self.peek().cloned() {
                Some(Token::Word(parts)) => {
                    self.advance();
                    words.push(parts);
                }
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
                    let target = self.expect_word(&format!("đích cho '{}{}'", fd.map(|n| n.to_string()).unwrap_or_default(), sym))?;
                    redirects.push(Redirect { fd: fd.unwrap_or(default_fd), kind, target });
                }
                Some(Token::HereDoc(fd, body)) => {
                    self.advance();
                    redirects.push(Redirect { fd: fd.unwrap_or(0), kind: RedirectKind::HereDoc, target: body });
                }
                _ => break,
            }
        }
        if words.is_empty() && redirects.is_empty() {
            return Err(anyhow!("lệnh trống"));
        }
        Ok(SimpleCommand { words, redirects })
    }

    fn expect_word(&mut self, ctx: &str) -> Result<Vec<WordPart>> {
        match self.advance() {
            Some(Token::Word(parts)) => Ok(parts.clone()),
            _ => Err(anyhow!("thiếu {}", ctx)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prog(src: &str) -> Program {
        parse(&crate::lexer::tokenize(src).unwrap()).unwrap()
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
        assert_eq!(p.items[0].0.commands[0].words.len(), 1);
        // `!` một mình là lệnh, không phải phủ định
        assert!(!prog("!").items[0].0.negate);
    }

    #[test]
    fn redirect_default_fds() {
        let p = prog("cmd < a > b 2>&1 <<< s");
        let r: Vec<(i32, RedirectKind)> = p.items[0].0.commands[0].redirects.iter().map(|r| (r.fd, r.kind)).collect();
        assert_eq!(
            r,
            vec![(0, RedirectKind::In), (1, RedirectKind::Out), (2, RedirectKind::DupOut), (0, RedirectKind::HereString)]
        );
    }
}
