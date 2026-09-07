use std::collections::HashSet;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime};

use ratatui::widgets::TableState;

use crate::engine::ScanEvent;
use crate::matcher::Artifact;

struct ScanError {
    path: PathBuf,
    reason: String,
}

pub(super) struct Row {
    pub(super) artifact: Artifact,
    path_hash: u64,
    pub(super) bytes: Option<u64>,
    pub(super) last_modified: Option<SystemTime>,
}

fn hash_path(path: &Path) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    path.hash(&mut hasher);
    hasher.finish()
}

pub struct App {
    root: PathBuf,
    rows: Vec<Row>,
    seen: HashSet<PathBuf>,
    errors: Vec<ScanError>,
    done: bool,
    started: Instant,
    finished: Option<Instant>,
    table_state: TableState,
}

impl App {
    pub fn new(root: PathBuf) -> App {
        App {
            root,
            rows: Vec::new(),
            seen: HashSet::new(),
            errors: Vec::new(),
            done: false,
            started: Instant::now(),
            finished: None,
            table_state: TableState::new(),
        }
    }

    pub fn apply(&mut self, event: ScanEvent) {
        match event {
            ScanEvent::Found { artifact } => self.insert_row(artifact),
            ScanEvent::Sized {
                artifact,
                bytes,
                last_modified,
            } => self.size_row(&artifact, bytes, last_modified),
            ScanEvent::WalkError { path, reason } => self.errors.push(ScanError { path, reason }),
            ScanEvent::Done => self.finish(),
        }
    }

    pub fn move_cursor(&mut self, delta: i32) {
        if !self.done {
            return;
        }
        if self.rows.is_empty() {
            return;
        }
        let current = self.table_state.selected().unwrap_or(0);
        let next = (current as i32 + delta).clamp(0, self.rows.len() as i32 - 1) as usize;
        self.table_state.select(Some(next));
    }

    pub fn selected_index(&self) -> Option<usize> {
        self.table_state.selected()
    }

    pub(super) fn root(&self) -> &Path {
        &self.root
    }

    pub(super) fn rows(&self) -> &[Row] {
        &self.rows
    }

    pub(super) fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    pub(super) fn error_count(&self) -> usize {
        self.errors.len()
    }

    pub(super) fn is_done(&self) -> bool {
        self.done
    }

    pub(super) fn started(&self) -> Instant {
        self.started
    }

    pub(super) fn finished(&self) -> Option<Instant> {
        self.finished
    }

    pub(super) fn table_state_mut(&mut self) -> &mut TableState {
        &mut self.table_state
    }

    pub(super) fn total_bytes(&self) -> u64 {
        self.rows.iter().filter_map(|row| row.bytes).sum()
    }

    fn insert_row(&mut self, artifact: Artifact) {
        if !self.seen.insert(artifact.path.clone()) {
            return;
        }
        if self.table_state.selected().is_none() {
            self.table_state.select(Some(0));
        }
        self.rows.push(Row {
            path_hash: hash_path(&artifact.path),
            artifact,
            bytes: None,
            last_modified: None,
        });
    }

    fn size_row(&mut self, artifact: &Artifact, bytes: u64, last_modified: Option<SystemTime>) {
        let path_hash = hash_path(&artifact.path);
        let Some(index) = self
            .rows
            .iter()
            .position(|row| row.path_hash == path_hash && row.artifact.path == artifact.path)
        else {
            return;
        };
        let mut row = self.rows.remove(index);
        row.bytes = Some(bytes);
        row.last_modified = last_modified;
        let position = self
            .rows
            .partition_point(|candidate| sorts_before(candidate, bytes, &row.artifact.path));
        self.rows.insert(position, row);
    }

    fn finish(&mut self) {
        self.done = true;
        self.finished = Some(Instant::now());
    }
}

fn sorts_before(candidate: &Row, bytes: u64, path: &Path) -> bool {
    match candidate.bytes {
        None => false,
        Some(existing) => {
            existing > bytes || (existing == bytes && candidate.artifact.path.as_path() < path)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn artifact(matcher_id: &'static str, name: &str) -> Artifact {
        Artifact {
            matcher_id,
            path: PathBuf::from(name),
        }
    }

    fn streaming_app() -> App {
        let mut app = App::new(PathBuf::from("/root"));
        app.apply(ScanEvent::Found {
            artifact: artifact("node_modules", "/root/a"),
        });
        app.apply(ScanEvent::Found {
            artifact: artifact("node_modules", "/root/b"),
        });
        app.apply(ScanEvent::Sized {
            artifact: artifact("node_modules", "/root/b"),
            bytes: 2048,
            last_modified: None,
        });
        app
    }

    #[test]
    fn selection_stays_pinned_while_streaming() {
        let mut app = streaming_app();
        assert_eq!(app.selected_index(), Some(0));
        app.move_cursor(1);
        assert_eq!(app.selected_index(), Some(0));
    }

    #[test]
    fn cursor_moves_and_clamps_after_done() {
        let mut app = streaming_app();
        app.apply(ScanEvent::Done);
        app.move_cursor(1);
        assert_eq!(app.selected_index(), Some(1));
        app.move_cursor(-5);
        assert_eq!(app.selected_index(), Some(0));
    }
}
