//! Benchmarks for the vault-wide operations that run in parallel: content /
//! tag search and tag rename. Run with `cargo bench`.

use std::fs;
use std::hint::black_box;

use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use obsidian_mcp_rs::vault::{SearchType, VaultManager};
use tempfile::TempDir;

/// Build a temp vault of `n` notes spread across subfolders. Each note carries
/// a frontmatter `alpha` tag and one line containing the `needle` token.
fn build_vault(n: usize) -> (TempDir, VaultManager, String) {
    let dir = TempDir::new().unwrap();
    for i in 0..n {
        let sub = dir.path().join(format!("folder{}", i % 16));
        fs::create_dir_all(&sub).unwrap();
        let mut body = String::with_capacity(1536);
        body.push_str("---\ntags:\n  - alpha\n  - project\n---\n");
        for line in 0..40 {
            if line == 20 {
                body.push_str("this line contains the needle token\n");
            } else {
                body.push_str("lorem ipsum dolor sit amet consectetur adipiscing\n");
            }
        }
        fs::write(sub.join(format!("note{i}.md")), body).unwrap();
    }
    let name = dir
        .path()
        .file_name()
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();
    let manager = VaultManager::new(vec![dir.path().to_path_buf()]);
    (dir, manager, name)
}

fn bench_search(c: &mut Criterion) {
    let (_dir, vault, name) = build_vault(2000);

    let mut group = c.benchmark_group("search_2000_files");
    group.bench_function("content", |b| {
        b.iter(|| {
            black_box(
                vault
                    .search_vault(&name, "needle", None, false, &SearchType::Content)
                    .unwrap(),
            )
        })
    });
    group.bench_function("tag", |b| {
        b.iter(|| {
            black_box(
                vault
                    .search_vault(&name, "tag:alpha", None, false, &SearchType::Content)
                    .unwrap(),
            )
        })
    });
    group.finish();
}

fn bench_rename_tag(c: &mut Criterion) {
    let mut group = c.benchmark_group("rename_tag_500_files");
    group.sample_size(10);
    group.bench_function("rename", |b| {
        b.iter_batched(
            || build_vault(500),
            |(_dir, vault, name)| black_box(vault.rename_tag(&name, "alpha", "beta").unwrap()),
            BatchSize::PerIteration,
        )
    });
    group.finish();
}

criterion_group!(benches, bench_search, bench_rename_tag);
criterion_main!(benches);
