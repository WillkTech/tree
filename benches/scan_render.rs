// Scan+render benchmarks for tree mode (the default; folders only).
//
// This is a bin-only crate, so these spawn the binary end-to-end (process
// spawn adds ~1-3 ms). The targets are end-to-end too: 10k files <= 60 ms,
// 100k files <= 200 ms. Corpora come from scripts/make_corpus.sh; the scan
// (rayon walk) dominates at 100k files, the Rust render pass is noise.
//
// Skips silently when a corpus is absent (CI has no corpora).

use criterion::{Criterion, criterion_group, criterion_main};
use std::path::Path;
use std::process::Command;

fn corpora_root() -> String {
    std::env::var("TREE_CORPORA").unwrap_or_else(|_| "/tmp/treecorp".to_string())
}

fn bench_tree(c: &mut Criterion) {
    let bin = env!("CARGO_BIN_EXE_tree");
    let root = corpora_root();

    for (label, depth) in [("medium", "2"), ("large", "2"), ("large", "3")] {
        let dir = format!("{root}/{label}");
        if !Path::new(&dir).exists() {
            eprintln!("skipping {dir} (run scripts/make_corpus.sh)");
            continue;
        }
        for sort in ["size", "name"] {
            c.bench_function(&format!("tree {label} depth{depth} {sort}"), |b| {
                b.iter(|| {
                    let out = Command::new(bin)
                        .args(["--style", "tree", "--sort", sort, "--depth", depth, &dir])
                        .output()
                        .unwrap();
                    assert!(out.status.success());
                    out
                });
            });
        }
    }
}

criterion_group!(benches, bench_tree);
criterion_main!(benches);
