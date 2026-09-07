mod common;

use std::time::{Duration, UNIX_EPOCH};

use filetime::FileTime;
use oweka::engine::{ScanEvent, scan};

#[test]
fn node_modules_is_found_then_sized() {
    let root = tempfile::tempdir().unwrap();
    common::write_bytes(&root.path().join("node_modules/a.js"), 10);
    common::write_bytes(&root.path().join("node_modules/b.js"), 5);
    let events = common::drain(scan(root.path()));
    assert_eq!(events.len(), 3);
    let artifact = root.path().join("node_modules");
    match &events[0] {
        ScanEvent::Found { artifact: found } => assert_eq!(found.path, artifact),
        other => panic!("expected Found first, saw {other:?}"),
    }
    match &events[1] {
        ScanEvent::Sized {
            artifact: sized,
            bytes,
            ..
        } => {
            assert_eq!(sized.path, artifact);
            assert_eq!(*bytes, 15);
        }
        other => panic!("expected Sized second, saw {other:?}"),
    }
    assert!(matches!(events[2], ScanEvent::Done));
}

#[test]
fn newest_mtime_wins() {
    let root = tempfile::tempdir().unwrap();
    let older = root.path().join("node_modules/older.js");
    let newer = root.path().join("node_modules/newer.js");
    common::write_bytes(&older, 4);
    common::write_bytes(&newer, 4);
    filetime::set_file_mtime(&older, FileTime::from_unix_time(1_600_000_000, 0)).unwrap();
    filetime::set_file_mtime(&newer, FileTime::from_unix_time(1_700_000_000, 0)).unwrap();
    let events = common::drain(scan(root.path()));
    let sized = events.iter().find_map(|event| match event {
        ScanEvent::Sized { last_modified, .. } => Some(*last_modified),
        _ => None,
    });
    assert_eq!(
        sized,
        Some(Some(UNIX_EPOCH + Duration::from_secs(1_700_000_000)))
    );
    assert!(common::ends_with_done(&events));
}

#[test]
fn empty_artifact_is_sized_zero() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join("node_modules")).unwrap();
    let events = common::drain(scan(root.path()));
    let sized = events.iter().find_map(|event| match event {
        ScanEvent::Sized { bytes, .. } => Some(*bytes),
        _ => None,
    });
    assert_eq!(sized, Some(0));
    assert_eq!(common::found_paths(&events).len(), 1);
    assert!(common::ends_with_done(&events));
}
