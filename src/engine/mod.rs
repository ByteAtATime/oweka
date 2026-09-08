mod frontier;
mod risk;

use std::fs::{self, DirEntry};
use std::io;
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::SystemTime;

use frontier::{Frontier, WalkItem};

use crate::matcher::Artifact;
use crate::registry::{claim, matcher_for};

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

pub struct AbortHandle {
    aborted: Arc<AtomicBool>,
}

impl AbortHandle {
    pub fn abort(&self) {
        self.aborted.store(true, Ordering::Release);
    }
}

pub fn scan(root: &Path) -> (Receiver<ScanEvent>, AbortHandle) {
    let (events, receiver) = mpsc::channel();
    let root = root.to_path_buf();
    let aborted = Arc::new(AtomicBool::new(false));
    let handle = AbortHandle {
        aborted: Arc::clone(&aborted),
    };
    thread::spawn(move || run_pipeline(&root, &events, &aborted));
    (receiver, handle)
}

fn run_pipeline(root: &Path, events: &Sender<ScanEvent>, aborted: &Arc<AtomicBool>) {
    let (jobs, queue) = mpsc::channel::<Artifact>();
    let shared = Arc::new(Mutex::new(queue));
    let sizers: Vec<_> = (0..worker_count().get())
        .map(|_| spawn_sizer(Arc::clone(&shared), events.clone(), Arc::clone(aborted)))
        .collect();
    discover(root, &jobs, events, aborted);
    drop(jobs);
    for sizer in sizers {
        let _ = sizer.join();
    }
    let _ = events.send(ScanEvent::Done);
}

fn spawn_sizer(
    queue: Arc<Mutex<Receiver<Artifact>>>,
    events: Sender<ScanEvent>,
    aborted: Arc<AtomicBool>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        while let Some(artifact) = next_job(&queue, &aborted) {
            let report = size_artifact_abortable(&artifact, &aborted);
            if aborted.load(Ordering::Acquire) {
                return;
            }
            for error in &report.errors {
                let _ = events.send(ScanEvent::WalkError {
                    path: error.path.clone(),
                    reason: error.reason.to_string(),
                });
            }
            let _ = events.send(ScanEvent::Sized {
                artifact,
                bytes: report.bytes,
                last_modified: report.last_modified,
            });
        }
    })
}

fn next_job(queue: &Mutex<Receiver<Artifact>>, aborted: &AtomicBool) -> Option<Artifact> {
    if aborted.load(Ordering::Acquire) {
        return None;
    }
    queue.lock().unwrap().recv().ok()
}

#[derive(Debug)]
pub struct SizeError {
    pub path: PathBuf,
    pub reason: io::Error,
}

#[derive(Debug, Default)]
pub struct SizeReport {
    pub bytes: u64,
    pub last_modified: Option<SystemTime>,
    pub errors: Vec<SizeError>,
}

impl SizeReport {
    fn add_file(&mut self, metadata: &fs::Metadata) {
        self.bytes += metadata.len();
        self.last_modified = self.last_modified.max(metadata.modified().ok());
    }

    fn record(&mut self, path: &Path, reason: io::Error) {
        self.errors.push(SizeError {
            path: path.to_path_buf(),
            reason,
        });
    }
}

#[derive(Debug)]
pub enum DeleteOutcome {
    Deleted,
    NotFound,
    IoFailed(io::Error),
}

#[derive(Debug)]
pub struct DeleteResult {
    pub artifact: Artifact,
    pub outcome: DeleteOutcome,
    pub rescan: Option<SizeReport>,
}

pub fn size_artifact(artifact: &Artifact) -> SizeReport {
    size_artifact_abortable(artifact, &AtomicBool::new(false))
}

fn size_artifact_abortable(artifact: &Artifact, aborted: &AtomicBool) -> SizeReport {
    let mut report = SizeReport::default();
    let mut pending = vec![artifact.path.clone()];
    while let Some(directory) = pending.pop() {
        if aborted.load(Ordering::Acquire) {
            return report;
        }
        let entries = match fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(reason) => {
                report.record(&directory, reason);
                continue;
            }
        };
        for entry in entries {
            match entry {
                Ok(entry) => accumulate(&entry, &mut report, &mut pending),
                Err(reason) => report.record(&directory, reason),
            }
        }
    }
    report
}

pub fn delete_artifact(artifact: &Artifact) -> DeleteResult {
    let outcome = delete_outcome(artifact);
    let rescan = rescan_after(artifact, &outcome);
    DeleteResult {
        artifact: artifact.clone(),
        outcome,
        rescan,
    }
}

fn delete_outcome(artifact: &Artifact) -> DeleteOutcome {
    if !is_still_claimed(artifact) {
        return DeleteOutcome::NotFound;
    }
    invoke_matcher_delete(artifact)
}

fn rescan_after(artifact: &Artifact, outcome: &DeleteOutcome) -> Option<SizeReport> {
    match outcome {
        DeleteOutcome::Deleted => None,
        _ => Some(size_artifact(artifact)),
    }
}

fn is_still_claimed(artifact: &Artifact) -> bool {
    artifact
        .path
        .symlink_metadata()
        .is_ok_and(|metadata| metadata.file_type().is_dir())
        && claim(&artifact.path).is_some_and(|matcher| matcher.id() == artifact.matcher_id)
}

fn invoke_matcher_delete(artifact: &Artifact) -> DeleteOutcome {
    let Some(matcher) = matcher_for(artifact.matcher_id) else {
        return DeleteOutcome::NotFound;
    };
    match matcher.delete(artifact.path()) {
        Ok(()) => DeleteOutcome::Deleted,
        Err(reason) => DeleteOutcome::IoFailed(reason),
    }
}

fn accumulate(entry: &DirEntry, report: &mut SizeReport, pending: &mut Vec<PathBuf>) {
    let file_type = match entry.file_type() {
        Ok(file_type) => file_type,
        Err(reason) => return report.record(&entry.path(), reason),
    };
    if file_type.is_symlink() {
        return;
    }
    if file_type.is_dir() {
        pending.push(entry.path());
        return;
    }
    match entry.metadata() {
        Ok(metadata) => report.add_file(&metadata),
        Err(reason) => report.record(&entry.path(), reason),
    }
}

fn worker_count() -> NonZeroUsize {
    thread::available_parallelism().unwrap_or(NonZeroUsize::new(4).unwrap())
}

fn is_git(path: &Path) -> bool {
    path.file_name().is_some_and(|name| name == ".git")
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
        risk: risk::analyze(path),
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

fn discover(
    root: &Path,
    jobs: &Sender<Artifact>,
    events: &Sender<ScanEvent>,
    aborted: &Arc<AtomicBool>,
) {
    let frontier = Frontier::new(root.to_path_buf(), aborted);
    while let Some(item) = frontier.next() {
        match item {
            WalkItem::WalkError { path, reason } => report_walk_error(&path, &reason, events),
            WalkItem::Dir(dir) => match claim(&dir) {
                Some(matcher) => emit_artifact(&dir, matcher.id(), jobs, events),
                None => {
                    if !is_git(&dir) {
                        frontier.expand(&dir);
                    }
                }
            },
        }
    }
}
