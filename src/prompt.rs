use crate::shell::Shell;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

pub fn render(shell: &Shell) -> String {
    let theme = &shell.theme;
    let template = shell.prompt_template.clone();

    let cwd = shell.cwd.display().to_string();
    let cwd_short = shorten_cwd(&cwd);
    let git = git_segment(&shell.cwd, theme);
    let status = shell.last_status();
    let arrow = if status == 0 {
        format!("{}{}{}", theme.success_ansi(), theme.arrow, "\x1b[0m")
    } else {
        format!("{}{}{}", theme.error_ansi(), theme.arrow, "\x1b[0m")
    };

    template
        .replace("{accent}", theme.accent_ansi())
        .replace("{success}", theme.success_ansi())
        .replace("{error}", theme.error_ansi())
        .replace("{dim}", theme.dim_ansi())
        .replace("{reset}", "\x1b[0m")
        .replace("{cwd_short}", &cwd_short)
        .replace("{cwd}", &cwd)
        .replace("{git}", &git)
        .replace("{arrow}", &arrow)
        .replace("{status}", &status.to_string())
}

fn shorten_cwd(cwd: &str) -> String {
    if let Some(home) = dirs::home_dir() {
        let h = home.display().to_string();
        if cwd == h {
            return "~".to_string();
        }
        if cwd.starts_with(&h) {
            return format!("~{}", &cwd[h.len()..]);
        }
    }
    cwd.to_string()
}

// ─── Git segment ──────────────────────────────────────────────────────────────

fn git_segment(cwd: &Path, theme: &crate::theme::Theme) -> String {
    let git_dir = match find_git_dir(cwd) {
        Some(d) => d,
        None => return String::new(),
    };

    // Read branch from .git/HEAD trước (cheap fallback nếu git status fail)
    let mut info = GitInfo::default();
    info.branch = read_head_branch(&git_dir).unwrap_or_else(|| "HEAD".to_string());

    // Gọi git status một lần để lấy mọi thứ.
    if let Some(status) = run_git_status(cwd) {
        info.merge(&status);
    }
    info.stash = count_stash(&git_dir);
    info.state = detect_state(&git_dir);

    render_git_info(&info, theme)
}

#[derive(Default, Debug)]
struct GitInfo {
    branch: String,
    detached: bool,
    ahead: i64,
    behind: i64,
    staged: u32,
    modified: u32,
    untracked: u32,
    conflict: u32,
    stash: u32,
    state: &'static str, // "MERGE" / "REBASE" / "PICK" / "REVERT" / "BISECT" / ""
}

impl GitInfo {
    fn merge(&mut self, other: &GitInfo) {
        if !other.branch.is_empty() {
            self.branch = other.branch.clone();
        }
        self.detached = other.detached;
        self.ahead = other.ahead;
        self.behind = other.behind;
        self.staged = other.staged;
        self.modified = other.modified;
        self.untracked = other.untracked;
        self.conflict = other.conflict;
    }
}

fn render_git_info(info: &GitInfo, theme: &crate::theme::Theme) -> String {
    if info.branch.is_empty() {
        return String::new();
    }
    let dim = theme.dim_ansi();
    let reset = if theme.use_color { "\x1b[0m" } else { "" };
    let yellow = if theme.use_color { "\x1b[33m" } else { "" };
    let red = if theme.use_color { "\x1b[31m" } else { "" };
    let green = if theme.use_color { "\x1b[32m" } else { "" };
    let cyan = if theme.use_color { "\x1b[36m" } else { "" };
    let magenta = if theme.use_color { "\x1b[35m" } else { "" };

    let mut out = String::new();
    out.push(' ');
    out.push_str(dim);
    out.push_str(&theme.git_branch_icon);
    out.push_str(reset);
    // Branch color: detached = magenta
    if info.detached {
        out.push_str(magenta);
    } else {
        out.push_str(cyan);
    }
    out.push_str(&info.branch);
    out.push_str(reset);

    // Dirty marker
    let dirty = info.staged + info.modified + info.untracked;
    if dirty > 0 {
        out.push(' ');
        out.push_str(yellow);
        out.push('*');
        out.push_str(reset);
    }
    // Conflict
    if info.conflict > 0 {
        out.push(' ');
        out.push_str(red);
        out.push_str(&format!("⚠{}", info.conflict));
        out.push_str(reset);
    }
    // Ahead / behind
    if info.ahead > 0 {
        out.push(' ');
        out.push_str(green);
        out.push_str(&format!("↑{}", info.ahead));
        out.push_str(reset);
    }
    if info.behind > 0 {
        out.push(' ');
        out.push_str(red);
        out.push_str(&format!("↓{}", info.behind));
        out.push_str(reset);
    }
    // Stash
    if info.stash > 0 {
        out.push(' ');
        out.push_str(magenta);
        out.push_str(&format!("⚑{}", info.stash));
        out.push_str(reset);
    }
    // State
    if !info.state.is_empty() {
        out.push(' ');
        out.push_str(red);
        out.push_str(info.state);
        out.push_str(reset);
    }
    out
}

fn find_git_dir(p: &Path) -> Option<PathBuf> {
    let mut cur: Option<&Path> = Some(p);
    while let Some(d) = cur {
        let g = d.join(".git");
        if g.exists() {
            // Có thể là file (worktree) hoặc dir — chấp nhận cả hai
            return Some(g);
        }
        cur = d.parent();
    }
    None
}

fn read_head_branch(git_dir: &Path) -> Option<String> {
    // .git có thể là file (linked worktree): "gitdir: <path>"
    let head_path = if git_dir.is_file() {
        let content = std::fs::read_to_string(git_dir).ok()?;
        let gitdir = content.trim().strip_prefix("gitdir: ")?;
        Path::new(gitdir).join("HEAD")
    } else {
        git_dir.join("HEAD")
    };
    let content = std::fs::read_to_string(&head_path).ok()?;
    let content = content.trim();
    if let Some(rest) = content.strip_prefix("ref: refs/heads/") {
        return Some(rest.to_string());
    }
    // Detached: short sha
    if content.len() >= 7 {
        return Some(format!("({})", &content[..7]));
    }
    None
}

fn run_git_status(cwd: &Path) -> Option<GitInfo> {
    let output = Command::new("git")
        .args(["-c", "color.ui=never",
               "status", "--branch", "--porcelain=v2", "-unormal"])
        .current_dir(cwd)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let mut info = GitInfo::default();
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("# branch.head ") {
            if rest == "(detached)" {
                info.detached = true;
            } else {
                info.branch = rest.to_string();
            }
        } else if let Some(rest) = line.strip_prefix("# branch.ab ") {
            // "+2 -3"
            let parts: Vec<&str> = rest.split_whitespace().collect();
            if parts.len() >= 2 {
                info.ahead = parts[0].trim_start_matches('+').parse().unwrap_or(0);
                info.behind = parts[1].trim_start_matches('-').parse().unwrap_or(0);
            }
        } else if line.starts_with("1 ") || line.starts_with("2 ") {
            // "1 XY ..." or "2 XY ..."
            if line.len() >= 4 {
                let xy = &line[2..4];
                let bytes = xy.as_bytes();
                if bytes.len() >= 2 {
                    if bytes[0] != b'.' && bytes[0] != b' ' {
                        info.staged += 1;
                    }
                    if bytes[1] != b'.' && bytes[1] != b' ' {
                        info.modified += 1;
                    }
                }
            }
        } else if line.starts_with("? ") {
            info.untracked += 1;
        } else if line.starts_with("u ") {
            info.conflict += 1;
        }
    }
    Some(info)
}

fn count_stash(git_dir: &Path) -> u32 {
    // git_dir có thể là file (worktree) — resolve về real .git dir
    let real = if git_dir.is_file() {
        match std::fs::read_to_string(git_dir) {
            Ok(c) => {
                if let Some(rest) = c.trim().strip_prefix("gitdir: ") {
                    PathBuf::from(rest)
                } else {
                    return 0;
                }
            }
            Err(_) => return 0,
        }
    } else {
        git_dir.to_path_buf()
    };
    let log_path = real.join("logs/refs/stash");
    let content = match std::fs::read_to_string(&log_path) {
        Ok(c) => c,
        Err(_) => return 0,
    };
    content.lines().filter(|l| !l.trim().is_empty()).count() as u32
}

fn detect_state(git_dir: &Path) -> &'static str {
    let real = if git_dir.is_file() {
        match std::fs::read_to_string(git_dir) {
            Ok(c) => {
                if let Some(rest) = c.trim().strip_prefix("gitdir: ") {
                    PathBuf::from(rest)
                } else {
                    return "";
                }
            }
            Err(_) => return "",
        }
    } else {
        git_dir.to_path_buf()
    };
    if real.join("MERGE_HEAD").exists() {
        return "MERGE";
    }
    if real.join("rebase-merge").exists() || real.join("rebase-apply").exists() {
        return "REBASE";
    }
    if real.join("CHERRY_PICK_HEAD").exists() {
        return "PICK";
    }
    if real.join("REVERT_HEAD").exists() {
        return "REVERT";
    }
    if real.join("BISECT_LOG").exists() {
        return "BISECT";
    }
    ""
}
