use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Receiver;
use std::time::Duration;

use oweka::engine::{AbortHandle, ScanEvent};

pub fn write_bytes(path: &Path, size: usize) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, vec![7u8; size]).unwrap();
}

pub fn drain((receiver, _abort): (Receiver<ScanEvent>, AbortHandle)) -> Vec<ScanEvent> {
    let mut events = Vec::new();
    while let Ok(event) = receiver.recv_timeout(Duration::from_secs(10)) {
        let done = matches!(event, ScanEvent::Done);
        events.push(event);
        if done {
            break;
        }
    }
    events
}

pub fn found_paths(events: &[ScanEvent]) -> Vec<PathBuf> {
    events
        .iter()
        .filter_map(|event| match event {
            ScanEvent::Found { artifact } => Some(artifact.path.clone()),
            _ => None,
        })
        .collect()
}

pub fn ends_with_done(events: &[ScanEvent]) -> bool {
    matches!(events.last(), Some(ScanEvent::Done))
}
