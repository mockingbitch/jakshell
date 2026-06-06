//! Banner thông tin JakShell — in ở đầu các lệnh trợ giúp (`?`, `jak`, `explain`).

/// In banner gồm: tên + version + tagline + URL repo + danh sách tính năng chính.
pub fn print_banner() {
    let bold = "\x1b[1m";
    let cyan = "\x1b[36m";
    let bright_cyan = "\x1b[96m";
    let yellow = "\x1b[33m";
    let dim = "\x1b[2m";
    let reset = "\x1b[0m";
    let bar = "┃";

    println!(
        "{cyan}{bar}{reset} {bold}{bc}JakShell{reset} {dim}{ver}{reset}  {dim}—{reset}  Shell Rust cho macOS & Linux",
        bc = bright_cyan,
        ver = env!("JAKSH_VERSION")
    );
    println!(
        "{cyan}{bar}{reset} Nhanh, gọn, thân thiện cho người Việt — {y}lowtech-friendly{reset}",
        y = yellow
    );
    println!(
        "{cyan}{bar}{reset} {bold}explain{reset}  ·  {bold}--jak{reset}  ·  {bold}bookmark{reset}  ·  {bold}jak utils{reset}  ·  {bold}smart git prompt{reset}",
    );
    println!(
        "{cyan}{bar}{reset} {dim}Developed by{reset} {bold}Jarvis Phong Tran{reset}  {dim}·  https://github.com/mockingbitch/jakshell{reset}",
    );
    println!();
}
