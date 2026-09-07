use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

use dua_core::{Order, Options};

use crate::matcher::Artifact;
use crate::registry::claim;

pub enum ScanEvent {
    Found { artifact: Artifact },
    WalkError { path: PathBuf, reason: String },
    Done,
}

pub fn scan(root: &Path) -> Receiver<ScanEvent> {
    let (events, receiver) = mpsc::channel();
    let root = root.to_path_buf();
    thread::spawn(move || discover(&root, &events));
    receiver
}

fn worker_count() -> usize {
    thread::available_parallelism()
        .map(|count| count.get().min(8))
        .unwrap_or(4)
}

fn is_git(path: &Path) -> bool {
    path.file_name().is_some_and(|name| name == ".git")
}

fn descend(entry: &dua_core::Entry) -> bool {
    if !entry.file_type.is_dir() {
        return true;
    }
    let path = entry.path();
    if is_git(&path) {
        return false;
    }
    claim(&path).is_none()
}

fn emit_if_artifact(entry: &dua_core::Entry, events: &Sender<ScanEvent>) {
    if entry.file_type.is_symlink() || !entry.file_type.is_dir() {
        return;
    }
    let path = entry.path();
    if is_git(&path) {
        return;
    }
    if let Some(matcher_id) = claim(&path) {
        let _ = events.send(ScanEvent::Found {
            artifact: Artifact { matcher_id, path },
        });
    }
}

fn send_walk_error(root: &Path, reason: &str, events: &Sender<ScanEvent>) {
    let _ = events.send(ScanEvent::WalkError {
        path: root.to_path_buf(),
        reason: reason.to_string(),
    });
}

fn discover(root: &Path, events: &Sender<ScanEvent>) {
    let walk = dua_core::walk(
        root,
        worker_count(),
        Order::ParentFirst,
        Options::default(),
        descend,
    );
    for item in walk {
        match item {
            Ok(entry) => emit_if_artifact(&entry, events),
            Err(reason) => send_walk_error(root, &reason.to_string(), events),
        }
    }
    let _ = events.send(ScanEvent::Done);
}
