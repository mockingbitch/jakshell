//! Build script: nhúng thông tin version vào binary.
//!
//! Các biến môi trường tạo ra (đọc bằng `env!`):
//!   JAKSH_VERSION       — `git describe --tags --always --dirty`
//!                         fallback: CARGO_PKG_VERSION
//!   JAKSH_COMMIT_HASH   — git rev-parse HEAD (short)
//!   JAKSH_COMMIT_DATE   — git log -1 --format=%ci
//!   JAKSH_BUILD_DATE    — thời điểm build (UTC, ISO 8601)
//!   JAKSH_RUSTC         — rustc --version

use std::process::Command;

fn main() {
    let version = git_describe()
        .unwrap_or_else(|| std::env::var("CARGO_PKG_VERSION").unwrap_or_else(|_| "unknown".into()));
    let commit = git_output(&["rev-parse", "--short", "HEAD"]).unwrap_or_else(|| "unknown".into());
    let commit_date = git_output(&["log", "-1", "--format=%ci"]).unwrap_or_else(|| "unknown".into());
    let build_date = chrono_now_iso();
    let rustc = rustc_version().unwrap_or_else(|| "unknown".into());

    println!("cargo:rustc-env=JAKSH_VERSION={}", version);
    println!("cargo:rustc-env=JAKSH_COMMIT_HASH={}", commit);
    println!("cargo:rustc-env=JAKSH_COMMIT_DATE={}", commit_date);
    println!("cargo:rustc-env=JAKSH_BUILD_DATE={}", build_date);
    println!("cargo:rustc-env=JAKSH_RUSTC={}", rustc);

    // Rebuild khi git state đổi.
    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-changed=.git/refs/tags");
    println!("cargo:rerun-if-changed=.git/index");
    println!("cargo:rerun-if-changed=build.rs");
    // Touch CHANGELOG để include_str! refresh khi đổi.
    println!("cargo:rerun-if-changed=CHANGELOG.md");
}

fn git_describe() -> Option<String> {
    git_output(&["describe", "--tags", "--always", "--dirty=-dirty"])
}

fn git_output(args: &[&str]) -> Option<String> {
    let out = Command::new("git").args(args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8(out.stdout).ok()?;
    let trimmed = s.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn rustc_version() -> Option<String> {
    let rustc = std::env::var("RUSTC").unwrap_or_else(|_| "rustc".into());
    let out = Command::new(rustc).arg("--version").output().ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8(out.stdout).ok()?;
    Some(s.trim().to_string())
}

fn chrono_now_iso() -> String {
    // Format thủ công bằng SystemTime để không cần dep ngoài cho build script.
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format_iso_utc(secs)
}

fn format_iso_utc(unix_secs: u64) -> String {
    // Chuyển epoch → YYYY-MM-DD HH:MM:SS UTC (không phụ thuộc chrono — dùng trong build.rs)
    let secs_per_day = 86400u64;
    let mut days = (unix_secs / secs_per_day) as i64;
    let sec_of_day = unix_secs % secs_per_day;
    let hour = (sec_of_day / 3600) as u32;
    let minute = ((sec_of_day % 3600) / 60) as u32;
    let second = (sec_of_day % 60) as u32;

    // Epoch là 1970-01-01 (Thursday). Tính year/month/day.
    let mut year: i64 = 1970;
    loop {
        let yd = if is_leap(year) { 366 } else { 365 };
        if days < yd {
            break;
        }
        days -= yd;
        year += 1;
    }
    let month_lengths: [i64; 12] = [
        31,
        if is_leap(year) { 29 } else { 28 },
        31, 30, 31, 30, 31, 31, 30, 31, 30, 31,
    ];
    let mut month = 0usize;
    while month < 12 && days >= month_lengths[month] {
        days -= month_lengths[month];
        month += 1;
    }
    let day = (days + 1) as u32;
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02} UTC",
        year,
        (month as u32) + 1,
        day,
        hour,
        minute,
        second
    )
}

fn is_leap(y: i64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || (y % 400 == 0)
}
