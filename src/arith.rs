//! Số học kiểu bash cho `$(( ))`, `(( ))`, `let`, chỉ số mảng, `${x:off:len}`.
//!
//! Số nguyên 64-bit (tràn thì quay vòng như bash). Tên biến trong biểu thức
//! được đọc rồi tự tính tiếp như biểu thức (`a=b+1; b=2` → `$((a))` = 3).
//! `&&`, `||`, `?:` đánh giá tắt — vế không chạy không gây hiệu ứng phụ.

use anyhow::{anyhow, bail, Result};

/// Nơi đọc / ghi biến cho biểu thức (Shell triển khai).
pub trait Vars {
    fn get(&self, name: &str, index: Option<i64>) -> Option<String>;
    fn set(&mut self, name: &str, index: Option<i64>, value: i64) -> Result<()>;
}

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Num(i64),
    Ident(String),
    Op(&'static str),
    LParen,
    RParen,
    LBracket,
    RBracket,
}

const OPS: [&str; 41] = [
    "<<=", ">>=", "**", "++", "--", "<<", ">>", "<=", ">=", "==", "!=", "&&", "||", "+=", "-=", "*=", "/=", "%=",
    "&=", "^=", "|=", "+", "-", "*", "/", "%", "<", ">", "&", "^", "|", "!", "~", "?", ":", "=", ",", "(", ")", "[",
    "]",
];

fn lex(src: &str) -> Result<Vec<Tok>> {
    let chars: Vec<char> = src.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        if c.is_ascii_digit() {
            let start = i;
            while i < chars.len() && (chars[i].is_ascii_alphanumeric() || matches!(chars[i], '#' | '@' | '_')) {
                i += 1;
            }
            let text: String = chars[start..i].iter().collect();
            out.push(Tok::Num(parse_number(&text)?));
            continue;
        }
        if c.is_ascii_alphabetic() || c == '_' {
            let start = i;
            while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            out.push(Tok::Ident(chars[start..i].iter().collect()));
            continue;
        }
        let rest: String = chars[i..chars.len().min(i + 3)].iter().collect();
        let Some(op) = OPS.iter().find(|op| rest.starts_with(**op)) else {
            bail!("lỗi cú pháp số học: ký tự không hợp lệ `{}` trong `{}`", c, src.trim());
        };
        i += op.chars().count();
        out.push(match *op {
            "(" => Tok::LParen,
            ")" => Tok::RParen,
            "[" => Tok::LBracket,
            "]" => Tok::RBracket,
            o => Tok::Op(o),
        });
    }
    Ok(out)
}

/// `42`, `0x2A`, `052` (bát phân), `2#101010`, `36#z`, `64#_`.
pub fn parse_number(text: &str) -> Result<i64> {
    let bad = || anyhow!("{}: số không hợp lệ", text);
    if let Some((base, digits)) = text.split_once('#') {
        let base: u32 = base.parse().map_err(|_| bad())?;
        if !(2..=64).contains(&base) {
            return Err(anyhow!("{}: cơ số không hợp lệ", text));
        }
        let mut n: i64 = 0;
        for d in digits.chars() {
            let v = match d {
                '0'..='9' => d as u32 - '0' as u32,
                'a'..='z' => d as u32 - 'a' as u32 + 10,
                'A'..='Z' if base <= 36 => d as u32 - 'A' as u32 + 10,
                'A'..='Z' => d as u32 - 'A' as u32 + 36,
                '@' => 62,
                '_' => 63,
                _ => return Err(bad()),
            };
            if v >= base {
                return Err(bad());
            }
            n = n.wrapping_mul(base as i64).wrapping_add(v as i64);
        }
        return Ok(n);
    }
    if let Some(hex) = text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
        return i64::from_str_radix(hex, 16).map_err(|_| bad());
    }
    if text.len() > 1 && text.starts_with('0') {
        return i64::from_str_radix(&text[1..], 8).map_err(|_| bad());
    }
    text.parse::<i64>().map_err(|_| bad())
}

/// Tính biểu thức. Chuỗi rỗng = 0.
pub fn eval(src: &str, vars: &mut dyn Vars) -> Result<i64> {
    let toks = lex(src)?;
    if toks.is_empty() {
        return Ok(0);
    }
    let mut p = Parser { toks, pos: 0, vars, depth: 0 };
    let v = p.comma(true)?;
    if p.pos < p.toks.len() {
        bail!("lỗi cú pháp số học gần `{}`", describe(&p.toks[p.pos]));
    }
    Ok(v)
}

fn describe(t: &Tok) -> String {
    match t {
        Tok::Num(n) => n.to_string(),
        Tok::Ident(s) => s.clone(),
        Tok::Op(o) => o.to_string(),
        Tok::LParen => "(".into(),
        Tok::RParen => ")".into(),
        Tok::LBracket => "[".into(),
        Tok::RBracket => "]".into(),
    }
}

/// Giá trị trái (có thể gán): tên biến + chỉ số mảng (nếu có).
struct LValue {
    name: String,
    index: Option<i64>,
}

struct Parser<'a> {
    toks: Vec<Tok>,
    pos: usize,
    vars: &'a mut dyn Vars,
    /// Chống đệ quy vô hạn khi biến tự tham chiếu (`a=a+1; $((a))`).
    depth: u32,
}

impl Parser<'_> {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.pos)
    }

    fn eat_op(&mut self, op: &str) -> bool {
        if self.peek() == Some(&Tok::Op(leak(op))) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn comma(&mut self, run: bool) -> Result<i64> {
        let mut v = self.assign(run)?;
        while self.eat_op(",") {
            v = self.assign(run)?;
        }
        Ok(v)
    }

    fn assign(&mut self, run: bool) -> Result<i64> {
        // Nhìn trước: `ident [ idx ]? (=|+=|…)`
        let save = self.pos;
        if let Some(lv) = self.try_lvalue(run)? {
            if let Some(Tok::Op(op)) = self.peek().cloned() {
                let compound = match op {
                    "=" => Some(None),
                    "+=" => Some(Some("+")),
                    "-=" => Some(Some("-")),
                    "*=" => Some(Some("*")),
                    "/=" => Some(Some("/")),
                    "%=" => Some(Some("%")),
                    "<<=" => Some(Some("<<")),
                    ">>=" => Some(Some(">>")),
                    "&=" => Some(Some("&")),
                    "^=" => Some(Some("^")),
                    "|=" => Some(Some("|")),
                    _ => None,
                };
                if let Some(bin) = compound {
                    self.pos += 1;
                    let rhs = self.assign(run)?;
                    if !run {
                        return Ok(0);
                    }
                    let value = match bin {
                        None => rhs,
                        Some(b) => {
                            let cur = self.read_var(&lv)?;
                            binop(b, cur, rhs)?
                        }
                    };
                    self.vars.set(&lv.name, lv.index, value)?;
                    return Ok(value);
                }
            }
        }
        self.pos = save;
        self.ternary(run)
    }

    fn try_lvalue(&mut self, run: bool) -> Result<Option<LValue>> {
        let Some(Tok::Ident(name)) = self.peek().cloned() else {
            return Ok(None);
        };
        self.pos += 1;
        let mut index = None;
        if self.peek() == Some(&Tok::LBracket) {
            self.pos += 1;
            let i = self.comma(run)?;
            if self.peek() != Some(&Tok::RBracket) {
                bail!("thiếu `]` sau chỉ số mảng {}", name);
            }
            self.pos += 1;
            index = Some(i);
        }
        Ok(Some(LValue { name, index }))
    }

    fn ternary(&mut self, run: bool) -> Result<i64> {
        let cond = self.binary(0, run)?;
        if self.eat_op("?") {
            let a = self.comma(run && cond != 0)?;
            if !self.eat_op(":") {
                bail!("thiếu `:` trong biểu thức `?:`");
            }
            let b = self.assign(run && cond == 0)?;
            return Ok(if cond != 0 { a } else { b });
        }
        Ok(cond)
    }

    /// Toán tử hai ngôi theo bảng độ ưu tiên (thấp → cao).
    fn binary(&mut self, level: usize, run: bool) -> Result<i64> {
        const LEVELS: [&[&str]; 10] = [
            &["||"],
            &["&&"],
            &["|"],
            &["^"],
            &["&"],
            &["==", "!="],
            &["<", ">", "<=", ">="],
            &["<<", ">>"],
            &["+", "-"],
            &["*", "/", "%"],
        ];
        if level == LEVELS.len() {
            return self.power(run);
        }
        let mut lhs = self.binary(level + 1, run)?;
        loop {
            let Some(Tok::Op(op)) = self.peek().cloned() else { break };
            if !LEVELS[level].contains(&op) {
                break;
            }
            self.pos += 1;
            match op {
                // Đánh giá tắt: vế phải chỉ chạy khi cần.
                "&&" => {
                    let rhs = self.binary(level + 1, run && lhs != 0)?;
                    lhs = (lhs != 0 && rhs != 0) as i64;
                }
                "||" => {
                    let rhs = self.binary(level + 1, run && lhs == 0)?;
                    lhs = (lhs != 0 || rhs != 0) as i64;
                }
                _ => {
                    let rhs = self.binary(level + 1, run)?;
                    lhs = if run { binop(op, lhs, rhs)? } else { 0 };
                }
            }
        }
        Ok(lhs)
    }

    fn power(&mut self, run: bool) -> Result<i64> {
        let base = self.unary(run)?;
        if self.eat_op("**") {
            let exp = self.power(run)?; // kết hợp phải
            if !run {
                return Ok(0);
            }
            return binop("**", base, exp);
        }
        Ok(base)
    }

    fn unary(&mut self, run: bool) -> Result<i64> {
        if let Some(Tok::Op(op)) = self.peek().cloned() {
            match op {
                "!" => {
                    self.pos += 1;
                    return Ok((self.unary(run)? == 0) as i64);
                }
                "~" => {
                    self.pos += 1;
                    return Ok(!self.unary(run)?);
                }
                "-" => {
                    self.pos += 1;
                    return Ok(self.unary(run)?.wrapping_neg());
                }
                "+" => {
                    self.pos += 1;
                    return self.unary(run);
                }
                "++" | "--" => {
                    self.pos += 1;
                    let lv = self.try_lvalue(run)?.ok_or_else(|| anyhow!("`{}` cần tên biến", op))?;
                    if !run {
                        return Ok(0);
                    }
                    let v = self.read_var(&lv)? + if op == "++" { 1 } else { -1 };
                    self.vars.set(&lv.name, lv.index, v)?;
                    return Ok(v);
                }
                _ => {}
            }
        }
        self.postfix(run)
    }

    fn postfix(&mut self, run: bool) -> Result<i64> {
        match self.peek().cloned() {
            Some(Tok::Num(n)) => {
                self.pos += 1;
                Ok(n)
            }
            Some(Tok::LParen) => {
                self.pos += 1;
                let v = self.comma(run)?;
                if self.peek() != Some(&Tok::RParen) {
                    bail!("thiếu `)` trong biểu thức số học");
                }
                self.pos += 1;
                Ok(v)
            }
            Some(Tok::Ident(_)) => {
                let lv = self.try_lvalue(run)?.expect("đã thấy Ident");
                if let Some(Tok::Op(op @ ("++" | "--"))) = self.peek().cloned() {
                    self.pos += 1;
                    if !run {
                        return Ok(0);
                    }
                    let v = self.read_var(&lv)?;
                    self.vars.set(&lv.name, lv.index, v + if op == "++" { 1 } else { -1 })?;
                    return Ok(v);
                }
                if !run {
                    return Ok(0);
                }
                self.read_var(&lv)
            }
            Some(t) => bail!("lỗi cú pháp số học gần `{}`", describe(&t)),
            None => bail!("biểu thức số học thiếu toán hạng"),
        }
    }

    /// Đọc biến: rỗng/không có = 0; giá trị không phải số được tính như biểu thức.
    fn read_var(&mut self, lv: &LValue) -> Result<i64> {
        let raw = self.vars.get(&lv.name, lv.index).unwrap_or_default();
        let raw = raw.trim();
        if raw.is_empty() {
            return Ok(0);
        }
        if let Ok(n) = parse_number(raw) {
            return Ok(n);
        }
        if self.depth > 64 {
            bail!("{}: biểu thức đệ quy quá sâu", lv.name);
        }
        let toks = lex(raw)?;
        let mut sub = Parser { toks, pos: 0, vars: &mut *self.vars, depth: self.depth + 1 };
        let v = sub.comma(true)?;
        if sub.pos < sub.toks.len() {
            bail!("{}: giá trị `{}` không phải biểu thức hợp lệ", lv.name, raw);
        }
        Ok(v)
    }
}

/// `&'static str` cho so sánh với `Tok::Op` (OPS là bảng tĩnh).
fn leak(op: &str) -> &'static str {
    OPS.iter().find(|o| **o == op).copied().unwrap_or("")
}

fn binop(op: &str, a: i64, b: i64) -> Result<i64> {
    Ok(match op {
        "+" => a.wrapping_add(b),
        "-" => a.wrapping_sub(b),
        "*" => a.wrapping_mul(b),
        "/" | "%" => {
            if b == 0 {
                bail!("chia cho 0");
            }
            if op == "/" {
                a.wrapping_div(b)
            } else {
                a.wrapping_rem(b)
            }
        }
        "**" => {
            if b < 0 {
                bail!("số mũ âm");
            }
            a.wrapping_pow(b.min(u32::MAX as i64) as u32)
        }
        "<<" => a.wrapping_shl(b as u32),
        ">>" => a.wrapping_shr(b as u32),
        "<" => (a < b) as i64,
        ">" => (a > b) as i64,
        "<=" => (a <= b) as i64,
        ">=" => (a >= b) as i64,
        "==" => (a == b) as i64,
        "!=" => (a != b) as i64,
        "&" => a & b,
        "^" => a ^ b,
        "|" => a | b,
        _ => bail!("toán tử không hỗ trợ: {}", op),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[derive(Default)]
    struct Map(HashMap<String, String>);

    impl Vars for Map {
        fn get(&self, name: &str, index: Option<i64>) -> Option<String> {
            let key = index.map_or(name.to_string(), |i| format!("{name}[{i}]"));
            self.0.get(&key).cloned()
        }
        fn set(&mut self, name: &str, index: Option<i64>, value: i64) -> Result<()> {
            let key = index.map_or(name.to_string(), |i| format!("{name}[{i}]"));
            self.0.insert(key, value.to_string());
            Ok(())
        }
    }

    fn ev(src: &str, m: &mut Map) -> i64 {
        eval(src, m).unwrap()
    }

    #[test]
    fn precedence_and_ops() {
        let mut m = Map::default();
        assert_eq!(ev("2+3*4", &mut m), 14);
        assert_eq!(ev("(2+3)*4", &mut m), 20);
        assert_eq!(ev("10/3", &mut m), 3);
        assert_eq!(ev("10%3", &mut m), 1);
        assert_eq!(ev("2**8", &mut m), 256);
        assert_eq!(ev("2**3**2", &mut m), 512);
        assert_eq!(ev("-3+5", &mut m), 2);
        assert_eq!(ev("!0 + ~0", &mut m), 0);
        assert_eq!(ev("1 < 2 && 3 >= 3", &mut m), 1);
        assert_eq!(ev("5 > 3 ? 10 : 20", &mut m), 10);
        assert_eq!(ev("1 << 4 | 1", &mut m), 17);
        assert_eq!(ev("0x1F + 010 + 2#101", &mut m), 31 + 8 + 5);
        assert_eq!(ev("", &mut m), 0);
    }

    #[test]
    fn variables_and_assignment() {
        let mut m = Map::default();
        m.0.insert("x".into(), "5".into());
        assert_eq!(ev("x*2", &mut m), 10);
        assert_eq!(ev("x++", &mut m), 5);
        assert_eq!(m.0["x"], "6");
        assert_eq!(ev("++x", &mut m), 7);
        assert_eq!(ev("y = x += 3", &mut m), 10);
        assert_eq!(m.0["y"], "10");
        assert_eq!(ev("unset_var + 1", &mut m), 1);
        m.0.insert("e".into(), "y-4".into());
        assert_eq!(ev("e", &mut m), 6, "giá trị là biểu thức → tính tiếp");
        assert_eq!(ev("a[2] = 7, a[2] * 2", &mut m), 14);
    }

    #[test]
    fn short_circuit_has_no_side_effects() {
        let mut m = Map::default();
        ev("0 && (z = 1)", &mut m);
        ev("1 || (z = 1)", &mut m);
        ev("1 ? 0 : (z = 1)", &mut m);
        assert!(!m.0.contains_key("z"));
    }

    #[test]
    fn errors() {
        let mut m = Map::default();
        assert!(eval("1/0", &mut m).is_err());
        assert!(eval("1 +", &mut m).is_err());
        assert!(eval("2 $ 3", &mut m).is_err());
        m.0.insert("a".into(), "a+1".into());
        assert!(eval("a", &mut m).is_err(), "đệ quy vô hạn phải báo lỗi");
    }
}
