//! `jaksh --check file...` / `jaksh -n file...` — kiểm tra cú pháp không chạy,
//! báo lỗi kèm số dòng và cảnh báo cú pháp bash mà jaksh chưa hỗ trợ.

/// 1 vấn đề: (số dòng, là lỗi hay chỉ cảnh báo, thông điệp).
pub type Issue = (usize, bool, String);

fn is_incomplete(e: &anyhow::Error) -> bool {
    e.downcast_ref::<crate::lexer::Incomplete>().is_some()
}

/// Một khối lệnh trọn vẹn của script, hoặc lỗi cú pháp ở 1 dòng.
pub enum Block {
    /// (dòng bắt đầu, nội dung)
    Code(usize, String),
    /// (dòng lỗi, thông điệp)
    Error(usize, String),
}

/// Dòng mở của nháy chưa đóng cuối cùng trong `chunk` (0-based, tương đối).
fn unclosed_quote_line(chunk: &str) -> Option<usize> {
    let mut quote: Option<(char, usize)> = None;
    let mut line = 0;
    let mut chars = chunk.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\n' {
            line += 1;
            continue;
        }
        match quote {
            Some((q, _)) if c == q => quote = None,
            Some(('"', _)) if c == '\\' => {
                chars.next();
            }
            Some(_) => {}
            None => match c {
                '\\' => {
                    if chars.next() == Some('\n') {
                        line += 1;
                    }
                }
                '\'' | '"' | '`' => quote = Some((c, line)),
                '#' => {
                    // comment tới hết dòng
                    for d in chars.by_ref() {
                        if d == '\n' {
                            line += 1;
                            break;
                        }
                    }
                }
                _ => {}
            },
        }
    }
    quote.map(|(_, l)| l)
}

/// Cắt script thành các khối lệnh trọn vẹn (mỗi lần thêm 1 dòng; khối còn dở —
/// thiếu `fi`, nháy chưa đóng… — thì đọc tiếp). Số dòng 1-based.
pub fn blocks(content: &str) -> Vec<Block> {
    let lines: Vec<&str> = content.split('\n').collect();
    let mut out = Vec::new();
    let mut start = 0;
    for k in 0..lines.len() {
        let chunk = lines[start..=k].join("\n");
        if chunk.trim().is_empty() {
            start = k + 1;
            continue;
        }
        match crate::lexer::tokenize(&chunk).and_then(|t| {
            let open = crate::lexer::last_heredoc_unterminated();
            crate::parser::parse(&t).map(|_| open)
        }) {
            Ok(false) => {
                out.push(Block::Code(start + 1, chunk));
                start = k + 1;
            }
            Ok(true) => {} // here-doc chưa có delimiter — đọc tiếp
            Err(e) if is_incomplete(&e) => {}
            Err(e) => {
                out.push(Block::Error(k + 1, e.to_string()));
                start = k + 1;
            }
        }
    }
    if start < lines.len() {
        let chunk = lines[start..].join("\n");
        if !chunk.trim().is_empty() {
            match crate::lexer::tokenize(&chunk).and_then(|t| crate::parser::parse(&t)) {
                Err(e) => {
                    // Nháy chưa đóng → chỉ đúng dòng mở nháy (như bash).
                    let line = unclosed_quote_line(&chunk).map_or(start + 1, |l| start + l + 1);
                    out.push(Block::Error(line, format!("{} (tới hết file vẫn chưa đóng)", e)));
                }
                Ok(_) => out.push(Block::Error(start + 1, "here-doc thiếu dòng kết thúc".into())),
            }
        }
    }
    out
}

/// Lỗi cú pháp kèm số dòng (1-based).
pub fn syntax_errors(content: &str) -> Vec<(usize, String)> {
    blocks(content)
        .into_iter()
        .filter_map(|b| match b {
            Block::Error(l, m) => Some((l, m)),
            Block::Code(..) => None,
        })
        .collect()
}

/// Cú pháp bash hợp lệ nhưng jaksh chưa hỗ trợ.
fn unsupported(content: &str) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    for (n, line) in content.lines().enumerate() {
        let code = line.split(" #").next().unwrap_or(line);
        let trimmed = code.trim_start();
        if trimmed.starts_with('#') {
            continue;
        }
        let first = trimmed.split_whitespace().next().unwrap_or("");
        match first {
            "select" => out.push((n + 1, "`select` chưa được hỗ trợ".to_string())),
            "coproc" => out.push((n + 1, "`coproc` chưa được hỗ trợ".to_string())),
            _ => {}
        }
        for pat in ["@(", "!(", "+(", "*(", "?("] {
            // `$(`, `$((` không phải extglob.
            if code.match_indices(pat).any(|(i, _)| i == 0 || !code[..i].ends_with('$')) && !code.contains("$((") {
                if pat != "*(" && pat != "?(" || code.contains(&format!(" {}", pat)) {
                    out.push((n + 1, format!("extglob `{}…)` chưa được hỗ trợ", pat)));
                    break;
                }
            }
        }
        let words: Vec<&str> = trimmed.split_whitespace().collect();
        if matches!(first, "declare" | "local" | "typeset") && words.iter().any(|w| w.starts_with('-') && w.contains('n')) {
            out.push((n + 1, "nameref (`declare -n`) chưa được hỗ trợ".to_string()));
        }
        if first == "trap" && words.iter().any(|w| matches!(*w, "DEBUG" | "RETURN")) {
            out.push((n + 1, "trap DEBUG / RETURN được nhận nhưng chưa chạy".to_string()));
        }
    }
    out
}

/// Tất cả vấn đề của 1 script, sắp theo dòng.
pub fn check(content: &str) -> Vec<Issue> {
    let mut issues: Vec<Issue> = syntax_errors(content).into_iter().map(|(l, m)| (l, true, m)).collect();
    issues.extend(unsupported(content).into_iter().map(|(l, m)| (l, false, m)));
    issues.sort_by_key(|i| i.0);
    issues
}

/// Chạy `--check`: in kết quả, trả exit code (0 ổn, 1 chỉ cảnh báo, 2 có lỗi).
pub fn run(paths: &[String]) -> i32 {
    let color = unsafe { libc::isatty(libc::STDOUT_FILENO) } == 1;
    let paint = |code: &str, s: &str| if color { format!("\x1b[{}m{}\x1b[0m", code, s) } else { s.to_string() };
    let mut worst = 0;
    if paths.is_empty() {
        eprintln!("dùng: jaksh --check <script> [script...]");
        return 2;
    }
    for path in paths {
        let content = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(e) => {
                println!("{}: {}", path, paint("31", &format!("không đọc được: {}", e)));
                worst = 2;
                continue;
            }
        };
        let content = blank_shebang(&content);
        let issues = check(&content);
        if issues.is_empty() {
            println!("{} {}", paint("32", "✓"), path);
            continue;
        }
        for (line, is_err, msg) in &issues {
            let tag = if *is_err { paint("31", "lỗi") } else { paint("33", "cảnh báo") };
            // Dạng `file:dòng:` để editor / terminal nhảy tới được.
            println!("{}:{}: {}: {}", path, line, tag, msg);
            worst = worst.max(if *is_err { 2 } else { 1 });
        }
    }
    worst
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_line_numbers() {
        let src = "echo ok\nif true; then\n  echo a\nfi\necho \"unclosed\nfor x in 1; do\necho fine\n";
        let errs = syntax_errors(src);
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].0, 5, "nháy mở ở dòng 5: {:?}", errs);
        // nháy mở giữa khối `for` → báo dòng mở nháy, không phải dòng `for`
        let errs = syntax_errors("for f in a; do\n  echo \"$f\ndone\n");
        assert_eq!(errs[0].0, 2, "{errs:?}");
        let errs = syntax_errors("echo a\nfi\necho b\nthen\n");
        assert_eq!(errs.iter().map(|e| e.0).collect::<Vec<_>>(), vec![2, 4]);
        assert!(syntax_errors("cat <<EOF\nx\nEOF\nf() {\n  :\n}\n").is_empty());
        let errs = syntax_errors("while true; do\n  echo\n");
        assert_eq!(errs[0].0, 1);
    }

    #[test]
    fn warns_unsupported() {
        let issues = check("select x in a b; do :; done\nls @(a|b)\necho $(date) $((1+2))\n");
        let lines: Vec<usize> = issues.iter().filter(|i| !i.1).map(|i| i.0).collect();
        assert!(lines.contains(&1) && lines.contains(&2) && !lines.contains(&3), "{issues:?}");
    }
}

/// Thay dòng shebang bằng dòng trống (giữ nguyên số dòng).
pub fn blank_shebang(content: &str) -> String {
    match content.strip_prefix("#!") {
        Some(rest) => format!("\n{}", rest.split_once('\n').map_or("", |(_, r)| r)),
        None => content.to_string(),
    }
}
