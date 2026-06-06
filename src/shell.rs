use anyhow::Result;
use std::collections::HashMap;
use std::path::PathBuf;

use crate::theme::Theme;

pub struct Shell {
    pub env: HashMap<String, String>,
    pub aliases: HashMap<String, String>,
    pub cwd: PathBuf,
    pub theme: Theme,
    pub jobs: Vec<Job>,
    last_status: i32,
    config_dir: PathBuf,
    pub prompt_template: String,
    pub timing: TimingConfig,
    pub greeting: GreetingConfig,
}

#[derive(Clone, Debug, serde::Deserialize)]
#[serde(default)]
pub struct GreetingConfig {
    /// Bật/tắt toàn bộ banner khi khởi động.
    pub enabled: bool,
    /// Có in dòng "Chào buổi …, <user>! Hôm nay là …" không.
    pub show_greeting: bool,
    /// Có in mẹo ngẫu nhiên không.
    pub show_tip: bool,
    /// Tên hiển thị thay cho $USER (vd "boss", "Jarvis"). Rỗng = lấy $USER.
    pub name: String,
}

impl Default for GreetingConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            show_greeting: true,
            show_tip: true,
            name: String::new(),
        }
    }
}

#[derive(Clone, Debug, serde::Deserialize)]
#[serde(default)]
pub struct TimingConfig {
    /// Bật/tắt hiển thị thời gian sau mỗi lệnh.
    pub enabled: bool,
    /// Chỉ hiện nếu thời gian thực thi >= ngưỡng này (ms). 0 = luôn hiện.
    pub threshold_ms: u64,
    /// Nếu true: kèm exit code khi khác 0.
    pub show_status: bool,
    /// Nếu true: kèm gợi ý lý do khi lệnh fail (exit != 0).
    pub show_hint: bool,
}

impl Default for TimingConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            threshold_ms: 0,
            show_status: true,
            show_hint: true,
        }
    }
}

#[derive(Clone)]
pub struct Job {
    pub id: u32,
    pub pid: i32,
    pub cmd: String,
    pub state: JobState,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum JobState {
    Running,
    Stopped,
    Done,
}

impl Shell {
    pub fn new() -> Result<Self> {
        // Bật màu mặc định cho `ls`: env var + (lát nữa) alias mặc định.
        setup_ls_colors();

        let env: HashMap<String, String> = std::env::vars().collect();
        let cwd = std::env::current_dir()?;
        let config_dir = dirs::home_dir()
            .map(|h| h.join(".config").join("jaksh"))
            .unwrap_or_else(|| PathBuf::from(".jaksh"));
        std::fs::create_dir_all(&config_dir).ok();

        Ok(Self {
            env,
            aliases: default_aliases(),
            cwd,
            theme: Theme::default(),
            jobs: Vec::new(),
            last_status: 0,
            config_dir,
            prompt_template: default_prompt_template(),
            timing: TimingConfig::default(),
            greeting: GreetingConfig::default(),
        })
    }

    pub fn last_status(&self) -> i32 {
        self.last_status
    }

    pub fn set_last_status(&mut self, code: i32) {
        self.last_status = code;
        self.env.insert("?".into(), code.to_string());
    }

    pub fn history_path(&self) -> PathBuf {
        self.config_dir.join("history")
    }

    pub fn rc_toml_path(&self) -> PathBuf {
        dirs::home_dir()
            .map(|h| h.join(".jakshrc.toml"))
            .unwrap_or_else(|| PathBuf::from(".jakshrc.toml"))
    }

    pub fn rc_script_path(&self) -> PathBuf {
        dirs::home_dir()
            .map(|h| h.join(".jakshrc"))
            .unwrap_or_else(|| PathBuf::from(".jakshrc"))
    }

    pub fn get_var(&self, name: &str) -> Option<String> {
        if name == "?" {
            return Some(self.last_status.to_string());
        }
        if name == "PWD" {
            return Some(self.cwd.display().to_string());
        }
        self.env.get(name).cloned()
    }

    pub fn set_var(&mut self, name: &str, value: &str) {
        self.env.insert(name.into(), value.into());
        // Also export to process env so child processes see it
        std::env::set_var(name, value);
    }

    pub fn unset_var(&mut self, name: &str) {
        self.env.remove(name);
        std::env::remove_var(name);
    }

    pub fn add_job(&mut self, pid: i32, cmd: String) -> u32 {
        let id = self.jobs.iter().map(|j| j.id).max().unwrap_or(0) + 1;
        self.jobs.push(Job {
            id,
            pid,
            cmd,
            state: JobState::Running,
        });
        id
    }

    pub fn reap_jobs(&mut self) {
        use nix::sys::wait::{waitpid, WaitPidFlag, WaitStatus};
        use nix::unistd::Pid;
        let mut done_ids = Vec::new();
        for j in &mut self.jobs {
            if j.state != JobState::Running {
                continue;
            }
            match waitpid(Pid::from_raw(j.pid), Some(WaitPidFlag::WNOHANG)) {
                Ok(WaitStatus::Exited(_, _)) | Ok(WaitStatus::Signaled(_, _, _)) => {
                    j.state = JobState::Done;
                    done_ids.push(j.id);
                }
                Ok(WaitStatus::Stopped(_, _)) => j.state = JobState::Stopped,
                _ => {}
            }
        }
        for id in done_ids {
            if let Some(j) = self.jobs.iter().find(|j| j.id == id) {
                println!("\x1b[2m[{}] xong: {}\x1b[0m", j.id, j.cmd);
            }
        }
        self.jobs.retain(|j| j.state != JobState::Done);
    }
}

/// Đặt env var để hệ thống `ls` tự tô màu. Chỉ set nếu user CHƯA đặt.
fn setup_ls_colors() {
    let set_if_unset = |k: &str, v: &str| {
        if std::env::var_os(k).is_none() {
            std::env::set_var(k, v);
        }
    };
    // BSD ls (macOS): CLICOLOR=1 bật màu, LSCOLORS định nghĩa palette.
    // 11 cặp ký tự (fg+bg): dir/sym/socket/pipe/exec/block/char/setuid/setgid/sticky-other-write/other-write
    // Ex = exec (bold) blue, Fx = bold magenta, ...
    set_if_unset("CLICOLOR", "1");
    set_if_unset("LSCOLORS", "ExFxBxDxCxegedabagacad");

    // GNU ls (Linux): LS_COLORS định nghĩa palette.
    // di=directory, ln=symlink, so=socket, pi=pipe, ex=executable, ...
    set_if_unset(
        "LS_COLORS",
        "di=1;34:ln=1;36:so=1;35:pi=1;33:ex=1;32:bd=33;1:cd=33;1:su=1;31:sg=1;31:tw=1;34:ow=1;34",
    );
}

/// Bí danh mặc định để `ls` tô màu + thêm `/` sau tên thư mục.
/// User có thể override qua `[aliases]` trong ~/.jakshrc.toml.
fn default_aliases() -> HashMap<String, String> {
    let mut m = HashMap::new();
    // `-p` = thêm `/` cuối tên thư mục (BSD + GNU).
    // macOS BSD: `-G` bật màu. GNU Linux: `--color=auto`.
    #[cfg(target_os = "macos")]
    {
        m.insert("ls".into(), "ls -Gp".into());
    }
    #[cfg(not(target_os = "macos"))]
    {
        m.insert("ls".into(), "ls --color=auto -p".into());
    }
    m
}

fn default_prompt_template() -> String {
    // {accent}{user}@{host} {path}{git} {status}{reset}
    "{accent}{cwd_short}{reset}{git} {arrow} ".to_string()
}
