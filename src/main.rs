use std::env;
use std::io::IsTerminal;
use std::path::PathBuf;

use oweka::engine;
use oweka::ui;

fn main() {
    let root = env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| env::current_dir().expect("working directory is readable"));
    if !root.is_dir() {
        eprintln!("{} does not exist or is not a directory", root.display());
        std::process::exit(1);
    }
    let (events, abort) = engine::scan(&root);
    match ui::run(&root, events, abort) {
        Ok(freed_bytes) => {
            let styled = std::io::stdout().is_terminal()
                && env::var_os("NO_COLOR").is_none_or(|value| value.is_empty());
            println!("{}", ui::farewell(freed_bytes, styled));
        }
        Err(reason) => {
            eprintln!("terminal error: {reason}");
            std::process::exit(1);
        }
    }
}
