mod common;

use oweka::engine::scan;

#[test]
fn node_modules_is_found() {
    let root = tempfile::tempdir().unwrap();
    common::write_bytes(&root.path().join("node_modules/a.js"), 10);
    let events = common::drain(scan(root.path()));
    assert_eq!(
        common::found_paths(&events),
        vec![root.path().join("node_modules")]
    );
    assert!(common::ends_with_done(&events));
}

#[test]
fn target_with_sibling_cargo_toml_is_found() {
    let root = tempfile::tempdir().unwrap();
    common::write_bytes(&root.path().join("proj/Cargo.toml"), 4);
    common::write_bytes(&root.path().join("proj/target/debug/app"), 6);
    let events = common::drain(scan(root.path()));
    assert_eq!(
        common::found_paths(&events),
        vec![root.path().join("proj/target")]
    );
    assert!(common::ends_with_done(&events));
}

#[test]
fn target_without_sibling_cargo_toml_is_not_found() {
    let root = tempfile::tempdir().unwrap();
    common::write_bytes(&root.path().join("proj/target/debug/app"), 6);
    let events = common::drain(scan(root.path()));
    assert!(common::found_paths(&events).is_empty());
    assert!(common::ends_with_done(&events));
}

#[test]
fn nested_artifacts_are_not_descended() {
    let root = tempfile::tempdir().unwrap();
    common::write_bytes(&root.path().join("node_modules/node_modules/inner.js"), 3);
    let events = common::drain(scan(root.path()));
    assert_eq!(
        common::found_paths(&events),
        vec![root.path().join("node_modules")]
    );
    assert!(common::ends_with_done(&events));
}

#[test]
fn git_dir_is_pruned() {
    let root = tempfile::tempdir().unwrap();
    common::write_bytes(&root.path().join(".git/node_modules/packed.js"), 3);
    let events = common::drain(scan(root.path()));
    assert!(common::found_paths(&events).is_empty());
    assert!(common::ends_with_done(&events));
}

#[cfg(unix)]
#[test]
fn symlinked_node_modules_is_skipped() {
    use std::os::unix::fs::symlink;
    let root = tempfile::tempdir().unwrap();
    let real = tempfile::tempdir().unwrap();
    common::write_bytes(&real.path().join("a.js"), 3);
    symlink(real.path(), root.path().join("node_modules")).unwrap();
    let events = common::drain(scan(root.path()));
    assert!(common::found_paths(&events).is_empty());
    assert!(common::ends_with_done(&events));
}
