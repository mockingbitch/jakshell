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
            let dyn_results = dynamic_completions(line, start, frag);
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
        let path_results = path_complete(frag, &self.shell.borrow().cwd, filter);
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
fn dynamic_completions(line: &str, start: usize, frag: &str) -> Vec<Pair> {
    let words = segment_words(line, start);
    let Some(cmd) = words.first() else { return Vec::new() };
    match cmd.as_str() {
        "docker" | "podman" => docker_completions(cmd, &words, frag),
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
        let r = dynamic_completions(line, start, frag);
        assert!(r.iter().any(|p| p.replacement == "exec"),
            "phải gợi ý subcommand 'exec', nhận: {:?}",
            r.iter().map(|p| &p.replacement).collect::<Vec<_>>());
    }

    #[test]
    fn docker_no_suggest_for_flag_fragment() {
        let line = "docker exec -i";
        let (start, frag) = current_word(line, line.len());
        let r = dynamic_completions(line, start, frag);
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
        let r = dynamic_completions(line, start, frag);
        assert!(!r.is_empty(), "phải gợi ý tên container sau `docker exec -it `");
        assert!(r.iter().all(|p| p.display.contains("container")));
    }
}
