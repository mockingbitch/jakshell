//! Expansion theo đúng thứ tự của bash cho mỗi từ:
//!   1. brace          `{a,b}` `{1..10..2}`          (chỉ chữ không nháy)
//!   2. tilde          `~` `~user`
//!   3. tham số / số học / lệnh / process substitution
//!   4. tách từ theo `$IFS`                           (chỉ kết quả expansion không nháy)
//!   5. glob (pathname)                              (`*` `?` `[...]` không nháy)
//!   6. bỏ nháy
//!
//! Mỗi đoạn chữ mang theo "loại" để bước 4–5 biết đoạn nào được tách / glob:
//! `Lit` (chữ gõ trực tiếp — glob nhưng không tách), `Quoted` (không gì cả),
//! `Exp` (kết quả expansion không nháy — tách + glob).

use anyhow::{anyhow, Result};
use std::cell::RefCell;
use std::rc::Rc;

use crate::lexer::{parse_embedded, ParamExpr, ParamOp, ReplaceMode, WordPart};
use crate::parser::Word;
use crate::shell::{Index, Shell};

/// Lỗi `set -u`: dùng biến chưa gán.
#[derive(Debug)]
pub struct UnboundVar(pub String);

impl std::fmt::Display for UnboundVar {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: biến chưa được gán (set -u)", self.0)
    }
}

impl std::error::Error for UnboundVar {}

/// Lỗi `${x:?msg}` — script dừng (như bash).
#[derive(Debug)]
pub struct ParamError(pub String);

impl std::fmt::Display for ParamError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for ParamError {}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    Lit,
    Quoted,
    Exp,
}

#[derive(Clone, Debug)]
struct Seg {
    text: String,
    kind: Kind,
}

#[derive(Clone, Debug, Default)]
struct Field {
    segs: Vec<Seg>,
    /// Có phần trong nháy → giữ lại kể cả khi rỗng (`""` là 1 đối số rỗng).
    quoted: bool,
}

impl Field {
    fn push(&mut self, text: String, kind: Kind) {
        if kind == Kind::Quoted {
            self.quoted = true;
        }
        if text.is_empty() {
            return;
        }
        match self.segs.last_mut() {
            Some(last) if last.kind == kind => last.text.push_str(&text),
            _ => self.segs.push(Seg { text, kind }),
        }
    }

    fn text(&self) -> String {
        self.segs.iter().map(|s| s.text.as_str()).collect()
    }
}

/// Kết quả của 1 tham số: chuỗi, danh sách (`$@`, `${a[@]}`), hoặc 1 từ cần
/// expand tiếp tại chỗ (`${x:-giá trị mặc định}`).
enum Value {
    Scalar(Option<String>),
    List(Vec<String>, bool),
    Word(Vec<WordPart>),
}

/// Expand đầy đủ (brace → … → glob) một danh sách từ thành argv.
pub fn expand_words(shell: &Rc<RefCell<Shell>>, words: &[Word]) -> Result<Vec<String>> {
    let mut out = Vec::new();
    for w in words {
        for bw in brace_expand(w) {
            let fields = expand_fields(shell, &bw)?;
            let ifs = shell.borrow().get_var("IFS");
            for f in fields {
                for sf in split_field(f, ifs.as_deref().unwrap_or(" \t\n")) {
                    out.extend(glob_field(shell, sf)?);
                }
            }
        }
    }
    Ok(out)
}

/// Expand không tách từ, không glob — giá trị phép gán, đích redirect, `case`,
/// toán hạng `[[ ]]`, here-doc.
pub fn expand_string(shell: &Rc<RefCell<Shell>>, word: &[WordPart]) -> Result<String> {
    let fields = expand_fields(shell, word)?;
    Ok(fields.iter().map(Field::text).collect::<Vec<_>>().join(" "))
}

/// Pattern cho `case` / `[[ == ]]` / `${x#pat}`: phần trong nháy được escape
/// bằng `\` nên khớp nguyên văn.
pub fn expand_pattern(shell: &Rc<RefCell<Shell>>, word: &[WordPart]) -> Result<String> {
    let fields = expand_fields(shell, word)?;
    let mut out = String::new();
    for (i, f) in fields.iter().enumerate() {
        if i > 0 {
            out.push(' ');
        }
        for seg in &f.segs {
            match seg.kind {
                Kind::Quoted => {
                    for c in seg.text.chars() {
                        if matches!(c, '*' | '?' | '[' | ']' | '\\') {
                            out.push('\\');
                        }
                        out.push(c);
                    }
                }
                _ => out.push_str(&seg.text),
            }
        }
    }
    Ok(out)
}

/// Regex cho `[[ x =~ re ]]`: phần trong nháy khớp nguyên văn.
pub fn expand_regex(shell: &Rc<RefCell<Shell>>, word: &[WordPart]) -> Result<String> {
    let fields = expand_fields(shell, word)?;
    let mut out = String::new();
    for f in &fields {
        for seg in &f.segs {
            match seg.kind {
                Kind::Quoted => out.push_str(&regex::escape(&seg.text)),
                _ => out.push_str(&seg.text),
            }
        }
    }
    Ok(out)
}

/// Tính chỉ số mảng: mảng kết hợp → khoá (expand chuỗi), mảng thường → số học.
pub fn eval_index(shell: &Rc<RefCell<Shell>>, name: &str, raw: &str) -> Result<Index> {
    if shell.borrow().is_assoc(name) {
        let key = expand_string(shell, &parse_embedded(raw)?)?;
        return Ok(Index::Key(key));
    }
    Ok(Index::Num(eval_arith(shell, raw)?))
}

/// Chỉ số của phép gán `a[...]=` (dạng từ, đã giữ nguyên `$i`…).
pub fn eval_index_word(shell: &Rc<RefCell<Shell>>, name: &str, word: &[WordPart]) -> Result<Index> {
    let text = expand_string(shell, word)?;
    if shell.borrow().is_assoc(name) {
        return Ok(Index::Key(text));
    }
    Ok(Index::Num(eval_arith(shell, &text)?))
}

/// `$(( expr ))`: thay `$x` / `$(...)` trong biểu thức trước rồi mới tính.
pub fn eval_arith(shell: &Rc<RefCell<Shell>>, raw: &str) -> Result<i64> {
    let expr = if raw.contains('$') || raw.contains('`') { expand_string(shell, &parse_embedded(raw)?)? } else { raw.to_string() };
    let mut sh = shell.borrow_mut();
    crate::arith::eval(&expr, &mut *sh)
}

fn expand_fields(shell: &Rc<RefCell<Shell>>, parts: &[WordPart]) -> Result<Vec<Field>> {
    let mut out = vec![Field::default()];
    for p in parts {
        expand_part(shell, p, false, &mut out)?;
    }
    Ok(out)
}

fn cur(out: &mut [Field]) -> &mut Field {
    out.last_mut().expect("luôn có ít nhất 1 field")
}

fn expand_part(shell: &Rc<RefCell<Shell>>, part: &WordPart, in_dq: bool, out: &mut Vec<Field>) -> Result<()> {
    let exp_kind = if in_dq { Kind::Quoted } else { Kind::Exp };
    match part {
        WordPart::Literal(s) => cur(out).push(s.clone(), if in_dq { Kind::Quoted } else { Kind::Lit }),
        WordPart::Quoted(s) => cur(out).push(s.clone(), Kind::Quoted),
        WordPart::DQ(inner) => {
            // `"$@"` không tham số → không sinh từ nào (không tính là "" ).
            let only_at = inner.len() == 1 && is_at_expansion(&inner[0]);
            if !only_at {
                cur(out).quoted = true;
            }
            for p in inner {
                expand_part(shell, p, true, out)?;
            }
        }
        WordPart::Tilde(user) => {
            let home = if user.is_empty() {
                shell.borrow().get_var("HOME").or_else(|| dirs::home_dir().map(|h| h.display().to_string()))
            } else {
                user_home(user)
            };
            match home {
                Some(h) => cur(out).push(h, Kind::Quoted),
                None => cur(out).push(format!("~{}", user), Kind::Lit),
            }
        }
        WordPart::Var(name) => {
            let value = simple_var(shell, name)?;
            push_value(shell, value, in_dq, exp_kind, out)?;
        }
        WordPart::Param(p) => {
            let value = param_value(shell, p, in_dq)?;
            push_value(shell, value, in_dq, exp_kind, out)?;
        }
        WordPart::CmdSub(src) => {
            let text = crate::executor::command_substitution(shell, src)?;
            cur(out).push(text, exp_kind);
        }
        WordPart::Arith(expr) => {
            let v = eval_arith(shell, expr)?;
            cur(out).push(v.to_string(), exp_kind);
        }
        WordPart::ProcSub(src, input) => {
            let path = crate::executor::process_substitution(shell, src, *input)?;
            cur(out).push(path, Kind::Quoted);
        }
    }
    Ok(())
}

fn is_at_expansion(p: &WordPart) -> bool {
    match p {
        WordPart::Var(n) => n == "@",
        WordPart::Param(pe) => pe.op == ParamOp::Plain && (pe.name == "@" || pe.index.as_deref() == Some("@")),
        _ => false,
    }
}

fn push_value(shell: &Rc<RefCell<Shell>>, value: Value, in_dq: bool, kind: Kind, out: &mut Vec<Field>) -> Result<()> {
    match value {
        Value::Scalar(v) => cur(out).push(v.unwrap_or_default(), kind),
        Value::List(items, star) => {
            if star && in_dq {
                // `"$*"` / `"${a[*]}"` → 1 từ, nối bằng ký tự đầu của IFS.
                let sep = shell.borrow().get_var("IFS").map_or(" ".to_string(), |s| s.chars().next().map(String::from).unwrap_or_default());
                cur(out).push(items.join(&sep), kind);
            } else {
                for (i, item) in items.into_iter().enumerate() {
                    if i > 0 {
                        out.push(Field::default());
                    }
                    if in_dq {
                        cur(out).quoted = true;
                    }
                    cur(out).push(item, kind);
                }
            }
        }
        Value::Word(parts) => {
            // Giá trị `${x:-a b}` là kết quả expansion: chữ không nháy trong đó
            // bị tách từ (khác chữ gõ trực tiếp trong lệnh).
            for p in &parts {
                match p {
                    WordPart::Literal(s) if !in_dq => cur(out).push(s.clone(), Kind::Exp),
                    _ => expand_part(shell, p, in_dq, out)?,
                }
            }
        }
    }
    Ok(())
}

fn nounset_check(shell: &Rc<RefCell<Shell>>, name: &str, v: &Option<String>) -> Result<()> {
    if v.is_none() && shell.borrow().opts.nounset && !matches!(name, "@" | "*" | "!") {
        return Err(anyhow::Error::new(UnboundVar(name.to_string())));
    }
    Ok(())
}

fn simple_var(shell: &Rc<RefCell<Shell>>, name: &str) -> Result<Value> {
    if name == "@" || name == "*" {
        return Ok(Value::List(shell.borrow().positional.clone(), name == "*"));
    }
    let v = shell.borrow().get_var(name);
    nounset_check(shell, name, &v)?;
    Ok(Value::Scalar(v))
}

/// Giá trị gốc của `${name}` / `${name[idx]}` trước khi áp toán tử.
fn base_value(shell: &Rc<RefCell<Shell>>, p: &ParamExpr) -> Result<Value> {
    match p.index.as_deref() {
        Some(idx @ ("@" | "*")) => Ok(Value::List(shell.borrow().array_values(&p.name), idx == "*")),
        Some(raw) => {
            let idx = eval_index(shell, &p.name, raw)?;
            Ok(Value::Scalar(shell.borrow().get_index(&p.name, &idx)))
        }
        None if p.name == "@" || p.name == "*" => Ok(Value::List(shell.borrow().positional.clone(), p.name == "*")),
        None => Ok(Value::Scalar(shell.borrow().get_var(&p.name))),
    }
}

/// "Chưa gán" (và "rỗng" nếu có `:`) cho các toán tử `-` `=` `?` `+`.
fn is_unset_or_null(v: &Value, colon: bool) -> bool {
    match v {
        Value::Scalar(None) => true,
        Value::Scalar(Some(s)) => colon && s.is_empty(),
        Value::List(items, _) => items.is_empty() || (colon && items.iter().all(|s| s.is_empty())),
        Value::Word(_) => false,
    }
}

fn map_value(v: Value, f: impl Fn(&str) -> String) -> Value {
    match v {
        Value::Scalar(s) => Value::Scalar(s.map(|s| f(&s))),
        Value::List(items, star) => Value::List(items.iter().map(|s| f(s)).collect(), star),
        w => w,
    }
}

fn param_value(shell: &Rc<RefCell<Shell>>, p: &ParamExpr, in_dq: bool) -> Result<Value> {
    match &p.op {
        ParamOp::Length => {
            let n = match base_value(shell, p)? {
                Value::List(items, _) => items.len(),
                Value::Scalar(v) => {
                    nounset_check(shell, &p.name, &v)?;
                    v.unwrap_or_default().chars().count()
                }
                Value::Word(_) => 0,
            };
            return Ok(Value::Scalar(Some(n.to_string())));
        }
        ParamOp::Indirect => {
            let target = shell.borrow().get_var(&p.name).unwrap_or_default();
            if target.is_empty() {
                return Ok(Value::Scalar(None));
            }
            // Tên đích có thể kèm chỉ số: ref='a[1]' / 'a[@]'.
            return match crate::lexer::parse_embedded(&format!("${{{}}}", target))?.into_iter().next() {
                Some(WordPart::Var(n)) => simple_var(shell, &n),
                Some(WordPart::Param(pe)) => param_value(shell, &pe, in_dq),
                _ => Err(anyhow!("{}: tham chiếu gián tiếp không hợp lệ", target)),
            };
        }
        ParamOp::Keys => return Ok(Value::List(shell.borrow().array_keys(&p.name), p.index.as_deref() == Some("*"))),
        ParamOp::NamesWithPrefix => {
            let sh = shell.borrow();
            let mut names: Vec<String> = sh
                .env
                .keys()
                .chain(sh.arrays.keys())
                .chain(sh.assocs.keys())
                .filter(|k| k.starts_with(&p.name) && *k != "?")
                .cloned()
                .collect();
            names.sort();
            names.dedup();
            return Ok(Value::List(names, false));
        }
        _ => {}
    }

    let base = base_value(shell, p)?;
    match &p.op {
        ParamOp::Plain => {
            if let Value::Scalar(v) = &base {
                nounset_check(shell, &p.name, v)?;
            }
            Ok(base)
        }
        ParamOp::Default { colon, word } => {
            Ok(if is_unset_or_null(&base, *colon) { Value::Word(word.clone()) } else { base })
        }
        ParamOp::Alt { colon, word } => {
            Ok(if is_unset_or_null(&base, *colon) { Value::Scalar(None) } else { Value::Word(word.clone()) })
        }
        ParamOp::Assign { colon, word } => {
            if !is_unset_or_null(&base, *colon) {
                return Ok(base);
            }
            let v = expand_string(shell, word)?;
            match p.index.as_deref() {
                Some(raw) => {
                    let idx = eval_index(shell, &p.name, raw)?;
                    shell.borrow_mut().set_index(&p.name, idx, &v)?;
                }
                None => shell.borrow_mut().set_var(&p.name, &v)?,
            }
            Ok(Value::Scalar(Some(v)))
        }
        ParamOp::Error { colon, word } => {
            if !is_unset_or_null(&base, *colon) {
                return Ok(base);
            }
            let msg = expand_string(shell, word)?;
            let msg = if msg.is_empty() { "biến rỗng hoặc chưa gán".to_string() } else { msg };
            Err(anyhow::Error::new(ParamError(format!("{}: {}", p.name, msg))))
        }
        ParamOp::RemovePrefix { longest, pat } => {
            let pat = expand_pattern(shell, pat)?;
            let nocase = false;
            Ok(map_value(base, |s| remove_prefix(s, &pat, *longest, nocase)))
        }
        ParamOp::RemoveSuffix { longest, pat } => {
            let pat = expand_pattern(shell, pat)?;
            Ok(map_value(base, |s| remove_suffix(s, &pat, *longest, false)))
        }
        ParamOp::Replace { mode, pat, rep } => {
            let pat = expand_pattern(shell, pat)?;
            let rep = expand_string(shell, rep)?;
            Ok(map_value(base, |s| replace(s, &pat, &rep, *mode)))
        }
        ParamOp::Substr { offset, length } => {
            let off = eval_arith(shell, offset)?;
            let len = match length {
                Some(l) => Some(eval_arith(shell, l)?),
                None => None,
            };
            match base {
                Value::List(items, star) => {
                    // `${@:1:2}` — với $@, chỉ số 0 là $0.
                    let items = if p.name == "@" || p.name == "*" {
                        std::iter::once(shell.borrow().arg0.clone()).chain(items).collect::<Vec<_>>()
                    } else {
                        items
                    };
                    let n = items.len() as i64;
                    let start = if off < 0 { (n + off).max(0) } else { off.min(n) };
                    let end = match len {
                        Some(l) if l < 0 => (n + l).max(start),
                        Some(l) => (start + l).min(n),
                        None => n,
                    };
                    Ok(Value::List(items[start as usize..end as usize].to_vec(), star))
                }
                Value::Scalar(v) => {
                    let chars: Vec<char> = v.unwrap_or_default().chars().collect();
                    let n = chars.len() as i64;
                    let start = if off < 0 { (n + off).max(0) } else { off.min(n) };
                    let end = match len {
                        Some(l) if l < 0 => n + l,
                        Some(l) => (start + l).min(n),
                        None => n,
                    };
                    if end < start {
                        return Err(anyhow!("{}: độ dài chuỗi con < 0", p.name));
                    }
                    Ok(Value::Scalar(Some(chars[start as usize..end as usize].iter().collect())))
                }
                w => Ok(w),
            }
        }
        ParamOp::Case { upper, all } => Ok(map_value(base, |s| {
            let mut out = String::with_capacity(s.len());
            for (i, c) in s.chars().enumerate() {
                if *all || i == 0 {
                    if *upper {
                        out.extend(c.to_uppercase());
                    } else {
                        out.extend(c.to_lowercase());
                    }
                } else {
                    out.push(c);
                }
            }
            out
        })),
        ParamOp::Transform(t) => {
            let t = *t;
            let name = p.name.clone();
            Ok(map_value(base, move |s| match t {
                'Q' | 'K' => crate::builtins::sq(s),
                'U' => s.to_uppercase(),
                'L' => s.to_lowercase(),
                'u' => {
                    let mut c = s.chars();
                    c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
                }
                'E' => crate::builtins::interpret_escapes(s).0,
                'A' => format!("{}={}", name, crate::builtins::sq(s)),
                _ => s.to_string(),
            }))
        }
        ParamOp::Length | ParamOp::Indirect | ParamOp::Keys | ParamOp::NamesWithPrefix => unreachable!(),
    }
}

/// Thư mục home của `~user` (getpwnam).
fn user_home(user: &str) -> Option<String> {
    let c = std::ffi::CString::new(user).ok()?;
    unsafe {
        let pw = libc::getpwnam(c.as_ptr());
        if pw.is_null() || (*pw).pw_dir.is_null() {
            return None;
        }
        Some(std::ffi::CStr::from_ptr((*pw).pw_dir).to_string_lossy().into_owned())
    }
}

// ── Tách từ ─────────────────────────────────────────────────────────────────

fn split_field(f: Field, ifs: &str) -> Vec<Field> {
    if ifs.is_empty() || !f.segs.iter().any(|s| s.kind == Kind::Exp) {
        return if f.segs.is_empty() && !f.quoted { vec![] } else { vec![f] };
    }
    let is_ws = |c: char| ifs.contains(c) && c.is_whitespace();
    let is_sep = |c: char| ifs.contains(c) && !c.is_whitespace();
    let mut out = Vec::new();
    let mut current = Field::default();
    // Có nội dung (kể cả `""`) → phải giữ field khi gặp dấu tách.
    let mut has = false;
    for seg in f.segs {
        if seg.kind != Kind::Exp {
            current.push(seg.text, seg.kind);
            has = true;
            continue;
        }
        let mut chars = seg.text.chars().peekable();
        while let Some(c) = chars.next() {
            if is_ws(c) {
                if has {
                    out.push(std::mem::take(&mut current));
                    has = false;
                }
            } else if is_sep(c) {
                // Ký tự tách không phải khoảng trắng: luôn kết thúc 1 field (kể cả rỗng).
                out.push(std::mem::take(&mut current));
                has = false;
                while chars.peek().is_some_and(|&n| is_ws(n)) {
                    chars.next();
                }
            } else {
                current.push(c.to_string(), Kind::Exp);
                has = true;
            }
        }
    }
    if f.quoted {
        current.quoted = true;
    }
    if has || current.quoted && !current.segs.is_empty() {
        out.push(current);
    } else if out.is_empty() && f.quoted {
        out.push(Field { segs: vec![], quoted: true });
    }
    out
}

// ── Glob ────────────────────────────────────────────────────────────────────

fn glob_field(shell: &Rc<RefCell<Shell>>, f: Field) -> Result<Vec<String>> {
    let has_meta = f
        .segs
        .iter()
        .any(|s| s.kind != Kind::Quoted && s.text.chars().any(|c| matches!(c, '*' | '?' | '[')));
    let text = f.text();
    if !has_meta || shell.borrow().opts.noglob {
        return Ok(vec![text]);
    }
    let mut pattern = String::new();
    for seg in &f.segs {
        if seg.kind == Kind::Quoted {
            pattern.push_str(&glob::Pattern::escape(&seg.text));
        } else {
            pattern.push_str(&seg.text);
        }
    }
    let (dotglob, nullglob, nocase, failglob, globstar) = {
        let sh = shell.borrow();
        (sh.shopts.dotglob, sh.shopts.nullglob, sh.shopts.nocaseglob, sh.shopts.failglob, sh.shopts.globstar)
    };
    // Không bật globstar: `**` chỉ như `*` (1 cấp thư mục), như bash.
    if !globstar {
        while pattern.contains("**") {
            pattern = pattern.replace("**", "*");
        }
    }
    // crate glob bỏ qua file ẩn kể cả khi pattern viết `.` tường minh → tự lọc
    // theo quy tắc bash: thành phần pattern bắt đầu bằng `.` mới khớp file ẩn.
    let opts = glob::MatchOptions {
        case_sensitive: !nocase,
        require_literal_separator: true,
        require_literal_leading_dot: false,
    };
    let pat_parts: Vec<&str> = pattern.split('/').collect();
    let hidden_ok = |path: &str| -> bool {
        if dotglob {
            return true;
        }
        let comps: Vec<&str> = path.split('/').collect();
        if comps.len() != pat_parts.len() {
            return !comps.last().is_some_and(|c| c.starts_with('.'));
        }
        comps.iter().zip(&pat_parts).all(|(c, p)| !c.starts_with('.') || p.starts_with('.'))
    };
    let mut matches: Vec<String> = match glob::glob_with(&pattern, opts) {
        Ok(paths) => paths
            .filter_map(|r| r.ok())
            .map(|p| p.display().to_string())
            .filter(|s| hidden_ok(s))
            .filter(|s| !s.rsplit('/').next().is_some_and(|b| b == "." || b == ".."))
            .collect(),
        Err(_) => vec![],
    };
    if matches.is_empty() {
        if failglob {
            return Err(anyhow!("không có file khớp: {}", text));
        }
        return Ok(if nullglob { vec![] } else { vec![text] });
    }
    matches.sort();
    Ok(matches)
}

// ── Brace expansion ─────────────────────────────────────────────────────────

#[derive(Clone, Debug)]
enum Atom {
    Ch(char),
    Part(WordPart),
}

fn to_atoms(word: &[WordPart]) -> Vec<Atom> {
    let mut atoms = Vec::new();
    for p in word {
        match p {
            WordPart::Literal(s) => atoms.extend(s.chars().map(Atom::Ch)),
            other => atoms.push(Atom::Part(other.clone())),
        }
    }
    atoms
}

fn from_atoms(atoms: &[Atom]) -> Word {
    let mut word = Vec::new();
    let mut buf = String::new();
    for a in atoms {
        match a {
            Atom::Ch(c) => buf.push(*c),
            Atom::Part(p) => {
                if !buf.is_empty() {
                    word.push(WordPart::Literal(std::mem::take(&mut buf)));
                }
                word.push(p.clone());
            }
        }
    }
    if !buf.is_empty() {
        word.push(WordPart::Literal(buf));
    }
    word
}

/// `a{b,c}d` → `abd acd`; `{1..5}`, `{a..e}`, `{01..10..3}`. Chỉ chữ không nháy.
pub fn brace_expand(word: &[WordPart]) -> Vec<Word> {
    let has_brace = word.iter().any(|p| matches!(p, WordPart::Literal(s) if s.contains('{')));
    if !has_brace {
        return vec![word.to_vec()];
    }
    expand_atoms(&to_atoms(word)).iter().map(|a| from_atoms(a)).collect()
}

fn expand_atoms(atoms: &[Atom]) -> Vec<Vec<Atom>> {
    for (i, a) in atoms.iter().enumerate() {
        if !matches!(a, Atom::Ch('{')) {
            continue;
        }
        // Tìm `}` khớp + các dấu `,` cấp ngoài cùng.
        let mut depth = 0;
        let mut commas = Vec::new();
        let mut close = None;
        for (j, b) in atoms.iter().enumerate().skip(i) {
            match b {
                Atom::Ch('{') => depth += 1,
                Atom::Ch('}') => {
                    depth -= 1;
                    if depth == 0 {
                        close = Some(j);
                        break;
                    }
                }
                Atom::Ch(',') if depth == 1 => commas.push(j),
                _ => {}
            }
        }
        let Some(close) = close else { continue };
        let prefix = &atoms[..i];
        let suffix = &atoms[close + 1..];
        let alternatives: Vec<Vec<Atom>> = if !commas.is_empty() {
            let mut alts = Vec::new();
            let mut start = i + 1;
            for &c in commas.iter().chain(std::iter::once(&close)) {
                alts.push(atoms[start..c].to_vec());
                start = c + 1;
            }
            alts
        } else {
            let inner: Option<String> = atoms[i + 1..close]
                .iter()
                .map(|a| match a {
                    Atom::Ch(c) => Some(*c),
                    _ => None,
                })
                .collect();
            match inner.and_then(|s| brace_range(&s)) {
                Some(items) => items.into_iter().map(|s| s.chars().map(Atom::Ch).collect()).collect(),
                None => continue,
            }
        };
        let mut out = Vec::new();
        for alt in alternatives {
            let mut combined = prefix.to_vec();
            combined.extend(alt);
            combined.extend(suffix.iter().cloned());
            out.extend(expand_atoms(&combined));
        }
        return out;
    }
    vec![atoms.to_vec()]
}

/// `1..5`, `5..1`, `01..10..2`, `a..e` → danh sách.
fn brace_range(s: &str) -> Option<Vec<String>> {
    let parts: Vec<&str> = s.split("..").collect();
    if parts.len() != 2 && parts.len() != 3 {
        return None;
    }
    let step: i64 = match parts.get(2) {
        Some(st) => st.parse::<i64>().ok()?.abs().max(1),
        None => 1,
    };
    if let (Ok(a), Ok(b)) = (parts[0].parse::<i64>(), parts[1].parse::<i64>()) {
        let width = if (parts[0].starts_with('0') && parts[0].len() > 1) || (parts[1].starts_with('0') && parts[1].len() > 1) {
            parts[0].len().max(parts[1].len())
        } else {
            0
        };
        let mut out = Vec::new();
        let mut x = a;
        loop {
            out.push(if width > 0 { format!("{:0width$}", x, width = width) } else { x.to_string() });
            if x == b || out.len() > 100_000 {
                break;
            }
            if a < b {
                x += step;
                if x > b {
                    break;
                }
            } else {
                x -= step;
                if x < b {
                    break;
                }
            }
        }
        return Some(out);
    }
    let (ca, cb): (Vec<char>, Vec<char>) = (parts[0].chars().collect(), parts[1].chars().collect());
    if ca.len() == 1 && cb.len() == 1 && ca[0].is_ascii_alphabetic() && cb[0].is_ascii_alphabetic() {
        let (a, b) = (ca[0] as i64, cb[0] as i64);
        let mut out = Vec::new();
        let mut x = a;
        loop {
            out.push(char::from_u32(x as u32)?.to_string());
            if x == b {
                break;
            }
            if a < b {
                x += step;
                if x > b {
                    break;
                }
            } else {
                x -= step;
                if x < b {
                    break;
                }
            }
        }
        return Some(out);
    }
    None
}

// ── So khớp pattern (case, [[ == ]], ${x#pat}) ──────────────────────────────

/// Khớp toàn bộ `text` với pattern kiểu shell: `*` `?` `[abc]` `[!a-z]`
/// `[[:alpha:]]`, `\x` = x nguyên văn.
pub fn fnmatch(pattern: &str, text: &str, nocase: bool) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let t: Vec<char> = text.chars().collect();
    match_from(&p, 0, &t, 0, nocase)
}

fn eq_char(a: char, b: char, nocase: bool) -> bool {
    a == b || (nocase && a.to_lowercase().eq(b.to_lowercase()))
}

fn match_from(p: &[char], mut pi: usize, t: &[char], mut ti: usize, nocase: bool) -> bool {
    // Backtracking cho `*` (vị trí sao gần nhất).
    let mut star: Option<(usize, usize)> = None;
    loop {
        if pi < p.len() {
            match p[pi] {
                '*' => {
                    while pi < p.len() && p[pi] == '*' {
                        pi += 1;
                    }
                    if pi == p.len() {
                        return true;
                    }
                    star = Some((pi, ti));
                    continue;
                }
                '?' if ti < t.len() => {
                    pi += 1;
                    ti += 1;
                    continue;
                }
                '[' if ti < t.len() => {
                    if let Some((matched, next)) = match_class(p, pi, t[ti], nocase) {
                        if matched {
                            pi = next;
                            ti += 1;
                            continue;
                        }
                    } else if eq_char('[', t[ti], nocase) {
                        pi += 1;
                        ti += 1;
                        continue;
                    }
                }
                '\\' if pi + 1 < p.len() && ti < t.len() && eq_char(p[pi + 1], t[ti], nocase) => {
                    pi += 2;
                    ti += 1;
                    continue;
                }
                c if c != '\\' && c != '*' && c != '?' && c != '[' && ti < t.len() && eq_char(c, t[ti], nocase) => {
                    pi += 1;
                    ti += 1;
                    continue;
                }
                _ => {}
            }
        } else if ti == t.len() {
            return true;
        }
        match star {
            Some((sp, st)) if st < t.len() => {
                star = Some((sp, st + 1));
                pi = sp;
                ti = st + 1;
            }
            _ => return false,
        }
    }
}

/// `[...]` tại `p[start]`: trả về (khớp?, vị trí sau `]`), `None` nếu không đóng.
fn match_class(p: &[char], start: usize, c: char, nocase: bool) -> Option<(bool, usize)> {
    let mut i = start + 1;
    let negate = matches!(p.get(i), Some('!') | Some('^'));
    if negate {
        i += 1;
    }
    let mut matched = false;
    let mut first = true;
    while i < p.len() {
        if p[i] == ']' && !first {
            return Some((matched != negate, i + 1));
        }
        first = false;
        // [:class:]
        if p[i] == '[' && p.get(i + 1) == Some(&':') {
            if let Some(end) = (i + 2..p.len().saturating_sub(1)).find(|&k| p[k] == ':' && p[k + 1] == ']') {
                let class: String = p[i + 2..end].iter().collect();
                matched |= match class.as_str() {
                    "alpha" => c.is_alphabetic(),
                    "digit" => c.is_ascii_digit(),
                    "alnum" => c.is_alphanumeric(),
                    "upper" => c.is_uppercase(),
                    "lower" => c.is_lowercase(),
                    "space" => c.is_whitespace(),
                    "blank" => c == ' ' || c == '\t',
                    "punct" => c.is_ascii_punctuation(),
                    "xdigit" => c.is_ascii_hexdigit(),
                    "cntrl" => c.is_control(),
                    "print" => !c.is_control(),
                    "graph" => !c.is_control() && !c.is_whitespace(),
                    _ => false,
                };
                i = end + 2;
                continue;
            }
        }
        let mut lo = p[i];
        if lo == '\\' && i + 1 < p.len() {
            i += 1;
            lo = p[i];
        }
        if p.get(i + 1) == Some(&'-') && p.get(i + 2).is_some_and(|&h| h != ']') {
            let hi = p[i + 2];
            let (cl, lol, hil) = if nocase {
                (c.to_ascii_lowercase(), lo.to_ascii_lowercase(), hi.to_ascii_lowercase())
            } else {
                (c, lo, hi)
            };
            matched |= lol <= cl && cl <= hil;
            i += 3;
        } else {
            matched |= eq_char(lo, c, nocase);
            i += 1;
        }
    }
    None
}

fn char_boundaries(s: &str) -> Vec<usize> {
    s.char_indices().map(|(i, _)| i).chain(std::iter::once(s.len())).collect()
}

fn remove_prefix(s: &str, pat: &str, longest: bool, nocase: bool) -> String {
    let b = char_boundaries(s);
    let iter: Box<dyn Iterator<Item = &usize>> = if longest { Box::new(b.iter().rev()) } else { Box::new(b.iter()) };
    for &end in iter {
        if fnmatch(pat, &s[..end], nocase) {
            return s[end..].to_string();
        }
    }
    s.to_string()
}

fn remove_suffix(s: &str, pat: &str, longest: bool, nocase: bool) -> String {
    let b = char_boundaries(s);
    let iter: Box<dyn Iterator<Item = &usize>> = if longest { Box::new(b.iter()) } else { Box::new(b.iter().rev()) };
    for &start in iter {
        if fnmatch(pat, &s[start..], nocase) {
            return s[..start].to_string();
        }
    }
    s.to_string()
}

fn replace(s: &str, pat: &str, rep: &str, mode: ReplaceMode) -> String {
    if pat.is_empty() {
        return s.to_string();
    }
    let b = char_boundaries(s);
    match mode {
        ReplaceMode::Prefix => {
            for &end in b.iter().rev() {
                if fnmatch(pat, &s[..end], false) {
                    return format!("{}{}", rep, &s[end..]);
                }
            }
            s.to_string()
        }
        ReplaceMode::Suffix => {
            for &start in &b {
                if fnmatch(pat, &s[start..], false) {
                    return format!("{}{}", &s[..start], rep);
                }
            }
            s.to_string()
        }
        ReplaceMode::First | ReplaceMode::All => {
            let mut out = String::new();
            let mut i = 0; // chỉ số trong `b`
            while i < b.len() - 1 {
                // Khớp dài nhất bắt đầu tại b[i].
                let start = b[i];
                let hit = b[i + 1..].iter().rev().find(|&&end| fnmatch(pat, &s[start..end], false));
                match hit {
                    Some(&end) => {
                        out.push_str(rep);
                        i = b.iter().position(|&x| x == end).unwrap_or(b.len() - 1);
                        if mode == ReplaceMode::First {
                            out.push_str(&s[end..]);
                            return out;
                        }
                    }
                    None => {
                        out.push_str(&s[start..b[i + 1]]);
                        i += 1;
                    }
                }
            }
            out
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(src: &str) -> Vec<Word> {
        crate::lexer::tokenize(src)
            .unwrap()
            .into_iter()
            .filter_map(|t| match t {
                crate::lexer::Token::Word(w) => Some(w),
                _ => None,
            })
            .collect()
    }

    fn sources(ws: Vec<Word>) -> Vec<String> {
        ws.iter().map(|w| crate::lexer::parts_to_source(w)).collect()
    }

    #[test]
    fn brace_lists_and_ranges() {
        assert_eq!(sources(brace_expand(&words("a{b,c}d")[0])), vec!["abd", "acd"]);
        assert_eq!(sources(brace_expand(&words("{1..3}")[0])), vec!["1", "2", "3"]);
        assert_eq!(sources(brace_expand(&words("{5..1..2}")[0])), vec!["5", "3", "1"]);
        assert_eq!(sources(brace_expand(&words("{01..03}")[0])), vec!["01", "02", "03"]);
        assert_eq!(sources(brace_expand(&words("{a..c}")[0])), vec!["a", "b", "c"]);
        assert_eq!(sources(brace_expand(&words("{x,{y,z}}1")[0])), vec!["x1", "y1", "z1"]);
        // không phải brace expansion
        assert_eq!(sources(brace_expand(&words("{a}")[0])), vec!["{a}"]);
        assert_eq!(sources(brace_expand(&words("'{a,b}'")[0])), vec!["{a,b}"]);
    }

    #[test]
    fn pattern_matching() {
        assert!(fnmatch("*.txt", "a.txt", false));
        assert!(!fnmatch("*.txt", "a.log", false));
        assert!(fnmatch("a?c", "abc", false));
        assert!(fnmatch("[a-c]x", "bx", false));
        assert!(fnmatch("[!a-c]x", "dx", false));
        assert!(fnmatch("[[:digit:]]*", "9lives", false));
        assert!(fnmatch("\\*", "*", false));
        assert!(!fnmatch("\\*", "x", false));
        assert!(fnmatch("*/*", "a/b", false));
        assert!(fnmatch("A*", "abc", true));
    }

    #[test]
    fn remove_and_replace() {
        assert_eq!(remove_prefix("/a/b/c.txt", "*/", false, false), "a/b/c.txt");
        assert_eq!(remove_prefix("/a/b/c.txt", "*/", true, false), "c.txt");
        assert_eq!(remove_suffix("c.tar.gz", ".*", false, false), "c.tar");
        assert_eq!(remove_suffix("c.tar.gz", ".*", true, false), "c");
        assert_eq!(replace("aXbXc", "X", "-", ReplaceMode::First), "a-bXc");
        assert_eq!(replace("aXbXc", "X", "-", ReplaceMode::All), "a-b-c");
        assert_eq!(replace("abc", "a", "Z", ReplaceMode::Prefix), "Zbc");
        assert_eq!(replace("abc", "c", "Z", ReplaceMode::Suffix), "abZ");
        assert_eq!(replace("aaa", "a*", "x", ReplaceMode::All), "x");
    }

    #[test]
    fn field_splitting() {
        let f = |segs: Vec<(&str, Kind)>| Field { segs: segs.into_iter().map(|(t, k)| Seg { text: t.into(), kind: k }).collect(), quoted: false };
        let texts = |fs: Vec<Field>| fs.iter().map(Field::text).collect::<Vec<_>>();
        assert_eq!(texts(split_field(f(vec![("a  b c", Kind::Exp)]), " \t\n")), vec!["a", "b", "c"]);
        assert_eq!(texts(split_field(f(vec![("x", Kind::Lit), ("1 2", Kind::Exp)]), " \t\n")), vec!["x1", "2"]);
        assert_eq!(texts(split_field(f(vec![("a::b", Kind::Exp)]), ":")), vec!["a", "", "b"]);
        assert!(split_field(f(vec![("  ", Kind::Exp)]), " \t\n").is_empty());
        assert_eq!(texts(split_field(f(vec![("a b", Kind::Quoted)]), " ")), vec!["a b"]);
    }
}
