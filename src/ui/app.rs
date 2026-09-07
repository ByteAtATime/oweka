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
    pub(super) bytes: Option<u64>,
    pub(super) last_modified: Option<SystemTime>,
    sequence: usize,
}

pub struct App {
    root: PathBuf,
    rows: Vec<Row>,
    errors: Vec<ScanError>,
    done: bool,
    started: Instant,
    finished: Option<Instant>,
    table_state: TableState,
    next_sequence: usize,
}

impl App {
    pub fn new(root: PathBuf) -> App {
        App {
            root,
            rows: Vec::new(),
            errors: Vec::new(),
            done: false,
            started: Instant::now(),
            finished: None,
            table_state: TableState::new(),
            next_sequence: 0,
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
        if self
            .rows
            .iter()
            .any(|row| row.artifact.path == artifact.path)
        {
            return;
        }
        if self.table_state.selected().is_none() {
            self.table_state.select(Some(0));
        }
        self.rows.push(Row {
            artifact,
            bytes: None,
            last_modified: None,
            sequence: self.next_sequence,
        });
        self.next_sequence += 1;
        self.sort_rows();
    }

    fn size_row(&mut self, artifact: &Artifact, bytes: u64, last_modified: Option<SystemTime>) {
        let Some(row) = self
            .rows
            .iter_mut()
            .find(|row| row.artifact.path == artifact.path)
        else {
            return;
        };
        row.bytes = Some(bytes);
        row.last_modified = last_modified;
        self.sort_rows();
    }

    fn finish(&mut self) {
        self.done = true;
        self.finished = Some(Instant::now());
    }

    fn sort_rows(&mut self) {
        self.rows
            .sort_by(|first, second| match (first.bytes, second.bytes) {
                (Some(a), Some(b)) => b
                    .cmp(&a)
                    .then_with(|| first.artifact.path.cmp(&second.artifact.path)),
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (None, None) => first.sequence.cmp(&second.sequence),
            });
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
