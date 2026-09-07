use std::fs::{self, DirEntry};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::SystemTime;

use dua_core::{Options, Order};

use crate::matcher::Artifact;
use crate::registry::claim;

#[derive(Debug)]
pub enum ScanEvent {
    Found {
        artifact: Artifact,
    },
    Sized {
        artifact: Artifact,
        bytes: u64,
        last_modified: Option<SystemTime>,
    },
    WalkError {
        path: PathBuf,
        reason: String,
    },
    Done,
}

pub fn scan(root: &Path) -> Receiver<ScanEvent> {
    let (events, receiver) = mpsc::channel();
    let root = root.to_path_buf();
    thread::spawn(move || run_pipeline(&root, &events));
    receiver
}

fn run_pipeline(root: &Path, events: &Sender<ScanEvent>) {
    let (jobs, queue) = mpsc::channel::<Artifact>();
    let shared = Arc::new(Mutex::new(queue));
    let workers: Vec<_> = (0..worker_count())
        .map(|_| spawn_sizer(Arc::clone(&shared), events.clone()))
        .collect();
    discover(root, &jobs, events);
    drop(jobs);
    for worker in workers {
        let _ = worker.join();
    }
    let _ = events.send(ScanEvent::Done);
}

fn spawn_sizer(
    queue: Arc<Mutex<Receiver<Artifact>>>,
    events: Sender<ScanEvent>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        while let Ok(artifact) = {
            let queue = queue.lock().unwrap();
            queue.recv()
        } {
            let tally = measure(&artifact.path, &events);
            let _ = events.send(ScanEvent::Sized {
                artifact,
                bytes: tally.bytes,
                last_modified: tally.last_modified,
            });
        }
    })
}

struct SizeTally {
    bytes: u64,
    last_modified: Option<SystemTime>,
}

fn newer(first: Option<SystemTime>, second: Option<SystemTime>) -> Option<SystemTime> {
    match (first, second) {
        (Some(first), Some(second)) => Some(first.max(second)),
        (Some(first), None) => Some(first),
        (None, second) => second,
    }
}

impl SizeTally {
    fn add_file(&mut self, metadata: &fs::Metadata) {
        self.bytes += metadata.len();
        self.last_modified = newer(self.last_modified, metadata.modified().ok());
    }
}

fn accumulate(
    entry: &DirEntry,
    tally: &mut SizeTally,
    pending: &mut Vec<PathBuf>,
    events: &Sender<ScanEvent>,
) {
    let file_type = match entry.file_type() {
        Ok(file_type) => file_type,
        Err(reason) => {
            send_walk_error(&entry.path(), &reason.to_string(), events);
            return;
        }
    };
    if file_type.is_symlink() {
        return;
    }
    if file_type.is_dir() {
        pending.push(entry.path());
        return;
    }
    match entry.metadata() {
        Ok(metadata) => tally.add_file(&metadata),
        Err(reason) => send_walk_error(&entry.path(), &reason.to_string(), events),
    }
}

fn measure(artifact: &Path, events: &Sender<ScanEvent>) -> SizeTally {
    let mut tally = SizeTally {
        bytes: 0,
        last_modified: None,
    };
    let mut pending = vec![artifact.to_path_buf()];
    while let Some(dir) = pending.pop() {
        match fs::read_dir(&dir) {
            Ok(entries) => {
                for entry in entries {
                    match entry {
                        Ok(entry) => accumulate(&entry, &mut tally, &mut pending, events),
                        Err(reason) => send_walk_error(&dir, &reason.to_string(), events),
                    }
                }
            }
            Err(reason) => send_walk_error(&dir, &reason.to_string(), events),
        }
    }
    tally
}

fn worker_count() -> usize {
    thread::available_parallelism()
        .map(|count| count.get())
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

fn emit_if_artifact(entry: &dua_core::Entry, jobs: &Sender<Artifact>, events: &Sender<ScanEvent>) {
    if entry.file_type.is_symlink() || !entry.file_type.is_dir() {
        return;
    }
    let path = entry.path();
    if is_git(&path) {
        return;
    }
    if let Some(matcher_id) = claim(&path) {
        let artifact = Artifact { matcher_id, path };
        let _ = events.send(ScanEvent::Found {
            artifact: artifact.clone(),
        });
        let _ = jobs.send(artifact);
    }
}

fn send_walk_error(root: &Path, reason: &str, events: &Sender<ScanEvent>) {
    let _ = events.send(ScanEvent::WalkError {
        path: root.to_path_buf(),
        reason: reason.to_string(),
    });
}

fn discover(root: &Path, jobs: &Sender<Artifact>, events: &Sender<ScanEvent>) {
    let walk = dua_core::walk(
        root,
        worker_count(),
        Order::ParentFirst,
        Options::default(),
        descend,
    );
    for item in walk {
        match item {
            Ok(entry) => emit_if_artifact(&entry, jobs, events),
            Err(reason) => send_walk_error(root, &reason.to_string(), events),
        }
    }
}
