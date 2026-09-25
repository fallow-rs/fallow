#![expect(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "benches use unwrap and expect to keep fixture setup concise"
)]
#![allow(
    clippy::significant_drop_tightening,
    reason = "the external Criterion macro owns the benchmark lifecycle"
)]

//! LSP save-to-publish lab shard.
//!
//! Each case measures the work of one editor save in the language server:
//! the project analysis, the diagnostic build, and the publish plan. The lab
//! keeps the pull cache and the previous URI set across saves, as the server
//! does. It models a push client with no open documents, so each planned
//! publish is one `textDocument/publishDiagnostics` message.
//!
//! - `lsp_save_publish_cold_first_run`: the first save on a project with no
//!   disk cache.
//! - `lsp_save_publish_one_changed_file`: a second save after one file got a
//!   new unused export.
//! - `lsp_save_publish_noop_save`: a second save with no change on disk.
//!
//! Each case also asserts its publish count, which is the side metric of the
//! editor publish work.

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use fallow_lsp::bench_support::{SavePublishCounts, SavePublishLab};
use tempfile::TempDir;

#[path = "support/threads.rs"]
mod threads;

use threads::bench_threads;

/// Modules imported by the entry. Each one has one used and one unused
/// export, so each module file gets one diagnostic.
const MODULE_COUNT: usize = 48;
/// The module that the one-changed-file case edits.
const CHANGED_MODULE: usize = 7;
/// Publishes of the first save: one per module with an unused export.
const FIRST_SAVE_PUBLISHES: usize = MODULE_COUNT;
/// Publishes of a save after one module got a new unused export: only that
/// module, because the other diagnostics did not change.
const ONE_CHANGED_FILE_PUBLISHES: usize = 1;
/// Publishes of a save with no change on disk.
const NOOP_SAVE_PUBLISHES: usize = 0;

struct LabInput {
    _temp_dir: TempDir,
    root: PathBuf,
    lab: SavePublishLab,
    pool: rayon::ThreadPool,
}

fn write_file(root: &Path, path: &str, source: &str) {
    let path = root.join(path);
    fs::create_dir_all(path.parent().expect("fixture file has parent")).unwrap();
    fs::write(path, source).unwrap();
}

fn module_source(index: usize) -> String {
    format!(
        "export const used{index} = (value: number): number => value + {index};\n\
         export const unused{index} = (value: number): number => value * {index};\n"
    )
}

fn create_lab() -> LabInput {
    let temp_dir = TempDir::new().unwrap();
    let root = temp_dir.path().to_path_buf();
    write_file(
        &root,
        "package.json",
        r#"{"name":"lsp-save-publish","private":true,"main":"src/index.ts"}"#,
    );
    let mut entry = String::new();
    for index in 0..MODULE_COUNT {
        write_file(
            &root,
            &format!("src/modules/module{index}.ts"),
            &module_source(index),
        );
        writeln!(
            entry,
            "import {{ used{index} }} from \"./modules/module{index}\";"
        )
        .unwrap();
    }
    entry.push_str("export const total = [");
    for index in 0..MODULE_COUNT {
        write!(entry, "used{index}(1), ").unwrap();
    }
    entry.push_str("];\n");
    write_file(&root, "src/index.ts", &entry);

    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(bench_threads())
        .build()
        .expect("bench thread pool builds");
    let lab = SavePublishLab::new(&root);
    LabInput {
        _temp_dir: temp_dir,
        root,
        lab,
        pool,
    }
}

fn save(input: &mut LabInput) -> SavePublishCounts {
    let LabInput { lab, pool, .. } = input;
    pool.install(|| lab.save().expect("lab save succeeds"))
}

fn create_saved_lab() -> LabInput {
    let mut input = create_lab();
    let counts = save(&mut input);
    assert_eq!(counts.files_with_diagnostics, MODULE_COUNT);
    assert_eq!(counts.publishes, FIRST_SAVE_PUBLISHES);
    input
}

fn create_lab_with_one_changed_file() -> LabInput {
    let input = create_saved_lab();
    let mut source = module_source(CHANGED_MODULE);
    source.push_str("export const addedUnused = 1;\n");
    write_file(
        &input.root,
        &format!("src/modules/module{CHANGED_MODULE}.ts"),
        &source,
    );
    input
}

fn lsp_save_publish_cold_first_run(c: &mut Criterion) {
    c.bench_function("lsp_save_publish_cold_first_run", |bencher| {
        bencher.iter_batched_ref(
            create_lab,
            |input| {
                let counts = save(input);
                assert_eq!(counts.files_with_diagnostics, MODULE_COUNT);
                assert_eq!(counts.publishes, FIRST_SAVE_PUBLISHES);
                counts
            },
            BatchSize::LargeInput,
        );
    });
}

fn lsp_save_publish_one_changed_file(c: &mut Criterion) {
    c.bench_function("lsp_save_publish_one_changed_file", |bencher| {
        bencher.iter_batched_ref(
            create_lab_with_one_changed_file,
            |input| {
                let counts = save(input);
                assert_eq!(counts.files_with_diagnostics, MODULE_COUNT);
                assert_eq!(counts.publishes, ONE_CHANGED_FILE_PUBLISHES);
                counts
            },
            BatchSize::LargeInput,
        );
    });
}

fn lsp_save_publish_noop_save(c: &mut Criterion) {
    c.bench_function("lsp_save_publish_noop_save", |bencher| {
        bencher.iter_batched_ref(
            create_saved_lab,
            |input| {
                let counts = save(input);
                assert_eq!(counts.files_with_diagnostics, MODULE_COUNT);
                assert_eq!(counts.publishes, NOOP_SAVE_PUBLISHES);
                counts
            },
            BatchSize::LargeInput,
        );
    });
}

criterion_group!(
    benches,
    lsp_save_publish_cold_first_run,
    lsp_save_publish_one_changed_file,
    lsp_save_publish_noop_save
);
criterion_main!(benches);
