//! Nhận diện lệnh nguy hiểm trước khi chạy ở REPL (hỏi xác nhận).
//!
//! Chỉ xét chữ người dùng gõ (chưa expand), theo từng lệnh đơn trong dòng —
//! ưu tiên ít báo nhầm: `rm -rf ./build` không bị hỏi, `rm -rf /` thì có.

use crate::lexer::{parts_to_source, Token};

/// Đích "quá rộng" cho `rm -r` / `chmod -R` / `chown -R`.
fn is_root_like(arg: &str) -> bool {
    let a = arg.trim_end_matches('/');
    matches!(
        a,
        "" | "/*" | "~" | "~/*" | "$HOME" | "${HOME}" | "$HOME/*" | "${HOME}/*" | "*" | "/home" | "/usr" | "/etc"
            | "/var" | "/boot" | "/bin" | "/lib" | "/opt" | "/root" | "/System" | "/Users" | "/Applications" | ".."
    ) || arg == "/"
}

/// Cờ đệ quy kiểu `-r`, `-R`, `-rf`, `-fr`, `--recursive`.
fn has_flag(words: &[String], short: char, long: &str) -> bool {
    words.iter().any(|w| {
        w == long || (w.starts_with('-') && !w.starts_with("--") && w.chars().skip(1).any(|c| c == short))
    })
}

/// Lý do nếu lệnh đơn `words` nguy hiểm.
fn check_simple(words: &[String]) -> Option<String> {
    // Bỏ tiền tố không đổi ý nghĩa: sudo/doas (kèm cờ của nó), command, nohup,
    // time, exec, env, và phép gán `VAR=…`.
    let mut i = 0;
    while i < words.len() {
        let w = words[i].as_str();
        if matches!(w, "sudo" | "doas") {
            i += 1;
            while i < words.len() && words[i].starts_with('-') {
                // `-u user` / `-g group` nhận thêm 1 đối số.
                let takes_value = matches!(words[i].as_str(), "-u" | "-g" | "-C" | "-p");
                i += if takes_value { 2 } else { 1 };
            }
        } else if matches!(w, "command" | "nohup" | "time" | "exec" | "env") || (w.contains('=') && !w.starts_with('-')) {
            i += 1;
        } else {
            break;
        }
    }
    let words = &words[i.min(words.len())..];
    let (cmd, args) = words.split_first()?;
    let targets: Vec<&String> = args.iter().filter(|a| !a.starts_with('-')).collect();
    match cmd.as_str() {
        "rm" if has_flag(args, 'r', "--recursive") || has_flag(args, 'R', "--recursive") => {
            if args.iter().any(|a| a == "--no-preserve-root") {
                return Some("rm --no-preserve-root".into());
            }
            targets.iter().find(|t| is_root_like(t)).map(|t| format!("xoá đệ quy `{}`", t))
        }
        // Đích đầu tiên là mode / owner — xét các đích còn lại.
        "chmod" | "chown" | "chgrp" if has_flag(args, 'R', "--recursive") => targets
            .iter()
            .skip(1)
            .find(|t| is_root_like(t))
            .map(|t| format!("{} đệ quy trên `{}`", cmd, t)),
        "mkfs" | "mke2fs" | "wipefs" => Some(format!("{} — xoá sạch định dạng ổ đĩa", cmd)),
        c if c.starts_with("mkfs.") => Some(format!("{} — tạo lại filesystem (mất dữ liệu)", c)),
        "dd" => args
            .iter()
            .find(|a| a.starts_with("of=/dev/") && !a.starts_with("of=/dev/null"))
            .map(|a| format!("dd ghi thẳng lên thiết bị `{}`", &a[3..])),
        "shred" if targets.iter().any(|t| t.starts_with("/dev/")) => Some("shred thiết bị".into()),
        "git" => {
            let sub = args.iter().find(|a| !a.starts_with('-'))?;
            match sub.as_str() {
                // `--force`, `-f` hoặc refspec `+nhánh` (ép ghi đè).
                "push" if args.iter().any(|a| a == "--force" || a == "-f" || a.starts_with('+')) => {
                    let protected = ["main", "master", "production", "prod", "release"];
                    let hit = args.iter().find(|a| protected.iter().any(|p| a.trim_start_matches('+') == *p || a.ends_with(&format!(":{}", p))));
                    hit.map(|b| format!("git push --force lên nhánh `{}`", b.trim_start_matches('+')))
                }
                "clean" if has_flag(args, 'f', "--force") && has_flag(args, 'd', "-d") && has_flag(args, 'x', "-x") => {
                    Some("git clean -fdx — xoá mọi file chưa track kể cả file bị ignore".into())
                }
                _ => None,
            }
        }
        "kill" | "pkill" if args.iter().any(|a| a == "-1") && args.iter().any(|a| a == "-9" || a == "-KILL") => {
            Some("kill -9 -1 — giết mọi tiến trình của bạn".into())
        }
        _ => None,
    }
}

/// Lý do cảnh báo (nếu có) cho cả dòng lệnh.
pub fn danger(line: &str) -> Option<String> {
    // Fork bomb kinh điển.
    let compact: String = line.chars().filter(|c| !c.is_whitespace()).collect();
    if compact.contains(":(){:|:&};:") {
        return Some("fork bomb — làm treo máy".into());
    }
    let tokens = crate::lexer::tokenize(line).ok()?;
    let mut words: Vec<String> = Vec::new();
    let mut redirect_next = false;
    for t in tokens.iter().chain(std::iter::once(&Token::Semicolon)) {
        match t {
            Token::Word(parts) => {
                let w = parts_to_source(parts);
                if redirect_next {
                    redirect_next = false;
                    // `> /dev/sda` — ghi đè thiết bị đĩa.
                    if w.starts_with("/dev/sd") || w.starts_with("/dev/nvme") || w.starts_with("/dev/disk") {
                        return Some(format!("ghi đè thiết bị `{}`", w));
                    }
                    continue;
                }
                words.push(w);
            }
            Token::Redir(_, _) => redirect_next = true,
            _ => {
                if let Some(r) = check_simple(&words) {
                    return Some(r);
                }
                words.clear();
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::danger;

    #[test]
    fn flags_dangerous() {
        for l in [
            "rm -rf /",
            "sudo rm -rf /*",
            "rm -fr ~",
            "rm -rf $HOME",
            "rm -r --no-preserve-root /",
            "cd /tmp && rm -Rf /",
            "chmod -R 777 /",
            "sudo chown -R me /",
            "mkfs.ext4 /dev/sda1",
            "dd if=img.iso of=/dev/sdb bs=4M",
            "echo x > /dev/sda",
            "git push --force origin main",
            "git push -f origin master",
            "git push origin +main",
            ":(){ :|:& };:",
            "git clean -fdx",
        ] {
            assert!(danger(l).is_some(), "phải cảnh báo: {l}");
        }
    }

    #[test]
    fn allows_normal() {
        for l in [
            "rm -rf ./build",
            "rm -rf node_modules target",
            "rm file.txt",
            "chmod -R 755 ./public",
            "chmod 777 /tmp/x",
            "dd if=/dev/zero of=disk.img bs=1M count=10",
            "dd if=/dev/urandom of=/dev/null",
            "git push origin main",
            "git push --force origin feature/x",
            "git clean -fd",
            "echo 'rm -rf /'",
            "ls -la /",
        ] {
            assert!(danger(l).is_none(), "không được cảnh báo: {l} → {:?}", danger(l));
        }
    }
}
