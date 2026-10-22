//! Builtin hệ thống: `cd` / `pushd` / `popd` / `dirs`, `umask`, `ulimit`,
//! `times`, `trap`.

use anyhow::{anyhow, Result};
use std::cell::RefCell;
use std::path::{Component, Path, PathBuf};
use std::rc::Rc;

use crate::shell::Shell;

/// Chuẩn hoá đường dẫn theo chữ (xử lý `.` / `..` không đi qua symlink) — chế độ
/// "logical" mặc định của `cd` trong bash.
fn normalize(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    if out.as_os_str().is_empty() {
        out.push("/");
    }
    out
}

/// Đổi thư mục hiện tại (cập nhật PWD / OLDPWD). `physical` = giải symlink.
pub fn change_dir(shell: &Rc<RefCell<Shell>>, target: &Path, physical: bool) -> Result<PathBuf> {
    let abs = if target.is_absolute() { target.to_path_buf() } else { shell.borrow().cwd.join(target) };
    let dest = if physical {
        std::fs::canonicalize(&abs).map_err(|e| anyhow!("cd: {}: {}", target.display(), e))?
    } else {
        normalize(&abs)
    };
    if !dest.is_dir() {
        let why = if dest.exists() { "không phải thư mục" } else { "không tồn tại" };
        return Err(anyhow!("cd: {}: {}", target.display(), why));
    }
    std::env::set_current_dir(&dest).map_err(|e| anyhow!("cd: {}: {}", target.display(), e))?;
    let old = shell.borrow().cwd.clone();
    let mut sh = shell.borrow_mut();
    sh.export_var("OLDPWD", Some(&old.display().to_string()))?;
    sh.cwd = dest.clone();
    sh.export_var("PWD", Some(&dest.display().to_string()))?;
    Ok(dest)
}

/// `cd [-L|-P] [dir]` — `cd` = $HOME, `cd -` = $OLDPWD (in thư mục mới), tìm
/// theo `$CDPATH` với đường dẫn tương đối.
pub fn cd(shell: &Rc<RefCell<Shell>>, args: &[&str]) -> Result<i32> {
    let mut physical = false;
    let mut rest = Vec::new();
    for a in args {
        match *a {
            "-P" => physical = true,
            "-L" => physical = false,
            "--" => {}
            _ => rest.push(*a),
        }
    }
    let (target, print) = match rest.first() {
        None => {
            let home = shell.borrow().get_var("HOME").ok_or_else(|| anyhow!("cd: HOME chưa được đặt"))?;
            (PathBuf::from(home), false)
        }
        Some(&"-") => {
            let prev = shell.borrow().get_var("OLDPWD").ok_or_else(|| anyhow!("cd: OLDPWD chưa được đặt"))?;
            (PathBuf::from(prev), true)
        }
        Some(dir) => {
            let p = PathBuf::from(dir);
            // CDPATH chỉ áp cho đường dẫn tương đối không bắt đầu bằng `.` / `..`.
            let starts_dot = dir.starts_with("./") || dir.starts_with("../") || *dir == "." || *dir == "..";
            let cdpath = shell.borrow().get_var("CDPATH").unwrap_or_default();
            let mut found = None;
            if !p.is_absolute() && !starts_dot && !cdpath.is_empty() {
                for base in cdpath.split(':') {
                    let candidate = if base.is_empty() { PathBuf::from(dir) } else { Path::new(base).join(dir) };
                    let abs = if candidate.is_absolute() { candidate.clone() } else { shell.borrow().cwd.join(&candidate) };
                    if abs.is_dir() {
                        found = Some((candidate, !base.is_empty()));
                        break;
                    }
                }
            }
            found.unwrap_or((p, false))
        }
    };
    let dest = change_dir(shell, &target, physical)?;
    if print {
        println!("{}", dest.display());
    }
    Ok(0)
}

/// Rút gọn `$HOME` thành `~` (cho `dirs`).
fn tilde(shell: &Shell, p: &Path) -> String {
    let s = p.display().to_string();
    match shell.get_var("HOME") {
        Some(h) if !h.is_empty() && (s == h || s.starts_with(&format!("{}/", h))) => format!("~{}", &s[h.len()..]),
        _ => s,
    }
}

/// Toàn bộ ngăn xếp: [thư mục hiện tại, …dirstack].
fn full_stack(shell: &Shell) -> Vec<PathBuf> {
    std::iter::once(shell.cwd.clone()).chain(shell.dirstack.iter().cloned()).collect()
}

fn print_stack(shell: &Shell) {
    let items: Vec<String> = full_stack(shell).iter().map(|p| tilde(shell, p)).collect();
    println!("{}", items.join(" "));
}

/// `+N` / `-N` → vị trí trong ngăn xếp đầy đủ.
fn stack_index(spec: &str, len: usize) -> Option<usize> {
    let n: usize = spec[1..].parse().ok()?;
    if n >= len {
        return None;
    }
    Some(if spec.starts_with('+') { n } else { len - 1 - n })
}

/// `dirs [-c] [-l] [-p] [-v] [+N|-N]`
pub fn dirs(shell: &Rc<RefCell<Shell>>, args: &[&str]) -> Result<i32> {
    let (mut long, mut per_line, mut verbose) = (false, false, false);
    for a in args {
        match *a {
            "-c" => {
                shell.borrow_mut().dirstack.clear();
                return Ok(0);
            }
            "-l" => long = true,
            "-p" => per_line = true,
            "-v" => verbose = true,
            spec if spec.starts_with('+') || spec.starts_with('-') => {
                let sh = shell.borrow();
                let stack = full_stack(&sh);
                let i = stack_index(spec, stack.len()).ok_or_else(|| anyhow!("dirs: {}: ngoài phạm vi ngăn xếp", spec))?;
                println!("{}", if long { stack[i].display().to_string() } else { tilde(&sh, &stack[i]) });
                return Ok(0);
            }
            other => return Err(anyhow!("dirs: {}: tuỳ chọn không hợp lệ", other)),
        }
    }
    let sh = shell.borrow();
    let show = |p: &PathBuf| if long { p.display().to_string() } else { tilde(&sh, p) };
    let stack = full_stack(&sh);
    if verbose {
        for (i, p) in stack.iter().enumerate() {
            println!("{:2}  {}", i, show(p));
        }
    } else if per_line {
        for p in &stack {
            println!("{}", show(p));
        }
    } else {
        println!("{}", stack.iter().map(show).collect::<Vec<_>>().join(" "));
    }
    Ok(0)
}

/// `pushd [dir | +N | -N]` — không đối số: đổi chỗ 2 thư mục đầu.
pub fn pushd(shell: &Rc<RefCell<Shell>>, args: &[&str]) -> Result<i32> {
    let quiet = args.contains(&"-n");
    let args: Vec<&str> = args.iter().copied().filter(|a| *a != "-n").collect();
    match args.first() {
        None => {
            let Some(top) = shell.borrow().dirstack.first().cloned() else {
                return Err(anyhow!("pushd: không có thư mục nào khác trong ngăn xếp"));
            };
            let cur = shell.borrow().cwd.clone();
            change_dir(shell, &top, false)?;
            shell.borrow_mut().dirstack[0] = cur;
        }
        Some(spec) if (spec.starts_with('+') || spec.starts_with('-')) && spec[1..].chars().all(|c| c.is_ascii_digit()) && spec.len() > 1 => {
            // Xoay ngăn xếp để phần tử N lên đầu.
            let mut stack = full_stack(&shell.borrow());
            let i = stack_index(spec, stack.len()).ok_or_else(|| anyhow!("pushd: {}: ngoài phạm vi ngăn xếp", spec))?;
            stack.rotate_left(i);
            change_dir(shell, &stack[0], false)?;
            shell.borrow_mut().dirstack = stack[1..].to_vec();
        }
        Some(dir) => {
            let cur = shell.borrow().cwd.clone();
            cd(shell, &[dir])?;
            shell.borrow_mut().dirstack.insert(0, cur);
        }
    }
    if !quiet {
        print_stack(&shell.borrow());
    }
    Ok(0)
}

/// `popd [+N | -N]` — bỏ thư mục đầu ngăn xếp và cd tới thư mục kế.
pub fn popd(shell: &Rc<RefCell<Shell>>, args: &[&str]) -> Result<i32> {
    let quiet = args.contains(&"-n");
    let spec = args.iter().find(|a| **a != "-n").copied();
    if shell.borrow().dirstack.is_empty() {
        return Err(anyhow!("popd: ngăn xếp thư mục trống"));
    }
    match spec {
        None | Some("+0") => {
            let next = shell.borrow_mut().dirstack.remove(0);
            change_dir(shell, &next, false)?;
        }
        Some(spec) => {
            let len = full_stack(&shell.borrow()).len();
            let i = stack_index(spec, len).ok_or_else(|| anyhow!("popd: {}: ngoài phạm vi ngăn xếp", spec))?;
            if i == 0 {
                let next = shell.borrow_mut().dirstack.remove(0);
                change_dir(shell, &next, false)?;
            } else {
                shell.borrow_mut().dirstack.remove(i - 1);
            }
        }
    }
    if !quiet {
        print_stack(&shell.borrow());
    }
    Ok(0)
}

/// `umask [-S] [mode]` — mode bát phân (`022`) hoặc ký hiệu (`u=rwx,g=rx,o=`).
pub fn umask(_shell: &Rc<RefCell<Shell>>, args: &[&str]) -> Result<i32> {
    let symbolic = args.contains(&"-S");
    let mode = args.iter().find(|a| !a.starts_with('-')).copied();
    let current = unsafe {
        let m = libc::umask(0);
        libc::umask(m);
        m as u32
    };
    let Some(mode) = mode else {
        if symbolic {
            let perms = |shift: u32| {
                let allowed = !current >> shift & 0o7;
                format!("{}{}{}", if allowed & 4 != 0 { "r" } else { "" }, if allowed & 2 != 0 { "w" } else { "" }, if allowed & 1 != 0 { "x" } else { "" })
            };
            println!("u={},g={},o={}", perms(6), perms(3), perms(0));
        } else {
            println!("{:04o}", current);
        }
        return Ok(0);
    };
    let new = if mode.chars().all(|c| c.is_digit(8)) {
        u32::from_str_radix(mode, 8).map_err(|_| anyhow!("umask: {}: mode không hợp lệ", mode))?
    } else {
        // Ký hiệu: thao tác trên quyền ĐƯỢC PHÉP (= phần bù của mask).
        let mut allowed = !current & 0o777;
        for clause in mode.split(',') {
            let pos = clause.find(['=', '+', '-']).ok_or_else(|| anyhow!("umask: {}: mode không hợp lệ", mode))?;
            let (who, rest) = clause.split_at(pos);
            let (op, perms) = rest.split_at(1);
            let who = if who.is_empty() || who.contains('a') { "ugo" } else { who };
            let mut bits = 0;
            for p in perms.chars() {
                bits |= match p {
                    'r' => 4,
                    'w' => 2,
                    'x' => 1,
                    _ => return Err(anyhow!("umask: {}: quyền không hợp lệ", p)),
                };
            }
            for w in who.chars() {
                let shift = match w {
                    'u' => 6,
                    'g' => 3,
                    'o' => 0,
                    _ => return Err(anyhow!("umask: {}: đối tượng không hợp lệ", w)),
                };
                match op {
                    "=" => allowed = (allowed & !(0o7 << shift)) | (bits << shift),
                    "+" => allowed |= bits << shift,
                    _ => allowed &= !(bits << shift),
                }
            }
        }
        !allowed & 0o777
    };
    unsafe { libc::umask(new as libc::mode_t) };
    Ok(0)
}

/// Bảng giới hạn của `ulimit`: (cờ, tài nguyên, mô tả, hệ số đơn vị).
#[cfg(target_os = "linux")]
type Resource = libc::__rlimit_resource_t;
#[cfg(not(target_os = "linux"))]
type Resource = libc::c_int;

fn ulimit_table() -> Vec<(char, Resource, &'static str, u64)> {
    vec![
        ('c', libc::RLIMIT_CORE, "core file size          (blocks)", 512),
        ('d', libc::RLIMIT_DATA, "data seg size           (kbytes)", 1024),
        ('f', libc::RLIMIT_FSIZE, "file size               (blocks)", 512),
        ('l', libc::RLIMIT_MEMLOCK, "max locked memory       (kbytes)", 1024),
        ('m', libc::RLIMIT_RSS, "max memory size         (kbytes)", 1024),
        ('n', libc::RLIMIT_NOFILE, "open files                      ", 1),
        ('s', libc::RLIMIT_STACK, "stack size              (kbytes)", 1024),
        ('t', libc::RLIMIT_CPU, "cpu time               (seconds)", 1),
        ('u', libc::RLIMIT_NPROC, "max user processes              ", 1),
        ('v', libc::RLIMIT_AS, "virtual memory          (kbytes)", 1024),
    ]
}

/// `ulimit [-SH] [-a | -cdflmnstuv] [giá_trị|unlimited]` (mặc định `-f`).
pub fn ulimit(_shell: &Rc<RefCell<Shell>>, args: &[&str]) -> Result<i32> {
    let table = ulimit_table();
    let mut hard = false;
    let mut soft = false;
    let mut all = false;
    let mut which = 'f';
    let mut value = None;
    for a in args {
        if let Some(flags) = a.strip_prefix('-') {
            for f in flags.chars() {
                match f {
                    'H' => hard = true,
                    'S' => soft = true,
                    'a' => all = true,
                    c if table.iter().any(|t| t.0 == c) => which = c,
                    c => return Err(anyhow!("ulimit: -{}: tuỳ chọn không hợp lệ", c)),
                }
            }
        } else {
            value = Some(*a);
        }
    }
    let get = |res: Resource| -> libc::rlimit {
        let mut rl = libc::rlimit { rlim_cur: 0, rlim_max: 0 };
        unsafe { libc::getrlimit(res, &mut rl) };
        rl
    };
    let show = |v: libc::rlim_t, unit: u64| {
        if v == libc::RLIM_INFINITY { "unlimited".to_string() } else { (v as u64 / unit).to_string() }
    };
    if all {
        for (flag, res, desc, unit) in &table {
            let rl = get(*res);
            println!("{} (-{}) {}", desc, flag, show(if hard { rl.rlim_max } else { rl.rlim_cur }, *unit));
        }
        return Ok(0);
    }
    let (_, res, _, unit) = *table.iter().find(|t| t.0 == which).expect("cờ hợp lệ");
    let mut rl = get(res);
    match value {
        None => println!("{}", show(if hard { rl.rlim_max } else { rl.rlim_cur }, unit)),
        Some(v) => {
            let n: libc::rlim_t = if v == "unlimited" {
                libc::RLIM_INFINITY
            } else {
                v.parse::<u64>().map_err(|_| anyhow!("ulimit: {}: số không hợp lệ", v))?.saturating_mul(unit) as libc::rlim_t
            };
            // Không chỉ định -H/-S → đặt cả hai (như bash).
            if hard || !soft {
                rl.rlim_max = n;
            }
            if soft || !hard {
                rl.rlim_cur = n;
            }
            if unsafe { libc::setrlimit(res, &rl) } != 0 {
                return Err(anyhow!("ulimit: {}", std::io::Error::last_os_error()));
            }
        }
    }
    Ok(0)
}

/// `times` — thời gian CPU (user / sys) của shell và các tiến trình con.
pub fn times() -> Result<i32> {
    let usage = |who| {
        let mut ru: libc::rusage = unsafe { std::mem::zeroed() };
        unsafe { libc::getrusage(who, &mut ru) };
        let t = |tv: libc::timeval| tv.tv_sec as f64 + tv.tv_usec as f64 / 1e6;
        (t(ru.ru_utime), t(ru.ru_stime))
    };
    let fmt = |s: f64| format!("{}m{:.3}s", (s / 60.0) as u64, s % 60.0);
    let (su, ss) = usage(libc::RUSAGE_SELF);
    let (cu, cs) = usage(libc::RUSAGE_CHILDREN);
    println!("{} {}\n{} {}", fmt(su), fmt(ss), fmt(cu), fmt(cs));
    Ok(0)
}

// ── trap ────────────────────────────────────────────────────────────────────

/// Chuẩn hoá tên trap: `EXIT`/`0`, `ERR`, `DEBUG`, `RETURN`, hoặc tên tín hiệu
/// không có tiền tố `SIG` (`INT`, `TERM`…). Trả về (tên, số tín hiệu nếu có).
pub fn trap_name(spec: &str) -> Option<(String, Option<i32>)> {
    use nix::sys::signal::Signal;
    use std::str::FromStr;
    let up = spec.to_ascii_uppercase();
    match up.as_str() {
        "0" | "EXIT" => return Some(("EXIT".into(), None)),
        "ERR" | "DEBUG" | "RETURN" => return Some((up, None)),
        _ => {}
    }
    let sig = match up.parse::<i32>() {
        Ok(n) => Signal::try_from(n).ok()?,
        Err(_) => {
            let full = if up.starts_with("SIG") { up.clone() } else { format!("SIG{}", up) };
            Signal::from_str(&full).ok()?
        }
    };
    if matches!(sig, Signal::SIGKILL | Signal::SIGSTOP) {
        return None;
    }
    Some((sig.as_str().trim_start_matches("SIG").to_string(), Some(sig as i32)))
}

/// `trap [-lp] [[hành_động] tín_hiệu...]`
pub fn trap(shell: &Rc<RefCell<Shell>>, args: &[&str]) -> Result<i32> {
    if args.first() == Some(&"-l") {
        return crate::builtins::kill_list();
    }
    let print_mode = args.first() == Some(&"-p");
    let args: Vec<&str> = args.iter().copied().filter(|a| *a != "-p" && *a != "--").collect();
    if args.is_empty() || print_mode {
        let sh = shell.borrow();
        let wanted: Vec<String> = args.iter().filter_map(|a| trap_name(a).map(|(n, _)| n)).collect();
        for (name, action) in &sh.traps {
            if wanted.is_empty() || wanted.contains(name) {
                // Tín hiệu in kèm `SIG` như bash (`SIGINT`); EXIT/ERR giữ nguyên.
                let shown = match trap_name(name) {
                    Some((_, Some(_))) => format!("SIG{}", name),
                    _ => name.clone(),
                };
                println!("trap -- {} {}", crate::builtins::sq_always(action), shown);
            }
        }
        return Ok(0);
    }
    // `trap SIG` (1 đối số là tên tín hiệu) = trả về mặc định.
    let (action, sigs): (Option<&str>, &[&str]) = if args.len() == 1 || args[0] == "-" {
        (None, if args.len() == 1 { &args[..] } else { &args[1..] })
    } else {
        (Some(args[0]), &args[1..])
    };
    let mut code = 0;
    for spec in sigs {
        let Some((name, signum)) = trap_name(spec) else {
            eprintln!("jaksh: trap: {}: tín hiệu không hợp lệ", spec);
            code = 1;
            continue;
        };
        let mut sh = shell.borrow_mut();
        match action {
            None => {
                sh.traps.remove(&name);
                if let Some(n) = signum {
                    crate::jobctl::set_signal_disposition(n, crate::jobctl::Disposition::Default);
                }
            }
            Some(act) => {
                sh.traps.insert(name, act.to_string());
                if let Some(n) = signum {
                    let d = if act.is_empty() { crate::jobctl::Disposition::Ignore } else { crate::jobctl::Disposition::Trap };
                    crate::jobctl::set_signal_disposition(n, d);
                }
            }
        }
    }
    Ok(code)
}
