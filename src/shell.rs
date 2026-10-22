use anyhow::{anyhow, Result};
use std::cell::Cell;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Instant;

use crate::theme::Theme;

pub struct Shell {
    /// Mọi biến shell (kể cả biến chưa export). Chỉ tên trong `exported` mới
    /// được truyền sang tiến trình con — như bash.
    pub env: HashMap<String, String>,
    pub exported: HashSet<String>,
    pub readonly: HashSet<String>,
    /// Biến khai báo `declare -i` — mọi phép gán được tính số học.
    pub integers: HashSet<String>,
    /// `$0` và tham số vị trí `$1..` (`set --`, `shift`, đối số script).
    pub arg0: String,
    pub positional: Vec<String>,
    /// `$!` — pid lệnh nền gần nhất.
    pub last_bg_pid: Option<i32>,
    /// Mảng chỉ số (`a=(x y)`, `a[3]=z`) — thưa, theo thứ tự chỉ số.
    pub arrays: HashMap<String, BTreeMap<i64, String>>,
    /// Mảng kết hợp (`declare -A m; m[k]=v`).
    pub assocs: HashMap<String, BTreeMap<String, String>>,
    pub functions: HashMap<String, Rc<crate::parser::Command>>,
    /// `trap`: tên (EXIT, ERR, INT, TERM…) → lệnh. Chuỗi rỗng = bỏ qua tín hiệu.
    pub traps: BTreeMap<String, String>,
    /// Đang chạy 1 trap (không kích hoạt ERR lồng nhau).
    pub in_trap: bool,
    /// Ngăn xếp `pushd` / `popd` (không gồm thư mục hiện tại).
    pub dirstack: Vec<PathBuf>,
    /// `break` / `continue` / `return` đang chờ xử lý.
    pub flow: Flow,
    pub loop_depth: u32,
    /// Ngăn xếp `local`: mỗi lần gọi function 1 tầng, lưu giá trị cũ để trả lại.
    pub local_frames: Vec<Vec<(String, SavedVar)>>,
    /// `${FUNCNAME[@]}`.
    pub funcnames: Vec<String>,
    /// > 0 khi đang chạy điều kiện `if` / `while` / `&&`… — `set -e` không áp.
    pub errexit_suppressed: u32,
    /// Exit code của `$(...)` gần nhất (cho `x=$(false)` → `$?` = 1).
    pub last_cmdsub_status: Option<i32>,
    /// `$$` — pid shell chính (subshell vẫn thấy giá trị này, như bash).
    pub shell_pid: u32,
    pub opts: ShellOpts,
    pub shopts: Shopts,
    started: Instant,
    rng: Cell<u64>,
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
    /// `history --scrub`: xoá khỏi lịch sử (cả file) các lệnh chứa bí mật.
    pub history_scrub_requested: bool,
    /// `history -d N`: vị trí (0-based) cần xoá.
    pub history_delete_request: Option<usize>,
    /// Lệnh do `fc` chạy — thêm vào lịch sử thay cho chính dòng `fc`.
    pub history_add_request: Vec<String>,
    /// `set -o vi` / `set -o emacs`: REPL đổi chế độ soạn dòng.
    pub edit_mode_request: Option<String>,
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

/// Điều khiển luồng chờ xử lý sau `break` / `continue` / `return`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Flow {
    #[default]
    Normal,
    Break(u32),
    Continue(u32),
    Return,
}

/// Giá trị cũ của biến trước `local` — trả lại khi function kết thúc.
#[derive(Clone, Debug)]
pub enum SavedVar {
    Unset,
    Scalar(String, bool),
    Indexed(BTreeMap<i64, String>),
    Assoc(BTreeMap<String, String>),
}

/// Chỉ số mảng đã tính.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Index {
    Num(i64),
    Key(String),
}

/// Tuỳ chọn `shopt`.
#[derive(Clone, Debug)]
pub struct Shopts {
    pub nullglob: bool,
    pub dotglob: bool,
    pub globstar: bool,
    pub extglob: bool,
    pub nocaseglob: bool,
    pub nocasematch: bool,
    pub failglob: bool,
    pub expand_aliases: bool,
    pub lastpipe: bool,
}

impl Default for Shopts {
    fn default() -> Self {
        Self {
            nullglob: false,
            dotglob: false,
            globstar: false,
            extglob: true,
            nocaseglob: false,
            nocasematch: false,
            failglob: false,
            expand_aliases: true,
            lastpipe: false,
        }
    }
}

impl Shopts {
    pub const NAMES: [&'static str; 9] =
        ["nullglob", "dotglob", "globstar", "extglob", "nocaseglob", "nocasematch", "failglob", "expand_aliases", "lastpipe"];

    pub fn slot(&mut self, name: &str) -> Option<&mut bool> {
        Some(match name {
            "nullglob" => &mut self.nullglob,
            "dotglob" => &mut self.dotglob,
            "globstar" => &mut self.globstar,
            "extglob" => &mut self.extglob,
            "nocaseglob" => &mut self.nocaseglob,
            "nocasematch" => &mut self.nocasematch,
            "failglob" => &mut self.failglob,
            "expand_aliases" => &mut self.expand_aliases,
            "lastpipe" => &mut self.lastpipe,
            _ => return None,
        })
    }
}

/// Tuỳ chọn `set` của bash.
#[derive(Clone, Debug, Default)]
pub struct ShellOpts {
    /// `-e`: thoát khi lệnh lỗi (trừ vế trái của `&&` / `||` và `!`).
    pub errexit: bool,
    /// `-u`: dùng biến chưa gán là lỗi.
    pub nounset: bool,
    /// `-x`: in từng lệnh (đã expand) ra stderr trước khi chạy.
    pub xtrace: bool,
    /// `-o pipefail`: exit code pipeline = lệnh lỗi cuối cùng thay vì lệnh cuối.
    pub pipefail: bool,
    /// `-a`: mọi phép gán đều tự export.
    pub allexport: bool,
    /// `-C` / `-o noclobber`: `>` không ghi đè file đã có.
    pub noclobber: bool,
    /// `-f` / `-o noglob`: tắt glob.
    pub noglob: bool,
}

impl ShellOpts {
    /// Cặp (tên -o, chữ cái) — dùng cho `set -o`, `set -x`, `$-`.
    pub const NAMES: [(&'static str, char); 7] = [
        ("errexit", 'e'),
        ("nounset", 'u'),
        ("xtrace", 'x'),
        ("pipefail", '\0'),
        ("allexport", 'a'),
        ("noclobber", 'C'),
        ("noglob", 'f'),
    ];

    pub fn get(&self, name: &str) -> Option<bool> {
        Some(match name {
            "errexit" => self.errexit,
            "nounset" => self.nounset,
            "xtrace" => self.xtrace,
            "pipefail" => self.pipefail,
            "allexport" => self.allexport,
            "noclobber" => self.noclobber,
            "noglob" => self.noglob,
            _ => return None,
        })
    }

    pub fn set(&mut self, name: &str, on: bool) -> bool {
        let slot = match name {
            "errexit" => &mut self.errexit,
            "nounset" => &mut self.nounset,
            "xtrace" => &mut self.xtrace,
            "pipefail" => &mut self.pipefail,
            "allexport" => &mut self.allexport,
            "noclobber" => &mut self.noclobber,
            "noglob" => &mut self.noglob,
            _ => return false,
        };
        *slot = on;
        true
    }

    pub fn name_of_flag(c: char) -> Option<&'static str> {
        Self::NAMES.iter().find(|(_, f)| *f == c && c != '\0').map(|(n, _)| *n)
    }
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
    /// Chế độ soạn dòng: "emacs" (mặc định) hoặc "vi".
    pub edit_mode: String,
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            blank_line: true,
            separator: "none".into(),
            pretty_curl: true,
            edit_mode: "emacs".into(),
        }
    }
}

#[derive(Clone, Debug, serde::Deserialize)]
#[serde(default)]
pub struct HistoryConfig {
    /// Số lệnh tối đa giữ trong lịch sử (bộ nhớ + file), như HISTSIZE của bash.
    pub size: usize,
    /// Không lưu lệnh chứa token / mật khẩu vào lịch sử (cảnh báo 1 dòng).
    pub secret_filter: bool,
}

impl Default for HistoryConfig {
    fn default() -> Self {
        Self { size: 10_000, secret_filter: true }
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

        let mut env: HashMap<String, String> = std::env::vars().collect();
        // Mọi biến kế thừa từ môi trường đều đã được export.
        let mut exported: HashSet<String> = env.keys().cloned().collect();
        // Biến shell bash luôn có sẵn (script dựa vào chúng, nhất là khi `set -u`).
        let uid = unsafe { libc::getuid() };
        let euid = unsafe { libc::geteuid() };
        let defaults = [
            ("IFS", " \t\n".to_string()),
            ("OPTIND", "1".to_string()),
            ("OPTERR", "1".to_string()),
            ("PPID", unsafe { libc::getppid() }.to_string()),
            ("UID", uid.to_string()),
            ("EUID", euid.to_string()),
            ("OSTYPE", if cfg!(target_os = "macos") { "darwin".into() } else { "linux-gnu".into() }),
            ("HOSTTYPE", std::env::consts::ARCH.to_string()),
            ("JAKSH_VERSION", env!("JAKSH_VERSION").to_string()),
        ];
        for (k, v) in defaults {
            env.entry(k.to_string()).or_insert(v);
        }
        if !env.contains_key("HOSTNAME") {
            let mut buf = [0u8; 256];
            if unsafe { libc::gethostname(buf.as_mut_ptr() as *mut libc::c_char, buf.len()) } == 0 {
                let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
                env.insert("HOSTNAME".into(), String::from_utf8_lossy(&buf[..end]).into_owned());
            }
        }
        // SHLVL tăng 1 mỗi tầng shell lồng nhau (export như bash).
        let shlvl = env.get("SHLVL").and_then(|v| v.parse::<i64>().ok()).unwrap_or(0) + 1;
        env.insert("SHLVL".into(), shlvl.to_string());
        exported.insert("SHLVL".into());
        std::env::set_var("SHLVL", shlvl.to_string());
        let cwd = std::env::current_dir()?;
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(1)
            ^ ((std::process::id() as u64) << 32);
        let config_dir = dirs::home_dir()
            .map(|h| h.join(".config").join("jaksh"))
            .unwrap_or_else(|| PathBuf::from(".jaksh"));
        std::fs::create_dir_all(&config_dir).ok();

        Ok(Self {
            env,
            exported,
            readonly: HashSet::new(),
            integers: HashSet::new(),
            arg0: std::env::args().next().unwrap_or_else(|| "jaksh".into()),
            positional: Vec::new(),
            last_bg_pid: None,
            arrays: HashMap::new(),
            assocs: HashMap::new(),
            functions: HashMap::new(),
            traps: BTreeMap::new(),
            in_trap: false,
            dirstack: Vec::new(),
            flow: Flow::Normal,
            loop_depth: 0,
            local_frames: Vec::new(),
            funcnames: Vec::new(),
            errexit_suppressed: 0,
            last_cmdsub_status: None,
            shell_pid: std::process::id(),
            opts: ShellOpts::default(),
            shopts: Shopts::default(),
            started: Instant::now(),
            rng: Cell::new(seed | 1),
            aliases: default_aliases(),
            cwd,
            theme: Theme::default(),
            jobs: Vec::new(),
            history: Vec::new(),
            history_clear_requested: false,
            history_scrub_requested: false,
            history_delete_request: None,
            history_add_request: Vec::new(),
            edit_mode_request: None,
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
        match name {
            "?" => return Some(self.last_status.to_string()),
            "PWD" => return Some(self.cwd.display().to_string()),
            "0" => return Some(self.arg0.clone()),
            "#" => return Some(self.positional.len().to_string()),
            "@" | "*" => return Some(self.positional.join(" ")),
            "$" => return Some(self.shell_pid.to_string()),
            "BASHPID" => return Some(std::process::id().to_string()),
            "!" => return self.last_bg_pid.map(|p| p.to_string()),
            "-" => return Some(self.flags_string()),
            "RANDOM" => {
                // xorshift64 — đủ cho $RANDOM (0..32767), không cần crate.
                let mut x = self.rng.get();
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                self.rng.set(x);
                return Some((x % 32768).to_string());
            }
            "SECONDS" => return Some(self.started.elapsed().as_secs().to_string()),
            "LINENO" => return Some(self.env.get("LINENO").cloned().unwrap_or_else(|| "1".into())),
            "FUNCNAME" => return self.funcnames.last().cloned(),
            _ => {}
        }
        if let Ok(n) = name.parse::<usize>() {
            return self.positional.get(n.wrapping_sub(1)).cloned().filter(|_| n > 0);
        }
        if let Some(v) = self.env.get(name) {
            return Some(v.clone());
        }
        // Tên mảng không kèm chỉ số = phần tử 0.
        if let Some(a) = self.arrays.get(name) {
            return a.get(&0).cloned();
        }
        self.assocs.get(name).and_then(|m| m.get("0").cloned())
    }

    /// Có phải mảng kết hợp không.
    pub fn is_assoc(&self, name: &str) -> bool {
        self.assocs.contains_key(name)
    }

    pub fn is_array(&self, name: &str) -> bool {
        self.arrays.contains_key(name) || self.assocs.contains_key(name) || name == "FUNCNAME"
    }

    /// `${a[i]}`.
    pub fn get_index(&self, name: &str, idx: &Index) -> Option<String> {
        match idx {
            Index::Key(k) => self.assocs.get(name).and_then(|m| m.get(k).cloned()),
            Index::Num(i) => {
                if name == "FUNCNAME" {
                    let n = self.funcnames.len() as i64;
                    let pos = if *i < 0 { n + i } else { *i };
                    return usize::try_from(n - 1 - pos).ok().and_then(|p| self.funcnames.get(p)).cloned();
                }
                if let Some(a) = self.arrays.get(name) {
                    let i = if *i < 0 { a.keys().next_back().map_or(0, |m| m + 1) + i } else { *i };
                    return a.get(&i).cloned();
                }
                if let Some(m) = self.assocs.get(name) {
                    return m.get(&i.to_string()).cloned();
                }
                // Biến thường như mảng 1 phần tử.
                (*i == 0).then(|| self.env.get(name).cloned()).flatten()
            }
        }
    }

    /// `${a[@]}` — giá trị theo thứ tự chỉ số; biến thường → 1 phần tử.
    pub fn array_values(&self, name: &str) -> Vec<String> {
        if name == "FUNCNAME" {
            return self.funcnames.iter().rev().cloned().collect();
        }
        if let Some(a) = self.arrays.get(name) {
            return a.values().cloned().collect();
        }
        if let Some(m) = self.assocs.get(name) {
            return m.values().cloned().collect();
        }
        self.get_var(name).into_iter().collect()
    }

    /// `${!a[@]}` — các chỉ số / khoá.
    pub fn array_keys(&self, name: &str) -> Vec<String> {
        if let Some(a) = self.arrays.get(name) {
            return a.keys().map(|k| k.to_string()).collect();
        }
        if let Some(m) = self.assocs.get(name) {
            return m.keys().cloned().collect();
        }
        if self.get_var(name).is_some() {
            return vec!["0".into()];
        }
        Vec::new()
    }

    /// `a[i]=v`. Biến thường được chuyển thành mảng (giá trị cũ ở chỉ số 0).
    pub fn set_index(&mut self, name: &str, idx: Index, value: &str) -> Result<()> {
        if self.readonly.contains(name) {
            return Err(anyhow!("{}: biến chỉ đọc (readonly)", name));
        }
        if let Some(m) = self.assocs.get_mut(name) {
            let key = match idx {
                Index::Key(k) => k,
                Index::Num(n) => n.to_string(),
            };
            m.insert(key, value.to_string());
            return Ok(());
        }
        let Index::Num(mut i) = idx else {
            return Err(anyhow!("{}: không phải mảng kết hợp (dùng `declare -A {}`)", name, name));
        };
        if !self.arrays.contains_key(name) {
            let mut a = BTreeMap::new();
            if let Some(old) = self.env.remove(name) {
                a.insert(0, old);
            }
            if self.exported.remove(name) {
                std::env::remove_var(name);
            }
            self.arrays.insert(name.to_string(), a);
        }
        let a = self.arrays.get_mut(name).expect("vừa tạo");
        if i < 0 {
            i += a.keys().next_back().map_or(0, |m| m + 1);
            if i < 0 {
                return Err(anyhow!("{}: chỉ số mảng âm không hợp lệ", name));
            }
        }
        a.insert(i, value.to_string());
        Ok(())
    }

    /// `a=(x y z)` — thay toàn bộ mảng chỉ số.
    pub fn set_array(&mut self, name: &str, values: BTreeMap<i64, String>) -> Result<()> {
        if self.readonly.contains(name) {
            return Err(anyhow!("{}: biến chỉ đọc (readonly)", name));
        }
        self.env.remove(name);
        if self.exported.remove(name) {
            std::env::remove_var(name);
        }
        self.assocs.remove(name);
        self.arrays.insert(name.to_string(), values);
        Ok(())
    }

    /// `declare -A m=([k]=v …)` / `m=([k]=v)` khi m đã là mảng kết hợp.
    pub fn set_assoc(&mut self, name: &str, values: BTreeMap<String, String>) -> Result<()> {
        if self.readonly.contains(name) {
            return Err(anyhow!("{}: biến chỉ đọc (readonly)", name));
        }
        self.env.remove(name);
        self.arrays.remove(name);
        self.assocs.insert(name.to_string(), values);
        Ok(())
    }

    /// `PIPESTATUS` sau mỗi pipeline.
    pub fn set_pipestatus(&mut self, codes: &[i32]) {
        let a = codes.iter().enumerate().map(|(i, c)| (i as i64, c.to_string())).collect();
        self.arrays.insert("PIPESTATUS".into(), a);
    }

    /// `$-` — chữ cái của các tuỳ chọn đang bật (+ `i` khi tương tác).
    pub fn flags_string(&self) -> String {
        let mut s: String = ShellOpts::NAMES
            .iter()
            .filter(|(n, c)| *c != '\0' && self.opts.get(n) == Some(true))
            .map(|(_, c)| *c)
            .collect();
        if crate::jobctl::interactive() {
            s.push('i');
        }
        s
    }

    /// Gán biến. Biến đã export (hoặc đang `set -a`) được đồng bộ sang env của
    /// tiến trình để lệnh con thấy; biến thường thì chỉ shell thấy. Gán vào tên
    /// mảng = gán phần tử 0 (như bash).
    pub fn set_var(&mut self, name: &str, value: &str) -> Result<()> {
        if self.readonly.contains(name) {
            return Err(anyhow!("{}: biến chỉ đọc (readonly)", name));
        }
        if self.arrays.contains_key(name) {
            return self.set_index(name, Index::Num(0), value);
        }
        if self.assocs.contains_key(name) {
            return self.set_index(name, Index::Key("0".into()), value);
        }
        self.env.insert(name.into(), value.into());
        if self.opts.allexport {
            self.exported.insert(name.into());
        }
        if self.exported.contains(name) {
            std::env::set_var(name, value);
        }
        Ok(())
    }

    /// `export NAME[=value]`.
    pub fn export_var(&mut self, name: &str, value: Option<&str>) -> Result<()> {
        if let Some(v) = value {
            if self.readonly.contains(name) {
                return Err(anyhow!("{}: biến chỉ đọc (readonly)", name));
            }
            self.env.insert(name.into(), v.into());
        }
        self.exported.insert(name.into());
        if let Some(v) = self.env.get(name) {
            std::env::set_var(name, v);
        }
        Ok(())
    }

    /// `export -n NAME` — vẫn là biến shell nhưng lệnh con không thấy nữa.
    pub fn unexport_var(&mut self, name: &str) {
        self.exported.remove(name);
        std::env::remove_var(name);
    }

    pub fn unset_var(&mut self, name: &str) -> Result<()> {
        if self.readonly.contains(name) {
            return Err(anyhow!("{}: không unset được biến chỉ đọc", name));
        }
        self.integers.remove(name);
        self.env.remove(name);
        self.arrays.remove(name);
        self.assocs.remove(name);
        self.exported.remove(name);
        std::env::remove_var(name);
        Ok(())
    }

    /// `unset 'a[i]'`.
    pub fn unset_index(&mut self, name: &str, idx: &Index) -> Result<()> {
        if self.readonly.contains(name) {
            return Err(anyhow!("{}: không unset được biến chỉ đọc", name));
        }
        match idx {
            Index::Num(i) => {
                if let Some(a) = self.arrays.get_mut(name) {
                    a.remove(i);
                } else if let Some(m) = self.assocs.get_mut(name) {
                    m.remove(&i.to_string());
                } else if *i == 0 {
                    self.env.remove(name);
                }
            }
            Index::Key(k) => {
                if let Some(m) = self.assocs.get_mut(name) {
                    m.remove(k);
                }
            }
        }
        Ok(())
    }

    /// `local NAME` trong function: lưu trạng thái cũ (1 lần mỗi tầng) để trả lại
    /// khi function kết thúc. Ngoài function → lỗi như bash.
    pub fn make_local(&mut self, name: &str) -> Result<()> {
        let saved = if let Some(a) = self.arrays.get(name) {
            SavedVar::Indexed(a.clone())
        } else if let Some(m) = self.assocs.get(name) {
            SavedVar::Assoc(m.clone())
        } else if let Some(v) = self.env.get(name) {
            SavedVar::Scalar(v.clone(), self.exported.contains(name))
        } else {
            SavedVar::Unset
        };
        let Some(frame) = self.local_frames.last_mut() else {
            return Err(anyhow!("local: chỉ dùng được bên trong function"));
        };
        if frame.iter().any(|(n, _)| n == name) {
            return Ok(());
        }
        frame.push((name.to_string(), saved));
        Ok(())
    }

    /// Trả lại các biến `local` của tầng function vừa kết thúc.
    pub fn pop_locals(&mut self) {
        let Some(frame) = self.local_frames.pop() else { return };
        for (name, saved) in frame.into_iter().rev() {
            self.env.remove(&name);
            self.arrays.remove(&name);
            self.assocs.remove(&name);
            match saved {
                SavedVar::Unset => {
                    if self.exported.remove(&name) {
                        std::env::remove_var(&name);
                    }
                }
                SavedVar::Scalar(v, exported) => {
                    if exported {
                        self.exported.insert(name.clone());
                        std::env::set_var(&name, &v);
                    } else if self.exported.remove(&name) {
                        std::env::remove_var(&name);
                    }
                    self.env.insert(name, v);
                }
                SavedVar::Indexed(a) => {
                    self.arrays.insert(name, a);
                }
                SavedVar::Assoc(m) => {
                    self.assocs.insert(name, m);
                }
            }
        }
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

/// Biểu thức số học đọc / ghi biến của shell (kể cả phần tử mảng).
impl crate::arith::Vars for Shell {
    fn get(&self, name: &str, index: Option<i64>) -> Option<String> {
        match index {
            Some(i) => self.get_index(name, &Index::Num(i)),
            None => self.get_var(name),
        }
    }

    fn set(&mut self, name: &str, index: Option<i64>, value: i64) -> Result<()> {
        match index {
            Some(i) => self.set_index(name, Index::Num(i), &value.to_string()),
            None => self.set_var(name, &value.to_string()),
        }
    }
}
