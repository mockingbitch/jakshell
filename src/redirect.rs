//! Redirect kiểu POSIX/bash, dùng chung cho:
//!   • lệnh ngoài — áp trong tiến trình con (pre_exec, sau khi std đã nối pipe);
//!   • builtin — áp tạm lên fd của chính shell rồi khôi phục khi xong.
//!
//! Áp theo ĐÚNG thứ tự viết: `cmd > f 2>&1` (cả hai vào f) khác `cmd 2>&1 > f`
//! (stderr ra terminal, stdout vào f).

use anyhow::{anyhow, Result};
use std::cell::RefCell;
use std::fs::{File, OpenOptions};
use std::io::{Seek, SeekFrom, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::process::CommandExt;
use std::process::Command;
use std::rc::Rc;

use crate::expand::{expand_string, expand_words};
use crate::parser::{Redirect, RedirectKind};
use crate::shell::Shell;

/// Một bước thao tác trên fd đích.
pub enum FdAction {
    /// Trỏ fd đích tới file / pipe đã mở sẵn.
    Open(OwnedFd),
    /// `N>&M` — fd đích thành bản sao của fd M (tại thời điểm áp).
    Dup(i32),
    /// `N>&-` — đóng fd đích.
    Close,
}

/// Danh sách thao tác theo thứ tự, đã mở sẵn file (lỗi mở file báo trước khi
/// spawn, như bash).
#[derive(Default)]
pub struct Plan {
    actions: Vec<(i32, FdAction)>,
}

impl Plan {
    pub fn is_empty(&self) -> bool {
        self.actions.is_empty()
    }

    pub fn push(&mut self, fd: i32, action: FdAction) {
        self.actions.push((fd, action));
    }
}

/// Đưa fd lên vùng cao (≥ 100, CLOEXEC) để không đụng fd đích như 3, 4…
fn lift(f: impl Into<OwnedFd>) -> Result<OwnedFd> {
    let f: OwnedFd = f.into();
    let high = unsafe { libc::fcntl(f.as_raw_fd(), libc::F_DUPFD_CLOEXEC, 100) };
    if high < 0 {
        return Err(anyhow!("không cấp được fd: {}", std::io::Error::last_os_error()));
    }
    Ok(unsafe { OwnedFd::from_raw_fd(high) })
}

/// File tạm chứa sẵn `data`, đã unlink (chỉ còn fd) — nguồn cho here-doc /
/// here-string. Không dùng pipe để khỏi kẹt khi nội dung lớn hơn buffer.
fn temp_input(data: &str) -> Result<File> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    let path = std::env::temp_dir().join(format!(
        "jaksh-heredoc-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    let mut f = OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&path)
        .map_err(|e| anyhow!("here-doc: không tạo được file tạm: {}", e))?;
    let _ = std::fs::remove_file(&path);
    f.write_all(data.as_bytes())?;
    f.seek(SeekFrom::Start(0))?;
    Ok(f)
}

fn fd_is_open(fd: i32) -> bool {
    unsafe { libc::fcntl(fd, libc::F_GETFD) >= 0 }
}

/// Mở file / chuẩn bị nguồn cho mọi redirect của 1 lệnh.
pub fn plan(shell: &Rc<RefCell<Shell>>, redirects: &[Redirect]) -> Result<Plan> {
    let mut plan = Plan::default();
    // fd đã được các redirect trước đó trong cùng lệnh mở — hợp lệ làm nguồn `>&`.
    let mut opened: Vec<i32> = Vec::new();
    for r in redirects {
        match r.kind {
            RedirectKind::HereDoc => {
                let body = expand_string(shell, &r.target)?;
                plan.push(r.fd, FdAction::Open(lift(temp_input(&body)?)?));
                opened.push(r.fd);
                continue;
            }
            RedirectKind::HereString => {
                let mut text = expand_string(shell, &r.target)?;
                text.push('\n');
                plan.push(r.fd, FdAction::Open(lift(temp_input(&text)?)?));
                opened.push(r.fd);
                continue;
            }
            _ => {}
        }

        let pieces = expand_words(shell, std::slice::from_ref(&r.target))?;
        if pieces.len() != 1 {
            return Err(anyhow!("redirect mơ hồ (đích bung ra {} từ)", pieces.len()));
        }
        let target = pieces.into_iter().next().unwrap_or_default();
        if target.is_empty() {
            return Err(anyhow!("đích redirect rỗng"));
        }

        if matches!(r.kind, RedirectKind::DupOut | RedirectKind::DupIn) {
            if target == "-" {
                plan.push(r.fd, FdAction::Close);
                opened.retain(|&f| f != r.fd);
                continue;
            }
            if let Ok(src) = target.parse::<i32>() {
                if !opened.contains(&src) && !fd_is_open(src) {
                    return Err(anyhow!("{}: fd không hợp lệ (Bad file descriptor)", src));
                }
                plan.push(r.fd, FdAction::Dup(src));
                opened.push(r.fd);
                continue;
            }
            if r.kind == RedirectKind::DupIn {
                return Err(anyhow!("{}: cần số fd sau '<&'", target));
            }
            // `>&file` (không phải số) = `&>file` như bash.
        }

        let noclobber = shell.borrow().opts.noclobber;
        let open_trunc = |path: &str| -> Result<File> {
            if noclobber && std::fs::metadata(path).map(|m| m.is_file()).unwrap_or(false) {
                return Err(anyhow!("{}: không ghi đè file đã có (set -o noclobber; dùng >| để ép)", path));
            }
            OpenOptions::new().write(true).create(true).truncate(true).open(path)
                .map_err(|e| anyhow!("{}: {}", path, e))
        };
        let open_append = |path: &str| -> Result<File> {
            OpenOptions::new().append(true).create(true).open(path).map_err(|e| anyhow!("{}: {}", path, e))
        };
        match r.kind {
            RedirectKind::In => {
                let f = File::open(&target).map_err(|e| anyhow!("{}: {}", target, e))?;
                plan.push(r.fd, FdAction::Open(lift(f)?));
            }
            RedirectKind::Out => plan.push(r.fd, FdAction::Open(lift(open_trunc(&target)?)?)),
            RedirectKind::Clobber => {
                let f = OpenOptions::new().write(true).create(true).truncate(true).open(&target)
                    .map_err(|e| anyhow!("{}: {}", target, e))?;
                plan.push(r.fd, FdAction::Open(lift(f)?));
            }
            RedirectKind::Append => plan.push(r.fd, FdAction::Open(lift(open_append(&target)?)?)),
            RedirectKind::ReadWrite => {
                let f = OpenOptions::new().read(true).write(true).create(true).open(&target)
                    .map_err(|e| anyhow!("{}: {}", target, e))?;
                plan.push(r.fd, FdAction::Open(lift(f)?));
            }
            RedirectKind::AllOut | RedirectKind::DupOut => {
                plan.push(1, FdAction::Open(lift(open_trunc(&target)?)?));
                plan.push(2, FdAction::Dup(1));
                opened.push(2);
            }
            RedirectKind::AllAppend => {
                plan.push(1, FdAction::Open(lift(open_append(&target)?)?));
                plan.push(2, FdAction::Dup(1));
                opened.push(2);
            }
            RedirectKind::DupIn | RedirectKind::HereDoc | RedirectKind::HereString => unreachable!(),
        }
        opened.push(r.fd);
    }
    Ok(plan)
}

/// Áp plan trong tiến trình con (chạy sau khi std đã dup pipe vào 0/1/2, nên
/// `cmd 2>&1 | grep` đưa stderr vào pipe đúng như bash).
pub fn apply_to_command(cmd: &mut Command, plan: Plan) {
    if plan.is_empty() {
        return;
    }
    let actions = plan.actions;
    unsafe {
        cmd.pre_exec(move || {
            for (target, action) in &actions {
                let rc = match action {
                    FdAction::Open(fd) => libc::dup2(fd.as_raw_fd(), *target),
                    FdAction::Dup(src) if src == target => {
                        // dup2(x, x) không xoá CLOEXEC — tự xoá để fd sống qua exec.
                        libc::fcntl(*target, libc::F_SETFD, 0)
                    }
                    FdAction::Dup(src) => libc::dup2(*src, *target),
                    FdAction::Close => {
                        libc::close(*target);
                        0
                    }
                };
                if rc < 0 {
                    return Err(std::io::Error::last_os_error());
                }
            }
            Ok(())
        });
    }
}

/// Redirect tạm thời trên fd của chính shell (cho builtin). Drop = khôi phục.
pub struct FdGuard {
    /// (fd đích, bản sao fd cũ — `None` nếu trước đó fd đích chưa mở).
    saved: Vec<(i32, Option<OwnedFd>)>,
}

fn flush_std() {
    let _ = std::io::stdout().flush();
    let _ = std::io::stderr().flush();
}

pub fn apply_in_process(plan: Plan) -> Result<FdGuard> {
    let mut guard = FdGuard { saved: Vec::new() };
    if plan.is_empty() {
        return Ok(guard);
    }
    flush_std();
    for (target, action) in plan.actions {
        let saved = unsafe {
            let d = libc::fcntl(target, libc::F_DUPFD_CLOEXEC, 10);
            (d >= 0).then(|| OwnedFd::from_raw_fd(d))
        };
        let rc = unsafe {
            match &action {
                FdAction::Open(fd) => libc::dup2(fd.as_raw_fd(), target),
                FdAction::Dup(src) if *src == target => 0,
                FdAction::Dup(src) => libc::dup2(*src, target),
                FdAction::Close => {
                    libc::close(target);
                    0
                }
            }
        };
        guard.saved.push((target, saved));
        if rc < 0 {
            return Err(anyhow!("fd {}: {}", target, std::io::Error::last_os_error()));
        }
    }
    Ok(guard)
}

impl FdGuard {
    /// Giữ nguyên các thay đổi (dùng cho `exec 3>file` không có lệnh).
    pub fn persist(mut self) {
        self.saved.clear();
    }
}

impl Drop for FdGuard {
    fn drop(&mut self) {
        flush_std();
        for (target, saved) in self.saved.drain(..).rev() {
            unsafe {
                match saved {
                    Some(fd) => {
                        libc::dup2(fd.as_raw_fd(), target);
                    }
                    None => {
                        libc::close(target);
                    }
                }
            }
        }
    }
}
