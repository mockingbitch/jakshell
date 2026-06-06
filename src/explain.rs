//! `explain <cmd> [args]` — in chú thích chuẩn hoá (usage / tham số / ví dụ)
//! cho các lệnh Unix phổ biến. Một số lệnh tabular còn được "live annotate"
//! từng giá trị trên output thật.
//!
//! Quy ước:
//!   • TÊN CỘT / THUẬT NGỮ giữ nguyên tiếng Anh (PID, owner, inode, MTU…),
//!     phần mô tả là tiếng Việt.
//!   • Lệnh "an toàn" (read-only / harmless) sẽ được chạy sau khi in legend.
//!   • Lệnh "destructive" (rm/mv/chmod/kill…) chỉ in legend, KHÔNG tự chạy.
//!   • `ls -l`, `ps`, `df`, `du`, `free`: ngoài legend còn có live annotation.

use anyhow::Result;
use std::cell::RefCell;
use std::io::Write;
use std::process::Command;
use std::rc::Rc;

use crate::shell::Shell;

// ─── Struct dữ liệu ───────────────────────────────────────────────────────────

struct Explanation {
    name: &'static str,
    summary: &'static str,
    usage: &'static str,
    flags: &'static [(&'static str, &'static str)],
    examples: &'static [(&'static str, &'static str)],
    note: &'static str,
    /// `true` → chỉ in legend, không chạy lệnh (destructive / interactive).
    skip_run: bool,
}

// ─── Entry point ──────────────────────────────────────────────────────────────

pub fn run(shell: &Rc<RefCell<Shell>>, args: &[String]) -> Result<i32> {
    if args.is_empty() || matches!(args[0].as_str(), "list" | "--list" | "-l") {
        list_all();
        return Ok(0);
    }

    let cmd = args[0].as_str();
    let rest: Vec<&str> = args.iter().skip(1).map(|s| s.as_str()).collect();

    let exp = lookup_explanation(cmd, &rest);
    let live = lookup_live_kind(cmd, &rest);

    // 1) In legend (nếu có)
    if let Some(e) = exp {
        print_explanation(e);
    } else {
        eprintln!(
            "\x1b[33m⚠ explain: chưa có chú thích cho `{}`. Gõ `explain list` để xem các lệnh đã có.\x1b[0m",
            cmd
        );
    }

    // 2) Chạy lệnh nếu hợp lý
    match (exp, live) {
        (_, Some(kind)) => {
            // Có live annotator → luôn chạy + annotate
            println!();
            println!("\x1b[2m{}\x1b[0m", crate::i18n::t("explain.output"));
            run_live(shell, cmd, &args[1..], kind)
        }
        (Some(e), None) if !e.skip_run => {
            // Lệnh an toàn không có annotator → chạy bình thường
            println!();
            println!("\x1b[2m{}\x1b[0m", crate::i18n::t("explain.output"));
            run_simple(shell, cmd, &args[1..])
        }
        (Some(e), None) if e.skip_run => {
            // Lệnh destructive / interactive → KHÔNG chạy
            println!();
            println!(
                "\x1b[2m(`{}` — {})\x1b[0m",
                e.name,
                crate::i18n::t("explain.skip_run")
            );
            Ok(0)
        }
        (None, _) => {
            // Không có chú thích — vẫn chạy nguyên si để không cản người dùng
            println!();
            println!("\x1b[2m{}\x1b[0m", crate::i18n::t("explain.output"));
            run_simple(shell, cmd, &args[1..])
        }
        _ => Ok(0),
    }
}

// ─── Lookup ───────────────────────────────────────────────────────────────────

fn lookup_explanation(cmd: &str, rest: &[&str]) -> Option<&'static Explanation> {
    if cmd == "docker" {
        let sub = rest.iter().find(|a| !a.starts_with('-')).copied();
        return match sub {
            Some("ps") => Some(&DOCKER_PS),
            Some("exec") => Some(&DOCKER_EXEC),
            Some("run") => Some(&DOCKER_RUN),
            Some("build") => Some(&DOCKER_BUILD),
            Some("images") | Some("image") => Some(&DOCKER_IMAGES),
            Some("pull") => Some(&DOCKER_PULL),
            Some("push") => Some(&DOCKER_PUSH),
            Some("logs") => Some(&DOCKER_LOGS),
            Some("stop") | Some("start") | Some("restart") | Some("kill") | Some("pause") | Some("unpause") => Some(&DOCKER_LIFECYCLE),
            Some("rm") => Some(&DOCKER_RM),
            Some("rmi") => Some(&DOCKER_RMI),
            Some("inspect") => Some(&DOCKER_INSPECT),
            Some("network") => Some(&DOCKER_NETWORK),
            Some("volume") => Some(&DOCKER_VOLUME),
            Some("compose") => Some(&DOCKER_COMPOSE),
            Some("cp") => Some(&DOCKER_CP),
            Some("login") | Some("logout") => Some(&DOCKER_LOGIN),
            Some("system") | Some("prune") => Some(&DOCKER_SYSTEM),
            Some("tag") => Some(&DOCKER_TAG),
            _ => Some(&DOCKER),
        };
    }

    if cmd == "git" {
        let sub = rest.iter().find(|a| !a.starts_with('-')).copied();
        return match sub {
            Some("status") => Some(&GIT_STATUS),
            Some("log") => Some(&GIT_LOG),
            Some("diff") => Some(&GIT_DIFF),
            Some("branch") => Some(&GIT_BRANCH),
            Some("clone") => Some(&GIT_CLONE),
            Some("init") => Some(&GIT_INIT),
            Some("add") => Some(&GIT_ADD),
            Some("commit") => Some(&GIT_COMMIT),
            Some("push") => Some(&GIT_PUSH),
            Some("pull") => Some(&GIT_PULL),
            Some("fetch") => Some(&GIT_FETCH),
            Some("merge") => Some(&GIT_MERGE),
            Some("rebase") => Some(&GIT_REBASE),
            Some("reset") => Some(&GIT_RESET),
            Some("restore") => Some(&GIT_RESTORE),
            Some("revert") => Some(&GIT_REVERT),
            Some("stash") => Some(&GIT_STASH),
            Some("tag") => Some(&GIT_TAG),
            Some("remote") => Some(&GIT_REMOTE),
            Some("checkout") => Some(&GIT_CHECKOUT),
            Some("switch") => Some(&GIT_SWITCH),
            Some("cherry-pick") => Some(&GIT_CHERRY_PICK),
            Some("blame") => Some(&GIT_BLAME),
            Some("show") => Some(&GIT_SHOW),
            Some("reflog") => Some(&GIT_REFLOG),
            Some("config") => Some(&GIT_CONFIG),
            _ => Some(&GIT),
        };
    }
    match cmd {
        // navigation
        "cd" => Some(&CD),
        "pwd" => Some(&PWD),
        "ls" => Some(&LS),
        "find" => Some(&FIND),
        // file mgmt
        "cp" => Some(&CP),
        "mv" => Some(&MV),
        "rm" => Some(&RM),
        "mkdir" => Some(&MKDIR),
        "rmdir" => Some(&RMDIR),
        "touch" => Some(&TOUCH),
        "ln" => Some(&LN),
        "chmod" => Some(&CHMOD),
        "chown" => Some(&CHOWN),
        // viewing
        "cat" => Some(&CAT),
        "less" => Some(&LESS),
        "head" => Some(&HEAD),
        "tail" => Some(&TAIL),
        "echo" => Some(&ECHO),
        // search & filter
        "grep" => Some(&GREP),
        "sort" => Some(&SORT),
        "uniq" => Some(&UNIQ),
        "wc" => Some(&WC),
        "cut" => Some(&CUT),
        "tr" => Some(&TR),
        "xargs" => Some(&XARGS),
        // process
        "ps" => Some(&PS),
        "top" | "htop" => Some(&TOP),
        "kill" => Some(&KILL),
        "pkill" => Some(&PKILL),
        "killall" => Some(&KILLALL),
        // disk
        "df" => Some(&DF),
        "du" => Some(&DU),
        "free" => Some(&FREE),
        "stat" => Some(&STAT),
        "lsof" => Some(&LSOF),
        // network
        "ssh" => Some(&SSH),
        "ssh-keygen" => Some(&SSH_KEYGEN),
        "ssh-copy-id" => Some(&SSH_COPY_ID),
        "ssh-add" => Some(&SSH_ADD),
        "sftp" => Some(&SFTP),
        "scp" => Some(&SCP),
        "curl" => Some(&CURL),
        "wget" => Some(&WGET),
        "ping" => Some(&PING),
        "netstat" => Some(&NETSTAT),
        "ss" => Some(&SS),
        "ifconfig" => Some(&IFCONFIG),
        "ip" => Some(&IP),
        // archive
        "tar" => Some(&TAR),
        "zip" => Some(&ZIP),
        "unzip" => Some(&UNZIP),
        // system info
        "uptime" => Some(&UPTIME),
        "who" | "w" => Some(&WHO),
        "date" => Some(&DATE),
        "env" => Some(&ENV),
        "alias" => Some(&ALIAS),
        "history" => Some(&HISTORY),
        "which" => Some(&WHICH),
        "man" => Some(&MAN),
        _ => None,
    }
}

#[derive(Clone, Copy)]
enum LiveKind {
    LsLong,
    Ps,
    Df,
    Du,
    Free,
}

fn lookup_live_kind(cmd: &str, rest: &[&str]) -> Option<LiveKind> {
    let flag_chars: String = rest
        .iter()
        .filter(|s| s.starts_with('-'))
        .copied()
        .collect::<Vec<_>>()
        .join("");
    match cmd {
        "ls" if flag_chars.contains('l') => Some(LiveKind::LsLong),
        "ps" => Some(LiveKind::Ps),
        "df" => Some(LiveKind::Df),
        "du" => Some(LiveKind::Du),
        "free" => Some(LiveKind::Free),
        _ => None,
    }
}

// ─── Render ───────────────────────────────────────────────────────────────────

fn normalize_name(name: &str) -> String {
    name.chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '_' })
        .collect()
}

fn print_explanation(e: &Explanation) {
    // Tra summary đã dịch nếu có (key: "explain.<name>.summary").
    // Normalize tên: thay mọi ký tự không phải alphanumeric/'-' bằng '_'.
    // Nhờ vậy "docker stop / start / ..." → "docker_stop___start___..."
    let key = format!("explain.{}.summary", normalize_name(e.name));
    let translated = crate::i18n::t(&key);
    let summary = if translated.is_empty() { e.summary } else { translated };

    println!(
        "\x1b[1m{}\x1b[0m \x1b[2m— {}\x1b[0m",
        e.name, summary
    );
    println!();
    println!("\x1b[36m{}\x1b[0m  \x1b[33m{}\x1b[0m", crate::i18n::t("explain.syntax"), e.usage);

    let safe_name = normalize_name(e.name);

    if !e.flags.is_empty() {
        println!();
        println!("\x1b[36m{}\x1b[0m", crate::i18n::t("explain.flags"));
        let w = e.flags.iter().map(|(f, _)| f.len()).max().unwrap_or(0).min(20);
        for (i, (f, d)) in e.flags.iter().enumerate() {
            let key = format!("explain.{}.flag.{}", safe_name, i);
            let tr = crate::i18n::t(&key);
            let desc = if tr.is_empty() { *d } else { tr };
            println!("  \x1b[33m{:<w$}\x1b[0m  {}", f, desc, w = w);
        }
    }

    if !e.examples.is_empty() {
        println!();
        println!("\x1b[36m{}\x1b[0m", crate::i18n::t("explain.examples"));
        for (i, (cmd, what)) in e.examples.iter().enumerate() {
            println!("  \x1b[2m$\x1b[0m \x1b[1m{}\x1b[0m", cmd);
            let key = format!("explain.{}.example.{}", safe_name, i);
            let tr = crate::i18n::t(&key);
            let desc = if tr.is_empty() { *what } else { tr };
            if !desc.is_empty() {
                println!("    \x1b[2m{}\x1b[0m", desc);
            }
        }
    }

    let note_key = format!("explain.{}.note", safe_name);
    let translated_note = crate::i18n::t(&note_key);
    let note = if translated_note.is_empty() { e.note } else { translated_note };
    if !note.is_empty() {
        println!();
        println!("\x1b[36m{}\x1b[0m", crate::i18n::t("explain.notes"));
        for line in note.lines() {
            println!("  {}", line);
        }
    }
}

// ─── Run helpers ──────────────────────────────────────────────────────────────

fn run_simple(shell: &Rc<RefCell<Shell>>, cmd: &str, args: &[String]) -> Result<i32> {
    let mut builder = Command::new(cmd);
    builder.args(args);
    builder.current_dir(&shell.borrow().cwd);
    match builder.status() {
        Ok(s) => Ok(s.code().unwrap_or(1)),
        Err(e) => {
            if e.kind() == std::io::ErrorKind::NotFound {
                eprintln!("explain: không tìm thấy lệnh: {}", cmd);
                Ok(127)
            } else {
                eprintln!("explain: không chạy được {}: {}", cmd, e);
                Ok(126)
            }
        }
    }
}

fn run_live(
    shell: &Rc<RefCell<Shell>>,
    cmd: &str,
    args: &[String],
    kind: LiveKind,
) -> Result<i32> {
    let mut builder = Command::new(cmd);
    builder.args(args);
    builder.current_dir(&shell.borrow().cwd);
    let output = match builder.output() {
        Ok(o) => o,
        Err(e) => {
            if e.kind() == std::io::ErrorKind::NotFound {
                eprintln!("explain: không tìm thấy lệnh: {}", cmd);
                return Ok(127);
            }
            eprintln!("explain: không chạy được {}: {}", cmd, e);
            return Ok(126);
        }
    };

    std::io::stdout().write_all(&output.stdout).ok();
    std::io::stderr().write_all(&output.stderr).ok();

    let stdout_text = String::from_utf8_lossy(&output.stdout);
    if !stdout_text.trim().is_empty() {
        println!();
        println!("\x1b[2m{}\x1b[0m", crate::i18n::t("explain.live_annotation"));
        match kind {
            LiveKind::LsLong => annotate_ls_long(&stdout_text),
            LiveKind::Ps => annotate_ps(&stdout_text),
            LiveKind::Df => annotate_df(&stdout_text),
            LiveKind::Du => annotate_du(&stdout_text),
            LiveKind::Free => annotate_free(&stdout_text),
        }
    }

    Ok(output.status.code().unwrap_or(0))
}

// ─── list_all ─────────────────────────────────────────────────────────────────

fn list_all() {
    crate::info::print_banner();
    let t = crate::i18n::t;
    println!("\x1b[1m{}\x1b[0m\n", t("explain.list_title"));
    let groups: &[(&str, &[&str])] = &[
        (t("explain.group.nav"),
            &["cd", "pwd", "ls", "find"]),
        (t("explain.group.file_mgmt"),
            &["cp", "mv", "rm", "mkdir", "rmdir", "touch", "ln", "chmod", "chown"]),
        (t("explain.group.view"),
            &["cat", "less", "head", "tail", "echo"]),
        (t("explain.group.filter"),
            &["grep", "sort", "uniq", "wc", "cut", "tr", "xargs"]),
        (t("explain.group.process"),
            &["ps", "top", "kill", "pkill", "killall"]),
        (t("explain.group.disk"),
            &["df", "du", "free", "stat", "lsof"]),
        (t("explain.group.network"),
            &["ssh", "ssh-keygen", "ssh-copy-id", "ssh-add", "sftp", "scp",
              "curl", "wget", "ping", "netstat", "ss", "ifconfig", "ip"]),
        (t("explain.group.docker"),
            &["docker", "docker ps", "docker exec", "docker run", "docker build",
              "docker images", "docker pull", "docker push", "docker logs",
              "docker stop/start/restart/kill", "docker rm", "docker rmi",
              "docker inspect", "docker network", "docker volume", "docker compose",
              "docker cp", "docker login", "docker system", "docker tag"]),
        (t("explain.group.archive"),
            &["tar", "zip", "unzip"]),
        (t("explain.group.system"),
            &["uptime", "who", "date", "env", "alias", "history", "which", "man"]),
        (t("explain.group.git_basic"),
            &["git", "git status", "git log", "git diff", "git branch", "git config"]),
        (t("explain.group.git_file"),
            &["git add", "git commit", "git restore", "git reset", "git revert", "git stash", "git show", "git blame", "git reflog"]),
        (t("explain.group.git_remote"),
            &["git clone", "git init", "git checkout", "git switch", "git merge", "git rebase", "git cherry-pick", "git tag", "git remote", "git fetch", "git pull", "git push"]),
    ];
    for (group, cmds) in groups {
        println!("\x1b[2m▸ {}:\x1b[0m", group);
        let line = cmds.iter().map(|c| format!("\x1b[36m{}\x1b[0m", c)).collect::<Vec<_>>().join(", ");
        println!("  {}", line);
        println!();
    }
    println!("{} \x1b[36mexplain <cmd> [args]\x1b[0m", t("explain.use_hint"));
    println!("{} \x1b[36mls -l, ps, df, du, free\x1b[0m", t("explain.live_label"));
}

// ─── Live annotators ──────────────────────────────────────────────────────────

fn pair(label: &str, value: &str, desc: &str) {
    println!(
        "  \x1b[36m{:<14}\x1b[0m = \x1b[1m{:<22}\x1b[0m \x1b[2m({})\x1b[0m",
        label, value, desc
    );
}

fn bullet(text: &str) {
    println!("    \x1b[2m{}\x1b[0m", text);
}

fn annotate_ls_long(out: &str) {
    let mut lines = out.lines();
    let first = lines.next();
    let (total_line, data_line_opt) = match first {
        Some(l) if l.starts_with("total ") => (Some(l), lines.next()),
        Some(l) => (None, Some(l)),
        None => (None, None),
    };

    if let Some(t) = total_line {
        let n = t.trim_start_matches("total ").trim();
        println!("\x1b[1m{}\x1b[0m", t);
        let kb = n.parse::<u64>().map(|v| v / 2).unwrap_or(0);
        println!(
            "    \x1b[2m→ tổng số block ổ đĩa thư mục này chiếm. {} block × \
             (512B trên macOS, 1KB trên Linux) ≈ {} KB (macOS) hoặc {} KB (Linux).\x1b[0m",
            n, kb, n
        );
        println!();
    }

    let data_line = match data_line_opt {
        Some(l) => l,
        None => return,
    };

    let toks: Vec<&str> = data_line.split_whitespace().collect();
    if toks.len() < 8 {
        return;
    }
    let perms = toks[0];
    let links = toks[1];
    let owner = toks[2];
    let group = toks[3];
    let size = toks[4];
    let date = format!("{} {} {}", toks[5], toks[6], toks[7]);
    let name = if toks.len() > 8 { toks[8..].join(" ") } else { String::new() };

    println!("\x1b[1mDòng đầu được giải thích:\x1b[0m");
    println!("  \x1b[2m{}\x1b[0m\n", data_line);

    pair("permissions", perms, "loại + quyền owner/group/others");
    if perms.len() >= 10 {
        let typ = match &perms[..1] {
            "d" => "directory (thư mục)",
            "-" => "file thường",
            "l" => "symlink (liên kết mềm)",
            "c" => "character device",
            "b" => "block device",
            "s" => "socket",
            "p" => "named pipe (FIFO)",
            _ => "loại khác",
        };
        bullet(&format!("[{}] ký tự 1 → {}", &perms[..1], typ));
        bullet(&format!("[{}] ký tự 2-4 → owner: {}", &perms[1..4], decode_rwx(&perms[1..4])));
        bullet(&format!("[{}] ký tự 5-7 → group: {}", &perms[4..7], decode_rwx(&perms[4..7])));
        bullet(&format!("[{}] ký tự 8-10 → others: {}", &perms[7..10], decode_rwx(&perms[7..10])));
        if perms.len() > 10 {
            let ext = &perms[10..];
            let ext_desc = if ext.contains('@') {
                "có extended attributes (xattr) — macOS"
            } else if ext.contains('+') {
                "có ACL (Access Control List) bổ sung"
            } else if ext.contains('.') {
                "có SELinux context (Linux)"
            } else {
                "chỉ báo bổ sung"
            };
            bullet(&format!("[{}] {}", ext, ext_desc));
        }
    }
    pair("links", links, "số hard link tới inode");
    pair("owner", owner, "user sở hữu");
    pair("group", group, "group sở hữu");
    pair("size", size, "bytes — thêm cờ -h để xem KB/MB/GB");
    pair("mtime", &date, "lần thay đổi nội dung gần nhất");
    pair("name", &name, "tên file/thư mục (→ trỏ tới đích nếu là symlink)");
}

fn decode_rwx(s: &str) -> String {
    if s.len() < 3 {
        return s.to_string();
    }
    let b = s.as_bytes();
    let r = b[0] as char;
    let w = b[1] as char;
    let x = b[2] as char;

    let mut perms = Vec::new();
    if r == 'r' { perms.push("read"); }
    if w == 'w' { perms.push("write"); }
    match x {
        'x' => perms.push("execute"),
        's' => perms.push("execute + setuid/setgid"),
        'S' => perms.push("setuid/setgid (không execute)"),
        't' => perms.push("execute + sticky bit"),
        'T' => perms.push("sticky bit (không execute)"),
        _ => {}
    }
    if perms.is_empty() {
        "không có quyền".to_string()
    } else {
        perms.join(" + ")
    }
}

fn annotate_ps(out: &str) {
    let mut lines = out.lines();
    let header = match lines.next() { Some(h) => h, None => return };
    let first = lines.next();
    let cols: Vec<&str> = header.split_whitespace().collect();

    println!("\x1b[1mCác cột (header) gắn với giá trị dòng đầu:\x1b[0m");
    if let Some(row) = first {
        let vals = align_values(row, cols.len());
        for (i, c) in cols.iter().enumerate() {
            let v = vals.get(i).map(|s| s.as_str()).unwrap_or("-");
            pair(c, v, ps_col_desc(c));
        }
    } else {
        for c in &cols { pair(c, "", ps_col_desc(c)); }
    }
    println!();
    println!("\x1b[2mGhi chú STAT:\x1b[0m");
    bullet("R=running  S=sleeping  D=disk-wait  T=stopped  Z=zombie");
    bullet("+ foreground   s session-leader   l multi-thread   < high-priority   N low-priority");
}

fn ps_col_desc(c: &str) -> &'static str {
    match c {
        "USER" | "UID" => "user chủ process",
        "PID" => "process ID (số định danh duy nhất)",
        "PPID" => "parent process ID",
        "%CPU" => "phần trăm CPU đang dùng",
        "%MEM" => "phần trăm RAM đang dùng",
        "VSZ" | "VSIZE" => "virtual size (KB)",
        "RSS" => "resident set size (KB)",
        "TTY" | "TT" => "terminal gắn với process (? = không có)",
        "STAT" | "S" | "STATE" => "trạng thái (R/S/D/T/Z + cờ phụ)",
        "STARTED" | "STIME" | "START" => "thời điểm khởi chạy",
        "TIME" => "tổng CPU time đã tiêu thụ",
        "COMMAND" | "CMD" => "dòng lệnh đang chạy",
        "PRI" | "PR" => "priority (số nhỏ = ưu tiên cao)",
        "NI" => "nice (-20 → 19; thấp = ưu tiên cao)",
        "WCHAN" => "kernel function nơi process đang ngủ",
        _ => "—",
    }
}

fn align_values(row: &str, n: usize) -> Vec<String> {
    if n == 0 { return Vec::new(); }
    let toks: Vec<&str> = row.split_whitespace().collect();
    if toks.len() <= n {
        return toks.iter().map(|s| s.to_string()).collect();
    }
    let mut out: Vec<String> = toks.iter().take(n - 1).map(|s| s.to_string()).collect();
    out.push(toks[(n - 1)..].join(" "));
    out
}

fn annotate_df(out: &str) {
    let mut lines = out.lines();
    let header = match lines.next() { Some(h) => h, None => return };
    let first = lines.next();
    let cols = df_header_cols(header);
    println!("\x1b[1mCác cột (giá trị từ dòng đầu):\x1b[0m");
    if let Some(row) = first {
        let vals = align_values(row, cols.len());
        for (i, c) in cols.iter().enumerate() {
            let v = vals.get(i).map(|s| s.as_str()).unwrap_or("-");
            pair(c, v, df_col_desc(c));
        }
    }
}

fn df_header_cols(header: &str) -> Vec<String> {
    let toks: Vec<&str> = header.split_whitespace().collect();
    let mut out: Vec<String> = toks.iter().map(|s| s.to_string()).collect();
    if out.len() >= 2 && out[out.len() - 2] == "Mounted" && out[out.len() - 1] == "on" {
        let on = out.pop().unwrap();
        let mounted = out.pop().unwrap();
        out.push(format!("{} {}", mounted, on));
    }
    out
}

fn df_col_desc(c: &str) -> &'static str {
    match c {
        "Filesystem" => "tên device / nguồn mount",
        "Size" | "1K-blocks" => "tổng dung lượng",
        "Used" => "đã dùng",
        "Avail" | "Available" => "còn trống (sau trừ reserve cho root)",
        "Use%" | "Capacity" => "phần trăm đã dùng",
        "iused" => "số inode đã dùng",
        "ifree" => "số inode còn trống",
        "%iused" => "phần trăm inode đã dùng",
        "Mounted on" => "thư mục đang được mount",
        _ => "—",
    }
}

fn annotate_du(out: &str) {
    let first = match out.lines().next() { Some(l) => l, None => return };
    let toks: Vec<&str> = first.splitn(2, char::is_whitespace).collect();
    println!("\x1b[1mDòng đầu:\x1b[0m \x1b[2m{}\x1b[0m\n", first);
    if toks.len() >= 2 {
        pair("size", toks[0].trim(), "dung lượng trên ĐĨA (block-allocated)");
        pair("path", toks[1].trim(), "đường dẫn được đo");
        println!();
        bullet("macOS mặc định block 512B → chia 2 để ra KB; Linux: 1K-blocks");
        bullet("Cờ hay dùng: -s (chỉ tổng), -h (human-readable), -d 1 (sâu 1 cấp)");
    }
}

fn annotate_free(out: &str) {
    let mut lines = out.lines();
    let header = match lines.next() { Some(h) => h, None => return };
    let mem = match lines.next() { Some(m) => m, None => return };
    let swap = lines.next();
    let cols: Vec<&str> = header.split_whitespace().collect();
    let mem_toks: Vec<&str> = mem.split_whitespace().collect();

    println!("\x1b[1mDòng Mem:\x1b[0m");
    for (i, c) in cols.iter().enumerate() {
        let v = mem_toks.get(i + 1).copied().unwrap_or("-");
        pair(c, v, free_col_desc(c));
    }
    if let Some(swap_line) = swap {
        println!("\n\x1b[1mDòng Swap:\x1b[0m");
        let st: Vec<&str> = swap_line.split_whitespace().collect();
        for (i, n) in ["total", "used", "free"].iter().enumerate() {
            if let Some(v) = st.get(i + 1) {
                pair(n, v, "vùng swap trên đĩa — chậm hơn RAM rất nhiều");
            }
        }
    }
    println!();
    bullet("Quan trọng: \x1b[1mavailable\x1b[0m mới là số RAM còn dùng được thật,");
    bullet("không phải \x1b[1mfree\x1b[0m (vì kernel chủ động dùng RAM rảnh làm cache).");
}

fn free_col_desc(c: &str) -> &'static str {
    match c {
        "total" => "tổng RAM",
        "used" => "đã dùng (không tính buff/cache)",
        "free" => "hoàn toàn rảnh (thường rất nhỏ — bình thường)",
        "shared" => "tmpfs + shared memory",
        "buff/cache" | "buffers" | "cache" => "đệm kernel — sẽ tự nhả khi cần",
        "available" => "RAM có thể cấp phát ngay",
        _ => "—",
    }
}

// ─── Dữ liệu Explanation ─────────────────────────────────────────────────────

// Navigation & info
const CD: Explanation = Explanation {
    name: "cd",
    summary: "Đổi thư mục làm việc hiện tại",
    usage: "cd [thư_mục]",
    flags: &[
        ("(không tham số)", "về HOME (~)"),
        ("-", "quay lại thư mục trước đó (OLDPWD)"),
        ("..", "lên thư mục cha"),
    ],
    examples: &[
        ("cd", "về HOME"),
        ("cd /etc", "đến /etc"),
        ("cd ..", "lên 1 cấp"),
        ("cd -", "quay lại thư mục trước"),
        ("cd ~/Desktop/JakShell", "tilde = HOME"),
    ],
    note: "`cd` là builtin của shell — không phải binary trên PATH.",
    skip_run: true,
};

const PWD: Explanation = Explanation {
    name: "pwd",
    summary: "In thư mục làm việc hiện tại",
    usage: "pwd [-LP]",
    flags: &[
        ("-L", "in path logic (default) — giữ nguyên symlink trong đường dẫn"),
        ("-P", "in path vật lý — resolve hết symlink"),
    ],
    examples: &[
        ("pwd", "in thư mục hiện tại"),
        ("pwd -P", "in path đã resolve symlink"),
    ],
    note: "",
    skip_run: false,
};

const LS: Explanation = Explanation {
    name: "ls",
    summary: "Liệt kê file và thư mục",
    usage: "ls [-lahStr1RA] [đường_dẫn ...]",
    flags: &[
        ("-l", "định dạng dài (permissions / owner / size / mtime)"),
        ("-a", "hiện cả file ẩn (bắt đầu bằng .)"),
        ("-A", "như -a nhưng bỏ . và .."),
        ("-h", "size dạng KB/MB/GB (cần đi với -l)"),
        ("-S", "sort theo size"),
        ("-t", "sort theo mtime (mới nhất trước)"),
        ("-r", "đảo chiều sort"),
        ("-R", "đệ quy vào thư mục con"),
        ("-1", "mỗi file một dòng"),
        ("-d", "thông tin về chính thư mục, không đi vào trong"),
    ],
    examples: &[
        ("ls", "liệt kê thư mục hiện tại"),
        ("ls -la", "tất cả file (cả ẩn), format chi tiết"),
        ("ls -lhSr", "sort theo size lớn → nhỏ, human-readable"),
        ("ls -lt | head", "10 file sửa gần đây nhất"),
    ],
    note: "Thêm cờ `--jak` để JakShell tô màu và decode permissions.",
    skip_run: false,
};

const FIND: Explanation = Explanation {
    name: "find",
    summary: "Tìm file/thư mục theo điều kiện (POSIX)",
    usage: "find <path> [biểu_thức]",
    flags: &[
        ("-name PATTERN", "khớp tên (case-sensitive). Dùng quote: `-name \"*.rs\"`"),
        ("-iname PATTERN", "khớp tên không phân biệt hoa thường"),
        ("-type f|d|l", "loại: file / directory / symlink"),
        ("-size +N|-N", "kích thước (k/M/G). +10M = lớn hơn 10MB"),
        ("-mtime +N|-N", "mtime cách đây N ngày. -1 = trong 1 ngày qua"),
        ("-mmin N", "mtime cách đây N phút"),
        ("-maxdepth N", "không đi sâu quá N cấp"),
        ("-exec CMD {} \\;", "chạy lệnh trên mỗi kết quả"),
        ("-delete", "xoá kết quả khớp (cẩn thận!)"),
    ],
    examples: &[
        ("find . -name \"*.rs\"", "mọi file .rs trong cwd (và con)"),
        ("find /var/log -size +100M", "log lớn hơn 100MB"),
        ("find . -mtime -1 -type f", "file sửa trong 24h gần đây"),
        ("find . -name '*.tmp' -delete", "xoá tất cả .tmp"),
    ],
    note: "JakShell có sẵn lệnh `jak find` thân thiện hơn cho thao tác thường gặp.",
    skip_run: true, // tránh chạy nhầm `find /` quá chậm
};

// File mgmt
const CP: Explanation = Explanation {
    name: "cp",
    summary: "Sao chép file/thư mục",
    usage: "cp [-rvfi] <nguồn ...> <đích>",
    flags: &[
        ("-r / -R", "đệ quy (cho thư mục)"),
        ("-v", "verbose — in mỗi file"),
        ("-f", "force — ghi đè không hỏi"),
        ("-i", "interactive — hỏi trước khi ghi đè"),
        ("-n", "no-clobber — không ghi đè"),
        ("-a", "archive — giữ nguyên metadata, đệ quy, symlink"),
        ("-p", "preserve — giữ permissions/mtime/owner"),
        ("-u", "update — chỉ sao nếu nguồn mới hơn đích"),
    ],
    examples: &[
        ("cp a.txt b.txt", "sao a.txt → b.txt"),
        ("cp -r src/ backup/", "sao cả thư mục"),
        ("cp -av src/ /backup/", "archive mode, có log"),
        ("cp -i *.txt /tmp/", "có hỏi trước khi ghi đè"),
    ],
    note: "Nếu đích kết thúc bằng `/` thì là thư mục đích. Nếu không, đó là tên mới.",
    skip_run: true,
};

const MV: Explanation = Explanation {
    name: "mv",
    summary: "Di chuyển hoặc đổi tên file/thư mục",
    usage: "mv [-vfin] <nguồn ...> <đích>",
    flags: &[
        ("-v", "verbose"),
        ("-f", "force — ghi đè không hỏi (default trên nhiều hệ)"),
        ("-i", "interactive — hỏi trước khi ghi đè"),
        ("-n", "no-clobber"),
    ],
    examples: &[
        ("mv old.txt new.txt", "đổi tên"),
        ("mv *.log /var/log/", "chuyển nhiều file vào thư mục"),
        ("mv folder1 folder2", "nếu folder2 tồn tại → chuyển folder1 vào trong; nếu không → đổi tên"),
    ],
    note: "`mv` không thực sự copy — chỉ đổi entry trong cùng filesystem. Khác filesystem thì nó copy + delete.",
    skip_run: true,
};

const RM: Explanation = Explanation {
    name: "rm",
    summary: "Xoá file/thư mục (KHÔNG có Trash — mất là mất luôn!)",
    usage: "rm [-rfvi] <file ...>",
    flags: &[
        ("-r / -R", "đệ quy (bắt buộc cho thư mục)"),
        ("-f", "force — không hỏi, bỏ qua file không tồn tại"),
        ("-i", "interactive — hỏi từng file"),
        ("-v", "verbose"),
        ("-d", "xoá thư mục rỗng (không cần -r)"),
    ],
    examples: &[
        ("rm file.txt", "xoá 1 file"),
        ("rm -r build/", "xoá cả thư mục build"),
        ("rm -rf node_modules/", "xoá thẳng, không hỏi"),
        ("rm -i *.bak", "hỏi từng file .bak"),
    ],
    note: "⚠ `rm -rf /` hoặc `rm -rf /*` có thể xoá toàn bộ hệ thống nếu chạy quyền cao. \nKhi không chắc, dùng `rm -i` hoặc chuyển vào Trash bằng tool khác (`trash` trên macOS).",
    skip_run: true,
};

const MKDIR: Explanation = Explanation {
    name: "mkdir",
    summary: "Tạo thư mục",
    usage: "mkdir [-pv] [-m MODE] <thư_mục ...>",
    flags: &[
        ("-p", "tạo cả cây cha nếu chưa có; không lỗi nếu đã tồn tại"),
        ("-v", "verbose — báo mỗi thư mục tạo"),
        ("-m MODE", "đặt permission ngay khi tạo (vd 755)"),
    ],
    examples: &[
        ("mkdir new", "tạo 1 thư mục"),
        ("mkdir -p a/b/c", "tạo cả a, a/b, a/b/c"),
        ("mkdir -m 700 secret", "tạo với quyền 700"),
        ("mkdir {jan,feb,mar}", "tạo 3 thư mục bằng brace expansion (cần shell hỗ trợ)"),
    ],
    note: "",
    skip_run: true,
};

const RMDIR: Explanation = Explanation {
    name: "rmdir",
    summary: "Xoá thư mục RỖNG (không xoá được nếu còn nội dung)",
    usage: "rmdir [-p] <thư_mục ...>",
    flags: &[
        ("-p", "xoá luôn các cha rỗng (vd `rmdir -p a/b/c` xoá c, rồi b, rồi a nếu trống)"),
    ],
    examples: &[
        ("rmdir empty/", "xoá thư mục empty (phải rỗng)"),
        ("rmdir -p a/b/c", "xoá c, b, a (nếu mỗi cấp đều rỗng)"),
    ],
    note: "Muốn xoá kèm nội dung: dùng `rm -r`.",
    skip_run: true,
};

const TOUCH: Explanation = Explanation {
    name: "touch",
    summary: "Tạo file rỗng, hoặc cập nhật atime/mtime",
    usage: "touch [-acm] [-t YYYYMMDDhhmm] [-d DATE] <file ...>",
    flags: &[
        ("-a", "chỉ cập nhật atime"),
        ("-m", "chỉ cập nhật mtime"),
        ("-c", "không tạo file mới (chỉ update nếu đã có)"),
        ("-t STAMP", "đặt time cụ thể: [[CC]YY]MMDDhhmm[.SS]"),
        ("-d STRING", "đặt time bằng chuỗi dễ đọc"),
        ("-r FILE", "lấy time từ FILE khác"),
    ],
    examples: &[
        ("touch new.txt", "tạo file rỗng (nếu chưa có)"),
        ("touch -d '2025-01-01' a.log", "đặt mtime về 2025-01-01"),
        ("touch -r src.txt dst.txt", "copy mtime từ src.txt sang dst.txt"),
    ],
    note: "",
    skip_run: true,
};

const LN: Explanation = Explanation {
    name: "ln",
    summary: "Tạo link (cứng hoặc symlink)",
    usage: "ln [-sfv] <đích> <tên_link>",
    flags: &[
        ("-s", "symbolic link (KHUYẾN NGHỊ — flexible, có thể trỏ cross-filesystem)"),
        ("-f", "force — ghi đè link cũ nếu có"),
        ("-v", "verbose"),
        ("-n", "không follow đích nếu đã là symlink trỏ tới thư mục"),
    ],
    examples: &[
        ("ln -s /usr/local/bin/jaksh ~/bin/jak", "tạo shortcut jak → jaksh"),
        ("ln -sf new old", "thay symlink cũ bằng symlink mới"),
        ("ln a.txt b.txt", "hard link — b.txt và a.txt cùng inode"),
    ],
    note: "Hard link: cùng inode, đếm vào link count. Symlink: file con trỏ tới đường dẫn (có thể vỡ).",
    skip_run: true,
};

const CHMOD: Explanation = Explanation {
    name: "chmod",
    summary: "Đổi permission file/thư mục",
    usage: "chmod [-R] <mode> <file ...>",
    flags: &[
        ("-R", "đệ quy (cho thư mục và file con)"),
        ("-v", "verbose"),
        ("u/g/o/a", "đối tượng: user(owner)/group/others/all"),
        ("+/-/=", "thêm / bớt / gán"),
        ("r/w/x", "read(4) / write(2) / execute(1)"),
    ],
    examples: &[
        ("chmod 755 script.sh", "owner rwx, group r-x, others r-x"),
        ("chmod 644 file.txt", "owner rw-, mọi người r--"),
        ("chmod +x build.sh", "thêm execute cho mọi đối tượng"),
        ("chmod -R go-w secret/", "bỏ write cho group + others, đệ quy"),
        ("chmod u=rwx,go= private", "owner rwx, group/others không có gì"),
    ],
    note: "Quy đổi nhanh: 7=rwx 6=rw- 5=r-x 4=r-- 0=---\nThường dùng: 755 (script/dir), 644 (file thường), 600 (ssh key).",
    skip_run: true,
};

const CHOWN: Explanation = Explanation {
    name: "chown",
    summary: "Đổi owner / group của file",
    usage: "chown [-R] <user>[:<group>] <file ...>",
    flags: &[
        ("-R", "đệ quy"),
        ("-v", "verbose"),
        (":group", "chỉ đổi group"),
        ("user:", "đổi owner, group thành primary group của user mới"),
    ],
    examples: &[
        ("sudo chown alice file.txt", "đổi owner sang alice"),
        ("sudo chown alice:devs file.txt", "owner=alice, group=devs"),
        ("sudo chown -R www-data:www-data /var/www", "đệ quy"),
    ],
    note: "Thường cần `sudo`. Group-only: `chgrp` hoặc `chown :group`.",
    skip_run: true,
};

// Viewing
const CAT: Explanation = Explanation {
    name: "cat",
    summary: "In nội dung file (concatenate)",
    usage: "cat [-nbsE] <file ...>",
    flags: &[
        ("-n", "đánh số tất cả dòng"),
        ("-b", "đánh số dòng không trống"),
        ("-s", "gộp các dòng trống liên tiếp"),
        ("-E", "hiện $ ở cuối mỗi dòng"),
        ("-T", "hiện tab thành ^I"),
        ("-A", "= -vET (hiện mọi ký tự đặc biệt)"),
    ],
    examples: &[
        ("cat file.txt", "in file"),
        ("cat a.txt b.txt > both.txt", "ghép 2 file thành 1"),
        ("cat -n script.sh", "in kèm số dòng"),
        ("cat << EOF\\nxin chào\\nEOF", "heredoc"),
    ],
    note: "File lớn → dùng `less` thay vì `cat` (cat in tuột không scrollback).",
    skip_run: false,
};

const LESS: Explanation = Explanation {
    name: "less",
    summary: "Xem file lớn theo trang (scroll lên/xuống)",
    usage: "less [-N] <file>",
    flags: &[
        ("-N", "hiện số dòng"),
        ("-S", "không wrap dòng dài"),
        ("-R", "giữ màu ANSI"),
        ("-F", "thoát nếu file vừa 1 màn hình"),
        ("-i", "tìm kiếm không phân biệt hoa thường"),
    ],
    examples: &[
        ("less file.log", "mở file"),
        ("less -RS access.log", "log có màu, không wrap"),
        ("command | less", "xem output dài"),
    ],
    note: "Phím trong less: \n  q thoát, /pattern tìm, n tìm tiếp, G cuối file, g đầu file,\n  Space trang sau, b trang trước, &pattern lọc.",
    skip_run: true, // interactive
};

const HEAD: Explanation = Explanation {
    name: "head",
    summary: "Xem N dòng đầu của file (mặc định 10)",
    usage: "head [-n N | -c N] <file>",
    flags: &[
        ("-n N", "lấy N dòng đầu (vd `-n 20`)"),
        ("-c N", "lấy N byte đầu"),
        ("-q", "không in tiêu đề file (khi nhiều file)"),
    ],
    examples: &[
        ("head file.log", "10 dòng đầu"),
        ("head -n 5 *.txt", "5 dòng đầu của mỗi file"),
        ("head -c 100 binary.bin", "100 byte đầu"),
        ("ls -lt | head", "10 file mới nhất"),
    ],
    note: "",
    skip_run: false,
};

const TAIL: Explanation = Explanation {
    name: "tail",
    summary: "Xem N dòng cuối của file (mặc định 10)",
    usage: "tail [-n N | -c N] [-f] <file>",
    flags: &[
        ("-n N", "lấy N dòng cuối"),
        ("-n +N", "in từ dòng N trở đi (bỏ qua đầu)"),
        ("-c N", "lấy N byte cuối"),
        ("-f", "follow — in dòng mới khi file được append (theo dõi log)"),
        ("-F", "follow + retry nếu file bị rotate"),
    ],
    examples: &[
        ("tail file.log", "10 dòng cuối"),
        ("tail -f access.log", "theo dõi log realtime"),
        ("tail -F /var/log/app.log", "theo dõi và bám theo rotation"),
        ("tail -n 100 huge.txt", "100 dòng cuối"),
    ],
    note: "Theo dõi nhiều file: `tail -f a.log b.log`. Thoát: Ctrl-C.",
    skip_run: false,
};

const ECHO: Explanation = Explanation {
    name: "echo",
    summary: "In chuỗi ra stdout",
    usage: "echo [-neE] <chuỗi ...>",
    flags: &[
        ("-n", "không xuống dòng cuối"),
        ("-e", "diễn giải escape (\\n, \\t, \\033[...m...)"),
        ("-E", "không diễn giải escape (default)"),
    ],
    examples: &[
        ("echo xin chào", "in 'xin chào' rồi xuống dòng"),
        ("echo -n abc; echo xyz", "abcxyz cùng dòng"),
        ("echo -e 'red:\\t\\033[31mhi\\033[0m'", "có escape"),
        ("echo $HOME", "in giá trị biến"),
        ("echo \"$(date)\" > stamp.txt", "ghi vào file"),
    ],
    note: "Cần escape phức tạp → dùng `printf` thay vì `echo -e` (portable hơn).",
    skip_run: false,
};

// Search & filter
const GREP: Explanation = Explanation {
    name: "grep",
    summary: "Tìm dòng khớp pattern trong file/input",
    usage: "grep [-irnvEoH] [-A N] [-B N] <pattern> [file ...]",
    flags: &[
        ("-i", "không phân biệt hoa thường"),
        ("-r / -R", "đệ quy vào thư mục"),
        ("-n", "in số dòng"),
        ("-H", "in tên file ngay cả khi 1 file"),
        ("-l", "chỉ in tên file có match"),
        ("-L", "chỉ in tên file KHÔNG match"),
        ("-v", "đảo — in dòng KHÔNG khớp"),
        ("-c", "đếm số dòng khớp"),
        ("-E", "extended regex (như +, ?, |)"),
        ("-F", "fixed string — không phải regex"),
        ("-o", "chỉ in phần khớp"),
        ("-w", "match cả từ (word boundary)"),
        ("-A N", "in N dòng SAU dòng khớp"),
        ("-B N", "in N dòng TRƯỚC"),
        ("-C N", "in N dòng both"),
        ("--include=GLOB", "chỉ tìm file khớp glob"),
        ("--exclude-dir=DIR", "bỏ qua thư mục"),
    ],
    examples: &[
        ("grep TODO src/main.rs", "tìm 'TODO'"),
        ("grep -rni 'error' /var/log", "đệ quy, có số dòng, không phân biệt hoa"),
        ("grep -v '^#' config.txt", "lọc bỏ dòng comment"),
        ("ps aux | grep [n]ginx", "tìm process nginx (mẹo `[n]` để khỏi match chính grep)"),
        ("grep -E 'cat|dog' file", "regex 'cat' hoặc 'dog'"),
        ("grep -rl 'TODO' --include='*.rs' .", "list file .rs có TODO"),
    ],
    note: "Nhanh hơn nhiều: `ripgrep` (rg). JakShell có `jak find text \"...\"` dùng rg nếu có.",
    skip_run: false,
};

const SORT: Explanation = Explanation {
    name: "sort",
    summary: "Sort dòng",
    usage: "sort [-nrfu] [-k FIELD] [-t SEP] [file]",
    flags: &[
        ("-n", "numeric sort (không alphabetical)"),
        ("-r", "đảo chiều"),
        ("-f", "ignore case"),
        ("-u", "unique — bỏ trùng (như uniq sau sort)"),
        ("-k N", "sort theo cột N (1-based)"),
        ("-t SEP", "ký tự phân tách cột (default: whitespace)"),
        ("-h", "human-numeric (2K < 1M < 1G)"),
        ("-V", "version sort (1.10 > 1.9)"),
    ],
    examples: &[
        ("sort names.txt", "sort alphabet"),
        ("sort -n nums.txt", "sort số (1, 2, 10 thay vì 1, 10, 2)"),
        ("sort -k2 -t',' data.csv", "sort theo cột 2 của CSV"),
        ("sort -u -h sizes.txt", "size dạng K/M/G, bỏ trùng"),
        ("ps aux | sort -k3 -nr | head", "process tốn CPU nhất"),
    ],
    note: "",
    skip_run: false,
};

const UNIQ: Explanation = Explanation {
    name: "uniq",
    summary: "Lọc dòng trùng LIÊN TIẾP (thường dùng sau sort)",
    usage: "uniq [-cdu] [-i] [file]",
    flags: &[
        ("-c", "đếm số lần xuất hiện"),
        ("-d", "chỉ in dòng có trùng"),
        ("-u", "chỉ in dòng không trùng"),
        ("-i", "ignore case"),
    ],
    examples: &[
        ("sort log.txt | uniq", "loại trùng"),
        ("sort | uniq -c | sort -rn", "đếm tần suất, sort giảm dần"),
        ("uniq -d names.txt", "in tên xuất hiện ≥ 2 lần (đã sort sẵn)"),
    ],
    note: "`uniq` CHỈ loại trùng liên tiếp! Luôn `sort` trước.",
    skip_run: false,
};

const WC: Explanation = Explanation {
    name: "wc",
    summary: "Đếm dòng / từ / ký tự / byte",
    usage: "wc [-lwmc] [file ...]",
    flags: &[
        ("-l", "lines (số dòng)"),
        ("-w", "words"),
        ("-c", "bytes"),
        ("-m", "characters (khác bytes nếu UTF-8)"),
        ("-L", "độ dài dòng dài nhất"),
    ],
    examples: &[
        ("wc file.txt", "lines / words / bytes"),
        ("wc -l *.rs", "số dòng mỗi file .rs"),
        ("cat file | wc -l", "đếm dòng qua pipe"),
        ("ls | wc -l", "đếm số entry trong cwd"),
    ],
    note: "",
    skip_run: false,
};

const CUT: Explanation = Explanation {
    name: "cut",
    summary: "Cắt cột/byte từ mỗi dòng",
    usage: "cut [-d SEP] -f LIST | -c LIST | -b LIST [file]",
    flags: &[
        ("-d SEP", "delimiter (default tab)"),
        ("-f LIST", "fields (cột) — `1,3` hoặc `2-5` hoặc `3-`"),
        ("-c LIST", "characters"),
        ("-b LIST", "bytes"),
        ("--complement", "lấy phần BÙ (mọi cột TRỪ LIST)"),
    ],
    examples: &[
        ("cut -d: -f1 /etc/passwd", "in tên user (cột 1, delimiter ':')"),
        ("cut -d, -f1,3 data.csv", "cột 1 và 3 của CSV"),
        ("cut -c1-10 file.txt", "10 ký tự đầu mỗi dòng"),
        ("ps aux | tr -s ' ' | cut -d' ' -f2,11", "lấy PID + COMMAND"),
    ],
    note: "Phân tách phức tạp hơn → dùng `awk`.",
    skip_run: false,
};

const TR: Explanation = Explanation {
    name: "tr",
    summary: "Translate / squeeze / delete ký tự từ stdin",
    usage: "tr [-cds] SET1 [SET2]",
    flags: &[
        ("-d", "delete — xoá ký tự trong SET1"),
        ("-s", "squeeze — gộp ký tự lặp liên tiếp"),
        ("-c", "complement — đảo SET1 thành 'mọi thứ TRỪ SET1'"),
    ],
    examples: &[
        ("echo HELLO | tr A-Z a-z", "viết thường: 'hello'"),
        ("echo 'a  b   c' | tr -s ' '", "gộp khoảng trắng: 'a b c'"),
        ("cat file | tr -d '\\r'", "xoá CR (chuyển CRLF → LF)"),
        ("echo abc123 | tr -cd '[:digit:]'", "chỉ giữ số: '123'"),
    ],
    note: "tr chỉ đọc stdin, không nhận file argument.",
    skip_run: false,
};

const XARGS: Explanation = Explanation {
    name: "xargs",
    summary: "Xây dựng lệnh từ stdin (chuyển từng dòng thành tham số)",
    usage: "<lệnh sinh list> | xargs [-n N] [-I {}] <lệnh>",
    flags: &[
        ("-n N", "tối đa N tham số mỗi lần gọi"),
        ("-I {}", "thay {} bằng từng item (mỗi item gọi lệnh 1 lần)"),
        ("-0", "input phân tách bằng NUL (đi với find -print0)"),
        ("-P N", "song song N tiến trình"),
        ("-r", "không chạy nếu input rỗng (default trên Linux)"),
        ("-t", "in lệnh trước khi chạy"),
    ],
    examples: &[
        ("find . -name '*.tmp' | xargs rm", "xoá mọi .tmp"),
        ("find . -print0 | xargs -0 grep TODO", "an toàn với tên có space"),
        ("ls *.jpg | xargs -I {} convert {} {}.png", "convert từng ảnh"),
        ("cat urls.txt | xargs -n1 -P4 curl -O", "tải song song 4 luồng"),
    ],
    note: "",
    skip_run: false,
};

// Process
const PS: Explanation = Explanation {
    name: "ps",
    summary: "Snapshot các process đang chạy",
    usage: "ps [aux | -ef] [-p PID]",
    flags: &[
        ("aux", "BSD-style: ALL user, kèm thông tin, kể cả process không có TTY"),
        ("-ef", "System V: tương đương aux trên Linux"),
        ("-p PID", "chỉ in process có PID đó"),
        ("-u USER", "chỉ process của USER"),
        ("--sort=KEY", "sort theo %cpu, %mem, pid, …"),
        ("-o COL,COL", "chọn cột tuỳ ý"),
    ],
    examples: &[
        ("ps", "process trong terminal hiện tại"),
        ("ps aux", "tất cả process toàn hệ thống"),
        ("ps aux --sort=-%cpu | head", "top CPU"),
        ("ps -p 1234 -o pid,ppid,user,cmd", "chỉ thông tin PID 1234"),
    ],
    note: "Dùng `top` hoặc `htop` để xem realtime.",
    skip_run: false,
};

const TOP: Explanation = Explanation {
    name: "top",
    summary: "Giám sát tài nguyên realtime",
    usage: "top [-o KEY] [-pid PID]",
    flags: &[
        ("q", "thoát"),
        ("k", "kill (top sẽ hỏi PID)"),
        ("r", "renice"),
        ("P", "sort theo CPU"),
        ("M", "sort theo MEM"),
        ("1", "hiện từng CPU core riêng (Linux)"),
        ("h / ?", "help bên trong top"),
    ],
    examples: &[
        ("top", "mở top"),
        ("top -o cpu", "macOS: sort theo CPU"),
        ("top -o mem", "macOS: sort theo MEM"),
    ],
    note: "Header: load average 1/5/15 phút (so với số core).\nThay thế đẹp hơn: `htop` (cài thêm).",
    skip_run: true, // interactive
};

const KILL: Explanation = Explanation {
    name: "kill",
    summary: "Gửi tín hiệu (signal) cho process",
    usage: "kill [-SIGNAL] <PID ...>",
    flags: &[
        ("-l", "liệt kê tên tất cả signal"),
        ("-9 / -KILL", "SIGKILL — kill cứng, không thể chặn (last resort)"),
        ("-15 / -TERM", "SIGTERM (default) — yêu cầu lịch sự, cho process tự dọn"),
        ("-2 / -INT", "SIGINT — giống Ctrl-C"),
        ("-1 / -HUP", "SIGHUP — reload config (nhiều daemon dùng)"),
        ("-19 / -STOP", "SIGSTOP — pause process"),
        ("-18 / -CONT", "SIGCONT — resume sau STOP"),
    ],
    examples: &[
        ("kill 1234", "TERM cho PID 1234"),
        ("kill -9 1234", "KILL cứng (chỉ khi TERM không xong)"),
        ("kill -HUP $(pgrep nginx)", "bảo nginx reload config"),
        ("kill -l", "list tất cả signal"),
    ],
    note: "Luôn thử TERM trước, KILL chỉ khi process không phản hồi.",
    skip_run: true,
};

const PKILL: Explanation = Explanation {
    name: "pkill",
    summary: "Kill process theo TÊN (không cần biết PID)",
    usage: "pkill [-SIGNAL] [-f] <pattern>",
    flags: &[
        ("-f", "match cả full command line, không chỉ tên binary"),
        ("-9", "SIGKILL"),
        ("-u USER", "chỉ process của USER"),
        ("-x", "match chính xác (không substring)"),
    ],
    examples: &[
        ("pkill nginx", "kill mọi nginx"),
        ("pkill -f 'python myapp.py'", "match cả command line"),
        ("pkill -9 -u alice node", "force-kill mọi node của alice"),
    ],
    note: "Dùng `pgrep <pattern>` trước để xem sẽ kill những PID nào.",
    skip_run: true,
};

const KILLALL: Explanation = Explanation {
    name: "killall",
    summary: "Kill tất cả process khớp tên (giống pkill)",
    usage: "killall [-SIGNAL] <tên>",
    flags: &[
        ("-i", "interactive — hỏi trước mỗi process"),
        ("-9", "SIGKILL"),
        ("-u USER", "chỉ process của USER"),
        ("-v", "verbose"),
    ],
    examples: &[
        ("killall Finder", "macOS: restart Finder"),
        ("killall -9 node", "force-kill mọi node"),
    ],
    note: "Cẩn thận: trên BSD/macOS, `killall <user>` có thể kill MỌI process của user đó.\nLinux thì `killall` chỉ theo TÊN process (an toàn hơn).",
    skip_run: true,
};

// Disk
const DF: Explanation = Explanation {
    name: "df",
    summary: "Dung lượng filesystem",
    usage: "df [-hTiH] [path]",
    flags: &[
        ("-h", "human-readable (K/M/G)"),
        ("-H", "human-readable, dùng 1000 thay vì 1024"),
        ("-T", "in cả filesystem type (Linux)"),
        ("-i", "thay vì size, in INODE usage"),
        ("-a", "kể cả pseudo-filesystem (tmpfs, devfs)"),
    ],
    examples: &[
        ("df -h", "size mọi mount"),
        ("df -h .", "mount chứa cwd"),
        ("df -i", "inode usage"),
        ("df -hT", "Linux: kèm fs type (ext4, xfs, btrfs…)"),
    ],
    note: "Hết inode dù còn size → vẫn không tạo được file mới! Check bằng `df -i`.",
    skip_run: false,
};

const DU: Explanation = Explanation {
    name: "du",
    summary: "Dung lượng thư mục / file (block-allocated)",
    usage: "du [-shak] [-d N] [path ...]",
    flags: &[
        ("-h", "human-readable"),
        ("-s", "summary — chỉ tổng, không liệt kê con"),
        ("-a", "all — kể cả file"),
        ("-d N / --max-depth=N", "sâu tối đa N cấp"),
        ("-c", "thêm dòng total ở cuối"),
        ("-x", "không vượt qua filesystem boundary"),
    ],
    examples: &[
        ("du -sh *", "size từng thư mục/file trong cwd"),
        ("du -h -d 1 /var", "/var sâu 1 cấp"),
        ("du -sh ~/.cache", "tổng cache trong HOME"),
        ("du -ah /tmp | sort -hr | head", "10 mục lớn nhất trong /tmp"),
    ],
    note: "`du` đo dung lượng BLOCK chiếm trên đĩa, không phải tổng size logic.",
    skip_run: false,
};

const FREE: Explanation = Explanation {
    name: "free",
    summary: "Tình trạng RAM/swap (Linux)",
    usage: "free [-hbkmgt] [-s SECS]",
    flags: &[
        ("-h", "human-readable"),
        ("-b / -k / -m / -g", "byte/KB/MB/GB"),
        ("-t", "thêm dòng Total"),
        ("-s N", "cập nhật mỗi N giây"),
        ("-w", "wide: tách buffers và cache"),
    ],
    examples: &[
        ("free -h", "tóm tắt RAM"),
        ("free -h -s 2", "cập nhật mỗi 2s"),
        ("free -m", "đơn vị MB"),
    ],
    note: "macOS không có `free` — dùng `vm_stat` hoặc `top`.\nChỉ số quan trọng nhất là `available`, không phải `free`.",
    skip_run: false,
};

const STAT: Explanation = Explanation {
    name: "stat",
    summary: "Metadata chi tiết của file",
    usage: "stat [-f FORMAT] <file>",
    flags: &[
        ("-c FMT (GNU)", "tự format output"),
        ("-f FMT (BSD/macOS)", "tự format output"),
        ("-L", "follow symlink"),
        ("--format=%s", "GNU: chỉ in size"),
    ],
    examples: &[
        ("stat file.txt", "metadata đầy đủ"),
        ("stat -c '%s %n' *.log", "GNU: in size + tên"),
        ("stat -f '%z %N' *.log", "BSD/macOS: in size + tên"),
    ],
    note: "Cột: Size, Blocks, IO Block, Inode, Links, Access (perm+user+group), atime/mtime/ctime/btime.",
    skip_run: false,
};

const LSOF: Explanation = Explanation {
    name: "lsof",
    summary: "List open files (file/socket/pipe đang được mở)",
    usage: "lsof [-p PID] [-i] [-u USER] [path]",
    flags: &[
        ("-p PID", "process này đang mở những gì"),
        ("-i", "chỉ network socket"),
        ("-i :PORT", "ai đang giữ port này"),
        ("-i @HOST", "kết nối tới HOST"),
        ("-u USER", "chỉ process của USER"),
        ("-c NAME", "chỉ process tên NAME"),
        ("+D DIR", "ai đang mở file trong DIR"),
    ],
    examples: &[
        ("lsof -i :8080", "process nào giữ port 8080"),
        ("lsof -p 1234", "PID 1234 đang mở gì"),
        ("lsof -i TCP:80", "TCP socket trên port 80"),
        ("lsof +D /var/log", "ai mở file trong /var/log"),
    ],
    note: "Cột FD: cwd=cur dir, txt=executable, mem=mmap, 0u/1u/2u=stdin/out/err.",
    skip_run: false,
};

// Network
const SSH: Explanation = Explanation {
    name: "ssh",
    summary: "Đăng nhập từ xa (Secure Shell)",
    usage: "ssh [-p PORT] [-i KEY] [-L L:H:R] [user@]host [lệnh]",
    flags: &[
        ("-p PORT", "đổi port (default 22)"),
        ("-i KEY", "private key file"),
        ("-l USER", "username (hoặc dùng user@host)"),
        ("-v / -vv / -vvv", "verbose debug"),
        ("-N", "không mở shell (cho tunnel-only)"),
        ("-f", "background sau khi đăng nhập"),
        ("-L L:H:R", "local port forward (L→H:R qua server)"),
        ("-R L:H:R", "remote port forward"),
        ("-D PORT", "SOCKS proxy"),
        ("-J host", "jump host (đi qua server trung gian)"),
        ("-o KEY=VAL", "tuỳ chọn thêm (vd `-o StrictHostKeyChecking=no`)"),
    ],
    examples: &[
        ("ssh alice@server.com", "đăng nhập"),
        ("ssh -p 2222 -i ~/.ssh/work alice@host", "port + key tuỳ chọn"),
        ("ssh user@host 'uname -a'", "chạy 1 lệnh từ xa rồi thoát"),
        ("ssh -L 8080:localhost:80 host", "tunnel localhost:8080 → host:80"),
        ("ssh -J bastion alice@internal", "qua bastion host"),
    ],
    note: "Sinh key: `ssh-keygen -t ed25519`. Copy public key: `ssh-copy-id user@host`.",
    skip_run: true,
};

const SCP: Explanation = Explanation {
    name: "scp",
    summary: "Copy file qua SSH",
    usage: "scp [-rPi] [user@]src [user@]dst",
    flags: &[
        ("-r", "đệ quy (cho thư mục)"),
        ("-P PORT", "port SSH (HOA — khác với ssh dùng -p thường)"),
        ("-i KEY", "private key"),
        ("-v", "verbose"),
        ("-p", "preserve mtime/permissions"),
        ("-C", "compress"),
    ],
    examples: &[
        ("scp file.txt alice@host:/tmp/", "upload"),
        ("scp alice@host:/tmp/file.txt .", "download"),
        ("scp -r src/ alice@host:/var/www/", "đệ quy"),
        ("scp -P 2222 file user@host:~/", "port tuỳ chỉnh"),
    ],
    note: "Hiện đại hơn: `rsync` (incremental, resume) hoặc `sftp`.",
    skip_run: true,
};

const CURL: Explanation = Explanation {
    name: "curl",
    summary: "HTTP/FTP client đa năng",
    usage: "curl [tuỳ chọn] <url ...>",
    flags: &[
        ("-s", "silent — bỏ progress bar"),
        ("-S", "show-error (đi cùng -s)"),
        ("-L", "follow redirect (3xx)"),
        ("-o FILE", "ghi body ra FILE"),
        ("-O", "ghi ra file cùng tên với URL"),
        ("-X METHOD", "GET/POST/PUT/DELETE/…"),
        ("-H 'Key: Val'", "thêm header"),
        ("-d 'data'", "body (mặc định = POST)"),
        ("-F 'name=@file'", "multipart upload"),
        ("-I", "chỉ headers (HEAD)"),
        ("-i", "in cả headers + body"),
        ("-u USER:PASS", "basic auth"),
        ("-w '%{http_code}'", "in metric sau khi xong (http_code, time_total, size_download…)"),
        ("--data-urlencode", "URL-encode body"),
        ("-k", "không verify SSL (cẩn thận)"),
    ],
    examples: &[
        ("curl https://api.github.com", "GET"),
        ("curl -sSLo file.zip https://...", "tải im lặng, follow redirect"),
        ("curl -X POST -H 'Content-Type: application/json' -d '{\"a\":1}' url", "POST JSON"),
        ("curl -F 'image=@photo.jpg' https://upload", "upload file"),
        ("curl -I url", "chỉ xem header response"),
        ("curl -w '\\n%{http_code} %{time_total}s\\n' url", "in status + thời gian"),
    ],
    note: "",
    skip_run: false,
};

const WGET: Explanation = Explanation {
    name: "wget",
    summary: "Tải file qua HTTP/HTTPS/FTP (đơn giản hơn curl cho tải)",
    usage: "wget [tuỳ chọn] <url>",
    flags: &[
        ("-c", "continue — resume download dở dang"),
        ("-O FILE", "ghi ra FILE"),
        ("-q", "quiet"),
        ("-r", "recursive — clone cả website"),
        ("--mirror", "= -r -N -l inf — mirror site"),
        ("-N", "chỉ tải nếu file mới hơn local"),
        ("--limit-rate=200k", "giới hạn tốc độ"),
        ("-P DIR", "ghi vào thư mục DIR"),
    ],
    examples: &[
        ("wget https://example.com/file.zip", "tải về file.zip"),
        ("wget -c url", "tiếp tục tải dở"),
        ("wget -O out.html url", "đổi tên file"),
        ("wget --mirror -p --convert-links https://site", "clone site offline"),
    ],
    note: "wget không có trên macOS mặc định. Cài qua `brew install wget` hoặc dùng `curl -O`.",
    skip_run: false,
};

const PING: Explanation = Explanation {
    name: "ping",
    summary: "Kiểm tra mạng bằng gói ICMP echo",
    usage: "ping [-c N] [-i SECS] [-W TIMEOUT] <host>",
    flags: &[
        ("-c N", "gửi N gói rồi thoát"),
        ("-i SECS", "khoảng cách giữa các gói (default 1s)"),
        ("-W SECS", "timeout chờ reply"),
        ("-s BYTES", "kích thước payload"),
        ("-q", "quiet — chỉ in tóm tắt cuối"),
        ("-4 / -6", "ép IPv4 / IPv6"),
    ],
    examples: &[
        ("ping google.com", "ping liên tục (Ctrl-C để dừng)"),
        ("ping -c 5 8.8.8.8", "gửi 5 gói rồi xong"),
        ("ping -i 0.2 -c 50 host", "50 gói, 200ms/gói"),
    ],
    note: "Reply: `icmp_seq=N ttl=T time=Xms`.\nCuối phiên: packet loss + min/avg/max/stddev.",
    skip_run: false,
};

const NETSTAT: Explanation = Explanation {
    name: "netstat",
    summary: "Socket / kết nối mạng / routing (lệnh cũ)",
    usage: "netstat [-tulpnar]",
    flags: &[
        ("-t", "TCP"),
        ("-u", "UDP"),
        ("-l", "chỉ listening socket"),
        ("-p", "in process giữ socket (Linux, cần root)"),
        ("-n", "không resolve DNS/service (nhanh hơn)"),
        ("-a", "tất cả (listening + connected)"),
        ("-r", "routing table"),
        ("-i", "interface statistics"),
    ],
    examples: &[
        ("netstat -tulpn", "Linux: port đang listen + PID"),
        ("netstat -an", "tất cả socket, dạng số"),
        ("netstat -r", "routing table"),
    ],
    note: "Trên Linux hiện đại nên dùng `ss` (nhanh hơn). Trên macOS dùng `lsof -i`.",
    skip_run: false,
};

const SS: Explanation = Explanation {
    name: "ss",
    summary: "Socket statistics (thay thế netstat — chỉ Linux)",
    usage: "ss [-tulpna] [filter]",
    flags: &[
        ("-t", "TCP"),
        ("-u", "UDP"),
        ("-l", "listening only"),
        ("-p", "process info"),
        ("-n", "no DNS resolve"),
        ("-a", "all"),
        ("-s", "summary"),
    ],
    examples: &[
        ("ss -tulpn", "port + PID đang listen"),
        ("ss -tn dst :443", "TCP đến port 443"),
        ("ss -s", "tóm tắt số kết nối"),
    ],
    note: "",
    skip_run: false,
};

const IFCONFIG: Explanation = Explanation {
    name: "ifconfig",
    summary: "Hiển thị / cấu hình interface (lệnh cũ — vẫn dùng nhiều trên macOS)",
    usage: "ifconfig [interface]",
    flags: &[
        ("(không tham số)", "list tất cả interface"),
        ("<iface> up/down", "bật/tắt interface"),
        ("<iface> IP netmask MASK", "đặt IP"),
    ],
    examples: &[
        ("ifconfig", "list tất cả"),
        ("ifconfig en0", "chỉ en0"),
        ("sudo ifconfig en0 down", "tắt en0"),
    ],
    note: "Trên Linux nên dùng `ip` (iproute2) hiện đại hơn.",
    skip_run: false,
};

const IP: Explanation = Explanation {
    name: "ip",
    summary: "Quản lý mạng hiện đại (Linux — iproute2)",
    usage: "ip [object] [command]",
    flags: &[
        ("ip addr / ip a", "list IP/interface"),
        ("ip link", "list/quản lý link layer"),
        ("ip route / ip r", "routing table"),
        ("ip neigh", "ARP table"),
        ("ip -s link", "stats per interface"),
        ("-4 / -6", "ép IPv4 / IPv6"),
        ("-c", "có màu"),
    ],
    examples: &[
        ("ip a", "list IP"),
        ("ip route", "default route + bảng định tuyến"),
        ("sudo ip addr add 10.0.0.5/24 dev eth0", "thêm IP"),
        ("sudo ip link set eth0 up", "bật interface"),
    ],
    note: "",
    skip_run: false,
};

// Archive
const TAR: Explanation = Explanation {
    name: "tar",
    summary: "Đóng / mở archive .tar / .tar.gz / .tar.bz2 / .tar.xz",
    usage: "tar [c|x|t][zjJ][vf] -f FILE [path ...]",
    flags: &[
        ("c", "create (tạo archive)"),
        ("x", "extract (giải)"),
        ("t", "list (xem nội dung)"),
        ("z", "gzip (.tar.gz / .tgz)"),
        ("j", "bzip2 (.tar.bz2)"),
        ("J", "xz (.tar.xz)"),
        ("v", "verbose"),
        ("f FILE", "tên file archive (BẮT BUỘC)"),
        ("-C DIR", "đổi sang DIR trước khi làm việc"),
        ("--exclude=PAT", "bỏ qua khớp PAT"),
    ],
    examples: &[
        ("tar -czvf out.tar.gz folder/", "nén thư mục thành .tar.gz"),
        ("tar -xzvf out.tar.gz", "giải nén"),
        ("tar -xzvf out.tar.gz -C /tmp/", "giải vào /tmp"),
        ("tar -tzf out.tar.gz", "xem nội dung không giải"),
        ("tar -czvf bk.tgz --exclude='*.log' folder/", "loại trừ log"),
    ],
    note: "Mẹo nhớ: \x1b[33mc\x1b[0mreate / e\x1b[33mx\x1b[0mtract / lis\x1b[33mt\x1b[0m ; \x1b[33mz\x1b[0m gzip ; \x1b[33mv\x1b[0merbose ; \x1b[33mf\x1b[0mile.",
    skip_run: false,
};

const ZIP: Explanation = Explanation {
    name: "zip",
    summary: "Đóng archive .zip (tương thích với Windows/macOS GUI)",
    usage: "zip [-r9q] <out.zip> <file ...>",
    flags: &[
        ("-r", "đệ quy"),
        ("-9", "compression cao nhất (chậm)"),
        ("-0", "không nén (chỉ gom)"),
        ("-q", "quiet"),
        ("-e", "mã hoá bằng password"),
        ("-x PATTERN", "loại trừ"),
    ],
    examples: &[
        ("zip out.zip a.txt b.txt", "zip 2 file"),
        ("zip -r out.zip folder/", "zip cả thư mục"),
        ("zip -re secret.zip folder/", "có password"),
        ("zip -r out.zip src/ -x '*.log'", "loại trừ .log"),
    ],
    note: "Trên macOS, file `.zip` từ Finder có thể chứa metadata `__MACOSX/` — đôi khi gây phiền.",
    skip_run: false,
};

const UNZIP: Explanation = Explanation {
    name: "unzip",
    summary: "Giải nén .zip",
    usage: "unzip [-lqo] <archive.zip> [-d DIR]",
    flags: &[
        ("-l", "list nội dung, không giải"),
        ("-d DIR", "giải vào DIR"),
        ("-o", "overwrite không hỏi"),
        ("-n", "không overwrite"),
        ("-q", "quiet"),
        ("-P PASS", "password (nếu có)"),
    ],
    examples: &[
        ("unzip out.zip", "giải vào cwd"),
        ("unzip out.zip -d /tmp/", "giải vào /tmp"),
        ("unzip -l out.zip", "xem nội dung"),
        ("unzip -o out.zip", "ghi đè không hỏi"),
    ],
    note: "",
    skip_run: false,
};

// System info
const UPTIME: Explanation = Explanation {
    name: "uptime",
    summary: "Máy đã chạy bao lâu + load average",
    usage: "uptime [-p] [-s]",
    flags: &[
        ("-p", "pretty — 'up 3 days, 4 hours' (Linux)"),
        ("-s", "in thời điểm boot"),
    ],
    examples: &[
        ("uptime", "uptime đầy đủ"),
        ("uptime -p", "ngắn gọn"),
        ("uptime -s", "thời điểm boot"),
    ],
    note: "Load average = số process đang chờ CPU, trung bình trong 1/5/15 phút.\nSo với số CPU core: < core = ổn, ≈ core = tải đầy, > core = quá tải.",
    skip_run: false,
};

const WHO: Explanation = Explanation {
    name: "who",
    summary: "Ai đang đăng nhập",
    usage: "who [-aH] | w [user]",
    flags: &[
        ("-a", "tất cả"),
        ("-H", "in header"),
        ("-q", "chỉ đếm số user"),
    ],
    examples: &[
        ("who", "list user + tty"),
        ("w", "kèm thông tin: IDLE, JCPU, WHAT"),
        ("whoami", "in tên user hiện tại"),
    ],
    note: "`w` cho thông tin hữu ích hơn (mỗi user đang làm gì).",
    skip_run: false,
};

const DATE: Explanation = Explanation {
    name: "date",
    summary: "Xem / đặt thời gian hệ thống",
    usage: "date [+FORMAT]",
    flags: &[
        ("+'%Y-%m-%d'", "format ngày: 2026-06-06"),
        ("+'%H:%M:%S'", "format giờ"),
        ("+'%s'", "Unix timestamp (epoch)"),
        ("-u", "UTC"),
        ("-d STRING", "parse STRING (GNU)"),
        ("-r FILE", "in mtime của FILE (BSD/macOS)"),
    ],
    examples: &[
        ("date", "ngày + giờ hiện tại"),
        ("date '+%Y%m%d-%H%M%S'", "20260606-153012 (đặt tên file theo ngày)"),
        ("date -u", "UTC"),
        ("date -d '2 days ago'", "GNU: ngày 2 hôm trước"),
        ("date -r 1700000000", "BSD/macOS: từ epoch"),
    ],
    note: "Đặt thời gian (cần root): `sudo date MMDDhhmm[CCYY]`.",
    skip_run: false,
};

const ENV: Explanation = Explanation {
    name: "env",
    summary: "In biến môi trường, hoặc chạy lệnh với env tuỳ chỉnh",
    usage: "env [-i] [VAR=VAL ...] [lệnh args ...]",
    flags: &[
        ("(không tham số)", "in tất cả biến"),
        ("-i", "bắt đầu với env rỗng (chỉ giữ VAR đã truyền)"),
        ("-u VAR", "bỏ biến VAR"),
        ("VAR=VAL", "đặt VAR khi chạy lệnh sau"),
    ],
    examples: &[
        ("env", "in tất cả env"),
        ("env | grep PATH", "lọc theo tên"),
        ("env DEBUG=1 ./app", "chạy app với DEBUG=1"),
        ("env -i bash", "shell sạch, không có env nào kế thừa"),
    ],
    note: "`printenv VAR` in 1 biến. `export` (builtin) thêm biến vào env hiện tại.",
    skip_run: false,
};

const ALIAS: Explanation = Explanation {
    name: "alias",
    summary: "Tạo / xem bí danh lệnh (builtin)",
    usage: "alias [name='value' ...]",
    flags: &[
        ("(không tham số)", "list tất cả alias"),
        ("name", "in alias `name`"),
        ("name='value'", "tạo alias"),
        ("unalias name", "xoá alias"),
    ],
    examples: &[
        ("alias", "list"),
        ("alias ll='ls -lah'", "tạo alias"),
        ("alias gs='git status'", ""),
        ("unalias ll", "xoá"),
    ],
    note: "Lưu vĩnh viễn: thêm vào `~/.jakshrc.toml` mục `[aliases]`, hoặc vào `~/.bashrc` / `~/.zshrc` với cú pháp tương ứng.",
    skip_run: false,
};

const HISTORY: Explanation = Explanation {
    name: "history",
    summary: "Xem lịch sử lệnh (builtin)",
    usage: "history [N]",
    flags: &[
        ("(không tham số)", "in tất cả"),
        ("N", "in N lệnh gần nhất"),
        ("!N", "chạy lại lệnh số N (trong nhiều shell)"),
        ("!!", "chạy lại lệnh trước"),
        ("!str", "chạy lại lệnh gần nhất bắt đầu bằng str"),
    ],
    examples: &[
        ("history", "tất cả"),
        ("history | grep git", "tìm trong history"),
        ("history -c (bash/zsh)", "xoá history"),
    ],
    note: "Tìm reverse: \x1b[33mCtrl-R\x1b[0m trong hầu hết shell (kể cả JakShell).",
    skip_run: false,
};

const WHICH: Explanation = Explanation {
    name: "which",
    summary: "Tìm đường dẫn đầy đủ của một lệnh",
    usage: "which [-a] <cmd ...>",
    flags: &[
        ("-a", "in TẤT CẢ kết quả khớp (theo thứ tự PATH)"),
    ],
    examples: &[
        ("which python", "/usr/bin/python"),
        ("which -a python", "mọi python trên PATH"),
        ("which ls", "/bin/ls"),
    ],
    note: "Tương tự: `type cmd` (cho biết là builtin/alias/function/file), `whereis cmd` (cả binary/source/manpage).",
    skip_run: false,
};

const MAN: Explanation = Explanation {
    name: "man",
    summary: "Đọc manual page (tài liệu hệ thống)",
    usage: "man [SECTION] <name> | man -k <keyword>",
    flags: &[
        ("1", "user commands"),
        ("2", "system calls"),
        ("3", "library functions"),
        ("5", "file formats (vd `man 5 crontab`)"),
        ("7", "miscellaneous (vd `man 7 signal`)"),
        ("8", "admin commands"),
        ("-k KW", "apropos — tìm man page theo từ khoá"),
        ("-f NAME", "= whatis"),
    ],
    examples: &[
        ("man ls", "đọc man của ls"),
        ("man 2 read", "system call read() (section 2)"),
        ("man -k password", "tìm man page liên quan password"),
        ("man man", "man về man"),
    ],
    note: "Phím trong man: q thoát, /pattern tìm, n tiếp, Space trang sau.\nThân thiện hơn man: `tldr` (cài thêm) — chỉ hiện ví dụ.",
    skip_run: true,
};

// Git
const GIT: Explanation = Explanation {
    name: "git",
    summary: "Version control system",
    usage: "git <subcommand> [args]",
    flags: &[
        ("init", "tạo repo mới"),
        ("clone URL", "clone repo từ URL"),
        ("add FILE", "stage thay đổi"),
        ("commit -m MSG", "tạo commit"),
        ("status", "xem tình trạng working tree"),
        ("log", "xem lịch sử commit"),
        ("diff", "xem thay đổi chưa commit"),
        ("branch", "xem/tạo branch"),
        ("checkout BR / switch BR", "chuyển branch"),
        ("merge BR", "merge BR vào branch hiện tại"),
        ("pull / push", "đồng bộ với remote"),
        ("stash", "tạm cất thay đổi"),
        ("reset", "huỷ stage / huỷ commit"),
        ("restore FILE", "khôi phục file"),
    ],
    examples: &[
        ("git status", "tình trạng"),
        ("git add -A && git commit -m 'msg'", "commit mọi thứ"),
        ("git log --oneline --graph --decorate -20", ""),
        ("git diff HEAD~1", "so với commit trước"),
    ],
    note: "Gõ `explain git status` / `explain git log` / `explain git diff` / `explain git branch` để xem chi tiết từng lệnh.",
    skip_run: false,
};

const GIT_STATUS: Explanation = Explanation {
    name: "git status",
    summary: "Tình trạng working tree (staged / modified / untracked)",
    usage: "git status [-s | --short] [-b | --branch]",
    flags: &[
        ("-s / --short", "short format: 2 ký tự XY + path"),
        ("-b / --branch", "in cả branch + tracking"),
        ("--porcelain", "format ổn định cho script"),
        ("--ignored", "in cả file bị ignore"),
    ],
    examples: &[
        ("git status", "đầy đủ"),
        ("git status -sb", "short + branch"),
    ],
    note: "Short format XY: X=staged state, Y=working-tree state.\nM modified  A added  D deleted  R renamed  ?? untracked  !! ignored  U unmerged.",
    skip_run: false,
};

const GIT_LOG: Explanation = Explanation {
    name: "git log",
    summary: "Lịch sử commit",
    usage: "git log [options] [path]",
    flags: &[
        ("--oneline", "1 dòng/commit"),
        ("--graph", "vẽ cấu trúc branch/merge"),
        ("--decorate", "hiện HEAD, branch, tag"),
        ("--all", "tất cả branch, không chỉ branch hiện tại"),
        ("-N", "chỉ N commit gần nhất"),
        ("--author=NAME", "lọc theo author"),
        ("--since='2 weeks ago'", "lọc theo thời gian"),
        ("-p", "hiện diff của mỗi commit"),
        ("--stat", "tóm tắt số dòng đổi mỗi file"),
        ("FILE", "chỉ commit ảnh hưởng FILE"),
    ],
    examples: &[
        ("git log --oneline --graph --decorate -20", ""),
        ("git log --author=alice --since='1 month'", ""),
        ("git log -p src/main.rs", "history kèm diff của 1 file"),
        ("git log --stat HEAD~5..HEAD", ""),
    ],
    note: "",
    skip_run: false,
};

const GIT_DIFF: Explanation = Explanation {
    name: "git diff",
    summary: "So sánh thay đổi (working/staged/commits)",
    usage: "git diff [options] [<commit>] [<commit>] [-- <path>]",
    flags: &[
        ("(không tham số)", "working ↔ index (staged)"),
        ("--staged / --cached", "staged ↔ HEAD"),
        ("HEAD", "working ↔ HEAD"),
        ("A..B", "A ↔ B"),
        ("--stat", "tóm tắt số dòng đổi"),
        ("--name-only", "chỉ tên file đổi"),
        ("--word-diff", "diff theo từ thay vì dòng"),
        ("-w", "ignore whitespace"),
    ],
    examples: &[
        ("git diff", "xem thay đổi chưa stage"),
        ("git diff --staged", "đã stage"),
        ("git diff main..feature", "so 2 branch"),
        ("git diff HEAD~3 HEAD", ""),
    ],
    note: "Hunk header: `@@ -X,Y +A,B @@` — X,Y old start,count; A,B new.\nKý hiệu: ` `=unchanged, `-`=removed, `+`=added.",
    skip_run: false,
};

const GIT_BRANCH: Explanation = Explanation {
    name: "git branch",
    summary: "Quản lý branch",
    usage: "git branch [options] [name]",
    flags: &[
        ("(không tham số)", "list local branch"),
        ("-a", "list cả remote"),
        ("-r", "chỉ remote"),
        ("-vv", "kèm SHA + upstream tracking"),
        ("-d NAME", "xoá branch (nếu đã merge)"),
        ("-D NAME", "force-xoá"),
        ("-m OLD NEW", "đổi tên"),
        ("--merged", "branch đã merge vào HEAD"),
        ("--no-merged", "branch chưa merge"),
    ],
    examples: &[
        ("git branch", "list"),
        ("git branch -vv", "kèm tracking info"),
        ("git branch feature/x", "tạo branch mới"),
        ("git branch -d old-branch", "xoá"),
        ("git switch -c new-branch", "tạo + chuyển (lệnh mới hơn)"),
    ],
    note: "Tracking format: `[origin/main: ahead 2, behind 1]` = local đi trước remote 2 commit, sau remote 1 commit.",
    skip_run: false,
};

// ─── Git extensions ───────────────────────────────────────────────────────────

const GIT_CLONE: Explanation = Explanation {
    name: "git clone",
    summary: "Sao bản sao của repo từ remote về local",
    usage: "git clone [options] <URL> [thư_mục_đích]",
    flags: &[
        ("--depth N", "shallow clone — chỉ lấy N commit cuối (nhanh, nhỏ)"),
        ("-b BRANCH", "clone branch cụ thể"),
        ("--single-branch", "chỉ tải branch đã chọn, không tải các branch khác"),
        ("--recurse-submodules", "clone luôn submodule"),
        ("-o NAME", "đặt tên remote (default: origin)"),
        ("--bare", "clone bare repo (chỉ .git, không working tree)"),
    ],
    examples: &[
        ("git clone https://github.com/user/repo.git", "clone về thư mục 'repo'"),
        ("git clone git@github.com:user/repo.git myrepo", "clone qua SSH, đổi tên thư mục"),
        ("git clone --depth 1 -b main URL", "shallow clone, chỉ branch main"),
        ("git clone --recurse-submodules URL", "kéo cả submodule"),
    ],
    note: "",
    skip_run: true,
};

const GIT_INIT: Explanation = Explanation {
    name: "git init",
    summary: "Khởi tạo repo git mới trong thư mục hiện tại",
    usage: "git init [-b <branch>] [--bare] [path]",
    flags: &[
        ("-b NAME / --initial-branch=NAME", "đặt tên branch đầu tiên (default: main hoặc master tuỳ config)"),
        ("--bare", "tạo bare repo (không có working tree — dùng làm server)"),
    ],
    examples: &[
        ("git init", "khởi tạo trong cwd"),
        ("git init -b main", "branch đầu là main"),
        ("git init --bare repo.git", "tạo bare repo (dùng làm remote)"),
    ],
    note: "Sau init: `git add .` rồi `git commit -m 'first'` để tạo commit đầu.",
    skip_run: true,
};

const GIT_ADD: Explanation = Explanation {
    name: "git add",
    summary: "Đưa thay đổi vào staging area (chuẩn bị commit)",
    usage: "git add [-A | -u | -p] <path ...>",
    flags: &[
        ("-A / --all", "stage TẤT CẢ (mới + sửa + xoá) trong toàn repo"),
        ("-u / --update", "chỉ stage file đã tracked (không lấy file mới)"),
        ("-p / --patch", "stage tương tác theo từng hunk — chọn phần nào staged"),
        ("-n / --dry-run", "in những gì sẽ stage, không làm thật"),
        ("-f / --force", "ép stage cả file bị ignore"),
        (".", "stage tất cả trong cwd (và con) — KHÔNG lấy thay đổi ở thư mục cha"),
    ],
    examples: &[
        ("git add file.txt", "stage 1 file"),
        ("git add .", "stage mọi thứ trong cwd"),
        ("git add -A", "stage mọi thay đổi toàn repo"),
        ("git add -u", "chỉ stage thay đổi của file đã track"),
        ("git add -p", "review từng hunk trước khi stage"),
    ],
    note: "`-A` vs `.` — `-A` lấy cả file bên ngoài cwd, `.` chỉ trong cwd.\nJakShell có `jak git save \"msg\"` = `git add -A && git commit -m`.",
    skip_run: true,
};

const GIT_COMMIT: Explanation = Explanation {
    name: "git commit",
    summary: "Tạo commit mới từ staged changes",
    usage: "git commit [-m MSG | --amend] [-a]",
    flags: &[
        ("-m MSG", "commit message inline (không mở editor)"),
        ("-a / --all", "tự add file đã track trước khi commit (KHÔNG lấy file mới)"),
        ("--amend", "sửa commit cuối (đổi message hoặc thêm file)"),
        ("--no-edit", "đi cùng --amend: giữ nguyên message"),
        ("--allow-empty", "cho phép commit không có thay đổi"),
        ("-S", "ký commit (cần GPG/SSH key)"),
        ("--no-verify", "bỏ qua pre-commit hook (CẨN THẬN)"),
    ],
    examples: &[
        ("git commit -m \"fix: typo\"", "commit nhanh"),
        ("git commit", "mở editor cho message dài (đa dòng)"),
        ("git commit -am \"msg\"", "auto-add file đã track + commit"),
        ("git commit --amend --no-edit", "thêm staged vào commit cuối, giữ message"),
        ("git commit --amend -m \"msg mới\"", "đổi message commit cuối"),
    ],
    note: "Đã push commit → amend là history rewrite, đừng làm nếu người khác đã pull.\nMessage chuẩn: dòng 1 ≤ 50 ký tự, kiểu mệnh lệnh ('add x' không 'added x').",
    skip_run: true,
};

const GIT_PUSH: Explanation = Explanation {
    name: "git push",
    summary: "Đẩy commit local lên remote",
    usage: "git push [<remote>] [<branch>] [-u] [-f]",
    flags: &[
        ("-u / --set-upstream", "đặt upstream cho branch (lần đầu push)"),
        ("-f / --force", "ép push (ghi đè history — NGUY HIỂM với người khác)"),
        ("--force-with-lease", "ép push AN TOÀN — fail nếu remote bị thay đổi từ lúc bạn pull"),
        ("--tags", "đẩy luôn tag"),
        ("--delete <branch>", "xoá branch trên remote"),
        ("--dry-run", "in những gì sẽ push, không thực sự push"),
    ],
    examples: &[
        ("git push", "push branch hiện tại lên upstream"),
        ("git push -u origin feature/x", "push lần đầu, đặt upstream"),
        ("git push --force-with-lease", "thay vì -f (an toàn hơn)"),
        ("git push origin --delete old-branch", "xoá branch trên remote"),
    ],
    note: "Tránh `push -f` lên branch chung. Dùng `--force-with-lease` để bảo vệ commit của người khác.",
    skip_run: true,
};

const GIT_PULL: Explanation = Explanation {
    name: "git pull",
    summary: "Tải commit từ remote và merge/rebase vào branch hiện tại",
    usage: "git pull [--rebase | --ff-only] [<remote>] [<branch>]",
    flags: &[
        ("--rebase / -r", "thay merge bằng rebase (history tuyến tính)"),
        ("--ff-only", "chỉ fast-forward — fail nếu cần merge"),
        ("--no-rebase", "ép merge dù cấu hình default là rebase"),
        ("--autostash", "tự stash thay đổi rồi pop sau khi xong"),
    ],
    examples: &[
        ("git pull", "pull theo cấu hình mặc định"),
        ("git pull --rebase", "rebase thay vì merge (sạch hơn)"),
        ("git pull --ff-only", "an toàn — chỉ chấp nhận fast-forward"),
        ("git pull origin main", "pull branch main từ origin"),
    ],
    note: "`pull` = `fetch` + `merge` (hoặc `rebase`). Nhiều người set default rebase: `git config --global pull.rebase true`.",
    skip_run: true,
};

const GIT_FETCH: Explanation = Explanation {
    name: "git fetch",
    summary: "Tải commit từ remote về (KHÔNG merge — chỉ cập nhật remote-tracking)",
    usage: "git fetch [--all] [--prune] [<remote>] [<branch>]",
    flags: &[
        ("--all", "fetch từ tất cả remote"),
        ("--prune / -p", "xoá ref local của branch đã bị xoá trên remote"),
        ("--tags", "fetch tags"),
        ("--depth N", "shallow fetch"),
    ],
    examples: &[
        ("git fetch", "fetch origin"),
        ("git fetch --all --prune", "cập nhật mọi remote + dọn ref cũ"),
        ("git fetch origin main", "chỉ fetch branch main"),
    ],
    note: "Sau fetch, branch local KHÔNG di chuyển. Xem update: `git log HEAD..origin/main`.",
    skip_run: false,
};

const GIT_MERGE: Explanation = Explanation {
    name: "git merge",
    summary: "Gộp branch khác vào branch hiện tại",
    usage: "git merge [--no-ff | --ff-only | --squash] <branch>",
    flags: &[
        ("--no-ff", "luôn tạo merge commit (giữ topology branch)"),
        ("--ff-only", "chỉ fast-forward, fail nếu cần merge commit"),
        ("--squash", "gộp tất cả commit của branch thành 1 (rồi cần commit thủ công)"),
        ("--abort", "huỷ merge đang dở, quay về trạng thái trước"),
        ("--continue", "tiếp tục merge sau khi giải xong conflict"),
        ("-m MSG", "message cho merge commit"),
    ],
    examples: &[
        ("git merge feature/x", "merge feature/x vào HEAD"),
        ("git merge --no-ff feature", "tạo merge commit để giữ history rõ"),
        ("git merge --squash feature && git commit -m \"feature\"", "squash"),
        ("git merge --abort", "huỷ khi gặp conflict không muốn xử lý"),
    ],
    note: "Có conflict: sửa file → `git add` file đã giải → `git merge --continue`.",
    skip_run: true,
};

const GIT_REBASE: Explanation = Explanation {
    name: "git rebase",
    summary: "Apply commit của branch hiện tại lên đỉnh branch khác (linear history)",
    usage: "git rebase [-i] [--onto X] <upstream>",
    flags: &[
        ("-i / --interactive", "rebase tương tác — reorder/squash/edit/drop commit"),
        ("--onto X", "rebase lên target X khác upstream"),
        ("--continue", "tiếp sau khi giải xong conflict"),
        ("--abort", "huỷ rebase đang dở"),
        ("--skip", "bỏ commit hiện tại, đi tiếp"),
        ("--autosquash", "tự sắp xếp commit fixup!/squash!"),
    ],
    examples: &[
        ("git rebase main", "đưa commit của branch hiện tại lên đỉnh main"),
        ("git rebase -i HEAD~5", "interactive rebase 5 commit cuối"),
        ("git rebase --onto main feature~3 feature", "lấy 3 commit cuối của feature, đặt lên main"),
    ],
    note: "⚠ Đừng rebase commit đã push lên branch chung (history rewrite gây vấn đề cho người khác).\nMerge an toàn hơn rebase khi làm việc nhóm.",
    skip_run: true,
};

const GIT_RESET: Explanation = Explanation {
    name: "git reset",
    summary: "Di chuyển HEAD (và tuỳ chọn: index, working tree)",
    usage: "git reset [--soft | --mixed | --hard] [<commit>]",
    flags: &[
        ("--soft", "chỉ di chuyển HEAD; index + working tree GIỮ NGUYÊN"),
        ("--mixed (default)", "di chuyển HEAD + reset index; working tree GIỮ NGUYÊN"),
        ("--hard", "di chuyển HEAD + reset index + xoá thay đổi working tree (MẤT DỮ LIỆU)"),
        ("HEAD~1", "lùi 1 commit"),
        ("<file>", "không kèm commit: bỏ stage file (giống `restore --staged`)"),
    ],
    examples: &[
        ("git reset --soft HEAD~1", "huỷ commit cuối, GIỮ staged → ~ `jak git uncommit`"),
        ("git reset HEAD~1", "huỷ commit cuối, GIỮ thay đổi (chưa stage)"),
        ("git reset --hard HEAD~1", "huỷ commit cuối, XOÁ luôn thay đổi"),
        ("git reset HEAD file.txt", "bỏ stage file.txt"),
    ],
    note: "⚠ `--hard` không phục hồi được qua `git status` — dùng `git reflog` để tìm commit cũ.\nCommit đã push? Dùng `git revert` thay vì reset.",
    skip_run: true,
};

const GIT_RESTORE: Explanation = Explanation {
    name: "git restore",
    summary: "Khôi phục file (cách mới, thay cho `checkout <file>` và `reset HEAD <file>`)",
    usage: "git restore [--staged] [--source <ref>] <file ...>",
    flags: &[
        ("--staged / -S", "bỏ stage file (KHÔNG đổi nội dung file trên đĩa)"),
        ("--worktree / -W", "khôi phục file về như HEAD (XOÁ thay đổi chưa stage)"),
        ("--source <ref>", "khôi phục từ commit/branch khác"),
        ("--patch / -p", "tương tác từng hunk"),
    ],
    examples: &[
        ("git restore --staged file.txt", "bỏ stage"),
        ("git restore file.txt", "khôi phục file về như HEAD — MẤT chỉnh sửa chưa commit"),
        ("git restore --source=main src/", "lấy thư mục src/ từ branch main"),
        ("git restore .", "khôi phục TẤT CẢ — CẨN THẬN"),
    ],
    note: "Lệnh mới (git ≥ 2.23). Cũ: `git checkout -- file` (worktree) hoặc `git reset HEAD file` (unstage).",
    skip_run: true,
};

const GIT_REVERT: Explanation = Explanation {
    name: "git revert",
    summary: "Tạo commit MỚI undo commit cũ (an toàn cho history đã push)",
    usage: "git revert [<commit> ...]",
    flags: &[
        ("--no-commit / -n", "stage thay đổi đảo, KHÔNG tự commit"),
        ("--no-edit", "không mở editor cho message"),
        ("-m N", "với merge commit: chọn parent N để revert"),
        ("--continue / --abort", "khi gặp conflict"),
    ],
    examples: &[
        ("git revert HEAD", "đảo commit cuối (tạo commit mới)"),
        ("git revert abc1234", "đảo commit abc1234"),
        ("git revert HEAD~3..HEAD", "đảo 3 commit cuối, tạo 3 commit đảo"),
        ("git revert -n abc1234", "stage thay đổi đảo, để gộp với thay đổi khác trước khi commit"),
    ],
    note: "Khác `reset`: revert KHÔNG xoá history, chỉ thêm commit mới. AN TOÀN với branch chung.",
    skip_run: true,
};

const GIT_STASH: Explanation = Explanation {
    name: "git stash",
    summary: "Cất tạm thay đổi để chuyển branch hoặc dọn working tree",
    usage: "git stash [push -m MSG] | list | pop | apply | drop | clear",
    flags: &[
        ("push -m MSG", "cất với message (default action)"),
        ("-u / --include-untracked", "cất luôn file untracked"),
        ("-k / --keep-index", "không cất file đã staged"),
        ("list", "xem danh sách stash"),
        ("show [-p] [stash@{N}]", "xem nội dung stash"),
        ("pop [stash@{N}]", "apply + xoá khỏi list"),
        ("apply [stash@{N}]", "apply nhưng GIỮ trong list"),
        ("drop [stash@{N}]", "xoá 1 stash"),
        ("clear", "xoá TẤT CẢ stash"),
        ("branch NAME [stash]", "tạo branch mới từ stash"),
    ],
    examples: &[
        ("git stash", "cất thay đổi"),
        ("git stash push -m \"WIP feature X\"", "cất có message"),
        ("git stash -u", "cất cả untracked"),
        ("git stash list", "xem stash@{0}, @{1}, …"),
        ("git stash pop", "lấy stash mới nhất ra"),
        ("git stash apply stash@{2}", "áp stash số 2, giữ trong list"),
        ("git stash drop stash@{0}", "xoá stash mới nhất"),
    ],
    note: "Stash là stack: @{0} mới nhất. `pop` an toàn nếu chắc apply OK; `apply` rồi `drop` thủ công nếu cần giữ backup.",
    skip_run: true,
};

const GIT_TAG: Explanation = Explanation {
    name: "git tag",
    summary: "Đánh dấu commit (thường cho release vX.Y.Z)",
    usage: "git tag [-a <name> -m MSG] [-d <name>] [<name> [<commit>]]",
    flags: &[
        ("(không tham số)", "list tag"),
        ("-l 'pattern'", "list tag khớp pattern"),
        ("<name>", "tạo lightweight tag (chỉ là 1 ref)"),
        ("-a <name> -m MSG", "tạo annotated tag (có metadata + signature)"),
        ("-s <name>", "tag có chữ ký GPG"),
        ("-d <name>", "xoá tag local"),
        ("git push origin <tag>", "push tag lên remote (push thường KHÔNG mang tag)"),
        ("git push --tags", "push tất cả tag"),
        ("git push origin --delete <tag>", "xoá tag trên remote"),
    ],
    examples: &[
        ("git tag", "list tag"),
        ("git tag v1.0.0", "lightweight tag tại HEAD"),
        ("git tag -a v1.0.0 -m \"Release 1.0\"", "annotated tag (khuyên dùng cho release)"),
        ("git push origin v1.0.0", "push tag lên remote"),
        ("git tag -d v1.0.0-rc", "xoá tag local"),
    ],
    note: "Tag là ref bất biến — gắn vào commit, không di chuyển.",
    skip_run: false,
};

const GIT_REMOTE: Explanation = Explanation {
    name: "git remote",
    summary: "Quản lý remote (bản sao của repo ở nơi khác)",
    usage: "git remote [-v] | add NAME URL | remove NAME | set-url NAME URL | rename OLD NEW",
    flags: &[
        ("(không tham số)", "list tên remote"),
        ("-v / --verbose", "kèm URL"),
        ("add NAME URL", "thêm remote mới"),
        ("remove / rm NAME", "xoá remote"),
        ("rename OLD NEW", "đổi tên remote"),
        ("set-url NAME URL", "đổi URL"),
        ("show NAME", "chi tiết: URL, fetch refs, branch theo dõi"),
        ("prune NAME", "xoá ref local của branch đã bị xoá trên remote"),
    ],
    examples: &[
        ("git remote -v", "list kèm URL"),
        ("git remote add upstream https://github.com/orig/repo", "thêm upstream"),
        ("git remote set-url origin git@github.com:user/repo.git", "đổi sang SSH"),
        ("git remote prune origin", "dọn ref cũ"),
    ],
    note: "Quy ước tên: `origin` = remote chính, `upstream` = repo gốc khi fork.",
    skip_run: false,
};

const GIT_CHECKOUT: Explanation = Explanation {
    name: "git checkout",
    summary: "Chuyển branch / khôi phục file (lệnh đa năng — Git 2.23 tách thành `switch` + `restore`)",
    usage: "git checkout <branch> | -b <new> | <commit> | -- <file>",
    flags: &[
        ("<branch>", "chuyển sang branch"),
        ("-b <new>", "tạo branch mới + chuyển"),
        ("-B <new>", "tạo hoặc reset branch + chuyển"),
        ("<commit>", "chuyển sang commit (detached HEAD)"),
        ("-- <file>", "khôi phục file về như HEAD (như `restore --worktree`)"),
        ("-t <remote-branch>", "track remote branch"),
        ("-f", "force — bỏ qua thay đổi chưa commit (MẤT DỮ LIỆU)"),
    ],
    examples: &[
        ("git checkout main", "chuyển sang main"),
        ("git checkout -b feature/x", "tạo branch + chuyển"),
        ("git checkout -- file.txt", "khôi phục file"),
        ("git checkout abc123", "detached HEAD ở commit abc123"),
    ],
    note: "Khuyên dùng: `git switch` để chuyển branch, `git restore` để khôi phục file (rõ ràng hơn).",
    skip_run: true,
};

const GIT_SWITCH: Explanation = Explanation {
    name: "git switch",
    summary: "Chuyển branch (lệnh mới, thay cho `checkout` cho mục đích chuyển branch)",
    usage: "git switch [-c | -C] <branch>",
    flags: &[
        ("<branch>", "chuyển sang branch đã có"),
        ("-c <new>", "tạo branch mới + chuyển"),
        ("-C <new>", "tạo hoặc reset branch + chuyển"),
        ("-d <commit>", "detached HEAD ở commit"),
        ("- (dấu trừ)", "chuyển về branch trước đó"),
        ("--orphan <new>", "tạo branch không có history (clean slate)"),
        ("-t <remote>", "track remote branch"),
    ],
    examples: &[
        ("git switch main", "chuyển sang main"),
        ("git switch -c feature/x", "tạo + chuyển"),
        ("git switch -", "quay lại branch trước"),
        ("git switch -t origin/feat-x", "tạo branch local track remote feat-x"),
    ],
    note: "Git ≥ 2.23. KHÔNG động vào file (so với `checkout` đa năng) — an toàn hơn.",
    skip_run: true,
};

const GIT_CHERRY_PICK: Explanation = Explanation {
    name: "git cherry-pick",
    summary: "Apply 1 (hoặc nhiều) commit từ branch khác lên HEAD",
    usage: "git cherry-pick <commit> [<commit> ...]",
    flags: &[
        ("<commit>", "apply commit này lên HEAD (tạo commit mới)"),
        ("A^..B", "range — apply A đến B (KHÔNG bao gồm A; +1 nếu muốn gồm A: A~1..B)"),
        ("-n / --no-commit", "stage thay đổi, không tự commit"),
        ("-x", "ghi 'cherry picked from commit X' vào message"),
        ("--continue / --abort / --skip", "khi gặp conflict"),
    ],
    examples: &[
        ("git cherry-pick abc1234", "lấy commit abc1234 áp vào branch hiện tại"),
        ("git cherry-pick abc..def", "lấy range"),
        ("git cherry-pick -x abc1234", "có note 'cherry picked from'"),
    ],
    note: "Conflict → fix → `git add` → `git cherry-pick --continue`.",
    skip_run: true,
};

const GIT_BLAME: Explanation = Explanation {
    name: "git blame",
    summary: "Cho biết mỗi dòng được commit bởi ai, khi nào",
    usage: "git blame [-L START,END] [-e] <file>",
    flags: &[
        ("-L START,END", "chỉ blame phạm vi dòng (vd -L 10,20)"),
        ("-L /regex/", "blame dòng khớp regex"),
        ("-e", "in email thay vì tên"),
        ("-w", "ignore whitespace changes"),
        ("--since=DATE", "chỉ commit từ DATE"),
        ("-C", "phát hiện code di chuyển/copy"),
    ],
    examples: &[
        ("git blame src/main.rs", "blame cả file"),
        ("git blame -L 10,20 src/main.rs", "chỉ dòng 10-20"),
        ("git blame -L /TODO/,+5 file", "blame 5 dòng quanh TODO"),
    ],
    note: "Để xem chi tiết commit từ blame: `git show <sha>`.",
    skip_run: false,
};

const GIT_SHOW: Explanation = Explanation {
    name: "git show",
    summary: "Xem chi tiết 1 object (commit / tag / file ở commit)",
    usage: "git show [<commit> | <tag> | <commit>:<file>]",
    flags: &[
        ("<commit>", "metadata + diff của commit"),
        ("<tag>", "thông tin tag (annotated) hoặc trỏ tới commit"),
        ("<commit>:<file>", "in nội dung file tại commit"),
        ("--stat", "tóm tắt số dòng đổi mỗi file"),
        ("--name-only", "chỉ tên file đổi"),
        ("--pretty=oneline", "format gọn"),
    ],
    examples: &[
        ("git show HEAD", "commit cuối"),
        ("git show abc123", "commit abc123"),
        ("git show abc123:src/main.rs", "file src/main.rs ở commit abc123"),
        ("git show --stat HEAD~5", "tóm tắt commit cách đây 5"),
    ],
    note: "",
    skip_run: false,
};

const GIT_REFLOG: Explanation = Explanation {
    name: "git reflog",
    summary: "Lịch sử mọi cú di chuyển HEAD (cứu commit đã 'mất' do reset/rebase)",
    usage: "git reflog [show] [-n N]",
    flags: &[
        ("(không tham số)", "in tất cả entry"),
        ("show", "= không tham số"),
        ("-n N", "N entry gần nhất"),
        ("expire", "xoá entry cũ (cấu hình hết hạn)"),
    ],
    examples: &[
        ("git reflog", "xem mọi cú di chuyển HEAD gần đây"),
        ("git reset --hard HEAD@{2}", "phục hồi HEAD về 2 bước trước"),
        ("git show HEAD@{1}", "xem commit ở vị trí HEAD trước đó"),
    ],
    note: "⭐ Cứu cánh khi lỡ `reset --hard` hoặc rebase nhầm. Default giữ 90 ngày.",
    skip_run: false,
};

const GIT_CONFIG: Explanation = Explanation {
    name: "git config",
    summary: "Xem/đặt cấu hình git (local / global / system)",
    usage: "git config [--global | --local | --system] [--list | --get | --unset] <key> [value]",
    flags: &[
        ("--global", "cho user hiện tại (~/.gitconfig)"),
        ("--local (default)", "chỉ repo hiện tại (.git/config)"),
        ("--system", "toàn hệ thống (/etc/gitconfig)"),
        ("--list / -l", "in tất cả config"),
        ("--get KEY", "in 1 key"),
        ("--unset KEY", "xoá key"),
        ("--edit / -e", "mở config bằng editor"),
    ],
    examples: &[
        ("git config --global user.name \"Alice\"", ""),
        ("git config --global user.email \"alice@example.com\"", ""),
        ("git config --global pull.rebase true", "default rebase khi pull"),
        ("git config --global init.defaultBranch main", ""),
        ("git config --list --show-origin", "xem mọi config + file nào set"),
        ("git config --global alias.lg \"log --oneline --graph --decorate\"", ""),
    ],
    note: "Thứ tự ưu tiên: system < global < local < environment.",
    skip_run: false,
};

// ─── SSH family ───────────────────────────────────────────────────────────────

const SSH_KEYGEN: Explanation = Explanation {
    name: "ssh-keygen",
    summary: "Tạo / quản lý SSH key pair",
    usage: "ssh-keygen -t <type> [-b BITS] [-C COMMENT] [-f FILE]",
    flags: &[
        ("-t TYPE", "loại key: ed25519 (khuyên dùng) / rsa / ecdsa / dsa (lỗi thời)"),
        ("-b BITS", "độ dài key (rsa: 3072+; ed25519 không cần)"),
        ("-C COMMENT", "comment (thường là email) gắn vào public key"),
        ("-f FILE", "tên file output (default ~/.ssh/id_<type>)"),
        ("-N PASSPHRASE", "đặt passphrase ngay (rỗng = không passphrase)"),
        ("-p", "đổi passphrase của key đã có"),
        ("-y", "in lại public key từ private key"),
        ("-l -f FILE", "in fingerprint của key"),
        ("-R HOST", "xoá HOST khỏi known_hosts"),
    ],
    examples: &[
        ("ssh-keygen -t ed25519 -C \"alice@example.com\"", "tạo key ed25519 (mặc định ~/.ssh/id_ed25519)"),
        ("ssh-keygen -t rsa -b 4096 -C \"work\" -f ~/.ssh/work_rsa", "rsa 4096-bit, file riêng"),
        ("ssh-keygen -p -f ~/.ssh/id_ed25519", "đổi passphrase"),
        ("ssh-keygen -y -f ~/.ssh/id_ed25519", "in public key từ private"),
        ("ssh-keygen -lf ~/.ssh/id_ed25519.pub", "in fingerprint"),
    ],
    note: "ed25519 nhanh + an toàn hơn RSA cùng độ bảo mật. Public key có đuôi `.pub`.",
    skip_run: true,
};

const SSH_COPY_ID: Explanation = Explanation {
    name: "ssh-copy-id",
    summary: "Copy public key sang authorized_keys của server (để SSH không cần password)",
    usage: "ssh-copy-id [-i KEY.pub] [-p PORT] [user@]host",
    flags: &[
        ("-i FILE", "chỉ định public key cụ thể (mặc định ~/.ssh/id_*.pub)"),
        ("-p PORT", "port SSH"),
        ("-f", "force — không check key đã có chưa"),
        ("-n", "dry run — in lệnh sẽ chạy, không thực sự copy"),
    ],
    examples: &[
        ("ssh-copy-id alice@server.com", "copy key mặc định"),
        ("ssh-copy-id -i ~/.ssh/work_rsa.pub alice@host", "key riêng"),
        ("ssh-copy-id -p 2222 user@host", "port khác 22"),
    ],
    note: "Yêu cầu đăng nhập bằng password lần này, sau đó SSH sẽ không cần password nữa.\nKhông có `ssh-copy-id` (Windows)? Dùng: `cat ~/.ssh/id_ed25519.pub | ssh user@host 'cat >> ~/.ssh/authorized_keys'`.",
    skip_run: true,
};

const SSH_ADD: Explanation = Explanation {
    name: "ssh-add",
    summary: "Thêm SSH key vào ssh-agent (để không phải gõ passphrase mỗi lần)",
    usage: "ssh-add [file] | -l | -d | -D | -t SECS",
    flags: &[
        ("(không tham số)", "thêm các key mặc định trong ~/.ssh/"),
        ("FILE", "thêm key cụ thể"),
        ("-l", "list fingerprint key đã add"),
        ("-L", "list public key đầy đủ"),
        ("-d FILE", "xoá 1 key khỏi agent"),
        ("-D", "xoá TẤT CẢ key"),
        ("-t SECS", "giới hạn thời gian sống của key (giây)"),
        ("-K (macOS)", "lưu passphrase vào Keychain"),
    ],
    examples: &[
        ("ssh-add", "add key mặc định"),
        ("ssh-add ~/.ssh/work_rsa", "add key cụ thể"),
        ("ssh-add -l", "xem các key đang trong agent"),
        ("ssh-add -K ~/.ssh/id_ed25519", "macOS: lưu passphrase vào Keychain"),
        ("ssh-add -t 3600 ~/.ssh/temp_key", "key sống 1 giờ"),
    ],
    note: "Agent chưa chạy? `eval \"$(ssh-agent -s)\"`. Trên macOS, agent đã sẵn (`ssh-agent` autostart).",
    skip_run: true,
};

const SFTP: Explanation = Explanation {
    name: "sftp",
    summary: "Secure FTP — interactive file transfer qua SSH",
    usage: "sftp [-P PORT] [-i KEY] [user@]host",
    flags: &[
        ("-P PORT (HOA)", "port SSH (khác với scp dùng -P, ssh dùng -p thường)"),
        ("-i KEY", "private key"),
        ("-r", "đệ quy (cho put / get)"),
        ("-b FILE", "batch mode — đọc lệnh từ FILE"),
    ],
    examples: &[
        ("sftp alice@host", "mở phiên interactive"),
        ("sftp -P 2222 -i ~/.ssh/work user@host", "port + key tuỳ chọn"),
        ("sftp -b script.txt user@host", "chạy lệnh từ file"),
    ],
    note: "Phím trong sftp:\n  ls / lls         remote / local ls\n  cd / lcd         remote / local cd\n  pwd / lpwd       remote / local pwd\n  put FILE         upload\n  get FILE         download\n  mkdir / rmdir / rm\n  bye / quit       thoát\nMặc định: kết nối tới HOME của user remote.",
    skip_run: true,
};

// ─── Docker ───────────────────────────────────────────────────────────────────

const DOCKER: Explanation = Explanation {
    name: "docker",
    summary: "Container runtime — quản lý image và container",
    usage: "docker <subcommand> [args]",
    flags: &[
        ("ps",           "list container đang chạy"),
        ("ps -a",        "list TẤT CẢ container (kể cả đã stop)"),
        ("images",       "list image local"),
        ("run IMAGE",    "chạy container mới"),
        ("exec CONT CMD","chạy lệnh trong container đang chạy"),
        ("logs CONT",    "xem log"),
        ("build PATH",   "build image từ Dockerfile"),
        ("pull IMAGE",   "tải image từ registry"),
        ("push IMAGE",   "đẩy image lên registry"),
        ("stop / start / restart / kill", "lifecycle container"),
        ("rm / rmi",     "xoá container / image"),
        ("inspect",      "metadata chi tiết (JSON)"),
        ("network / volume", "quản lý network / volume"),
        ("compose",      "Docker Compose (docker-compose.yml)"),
        ("system prune", "dọn dẹp toàn bộ resource không dùng"),
    ],
    examples: &[
        ("docker ps", "container đang chạy"),
        ("docker exec -it payin_app sh", "shell vào container"),
        ("docker logs -f my_api", "follow log"),
        ("docker compose up -d", "khởi động stack"),
    ],
    note: "Gõ `explain docker <sub>` để xem chi tiết từng tiểu lệnh.",
    skip_run: false,
};

const DOCKER_PS: Explanation = Explanation {
    name: "docker ps",
    summary: "List container",
    usage: "docker ps [-a] [-q] [-f FILTER] [--format FMT]",
    flags: &[
        ("-a / --all",        "list cả container đã stop"),
        ("-q / --quiet",      "chỉ in container ID (cho script: `docker rm $(docker ps -aq)`)"),
        ("-s / --size",       "kèm dung lượng"),
        ("-n N",              "N container mới nhất"),
        ("-l",                "container vừa tạo gần nhất"),
        ("-f KEY=VAL",        "lọc: status=running, name=foo, label=app=web…"),
        ("--format FMT",      "Go template (vd '{{.Names}}\\t{{.Status}}')"),
        ("--no-trunc",        "không cắt cột"),
    ],
    examples: &[
        ("docker ps",                                 "đang chạy"),
        ("docker ps -a",                              "tất cả"),
        ("docker ps -aq",                             "chỉ ID, dễ pipe"),
        ("docker ps -f status=exited",                "chỉ đã exit"),
        ("docker ps --format 'table {{.Names}}\\t{{.Status}}\\t{{.Ports}}'", "format gọn"),
    ],
    note: "Cột mặc định: CONTAINER ID, IMAGE, COMMAND, CREATED, STATUS, PORTS, NAMES.",
    skip_run: false,
};

const DOCKER_EXEC: Explanation = Explanation {
    name: "docker exec",
    summary: "Chạy lệnh trong container ĐANG CHẠY",
    usage: "docker exec [-it] [-u USER] [-w DIR] [-e VAR=VAL] <container> <command> [args]",
    flags: &[
        ("-i / --interactive", "giữ STDIN mở (cho input)"),
        ("-t / --tty",         "cấp pseudo-TTY (cho shell tương tác)"),
        ("-it",                "combo phổ biến — shell interactive"),
        ("-d / --detach",      "chạy background, không chờ"),
        ("-u USER",            "chạy với user (vd 'root', 'node')"),
        ("-w DIR",             "working directory bên trong container"),
        ("-e VAR=VAL",         "biến môi trường tạm"),
        ("--privileged",       "đặc quyền (NGUY HIỂM)"),
    ],
    examples: &[
        ("docker exec -it payin_app sh",            "mở shell trong container"),
        ("docker exec -it db psql -U postgres",     "psql interactive"),
        ("docker exec api npm run migrate",         "chạy lệnh rồi thoát"),
        ("docker exec -u root -it nginx bash",      "vào với quyền root"),
        ("docker exec -e DEBUG=1 worker ./debug.sh","truyền env tạm"),
    ],
    note: "Container PHẢI đang chạy. Nếu đã stop → dùng `docker start` trước, hoặc `docker run` với image.\nContainer dùng `alpine` thường không có `bash` — dùng `sh`.",
    skip_run: true,
};

const DOCKER_RUN: Explanation = Explanation {
    name: "docker run",
    summary: "Tạo + chạy container mới từ image",
    usage: "docker run [options] <image> [command] [args]",
    flags: &[
        ("-d / --detach",       "chạy nền (daemon)"),
        ("-it",                 "interactive + tty"),
        ("--name NAME",         "đặt tên container (nếu không sẽ random)"),
        ("--rm",                "tự xoá container khi exit (cho lệnh tạm)"),
        ("-p H:C / --publish",  "map port HOST:CONTAINER (vd 8080:80)"),
        ("-P",                  "map mọi expose port với port random ở host"),
        ("-v H:C / --volume",   "mount HOST_PATH:CONT_PATH (or named volume)"),
        ("-e VAR=VAL",          "biến môi trường"),
        ("--env-file FILE",     "đọc env từ file"),
        ("--network NET",       "kết nối vào network"),
        ("--restart POLICY",    "no / on-failure / always / unless-stopped"),
        ("-u USER",             "user chạy bên trong"),
        ("-w DIR",              "working dir"),
        ("--entrypoint CMD",    "ghi đè ENTRYPOINT"),
        ("--memory / --cpus",   "giới hạn tài nguyên"),
    ],
    examples: &[
        ("docker run --rm -it alpine sh",               "shell tạm, tự xoá"),
        ("docker run -d --name web -p 8080:80 nginx",   "nginx nền, expose 8080"),
        ("docker run -v $(pwd):/app -w /app node npm test", "test với mount cwd"),
        ("docker run --env-file .env -d my_api:latest", "env từ file"),
    ],
    note: "`run` = `create` + `start`. Sau khi exit → container vẫn tồn tại (trừ khi có `--rm`).",
    skip_run: true,
};

const DOCKER_BUILD: Explanation = Explanation {
    name: "docker build",
    summary: "Build image từ Dockerfile",
    usage: "docker build [-t NAME:TAG] [-f Dockerfile] [build-args] <context>",
    flags: &[
        ("-t NAME:TAG",       "đặt tên + tag (có thể lặp nhiều `-t`)"),
        ("-f Dockerfile",     "chỉ định Dockerfile khác file mặc định"),
        ("--build-arg K=V",   "truyền ARG vào build"),
        ("--target STAGE",    "chỉ build tới stage này (multi-stage)"),
        ("--no-cache",        "không dùng cache layer"),
        ("--pull",            "luôn pull base image mới nhất"),
        ("--platform PLAT",   "build cho platform khác (vd linux/arm64)"),
        ("--progress=plain",  "in log đầy đủ, không TUI"),
    ],
    examples: &[
        ("docker build -t my_api:1.0 .",                  "build từ ./Dockerfile, tag 1.0"),
        ("docker build -t my_api:1.0 -t my_api:latest .", "2 tag cùng lúc"),
        ("docker build -f docker/Dockerfile.prod .",      "file Dockerfile khác"),
        ("docker build --build-arg VERSION=1.2 .",        "truyền ARG"),
        ("docker build --platform linux/arm64 -t app .",  "cross-build"),
    ],
    note: "`<context>` là thư mục được gửi vào daemon — tránh để `.` lớn. Dùng `.dockerignore` để bỏ qua file không cần.",
    skip_run: true,
};

const DOCKER_IMAGES: Explanation = Explanation {
    name: "docker images",
    summary: "List image local",
    usage: "docker images [-a] [-q] [-f FILTER] [REPOSITORY[:TAG]]",
    flags: &[
        ("-a",                "kể cả intermediate layer"),
        ("-q",                "chỉ in image ID"),
        ("-f dangling=true",  "image lơ lửng (không tag, không ref)"),
        ("--format FMT",      "Go template"),
        ("--no-trunc",        "không cắt"),
    ],
    examples: &[
        ("docker images",                       "tất cả image"),
        ("docker images nginx",                 "chỉ nginx"),
        ("docker images -f dangling=true -q",   "ID image lơ lửng (để rmi)"),
    ],
    note: "Bí danh: `docker image ls`.\nXoá lơ lửng: `docker image prune` hoặc `docker rmi $(docker images -f dangling=true -q)`.",
    skip_run: false,
};

const DOCKER_PULL: Explanation = Explanation {
    name: "docker pull",
    summary: "Tải image từ registry",
    usage: "docker pull [OPTIONS] IMAGE[:TAG|@DIGEST]",
    flags: &[
        ("--platform PLAT", "tải bản cho platform khác (vd linux/amd64 trên Mac M1)"),
        ("-a",              "tải MỌI tag của image (cẩn thận!)"),
        ("--quiet",         "giảm log"),
    ],
    examples: &[
        ("docker pull nginx",           "tag mặc định 'latest'"),
        ("docker pull nginx:1.25",      "tag cụ thể"),
        ("docker pull ghcr.io/owner/repo:sha", "registry private"),
        ("docker pull --platform linux/amd64 mysql:8", "ép platform (Mac M1)"),
    ],
    note: "Đăng nhập registry private trước: `docker login <registry>`.",
    skip_run: true,
};

const DOCKER_PUSH: Explanation = Explanation {
    name: "docker push",
    summary: "Đẩy image lên registry",
    usage: "docker push IMAGE[:TAG]",
    flags: &[
        ("--all-tags / -a", "push tất cả tag của image"),
        ("--quiet",         "giảm log"),
    ],
    examples: &[
        ("docker push my_user/my_api:1.0",          "Docker Hub"),
        ("docker push ghcr.io/owner/api:latest",    "GitHub Container Registry"),
    ],
    note: "Image phải đã được `docker login` vào registry tương ứng.\nĐặt tag matches registry: vd `ghcr.io/<owner>/<name>:<tag>`.",
    skip_run: true,
};

const DOCKER_LOGS: Explanation = Explanation {
    name: "docker logs",
    summary: "Xem log container",
    usage: "docker logs [-f] [--tail N] [-t] [--since TIME] <container>",
    flags: &[
        ("-f / --follow",       "follow log (như tail -f)"),
        ("--tail N",            "chỉ N dòng cuối (default: all)"),
        ("-t / --timestamps",   "in timestamp"),
        ("--since 10m",         "log từ 10 phút trước"),
        ("--since 2025-01-01",  "log từ ngày cụ thể"),
        ("--until TIME",        "cho đến TIME"),
        ("--details",           "kèm extra label"),
    ],
    examples: &[
        ("docker logs my_api",              "tất cả log"),
        ("docker logs -f --tail 100 web",   "100 dòng cuối + follow"),
        ("docker logs --since 1h db",       "1 giờ gần đây"),
        ("docker logs -t web 2>&1 | grep ERROR", "lọc lỗi (cả stdout + stderr)"),
    ],
    note: "Log = stdout + stderr của process chính trong container. Application ghi vào file bên trong sẽ KHÔNG hiện ở đây.",
    skip_run: true,
};

const DOCKER_LIFECYCLE: Explanation = Explanation {
    name: "docker stop / start / restart / kill / pause / unpause",
    summary: "Quản lý vòng đời container",
    usage: "docker <stop|start|restart|kill|pause|unpause> <container ...>",
    flags: &[
        ("stop",           "gửi SIGTERM rồi chờ 10s, sau đó SIGKILL"),
        ("stop -t SECS",   "đợi SECS thay vì 10s"),
        ("start",          "khởi động lại container đã stop"),
        ("start -a",       "+ attach stdout/stderr"),
        ("start -i",       "+ attach stdin (interactive)"),
        ("restart",        "= stop + start"),
        ("restart -t SECS","đợi SECS trước khi kill"),
        ("kill",           "gửi SIGKILL ngay (không lịch sự)"),
        ("kill -s SIGNAL", "gửi signal khác (vd HUP, USR1)"),
        ("pause / unpause","đóng băng / mở băng process (SIGSTOP/SIGCONT)"),
    ],
    examples: &[
        ("docker stop my_api",                  "dừng lịch sự"),
        ("docker stop -t 30 my_api",            "đợi 30s trước khi force"),
        ("docker restart $(docker ps -q)",      "restart mọi container đang chạy"),
        ("docker kill -s HUP nginx",            "gửi SIGHUP cho nginx reload config"),
    ],
    note: "Container exit khi process chính exit. `start` chỉ chạy lại — KHÔNG tạo mới.",
    skip_run: true,
};

const DOCKER_RM: Explanation = Explanation {
    name: "docker rm",
    summary: "Xoá container (đã stop)",
    usage: "docker rm [-f] [-v] <container ...>",
    flags: &[
        ("-f / --force",  "force xoá kể cả container đang chạy (= stop + rm)"),
        ("-v / --volumes","xoá luôn anonymous volume gắn với container"),
        ("-l / --link",   "xoá link, không xoá container"),
    ],
    examples: &[
        ("docker rm web db",                          "xoá 2 container"),
        ("docker rm -f $(docker ps -aq)",             "force xoá MỌI container (cẩn thận!)"),
        ("docker rm $(docker ps -q -f status=exited)","xoá container đã exit"),
        ("docker rm -v abc123",                       "xoá + volume anonymous"),
    ],
    note: "Container chạy: phải `docker stop` trước hoặc dùng `-f`.\nLưu trữ trong volume named KHÔNG bị xoá theo (chỉ anonymous).",
    skip_run: true,
};

const DOCKER_RMI: Explanation = Explanation {
    name: "docker rmi",
    summary: "Xoá image local",
    usage: "docker rmi [-f] <image ...>",
    flags: &[
        ("-f / --force",  "force — kể cả khi có container tag từ image này"),
        ("--no-prune",    "không xoá parent layer chưa tag"),
    ],
    examples: &[
        ("docker rmi nginx:1.20",                              "xoá 1 image"),
        ("docker rmi $(docker images -q -f dangling=true)",    "xoá image lơ lửng"),
        ("docker image prune",                                  "(khuyên hơn) xoá lơ lửng tự động"),
        ("docker image prune -a",                               "xoá MỌI image không có container đang dùng"),
    ],
    note: "Bí danh: `docker image rm`.",
    skip_run: true,
};

const DOCKER_INSPECT: Explanation = Explanation {
    name: "docker inspect",
    summary: "Metadata chi tiết (JSON) của container / image / volume / network",
    usage: "docker inspect [--format FMT] [-s] <object ...>",
    flags: &[
        ("--format FMT",  "Go template — trích trường cụ thể, gọn hơn JSON đầy đủ"),
        ("-s / --size",   "kèm size cho container"),
        ("--type TYPE",   "ép type: container/image/network/volume"),
    ],
    examples: &[
        ("docker inspect my_api",                                       "JSON đầy đủ"),
        ("docker inspect -f '{{.State.Status}}' my_api",                "chỉ status"),
        ("docker inspect -f '{{.NetworkSettings.IPAddress}}' my_api",   "chỉ IP"),
        ("docker inspect -f '{{.Config.Env}}' my_api",                  "env var của container"),
        ("docker inspect -f '{{json .Mounts}}' my_api | jq",            "mount points qua jq"),
    ],
    note: "Khám phá trường: `docker inspect <obj> | jq '. | keys'`.",
    skip_run: false,
};

const DOCKER_NETWORK: Explanation = Explanation {
    name: "docker network",
    summary: "Quản lý network",
    usage: "docker network <ls | create | rm | inspect | connect | disconnect | prune>",
    flags: &[
        ("ls",                       "list network"),
        ("create NAME",              "tạo network mới (default: bridge)"),
        ("create --driver overlay",  "swarm overlay network"),
        ("create --subnet 10.5.0.0/24 ", "custom subnet"),
        ("rm NAME",                  "xoá"),
        ("inspect NAME",             "chi tiết"),
        ("connect NET CONTAINER",    "gắn container vào network"),
        ("disconnect NET CONTAINER", "gỡ container ra"),
        ("prune",                    "xoá network không dùng"),
    ],
    examples: &[
        ("docker network ls",                              "list"),
        ("docker network create app_net",                  "tạo network"),
        ("docker run --network app_net --name db postgres","container vào network"),
        ("docker network inspect app_net",                 "chi tiết + container đang ở đó"),
    ],
    note: "Container cùng network thấy nhau qua tên (DNS). Default network `bridge` KHÔNG có DNS giữa các container.",
    skip_run: false,
};

const DOCKER_VOLUME: Explanation = Explanation {
    name: "docker volume",
    summary: "Quản lý volume (lưu trữ bền vững)",
    usage: "docker volume <ls | create | rm | inspect | prune>",
    flags: &[
        ("ls",            "list volume"),
        ("create NAME",   "tạo named volume"),
        ("rm NAME",       "xoá"),
        ("inspect NAME",  "chi tiết (mountpoint, driver, …)"),
        ("prune",         "xoá volume không có container nào dùng"),
        ("-f label=K=V",  "lọc theo label"),
    ],
    examples: &[
        ("docker volume ls",                              "list"),
        ("docker volume create db_data",                  "tạo"),
        ("docker run -v db_data:/var/lib/postgresql postgres", "mount vào container"),
        ("docker volume inspect db_data",                 "xem mountpoint"),
        ("docker volume prune",                           "dọn không dùng"),
    ],
    note: "Khác bind-mount (`-v /host/path:/cont/path`): named volume do Docker quản lý, di động + dễ backup.",
    skip_run: false,
};

const DOCKER_COMPOSE: Explanation = Explanation {
    name: "docker compose",
    summary: "Quản lý stack multi-container (docker-compose.yml)",
    usage: "docker compose <up | down | ps | logs | exec | build | pull | restart | ...>",
    flags: &[
        ("up",             "tạo + chạy stack"),
        ("up -d",          "background"),
        ("up --build",     "build lại image trước khi chạy"),
        ("up --force-recreate", "tạo lại container kể cả không thay đổi"),
        ("down",           "stop + xoá container + network (giữ volume)"),
        ("down -v",        "xoá luôn volume"),
        ("ps",             "list service trong stack"),
        ("logs -f [SVC]",  "follow log (mặc định tất cả)"),
        ("exec SVC CMD",   "= docker exec, dùng tên service"),
        ("restart [SVC]",  "restart service"),
        ("build",          "chỉ build, không chạy"),
        ("pull",           "pull lại image"),
        ("config",         "validate + in compose file đã merge"),
        ("-f FILE",        "compose file khác mặc định"),
        ("-p NAME",        "đặt project name"),
        ("--profile NAME", "chạy service trong profile"),
    ],
    examples: &[
        ("docker compose up -d",            "start stack nền"),
        ("docker compose ps",               "list service"),
        ("docker compose logs -f api",      "follow log service 'api'"),
        ("docker compose exec db psql -U postgres", "psql trong service 'db'"),
        ("docker compose down -v",          "stop + xoá hết kể cả volume"),
        ("docker compose -f docker-compose.prod.yml up -d", "file riêng"),
    ],
    note: "Lệnh mới (compose v2) — tích hợp vào docker CLI.\nCũ: `docker-compose` (có dấu gạch) — cú pháp tương đương.\nMặc định đọc `docker-compose.yml` hoặc `compose.yml` ở cwd.",
    skip_run: true,
};

const DOCKER_CP: Explanation = Explanation {
    name: "docker cp",
    summary: "Copy file giữa container và host",
    usage: "docker cp [-a] <src> <dst>  (src/dst có dạng CONTAINER:PATH)",
    flags: &[
        ("-a / --archive", "giữ uid/gid/mtime"),
        ("-L",             "follow symlink"),
    ],
    examples: &[
        ("docker cp my_api:/var/log/app.log .",       "copy từ container ra host"),
        ("docker cp ./config.yml my_api:/etc/app/",   "copy từ host vào container"),
        ("docker cp my_api:/data ./backup",           "copy cả thư mục"),
    ],
    note: "Container không cần đang chạy — `docker cp` đọc/ghi trực tiếp filesystem.",
    skip_run: true,
};

const DOCKER_LOGIN: Explanation = Explanation {
    name: "docker login / logout",
    summary: "Đăng nhập / đăng xuất registry",
    usage: "docker login [REGISTRY] [-u USER] [-p PASS] | docker logout [REGISTRY]",
    flags: &[
        ("-u USER",        "username"),
        ("-p PASS",        "password (KHÔNG khuyên — hiện trong history; dùng stdin)"),
        ("--password-stdin", "đọc password từ stdin (an toàn)"),
        ("REGISTRY",       "default: docker.io. Khác: ghcr.io, registry.gitlab.com, …"),
    ],
    examples: &[
        ("docker login",                                                "Docker Hub interactive"),
        ("echo $GHCR_TOKEN | docker login ghcr.io -u alice --password-stdin", "GHCR an toàn"),
        ("docker logout ghcr.io",                                       "xoá credentials"),
    ],
    note: "Credentials lưu ở `~/.docker/config.json` (hoặc keychain trên macOS).",
    skip_run: true,
};

const DOCKER_SYSTEM: Explanation = Explanation {
    name: "docker system / docker prune",
    summary: "Thông tin hệ thống & dọn dẹp resource",
    usage: "docker system <df | info | prune | events>",
    flags: &[
        ("df",                "dung lượng image/container/volume/cache đang chiếm"),
        ("info",              "thông tin daemon (storage driver, version, …)"),
        ("prune",             "xoá: container đã stop + network không dùng + dangling image"),
        ("prune -a",          "xoá thêm: image không có container đang dùng"),
        ("prune --volumes",   "xoá luôn volume không dùng (CẨN THẬN — mất data!)"),
        ("events",            "stream realtime mọi sự kiện Docker"),
    ],
    examples: &[
        ("docker system df",                            "xem đang chiếm bao nhiêu"),
        ("docker system prune",                         "dọn cơ bản (an toàn)"),
        ("docker system prune -a --volumes",            "dọn tới đáy (nguy hiểm)"),
        ("docker image prune  /  docker container prune  /  docker volume prune", "dọn theo resource"),
    ],
    note: "Trên dev machine, `system prune` định kỳ giải phóng đáng kể dung lượng.",
    skip_run: true,
};

const DOCKER_TAG: Explanation = Explanation {
    name: "docker tag",
    summary: "Tạo tên/tag mới cho image (alias)",
    usage: "docker tag <source[:tag]> <target[:tag]>",
    flags: &[
        ("(không có cờ)", "chỉ truyền 2 tham số: image gốc + image đích"),
    ],
    examples: &[
        ("docker tag my_api:1.0 my_api:latest",        "alias 'latest'"),
        ("docker tag my_api:1.0 ghcr.io/me/my_api:1.0","đổi sang dạng registry"),
    ],
    note: "`tag` không tạo image mới — chỉ thêm 1 ref trỏ tới image gốc.\nSau tag cần `docker push` để đẩy lên registry.",
    skip_run: true,
};
