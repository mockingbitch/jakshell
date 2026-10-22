//! Builtin phục vụ viết script: điều khiển luồng, `local`, `let`, `test`,
//! `shopt`, `getopts`, `mapfile`, `printf`.

use anyhow::{anyhow, Result};
use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::rc::Rc;

use crate::shell::{Flow, Shell, Shopts};

thread_local! {
    /// Đang `source` lồng bao nhiêu tầng — `return` hợp lệ trong file được source.
    pub static SOURCE_DEPTH: Cell<u32> = const { Cell::new(0) };
    /// Trạng thái nội bộ của `getopts`: (OPTIND lần trước, vị trí ký tự trong cụm `-abc`).
    static GETOPTS: Cell<(i64, usize)> = const { Cell::new((0, 0)) };
}

fn count_arg(name: &str, args: &[&str]) -> Result<u32> {
    match args.first() {
        None => Ok(1),
        Some(a) => match a.parse::<u32>() {
            Ok(n) if n >= 1 => Ok(n),
            _ => Err(anyhow!("{}: {}: cần số nguyên ≥ 1", name, a)),
        },
    }
}

/// `break [n]` / `continue [n]`.
pub fn loop_control(shell: &Rc<RefCell<Shell>>, name: &str, args: &[&str]) -> Result<i32> {
    let n = count_arg(name, args)?;
    let mut sh = shell.borrow_mut();
    if sh.loop_depth == 0 {
        eprintln!("jaksh: {}: chỉ dùng được bên trong vòng lặp", name);
        return Ok(0);
    }
    let n = n.min(sh.loop_depth);
    sh.flow = if name == "break" { Flow::Break(n) } else { Flow::Continue(n) };
    Ok(0)
}

/// `return [n]` — trong function hoặc file đang `source`.
pub fn return_(shell: &Rc<RefCell<Shell>>, args: &[&str]) -> Result<i32> {
    let mut sh = shell.borrow_mut();
    if sh.funcnames.is_empty() && SOURCE_DEPTH.with(|d| d.get()) == 0 {
        return Err(anyhow!("return: chỉ dùng được trong function hoặc script được source"));
    }
    let code = match args.first() {
        Some(a) => a.parse::<i64>().map_err(|_| anyhow!("return: {}: cần số", a))? as i32 & 0xff,
        None => sh.last_status(),
    };
    sh.flow = Flow::Return;
    sh.set_last_status(code);
    Ok(code)
}

/// `local [-a|-A|-r|-x] name[=value]...`
pub fn local(shell: &Rc<RefCell<Shell>>, args: &[&str]) -> Result<i32> {
    let mut indexed = false;
    let mut assoc = false;
    let mut ro = false;
    let mut export = false;
    let mut code = 0;
    for a in args {
        if let Some(flags) = a.strip_prefix('-').filter(|f| !f.is_empty() && !f.contains('=')) {
            for f in flags.chars() {
                match f {
                    'a' => indexed = true,
                    'A' => assoc = true,
                    'r' => ro = true,
                    'x' => export = true,
                    'i' | 'l' | 'u' | 'n' | 't' => {}
                    _ => return Err(anyhow!("local: -{}: tuỳ chọn không hợp lệ", f)),
                }
            }
            continue;
        }
        let (name, value) = a.split_once('=').map_or((*a, None), |(k, v)| (k, Some(v)));
        let mut sh = shell.borrow_mut();
        sh.make_local(name)?;
        // Biến local bắt đầu rỗng (chưa gán) cho tới khi được gán.
        sh.env.remove(name);
        sh.arrays.remove(name);
        sh.assocs.remove(name);
        if assoc {
            sh.set_assoc(name, BTreeMap::new())?;
        } else if indexed {
            sh.set_array(name, BTreeMap::new())?;
        }
        if let Some(v) = value {
            if let Err(e) = sh.set_var(name, v) {
                eprintln!("jaksh: local: {e}");
                code = 1;
            }
        }
        if export {
            sh.export_var(name, None)?;
        }
        if ro {
            sh.readonly.insert(name.to_string());
        }
    }
    Ok(code)
}

/// `let expr...` — 0 nếu biểu thức cuối khác 0.
pub fn let_(shell: &Rc<RefCell<Shell>>, args: &[&str]) -> Result<i32> {
    if args.is_empty() {
        return Err(anyhow!("let: thiếu biểu thức"));
    }
    let mut last = 0;
    for a in args {
        last = crate::expand::eval_arith(shell, a)?;
    }
    Ok((last == 0) as i32)
}

/// `shopt [-s|-u|-p|-q] [tên...]`
pub fn shopt(shell: &Rc<RefCell<Shell>>, args: &[&str]) -> Result<i32> {
    // Tuỳ chọn của bash không ảnh hưởng jaksh — nhận cho êm để .bashrc chạy được.
    const HARMLESS: [&str; 12] = [
        "checkwinsize", "histappend", "cmdhist", "progcomp", "interactive_comments", "promptvars", "hostcomplete",
        "complete_fullquote", "sourcepath", "autocd", "cdspell", "checkhash",
    ];
    let mut set = None;
    let mut print = false;
    let mut quiet = false;
    let mut names = Vec::new();
    for a in args {
        match *a {
            "-s" => set = Some(true),
            "-u" => set = Some(false),
            "-p" => print = true,
            "-q" => quiet = true,
            "-o" => {}
            n => names.push(n),
        }
    }
    let mut sh = shell.borrow_mut();
    if names.is_empty() {
        for n in Shopts::NAMES {
            let v = *sh.shopts.slot(n).expect("tên trong NAMES");
            if set.is_none_or(|s| s == v) {
                if print {
                    println!("shopt {} {}", if v { "-s" } else { "-u" }, n);
                } else {
                    println!("{:<15} {}", n, if v { "on" } else { "off" });
                }
            }
        }
        return Ok(0);
    }
    let mut code = 0;
    for n in names {
        match sh.shopts.slot(n) {
            Some(slot) => match set {
                Some(v) => *slot = v,
                None => {
                    if !*slot {
                        code = 1;
                    }
                    if !quiet {
                        println!("{:<15} {}", n, if *slot { "on" } else { "off" });
                    }
                }
            },
            None if HARMLESS.contains(&n) => {}
            None => {
                eprintln!("jaksh: shopt: {}: tên tuỳ chọn không hợp lệ", n);
                code = 1;
            }
        }
    }
    Ok(code)
}

/// `getopts optstring name [args...]`
pub fn getopts(shell: &Rc<RefCell<Shell>>, args: &[&str]) -> Result<i32> {
    let (optstring, name) = match args {
        [o, n, ..] => (*o, *n),
        _ => return Err(anyhow!("getopts: cách dùng: getopts optstring tên [đối số...]")),
    };
    let params: Vec<String> =
        if args.len() > 2 { args[2..].iter().map(|s| s.to_string()).collect() } else { shell.borrow().positional.clone() };
    let silent = optstring.starts_with(':');
    let spec = optstring.trim_start_matches(':');
    let mut sh = shell.borrow_mut();
    let mut optind: i64 = sh.get_var("OPTIND").and_then(|v| v.parse().ok()).unwrap_or(1).max(1);
    let (last_ind, mut pos) = GETOPTS.with(|g| g.get());
    if last_ind != optind {
        pos = 0;
    }
    let finish = |sh: &mut Shell, optind: i64, pos: usize| -> Result<()> {
        GETOPTS.with(|g| g.set((optind, pos)));
        sh.set_var("OPTIND", &optind.to_string())
    };

    let Some(arg) = params.get((optind - 1) as usize) else {
        sh.set_var(name, "?")?;
        finish(&mut sh, optind, 0)?;
        return Ok(1);
    };
    if pos == 0 {
        if arg == "--" {
            sh.set_var(name, "?")?;
            finish(&mut sh, optind + 1, 0)?;
            return Ok(1);
        }
        if !arg.starts_with('-') || arg == "-" {
            sh.set_var(name, "?")?;
            finish(&mut sh, optind, 0)?;
            return Ok(1);
        }
        pos = 1;
    }
    let chars: Vec<char> = arg.chars().collect();
    let c = chars[pos];
    pos += 1;
    let at_end = pos >= chars.len();
    let takes_arg = spec.find(c).filter(|_| c != ':').map(|i| spec[i + c.len_utf8()..].starts_with(':'));
    match takes_arg {
        None => {
            if silent {
                sh.set_var("OPTARG", &c.to_string())?;
            } else {
                eprintln!("{}: tuỳ chọn không hợp lệ -- {}", sh.arg0, c);
                let _ = sh.unset_var("OPTARG");
            }
            sh.set_var(name, "?")?;
            if at_end {
                optind += 1;
                pos = 0;
            }
        }
        Some(true) => {
            if !at_end {
                let rest: String = chars[pos..].iter().collect();
                sh.set_var("OPTARG", &rest)?;
                sh.set_var(name, &c.to_string())?;
                optind += 1;
            } else if let Some(v) = params.get(optind as usize) {
                sh.set_var("OPTARG", v)?;
                sh.set_var(name, &c.to_string())?;
                optind += 2;
            } else if silent {
                sh.set_var("OPTARG", &c.to_string())?;
                sh.set_var(name, ":")?;
                optind += 1;
            } else {
                eprintln!("{}: tuỳ chọn cần đối số -- {}", sh.arg0, c);
                let _ = sh.unset_var("OPTARG");
                sh.set_var(name, "?")?;
                optind += 1;
            }
            pos = 0;
        }
        Some(false) => {
            let _ = sh.unset_var("OPTARG");
            sh.set_var(name, &c.to_string())?;
            if at_end {
                optind += 1;
                pos = 0;
            }
        }
    }
    finish(&mut sh, optind, pos)?;
    Ok(0)
}

/// `mapfile` / `readarray [-t] [-n N] [-s N] [-d delim] [mảng]` — đọc stdin vào mảng.
pub fn mapfile(shell: &Rc<RefCell<Shell>>, args: &[&str]) -> Result<i32> {
    let mut trim = false;
    let mut count: Option<usize> = None;
    let mut skip = 0usize;
    let mut delim = '\n';
    let mut name = "MAPFILE".to_string();
    let mut i = 0;
    while i < args.len() {
        let next = |i: &mut usize| -> Result<String> {
            *i += 1;
            args.get(*i).map(|s| s.to_string()).ok_or_else(|| anyhow!("mapfile: thiếu giá trị"))
        };
        match args[i] {
            "-t" => trim = true,
            "-n" => count = Some(next(&mut i)?.parse().map_err(|_| anyhow!("mapfile: -n cần số"))?),
            "-s" => skip = next(&mut i)?.parse().map_err(|_| anyhow!("mapfile: -s cần số"))?,
            "-d" => delim = next(&mut i)?.chars().next().unwrap_or('\0'),
            a if a.starts_with('-') => return Err(anyhow!("mapfile: {}: tuỳ chọn không hỗ trợ", a)),
            a => name = a.to_string(),
        }
        i += 1;
    }
    let mut data = Vec::new();
    std::io::Read::read_to_end(&mut std::io::stdin().lock(), &mut data)?;
    let text = String::from_utf8_lossy(&data);
    let mut arr = BTreeMap::new();
    let mut idx = 0i64;
    for (n, piece) in text.split_inclusive(delim).enumerate() {
        if n < skip {
            continue;
        }
        if count.is_some_and(|c| idx as usize >= c && c > 0) {
            break;
        }
        let item = if trim { piece.strip_suffix(delim).unwrap_or(piece) } else { piece };
        arr.insert(idx, item.to_string());
        idx += 1;
    }
    shell.borrow_mut().set_array(&name, arr)?;
    Ok(0)
}

/// `printf [-v var] format [args...]` — builtin đầy đủ: %s %b %q %d %i %u %o
/// %x %X %c %f %e %g %%, cờ `-+ #0`, độ rộng / độ chính xác (kể cả `*`).
/// Định dạng được dùng lại tới khi hết đối số (như bash).
pub fn printf(shell: &Rc<RefCell<Shell>>, args: &[&str]) -> Result<i32> {
    use std::io::Write;
    let (var, rest) = match args {
        ["-v", v, rest @ ..] => (Some(*v), rest),
        ["--", rest @ ..] => (None, rest),
        _ => (None, args),
    };
    let Some((fmt, params)) = rest.split_first() else {
        return Err(anyhow!("printf: cách dùng: printf [-v biến] định_dạng [đối số...]"));
    };
    let (out, ok) = format_printf(fmt, params);
    match var {
        Some(v) => shell.borrow_mut().set_var(v, &out)?,
        None => {
            if unsafe { libc::fcntl(libc::STDOUT_FILENO, libc::F_GETFD) } < 0 {
                return Err(anyhow!("printf: lỗi ghi: Bad file descriptor"));
            }
            let mut stdout = std::io::stdout().lock();
            stdout.write_all(out.as_bytes()).and_then(|_| stdout.flush()).map_err(|e| anyhow!("printf: lỗi ghi: {}", e))?;
        }
    }
    Ok(if ok { 0 } else { 1 })
}

/// Định dạng kiểu printf. Trả về (chuỗi, mọi số hợp lệ?).
pub fn format_printf(fmt: &str, params: &[&str]) -> (String, bool) {
    let f: Vec<char> = fmt.chars().collect();
    let mut out = String::new();
    let mut idx = 0;
    let mut ok = true;
    loop {
        let start_idx = idx;
        let mut i = 0;
        let mut stop = false;
        while i < f.len() && !stop {
            let c = f[i];
            if c == '\\' {
                // Escape trong chuỗi định dạng.
                let mut j = i + 1;
                let mut esc = String::from("\\");
                if j < f.len() {
                    esc.push(f[j]);
                    j += 1;
                    if matches!(f[j - 1], '0'..='7' | 'x') {
                        while j < f.len() && esc.len() < 5 && f[j].is_ascii_hexdigit() {
                            esc.push(f[j]);
                            j += 1;
                        }
                    }
                }
                let (txt, c_stop) = crate::builtins::interpret_escapes(&esc);
                out.push_str(&txt);
                if c_stop {
                    return (out, ok);
                }
                i = j;
                continue;
            }
            if c != '%' {
                out.push(c);
                i += 1;
                continue;
            }
            if f.get(i + 1) == Some(&'%') {
                out.push('%');
                i += 2;
                continue;
            }
            // %[flags][width][.precision]conv
            let mut j = i + 1;
            let mut flags = String::new();
            while j < f.len() && "-+ #0".contains(f[j]) {
                flags.push(f[j]);
                j += 1;
            }
            let take_num = |j: &mut usize, idx: &mut usize| -> Option<usize> {
                if f.get(*j) == Some(&'*') {
                    *j += 1;
                    let v = params.get(*idx).and_then(|s| s.parse::<i64>().ok()).unwrap_or(0);
                    *idx += 1;
                    return Some(v.unsigned_abs() as usize);
                }
                let st = *j;
                while *j < f.len() && f[*j].is_ascii_digit() {
                    *j += 1;
                }
                (st != *j).then(|| f[st..*j].iter().collect::<String>().parse().unwrap_or(0))
            };
            let width = take_num(&mut j, &mut idx);
            let mut precision = None;
            if f.get(j) == Some(&'.') {
                j += 1;
                precision = Some(take_num(&mut j, &mut idx).unwrap_or(0));
            }
            // bỏ qua tiền tố độ dài kiểu C (l, h, …)
            while j < f.len() && "hlLqjzt".contains(f[j]) {
                j += 1;
            }
            let Some(&conv) = f.get(j) else {
                out.push_str(&f[i..].iter().collect::<String>());
                break;
            };
            let arg = params.get(idx).copied();
            if arg.is_some() {
                idx += 1;
            }
            let arg = arg.unwrap_or("");
            let body = match conv {
                's' => {
                    let s: String = match precision {
                        Some(p) => arg.chars().take(p).collect(),
                        None => arg.to_string(),
                    };
                    s
                }
                'b' => {
                    let (txt, c_stop) = crate::builtins::interpret_escapes(arg);
                    if c_stop {
                        out.push_str(&pad(&txt, width, &flags, false));
                        return (out, ok);
                    }
                    txt
                }
                'q' => crate::builtins::sq(arg),
                'c' => arg.chars().next().map(String::from).unwrap_or_default(),
                'd' | 'i' | 'u' | 'o' | 'x' | 'X' => {
                    let n = parse_int_arg(arg).unwrap_or_else(|| {
                        if !arg.is_empty() {
                            eprintln!("jaksh: printf: {}: cần số", arg);
                            ok = false;
                        }
                        0
                    });
                    let digits = match conv {
                        'o' => format!("{:o}", n),
                        'x' => format!("{:x}", n),
                        'X' => format!("{:X}", n),
                        'u' => (n as u64).to_string(),
                        _ => n.unsigned_abs().to_string(),
                    };
                    let digits = match precision {
                        Some(p) if digits.len() < p => format!("{}{}", "0".repeat(p - digits.len()), digits),
                        _ => digits,
                    };
                    let sign = if matches!(conv, 'd' | 'i') {
                        if n < 0 {
                            "-"
                        } else if flags.contains('+') {
                            "+"
                        } else if flags.contains(' ') {
                            " "
                        } else {
                            ""
                        }
                    } else {
                        ""
                    };
                    let prefix = if flags.contains('#') && n != 0 {
                        match conv {
                            'o' => "0",
                            'x' => "0x",
                            'X' => "0X",
                            _ => "",
                        }
                    } else {
                        ""
                    };
                    out.push_str(&pad_num(sign, prefix, &digits, width, &flags, precision.is_some()));
                    i = j + 1;
                    continue;
                }
                'f' | 'F' | 'e' | 'E' | 'g' | 'G' => {
                    let x: f64 = arg.trim().parse().or_else(|_| parse_int_arg(arg).map(|n| n as f64).ok_or(())).unwrap_or_else(|_| {
                        if !arg.is_empty() {
                            eprintln!("jaksh: printf: {}: cần số", arg);
                            ok = false;
                        }
                        0.0
                    });
                    let p = precision.unwrap_or(6);
                    let s = match conv {
                        'f' | 'F' => format!("{:.*}", p, x.abs()),
                        'e' | 'E' => {
                            let s = format_exp(x.abs(), p);
                            if conv == 'E' { s.to_uppercase() } else { s }
                        }
                        _ => {
                            // %g: dạng ngắn hơn giữa %f và %e, bỏ số 0 thừa.
                            let p = p.max(1);
                            let exp = if x == 0.0 { 0 } else { x.abs().log10().floor() as i32 };
                            let s = if exp < -4 || exp >= p as i32 {
                                format_exp(x.abs(), p - 1)
                            } else {
                                format!("{:.*}", (p as i32 - 1 - exp).max(0) as usize, x.abs())
                            };
                            let s = if s.contains('.') && !flags.contains('#') {
                                let (mant, e) = s.split_once('e').map_or((s.as_str(), None), |(m, e)| (m, Some(e)));
                                let mant = mant.trim_end_matches('0').trim_end_matches('.');
                                e.map_or(mant.to_string(), |e| format!("{}e{}", mant, e))
                            } else {
                                s
                            };
                            if conv == 'G' { s.to_uppercase() } else { s }
                        }
                    };
                    let sign = if x.is_sign_negative() && x != 0.0 {
                        "-"
                    } else if flags.contains('+') {
                        "+"
                    } else if flags.contains(' ') {
                        " "
                    } else {
                        ""
                    };
                    out.push_str(&pad_num(sign, "", &s, width, &flags, false));
                    i = j + 1;
                    continue;
                }
                other => {
                    eprintln!("jaksh: printf: %{}: định dạng không hợp lệ", other);
                    ok = false;
                    stop = true;
                    String::new()
                }
            };
            out.push_str(&pad(&body, width, &flags, false));
            i = j + 1;
        }
        // Dùng lại định dạng nếu còn đối số chưa dùng (và lượt vừa rồi có dùng).
        if idx >= params.len() || idx == start_idx || stop {
            break;
        }
    }
    (out, ok)
}

/// Số nguyên kiểu printf: thập phân, 0x…, 0…(bát phân), `'c` = mã ký tự.
fn parse_int_arg(s: &str) -> Option<i64> {
    let t = s.trim();
    if let Some(c) = t.strip_prefix('\'').or_else(|| t.strip_prefix('"')) {
        return c.chars().next().map(|c| c as i64);
    }
    if t.is_empty() {
        return Some(0);
    }
    let (neg, body) = match t.strip_prefix('-') {
        Some(b) => (true, b),
        None => (false, t.strip_prefix('+').unwrap_or(t)),
    };
    let v = crate::arith::parse_number(body).ok()?;
    Some(if neg { -v } else { v })
}

fn format_exp(x: f64, p: usize) -> String {
    // Rust in `1.5e2`; printf muốn `1.500000e+02`.
    let s = format!("{:.*e}", p, x);
    match s.split_once('e') {
        Some((m, e)) => {
            let (sign, digits) = match e.strip_prefix('-') {
                Some(d) => ('-', d),
                None => ('+', e),
            };
            format!("{}e{}{:0>2}", m, sign, digits)
        }
        None => s,
    }
}

fn pad(s: &str, width: Option<usize>, flags: &str, _numeric: bool) -> String {
    let w = width.unwrap_or(0);
    let len = s.chars().count();
    if len >= w {
        return s.to_string();
    }
    if flags.contains('-') {
        format!("{}{}", s, " ".repeat(w - len))
    } else {
        format!("{}{}", " ".repeat(w - len), s)
    }
}

fn pad_num(sign: &str, prefix: &str, digits: &str, width: Option<usize>, flags: &str, has_precision: bool) -> String {
    let w = width.unwrap_or(0);
    let len = sign.len() + prefix.len() + digits.len();
    if len >= w {
        return format!("{}{}{}", sign, prefix, digits);
    }
    if flags.contains('-') {
        format!("{}{}{}{}", sign, prefix, digits, " ".repeat(w - len))
    } else if flags.contains('0') && !has_precision {
        format!("{}{}{}{}", sign, prefix, "0".repeat(w - len), digits)
    } else {
        format!("{}{}{}{}", " ".repeat(w - len), sign, prefix, digits)
    }
}

#[cfg(test)]
mod tests {
    use super::format_printf;

    fn pf(fmt: &str, args: &[&str]) -> String {
        format_printf(fmt, args).0
    }

    #[test]
    fn printf_formats() {
        assert_eq!(pf("%s=%03d %.2f\\n", &["n", "7", "3.14159"]), "n=007 3.14\n");
        assert_eq!(pf("%-5s|%5s|", &["ab", "cd"]), "ab   |   cd|");
        assert_eq!(pf("%x %X %o %#x", &["255", "255", "8", "255"]), "ff FF 10 0xff");
        assert_eq!(pf("%s\\n", &["a", "b", "c"]), "a\nb\nc\n", "dùng lại định dạng");
        assert_eq!(pf("%d %d\\n", &["1"]), "1 0\n", "thiếu đối số = 0");
        assert_eq!(pf("%b", &["a\\tb"]), "a\tb");
        assert_eq!(pf("%c%c", &["xyz", "q"]), "xq");
        assert_eq!(pf("%d", &["'A"]), "65");
        assert_eq!(pf("%+d % d %05.1f", &["5", "5", "-2.25"]), "+5  5 -02.2");
        assert_eq!(pf("%e", &["12345.678"]), "1.234568e+04");
        assert_eq!(pf("%g %g", &["0.0001", "123456789"]), "0.0001 1.23457e+08");
        assert_eq!(pf("%*d|%-*d|", &["4", "7", "3", "8"]), "   7|8  |");
        assert_eq!(pf("100%%", &[]), "100%");
    }
}
