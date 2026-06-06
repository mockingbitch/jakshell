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
        "{cyan}{bar}{reset} {bold}{bc}JakShell{reset} {dim}{ver}{reset}  {dim}—{reset}  {tagline}",
        bc = bright_cyan,
        ver = env!("JAKSH_VERSION"),
        tagline = crate::i18n::t("banner.tagline"),
    );
    println!(
        "{cyan}{bar}{reset} {tagline2}",
        tagline2 = crate::i18n::t("banner.tagline2").replace("lowtech-friendly", &format!("{}lowtech-friendly{}", yellow, reset)),
    );
    println!(
        "{cyan}{bar}{reset} {bold}explain{reset}  ·  {bold}--jak{reset}  ·  {bold}bookmark{reset}  ·  {bold}jak utils{reset}  ·  {bold}smart git prompt{reset}",
    );
    println!(
        "{cyan}{bar}{reset} {dim}{dev}{reset} {bold}Jarvis Phong Tran{reset}  {dim}·  https://github.com/mockingbitch/jakshell{reset}",
        dev = crate::i18n::t("banner.developed_by"),
    );
    println!();
}
