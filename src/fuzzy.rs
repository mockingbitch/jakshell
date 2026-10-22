//! Ctrl-R: tìm lịch sử kiểu fuzzy (gõ "gpm" ra "git push origin main").
//!
//! Xếp hạng = độ khớp (liên tiếp, đầu từ) + độ mới + bonus lệnh từng chạy ở
//! thư mục hiện tại + bonus lệnh thành công. Mỗi lệnh chỉ hiện 1 lần (bản mới nhất).

use crossterm::{
    cursor,
    event::{self, Event, KeyCode, KeyEvent, KeyModifiers},
    queue,
    style::Print,
    terminal::{self, Clear, ClearType},
};
use std::io::Write;

use crate::histstore::Entry;

/// Điểm khớp fuzzy của `query` trong `text` (None = không khớp) + vị trí ký tự khớp.
pub fn score(query: &str, text: &str) -> Option<(i64, Vec<usize>)> {
    if query.is_empty() {
        return Some((0, Vec::new()));
    }
    let t: Vec<char> = text.chars().collect();
    let tl: Vec<char> = t.iter().map(|c| c.to_ascii_lowercase()).collect();
    let q: Vec<char> = query.chars().map(|c| c.to_ascii_lowercase()).collect();
    // Khớp liền mạch (substring) được thưởng lớn.
    let text_lower: String = tl.iter().collect();
    let query_lower: String = q.iter().collect();
    if let Some(byte_pos) = text_lower.find(&query_lower) {
        let start = text_lower[..byte_pos].chars().count();
        let positions: Vec<usize> = (start..start + q.len()).collect();
        let boundary = start == 0 || !tl[start - 1].is_alphanumeric();
        return Some((200 + q.len() as i64 * 10 + if boundary { 40 } else { 0 } - start as i64, positions));
    }
    // Subsequence: tham lam từ trái, thưởng ký tự liên tiếp / đầu từ.
    let mut positions = Vec::with_capacity(q.len());
    let mut s = 0i64;
    let mut ti = 0;
    let mut prev: Option<usize> = None;
    for &qc in &q {
        let found = (ti..tl.len()).find(|&i| tl[i] == qc)?;
        let boundary = found == 0 || !tl[found - 1].is_alphanumeric();
        s += 10;
        if boundary {
            s += 15;
        }
        if prev.is_some_and(|p| p + 1 == found) {
            s += 12;
        } else if let Some(p) = prev {
            s -= ((found - p) as i64).min(10);
        }
        positions.push(found);
        prev = Some(found);
        ti = found + 1;
    }
    Some((s, positions))
}

/// Xếp hạng lịch sử theo `query`, trả về (chỉ số entry, vị trí khớp) tốt nhất trước.
pub fn rank(entries: &[Entry], query: &str, cwd: &str, limit: usize) -> Vec<(usize, Vec<usize>)> {
    let n = entries.len().max(1) as i64;
    let mut seen = std::collections::HashSet::new();
    let mut scored: Vec<(i64, usize, Vec<usize>)> = Vec::new();
    for (i, e) in entries.iter().enumerate().rev() {
        if !seen.insert(e.cmd.as_str()) {
            continue;
        }
        let Some((s, pos)) = score(query, &e.cmd) else { continue };
        let recency = (i as i64 * 60) / n;
        let here = if !cwd.is_empty() && e.cwd == cwd { 50 } else { 0 };
        let ok = if e.code == Some(0) { 10 } else if e.code.is_some() { -10 } else { 0 };
        scored.push((s + recency + here + ok, i, pos));
    }
    scored.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.cmp(&a.1)));
    scored.into_iter().take(limit).map(|(_, i, p)| (i, p)).collect()
}

/// Giao diện chọn: gõ để lọc, ↑↓ / Ctrl-P Ctrl-N / Ctrl-R để chọn, Enter đưa
/// lệnh lên prompt (chưa chạy), Esc / Ctrl-C / Ctrl-G huỷ.
pub fn pick(entries: &[Entry], initial: &str, cwd: &str, use_color: bool) -> Option<String> {
    let mut out = std::io::stdout();
    // Một số terminal (pty mới tạo, CI) báo kích thước 0 → dùng 80×24.
    let (cols, rows) = match terminal::size() {
        Ok((w, h)) if w >= 20 && h >= 5 => (w as usize, h as usize),
        _ => (80, 24),
    };
    let max_items = rows.saturating_sub(4).clamp(3, 12);
    let mut query = initial.to_string();
    let mut sel = 0usize;
    terminal::enable_raw_mode().ok()?;
    let mut drawn = 0usize;
    let result = loop {
        let hits = rank(entries, &query, cwd, max_items);
        sel = sel.min(hits.len().saturating_sub(1));
        // Vẽ lại: về đầu vùng đã vẽ, xoá xuống dưới.
        if drawn > 0 {
            let _ = queue!(out, cursor::MoveUp(drawn as u16));
        }
        let _ = queue!(out, cursor::MoveToColumn(0), Clear(ClearType::FromCursorDown));
        let header = format!("🔍 {}{}", query, if use_color { "\x1b[7m \x1b[0m" } else { "_" });
        let count = format!("  ({} kết quả · ↑↓ chọn · Enter dùng · Esc huỷ)", hits.len());
        let _ = queue!(out, Print(&header), Print(if use_color { format!("\x1b[2m{}\x1b[0m", count) } else { count }), Print("\r\n"));
        for (row, (idx, pos)) in hits.iter().enumerate() {
            let e = &entries[*idx];
            let first_line = e.cmd.lines().next().unwrap_or("");
            let multi = if e.cmd.contains('\n') { " …" } else { "" };
            let mut line = String::new();
            let marker = if row == sel { "❯ " } else { "  " };
            for (ci, ch) in first_line.chars().enumerate() {
                if ci + 4 >= cols.saturating_sub(12) {
                    line.push('…');
                    break;
                }
                if use_color && pos.contains(&ci) {
                    line.push_str(&format!("\x1b[1;33m{}\x1b[22;39m", ch));
                } else {
                    line.push(ch);
                }
            }
            let here = if !cwd.is_empty() && e.cwd == cwd { " ·" } else { "" };
            let styled = if use_color && row == sel {
                format!("\x1b[36m{}\x1b[0m\x1b[1m{}{}\x1b[0m\x1b[2m{}\x1b[0m", marker, line, multi, here)
            } else {
                format!("{}{}{}{}", marker, line, multi, if use_color { format!("\x1b[2m{}\x1b[0m", here) } else { here.to_string() })
            };
            let _ = queue!(out, Print(styled), Print("\r\n"));
        }
        drawn = hits.len() + 1;
        let _ = out.flush();

        let Ok(Event::Key(KeyEvent { code, modifiers, .. })) = event::read() else { continue };
        let ctrl = modifiers.contains(KeyModifiers::CONTROL);
        match code {
            KeyCode::Enter => break hits.get(sel).map(|(i, _)| entries[*i].cmd.clone()),
            KeyCode::Esc => break None,
            KeyCode::Char('c' | 'g') if ctrl => break None,
            KeyCode::Up => sel = sel.saturating_sub(1),
            KeyCode::Char('p') if ctrl => sel = sel.saturating_sub(1),
            KeyCode::Down | KeyCode::Tab => sel = (sel + 1).min(hits.len().saturating_sub(1)),
            KeyCode::Char('n' | 'r') if ctrl => sel = (sel + 1).min(hits.len().saturating_sub(1)),
            KeyCode::Char('u') if ctrl => {
                query.clear();
                sel = 0;
            }
            KeyCode::Char('w') if ctrl => {
                let trimmed = query.trim_end();
                let cut = trimmed.rfind(' ').map_or(0, |p| p + 1);
                query.truncate(cut);
                sel = 0;
            }
            KeyCode::Backspace => {
                query.pop();
                sel = 0;
            }
            KeyCode::Char(c) if !ctrl => {
                query.push(c);
                sel = 0;
            }
            _ => {}
        }
    };
    // Xoá vùng picker, trả con trỏ về chỗ cũ cho prompt.
    let _ = queue!(out, cursor::MoveUp(drawn as u16), cursor::MoveToColumn(0), Clear(ClearType::FromCursorDown));
    let _ = out.flush();
    let _ = terminal::disable_raw_mode();
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(cmd: &str, cwd: &str, code: Option<i32>) -> Entry {
        Entry { cmd: cmd.into(), ts: 0, cwd: cwd.into(), code, ms: None, sid: 0 }
    }

    #[test]
    fn fuzzy_scores() {
        assert!(score("gpom", "git push origin main").is_some());
        assert!(score("xyz", "git push").is_none());
        let (sub, _) = score("push", "git push origin").unwrap();
        let (fz, _) = score("pso", "git push origin").unwrap();
        assert!(sub > fz, "khớp liền mạch phải hơn khớp rời");
    }

    #[test]
    fn ranking_prefers_cwd_recent_unique() {
        let entries = vec![
            e("make build", "/other", Some(0)),
            e("make test", "/proj", Some(0)),
            e("make build", "/proj", Some(0)),
            e("make deploy", "/other", Some(1)),
        ];
        let r = rank(&entries, "make", "/proj", 10);
        let cmds: Vec<&str> = r.iter().map(|(i, _)| entries[*i].cmd.as_str()).collect();
        assert_eq!(cmds.len(), 3, "trùng lệnh chỉ hiện 1 lần");
        assert_eq!(cmds[0], "make build");
        assert_eq!(*cmds.last().unwrap(), "make deploy", "lệnh lỗi, khác thư mục xếp cuối");
    }
}
