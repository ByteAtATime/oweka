use std::collections::HashSet;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime};

use super::format::format_size;
use crate::engine::AbortHandle;
use crate::engine::ScanEvent;
use crate::engine::{DeleteOutcome, DeleteResult, SizeReport};
use crate::matcher::Artifact;

struct ScanError {
    pub(super) path: PathBuf,
    pub(super) reason: String,
}

struct Pending {
    artifact: Artifact,
    note: Option<&'static str>,
}

const SPINNER: [char; 4] = ['|', '/', '-', '\\'];

pub(super) const MAX_ERROR_ROWS: u16 = 20;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RowStatus {
    Live,
    Deleting,
    Deleted,
    Failed,
}

pub(super) struct Row {
    pub(super) artifact: Artifact,
    path_hash: u64,
    pub(super) bytes: Option<u64>,
    pub(super) last_modified: Option<SystemTime>,
    pub(super) status: RowStatus,
}

fn hash_path(path: &Path) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    path.hash(&mut hasher);
    hasher.finish()
}

pub struct RowView {
    pub path: PathBuf,
    pub matcher_id: &'static str,
    pub risk: Option<&'static str>,
    pub bytes: Option<u64>,
    pub last_modified: Option<SystemTime>,
    pub status: RowStatus,
}

pub struct ConfirmView {
    pub path: PathBuf,
    pub matcher_id: &'static str,
    pub note: Option<&'static str>,
    pub bytes: Option<u64>,
    pub last_modified: Option<SystemTime>,
}

pub struct ErrorView {
    pub path: PathBuf,
    pub reason: String,
}

pub struct ViewState {
    pub scan_status: String,
    pub elapsed_secs: u64,
    pub done: bool,
    pub aborted: bool,
    pub row_count: usize,
    pub root: PathBuf,
    pub potential: String,
    pub freed: String,
    pub error_count: usize,
    pub errors: Vec<ErrorView>,
    pub errors_open: bool,
    pub error_scroll: usize,
    pub rows: Vec<RowView>,
    pub selection: Option<usize>,
    pub confirm: Option<ConfirmView>,
}

pub struct App {
    root: PathBuf,
    rows: Vec<Row>,
    seen: HashSet<PathBuf>,
    errors: Vec<ScanError>,
    freed_bytes: u64,
    done: bool,
    aborted: bool,
    abort_handle: Option<AbortHandle>,
    started: Instant,
    finished: Option<Instant>,
    scroll: usize,
    page_size: usize,
    selected: Option<usize>,
    pending: Option<Pending>,
    show_errors: bool,
    error_scroll: usize,
    error_page: usize,
}

impl App {
    pub fn new(root: PathBuf, started: Instant) -> App {
        App {
            root,
            rows: Vec::new(),
            seen: HashSet::new(),
            errors: Vec::new(),
            freed_bytes: 0,
            done: false,
            aborted: false,
            abort_handle: None,
            started,
            finished: None,
            scroll: 0,
            page_size: 0,
            selected: None,
            pending: None,
            show_errors: false,
            error_scroll: 0,
            error_page: 0,
        }
    }

    pub fn set_abort_handle(&mut self, handle: AbortHandle) {
        self.abort_handle = Some(handle);
    }

    pub fn abort(&mut self) {
        if self.done {
            return;
        }
        if let Some(handle) = self.abort_handle.take() {
            handle.abort();
        }
        self.aborted = true;
        self.finish();
    }

    pub fn apply(&mut self, event: ScanEvent) {
        if self.aborted {
            return;
        }
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

    pub fn toggle_errors(&mut self) {
        self.show_errors = !self.show_errors;
        if self.show_errors {
            self.error_scroll = 0;
        }
    }

    pub fn close_errors(&mut self) {
        self.show_errors = false;
    }

    pub fn errors_open(&self) -> bool {
        self.show_errors
    }

    pub fn is_done(&self) -> bool {
        self.done
    }

    pub fn scroll_errors(&mut self, delta: i32) {
        if self.errors.is_empty() {
            return;
        }
        let last = self.errors.len().saturating_sub(self.error_page.max(1)) as i32;
        self.error_scroll = (self.error_scroll as i32 + delta).clamp(0, last) as usize;
    }

    pub fn move_cursor(&mut self, delta: i32) {
        if !self.done {
            return;
        }
        if self.rows.is_empty() {
            return;
        }
        let current = self.selected.unwrap_or(0);
        let next = (current as i32 + delta).clamp(0, self.rows.len() as i32 - 1) as usize;
        self.selected = Some(next);
    }

    pub fn page_up(&mut self) {
        self.move_cursor(-(page_step(self.page_size) as i32));
    }

    pub fn page_down(&mut self) {
        self.move_cursor(page_step(self.page_size) as i32);
    }

    pub fn selected_artifact(&self) -> Option<Artifact> {
        let selected = self.selected?;
        let row = self.rows.get(selected)?;
        Some(row.artifact.clone())
    }

    pub fn deletion_target(&self) -> Option<Artifact> {
        let selected = self.selected?;
        let row = self.rows.get(selected)?;
        if matches!(row.status, RowStatus::Deleting | RowStatus::Deleted) {
            return None;
        }
        Some(row.artifact.clone())
    }

    pub fn mark_deleting(&mut self, artifact: &Artifact) {
        let Some(row) = find_row_mut(&mut self.rows, artifact) else {
            return;
        };
        if matches!(row.status, RowStatus::Deleting | RowStatus::Deleted) {
            return;
        }
        row.status = RowStatus::Deleting;
    }

    pub fn open_confirm(&mut self, artifact: Artifact, note: Option<&'static str>) {
        self.pending = Some(Pending { artifact, note });
    }

    pub fn cancel_confirm(&mut self) {
        self.pending = None;
    }

    pub fn pending_confirm(&self) -> Option<(&Artifact, Option<&'static str>)> {
        self.pending
            .as_ref()
            .map(|pending| (&pending.artifact, pending.note))
    }

    pub fn confirm_pending(&mut self) -> Option<Artifact> {
        let pending = self.pending.take()?;
        let row = find_row_mut(&mut self.rows, &pending.artifact)?;
        if matches!(row.status, RowStatus::Deleting | RowStatus::Deleted) {
            return None;
        }
        row.status = RowStatus::Deleting;
        Some(pending.artifact)
    }

    pub fn apply_delete_result(&mut self, result: DeleteResult) {
        match result.outcome {
            DeleteOutcome::Deleted => self.mark_deleted(&result.artifact),
            DeleteOutcome::NotFound => {
                self.fail_row(&result.artifact, "no longer recognized", result.rescan);
            }
            DeleteOutcome::IoFailed(reason) => {
                self.fail_row(&result.artifact, &reason.to_string(), result.rescan);
            }
        }
    }

    pub fn freed_bytes(&self) -> u64 {
        self.freed_bytes
    }

    pub fn view_state(&mut self, height: u16, now: Instant, _wall: SystemTime) -> ViewState {
        let visible = height.saturating_sub(1) as usize;
        self.page_size = visible;
        self.error_page = (MAX_ERROR_ROWS as usize)
            .min(height.saturating_sub(5) as usize)
            .max(1);
        self.error_scroll = self
            .error_scroll
            .min(self.errors.len().saturating_sub(self.error_page));
        let len = self.rows.len();
        let mut start = self.scroll.min(len.saturating_sub(1));
        let selection = self.selected;
        if let Some(selected) = selection {
            if selected < start {
                start = selected;
            } else if selected >= start + visible.max(1) {
                start = selected + 1 - visible.max(1);
            }
        }
        self.scroll = start;
        let end = (start + visible).min(len);
        let window = self.rows[start..end]
            .iter()
            .map(|row| RowView {
                path: row.artifact.path.clone(),
                matcher_id: row.artifact.matcher_id,
                risk: row.artifact.risk,
                bytes: row.bytes,
                last_modified: row.last_modified,
                status: row.status,
            })
            .collect();
        let confirm = self.pending.as_ref().map(|pending| {
            let peer = self
                .rows
                .iter()
                .find(|row| row_matches(row, &pending.artifact));
            ConfirmView {
                path: pending.artifact.path.clone(),
                matcher_id: pending.artifact.matcher_id,
                note: pending.note,
                bytes: peer.and_then(|row| row.bytes),
                last_modified: peer.and_then(|row| row.last_modified),
            }
        });
        ViewState {
            scan_status: self.scan_status(now),
            elapsed_secs: elapsed_secs(self.done, self.started, now),
            done: self.done,
            aborted: self.aborted,
            row_count: len,
            root: self.root.clone(),
            potential: format_size(self.potential_bytes()),
            freed: format_size(self.freed_bytes),
            error_count: self.errors.len(),
            errors: self
                .errors
                .iter()
                .map(|error| ErrorView {
                    path: error.path.clone(),
                    reason: error.reason.clone(),
                })
                .collect(),
            errors_open: self.show_errors,
            error_scroll: self.error_scroll,
            rows: window,
            selection: selection.map(|selected| selected - start),
            confirm,
        }
    }

    fn scan_status(&self, now: Instant) -> String {
        if self.aborted {
            return format!("stopped · {} artifacts", self.rows.len());
        }
        if self.done {
            let elapsed = self
                .finished
                .map(|end| end.duration_since(self.started).as_secs_f32())
                .unwrap_or(0.0);
            return format!("done in {elapsed:.1}s");
        }
        let tick = now.duration_since(self.started).as_millis() / 200 % SPINNER.len() as u128;
        format!("scanning {}", SPINNER[tick as usize])
    }

    fn potential_bytes(&self) -> u64 {
        self.rows
            .iter()
            .filter(|row| !matches!(row.status, RowStatus::Deleted))
            .filter_map(|row| row.bytes)
            .sum()
    }

    fn insert_row(&mut self, artifact: Artifact) {
        if !self.seen.insert(artifact.path.clone()) {
            return;
        }
        if self.selected.is_none() {
            self.selected = Some(0);
        }
        self.rows.push(Row {
            path_hash: hash_path(&artifact.path),
            artifact,
            bytes: None,
            last_modified: None,
            status: RowStatus::Live,
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
        if row.status == RowStatus::Deleted {
            self.freed_bytes += bytes;
        }
        row.bytes = Some(bytes);
        row.last_modified = last_modified;
        self.insert_sorted(row);
    }

    fn mark_deleted(&mut self, artifact: &Artifact) {
        let Some(row) = find_row_mut(&mut self.rows, artifact) else {
            return;
        };
        if row.status == RowStatus::Deleted {
            return;
        }
        self.freed_bytes += row.bytes.unwrap_or(0);
        row.status = RowStatus::Deleted;
    }

    fn fail_row(&mut self, artifact: &Artifact, reason: &str, rescan: Option<SizeReport>) {
        self.errors.push(ScanError {
            path: artifact.path.clone(),
            reason: reason.to_string(),
        });
        let Some(row) = find_row_mut(&mut self.rows, artifact) else {
            return;
        };
        row.status = RowStatus::Failed;
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
        if self.rows.is_empty() {
            self.selected = None;
        } else {
            self.selected = Some(0);
        }
    }
}

fn page_step(page_size: usize) -> usize {
    page_size.saturating_sub(1).max(1)
}

fn elapsed_secs(done: bool, started: Instant, now: Instant) -> u64 {
    if done {
        return 0;
    }
    now.saturating_duration_since(started).as_secs()
}

fn sorts_before(candidate: &Row, bytes: u64, path: &Path) -> bool {
    matches!(candidate.bytes, Some(existing) if existing > bytes || (existing == bytes && candidate.artifact.path.as_path() < path))
}

fn row_matches(row: &Row, artifact: &Artifact) -> bool {
    row.path_hash == hash_path(&artifact.path) && row.artifact.path == artifact.path
}

fn find_row_mut<'rows>(rows: &'rows mut [Row], artifact: &Artifact) -> Option<&'rows mut Row> {
    rows.iter_mut().find(|row| row_matches(row, artifact))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn artifact(matcher_id: &'static str, name: &str) -> Artifact {
        Artifact {
            matcher_id,
            path: PathBuf::from(name),
            risk: None,
        }
    }

    fn sized_app(count: usize) -> App {
        let mut app = App::new(PathBuf::from("/root"), Instant::now());
        for index in 0..count {
            let name = format!("/root/{index:03}");
            app.apply(ScanEvent::Found {
                artifact: artifact("node_modules", &name),
            });
            app.apply(ScanEvent::Sized {
                artifact: artifact("node_modules", &name),
                bytes: (count - index) as u64 * 100,
                last_modified: None,
            });
        }
        app.apply(ScanEvent::Done);
        app
    }

    fn deleted(artifact: Artifact) -> DeleteResult {
        DeleteResult {
            artifact,
            outcome: DeleteOutcome::Deleted,
            rescan: None,
        }
    }

    #[test]
    fn abort_freezes_rows_and_reports_stopped() {
        let mut app = App::new(PathBuf::from("/root"), Instant::now());
        for index in 0..3 {
            let name = format!("/root/{index:03}");
            app.apply(ScanEvent::Found {
                artifact: artifact("node_modules", &name),
            });
            app.apply(ScanEvent::Sized {
                artifact: artifact("node_modules", &name),
                bytes: 100,
                last_modified: None,
            });
        }
        let frozen = app.view_state(10, Instant::now(), SystemTime::UNIX_EPOCH);
        assert_eq!(frozen.row_count, 3);
        app.abort();
        assert!(app.is_done());
        app.apply(ScanEvent::Found {
            artifact: artifact("node_modules", "/root/999"),
        });
        app.apply(ScanEvent::Sized {
            artifact: artifact("node_modules", "/root/999"),
            bytes: 9999,
            last_modified: None,
        });
        app.apply(ScanEvent::Done);
        let state = app.view_state(10, Instant::now(), SystemTime::UNIX_EPOCH);
        assert_eq!(state.row_count, 3);
        assert!(state.done);
        assert_eq!(state.selection, Some(0));
        assert!(state.scan_status.contains("stopped · 3 artifacts"));
        app.abort();
        assert!(app.is_done());
    }

    #[test]
    fn abort_after_done_keeps_done_status() {
        let mut app = sized_app(2);
        let before = app.view_state(10, Instant::now(), SystemTime::UNIX_EPOCH);
        assert!(before.scan_status.contains("done in"));
        app.abort();
        let after = app.view_state(10, Instant::now(), SystemTime::UNIX_EPOCH);
        assert!(after.scan_status.contains("done in"));
    }

    #[test]
    fn window_follows_and_sticks() {
        let mut app = sized_app(10);
        app.move_cursor(8);
        let vs = app.view_state(4, Instant::now(), SystemTime::UNIX_EPOCH);
        assert_eq!(vs.rows[2].path, PathBuf::from("/root/008"));
        assert_eq!(vs.selection, Some(2));
        app.move_cursor(-8);
        let vs = app.view_state(4, Instant::now(), SystemTime::UNIX_EPOCH);
        assert_eq!(vs.rows[0].path, PathBuf::from("/root/000"));
        assert_eq!(vs.selection, Some(0));
        app.move_cursor(1);
        let vs = app.view_state(4, Instant::now(), SystemTime::UNIX_EPOCH);
        assert_eq!(vs.rows[0].path, PathBuf::from("/root/000"));
        assert_eq!(vs.selection, Some(1));
    }

    #[test]
    fn deleted_rows_stay_visible_as_tombstones() {
        let mut app = sized_app(6);
        app.move_cursor(5);
        for index in 3..6 {
            let name = format!("/root/{index:03}");
            app.apply_delete_result(deleted(artifact("node_modules", &name)));
        }
        let marked = app.view_state(10, Instant::now(), SystemTime::UNIX_EPOCH);
        assert_eq!(marked.rows.len(), 6);
        assert_eq!(marked.rows.last().unwrap().path, PathBuf::from("/root/005"));
        for row in marked.rows.iter().take(3) {
            assert_eq!(row.status, RowStatus::Live);
        }
        for row in marked.rows.iter().skip(3) {
            assert_eq!(row.status, RowStatus::Deleted);
        }
        assert_eq!(marked.selection, Some(5));
    }

    #[test]
    fn successful_delete_marks_row_and_credits_freed_space() {
        let mut app = sized_app(3);
        app.move_cursor(1);
        app.apply_delete_result(deleted(artifact("node_modules", "/root/001")));
        let state = app.view_state(10, Instant::now(), SystemTime::UNIX_EPOCH);
        assert_eq!(state.rows.len(), 3);
        assert_eq!(state.rows[1].path, PathBuf::from("/root/001"));
        assert_eq!(state.rows[1].status, RowStatus::Deleted);
        assert_eq!(app.freed_bytes(), 200);
        assert_eq!(state.freed, format_size(200));
        assert_eq!(state.potential, format_size(400));
        assert_eq!(state.selection, Some(1));
        assert!(app.deletion_target().is_none());
        app.move_cursor(-1);
        assert!(app.deletion_target().is_some());
    }

    #[test]
    fn failed_delete_updates_row_in_place_without_reordering() {
        let mut app = sized_app(3);
        app.move_cursor(1);
        app.apply_delete_result(DeleteResult {
            artifact: artifact("node_modules", "/root/001"),
            outcome: DeleteOutcome::NotFound,
            rescan: Some(SizeReport {
                bytes: 50,
                last_modified: None,
                errors: Vec::new(),
            }),
        });
        let state = app.view_state(10, Instant::now(), SystemTime::UNIX_EPOCH);
        assert_eq!(state.rows.len(), 3);
        assert_eq!(state.rows[1].path, PathBuf::from("/root/001"));
        assert_eq!(state.rows[1].status, RowStatus::Failed);
        assert_eq!(state.rows[1].bytes, Some(50));
        assert_eq!(state.rows[0].path, PathBuf::from("/root/000"));
        assert_eq!(state.rows[2].path, PathBuf::from("/root/002"));
        assert_eq!(state.selection, Some(1));
        assert_eq!(state.error_count, 1);
        assert_eq!(state.potential, format_size(450));
        assert!(app.deletion_target().is_some());
        app.mark_deleting(&artifact("node_modules", "/root/001"));
        let retried = app.view_state(10, Instant::now(), SystemTime::UNIX_EPOCH);
        assert_eq!(retried.rows[1].status, RowStatus::Deleting);
    }

    #[test]
    fn selection_pins_and_clamps() {
        let mut app = App::new(PathBuf::from("/root"), Instant::now());
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
        let wall = SystemTime::UNIX_EPOCH;
        app.apply(ScanEvent::Found {
            artifact: artifact("node_modules", "/root/c"),
        });
        app.apply(ScanEvent::Sized {
            artifact: artifact("node_modules", "/root/c"),
            bytes: 1,
            last_modified: None,
        });
        let pinned = app.view_state(10, Instant::now(), wall);
        assert_eq!(pinned.rows[0].path, PathBuf::from("/root/b"));
        app.apply(ScanEvent::Done);
        app.move_cursor(99);
        let bottom = app.view_state(10, Instant::now(), wall);
        assert_eq!(bottom.rows[2].path, PathBuf::from("/root/a"));
        assert_eq!(bottom.selection, Some(2));
        app.move_cursor(-99);
        let top = app.view_state(10, Instant::now(), wall);
        assert_eq!(top.rows[0].path, PathBuf::from("/root/b"));
        assert_eq!(top.selection, Some(0));
    }

    #[test]
    fn confirm_targets_pending_path() {
        let mut app = sized_app(3);
        app.open_confirm(artifact("node_modules", "/root/002"), None);
        app.cancel_confirm();
        let cleared = app.view_state(10, Instant::now(), SystemTime::UNIX_EPOCH);
        assert!(cleared.confirm.is_none());
        assert!(cleared.rows.iter().all(|row| row.status == RowStatus::Live));
        app.open_confirm(artifact("node_modules", "/root/002"), None);
        app.confirm_pending();
        let marked = app.view_state(10, Instant::now(), SystemTime::UNIX_EPOCH);
        assert_eq!(marked.rows[0].status, RowStatus::Live);
        assert_eq!(marked.rows[2].status, RowStatus::Deleting);
        app.apply_delete_result(deleted(artifact("node_modules", "/root/002")));
        app.open_confirm(artifact("node_modules", "/root/002"), None);
        assert!(app.confirm_pending().is_none());
    }
}
