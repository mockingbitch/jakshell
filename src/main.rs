mod builtins;
mod completion;
mod config;
mod executor;
mod expand;
mod explain;
mod findcmd;
mod jak;
mod lexer;
mod parser;
mod pretty;
mod prompt;
mod shell;
mod suggest;
mod theme;

use anyhow::Result;
use rustyline::error::ReadlineError;
use rustyline::Editor;
use std::cell::RefCell;
use std::rc::Rc;

use crate::completion::ShellHelper;
use crate::shell::Shell;

fn main() -> Result<()> {
    let shell = Rc::new(RefCell::new(Shell::new()?));

    if let Err(e) = config::load(&shell) {
        eprintln!("\x1b[33mjaksh: lỗi khi nạp cấu hình: {e}\x1b[0m");
    }

    let helper = ShellHelper::new(Rc::clone(&shell));
    let mut rl: Editor<ShellHelper, rustyline::history::FileHistory> = Editor::new()?;
    rl.set_helper(Some(helper));

    let history_path = shell.borrow().history_path();
    let _ = rl.load_history(&history_path);

    print_welcome(&shell);

    loop {
        let prompt_str = prompt::render(&shell.borrow());
        match rl.readline(&prompt_str) {
            Ok(line) => {
                let line = line.trim_end_matches('\n').to_string();
                if line.trim().is_empty() {
                    continue;
                }
                rl.add_history_entry(line.as_str()).ok();

                match run_line(&shell, &line) {
                    Ok(code) => {
                        if code == 127 {
                            suggest::maybe_suggest(&shell.borrow(), &line);
                        }
                        shell.borrow_mut().set_last_status(code);
                    }
                    Err(e) => {
                        eprintln!("\x1b[31mjaksh: {e}\x1b[0m");
                        suggest::maybe_suggest(&shell.borrow(), &line);
                        shell.borrow_mut().set_last_status(1);
                    }
                }
            }
            Err(ReadlineError::Interrupted) => {
                // Ctrl-C: clear current line, continue
                continue;
            }
            Err(ReadlineError::Eof) => {
                // Ctrl-D: exit
                println!("tạm biệt 👋");
                break;
            }
            Err(e) => {
                eprintln!("\x1b[31mjaksh: lỗi đọc dòng lệnh: {e}\x1b[0m");
                break;
            }
        }
    }

    let _ = rl.save_history(&history_path);
    Ok(())
}

fn run_line(shell: &Rc<RefCell<Shell>>, line: &str) -> Result<i32> {
    let tokens = lexer::tokenize(line)?;
    let ast = parser::parse(&tokens)?;
    executor::execute(shell, &ast)
}

fn print_welcome(shell: &Rc<RefCell<Shell>>) {
    let theme = &shell.borrow().theme.clone();
    let banner = format!(
        "{bold}{accent}JakShell{reset} {dim}v{ver}{reset}  {dim}—{reset} {note}\n{dim}gõ {reset}{accent}help{reset}{dim} hoặc {reset}{accent}?{reset}{dim} để xem hướng dẫn{reset}\n",
        bold = "\x1b[1m",
        accent = theme.accent_ansi(),
        dim = "\x1b[2m",
        reset = "\x1b[0m",
        ver = env!("CARGO_PKG_VERSION"),
        note = "shell nhanh, gọn, thân thiện",
    );
    print!("{banner}");
}
