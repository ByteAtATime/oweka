use std::env;
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
    let events = engine::scan(&root);
    if let Err(reason) = ui::run(&root, events) {
        eprintln!("terminal error: {reason}");
        std::process::exit(1);
    }
}
