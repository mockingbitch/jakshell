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
    /// Bản sao vòng lịch sử của rustyline (đã nạp từ file + lệnh vừa gõ, đã
    /// bỏ trùng liên tiếp + giới hạn kích thước). Là nguồn dữ liệu cho builtin
    /// `history` — cập nhật theo thời gian thực, KHÔNG đọc lại file (file chỉ
    /// được ghi khi thoát/append) nên không còn hiện ảnh chụp cũ.
    pub history: Vec<String>,
    /// `history -c` bật cờ này; vòng lặp chính (nơi giữ rustyline) xoá lịch
    /// sử thật + file rồi tắt cờ.
    pub history_clear_requested: bool,
    pub history_cfg: HistoryConfig,
    last_status: i32,
    config_dir: PathBuf,
    pub prompt_template: String,
    pub timing: TimingConfig,
    pub greeting: GreetingConfig,
    pub ui: UiConfig,
    pub update: UpdateConfig,
    pub news: NewsConfig,
}

#[derive(Clone, Debug, serde::Deserialize)]
#[serde(default)]
pub struct UiConfig {
    /// In 1 dòng trống trước mỗi prompt (trừ prompt đầu) để các khối lệnh
    /// tách biệt nhau, dễ nhìn lại output cũ.
    pub blank_line: bool,
    /// Đường kẻ phân cách giữa các khối lệnh: "none" hoặc "line" (kẻ mờ
    /// ngang hết chiều rộng terminal).
    pub separator: String,
    /// `curl` gõ trực tiếp ra terminal: in dòng HTTP status · content-type ·
    /// thời gian + format JSON. false = curl chạy nguyên bản như bash.
    pub pretty_curl: bool,
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            blank_line: true,
            separator: "none".into(),
            pretty_curl: true,
        }
    }
}

#[derive(Clone, Debug, serde::Deserialize)]
#[serde(default)]
pub struct HistoryConfig {
    /// Số lệnh tối đa giữ trong lịch sử (bộ nhớ + file), như HISTSIZE của bash.
    pub size: usize,
}

impl Default for HistoryConfig {
    fn default() -> Self {
        Self { size: 10_000 }
    }
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

#[derive(Clone, Debug, serde::Deserialize)]
#[serde(default)]
pub struct UpdateConfig {
    /// Bật kiểm tra phiên bản mới khi khởi động (chạy NỀN — không làm chậm shell).
    pub check: bool,
    /// Tối thiểu bao nhiêu giờ giữa 2 lần gọi mạng kiểm tra (mặc định 24h).
    pub interval_hours: u64,
    /// Khi user chọn "để sau": im trong bao nhiêu giờ rồi nhắc lại (mặc định 8h).
    pub remind_hours: u64,
    /// true = hỏi tương tác (Có / Để sau / Bỏ qua / Không); false = chỉ in 1 dòng nhắc.
    pub prompt: bool,
}

impl Default for UpdateConfig {
    fn default() -> Self {
        Self {
            check: true,
            interval_hours: 24,
            remind_hours: 8,
            prompt: true,
        }
    }
}

#[derive(Clone, Debug, serde::Deserialize)]
#[serde(default)]
pub struct NewsConfig {
    /// Danh sách link RSS để crawl tin.
    pub sources: Vec<String>,
    /// Số bài tối đa mỗi lần làm mới.
    pub max_items: usize,
    /// Cache còn "tươi" trong bao nhiêu phút (`jak news` không crawl lại trong
    /// khoảng này — tránh gọi AI lặp lại tốn tiền).
    pub ttl_minutes: u64,
    /// Model Claude dùng để phân loại + tóm tắt.
    pub model: String,
    /// API key Anthropic. Để rỗng và dùng env `ANTHROPIC_API_KEY` (ưu tiên env).
    pub api_key: String,
    /// Bật phân loại + tóm tắt bằng AI. false = chỉ crawl + hiện tin thô.
    pub ai: bool,
}

impl Default for NewsConfig {
    fn default() -> Self {
        Self {
            // Vài nguồn tiếng Việt uy tín có RSS sẵn. User tự thêm/bớt trong
            // ~/.jakshrc.toml [news] sources.
            sources: vec![
                "https://vnexpress.net/rss/tin-moi-nhat.rss".into(),
                "https://tuoitre.vn/rss/tin-moi-nhat.rss".into(),
                "https://thanhnien.vn/rss/home.rss".into(),
            ],
            max_items: 20,
            ttl_minutes: 30,
            model: "claude-haiku-4-5".into(),
            api_key: String::new(),
            ai: true,
        }
    }
}

#[derive(Clone)]
pub struct Job {
    pub id: u32,
    /// Process group của job — tín hiệu (`fg`, `bg`, `kill %N`) gửi cho cả group.
    pub pgid: i32,
    /// Các tiến trình trong job chưa kết thúc (pipeline có nhiều).
    pub pids: Vec<i32>,
    pub cmd: String,
    pub state: JobState,
    /// Chế độ terminal lúc job bị Ctrl-Z — trả lại khi `fg` (vd vim ở raw mode).
    pub tmodes: Option<libc::termios>,
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
            history: Vec::new(),
            history_clear_requested: false,
            history_cfg: HistoryConfig::default(),
            last_status: 0,
            config_dir,
            prompt_template: default_prompt_template(),
            timing: TimingConfig::default(),
            greeting: GreetingConfig::default(),
            ui: UiConfig::default(),
            update: UpdateConfig::default(),
            news: NewsConfig::default(),
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

    pub fn add_job(&mut self, pgid: i32, pids: Vec<i32>, cmd: String) -> u32 {
        let id = self.jobs.iter().map(|j| j.id).max().unwrap_or(0) + 1;
        self.jobs.push(Job {
            id,
            pgid,
            pids,
            cmd,
            state: JobState::Running,
            tmodes: None,
        });
        id
    }

    /// Đưa job lên làm "job hiện tại" (`%+`) — cuối danh sách.
    pub fn touch_job(&mut self, id: u32) {
        if let Some(pos) = self.jobs.iter().position(|j| j.id == id) {
            let j = self.jobs.remove(pos);
            self.jobs.push(j);
        }
    }

    /// Tìm job theo spec kiểu bash: `%N`/`N`, `%%`/`%+`/rỗng = job hiện tại,
    /// `%-` = job trước đó, `%chuỗi` = job có lệnh bắt đầu bằng chuỗi.
    pub fn find_job(&self, spec: Option<&str>) -> Option<u32> {
        let spec = spec.unwrap_or("%%");
        let body = spec.strip_prefix('%').unwrap_or(spec);
        match body {
            "" | "%" | "+" => self.jobs.last().map(|j| j.id),
            "-" => self.jobs.iter().rev().nth(1).or(self.jobs.last()).map(|j| j.id),
            _ => match body.parse::<u32>() {
                Ok(n) => self.jobs.iter().find(|j| j.id == n).map(|j| j.id),
                Err(_) => self.jobs.iter().rev().find(|j| j.cmd.starts_with(body)).map(|j| j.id),
            },
        }
    }

    /// Thu dọn tiến trình nền đã xong / đổi trạng thái (không chặn) và in thông
    /// báo kiểu bash: `[1]+ xong: cmd`, `[1]+ đã tạm dừng: cmd`.
    pub fn reap_jobs(&mut self) {
        for j in &mut self.jobs {
            let mut changed_to = None;
            j.pids.retain(|&pid| {
                let mut st = 0;
                let r = unsafe { libc::waitpid(pid, &mut st, libc::WNOHANG | libc::WUNTRACED | libc::WCONTINUED) };
                if r == 0 {
                    return true;
                }
                if r < 0 {
                    // ECHILD: đã được thu ở nơi khác → coi như xong.
                    return false;
                }
                if libc::WIFSTOPPED(st) {
                    changed_to = Some(JobState::Stopped);
                    return true;
                }
                if libc::WIFCONTINUED(st) {
                    changed_to = Some(JobState::Running);
                    return true;
                }
                false
            });
            if j.pids.is_empty() {
                j.state = JobState::Done;
                println!("\x1b[2m[{}]+ xong: {}\x1b[0m", j.id, j.cmd);
            } else if let Some(state) = changed_to {
                if state != j.state && state == JobState::Stopped {
                    println!("\x1b[33m[{}]+ đã tạm dừng: {}\x1b[0m", j.id, j.cmd);
                }
                j.state = state;
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
