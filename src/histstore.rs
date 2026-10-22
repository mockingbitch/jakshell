//! Kho lịch sử lệnh: `~/.config/jaksh/history.jsonl`, mỗi dòng 1 lệnh kèm
//! thời điểm, thư mục, exit code, thời lượng và phiên (pid shell).
//!
//! Nhiều phiên (tab) cùng ghi nối vào 1 file bằng `O_APPEND` (mỗi lệnh = 1 lần
//! ghi → không xen lẫn); mỗi phiên đọc thêm phần các phiên khác vừa ghi trước
//! mỗi prompt. Lần đầu chạy tự nhập file lịch sử cũ (định dạng rustyline `#V2`).

use serde::{Deserialize, Serialize};
use std::fs::OpenOptions;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Entry {
    /// Lệnh (có thể nhiều dòng).
    pub cmd: String,
    /// Unix time (giây) lúc bắt đầu chạy; 0 = nhập từ lịch sử cũ.
    #[serde(default)]
    pub ts: i64,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub cwd: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ms: Option<u64>,
    /// pid của phiên đã chạy lệnh.
    #[serde(default)]
    pub sid: u32,
}

impl Entry {
    pub fn new(cmd: &str, cwd: &str) -> Self {
        Entry {
            cmd: cmd.to_string(),
            ts: chrono::Utc::now().timestamp(),
            cwd: cwd.to_string(),
            code: None,
            ms: None,
            sid: std::process::id(),
        }
    }
}

pub struct Store {
    path: PathBuf,
    /// Đã đọc tới byte nào (phần sau đó là của phiên khác ghi thêm).
    offset: u64,
    sid: u32,
}

/// Đọc file lịch sử cũ của rustyline (`#V2`, `\n` / `\\` được escape).
fn read_legacy(path: &Path) -> Vec<Entry> {
    let Ok(text) = std::fs::read_to_string(path) else { return Vec::new() };
    let mut out = Vec::new();
    for line in text.lines() {
        if line == "#V2" || line.trim().is_empty() {
            continue;
        }
        let mut cmd = String::new();
        let mut chars = line.chars();
        while let Some(c) = chars.next() {
            if c == '\\' {
                match chars.next() {
                    Some('n') => cmd.push('\n'),
                    Some(o) => cmd.push(o),
                    None => cmd.push('\\'),
                }
            } else {
                cmd.push(c);
            }
        }
        out.push(Entry { cmd, ts: 0, cwd: String::new(), code: None, ms: None, sid: 0 });
    }
    out
}

fn parse_lines(text: &str) -> Vec<Entry> {
    text.lines().filter_map(|l| serde_json::from_str::<Entry>(l).ok()).collect()
}

impl Store {
    /// Mở kho; nạp toàn bộ lịch sử (giữ tối đa `max` lệnh mới nhất).
    pub fn open(path: PathBuf, legacy: &Path, max: usize) -> (Store, Vec<Entry>) {
        let sid = std::process::id();
        let mut entries = if path.exists() {
            parse_lines(&std::fs::read_to_string(&path).unwrap_or_default())
        } else {
            let imported = read_legacy(legacy);
            let mut store = Store { path: path.clone(), offset: 0, sid };
            store.rewrite(&imported);
            imported
        };
        // Gọn file khi vượt quá giới hạn 25%.
        let mut store = Store { path, offset: 0, sid };
        if entries.len() > max + max / 4 {
            entries.drain(..entries.len() - max);
            store.rewrite(&entries);
        }
        store.offset = std::fs::metadata(&store.path).map(|m| m.len()).unwrap_or(0);
        (store, entries)
    }

    /// Ghi nối 1 lệnh (1 lần `write` với O_APPEND — an toàn khi nhiều phiên).
    pub fn append(&mut self, e: &Entry) {
        let Ok(mut line) = serde_json::to_string(e) else { return };
        line.push('\n');
        if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(&self.path) {
            let _ = f.write_all(line.as_bytes());
        }
    }

    /// Lệnh mới do PHIÊN KHÁC ghi từ lần đọc trước.
    pub fn poll(&mut self) -> Vec<Entry> {
        let Ok(mut f) = std::fs::File::open(&self.path) else { return Vec::new() };
        let len = f.metadata().map(|m| m.len()).unwrap_or(0);
        if len < self.offset {
            // File bị ghi lại (history -c / --scrub ở phiên khác).
            self.offset = len;
            return Vec::new();
        }
        if len == self.offset || f.seek(SeekFrom::Start(self.offset)).is_err() {
            return Vec::new();
        }
        let mut buf = String::new();
        if f.read_to_string(&mut buf).is_err() {
            return Vec::new();
        }
        // Chỉ lấy các dòng đã ghi trọn.
        let complete = buf.rfind('\n').map_or(0, |p| p + 1);
        self.offset += complete as u64;
        parse_lines(&buf[..complete]).into_iter().filter(|e| e.sid != self.sid).collect()
    }

    /// Ghi lại toàn bộ (sau `history -c`, `-d`, `--scrub`) — qua file tạm + rename.
    pub fn rewrite(&mut self, entries: &[Entry]) {
        let tmp = self.path.with_extension("jsonl.tmp");
        let mut text = String::new();
        for e in entries {
            if let Ok(l) = serde_json::to_string(e) {
                text.push_str(&l);
                text.push('\n');
            }
        }
        use std::os::unix::fs::OpenOptionsExt;
        let ok = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&tmp)
            .and_then(|mut f| f.write_all(text.as_bytes()))
            .and_then(|_| std::fs::rename(&tmp, &self.path))
            .is_ok();
        if ok {
            self.offset = text.len() as u64;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp() -> PathBuf {
        let d = std::env::temp_dir().join(format!("jaksh-hist-test-{}-{}", std::process::id(), rand_suffix()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn rand_suffix() -> u128 {
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
    }

    #[test]
    fn imports_legacy_and_appends() {
        let d = tmp();
        std::fs::write(d.join("history"), "#V2\nls -la\nfor x in 1\\ndo :\\ndone\necho a\\\\b\n").unwrap();
        let (mut store, entries) = Store::open(d.join("history.jsonl"), &d.join("history"), 100);
        let cmds: Vec<&str> = entries.iter().map(|e| e.cmd.as_str()).collect();
        assert_eq!(cmds, vec!["ls -la", "for x in 1\ndo :\ndone", "echo a\\b"]);
        store.append(&Entry::new("pwd", "/tmp"));
        let (_, again) = Store::open(d.join("history.jsonl"), &d.join("history"), 100);
        assert_eq!(again.last().unwrap().cmd, "pwd");
        assert_eq!(again.last().unwrap().cwd, "/tmp");
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn poll_sees_other_sessions_only() {
        let d = tmp();
        let path = d.join("history.jsonl");
        let (mut a, _) = Store::open(path.clone(), &d.join("none"), 100);
        a.append(&Entry::new("mine", "/"));
        // Phiên khác (sid khác) ghi thêm.
        let mut other = Entry::new("theirs", "/x");
        other.sid = 999_999;
        let mut f = OpenOptions::new().append(true).open(&path).unwrap();
        writeln!(f, "{}", serde_json::to_string(&other).unwrap()).unwrap();
        let new = a.poll();
        assert_eq!(new.iter().map(|e| e.cmd.as_str()).collect::<Vec<_>>(), vec!["theirs"]);
        assert!(a.poll().is_empty());
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn trims_to_max() {
        let d = tmp();
        let path = d.join("history.jsonl");
        let (mut s, _) = Store::open(path.clone(), &d.join("none"), 10);
        for i in 0..20 {
            s.append(&Entry::new(&format!("cmd{i}"), "/"));
        }
        let (_, entries) = Store::open(path, &d.join("none"), 10);
        assert_eq!(entries.len(), 10);
        assert_eq!(entries[0].cmd, "cmd10");
        std::fs::remove_dir_all(&d).ok();
    }
}
