mod common;

use std::os::unix::fs::PermissionsExt;
use std::time::{Duration, UNIX_EPOCH};

use filetime::FileTime;
use oweka::engine::{DeleteOutcome, delete_artifact, size_artifact};
use oweka::matcher::Artifact;

fn node_modules_artifact(root: &std::path::Path) -> Artifact {
    Artifact {
        matcher_id: "node_modules",
        path: root.join("node_modules"),
    }
}

#[test]
fn delete_removes_artifact_from_disk() {
    let root = tempfile::tempdir().unwrap();
    common::write_bytes(&root.path().join("node_modules/a.js"), 10);
    let artifact = node_modules_artifact(root.path());
    match delete_artifact(&artifact) {
        DeleteOutcome::Deleted => {}
        other => panic!("expected Deleted, saw {other:?}"),
    }
    assert!(!artifact.path.exists());
}

#[test]
fn delete_after_rename_aborts_safely() {
    let root = tempfile::tempdir().unwrap();
    common::write_bytes(&root.path().join("node_modules/a.js"), 10);
    let stale = node_modules_artifact(root.path());
    std::fs::rename(&stale.path, root.path().join("renamed")).unwrap();
    match delete_artifact(&stale) {
        DeleteOutcome::NotFound => {}
        other => panic!("expected NotFound, saw {other:?}"),
    }
    assert!(root.path().join("renamed").is_dir());
}

#[cfg(unix)]
#[test]
fn delete_under_read_only_parent_reports_io_failure() {
    let root = tempfile::tempdir().unwrap();
    let parent = root.path().join("proj");
    common::write_bytes(&parent.join("node_modules/a.js"), 10);
    std::fs::set_permissions(&parent, std::fs::Permissions::from_mode(0o555)).unwrap();
    let artifact = Artifact {
        matcher_id: "node_modules",
        path: parent.join("node_modules"),
    };
    match delete_artifact(&artifact) {
        DeleteOutcome::IoFailed(_) => {}
        other => {
            std::fs::set_permissions(&parent, std::fs::Permissions::from_mode(0o755)).unwrap();
            panic!("expected IoFailed, saw {other:?}");
        }
    }
    std::fs::set_permissions(&parent, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(artifact.path.is_dir());
}

#[test]
fn delete_works_when_artifact_is_scan_root() {
    let parent = tempfile::tempdir().unwrap();
    let root = parent.path().join("node_modules");
    common::write_bytes(&root.join("a.js"), 10);
    let artifact = Artifact {
        matcher_id: "node_modules",
        path: root.clone(),
    };
    match delete_artifact(&artifact) {
        DeleteOutcome::Deleted => {}
        other => panic!("expected Deleted, saw {other:?}"),
    }
    assert!(!root.exists());
}

#[test]
fn size_artifact_reports_bytes_and_newest_mtime() {
    let root = tempfile::tempdir().unwrap();
    let older = root.path().join("node_modules/older.js");
    let newer = root.path().join("node_modules/newer.js");
    common::write_bytes(&older, 10);
    common::write_bytes(&newer, 5);
    filetime::set_file_mtime(&older, FileTime::from_unix_time(1_600_000_000, 0)).unwrap();
    filetime::set_file_mtime(&newer, FileTime::from_unix_time(1_700_000_000, 0)).unwrap();
    let report = size_artifact(&node_modules_artifact(root.path()));
    assert_eq!(report.bytes, 15);
    assert_eq!(
        report.last_modified,
        Some(UNIX_EPOCH + Duration::from_secs(1_700_000_000))
    );
    assert!(report.errors.is_empty());
}
