//! Kiểm tra & thông báo phiên bản mới của JakShell.
//!
//! Mô hình (đã chốt với người dùng):
//!   - Kiểm tra CHẠY NỀN, không bao giờ làm chậm lúc mở shell. Mỗi
//!     `interval_hours` (mặc định 24h) spawn một tiến trình con
//!     `jaksh --update-refresh` gọi mạng lấy tag mới nhất rồi ghi cache.
//!   - Thông báo dựa trên cache: nếu cache (từ lần check trước) cho biết có
//!     bản mới hơn → hiện thông báo ở LẦN mở shell kế tiếp.
//!   - Hỏi cập nhật 1 LẦN cho mỗi version: Có / Để sau / Bỏ qua bản này / Không.
//!
//! Cache: `~/.config/jaksh/update-check.json`.

use std::cell::RefCell;
use std::io::{IsTerminal, Write};
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::shell::Shell;

/// Trạng thái lưu giữa các phiên để (a) giãn nhịp gọi mạng, (b) tôn trọng lựa
/// chọn "để sau" / "bỏ qua" của người dùng.
#[derive(Default, Serialize, Deserialize)]
struct Cache {
    /// Unix secs của lần gọi mạng gần nhất.
    #[serde(default)]
    last_check: u64,
    /// Tag mới nhất thấy trên remote (vd "v1.0.9"). Rỗng = chưa biết.
    #[serde(default)]
    latest_version: String,
    /// Version người dùng chọn "bỏ qua" — im cho tới khi có bản mới hơn nó.
    #[serde(default)]
    skip_version: String,
    /// Chọn "để sau": im cho tới mốc unix secs này.
    #[serde(default)]
    remind_after: u64,
}

pub fn current_version() -> &'static str {
    env!("JAKSH_VERSION")
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn cache_path() -> PathBuf {
    dirs::home_dir()
        .map(|h| h.join(".config").join("jaksh").join("update-check.json"))
        .unwrap_or_else(|| PathBuf::from("update-check.json"))
}

fn read_cache() -> Cache {
    std::fs::read_to_string(cache_path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn write_cache(c: &Cache) {
    let path = cache_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(s) = serde_json::to_string_pretty(c) {
        // Ghi vào file tạm (tên kèm PID để 2 tiến trình không đụng nhau) rồi
        // rename — đổi tên là atomic trên cùng thư mục, nên không bao giờ đọc
        // phải file ghi dở khi refresh nền và prompt cùng ghi.
        let tmp = path.with_file_name(format!("update-check.json.tmp.{}", std::process::id()));
        if std::fs::write(&tmp, &s).is_ok() {
            let _ = std::fs::rename(&tmp, &path);
        }
    }
}

/// (major, minor, patch) từ chuỗi version bất kỳ: "v1.0.8", "1.0.8",
/// "v1.0.8-3-gabc123" (git describe giữa 2 tag), "v1.2.10-dirty". Bỏ tiền tố
/// `v` và mọi hậu tố sau dấu `-`. None nếu không parse được X.Y.Z.
fn parse_semver(v: &str) -> Option<(u64, u64, u64)> {
    let v = v.trim().trim_start_matches('v');
    let core = v.split('-').next().unwrap_or(v);
    let mut it = core.split('.');
    let major = it.next()?.parse().ok()?;
    let minor = it.next().unwrap_or("0").parse().ok()?;
    let patch = it.next().unwrap_or("0").parse().ok()?;
    Some((major, minor, patch))
}

/// true nếu `candidate` mới hơn `current` theo semver (parse fail → false).
fn is_newer(candidate: &str, current: &str) -> bool {
    match (parse_semver(candidate), parse_semver(current)) {
        (Some(a), Some(b)) => a > b,
        _ => false,
    }
}

// ─────────────────────────── refresh nền (gọi mạng) ───────────────────────────

/// Chạy bởi tiến trình con `jaksh --update-refresh`. Gọi `git ls-remote` lấy
/// tag mới nhất rồi ghi cache. KHÔNG in ra gì (mọi fd đã bị nuốt).
pub fn refresh_blocking() {
    // Hỏi remote TRƯỚC (phần tốn thời gian). None nếu không có source / mạng lỗi.
    let latest = crate::jak::source_dir().and_then(|src| query_latest_tag(&src));

    // Đọc LẠI cache ngay trước khi ghi (không dùng snapshot cũ): nếu trong lúc
    // gọi mạng, prompt foreground đã lưu skip_version/remind_after thì giữ
    // nguyên — chỉ ghi đè các field do refresh sở hữu.
    let cache = merge_refresh(read_cache(), latest, now_secs());
    write_cache(&cache);
}

/// Gộp kết quả refresh vào cache hiện có (đã đọc lại tươi). LUÔN cập nhật
/// last_check (kể cả khi `latest` = None do mạng/source lỗi) để backoff đúng
/// interval, không spawn lại mỗi lần mở shell. Chỉ đụng các field refresh sở
/// hữu (last_check, latest_version, và reset skip khi có bản mới hơn) — KHÔNG
/// đè remind_after/skip_version mà foreground có thể vừa ghi.
fn merge_refresh(mut cache: Cache, latest: Option<String>, now: u64) -> Cache {
    cache.last_check = now;
    if let Some(tag) = latest {
        // Có bản mới HƠN bản từng biết → release mới xuất hiện: huỷ "để sau"
        // (vốn nhắm bản cũ) để báo về bản mới ngay, không đợi hết remind_hours.
        if is_newer(&tag, &cache.latest_version) {
            cache.remind_after = 0;
        }
        // Có bản mới HƠN bản từng "bỏ qua" → huỷ skip để hỏi lại.
        if !cache.skip_version.is_empty() && is_newer(&tag, &cache.skip_version) {
            cache.skip_version.clear();
        }
        cache.latest_version = tag;
    }
    cache
}

/// `git ls-remote` lấy tag semver mới nhất trên remote. None nếu không có
/// source / git lỗi / mạng lỗi / không có tag hợp lệ.
/// ls-remote chỉ đọc, không đụng working tree; `--refs` bỏ ref bóc `^{}`.
/// GIT_TERMINAL_PROMPT=0: không bao giờ chờ nhập credential (tránh treo).
fn query_latest_tag(source: &std::path::Path) -> Option<String> {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(source)
        .args(["ls-remote", "--refs", "--tags", "origin", "v*"])
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let mut best: Option<(u64, u64, u64)> = None;
    let mut best_str = String::new();
    for line in text.lines() {
        // Dạng: "<sha>\trefs/tags/v1.0.9"
        let Some(tag) = line
            .split('\t')
            .nth(1)
            .and_then(|r| r.strip_prefix("refs/tags/"))
        else {
            continue;
        };
        if let Some(sv) = parse_semver(tag) {
            if best.map_or(true, |b| sv > b) {
                best = Some(sv);
                best_str = tag.to_string();
            }
        }
    }
    if best_str.is_empty() {
        None
    } else {
        Some(best_str)
    }
}

/// Spawn tiến trình con chạy refresh ở nền — KHÔNG chờ. Mọi fd → null nên nó
/// không in gì ra terminal; shell tiếp tục ngay lập tức.
fn spawn_background_refresh() {
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    let mut cmd = std::process::Command::new(exe);
    cmd.arg("--update-refresh")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // setsid: tách tiến trình refresh khỏi terminal điều khiển + process
        // group của shell, nên Ctrl-C lúc nó đang `git ls-remote` không giết nó
        // (refresh vẫn ghi xong cache → last_check tiến đúng, không spawn lại).
        unsafe {
            cmd.pre_exec(|| {
                libc::setsid();
                Ok(())
            });
        }
    }
    let _ = cmd.spawn();
}

// ─────────────────────────── thông báo + hỏi cập nhật ─────────────────────────

/// Gọi ở đầu phiên tương tác (sau banner, trước prompt đầu tiên).
/// (a) Nếu cache cũ → spawn refresh nền cho lần sau.
/// (b) Nếu cache cho biết có bản mới → thông báo và (tuỳ config) hỏi cập nhật.
pub fn startup_check(shell: &Rc<RefCell<Shell>>) {
    let cfg = shell.borrow().update.clone();
    if !cfg.check {
        return;
    }
    // Chỉ khi thực sự tương tác (không phải pipe / redirect).
    if !std::io::stdin().is_terminal() || !std::io::stdout().is_terminal() {
        return;
    }
    // Không có source repo hợp lệ → không thể tự cập nhật, bỏ qua hẳn.
    if crate::jak::source_dir().is_none() {
        return;
    }

    let cache = read_cache();
    let now = now_secs();

    // (a) Quá hạn interval (hoặc chưa từng check) → làm tươi cache cho LẦN SAU.
    let interval = cfg.interval_hours.saturating_mul(3600);
    if now.saturating_sub(cache.last_check) >= interval {
        spawn_background_refresh();
    }

    // (b) Quyết định thông báo dựa trên cache HIỆN TẠI.
    let latest = cache.latest_version.trim().to_string();
    if latest.is_empty() || !is_newer(&latest, current_version()) {
        return;
    }
    if cache.skip_version.trim() == latest {
        return; // đã "bỏ qua" đúng bản này
    }
    if now < cache.remind_after {
        return; // đang trong thời gian "để sau"
    }

    notify(shell, &latest);
}

fn notify(shell: &Rc<RefCell<Shell>>, latest: &str) {
    let cur = current_version();
    println!(
        "\x1b[1;36m⬆ JakShell {}\x1b[0m đã có \x1b[2m(bạn đang dùng {}).\x1b[0m",
        latest, cur
    );

    if !shell.borrow().update.prompt {
        // Chế độ chỉ-nhắc: in 1 dòng, không hỏi.
        println!(
            "  \x1b[2mGõ \x1b[0m\x1b[36mjak update\x1b[0m\x1b[2m để cập nhật.\x1b[0m\n"
        );
        return;
    }

    // Xả type-ahead: bỏ mọi ký tự user lỡ gõ TRƯỚC khi prompt hiện (vd vừa mở
    // terminal đã gõ luôn `ls<Enter>`). Không làm vậy thì read_line bên dưới
    // nuốt mất lệnh đó làm "câu trả lời" cho prompt.
    #[cfg(unix)]
    unsafe {
        libc::tcflush(libc::STDIN_FILENO, libc::TCIFLUSH);
    }

    print!(
        "  Cập nhật ngay? \x1b[2m[\x1b[0my\x1b[2m] có   [\x1b[0ml/Enter\x1b[2m] để sau   [\x1b[0ms\x1b[2m] bỏ qua bản này   [\x1b[0mn\x1b[2m] không\x1b[0m  "
    );
    let _ = std::io::stdout().flush();

    let mut line = String::new();
    if std::io::stdin().read_line(&mut line).is_err() {
        println!();
        return;
    }
    let ans = line.trim().to_lowercase();

    // Đọc lại cache ngay trước khi ghi để không đè kết quả refresh nền vừa chạy.
    let mut cache = read_cache();
    match ans.as_str() {
        "y" | "yes" | "c" | "co" | "có" => {
            println!();
            let _ = crate::jak::run_self_update(shell);
            // Binary đang chạy vẫn là bản cũ — self_update đã in lời nhắc mở
            // terminal mới.
        }
        "s" | "skip" | "bo qua" | "bỏ qua" => {
            cache.skip_version = latest.to_string();
            write_cache(&cache);
            println!(
                "  \x1b[2mĐã bỏ qua {}. Sẽ chỉ nhắc lại khi có bản mới hơn.\x1b[0m\n",
                latest
            );
        }
        // Enter (rỗng) / "l" / "later" → để sau: im trong remind_hours.
        "" | "l" | "later" | "de sau" | "để sau" => {
            let remind = shell.borrow().update.remind_hours.saturating_mul(3600);
            cache.remind_after = now_secs().saturating_add(remind);
            write_cache(&cache);
            println!("  \x1b[2mOK, sẽ nhắc lại sau.\x1b[0m\n");
        }
        // "n"/"không" HOẶC input lạ (gõ nhầm / lệnh bị nuốt) → chỉ im PHIÊN này,
        // KHÔNG snooze, để lệnh gõ nhầm không vô tình hoãn thông báo.
        _ => {
            println!();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_semver_handles_describe_and_prefix() {
        assert_eq!(parse_semver("v1.0.8"), Some((1, 0, 8)));
        assert_eq!(parse_semver("1.0.8"), Some((1, 0, 8)));
        assert_eq!(parse_semver("v1.2.10-3-gabc123"), Some((1, 2, 10)));
        assert_eq!(parse_semver("v1.0.8-dirty"), Some((1, 0, 8)));
        assert_eq!(parse_semver("v2.0"), Some((2, 0, 0)));
        assert_eq!(parse_semver("unknown"), None);
        assert_eq!(parse_semver(""), None);
    }

    #[test]
    fn is_newer_semver_ordering() {
        assert!(is_newer("v1.0.9", "v1.0.8"));
        assert!(is_newer("v1.1.0", "v1.0.99"));
        assert!(is_newer("v2.0.0", "v1.9.9"));
        assert!(!is_newer("v1.0.8", "v1.0.8"));
        assert!(!is_newer("v1.0.7", "v1.0.8"));
        // Build dev giữa 2 tag: base bằng tag → không coi là mới.
        assert!(!is_newer("v1.0.8", "v1.0.8-3-gabc123"));
        // Không parse được → không bao giờ báo có bản mới.
        assert!(!is_newer("garbage", "v1.0.8"));
        assert!(!is_newer("v1.0.9", "unknown"));
    }

    #[test]
    fn merge_refresh_always_advances_last_check_even_on_failure() {
        // Mạng/source lỗi → latest=None, nhưng last_check vẫn tiến (backoff).
        let c = merge_refresh(Cache::default(), None, 12345);
        assert_eq!(c.last_check, 12345);
        assert_eq!(c.latest_version, ""); // không có dữ liệu mới
    }

    #[test]
    fn merge_refresh_preserves_foreground_choices() {
        // Foreground vừa lưu skip + remind; refresh thấy ĐÚNG bản đã skip
        // (không mới hơn) → phải GIỮ NGUYÊN skip_version và remind_after.
        let mut cache = Cache::default();
        cache.skip_version = "v1.0.9".into();
        cache.remind_after = 999;
        let c = merge_refresh(cache, Some("v1.0.9".into()), 50);
        assert_eq!(c.skip_version, "v1.0.9", "không được xoá skip user vừa đặt");
        assert_eq!(c.remind_after, 999, "không được đè remind user vừa đặt");
        assert_eq!(c.latest_version, "v1.0.9");
        assert_eq!(c.last_check, 50);
    }

    #[test]
    fn merge_refresh_resets_skip_when_strictly_newer_appears() {
        let mut cache = Cache::default();
        cache.skip_version = "v1.0.9".into();
        let c = merge_refresh(cache, Some("v1.1.0".into()), 7);
        assert_eq!(c.skip_version, "", "bản mới hơn → reset skip để hỏi lại");
        assert_eq!(c.latest_version, "v1.1.0");
    }

    #[test]
    fn merge_refresh_resets_remind_when_newer_release_appears() {
        // User hoãn v1.0.9 ("để sau"); v1.1.0 ra trong lúc còn hạn hoãn →
        // huỷ remind cũ để báo về v1.1.0 ngay.
        let mut cache = Cache::default();
        cache.latest_version = "v1.0.9".into();
        cache.remind_after = 999;
        let c = merge_refresh(cache, Some("v1.1.0".into()), 10);
        assert_eq!(c.remind_after, 0, "release mới → huỷ 'để sau' của bản cũ");
        assert_eq!(c.latest_version, "v1.1.0");
        // Nhưng cùng một bản (không mới hơn) thì GIỮ remind đang chờ.
        let mut same = Cache::default();
        same.latest_version = "v1.0.9".into();
        same.remind_after = 999;
        let c2 = merge_refresh(same, Some("v1.0.9".into()), 10);
        assert_eq!(c2.remind_after, 999, "cùng bản → giữ nguyên 'để sau'");
    }

    #[test]
    fn cache_roundtrips_and_tolerates_missing_fields() {
        // Thiếu field → default (không panic).
        let c: Cache = serde_json::from_str("{}").unwrap();
        assert_eq!(c.last_check, 0);
        assert_eq!(c.latest_version, "");
        let c2: Cache =
            serde_json::from_str(r#"{"latest_version":"v1.0.9","skip_version":"v1.0.9"}"#).unwrap();
        assert_eq!(c2.latest_version, "v1.0.9");
        assert_eq!(c2.skip_version, "v1.0.9");
        assert_eq!(c2.remind_after, 0);
    }
}
