use std::fs::{self, DirEntry};
use std::io;
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::SystemTime;

use crate::matcher::Artifact;
use crate::registry::claim;
use crate::walker::walk_dirs;

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
    let sizers: Vec<_> = (0..worker_count().get())
        .map(|_| spawn_sizer(Arc::clone(&shared), events.clone()))
        .collect();
    discover(root, &jobs, events);
    drop(jobs);
    for sizer in sizers {
        let _ = sizer.join();
    }
    let _ = events.send(ScanEvent::Done);
}

fn spawn_sizer(
    queue: Arc<Mutex<Receiver<Artifact>>>,
    events: Sender<ScanEvent>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        while let Some(artifact) = next_job(&queue) {
            let tally = measure(&artifact.path, &events);
            let _ = events.send(ScanEvent::Sized {
                artifact,
                bytes: tally.bytes,
                last_modified: tally.last_modified,
            });
        }
    })
}

fn next_job(queue: &Mutex<Receiver<Artifact>>) -> Option<Artifact> {
    queue.lock().unwrap().recv().ok()
}

#[derive(Default)]
struct SizeTally {
    bytes: u64,
    last_modified: Option<SystemTime>,
}

impl SizeTally {
    fn add_file(&mut self, metadata: &fs::Metadata) {
        self.bytes += metadata.len();
        self.last_modified = self.last_modified.max(metadata.modified().ok());
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
        Err(reason) => return report_walk_error(&entry.path(), &reason, events),
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
        Err(reason) => report_walk_error(&entry.path(), &reason, events),
    }
}

fn measure(artifact: &Path, events: &Sender<ScanEvent>) -> SizeTally {
    let mut tally = SizeTally::default();
    let mut pending = vec![artifact.to_path_buf()];
    while let Some(directory) = pending.pop() {
        let entries = match fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(reason) => {
                report_walk_error(&directory, &reason, events);
                continue;
            }
        };
        for entry in entries {
            match entry {
                Ok(entry) => accumulate(&entry, &mut tally, &mut pending, events),
                Err(reason) => report_walk_error(&directory, &reason, events),
            }
        }
    }
    tally
}

fn worker_count() -> NonZeroUsize {
    thread::available_parallelism().unwrap_or(NonZeroUsize::new(4).unwrap())
}

fn is_git(path: &Path) -> bool {
    path.file_name().is_some_and(|name| name == ".git")
}

fn visit_candidate(candidate: &Path, jobs: &Sender<Artifact>, events: &Sender<ScanEvent>) -> bool {
    if let Some(matcher_id) = claim(candidate) {
        emit_artifact(candidate, matcher_id, jobs, events);
        return false;
    }
    !is_git(candidate)
}

fn emit_artifact(
    path: &Path,
    matcher_id: &'static str,
    jobs: &Sender<Artifact>,
    events: &Sender<ScanEvent>,
) {
    let artifact = Artifact {
        matcher_id,
        path: path.to_path_buf(),
    };
    let _ = events.send(ScanEvent::Found {
        artifact: artifact.clone(),
    });
    let _ = jobs.send(artifact);
}

fn report_walk_error(path: &Path, reason: &io::Error, events: &Sender<ScanEvent>) {
    let _ = events.send(ScanEvent::WalkError {
        path: path.to_path_buf(),
        reason: reason.to_string(),
    });
}

fn discover(root: &Path, jobs: &Sender<Artifact>, events: &Sender<ScanEvent>) {
    if !visit_candidate(root, jobs, events) {
        return;
    }
    walk_dirs(
        root,
        worker_count(),
        &|candidate| visit_candidate(candidate, jobs, events),
        &|path, reason| report_walk_error(path, reason, events),
    );
}
