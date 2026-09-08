mod common;

use std::time::{Duration, Instant};

use oweka::engine::{ScanEvent, scan};

#[test]
fn abort_yields_done_and_closes() {
    let root = tempfile::tempdir().unwrap();
    for index in 0..2000 {
        common::write_bytes(
            &root.path().join(format!("dir{index:04}/node_modules/a.js")),
            10,
        );
    }
    let (receiver, handle) = scan(root.path());
    handle.abort();
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut done_count = 0;
    let mut found_count = 0;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            panic!("timed out waiting for Done");
        }
        match receiver.recv_timeout(remaining) {
            Ok(ScanEvent::Done) => {
                done_count += 1;
                break;
            }
            Ok(ScanEvent::Found { .. }) => {
                found_count += 1;
                continue;
            }
            Ok(_) => continue,
            Err(_) => break,
        }
    }
    assert_eq!(done_count, 1);
    assert!(found_count < 2000);
    match receiver.recv_timeout(Duration::from_millis(500)) {
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {}
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => panic!("channel still open after Done"),
        Ok(event) => panic!("event after Done: {event:?}"),
    }
    handle.abort();
}
