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
//! - `lsp_save_publish_position_mapping_5000_lines`: the byte-column to
//!   UTF-16 conversion for the diagnostics of one 5,000-line file.
//!
//! Each case also asserts its publish count, which is the side metric of the
//! editor publish work. The second-save cases also assert the parse work: the
//! kept project session loads no config, reads no persisted parse cache, and
//! parses only the changed files.

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use fallow_lsp::bench_support::{SavePublishCounts, SavePublishLab, map_utf16_columns};
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
/// Files that a second save parses after one module changed: that module
/// only. The kept project session serves the other modules from memory.
const ONE_CHANGED_FILE_PARSES: usize = 1;

/// Lines in the position-mapping file.
const MAPPING_LINE_COUNT: usize = 5_000;
/// One diagnostic on each tenth line.
const MAPPING_LINE_STEP: usize = 10;
/// Each fiftieth line has a non-ASCII character before the token.
const MAPPING_NON_ASCII_STEP: usize = 50;

struct MappingInput {
    _temp_dir: TempDir,
    path: PathBuf,
    positions: Vec<(u32, u32)>,
    expected_sum: u64,
}

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

fn create_mapping_input() -> MappingInput {
    let temp_dir = TempDir::new().unwrap();
    let mut content = String::new();
    let mut positions = Vec::new();
    let mut expected_sum = 0;
    for line in 0..MAPPING_LINE_COUNT {
        let prefix = if line % MAPPING_NON_ASCII_STEP == 0 {
            "const label = \"\u{1F389}\"; "
        } else {
            "const label = \"plain\"; "
        };
        if line % MAPPING_LINE_STEP == 0 {
            positions.push((
                u32::try_from(line).unwrap(),
                u32::try_from(prefix.len()).unwrap(),
            ));
            expected_sum += prefix.encode_utf16().count() as u64;
        }
        writeln!(content, "{prefix}export const value{line} = {line};").unwrap();
    }
    let path = temp_dir.path().join("large.ts");
    fs::write(&path, &content).unwrap();
    MappingInput {
        _temp_dir: temp_dir,
        path,
        positions,
        expected_sum,
    }
}

fn lsp_save_publish_position_mapping_5000_lines(c: &mut Criterion) {
    let input = create_mapping_input();
    c.bench_function("lsp_save_publish_position_mapping_5000_lines", |bencher| {
        bencher.iter(|| {
            let sum = map_utf16_columns(&input.path, &input.positions);
            assert_eq!(sum, input.expected_sum);
            sum
        });
    });
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
                assert_eq!(
                    (
                        counts.sessions_loaded,
                        counts.modules_parsed,
                        counts.disk_cache_hits
                    ),
                    (0, ONE_CHANGED_FILE_PARSES, 0)
                );
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
                assert_eq!(
                    (
                        counts.sessions_loaded,
                        counts.modules_parsed,
                        counts.disk_cache_hits
                    ),
                    (0, 0, 0)
                );
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
    lsp_save_publish_noop_save,
    lsp_save_publish_position_mapping_5000_lines
);
criterion_main!(benches);
