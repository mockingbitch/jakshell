//! Xử lý lịch sử lệnh phía JakShell (phần rustyline không làm):
//!   • gộp dòng nối bằng `\` thành 1 entry khi paste lệnh nhiều dòng;
//!   • history expansion kiểu bash: `!!`, `!$`, `!*`, `!N`, `!-N`, `!prefix`;
//!   • phát hiện token / mật khẩu để KHÔNG lưu vào file lịch sử.

use anyhow::{anyhow, Result};

/// Tách 1 lần nhập (có thể là khối paste nhiều dòng) thành các entry lịch sử.
/// Dòng kết thúc bằng `\` được nối với dòng sau (bỏ `\`, ghép bằng 1 dấu cách)
/// — nhờ đó `curl ... \` paste nhiều dòng gọi lại bằng ↑ thành 1 lệnh chạy được
/// thay vì từng mẩu `--header ...` rời rạc. Lệnh ghép nhiều dòng (`for … done`,
/// `if … fi`, chuỗi / here-doc nhiều dòng) được giữ nguyên thành 1 entry.
pub fn entries(input: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    // Khối nhiều dòng chưa trọn cú pháp (giữ xuống dòng thật).
    let mut block = String::new();
    for raw in input.split('\n') {
        let line = raw.strip_suffix('\r').unwrap_or(raw);
        if !block.is_empty() {
            block.push('\n');
            block.push_str(line);
            if !crate::parser::needs_more(&block) {
                out.push(std::mem::take(&mut block).trim().to_string());
            }
            continue;
        }
        let piece = if cur.is_empty() { line } else { line.trim_start() };
        match piece.strip_suffix('\\') {
            Some(head) => {
                cur.push_str(head.trim_end());
                cur.push(' ');
            }
            None => {
                cur.push_str(piece);
                let entry = cur.trim().to_string();
                cur.clear();
                if entry.is_empty() {
                    continue;
                }
                if crate::parser::needs_more(&entry) {
                    block = entry;
                } else {
                    out.push(entry);
                }
            }
        }
    }
    for rest in [cur, block] {
        let entry = rest.trim().to_string();
        if !entry.is_empty() {
            out.push(entry);
        }
    }
    out
}

/// History expansion. `hist` = lịch sử TRƯỚC lệnh hiện tại (cũ → mới).
/// Trả `Ok(None)` nếu dòng không có gì để mở rộng, `Ok(Some(dòng mới))` nếu có,
/// `Err` khi không tìm thấy lệnh (bash: "event not found").
///
/// Như bash: không mở rộng trong nháy đơn, sau `\`, hay khi `!` đứng trước
/// khoảng trắng / `=` / `(` / cuối dòng (vd `[ ! -f x ]`, `!=`). Khác bash:
/// `!prefix` chỉ mở rộng khi `!` ở ĐẦU một từ — `echo hi!there` giữ nguyên
/// thay vì báo lỗi.
pub fn expand(line: &str, hist: &[String]) -> Result<Option<String>> {
    if !line.contains('!') {
        return Ok(None);
    }
    let chars: Vec<char> = line.chars().collect();
    let mut out = String::with_capacity(line.len());
    let mut changed = false;
    let mut in_single = false;
    let mut in_double = false;
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '\\' && !in_single {
            out.push(c);
            if let Some(&n) = chars.get(i + 1) {
                out.push(n);
            }
            i += 2;
            continue;
        }
        if c == '\'' && !in_double {
            in_single = !in_single;
        } else if c == '"' && !in_single {
            in_double = !in_double;
        }
        if c != '!' || in_single || (i > 0 && chars[i - 1] == '$') {
            out.push(c);
            i += 1;
            continue;
        }
        let next = chars.get(i + 1).copied();
        let at_word_start = i == 0 || matches!(chars[i - 1], ' ' | '\t' | ';' | '|' | '&' | '(');
        let last = || hist.last().ok_or_else(|| anyhow!("!!: event not found"));
        match next {
            Some('!') => {
                out.push_str(last()?);
                i += 2;
            }
            Some('$') => {
                let entry = last()?;
                out.push_str(words(entry).last().copied().unwrap_or(""));
                i += 2;
            }
            Some('*') => {
                let entry = last()?;
                out.push_str(&words(entry).get(1..).unwrap_or(&[]).join(" "));
                i += 2;
            }
            Some(d) if d.is_ascii_digit() || (d == '-' && chars.get(i + 2).is_some_and(|c| c.is_ascii_digit())) => {
                let start = i + 1;
                let mut j = if d == '-' { start + 1 } else { start };
                while j < chars.len() && chars[j].is_ascii_digit() {
                    j += 1;
                }
                let spec: String = chars[start..j].iter().collect();
                let n: i64 = spec.parse().unwrap_or(0);
                let idx = if n < 0 { hist.len() as i64 + n } else { n - 1 };
                let entry = usize::try_from(idx)
                    .ok()
                    .and_then(|k| hist.get(k))
                    .ok_or_else(|| anyhow!("!{}: event not found", spec))?;
                out.push_str(entry);
                i = j;
            }
            Some(n) if at_word_start && !is_word_end(n) && n != '=' && n != '"' => {
                let start = i + 1;
                let mut j = start;
                while j < chars.len() && !is_word_end(chars[j]) && chars[j] != '"' && chars[j] != '\'' {
                    j += 1;
                }
                let prefix: String = chars[start..j].iter().collect();
                let entry = hist
                    .iter()
                    .rev()
                    .find(|e| e.starts_with(&prefix))
                    .ok_or_else(|| anyhow!("!{}: event not found", prefix))?;
                out.push_str(entry);
                i = j;
            }
            _ => {
                out.push(c);
                i += 1;
                continue;
            }
        }
        changed = true;
    }
    Ok(changed.then_some(out))
}

fn is_word_end(c: char) -> bool {
    c.is_whitespace() || matches!(c, ';' | '|' | '&' | '(' | ')' | '<' | '>')
}

/// Tách lệnh thành từ (giữ nguyên nháy) — đủ cho `!$` / `!*`.
fn words(line: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = None;
    let mut quote: Option<char> = None;
    for (idx, c) in line.char_indices() {
        match quote {
            Some(q) => {
                if c == q {
                    quote = None;
                }
            }
            None if c.is_whitespace() => {
                if let Some(s) = start.take() {
                    out.push(&line[s..idx]);
                }
            }
            None => {
                if start.is_none() {
                    start = Some(idx);
                }
                if c == '\'' || c == '"' {
                    quote = Some(c);
                }
            }
        }
    }
    if let Some(s) = start {
        out.push(&line[s..]);
    }
    out
}

/// Lệnh có chứa token / mật khẩu không? Trả về loại (để báo cho người dùng).
/// Heuristic, ưu tiên ít báo nhầm: chỉ bắt tiền tố token đã biết với độ dài
/// đủ, header xác thực, URL có user:pass@, `curl -u`, `mysql -pXXX` và phép
/// gán biến có tên kiểu TOKEN/PASSWORD/SECRET.
pub fn secret_kind(line: &str) -> Option<&'static str> {
    // Tiền tố token đã biết + số ký tự tối thiểu phía sau.
    const PREFIXES: &[(&str, usize, &str)] = &[
        ("ghp_", 30, "GitHub token"),
        ("gho_", 30, "GitHub token"),
        ("ghu_", 30, "GitHub token"),
        ("ghs_", 30, "GitHub token"),
        ("ghr_", 30, "GitHub token"),
        ("github_pat_", 30, "GitHub token"),
        ("glpat-", 20, "GitLab token"),
        ("xoxb-", 20, "Slack token"),
        ("xoxp-", 20, "Slack token"),
        ("xoxa-", 20, "Slack token"),
        ("sk-ant-", 30, "Anthropic API key"),
        ("sk-proj-", 30, "OpenAI API key"),
        ("AKIA", 16, "AWS access key"),
        ("AIza", 35, "Google API key"),
        ("npm_", 36, "npm token"),
        ("hf_", 30, "Hugging Face token"),
    ];
    let token_char = |c: char| c.is_ascii_alphanumeric() || c == '_' || c == '-';
    for (prefix, min, kind) in PREFIXES {
        let mut from = 0;
        while let Some(pos) = line[from..].find(prefix) {
            let at = from + pos;
            let boundary = line[..at].chars().next_back().is_none_or(|c| !token_char(c));
            let tail = line[at + prefix.len()..].chars().take_while(|&c| token_char(c)).count();
            if boundary && tail >= *min {
                return Some(kind);
            }
            from = at + prefix.len();
        }
    }
    // JWT: eyJ….eyJ…
    if let Some(pos) = line.find("eyJ") {
        let rest = &line[pos..];
        let tok: String = rest.chars().take_while(|&c| token_char(c) || c == '.').collect();
        if tok.matches('.').count() >= 2 && tok.len() > 40 {
            return Some("JWT");
        }
    }
    let lower = line.to_ascii_lowercase();
    // Header xác thực: `Authorization: Bearer …`, `X-Api-Key: …`, …
    for header in ["authorization:", "x-api-key:", "api-key:", "apikey:", "x-auth-token:", "private-token:", "x-access-token:"] {
        if let Some(pos) = lower.find(header) {
            let value = lower[pos + header.len()..].trim_start();
            let value = value.trim_start_matches(|c: char| c == '\'' || c == '"');
            let v: String = value.chars().take_while(|c| !matches!(c, '\'' | '"')).collect();
            let v = v.trim();
            let v = ["bearer ", "basic ", "token "].iter().find_map(|p| v.strip_prefix(p)).unwrap_or(v).trim_start();
            if v.len() >= 8 && !v.starts_with('$') {
                return Some("header xác thực");
            }
        }
    }
    // URL kèm thông tin đăng nhập: scheme://user:pass@host hoặc scheme://TOKEN@host.
    let mut from = 0;
    while let Some(pos) = line[from..].find("://") {
        let start = from + pos + 3;
        let auth: String = line[start..].chars().take_while(|c| !matches!(c, '/' | ' ' | '\'' | '"' | '?' | '#')).collect();
        if let Some((cred, _host)) = auth.rsplit_once('@') {
            let has_pass = cred.split_once(':').is_some_and(|(_, p)| !p.is_empty() && !p.starts_with('$'));
            if has_pass || (cred.len() >= 20 && !cred.starts_with('$')) {
                return Some("URL có mật khẩu/token");
            }
        }
        from = start;
    }
    let words: Vec<&str> = line.split_whitespace().collect();
    for (i, w) in words.iter().enumerate() {
        let w = w.trim_matches(|c| c == '\'' || c == '"');
        // curl/wget -u user:pass, --user user:pass, --password=…
        if matches!(w, "-u" | "--user" | "--proxy-user") {
            if let Some(next) = words.get(i + 1) {
                if next.split_once(':').is_some_and(|(_, p)| !p.is_empty() && !p.starts_with('$')) {
                    return Some("user:password");
                }
            }
        }
        for flag in ["--password=", "--passwd=", "--token=", "--api-key=", "--secret="] {
            if let Some(v) = w.strip_prefix(flag) {
                if !v.is_empty() && !v.starts_with('$') {
                    return Some("mật khẩu trong tham số");
                }
            }
        }
        // mysql -pSECRET (mật khẩu dính liền -p)
        if i > 0 && w.len() > 2 && w.starts_with("-p") && words.iter().take(i).any(|c| matches!(*c, "mysql" | "mysqldump" | "mariadb" | "sshpass")) {
            return Some("mật khẩu trong tham số");
        }
        // NAME=value với NAME gợi ý bí mật (GITHUB_TOKEN=…, DB_PASSWORD=…)
        if let Some((name, value)) = w.split_once('=') {
            let upper = name.to_ascii_uppercase();
            let looks_secret = ["TOKEN", "PASSWORD", "PASSWD", "SECRET", "API_KEY", "APIKEY", "ACCESS_KEY", "PRIVATE_KEY"]
                .iter()
                .any(|k| upper.contains(k));
            let value = value.trim_matches(|c| c == '\'' || c == '"');
            if looks_secret && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') && value.len() >= 6 && !value.starts_with('$') {
                return Some("biến chứa bí mật");
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn h() -> Vec<String> {
        vec!["git status".into(), "ls -la /tmp".into(), "echo 'a b' c".into()]
    }

    #[test]
    fn joins_continuation_lines() {
        let paste = "curl --location 'https://x/v' \\\n  --header 'A: b' \\\n  --data '{}'\nls";
        assert_eq!(
            entries(paste),
            vec!["curl --location 'https://x/v' --header 'A: b' --data '{}'", "ls"]
        );
        assert_eq!(entries("a\n\nb"), vec!["a", "b"]);
        // lệnh ghép nhiều dòng = 1 entry
        assert_eq!(entries("for x in 1 2\ndo\n  echo $x\ndone\nls"), vec!["for x in 1 2\ndo\n  echo $x\ndone", "ls"]);
    }

    #[test]
    fn bang_forms() {
        let h = h();
        assert_eq!(expand("sudo !!", &h).unwrap().unwrap(), "sudo echo 'a b' c");
        assert_eq!(expand("cat !$", &h).unwrap().unwrap(), "cat c");
        assert_eq!(expand("echo !*", &h).unwrap().unwrap(), "echo 'a b' c");
        assert_eq!(expand("!1", &h).unwrap().unwrap(), "git status");
        assert_eq!(expand("!-2", &h).unwrap().unwrap(), "ls -la /tmp");
        assert_eq!(expand("!git", &h).unwrap().unwrap(), "git status");
    }

    #[test]
    fn bang_left_alone() {
        let h = h();
        for s in ["[ ! -f x ]", "a != b", "echo '!!'", "echo \\!!", "echo $!", "echo hi!there", "echo \"wow!\"", "x!"] {
            assert_eq!(expand(s, &h).unwrap(), None, "{s}");
        }
    }

    #[test]
    fn detects_secrets() {
        let yes = [
            "git clone https://oauth:ghp_AbCdEfGhIjKlMnOpQrStUvWxYz0123456789@github.com/a/b.git",
            "curl -H 'X-Api-Key: fx_ykrzmjzn_yeeyjcu53njtlyqxsdzog2ot53iiqrmiexwtuzpi5t6arofldtna' https://x",
            "curl -H \"Authorization: Bearer abcdef123456\" https://x",
            "export GITHUB_TOKEN=abcdef123456",
            "DB_PASSWORD='hunter22' ./run",
            "curl -u admin:s3cret https://x",
            "mysql -uroot -pS3cretPass db",
            "psql postgres://app:pa55word@db:5432/x",
            "aws configure set aws_access_key_id AKIAIOSFODNN7EXAMPLE",
            "echo sk-ant-api03-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
        ];
        for l in yes {
            assert!(secret_kind(l).is_some(), "phải bắt: {l}");
        }
        let no = [
            "git push origin feature/fx-conversion",
            "curl -H 'Content-Type: application/json' https://api.example.com",
            "curl -H \"Authorization: Bearer $TOKEN\" https://x",
            "export TOKEN_FILE=$HOME/.tok",
            "git clone https://github.com/a/b.git",
            "ssh user@host",
            "echo task-list ghp_short",
            "PASSWORD=$(cat pw.txt) ./run",
            "docker compose exec backend pnpm db:migrate",
        ];
        for l in no {
            assert!(secret_kind(l).is_none(), "không được bắt nhầm: {l} → {:?}", secret_kind(l));
        }
    }

    #[test]
    fn bang_not_found() {
        assert!(expand("!nope", &h()).is_err());
        assert!(expand("!99", &h()).is_err());
        assert!(expand("!!", &[]).is_err());
    }
}
