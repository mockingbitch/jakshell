//! Build script: nhúng version từ `git describe` vào binary.
//!
//! Ưu tiên: `git describe --tags --always --dirty`
//! Fallback: CARGO_PKG_VERSION từ Cargo.toml.
//!
//! Expose qua biến môi trường JAKSH_VERSION — đọc bằng `env!("JAKSH_VERSION")`.

use std::process::Command;

fn main() {
    let version = git_describe()
        .unwrap_or_else(|| std::env::var("CARGO_PKG_VERSION").unwrap_or_else(|_| "unknown".into()));
    println!("cargo:rustc-env=JAKSH_VERSION={}", version);

    // Rebuild nếu git state đổi.
    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-changed=.git/refs/tags");
    println!("cargo:rerun-if-changed=.git/index");
    println!("cargo:rerun-if-changed=build.rs");
}

fn git_describe() -> Option<String> {
    let out = Command::new("git")
        .args(["describe", "--tags", "--always", "--dirty=-dirty"])
        .output()
        .ok()?;
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
