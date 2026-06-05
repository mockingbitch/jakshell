use crate::shell::Shell;

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

fn git_segment(cwd: &std::path::Path, theme: &crate::theme::Theme) -> String {
    let branch = match git_branch(cwd) {
        Some(b) => b,
        None => return String::new(),
    };
    let dirty = git_dirty(cwd);
    let dirty_mark = if dirty { "*" } else { "" };
    format!(
        " {dim}{icon}{branch}{mark}{reset}",
        dim = theme.dim_ansi(),
        icon = theme.git_branch_icon,
        branch = branch,
        mark = dirty_mark,
        reset = "\x1b[0m",
    )
}

fn git_branch(cwd: &std::path::Path) -> Option<String> {
    let mut p: Option<&std::path::Path> = Some(cwd);
    while let Some(dir) = p {
        let head = dir.join(".git").join("HEAD");
        if let Ok(content) = std::fs::read_to_string(&head) {
            let content = content.trim();
            if let Some(rest) = content.strip_prefix("ref: refs/heads/") {
                return Some(rest.to_string());
            }
            // Detached HEAD: show short sha
            if content.len() >= 7 {
                return Some(content[..7].to_string());
            }
        }
        p = dir.parent();
    }
    None
}

fn git_dirty(cwd: &std::path::Path) -> bool {
    // Cheap check: any modification newer than .git/index inside .git's parent.
    // For correctness we'd shell out to `git status --porcelain` but that slows the prompt.
    // Skip dirty-check by default to keep prompt fast.
    let _ = cwd;
    false
}
