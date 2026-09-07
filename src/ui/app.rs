use std::collections::HashSet;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime};

use ratatui::widgets::TableState;

use crate::engine::ScanEvent;
use crate::engine::{DeleteOutcome, DeleteResult, SizeReport};
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
    pub(super) deleting: bool,
    pub(super) failed: bool,
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
    freed_bytes: u64,
    done: bool,
    started: Instant,
    finished: Option<Instant>,
    scroll: usize,
    page_size: usize,
    table_state: TableState,
    pending: Option<Artifact>,
}

impl App {
    pub fn new(root: PathBuf) -> App {
        App {
            root,
            rows: Vec::new(),
            seen: HashSet::new(),
            errors: Vec::new(),
            freed_bytes: 0,
            done: false,
            started: Instant::now(),
            finished: None,
            scroll: 0,
            page_size: 0,
            table_state: TableState::new(),
            pending: None,
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

    pub fn page_up(&mut self) {
        self.move_cursor(-(page_step(self.page_size) as i32));
    }

    pub fn page_down(&mut self) {
        self.move_cursor(page_step(self.page_size) as i32);
    }

    pub fn selected_index(&self) -> Option<usize> {
        self.table_state.selected()
    }

    pub fn selected_artifact(&self) -> Option<Artifact> {
        let selected = self.table_state.selected()?;
        let row = self.rows.get(selected)?;
        Some(row.artifact.clone())
    }

    pub fn deletion_target(&self) -> Option<Artifact> {
        let selected = self.table_state.selected()?;
        let row = self.rows.get(selected)?;
        if row.deleting {
            return None;
        }
        Some(row.artifact.clone())
    }

    pub fn mark_deleting(&mut self, artifact: &Artifact) {
        if let Some(row) = find_row_mut(&mut self.rows, artifact) {
            row.deleting = true;
        }
    }

    pub fn open_confirm(&mut self, artifact: Artifact) {
        self.pending = Some(artifact);
    }

    pub fn cancel_confirm(&mut self) {
        self.pending = None;
    }

    pub fn pending_confirm(&self) -> Option<&Artifact> {
        self.pending.as_ref()
    }

    pub fn confirm_pending(&mut self) -> Option<Artifact> {
        let artifact = self.pending.take()?;
        let row = find_row_mut(&mut self.rows, &artifact)?;
        if row.deleting {
            return None;
        }
        row.deleting = true;
        Some(artifact)
    }

    pub fn apply_delete_result(&mut self, result: DeleteResult) {
        match result.outcome {
            DeleteOutcome::Deleted => self.remove_deleted(&result.artifact),
            DeleteOutcome::NotFound => {
                self.fail_row(&result.artifact, "no longer recognized", result.rescan);
            }
            DeleteOutcome::IoFailed(reason) => {
                self.fail_row(&result.artifact, &reason.to_string(), result.rescan);
            }
        }
    }

    pub(super) fn root(&self) -> &Path {
        &self.root
    }

    pub(super) fn rows(&self) -> &[Row] {
        &self.rows
    }

    pub(super) fn row_for(&self, artifact: &Artifact) -> Option<&Row> {
        self.rows.iter().find(|row| row_matches(row, artifact))
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

    pub(super) fn scroll(&self) -> usize {
        self.scroll
    }

    pub(super) fn set_scroll(&mut self, scroll: usize) {
        self.scroll = scroll;
    }

    pub(super) fn set_page_size(&mut self, page_size: usize) {
        self.page_size = page_size;
    }

    pub(super) fn set_render_selection(&mut self, selected: Option<usize>) {
        self.table_state.select(selected);
    }

    pub(super) fn total_bytes(&self) -> u64 {
        self.rows.iter().filter_map(|row| row.bytes).sum()
    }

    pub(super) fn freed_bytes(&self) -> u64 {
        self.freed_bytes
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
            deleting: false,
            failed: false,
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
        self.insert_sorted(row);
    }

    fn remove_deleted(&mut self, artifact: &Artifact) {
        let Some(index) = self.rows.iter().position(|row| row_matches(row, artifact)) else {
            return;
        };
        let selected = self.table_state.selected();
        self.freed_bytes += self.rows[index].bytes.unwrap_or(0);
        self.rows.remove(index);
        if self.rows.is_empty() {
            self.table_state.select(None);
            return;
        }
        if let Some(selected) = selected {
            self.table_state
                .select(Some(shift_after_removal(selected, index, self.rows.len())));
        }
    }

    fn fail_row(&mut self, artifact: &Artifact, reason: &str, rescan: Option<SizeReport>) {
        self.errors.push(ScanError {
            path: artifact.path.clone(),
            reason: reason.to_string(),
        });
        let Some(index) = self.rows.iter().position(|row| row_matches(row, artifact)) else {
            return;
        };
        let mut row = self.rows.remove(index);
        row.deleting = false;
        row.failed = true;
        if let Some(report) = rescan {
            row.bytes = Some(report.bytes);
            row.last_modified = report.last_modified;
            for error in report.errors {
                self.errors.push(ScanError {
                    path: error.path,
                    reason: error.reason.to_string(),
                });
            }
        }
        self.insert_sorted(row);
        let settled = self.rows.iter().position(|row| row_matches(row, artifact));
        self.table_state.select(settled);
    }

    fn insert_sorted(&mut self, row: Row) {
        let position = match row.bytes {
            None => self.rows.len(),
            Some(bytes) => self
                .rows
                .partition_point(|candidate| sorts_before(candidate, bytes, &row.artifact.path)),
        };
        self.rows.insert(position, row);
    }

    fn finish(&mut self) {
        self.done = true;
        self.finished = Some(Instant::now());
    }
}

fn page_step(page_size: usize) -> usize {
    page_size.saturating_sub(1).max(1)
}

fn sorts_before(candidate: &Row, bytes: u64, path: &Path) -> bool {
    match candidate.bytes {
        None => false,
        Some(existing) => {
            existing > bytes || (existing == bytes && candidate.artifact.path.as_path() < path)
        }
    }
}

fn row_matches(row: &Row, artifact: &Artifact) -> bool {
    row.path_hash == hash_path(&artifact.path) && row.artifact.path == artifact.path
}

fn find_row_mut<'rows>(rows: &'rows mut [Row], artifact: &Artifact) -> Option<&'rows mut Row> {
    rows.iter_mut().find(|row| row_matches(row, artifact))
}

fn shift_after_removal(selected: usize, removed: usize, remaining: usize) -> usize {
    if selected > removed {
        return selected - 1;
    }
    selected.min(remaining - 1)
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

    #[test]
    fn opening_confirmation_stores_pending_artifact() {
        let mut app = streaming_app();
        app.apply(ScanEvent::Done);
        let target = artifact("venv", "/root/b");
        app.open_confirm(target.clone());
        assert_eq!(app.pending_confirm(), Some(&target));
    }

    #[test]
    fn canceling_confirmation_marks_no_row_deleting() {
        let mut app = streaming_app();
        app.apply(ScanEvent::Done);
        app.open_confirm(artifact("venv", "/root/b"));
        app.cancel_confirm();
        assert_eq!(app.pending_confirm(), None);
        assert!(app.rows().iter().all(|row| !row.deleting));
    }

    #[test]
    fn confirming_marks_row_by_path_not_selection() {
        let mut app = streaming_app();
        app.apply(ScanEvent::Done);
        app.open_confirm(artifact("node_modules", "/root/b"));
        app.move_cursor(-1);
        let confirmed = app.confirm_pending();
        assert_eq!(confirmed, Some(artifact("node_modules", "/root/b")));
        assert_eq!(app.pending_confirm(), None);
        let row = app
            .rows()
            .iter()
            .find(|row| row.artifact.path == PathBuf::from("/root/b"))
            .unwrap();
        assert!(row.deleting);
        assert!(app.rows().iter().filter(|row| row.deleting).count() == 1);
    }

    #[test]
    fn confirming_missing_row_clears_pending_without_panic() {
        let mut app = streaming_app();
        app.apply(ScanEvent::Done);
        app.open_confirm(artifact("venv", "/root/gone"));
        let confirmed = app.confirm_pending();
        assert_eq!(confirmed, None);
        assert_eq!(app.pending_confirm(), None);
        assert!(app.rows().iter().all(|row| !row.deleting));
    }
}
