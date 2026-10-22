//! Job control kiểu bash cho REPL tương tác.
//!
//! Mô hình (giống bash/zsh):
//!   • Shell ở process group riêng, giữ terminal khi đứng ở prompt. Shell BẮT
//!     SIGINT/SIGQUIT bằng handler (không chết) và BỎ QUA SIGTSTP/SIGTTIN/SIGTTOU.
//!   • Mỗi job foreground (lệnh / pipeline) có process group riêng và được trao
//!     terminal → Ctrl-C / Ctrl-Z chỉ tới job đó, không bao giờ tới shell.
//!   • Job xong / bị dừng → shell lấy lại terminal và khôi phục chế độ terminal
//!     (phòng chương trình bị giết khi đang ở raw mode, vd vim, less).
//!
//! Lệnh chạy NGOÀI cơ chế này (tiện ích `jak`, `--jak`) và `$(...)` ở chung
//! group với shell: Ctrl-C giết lệnh con, còn shell chỉ ghi nhận cờ ngắt nhờ
//! handler; Ctrl-Z bị bỏ qua (kế thừa SIG_IGN) nên không thể treo shell.
//!
//! Không tương tác (`-c`, script): không bật gì — tín hiệu mặc định như bash.

use std::cell::{Cell, RefCell};
use std::os::unix::process::CommandExt;
use std::process::Command;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use crate::shell::{JobState, Shell};

/// Có Ctrl-C trong lúc chạy dòng lệnh hiện tại (job bị SIGINT giết, hoặc chính
/// shell nhận SIGINT khi đang chạy lệnh in-process) → bỏ phần còn lại của dòng.
static INTERRUPTED: AtomicBool = AtomicBool::new(false);

const JOB_SIGNALS: [libc::c_int; 5] = [libc::SIGINT, libc::SIGQUIT, libc::SIGTSTP, libc::SIGTTIN, libc::SIGTTOU];

struct State {
    /// fd terminal riêng của shell (dup từ stdin, CLOEXEC) — vẫn đúng kể cả
    /// khi lệnh redirect stdin (`cmd < file`).
    tty: i32,
    shell_pgid: i32,
    /// Group đang giữ terminal lúc jaksh khởi động — trả lại khi thoát.
    original_pgid: i32,
    /// Chế độ terminal "sạch" của shell, khôi phục sau mỗi job.
    tmodes: libc::termios,
}

thread_local! {
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
    /// Đã cảnh báo "còn job đang dừng" cho lần `exit` trước chưa.
    static EXIT_WARNED: Cell<bool> = const { Cell::new(false) };
}

/// Tín hiệu đã tới nhưng trap chưa chạy (bit thứ N = tín hiệu N).
static PENDING: AtomicU64 = AtomicU64::new(0);

extern "C" fn on_sigint(_: libc::c_int) {
    INTERRUPTED.store(true, Ordering::SeqCst);
    PENDING.fetch_or(1 << libc::SIGINT, Ordering::SeqCst);
}

extern "C" fn on_trapped(sig: libc::c_int) {
    if (0..64).contains(&sig) {
        PENDING.fetch_or(1 << sig, Ordering::SeqCst);
    }
}

/// Cách xử lý 1 tín hiệu theo `trap`.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Disposition {
    /// `trap 'lệnh' SIG` — ghi nhận, chạy trap sau lệnh hiện tại.
    Trap,
    /// `trap '' SIG` — bỏ qua (tiến trình con cũng bỏ qua, như bash).
    Ignore,
    /// `trap - SIG` — như lúc khởi động.
    Default,
}

pub fn set_signal_disposition(sig: i32, d: Disposition) {
    unsafe {
        match d {
            Disposition::Ignore => {
                libc::signal(sig, libc::SIG_IGN);
            }
            Disposition::Trap => {
                // REPL: SIGINT đã có handler riêng (cũng ghi nhận trap).
                if sig == libc::SIGINT && interactive() {
                    set_handler(libc::SIGINT, on_sigint);
                } else {
                    set_handler(sig, on_trapped);
                }
            }
            Disposition::Default => {
                // Ở REPL, "mặc định" của INT/QUIT/TSTP… là chế độ bảo vệ shell.
                if interactive() && JOB_SIGNALS.contains(&sig) {
                    match sig {
                        libc::SIGINT => set_handler(libc::SIGINT, on_sigint),
                        libc::SIGQUIT => set_handler(libc::SIGQUIT, on_sigquit),
                        _ => {
                            libc::signal(sig, libc::SIG_IGN);
                        }
                    }
                } else {
                    libc::signal(sig, libc::SIG_DFL);
                }
            }
        }
    }
}

/// Lấy và xoá danh sách tín hiệu đang chờ chạy trap.
pub fn take_pending_signals() -> Vec<i32> {
    let bits = PENDING.swap(0, Ordering::SeqCst);
    (1..64).filter(|n| bits & (1 << n) != 0).collect()
}

extern "C" fn on_sigquit(_: libc::c_int) {}

/// Đặt handler (không phải SIG_IGN) để tiến trình con tự về mặc định khi exec.
unsafe fn set_handler(sig: libc::c_int, handler: extern "C" fn(libc::c_int)) {
    let mut sa: libc::sigaction = std::mem::zeroed();
    sa.sa_sigaction = handler as usize;
    sa.sa_flags = libc::SA_RESTART;
    libc::sigemptyset(&mut sa.sa_mask);
    libc::sigaction(sig, &sa, std::ptr::null_mut());
}

/// Bật job control cho REPL. No-op nếu stdin không phải terminal.
pub fn init() {
    unsafe {
        if libc::isatty(libc::STDIN_FILENO) != 1 {
            return;
        }
        let tty = libc::fcntl(libc::STDIN_FILENO, libc::F_DUPFD_CLOEXEC, 10);
        if tty < 0 {
            return;
        }
        // Bị chạy nền từ shell khác (`jaksh &`) → tự dừng tới khi được `fg`,
        // như bash; không tranh terminal với shell cha.
        loop {
            let fg = libc::tcgetpgrp(tty);
            let me = libc::getpgrp();
            if fg < 0 || fg == me {
                break;
            }
            libc::kill(-me, libc::SIGTTIN);
        }
        let original_pgid = libc::tcgetpgrp(tty);

        set_handler(libc::SIGINT, on_sigint);
        set_handler(libc::SIGQUIT, on_sigquit);
        for sig in [libc::SIGTSTP, libc::SIGTTIN, libc::SIGTTOU] {
            libc::signal(sig, libc::SIG_IGN);
        }

        // Tự làm trưởng group (session leader thì đã là sẵn — setpgid sẽ EPERM).
        if libc::getpgrp() != libc::getpid() {
            libc::setpgid(0, 0);
        }
        let shell_pgid = libc::getpgrp();
        libc::tcsetpgrp(tty, shell_pgid);

        let mut tmodes: libc::termios = std::mem::zeroed();
        libc::tcgetattr(tty, &mut tmodes);
        STATE.with(|s| {
            *s.borrow_mut() = Some(State { tty, shell_pgid, original_pgid, tmodes });
        });
    }
}

/// Job control đang bật (REPL tương tác, không phải subshell)?
pub fn active() -> bool {
    STATE.with(|s| s.borrow().is_some())
}

/// Phiên tương tác (shell chính, không phải subshell)?
pub fn interactive() -> bool {
    STATE.with(|s| s.borrow().is_some())
}

fn with_state<T>(f: impl FnOnce(&State) -> T) -> Option<T> {
    STATE.with(|s| s.borrow().as_ref().map(f))
}

/// Có Ctrl-C chưa xử lý không (không xoá cờ — để `execute` còn bỏ phần sau).
pub fn interrupted() -> bool {
    INTERRUPTED.load(Ordering::SeqCst)
}

/// Lấy và xoá cờ Ctrl-C.
pub fn take_interrupted() -> bool {
    INTERRUPTED.swap(false, Ordering::SeqCst)
}

/// Cấu hình tiến trình con trước khi spawn: vào group `pgid` (0 = group mới do
/// chính nó làm trưởng), nhận terminal nếu foreground, tín hiệu về mặc định.
/// Làm cả ở con lẫn ở cha (`after_spawn`) để không có race — như bash.
pub fn prepare(cmd: &mut Command, pgid: i32, foreground: bool) {
    let Some(tty) = with_state(|s| s.tty).filter(|_| active()) else {
        return;
    };
    unsafe {
        cmd.pre_exec(move || {
            libc::setpgid(0, pgid);
            if foreground {
                // SIGTTOU vẫn đang SIG_IGN (kế thừa từ shell) nên gọi được từ
                // group nền; reset về mặc định ngay sau.
                libc::tcsetpgrp(tty, libc::getpgrp());
            }
            for sig in JOB_SIGNALS {
                libc::signal(sig, libc::SIG_DFL);
            }
            Ok(())
        });
    }
}

/// Trong tiến trình con vừa fork (thành viên pipeline / subshell / nền): vào
/// group của job, nhận terminal nếu foreground, tín hiệu về mặc định, và tắt
/// job control (subshell không quản lý job riêng — như bash).
pub fn child_setup(pgid: i32, foreground: bool) {
    if let Some(tty) = with_state(|s| s.tty).filter(|_| active()) {
        unsafe {
            libc::setpgid(0, pgid);
            if foreground {
                libc::tcsetpgrp(tty, libc::getpgrp());
            }
            for sig in JOB_SIGNALS {
                libc::signal(sig, libc::SIG_DFL);
            }
        }
    }
    STATE.with(|s| *s.borrow_mut() = None);
}

/// Trong tiến trình con của `$(...)` / `<(...)`: ở chung group với shell; Ctrl-C
/// giết được nó (SIGINT/SIGQUIT về mặc định), Ctrl-Z vẫn bị bỏ qua.
pub fn child_setup_capture() {
    if interactive() {
        unsafe {
            libc::signal(libc::SIGINT, libc::SIG_DFL);
            libc::signal(libc::SIGQUIT, libc::SIG_DFL);
        }
    }
    STATE.with(|s| *s.borrow_mut() = None);
}

/// Phía cha sau khi spawn: trả về pgid hiệu lực của job.
pub fn after_spawn(pid: i32, pgid: i32, foreground: bool) -> i32 {
    let pgid = if pgid == 0 { pid } else { pgid };
    if active() {
        with_state(|s| unsafe {
            libc::setpgid(pid, pgid);
            if foreground {
                libc::tcsetpgrp(s.tty, pgid);
            }
        });
    }
    pgid
}

/// Lấy lại terminal cho shell. Cần khi spawn thất bại (lệnh không tồn tại):
/// tiến trình con đã kịp chạy `pre_exec` (chiếm terminal) trước khi `exec` lỗi.
pub fn reclaim_terminal() {
    if active() {
        with_state(|s| unsafe {
            libc::tcsetpgrp(s.tty, s.shell_pgid);
        });
    }
}

/// `exec` thay thế shell: chương trình mới phải nhận tín hiệu mặc định.
pub fn reset_for_exec(cmd: &mut Command) {
    unsafe {
        cmd.pre_exec(|| {
            for sig in JOB_SIGNALS {
                libc::signal(sig, libc::SIG_DFL);
            }
            Ok(())
        });
    }
}

/// Chờ job foreground tới khi xong hoặc bị Ctrl-Z. Trả về exit code kiểu bash
/// (128+N khi chết vì tín hiệu N; 148 khi bị dừng). `existing` = id job khi
/// chờ lại một job cũ (`fg`).
pub fn wait_foreground(shell: &Rc<RefCell<Shell>>, pgid: i32, pids: &[i32], cmd: &str, existing: Option<u32>) -> i32 {
    let flags = if active() { libc::WUNTRACED } else { 0 };
    let mut codes: Vec<i32> = Vec::with_capacity(pids.len());
    let mut stopped: Vec<i32> = Vec::new();
    let mut sigint = false;
    for &pid in pids {
        let code = loop {
            let mut st = 0;
            let r = unsafe { libc::waitpid(pid, &mut st, flags) };
            if r < 0 {
                if std::io::Error::last_os_error().raw_os_error() == Some(libc::EINTR) {
                    continue;
                }
                break 0;
            }
            if libc::WIFSTOPPED(st) {
                stopped.push(pid);
                break 128 + libc::WSTOPSIG(st);
            }
            if libc::WIFSIGNALED(st) {
                let sig = libc::WTERMSIG(st);
                sigint |= sig == libc::SIGINT;
                break 128 + sig;
            }
            break libc::WEXITSTATUS(st);
        };
        codes.push(code);
    }
    // Exit code pipeline = lệnh cuối; `set -o pipefail` → lệnh lỗi cuối cùng.
    let last_code = if shell.borrow().opts.pipefail {
        codes.iter().rev().find(|&&c| c != 0).copied().unwrap_or(0)
    } else {
        codes.last().copied().unwrap_or(0)
    };

    // Lấy lại terminal + chế độ sạch; nhớ chế độ của job bị dừng để `fg` trả lại.
    let job_tmodes = if active() {
        with_state(|s| unsafe {
            let mut t: libc::termios = std::mem::zeroed();
            let saved = (libc::tcgetattr(s.tty, &mut t) == 0).then_some(t);
            libc::tcsetpgrp(s.tty, s.shell_pgid);
            libc::tcsetattr(s.tty, libc::TCSADRAIN, &s.tmodes);
            saved
        })
        .flatten()
    } else {
        None
    };

    if sigint {
        INTERRUPTED.store(true, Ordering::SeqCst);
        // Job bị Ctrl-C = shell coi như nhận SIGINT → trap INT (nếu có) chạy, như bash.
        PENDING.fetch_or(1 << libc::SIGINT, Ordering::SeqCst);
        // Terminal đã echo "^C" — xuống dòng để output sau không dính vào.
        println!();
    }

    let mut sh = shell.borrow_mut();
    if !stopped.is_empty() {
        let id = match existing {
            Some(id) => id,
            None => sh.add_job(pgid, stopped.clone(), cmd.to_string()),
        };
        if let Some(job) = sh.jobs.iter_mut().find(|j| j.id == id) {
            job.state = JobState::Stopped;
            job.pids = stopped;
            job.tmodes = job_tmodes;
        }
        sh.touch_job(id);
        println!(
            "\n\x1b[33m[{}]+ đã tạm dừng: {}\x1b[0m  \x1b[2m(fg = chạy tiếp, bg = chạy nền)\x1b[0m",
            id, cmd
        );
        return 128 + libc::SIGTSTP;
    }
    if let Some(id) = existing {
        sh.jobs.retain(|j| j.id != id);
    }
    sh.set_pipestatus(&codes);
    last_code
}

/// Đưa job về foreground (`fg`): trả terminal + chế độ terminal của nó, gửi
/// SIGCONT cho cả group rồi chờ như job mới.
pub fn foreground(shell: &Rc<RefCell<Shell>>, id: u32) -> i32 {
    let Some(job) = shell.borrow().jobs.iter().find(|j| j.id == id).cloned() else {
        return 1;
    };
    println!("{}", job.cmd);
    with_state(|s| unsafe {
        if let Some(t) = job.tmodes.as_ref() {
            libc::tcsetattr(s.tty, libc::TCSADRAIN, t);
        }
        libc::tcsetpgrp(s.tty, job.pgid);
    });
    unsafe { libc::kill(-job.pgid, libc::SIGCONT) };
    if let Some(j) = shell.borrow_mut().jobs.iter_mut().find(|j| j.id == id) {
        j.state = JobState::Running;
    }
    wait_foreground(shell, job.pgid, &job.pids, &job.cmd, Some(id))
}

/// Còn job đang tạm dừng → cảnh báo một lần; `exit` lần nữa mới thoát (bash).
pub fn confirm_exit(shell: &Rc<RefCell<Shell>>) -> bool {
    if !interactive() {
        return true;
    }
    shell.borrow_mut().reap_jobs();
    let stopped = shell.borrow().jobs.iter().any(|j| j.state == JobState::Stopped);
    if stopped && !EXIT_WARNED.with(|w| w.replace(true)) {
        eprintln!("\x1b[33mjaksh: còn job đang tạm dừng (gõ `jobs` để xem). Thoát lần nữa để bỏ chúng.\x1b[0m");
        return false;
    }
    true
}

/// Dọn dẹp trước khi thoát: job đang dừng nhận SIGHUP + SIGCONT (để không bị
/// treo vĩnh viễn), trả terminal cho group ban đầu.
pub fn shutdown(shell: &Rc<RefCell<Shell>>) {
    for j in shell.borrow().jobs.iter().filter(|j| j.state == JobState::Stopped) {
        unsafe {
            libc::kill(-j.pgid, libc::SIGHUP);
            libc::kill(-j.pgid, libc::SIGCONT);
        }
    }
    with_state(|s| unsafe {
        libc::tcsetattr(s.tty, libc::TCSADRAIN, &s.tmodes);
        if s.original_pgid > 0 && s.original_pgid != s.shell_pgid {
            libc::tcsetpgrp(s.tty, s.original_pgid);
        }
    });
}

/// Cờ cảnh báo `exit` chỉ có hiệu lực cho lệnh ngay sau — lệnh khác thì reset.
pub fn reset_exit_warning() {
    EXIT_WARNED.with(|w| w.set(false));
}
