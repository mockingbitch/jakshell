use serde::Deserialize;

#[derive(Clone, Debug, Deserialize)]
#[serde(default)]
pub struct Theme {
    pub accent: String,
    pub success: String,
    pub error: String,
    pub dim: String,
    pub arrow: String,
    pub git_branch_icon: String,
    pub use_color: bool,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            accent: "cyan".into(),
            success: "green".into(),
            error: "red".into(),
            dim: "bright_black".into(),
            arrow: "❯".into(),
            git_branch_icon: " ".into(),
            use_color: true,
        }
    }
}

/// Tên các theme dựng sẵn (theo thứ tự hiển thị trong `jak theme list`).
pub const BUILTIN_NAMES: &[&str] = &[
    "default", "ocean", "forest", "sunset", "mono",
    "dracula", "nord", "monokai", "solarized", "gruvbox",
    "tokyo-night", "catppuccin", "rose-pine", "cyberpunk",
    "retro", "paper", "light",
];

/// Trả về Theme theo tên (None nếu không có).
pub fn by_name(name: &str) -> Option<Theme> {
    let mut t = Theme::default();
    match name {
        "default" => {}
        "ocean" => {
            t.accent = "bright_cyan".into();
            t.arrow = "❯".into();
        }
        "forest" => {
            t.accent = "bright_green".into();
            t.success = "bright_green".into();
            t.arrow = "→".into();
            t.git_branch_icon = "⌥ ".into();
        }
        "sunset" => {
            t.accent = "bright_magenta".into();
            t.success = "bright_yellow".into();
            t.error = "bright_red".into();
            t.arrow = "✦".into();
        }
        "mono" => {
            t.accent = "white".into();
            t.success = "white".into();
            t.error = "white".into();
            t.dim = "white".into();
            t.use_color = false;
            t.arrow = ">".into();
            t.git_branch_icon = "git:".into();
        }
        "dracula" => {
            t.accent = "magenta".into();
            t.success = "bright_green".into();
            t.error = "bright_red".into();
            t.arrow = "❯".into();
            t.git_branch_icon = " ".into();
        }
        "nord" => {
            t.accent = "blue".into();
            t.success = "cyan".into();
            t.error = "red".into();
            t.arrow = "▶".into();
            t.git_branch_icon = " ".into();
        }
        "monokai" => {
            t.accent = "bright_yellow".into();
            t.success = "bright_green".into();
            t.error = "bright_red".into();
            t.arrow = "❯".into();
        }
        "solarized" => {
            t.accent = "yellow".into();
            t.success = "green".into();
            t.error = "red".into();
            t.dim = "blue".into();
            t.arrow = "❯".into();
        }
        "gruvbox" => {
            t.accent = "yellow".into();
            t.success = "green".into();
            t.error = "bright_red".into();
            t.arrow = "➜".into();
            t.git_branch_icon = "⎇ ".into();
        }
        "tokyo-night" => {
            t.accent = "bright_blue".into();
            t.success = "bright_green".into();
            t.error = "bright_magenta".into();
            t.arrow = "❯".into();
        }
        "catppuccin" => {
            t.accent = "bright_magenta".into();
            t.success = "bright_green".into();
            t.error = "bright_red".into();
            t.dim = "bright_blue".into();
            t.arrow = "✨".into();
        }
        "rose-pine" => {
            t.accent = "magenta".into();
            t.success = "cyan".into();
            t.error = "red".into();
            t.dim = "bright_black".into();
            t.arrow = "❯".into();
        }
        "cyberpunk" => {
            t.accent = "bright_magenta".into();
            t.success = "bright_cyan".into();
            t.error = "bright_red".into();
            t.arrow = "⚡".into();
            t.git_branch_icon = "⌬ ".into();
        }
        "retro" => {
            // Green CRT — chỉ một màu xanh lá đặc trưng
            t.accent = "bright_green".into();
            t.success = "bright_green".into();
            t.error = "bright_green".into();
            t.dim = "green".into();
            t.arrow = ">".into();
            t.git_branch_icon = "[git] ".into();
        }
        "paper" => {
            // Tối giản, grayscale
            t.accent = "white".into();
            t.success = "bright_black".into();
            t.error = "bright_black".into();
            t.dim = "bright_black".into();
            t.arrow = "›".into();
            t.git_branch_icon = "@ ".into();
        }
        "light" => {
            // Cho terminal nền sáng — dùng màu đậm
            t.accent = "blue".into();
            t.success = "green".into();
            t.error = "red".into();
            t.dim = "bright_black".into();
            t.arrow = "❯".into();
        }
        _ => return None,
    }
    Some(t)
}

/// Mô tả ngắn cho mỗi theme (hiển thị trong `jak theme list`).
pub fn describe(name: &str) -> &'static str {
    match name {
        "default"     => "cyan trung tính, ổn định mọi terminal",
        "ocean"       => "bright cyan, mát mắt",
        "forest"      => "xanh lá đậm, mũi tên →",
        "sunset"      => "magenta + vàng, ✦",
        "mono"        => "không màu, ASCII (>)",
        "dracula"     => "tím classic Dracula",
        "nord"        => "xanh Nordic, ▶",
        "monokai"     => "vàng/cam Monokai",
        "solarized"   => "vàng Solarized, dim blue",
        "gruvbox"     => "vàng đất, ➜ ⎇",
        "tokyo-night" => "xanh dương đậm Tokyo Night",
        "catppuccin"  => "magenta pastel ✨",
        "rose-pine"   => "magenta + cyan",
        "cyberpunk"   => "neon ⚡, magenta/cyan",
        "retro"       => "xanh CRT cổ điển",
        "paper"       => "grayscale tối giản",
        "light"       => "cho terminal nền sáng",
        _             => "—",
    }
}

impl Theme {
    pub fn accent_ansi(&self) -> &'static str {
        if !self.use_color { return ""; }
        color_to_ansi(&self.accent)
    }
    pub fn success_ansi(&self) -> &'static str {
        if !self.use_color { return ""; }
        color_to_ansi(&self.success)
    }
    pub fn error_ansi(&self) -> &'static str {
        if !self.use_color { return ""; }
        color_to_ansi(&self.error)
    }
    pub fn dim_ansi(&self) -> &'static str {
        if !self.use_color { return ""; }
        color_to_ansi(&self.dim)
    }
}

fn color_to_ansi(name: &str) -> &'static str {
    match name.to_lowercase().as_str() {
        "black" => "\x1b[30m",
        "red" => "\x1b[31m",
        "green" => "\x1b[32m",
        "yellow" => "\x1b[33m",
        "blue" => "\x1b[34m",
        "magenta" | "purple" => "\x1b[35m",
        "cyan" => "\x1b[36m",
        "white" => "\x1b[37m",
        "bright_black" | "gray" | "grey" => "\x1b[90m",
        "bright_red" => "\x1b[91m",
        "bright_green" => "\x1b[92m",
        "bright_yellow" => "\x1b[93m",
        "bright_blue" => "\x1b[94m",
        "bright_magenta" => "\x1b[95m",
        "bright_cyan" => "\x1b[96m",
        "bright_white" => "\x1b[97m",
        _ => "\x1b[36m",
    }
}
