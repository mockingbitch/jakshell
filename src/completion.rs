use rustyline::completion::{Completer, Pair};
use rustyline::highlight::Highlighter;
use rustyline::hint::Hinter;
use rustyline::history::SearchDirection;
use rustyline::validate::Validator;
use rustyline::Context;
use rustyline_derive::Helper as DeriveHelper;
use std::borrow::Cow;
use std::cell::RefCell;
use std::rc::Rc;

use crate::shell::Shell;

// Icon prefixes for visual categorization in tab completion.
const ICON_BUILTIN:  &str = "⚙ ";   // built-in shell command
const ICON_ALIAS:    &str = "↪ ";   // alias
const ICON_JAK:      &str = "★ ";   // jak utility
const ICON_BOOKMARK: &str = "🔖 ";  // bookmark
const ICON_PATH:     &str = "$ ";   // PATH binary
const ICON_DIR:      &str = "📁 ";  // directory
const ICON_FILE:     &str = "📄 ";  // file
const ICON_EXEC:     &str = "▶ ";   // executable file
const ICON_DOCKER:   &str = "🐳 ";  // docker container/image
const ICON_GIT:        &str = "🔀 ";  // git subcommand
const ICON_GIT_BRANCH: &str = "⎇ ";   // git branch (local or remote-tracking)
const ICON_GIT_REMOTE: &str = "☁ ";   // git remote name
const ICON_GIT_TAG:    &str = "🏷 ";  // git tag

#[derive(DeriveHelper)]
pub struct ShellHelper {
    shell: Rc<RefCell<Shell>>,
}

impl ShellHelper {
    pub fn new(shell: Rc<RefCell<Shell>>) -> Self {
        Self { shell }
    }
}

impl Completer for ShellHelper {
    type Candidate = Pair;
    fn complete(&self, line: &str, pos: usize, _ctx: &Context<'_>) -> rustyline::Result<(usize, Vec<Pair>)> {
        let (start, frag) = current_word(line, pos);
        let first_word = is_first_word(line, start);
        let cwd = self.shell.borrow().cwd.clone();
        let mut results = Vec::new();

        if first_word {
            // Builtin shell commands
            for b in crate::builtins::BUILTINS {
                if b.starts_with(frag) {
                    results.push(Pair {
                        display: format!("{}{:<14}  \x1b[2mbuiltin\x1b[0m", ICON_BUILTIN, b),
                        replacement: b.to_string(),
                    });
                }
            }
            // User aliases
            for (k, v) in self.shell.borrow().aliases.iter() {
                if k.starts_with(frag) {
                    let preview: String = v.chars().take(40).collect();
                    results.push(Pair {
                        display: format!("{}{:<14}  \x1b[2m→ {}\x1b[0m", ICON_ALIAS, k, preview),
                        replacement: k.clone(),
                    });
                }
            }
            // Bookmarks (jak <name>)
            for (name, cmd) in crate::bookmark::list_all() {
                let full = format!("jak {}", name);
                if full.starts_with(frag) {
                    let preview: String = cmd.chars().take(40).collect();
                    results.push(Pair {
                        display: format!("{}{:<14}  \x1b[2m→ {}\x1b[0m", ICON_BOOKMARK, full, preview),
                        replacement: full,
                    });
                }
            }
            // jak subcommands
            for sub in ["clean", "backup", "update", "self-update", "version", "lang", "find", "open", "sysinfo", "theme", "weather", "ip", "help", "git"] {
                let full = format!("jak {}", sub);
                if full.starts_with(frag) {
                    results.push(Pair {
                        display: format!("{}{:<18}  \x1b[2mjak utility\x1b[0m", ICON_JAK, full),
                        replacement: full,
                    });
                }
            }
            for cmd in ["jak git save ", "jak git sync", "jak git wip", "jak git amend", "jak git undo", "jak git uncommit", "jak git publish", "jak git unstage ", "jak git clean-branches", "jak git help"] {
                if cmd.starts_with(frag) {
                    results.push(Pair {
                        display: format!("{}{:<22}  \x1b[2mgit workflow\x1b[0m", ICON_JAK, cmd.trim_end()),
                        replacement: cmd.to_string(),
                    });
                }
            }
            for cmd in ["explain ls", "explain ps", "explain df", "explain du", "explain free", "explain top", "explain chmod", "explain git status", "explain git log", "explain netstat", "explain ifconfig", "explain ip", "explain lsof"] {
                if cmd.starts_with(frag) {
                    results.push(Pair {
                        display: format!("{}{:<22}  \x1b[2mexplain\x1b[0m", ICON_JAK, cmd),
                        replacement: cmd.to_string(),
                    });
                }
            }
            for cmd in ["ls -la --jak", "ls -lh --jak", "ps aux --jak", "df -h --jak", "du -sh --jak"] {
                if cmd.starts_with(frag) {
                    results.push(Pair {
                        display: format!("{}{:<22}  \x1b[2mpretty output\x1b[0m", ICON_JAK, cmd),
                        replacement: cmd.to_string(),
                    });
                }
            }
            for cmd in ["jak find file ", "jak find dir ", "jak find text ", "jak find big", "jak find recent", "jak find empty", "jak find help"] {
                if cmd.starts_with(frag) {
                    results.push(Pair {
                        display: format!("{}{:<22}  \x1b[2msearch\x1b[0m", ICON_JAK, cmd.trim_end()),
                        replacement: cmd.to_string(),
                    });
                }
            }
            // PATH binaries
            for p in path_binaries(frag, 50) {
                results.push(Pair {
                    display: format!("{}{:<14}  \x1b[2mPATH\x1b[0m", ICON_PATH, p),
                    replacement: p,
                });
            }
        }

        // Completion theo ngữ cảnh lệnh (vd `docker exec <TAB>` → tên container).
        // Nếu có kết quả thì trả về luôn, không trộn lẫn với danh sách file/thư mục
        // trong cwd (tránh nhiễu).
        if !first_word {
            let dyn_results = dynamic_completions(line, start, frag, &cwd);
            if !dyn_results.is_empty() {
                let mut seen = std::collections::HashSet::new();
                let mut dr = dyn_results;
                dr.retain(|p| seen.insert(p.replacement.clone()));
                return Ok((start, dr));
            }
        }

        // Path completion — context-aware theo TÊN LỆNH đang gõ.
        let cmd = command_name(line, start);
        let filter = path_filter_for(&cmd);
        let path_results = path_complete(frag, &cwd, filter);
        results.extend(path_results);

        // Deduplicate by replacement
        let mut seen = std::collections::HashSet::new();
        results.retain(|p| seen.insert(p.replacement.clone()));

        Ok((start, results))
    }
}

impl Hinter for ShellHelper {
    type Hint = String;

    /// Trả về gợi ý inline (ghost text) khi user đang gõ.
    /// Ưu tiên: (1) history khớp prefix, (2) builtin/alias/jak subcommand/bookmark.
    /// Chỉ gợi ý khi cursor ở cuối dòng & dòng không rỗng.
    fn hint(&self, line: &str, pos: usize, ctx: &Context<'_>) -> Option<String> {
        if line.is_empty() || pos != line.len() {
            return None;
        }
        // Không gợi ý nếu kết thúc bằng space (user đã gõ xong từ)
        if line.ends_with(' ') {
            return None;
        }

        // 1. History (most-recent matching prefix)
        let history = ctx.history();
        let len = history.len();
        for i in (0..len).rev() {
            if let Ok(Some(result)) = history.get(i, SearchDirection::Forward) {
                let entry: &str = result.entry.as_ref();
                if entry.starts_with(line) && entry.len() > line.len() {
                    return Some(entry[line.len()..].to_string());
                }
            }
        }

        // Cho việc gợi ý builtin/alias/etc: chỉ khi đang gõ TỪ ĐẦU TIÊN
        if line.contains(' ') {
            return None;
        }

        // 2. Builtin commands
        for b in crate::builtins::BUILTINS {
            if b.starts_with(line) && b.len() > line.len() {
                return Some(b[line.len()..].to_string());
            }
        }
        // 3. Aliases
        let aliases: Vec<String> = self.shell.borrow().aliases.keys().cloned().collect();
        for k in &aliases {
            if k.starts_with(line) && k.len() > line.len() {
                return Some(k[line.len()..].to_string());
            }
        }
        // 4. "jak " prefix to encourage discovery
        if "jak".starts_with(line) && line.len() < 3 {
            return Some("jak".to_string()[line.len()..].to_string());
        }
        None
    }
}

impl Highlighter for ShellHelper {
    fn highlight_prompt<'b, 's: 'b, 'p: 'b>(&'s self, prompt: &'p str, _default: bool) -> Cow<'b, str> {
        Cow::Borrowed(prompt)
    }
    fn highlight_hint<'h>(&self, hint: &'h str) -> Cow<'h, str> {
        Cow::Owned(format!("\x1b[2m{}\x1b[0m", hint))
    }
}

impl Validator for ShellHelper {}

fn current_word(line: &str, pos: usize) -> (usize, &str) {
    let bytes = line.as_bytes();
    let mut start = pos;
    while start > 0 {
        let c = bytes[start - 1] as char;
        if c.is_whitespace() || matches!(c, '|' | '&' | ';' | '<' | '>') {
            break;
        }
        start -= 1;
    }
    (start, &line[start..pos])
}

fn is_first_word(line: &str, start: usize) -> bool {
    let before = &line[..start];
    let trimmed = before.trim_end();
    trimmed.is_empty()
        || trimmed.ends_with('|')
        || trimmed.ends_with('&')
        || trimmed.ends_with(';')
        || trimmed.ends_with("&&")
        || trimmed.ends_with("||")
}

/// Lọc kết quả path completion theo loại lệnh đang gõ.
#[derive(Clone, Copy, PartialEq, Eq)]
enum PathFilter {
    /// Chỉ thư mục (cho `cd`, `pushd`, `rmdir`, …)
    DirsOnly,
    /// File + thư mục (mặc định — vẫn cho user navigate)
    Both,
}

/// Lấy tên lệnh (token đầu tiên) trước vị trí `start`.
/// Trả về "" nếu `start` chính là đầu dòng (đang gõ lệnh đầu tiên).
fn command_name(line: &str, start: usize) -> String {
    let before = &line[..start];
    // Tìm vị trí "đầu lệnh hiện tại" — sau | && || ; & gần nhất.
    let mut anchor = 0usize;
    let bytes = before.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        if c == b'|' || c == b'&' || c == b';' {
            // Sau ký tự này là lệnh mới
            // Skip cả `&&` và `||` (2 ký tự)
            if i + 1 < bytes.len() && (bytes[i + 1] == c) {
                i += 2;
            } else {
                i += 1;
            }
            anchor = i;
        } else {
            i += 1;
        }
    }
    let segment = &before[anchor..];
    let trimmed = segment.trim_start();
    trimmed.split_whitespace().next().unwrap_or("").to_string()
}

fn path_filter_for(cmd: &str) -> PathFilter {
    match cmd {
        "cd" | "pushd" | "popd" | "rmdir" | "chdir" => PathFilter::DirsOnly,
        _ => PathFilter::Both,
    }
}

fn path_complete(frag: &str, cwd: &std::path::Path, filter: PathFilter) -> Vec<Pair> {
    let expanded = shellexpand::tilde(frag).to_string();
    let path = std::path::Path::new(&expanded);
    let (dir, prefix) = if expanded.ends_with('/') || expanded.is_empty() {
        (path.to_path_buf(), String::new())
    } else if let Some(parent) = path.parent() {
        let name = path.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        if parent.as_os_str().is_empty() {
            (std::path::PathBuf::from("."), name)
        } else {
            (parent.to_path_buf(), name)
        }
    } else {
        (std::path::PathBuf::from("."), expanded.clone())
    };

    let scan_dir = if dir.is_absolute() { dir.clone() } else { cwd.join(&dir) };
    let mut out = Vec::new();
    if let Ok(it) = std::fs::read_dir(&scan_dir) {
        for entry in it.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if !name.starts_with(&prefix) {
                continue;
            }
            if prefix.is_empty() && name.starts_with('.') {
                continue;
            }
            let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
            // Áp filter
            if filter == PathFilter::DirsOnly && !is_dir {
                continue;
            }
            let is_exec = if is_dir {
                false
            } else {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    entry.metadata().map(|m| m.permissions().mode() & 0o111 != 0).unwrap_or(false)
                }
                #[cfg(not(unix))]
                { false }
            };
            let icon = if is_dir { ICON_DIR } else if is_exec { ICON_EXEC } else { ICON_FILE };
            let label = if is_dir { format!("{}/", name) } else { name.clone() };
            let display = format!("{}{}", icon, label);
            let replacement = if dir.as_os_str().is_empty() || dir == std::path::Path::new(".") {
                if is_dir { format!("{}/", name) } else { name }
            } else {
                let base = dir.to_string_lossy().to_string();
                let sep = if base.ends_with('/') { "" } else { "/" };
                if is_dir {
                    format!("{}{}{}/", base, sep, label.trim_end_matches('/'))
                } else {
                    format!("{}{}{}", base, sep, label)
                }
            };
            out.push(Pair { display, replacement });
        }
    }
    out
}

/// Các từ (đã tách theo whitespace) của segment lệnh hiện tại, TRƯỚC vị trí
/// `start` — tức là không gồm fragment đang gõ dở. "segment" = phần sau dấu
/// `| & ; && ||` gần nhất. Dùng để hiểu ngữ cảnh (lệnh gì, subcommand nào, đã
/// có mấy positional argument).
fn segment_words(line: &str, start: usize) -> Vec<String> {
    let before = &line[..start];
    let bytes = before.as_bytes();
    let mut anchor = 0usize;
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        if c == b'|' || c == b'&' || c == b';' {
            if i + 1 < bytes.len() && bytes[i + 1] == c {
                i += 2;
            } else {
                i += 1;
            }
            anchor = i;
        } else {
            i += 1;
        }
    }
    before[anchor..].split_whitespace().map(|s| s.to_string()).collect()
}

/// Completion theo ngữ cảnh lệnh. Trả về rỗng nếu không áp dụng (rơi về path).
fn dynamic_completions(line: &str, start: usize, frag: &str, cwd: &std::path::Path) -> Vec<Pair> {
    let words = segment_words(line, start);
    let Some(cmd) = words.first() else { return Vec::new() };
    match cmd.as_str() {
        "docker" | "podman" => docker_completions(cmd, &words, frag),
        "git" => git_completions(&words, frag, cwd),
        _ => Vec::new(),
    }
}

/// `docker <sub> ...` — gợi ý tên container (hoặc image) tuỳ subcommand.
fn docker_completions(bin: &str, words: &[String], frag: &str) -> Vec<Pair> {
    // Đang chỉ gõ flag (vd `-it`) thì không gợi ý gì.
    if frag.starts_with('-') {
        return Vec::new();
    }
    // subcommand = positional đầu tiên sau "docker" (bỏ qua các flag như -D, -H ...).
    let positionals: Vec<&String> = words
        .iter()
        .skip(1)
        .filter(|w| !w.starts_with('-'))
        .collect();
    let Some(sub) = positionals.first().map(|s| s.as_str()) else {
        // Chưa có subcommand → đang gõ chính subcommand đó.
        return docker_subcommand_pairs(frag);
    };
    // Số positional đã có SAU subcommand (chưa tính fragment đang gõ).
    let args_after_sub = positionals.len() - 1;

    // Subcommand thao tác trên container đang chạy → `docker ps`.
    const RUNNING: &[&str] = &[
        "exec", "attach", "logs", "top", "port", "stats", "pause",
        "unpause", "kill", "stop", "restart", "wait", "diff",
    ];
    // Subcommand thao tác trên mọi container (kể cả đã dừng) → `docker ps -a`.
    const ALL: &[&str] = &[
        "start", "rm", "inspect", "rename", "commit", "update", "export", "cp",
    ];
    // Subcommand nhận NHIỀU container (lặp lại) → gợi ý cho mọi positional.
    const MULTI: &[&str] = &[
        "start", "stop", "restart", "kill", "pause", "unpause", "rm", "wait",
    ];
    // Subcommand thao tác trên image → `docker images`.
    const IMAGES: &[&str] = &["run", "create", "rmi", "save", "tag", "push", "history"];

    let is_multi = MULTI.contains(&sub);
    // Với subcommand 1-container (exec, logs, ...): chỉ gợi ý ở positional đầu
    // (container). Với subcommand nhiều container: gợi ý ở mọi positional.
    let want_container = (RUNNING.contains(&sub) || ALL.contains(&sub))
        && (is_multi || args_after_sub == 0);

    if want_container {
        let all = ALL.contains(&sub);
        let names = docker_container_names(bin, all);
        return names
            .into_iter()
            .filter(|n| n.starts_with(frag))
            .map(|n| Pair {
                display: format!("{}{:<24}  \x1b[2mcontainer\x1b[0m", ICON_DOCKER, n),
                replacement: n,
            })
            .collect();
    }

    // image cho `docker run <TAB>` (positional đầu = image), `rmi`, ...
    if IMAGES.contains(&sub) && args_after_sub == 0 {
        let images = docker_image_names(bin);
        return images
            .into_iter()
            .filter(|n| n.starts_with(frag))
            .map(|n| Pair {
                display: format!("{}{:<24}  \x1b[2mimage\x1b[0m", ICON_DOCKER, n),
                replacement: n,
            })
            .collect();
    }

    Vec::new()
}

fn docker_subcommand_pairs(frag: &str) -> Vec<Pair> {
    const SUBS: &[&str] = &[
        "run", "exec", "ps", "build", "pull", "push", "images", "logs",
        "stop", "start", "restart", "rm", "rmi", "inspect", "attach",
        "kill", "pause", "unpause", "stats", "top", "port", "cp", "commit",
        "rename", "network", "volume", "compose", "system", "version", "info",
    ];
    SUBS.iter()
        .filter(|s| s.starts_with(frag))
        .map(|s| Pair {
            display: format!("{}{:<16}  \x1b[2mdocker\x1b[0m", ICON_DOCKER, s),
            replacement: s.to_string(),
        })
        .collect()
}

fn docker_container_names(bin: &str, all: bool) -> Vec<String> {
    let mut args: Vec<&str> = vec!["ps", "--format", "{{.Names}}"];
    if all {
        args.insert(1, "-a");
    }
    run_command_lines(bin, &args)
}

fn docker_image_names(bin: &str) -> Vec<String> {
    let out = run_command_lines(bin, &["images", "--format", "{{.Repository}}:{{.Tag}}"]);
    // Bỏ image <none>:<none> (dangling) cho gọn.
    out.into_iter().filter(|s| !s.contains("<none>")).collect()
}

// ───────────────────────────── git ──────────────────────────────
// `git <sub> ...` — gợi ý branch / remote / tag tuỳ subcommand.
// Ví dụ: `git push feat/<TAB>` → các branch bắt đầu bằng `feat/`.

/// Global option của git nhận 1 giá trị → cần bỏ qua cả token kế tiếp khi đi
/// tìm subcommand (vd `git -C /path status`).
const GIT_VALUE_GLOBALS: &[&str] = &[
    "-C", "-c", "--git-dir", "--work-tree", "--namespace", "--exec-path",
];

/// Flag tạo branch MỚI cho `switch`/`checkout` — sau nó user gõ TÊN MỚI nên
/// không gợi ý branch đang có.
const GIT_NEW_BRANCH_FLAGS: &[&str] = &["-c", "-C", "-b", "-B"];

fn git_completions(words: &[String], frag: &str, cwd: &std::path::Path) -> Vec<Pair> {
    // Đang gõ flag (`-`, `--`) → không gợi ý ref (rơi về path/no-op).
    if frag.starts_with('-') {
        return Vec::new();
    }

    // Tìm subcommand: positional đầu sau "git", bỏ qua global flag (và giá trị
    // của các global nhận tham số).
    let mut idx = 1;
    let mut sub: Option<&str> = None;
    let mut sub_idx = 0usize;
    while idx < words.len() {
        let w = words[idx].as_str();
        if GIT_VALUE_GLOBALS.contains(&w) {
            idx += 2; // bỏ qua cả flag lẫn giá trị
            continue;
        }
        if w.starts_with('-') {
            idx += 1;
            continue;
        }
        sub = Some(w);
        sub_idx = idx;
        break;
    }

    // Chưa có subcommand → đang gõ chính subcommand đó.
    let Some(sub) = sub else {
        return git_subcommand_pairs(frag, cwd);
    };

    let after = &words[sub_idx + 1..];
    // Sau `--` mọi thứ là pathspec → để path completion xử lý file.
    if after.iter().any(|w| w == "--") {
        return Vec::new();
    }
    // Token ngay trước fragment (để bắt `switch -c <tên mới>`).
    let prev = words.last().map(|s| s.as_str()).unwrap_or("");
    // Positional đã gõ SAU subcommand (chưa tính fragment đang gõ).
    let post: Vec<&str> = after.iter().map(|s| s.as_str()).filter(|w| !w.starts_with('-')).collect();
    let nargs = post.len();

    match sub {
        // Thao tác trên file → để path completion gợi ý file, không chen ref vào.
        "add" | "rm" | "mv" | "restore" | "stage" | "clean" | "apply" => Vec::new(),

        // Chuyển/đổi branch: branch local + remote-tracking + tag (DWIM).
        "checkout" | "switch" => {
            if GIT_NEW_BRANCH_FLAGS.contains(&prev) {
                return Vec::new(); // đang đặt tên branch mới
            }
            let mut out = ref_pairs(git_local_branches(cwd), frag, ICON_GIT_BRANCH, "branch");
            out.extend(ref_pairs(git_remote_branches(cwd), frag, ICON_GIT_BRANCH, "remote branch"));
            out.extend(ref_pairs(git_tags(cwd), frag, ICON_GIT_TAG, "tag"));
            out
        }

        // `git branch -d/-m/--merged ...` → branch local; thêm remote nếu có -r/-a.
        "branch" => {
            let mut out = ref_pairs(git_local_branches(cwd), frag, ICON_GIT_BRANCH, "branch");
            if has_flag(words, "-r") || has_flag(words, "--remotes")
                || has_flag(words, "-a") || has_flag(words, "--all")
            {
                out.extend(ref_pairs(git_remote_branches(cwd), frag, ICON_GIT_BRANCH, "remote branch"));
            }
            out
        }

        // Lệnh nhận ref bất kỳ (branch / remote-tracking / tag).
        "merge" | "rebase" | "cherry-pick" | "revert" | "reset" | "log" | "diff"
        | "show" | "describe" => {
            let mut out = ref_pairs(git_local_branches(cwd), frag, ICON_GIT_BRANCH, "branch");
            out.extend(ref_pairs(git_remote_branches(cwd), frag, ICON_GIT_BRANCH, "remote branch"));
            out.extend(ref_pairs(git_tags(cwd), frag, ICON_GIT_TAG, "tag"));
            out
        }

        // `git push <remote> <branch>`: positional đầu là remote, sau đó là branch.
        // Gõ `feat/` sẽ chỉ khớp branch (remote như `origin` không khớp) — đúng ý
        // user: `git push feat/<TAB>` → các branch `feat/...`.
        "push" => {
            if nargs == 0 {
                let mut out = ref_pairs(git_remotes(cwd), frag, ICON_GIT_REMOTE, "remote");
                out.extend(ref_pairs(git_local_branches(cwd), frag, ICON_GIT_BRANCH, "branch"));
                out
            } else {
                ref_pairs(git_local_branches(cwd), frag, ICON_GIT_BRANCH, "branch")
            }
        }

        // `git pull/fetch <remote> <branch>`: remote ở positional đầu; positional
        // sau là branch PHÍA REMOTE (đã bỏ tiền tố `<remote>/`).
        "pull" | "fetch" => {
            if nargs == 0 {
                ref_pairs(git_remotes(cwd), frag, ICON_GIT_REMOTE, "remote")
            } else {
                let remote_prefix = format!("{}/", post[0]);
                let mut names: Vec<String> = git_remote_branches(cwd)
                    .into_iter()
                    .filter_map(|b| b.strip_prefix(&remote_prefix).map(|s| s.to_string()))
                    .collect();
                if names.is_empty() {
                    names = git_local_branches(cwd); // fallback nếu remote chưa có nhánh nào khớp
                }
                ref_pairs(names, frag, ICON_GIT_BRANCH, "branch")
            }
        }

        // `git remote <sub> [name]`.
        "remote" => {
            if nargs == 0 {
                static_pairs(
                    &["add", "remove", "rename", "set-url", "get-url", "set-head",
                      "set-branches", "show", "prune", "update"],
                    frag, ICON_GIT, "remote cmd",
                )
            } else if matches!(post[0],
                "remove" | "rename" | "set-url" | "get-url" | "set-head" | "set-branches" | "show" | "prune")
            {
                ref_pairs(git_remotes(cwd), frag, ICON_GIT_REMOTE, "remote")
            } else {
                Vec::new()
            }
        }

        // `git stash <sub>`.
        "stash" => {
            if nargs == 0 {
                static_pairs(
                    &["list", "show", "pop", "apply", "drop", "push", "branch",
                      "clear", "create", "store"],
                    frag, ICON_GIT, "stash cmd",
                )
            } else {
                Vec::new()
            }
        }

        // `git worktree <sub>`.
        "worktree" => {
            if nargs == 0 {
                static_pairs(
                    &["add", "list", "lock", "unlock", "move", "prune", "remove", "repair"],
                    frag, ICON_GIT, "worktree cmd",
                )
            } else {
                Vec::new()
            }
        }

        // `git tag [-d] <tag>`.
        "tag" => ref_pairs(git_tags(cwd), frag, ICON_GIT_TAG, "tag"),

        // Subcommand khác → không gợi ý đặc thù (rơi về path).
        _ => Vec::new(),
    }
}

/// Gợi ý tên subcommand của git: danh sách phổ biến + alias do user định nghĩa
/// trong `git config`.
fn git_subcommand_pairs(frag: &str, cwd: &std::path::Path) -> Vec<Pair> {
    const SUBS: &[&str] = &[
        "status", "add", "commit", "push", "pull", "fetch", "clone", "init",
        "branch", "checkout", "switch", "merge", "rebase", "log", "diff", "show",
        "stash", "tag", "remote", "reset", "revert", "restore", "cherry-pick",
        "mv", "rm", "clean", "config", "reflog", "blame", "bisect", "worktree",
        "describe", "shortlog", "apply", "format-patch", "am", "submodule",
        "sparse-checkout", "gc", "fsck",
    ];
    let mut out: Vec<Pair> = SUBS
        .iter()
        .filter(|s| s.starts_with(frag))
        .map(|s| Pair {
            display: format!("{}{:<16}  \x1b[2mgit\x1b[0m", ICON_GIT, s),
            replacement: s.to_string(),
        })
        .collect();
    // Alias người dùng: `git config --get-regexp ^alias.` → `alias.co checkout`.
    for line in run_git_lines(cwd, &["config", "--get-regexp", "^alias\\."]) {
        if let Some(name) = line.split_whitespace().next().and_then(|k| k.strip_prefix("alias.")) {
            if name.starts_with(frag) {
                out.push(Pair {
                    display: format!("{}{:<16}  \x1b[2mgit alias\x1b[0m", ICON_GIT, name),
                    replacement: name.to_string(),
                });
            }
        }
    }
    out
}

fn has_flag(words: &[String], flag: &str) -> bool {
    words.iter().any(|w| w == flag)
}

/// Dựng Pair từ danh sách ref/remote, lọc theo prefix `frag`.
fn ref_pairs(names: Vec<String>, frag: &str, icon: &str, label: &str) -> Vec<Pair> {
    names
        .into_iter()
        .filter(|n| n.starts_with(frag))
        .map(|n| Pair {
            display: format!("{}{:<28}  \x1b[2m{}\x1b[0m", icon, n, label),
            replacement: n,
        })
        .collect()
}

/// Dựng Pair từ danh sách subcommand tĩnh, lọc theo prefix `frag`.
fn static_pairs(items: &[&str], frag: &str, icon: &str, label: &str) -> Vec<Pair> {
    items
        .iter()
        .filter(|s| s.starts_with(frag))
        .map(|s| Pair {
            display: format!("{}{:<16}  \x1b[2m{}\x1b[0m", icon, s, label),
            replacement: s.to_string(),
        })
        .collect()
}

fn git_local_branches(cwd: &std::path::Path) -> Vec<String> {
    run_git_lines(cwd, &["for-each-ref", "--format=%(refname:short)", "refs/heads"])
}

fn git_remote_branches(cwd: &std::path::Path) -> Vec<String> {
    run_git_lines(cwd, &["for-each-ref", "--format=%(refname:short)", "refs/remotes"])
        .into_iter()
        // Bỏ con trỏ HEAD của remote: `refs/remotes/origin/HEAD` được rút gọn
        // thành đúng `origin` (KHÔNG phải `origin/HEAD`), nên vừa loại tên
        // remote trần (không có `/`) vừa loại mọi `*/HEAD` cho chắc.
        .filter(|s| s.contains('/') && !s.ends_with("/HEAD"))
        .collect()
}

fn git_tags(cwd: &std::path::Path) -> Vec<String> {
    run_git_lines(cwd, &["for-each-ref", "--format=%(refname:short)", "refs/tags"])
}

fn git_remotes(cwd: &std::path::Path) -> Vec<String> {
    run_git_lines(cwd, &["remote"])
}

/// Như `run_command_lines` nhưng chạy `git` trong thư mục `cwd` của shell
/// (process cwd thường đã đồng bộ, nhưng truyền tường minh cho chắc + dễ test).
fn run_git_lines(cwd: &std::path::Path, args: &[&str]) -> Vec<String> {
    std::process::Command::new("git")
        .current_dir(cwd)
        .args(args)
        .stderr(std::process::Stdio::null())
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .map(|l| l.trim().to_string())
                .filter(|l| !l.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

/// Chạy lệnh ngoài, trả về các dòng stdout (đã trim, bỏ dòng rỗng).
/// Trả rỗng nếu lệnh fail / không tồn tại — completion sẽ rơi về path.
fn run_command_lines(bin: &str, args: &[&str]) -> Vec<String> {
    std::process::Command::new(bin)
        .args(args)
        .stderr(std::process::Stdio::null())
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .map(|l| l.trim().to_string())
                .filter(|l| !l.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

fn path_binaries(prefix: &str, limit: usize) -> Vec<String> {
    if prefix.is_empty() || prefix.contains('/') {
        return Vec::new();
    }
    let mut out = Vec::new();
    if let Some(path) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path) {
            if let Ok(it) = std::fs::read_dir(&dir) {
                for entry in it.flatten() {
                    let name = entry.file_name().to_string_lossy().to_string();
                    if name.starts_with(prefix) {
                        out.push(name);
                        if out.len() >= limit {
                            return out;
                        }
                    }
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn segment_words_basics() {
        assert_eq!(segment_words("docker exec -it ", 16), vec!["docker", "exec", "-it"]);
        // sau pipe → segment mới
        assert_eq!(segment_words("foo | docker exec ", 18), vec!["docker", "exec"]);
        // fragment đang gõ không được tính (start ở đầu fragment)
        assert_eq!(segment_words("docker ", 7), vec!["docker"]);
    }

    #[test]
    fn docker_suggests_subcommands_while_typing_sub() {
        let line = "docker ex";
        let (start, frag) = current_word(line, line.len());
        let r = dynamic_completions(line, start, frag, std::path::Path::new("."));
        assert!(r.iter().any(|p| p.replacement == "exec"),
            "phải gợi ý subcommand 'exec', nhận: {:?}",
            r.iter().map(|p| &p.replacement).collect::<Vec<_>>());
    }

    #[test]
    fn docker_no_suggest_for_flag_fragment() {
        let line = "docker exec -i";
        let (start, frag) = current_word(line, line.len());
        let r = dynamic_completions(line, start, frag, std::path::Path::new("."));
        assert!(r.is_empty(), "đang gõ flag thì không gợi ý: {:?}",
            r.iter().map(|p| &p.replacement).collect::<Vec<_>>());
    }

    // Chỉ chạy khi máy có docker + container đang chạy.
    #[test]
    fn docker_suggests_containers_after_exec() {
        if run_command_lines("docker", &["ps", "-q"]).is_empty() {
            eprintln!("skip: không có docker container đang chạy");
            return;
        }
        let line = "docker exec -it ";
        let (start, frag) = current_word(line, line.len());
        let r = dynamic_completions(line, start, frag, std::path::Path::new("."));
        assert!(!r.is_empty(), "phải gợi ý tên container sau `docker exec -it `");
        assert!(r.iter().all(|p| p.display.contains("container")));
    }

    // ───────────────────────── git completion ─────────────────────────

    fn git_available() -> bool {
        std::process::Command::new("git")
            .arg("--version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }

    /// Tạo repo tạm với nhánh `main` + vài nhánh `feat/*`, `bugfix/*`, 1 tag.
    fn setup_repo(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("jaksh-gittest-{}-{}", std::process::id(), tag));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let run = |args: &[&str]| {
            std::process::Command::new("git")
                .current_dir(&dir)
                // Cô lập khỏi config global/system của máy chạy test.
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .env("GIT_CONFIG_SYSTEM", "/dev/null")
                .env("GIT_AUTHOR_NAME", "t").env("GIT_AUTHOR_EMAIL", "t@t")
                .env("GIT_COMMITTER_NAME", "t").env("GIT_COMMITTER_EMAIL", "t@t")
                .args(args)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .unwrap();
        };
        run(&["init", "-q", "-b", "main"]);
        std::fs::write(dir.join("f.txt"), "x").unwrap();
        run(&["add", "-A"]);
        run(&["commit", "-q", "-m", "init"]);
        run(&["branch", "feat/create-api-list"]);
        run(&["branch", "feat/login"]);
        run(&["branch", "bugfix/typo"]);
        run(&["tag", "v1.0.0"]);
        dir
    }

    fn complete_git(line: &str, cwd: &std::path::Path) -> Vec<String> {
        let (start, frag) = current_word(line, line.len());
        git_completions(&segment_words(line, start), frag, cwd)
            .into_iter()
            .map(|p| p.replacement)
            .collect()
    }

    #[test]
    fn git_push_prefix_suggests_matching_branches() {
        if !git_available() { eprintln!("skip: không có git"); return; }
        let dir = setup_repo("push-prefix");
        // Kịch bản chính của user: `git push feat/<TAB>`.
        let r = complete_git("git push feat/", &dir);
        assert!(r.iter().any(|n| n == "feat/create-api-list"), "got {:?}", r);
        assert!(r.iter().any(|n| n == "feat/login"), "got {:?}", r);
        assert!(!r.iter().any(|n| n == "bugfix/typo"), "prefix filter hỏng: {:?}", r);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn git_switch_lists_all_local_branches() {
        if !git_available() { eprintln!("skip: không có git"); return; }
        let dir = setup_repo("switch-all");
        let r = complete_git("git switch ", &dir);
        for b in ["main", "feat/create-api-list", "feat/login", "bugfix/typo"] {
            assert!(r.iter().any(|n| n == b), "thiếu branch {}: {:?}", b, r);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn git_switch_new_branch_flag_suppresses_suggestions() {
        if !git_available() { eprintln!("skip: không có git"); return; }
        let dir = setup_repo("switch-c");
        // `git switch -c <tên mới>` → không gợi ý branch đang có.
        assert!(complete_git("git switch -c ", &dir).is_empty());
        assert!(complete_git("git checkout -b ", &dir).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn git_push_second_positional_is_branch_not_remote() {
        if !git_available() { eprintln!("skip: không có git"); return; }
        let dir = setup_repo("push-refspec");
        let r = complete_git("git push origin feat/", &dir);
        assert!(r.iter().any(|n| n == "feat/login"), "got {:?}", r);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn git_double_dash_falls_back_to_path() {
        if !git_available() { eprintln!("skip: không có git"); return; }
        let dir = setup_repo("dashdash");
        // Sau `--` là pathspec → git_completions trả rỗng (path completion lo).
        assert!(complete_git("git checkout -- f", &dir).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn git_tag_lists_tags() {
        if !git_available() { eprintln!("skip: không có git"); return; }
        let dir = setup_repo("tags");
        let r = complete_git("git tag -d v", &dir);
        assert!(r.iter().any(|n| n == "v1.0.0"), "got {:?}", r);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn git_remote_branches_excludes_phantom_origin_head() {
        if !git_available() { eprintln!("skip: không có git"); return; }
        let dir = setup_repo("remote-head");
        // Dựng remote-tracking ref + con trỏ origin/HEAD (như sau khi clone).
        let head = String::from_utf8_lossy(
            &std::process::Command::new("git").current_dir(&dir)
                .args(["rev-parse", "HEAD"]).output().unwrap().stdout,
        ).trim().to_string();
        let run = |args: &[&str]| {
            std::process::Command::new("git").current_dir(&dir).args(args)
                .stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null())
                .status().unwrap();
        };
        run(&["update-ref", "refs/remotes/origin/main", &head]);
        run(&["update-ref", "refs/remotes/origin/feat/remote-only", &head]);
        run(&["symbolic-ref", "refs/remotes/origin/HEAD", "refs/remotes/origin/main"]);

        let branches = git_remote_branches(&dir);
        assert!(branches.iter().any(|b| b == "origin/main"), "got {:?}", branches);
        assert!(branches.iter().any(|b| b == "origin/feat/remote-only"), "got {:?}", branches);
        // Bug đã sửa: `refs/remotes/origin/HEAD` rút gọn thành `origin` — KHÔNG
        // được lọt vào danh sách "remote branch".
        assert!(!branches.iter().any(|b| b == "origin"), "phantom 'origin' leaked: {:?}", branches);
        assert!(!branches.iter().any(|b| b.ends_with("/HEAD")), "got {:?}", branches);

        // Và qua completion: `git checkout <TAB>` không gợi ý `origin` trần.
        let r = complete_git("git checkout ", &dir);
        assert!(!r.iter().any(|n| n == "origin"), "checkout gợi ý 'origin': {:?}", r);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn git_suggests_subcommands_by_prefix() {
        // Danh sách tĩnh — không cần repo.
        let r = complete_git("git pus", std::path::Path::new("."));
        assert!(r.iter().any(|n| n == "push"), "got {:?}", r);
        let r2 = complete_git("git ", std::path::Path::new("."));
        assert!(r2.iter().any(|n| n == "status"), "got {:?}", r2);
    }

    #[test]
    fn git_remote_subcommands() {
        let r = complete_git("git remote ", std::path::Path::new("."));
        assert!(r.iter().any(|n| n == "add"), "got {:?}", r);
        assert!(r.iter().any(|n| n == "remove"), "got {:?}", r);
    }

    #[test]
    fn git_flag_fragment_no_suggestions() {
        let dir_dot = std::path::Path::new(".");
        assert!(complete_git("git push -", dir_dot).is_empty());
        assert!(complete_git("git switch --fo", dir_dot).is_empty());
    }
}
