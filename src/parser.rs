use anyhow::{anyhow, Result};

use crate::lexer::{Token, WordPart};

#[derive(Debug, Clone)]
pub struct SimpleCommand {
    pub words: Vec<Vec<WordPart>>,
    pub redirects: Vec<Redirect>,
}

#[derive(Debug, Clone)]
pub struct Redirect {
    pub kind: RedirectKind,
    pub target: Vec<WordPart>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RedirectKind {
    In,
    Out,
    Append,
    ErrOut,
    ErrAppend,
    AllOut,
}

#[derive(Debug, Clone)]
pub struct Pipeline {
    pub commands: Vec<SimpleCommand>,
    pub background: bool,
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
            items.push((pipeline, next_op));
            match self.peek() {
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
        Ok(Pipeline { commands, background })
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
                Some(Token::Less) => {
                    self.advance();
                    let target = self.expect_word("đích cho '<'")?;
                    redirects.push(Redirect { kind: RedirectKind::In, target });
                }
                Some(Token::Great) => {
                    self.advance();
                    let target = self.expect_word("đích cho '>'")?;
                    redirects.push(Redirect { kind: RedirectKind::Out, target });
                }
                Some(Token::DGreat) => {
                    self.advance();
                    let target = self.expect_word("đích cho '>>'")?;
                    redirects.push(Redirect { kind: RedirectKind::Append, target });
                }
                Some(Token::ErrGreat) => {
                    self.advance();
                    let target = self.expect_word("đích cho '2>'")?;
                    redirects.push(Redirect { kind: RedirectKind::ErrOut, target });
                }
                Some(Token::ErrDGreat) => {
                    self.advance();
                    let target = self.expect_word("đích cho '2>>'")?;
                    redirects.push(Redirect { kind: RedirectKind::ErrAppend, target });
                }
                Some(Token::AndGreat) => {
                    self.advance();
                    let target = self.expect_word("đích cho '&>'")?;
                    redirects.push(Redirect { kind: RedirectKind::AllOut, target });
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
