use crate::shell::Shell;
use strsim::jaro_winkler;

pub fn maybe_suggest(shell: &Shell, line: &str) {
    let first = line.split_whitespace().next().unwrap_or("");
    if first.is_empty() {
        return;
    }
    // Bỏ qua các namespace nội bộ — chúng thuộc về JakShell, không phải lệnh ngoài.
    if matches!(first, "jak" | "explain") {
        return;
    }
    // Don't suggest for builtins/aliases (they wouldn't have failed for "not found")
    if crate::builtins::is_builtin(first) || shell.aliases.contains_key(first) {
        return;
    }
    if which::which(first).is_ok() {
        return;
    }
    let mut candidates: Vec<String> = crate::builtins::BUILTINS.iter().map(|s| s.to_string()).collect();
    candidates.extend(shell.aliases.keys().cloned());
    candidates.extend(["jak clean".into(), "jak backup".into(), "jak update".into(), "jak find".into(), "jak open".into(), "jak sysinfo".into(), "jak theme".into()]);
    if let Some(path) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path) {
            if let Ok(it) = std::fs::read_dir(&dir) {
                for entry in it.flatten() {
                    candidates.push(entry.file_name().to_string_lossy().to_string());
                }
            }
        }
    }
    let mut best: Option<(String, f64)> = None;
    for c in &candidates {
        let score = jaro_winkler(first, c);
        if score > 0.86 {
            if best.as_ref().map(|b| score > b.1).unwrap_or(true) {
                best = Some((c.clone(), score));
            }
        }
    }
    if let Some((s, _)) = best {
        eprintln!("\x1b[2m💡 có phải bạn muốn:\x1b[0m \x1b[36m{}\x1b[0m?", s);
    }
}
