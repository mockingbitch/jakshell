use serde::Deserialize;

#[derive(Clone, Debug, Deserialize)]
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

impl Theme {
    pub fn accent_ansi(&self) -> &'static str {
        if !self.use_color {
            return "";
        }
        color_to_ansi(&self.accent)
    }
    pub fn success_ansi(&self) -> &'static str {
        if !self.use_color {
            return "";
        }
        color_to_ansi(&self.success)
    }
    pub fn error_ansi(&self) -> &'static str {
        if !self.use_color {
            return "";
        }
        color_to_ansi(&self.error)
    }
    pub fn dim_ansi(&self) -> &'static str {
        if !self.use_color {
            return "";
        }
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
