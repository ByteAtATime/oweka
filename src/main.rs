use std::env;
use std::path::PathBuf;

use oweka::engine::{self, ScanEvent};

fn main() {
    let root = env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| env::current_dir().expect("working directory is readable"));
    if !root.is_dir() {
        eprintln!("{} does not exist or is not a directory", root.display());
        std::process::exit(1);
    }
    for event in engine::scan(&root) {
        match event {
            ScanEvent::Found { artifact } => {
                println!("found {} {}", artifact.matcher_id, artifact.path.display())
            }
            ScanEvent::Sized {
                artifact,
                bytes,
                last_modified,
            } => {
                println!(
                    "sized {} {bytes} {:?}",
                    artifact.path.display(),
                    last_modified
                )
            }
            ScanEvent::WalkError { path, reason } => {
                println!("error {} {reason}", path.display())
            }
            ScanEvent::Done => println!("done"),
        }
    }
}
