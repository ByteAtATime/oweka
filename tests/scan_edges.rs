mod common;

use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use oweka::engine::{ScanEvent, scan};

fn lock(dir: &Path) {
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o000)).unwrap();
}

fn unlock(dir: &Path) {
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o755)).unwrap();
}

fn walk_errors(events: &[ScanEvent]) -> Vec<&ScanEvent> {
    events
        .iter()
        .filter(|event| matches!(event, ScanEvent::WalkError { .. }))
        .collect()
}

#[cfg(unix)]
#[test]
fn permission_error_is_tallied_and_walk_continues() {
    let root = tempfile::tempdir().unwrap();
    let locked = root.path().join("locked");
    std::fs::create_dir_all(&locked).unwrap();
    common::write_bytes(&root.path().join("proj/node_modules/a.js"), 10);
    lock(&locked);
    let events = common::drain(scan(root.path()));
    unlock(&locked);
    assert!(!walk_errors(&events).is_empty());
    assert_eq!(
        common::found_paths(&events),
        vec![root.path().join("proj/node_modules")]
    );
    assert!(common::ends_with_done(&events));
}

#[test]
fn root_as_artifact_is_found_and_sized() {
    let parent = tempfile::tempdir().unwrap();
    let root = parent.path().join("node_modules");
    common::write_bytes(&root.join("a.js"), 10);
    common::write_bytes(&root.join("b.js"), 5);
    let events = common::drain(scan(&root));
    assert_eq!(common::found_paths(&events), vec![root.clone()]);
    let sized = events.iter().find_map(|event| match event {
        ScanEvent::Sized { bytes, .. } => Some(*bytes),
        _ => None,
    });
    assert_eq!(sized, Some(15));
    assert!(common::ends_with_done(&events));
}

#[cfg(unix)]
#[test]
fn unreadable_artifact_lists_zero_with_error() {
    let root = tempfile::tempdir().unwrap();
    let artifact = root.path().join("node_modules");
    common::write_bytes(&artifact.join("a.js"), 10);
    lock(&artifact);
    let events = common::drain(scan(root.path()));
    unlock(&artifact);
    let errors = walk_errors(&events);
    assert!(errors.iter().any(|event| match event {
        ScanEvent::WalkError { path, .. } => path == &artifact,
        _ => false,
    }));
    let sized = events.iter().find_map(|event| match event {
        ScanEvent::Sized { bytes, .. } => Some(*bytes),
        _ => None,
    });
    assert_eq!(sized, Some(0));
    assert!(common::ends_with_done(&events));
}

#[test]
fn missing_root_exits_one() {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_oweka"))
        .arg("/nonexistent-oweka-root-xyz")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(!output.stderr.is_empty());
}

#[test]
fn file_as_root_exits_one() {
    let root = tempfile::tempdir().unwrap();
    let file = root.path().join("plain.txt");
    common::write_bytes(&file, 3);
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_oweka"))
        .arg(&file)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(!output.stderr.is_empty());
}
