//! Điều kiện: `[[ … ]]` và builtin `test` / `[`.

use anyhow::{anyhow, Result};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};
use std::rc::Rc;

use crate::expand::{expand_pattern, expand_regex, expand_string, fnmatch};
use crate::parser::CondExpr;
use crate::shell::Shell;

const UNARY: [&str; 25] = [
    "-a", "-b", "-c", "-d", "-e", "-f", "-g", "-h", "-k", "-p", "-r", "-s", "-t", "-u", "-w", "-x", "-G", "-L", "-N",
    "-O", "-S", "-z", "-n", "-o", "-v",
];

const BINARY: [&str; 15] =
    ["==", "=", "!=", "=~", "<", ">", "-eq", "-ne", "-lt", "-le", "-gt", "-ge", "-nt", "-ot", "-ef"];

pub fn is_unary_op(op: &str) -> bool {
    UNARY.contains(&op)
}

pub fn is_binary_op(op: &str) -> bool {
    BINARY.contains(&op)
}

/// Đánh giá `[[ … ]]`.
pub fn eval(shell: &Rc<RefCell<Shell>>, e: &CondExpr) -> Result<bool> {
    Ok(match e {
        CondExpr::And(a, b) => eval(shell, a)? && eval(shell, b)?,
        CondExpr::Or(a, b) => eval(shell, a)? || eval(shell, b)?,
        CondExpr::Not(a) => !eval(shell, a)?,
        CondExpr::Word(w) => !expand_string(shell, w)?.is_empty(),
        CondExpr::Unary(op, w) => {
            let v = expand_string(shell, w)?;
            unary(shell, op, &v)?
        }
        CondExpr::Binary(l, op, r) => {
            let lhs = expand_string(shell, l)?;
            match op.as_str() {
                "==" | "=" | "!=" => {
                    let pat = expand_pattern(shell, r)?;
                    let nocase = shell.borrow().shopts.nocasematch;
                    let m = fnmatch(&pat, &lhs, nocase);
                    if op == "!=" { !m } else { m }
                }
                "=~" => {
                    let re = expand_regex(shell, r)?;
                    regex_match(shell, &lhs, &re)?
                }
                _ => {
                    let rhs = expand_string(shell, r)?;
                    binary(shell, &lhs, op, &rhs)?
                }
            }
        }
    })
}

/// `[[ s =~ re ]]` — gán `BASH_REMATCH` (0 = cả chuỗi khớp, 1.. = nhóm).
fn regex_match(shell: &Rc<RefCell<Shell>>, s: &str, re: &str) -> Result<bool> {
    let nocase = shell.borrow().shopts.nocasematch;
    let re = regex::RegexBuilder::new(re)
        .case_insensitive(nocase)
        .build()
        .map_err(|e| anyhow!("regex không hợp lệ `{}`: {}", re, e))?;
    let mut groups = BTreeMap::new();
    let matched = match re.captures(s) {
        Some(caps) => {
            for (i, g) in caps.iter().enumerate() {
                groups.insert(i as i64, g.map(|m| m.as_str().to_string()).unwrap_or_default());
            }
            true
        }
        None => false,
    };
    shell.borrow_mut().arrays.insert("BASH_REMATCH".into(), groups);
    Ok(matched)
}

fn unary(shell: &Rc<RefCell<Shell>>, op: &str, v: &str) -> Result<bool> {
    let meta = || std::fs::metadata(v);
    let lmeta = || std::fs::symlink_metadata(v);
    let access = |mode: libc::c_int| {
        std::ffi::CString::new(v).map(|c| unsafe { libc::access(c.as_ptr(), mode) } == 0).unwrap_or(false)
    };
    Ok(match op {
        "-z" => v.is_empty(),
        "-n" => !v.is_empty(),
        "-e" | "-a" => meta().is_ok(),
        "-f" => meta().map(|m| m.is_file()).unwrap_or(false),
        "-d" => meta().map(|m| m.is_dir()).unwrap_or(false),
        "-h" | "-L" => lmeta().map(|m| m.file_type().is_symlink()).unwrap_or(false),
        "-b" => meta().map(|m| m.file_type().is_block_device()).unwrap_or(false),
        "-c" => meta().map(|m| m.file_type().is_char_device()).unwrap_or(false),
        "-p" => meta().map(|m| m.file_type().is_fifo()).unwrap_or(false),
        "-S" => meta().map(|m| m.file_type().is_socket()).unwrap_or(false),
        "-s" => meta().map(|m| m.len() > 0).unwrap_or(false),
        "-r" => access(libc::R_OK),
        "-w" => access(libc::W_OK),
        "-x" => access(libc::X_OK),
        "-u" => meta().map(|m| m.permissions().mode() & 0o4000 != 0).unwrap_or(false),
        "-g" => meta().map(|m| m.permissions().mode() & 0o2000 != 0).unwrap_or(false),
        "-k" => meta().map(|m| m.permissions().mode() & 0o1000 != 0).unwrap_or(false),
        "-O" => meta().map(|m| m.uid() == unsafe { libc::geteuid() }).unwrap_or(false),
        "-G" => meta().map(|m| m.gid() == unsafe { libc::getegid() }).unwrap_or(false),
        "-N" => meta().map(|m| m.mtime() > m.atime()).unwrap_or(false),
        "-t" => v.trim().parse::<i32>().map(|fd| unsafe { libc::isatty(fd) } == 1).unwrap_or(false),
        "-o" => shell.borrow().opts.get(v).unwrap_or(false),
        "-v" => {
            let sh = shell.borrow();
            match v.split_once('[') {
                Some((name, rest)) => {
                    let idx = rest.trim_end_matches(']');
                    let key = if sh.is_assoc(name) {
                        crate::shell::Index::Key(idx.to_string())
                    } else {
                        crate::shell::Index::Num(idx.parse().unwrap_or(0))
                    };
                    sh.get_index(name, &key).is_some()
                }
                None => sh.get_var(v).is_some() || sh.is_array(v),
            }
        }
        _ => return Err(anyhow!("{}: toán tử một ngôi không hợp lệ", op)),
    })
}

fn int(s: &str) -> Result<i64> {
    let t = s.trim();
    t.parse::<i64>().or_else(|_| crate::arith::parse_number(t)).map_err(|_| anyhow!("{}: cần số nguyên", s))
}

fn binary(_shell: &Rc<RefCell<Shell>>, l: &str, op: &str, r: &str) -> Result<bool> {
    let mtime = |p: &str| std::fs::metadata(p).and_then(|m| m.modified()).ok();
    Ok(match op {
        "=" | "==" => l == r,
        "!=" => l != r,
        "<" => l < r,
        ">" => l > r,
        "-eq" => int(l)? == int(r)?,
        "-ne" => int(l)? != int(r)?,
        "-lt" => int(l)? < int(r)?,
        "-le" => int(l)? <= int(r)?,
        "-gt" => int(l)? > int(r)?,
        "-ge" => int(l)? >= int(r)?,
        "-nt" => match (mtime(l), mtime(r)) {
            (Some(a), Some(b)) => a > b,
            (Some(_), None) => true,
            _ => false,
        },
        "-ot" => match (mtime(l), mtime(r)) {
            (Some(a), Some(b)) => a < b,
            (None, Some(_)) => true,
            _ => false,
        },
        "-ef" => match (std::fs::metadata(l), std::fs::metadata(r)) {
            (Ok(a), Ok(b)) => a.dev() == b.dev() && a.ino() == b.ino(),
            _ => false,
        },
        _ => return Err(anyhow!("{}: toán tử hai ngôi không hợp lệ", op)),
    })
}

/// Builtin `test` / `[` theo quy tắc POSIX (số đối số quyết định cách hiểu),
/// cộng `!`, `-a`, `-o`, `( )`. Trả 0 đúng, 1 sai, 2 lỗi cú pháp.
pub fn test_builtin(shell: &Rc<RefCell<Shell>>, name: &str, args: &[&str]) -> Result<i32> {
    let mut args = args.to_vec();
    if name == "[" {
        if args.last() != Some(&"]") {
            eprintln!("jaksh: [: thiếu `]`");
            return Ok(2);
        }
        args.pop();
    }
    let mut p = TestParser { shell, args: &args, pos: 0 };
    match p.expr_or() {
        Ok(v) if p.pos == args.len() => Ok(if v { 0 } else { 1 }),
        Ok(_) => {
            eprintln!("jaksh: {}: thừa đối số `{}`", name, args[p.pos]);
            Ok(2)
        }
        Err(e) => {
            eprintln!("jaksh: {}: {}", name, e);
            Ok(2)
        }
    }
}

struct TestParser<'a> {
    shell: &'a Rc<RefCell<Shell>>,
    args: &'a [&'a str],
    pos: usize,
}

impl TestParser<'_> {
    fn remaining(&self) -> usize {
        self.args.len() - self.pos
    }

    fn expr_or(&mut self) -> Result<bool> {
        let mut v = self.expr_and()?;
        while self.args.get(self.pos) == Some(&"-o") && self.remaining() > 1 {
            self.pos += 1;
            let r = self.expr_and()?;
            v = v || r;
        }
        Ok(v)
    }

    fn expr_and(&mut self) -> Result<bool> {
        let mut v = self.expr_not()?;
        while self.args.get(self.pos) == Some(&"-a") && self.remaining() > 1 {
            self.pos += 1;
            let r = self.expr_not()?;
            v = v && r;
        }
        Ok(v)
    }

    fn expr_not(&mut self) -> Result<bool> {
        // `!` chỉ là phủ định khi còn toán hạng phía sau (`[ ! ]` = chuỗi "!").
        if self.args.get(self.pos) == Some(&"!") && self.remaining() > 1 {
            self.pos += 1;
            return Ok(!self.expr_not()?);
        }
        self.primary()
    }

    fn primary(&mut self) -> Result<bool> {
        let a = self.args;
        let i = self.pos;
        let Some(&first) = a.get(i) else {
            return Ok(false);
        };
        // ( expr )
        if first == "(" && self.remaining() >= 3 {
            self.pos += 1;
            let v = self.expr_or()?;
            if a.get(self.pos) != Some(&")") {
                return Err(anyhow!("thiếu `)`"));
            }
            self.pos += 1;
            return Ok(v);
        }
        // a OP b (ưu tiên hai ngôi khi đủ 3 đối số)
        if let (Some(&op), Some(&rhs)) = (a.get(i + 1), a.get(i + 2)) {
            if is_binary_op(op) && op != "=~" {
                self.pos += 3;
                return binary(self.shell, first, op, rhs);
            }
        }
        // -op x
        if is_unary_op(first) && a.get(i + 1).is_some() && !(first == "-a" || first == "-o") || (first == "-a" || first == "-o") && self.remaining() == 2 {
            self.pos += 2;
            return unary(self.shell, first, a[i + 1]);
        }
        // 1 đối số: đúng nếu không rỗng.
        self.pos += 1;
        Ok(!first.is_empty())
    }
}
