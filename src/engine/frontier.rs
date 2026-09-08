use std::collections::VecDeque;
use std::fs;
use std::io;
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::Duration;

pub(super) enum WalkItem {
    Dir(PathBuf),
    WalkError { path: PathBuf, reason: io::Error },
}

pub(super) struct Frontier {
    shared: Arc<Shared>,
    handles: Option<Vec<thread::JoinHandle<()>>>,
}

struct Shared {
    state: Mutex<State>,
    ready: Condvar,
    aborted: Arc<AtomicBool>,
}

struct State {
    items: VecDeque<WalkItem>,
    work: VecDeque<PathBuf>,
    active: usize,
    done: bool,
}

impl Frontier {
    pub(super) fn new(root: PathBuf, aborted: &Arc<AtomicBool>) -> Self {
        let shared = Arc::new(Shared {
            state: Mutex::new(State {
                items: VecDeque::from([WalkItem::Dir(root)]),
                work: VecDeque::new(),
                active: 0,
                done: false,
            }),
            ready: Condvar::new(),
            aborted: Arc::clone(aborted),
        });
        let workers = thread::available_parallelism()
            .unwrap_or(NonZeroUsize::new(4).unwrap())
            .get();
        let handles = (0..workers)
            .map(|_| {
                let shared = Arc::clone(&shared);
                thread::spawn(move || run_worker(&shared))
            })
            .collect();
        Self {
            shared,
            handles: Some(handles),
        }
    }

    pub(super) fn next(&self) -> Option<WalkItem> {
        let mut state = self.shared.state.lock().unwrap();
        loop {
            if self.shared.aborted.load(Ordering::Acquire) {
                state.done = true;
                drop(state);
                self.shared.ready.notify_all();
                return None;
            }
            if let Some(item) = state.items.pop_front() {
                return Some(item);
            }
            if state.work.is_empty() && state.active == 0 {
                state.done = true;
                drop(state);
                self.shared.ready.notify_all();
                return None;
            }
            let waited = self
                .shared
                .ready
                .wait_timeout(state, Duration::from_millis(10))
                .unwrap();
            state = waited.0;
        }
    }

    pub(super) fn expand(&self, dir: &Path) {
        {
            let mut state = self.shared.state.lock().unwrap();
            if state.done || self.shared.aborted.load(Ordering::Acquire) {
                return;
            }
            state.work.push_back(dir.to_path_buf());
        }
        self.shared.ready.notify_one();
    }
}

impl Drop for Frontier {
    fn drop(&mut self) {
        {
            let mut state = self.shared.state.lock().unwrap();
            state.done = true;
        }
        self.shared.ready.notify_all();
        if let Some(handles) = self.handles.take() {
            for handle in handles {
                let _ = handle.join();
            }
        }
    }
}

fn run_worker(shared: &Shared) {
    loop {
        let directory = {
            let mut state = shared.state.lock().unwrap();
            loop {
                if shared.aborted.load(Ordering::Acquire) || state.done {
                    return;
                }
                if !state.work.is_empty() {
                    break;
                }
                let waited = shared
                    .ready
                    .wait_timeout(state, Duration::from_millis(10))
                    .unwrap();
                state = waited.0;
            }
            match state.work.pop_front() {
                Some(directory) => {
                    state.active += 1;
                    directory
                }
                None => return,
            }
        };
        let produced = enumerate(&directory);
        {
            let mut state = shared.state.lock().unwrap();
            state.active -= 1;
            state.items.extend(produced);
        }
        shared.ready.notify_all();
    }
}

fn enumerate(directory: &Path) -> Vec<WalkItem> {
    let mut found = Vec::new();
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(reason) => {
            return vec![WalkItem::WalkError {
                path: directory.to_path_buf(),
                reason,
            }];
        }
    };
    let mut candidate = directory.to_path_buf();
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(reason) => {
                found.push(WalkItem::WalkError {
                    path: candidate.clone(),
                    reason,
                });
                continue;
            }
        };
        let file_type = match entry.file_type() {
            Ok(file_type) => file_type,
            Err(reason) => {
                found.push(WalkItem::WalkError {
                    path: candidate.join(entry.file_name()),
                    reason,
                });
                continue;
            }
        };
        if !file_type.is_dir() {
            continue;
        }
        candidate.push(entry.file_name());
        found.push(WalkItem::Dir(candidate.clone()));
        candidate.pop();
    }
    found
}
