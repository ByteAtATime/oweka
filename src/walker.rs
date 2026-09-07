use std::fs;
use std::io;
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::sync::{Condvar, Mutex};
use std::thread;

pub fn walk_dirs<F, E>(root: &Path, threads: NonZeroUsize, visit_dir: &F, on_error: &E)
where
    F: Fn(&Path) -> bool + Sync,
    E: Fn(&Path, &io::Error) + Sync,
{
    let frontier = Frontier::rooted_at(root);
    thread::scope(|scope| {
        for _ in 0..threads.get() {
            scope.spawn(|| drain(&frontier, visit_dir, on_error));
        }
    });
}

fn drain<F, E>(frontier: &Frontier, visit_dir: &F, on_error: &E)
where
    F: Fn(&Path) -> bool + Sync,
    E: Fn(&Path, &io::Error) + Sync,
{
    let mut accepted = Vec::new();
    while let Some(directory) = frontier.take() {
        expand(directory, visit_dir, on_error, &mut accepted);
        frontier.replenish(&mut accepted);
    }
}

fn expand<F, E>(directory: PathBuf, visit_dir: &F, on_error: &E, accepted: &mut Vec<PathBuf>)
where
    F: Fn(&Path) -> bool + Sync,
    E: Fn(&Path, &io::Error) + Sync,
{
    let entries = match fs::read_dir(&directory) {
        Ok(entries) => entries,
        Err(reason) => return on_error(&directory, &reason),
    };
    let mut candidate = directory;
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(reason) => {
                on_error(&candidate, &reason);
                continue;
            }
        };
        let file_type = match entry.file_type() {
            Ok(file_type) => file_type,
            Err(reason) => {
                on_error(&candidate.join(entry.file_name()), &reason);
                continue;
            }
        };
        if !file_type.is_dir() {
            continue;
        }
        candidate.push(entry.file_name());
        if visit_dir(&candidate) {
            accepted.push(candidate.clone());
        }
        candidate.pop();
    }
}

struct Frontier {
    state: Mutex<FrontierState>,
    available: Condvar,
}

struct FrontierState {
    pending: Vec<PathBuf>,
    expanding: usize,
}

impl Frontier {
    fn rooted_at(root: &Path) -> Self {
        Self {
            state: Mutex::new(FrontierState {
                pending: vec![root.to_path_buf()],
                expanding: 0,
            }),
            available: Condvar::new(),
        }
    }

    fn take(&self) -> Option<PathBuf> {
        let mut state = self.state.lock().unwrap();
        while state.pending.is_empty() && state.expanding > 0 {
            state = self.available.wait(state).unwrap();
        }
        let directory = state.pending.pop()?;
        state.expanding += 1;
        Some(directory)
    }

    fn replenish(&self, accepted: &mut Vec<PathBuf>) {
        let mut state = self.state.lock().unwrap();
        let may_have_waiters = state.pending.is_empty();
        state.pending.append(accepted);
        state.expanding -= 1;
        let walk_finished = state.pending.is_empty() && state.expanding == 0;
        if may_have_waiters || walk_finished {
            drop(state);
            self.available.notify_all();
        }
    }
}
