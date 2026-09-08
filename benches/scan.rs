use std::hint::black_box;
use std::path::{Path, PathBuf};
use std::time::Duration;

use criterion::{Criterion, criterion_group, criterion_main};
use oweka::engine::{ScanEvent, scan};

fn bench_root() -> PathBuf {
    std::env::var("OWEKA_BENCH_ROOT")
        .map(PathBuf::from)
        .ok()
        .or_else(dirs::home_dir)
        .expect("OWEKA_BENCH_ROOT or HOME must be set")
}

fn drained_events(root: &Path) -> usize {
    scan(root)
        .0
        .into_iter()
        .filter(|event| !matches!(event, ScanEvent::Done))
        .count()
}

fn bench_scan(c: &mut Criterion) {
    let root = bench_root();
    c.bench_function("scan", |b| {
        b.iter(|| black_box(drained_events(black_box(&root))))
    });
}

criterion_group! {
    name = benches;
    config = Criterion::default()
        .sample_size(10)
        .warm_up_time(Duration::from_secs(1));
    targets = bench_scan
}
criterion_main!(benches);
