#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests and benches use unwrap and expect to keep fixture setup concise"
)]
#![allow(
    clippy::significant_drop_tightening,
    reason = "the external Criterion macro owns the benchmark lifecycle"
)]

use std::fmt::Write as _;
use std::path::PathBuf;

use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use fallow_config::{BoundaryConfig, FallowConfig, OutputFormat};
use tempfile::TempDir;

struct DupesInput {
    _temp_dir: TempDir,
    root: PathBuf,
    files: Vec<fallow_engine::discover::DiscoveredFile>,
    config: fallow_config::DuplicatesConfig,
}

fn make_config(root: PathBuf) -> fallow_config::ResolvedConfig {
    FallowConfig {
        type_aware: fallow_config::TypeAwareConfig::default(),
        schema: None,
        minimum_version: None,
        extends: vec![],
        entry: vec![],
        ignore_patterns: vec![],
        ignore_findings: vec![],
        framework: vec![],
        workspaces: None,
        ignore_dependencies: vec![],
        ignore_unresolved_imports: vec![],
        ignore_exports: vec![],
        ignore_catalog_references: vec![],
        ignore_dependency_overrides: vec![],
        ignore_exports_used_in_file: fallow_config::IgnoreExportsUsedInFileConfig::default(),
        used_class_members: vec![],
        ignore_decorators: vec![],
        unused_component_props: fallow_config::UnusedComponentPropsConfig::default(),
        duplicates: fallow_config::DuplicatesConfig::default(),
        similar_code: fallow_config::SimilarCodeConfig::default(),
        health: fallow_config::HealthConfig::default(),
        rules: fallow_config::RulesConfig::default(),
        boundaries: BoundaryConfig::default(),
        production: false.into(),
        plugins: vec![],
        rule_packs: vec![],
        dynamically_loaded: vec![],
        overrides: vec![],
        regression: None,
        audit: fallow_config::AuditConfig::default(),
        codeowners: None,
        public_packages: vec![],
        flags: fallow_config::FlagsConfig::default(),
        security: fallow_config::SecurityConfig::default(),
        fix: fallow_config::FixConfig::default(),
        resolve: fallow_config::ResolveConfig::default(),
        sealed: false,
        include_entry_exports: false,
        auto_imports: false,
        cache: fallow_config::CacheConfig::default(),
    }
    .resolve(root, OutputFormat::Human, 4, true, true, None)
}

/// Generate a synthetic project with duplicated code blocks for dupe detection benchmarks.
/// ~40% of files contain shared code blocks (each ~30 lines), rest is unique.
/// # Panics
///
/// Panics if temporary directory creation or file writes fail.
fn create_dupe_project(name: &str, file_count: usize) -> (TempDir, fallow_config::ResolvedConfig) {
    let temp_dir = tempfile::Builder::new()
        .prefix(&format!("fallow-bench-dupes-{name}-"))
        .tempdir()
        .unwrap();
    let root = temp_dir.path().to_path_buf();
    std::fs::create_dir_all(root.join("src")).unwrap();

    std::fs::write(
        root.join("package.json"),
        r#"{"name": "bench-dupes", "main": "src/index.ts"}"#,
    )
    .unwrap();

    let dupe_groups = file_count / 25;
    let blocks: Vec<String> = (0..dupe_groups)
        .map(|g| {
            let mut block = String::new();
            writeln!(
                &mut block,
                "export const processData_{g} = (input: string): Record<string, unknown> => {{"
            )
            .unwrap();
            block.push_str("  const result: Record<string, unknown> = {};\n");
            block.push_str("  const timestamp = Date.now();\n");
            writeln!(&mut block, "  const id = `item_${{timestamp}}_{g}`;").unwrap();
            block.push_str("  if (!input) {\n");
            writeln!(
                &mut block,
                "    throw new Error('Input is required for group {g}');"
            )
            .unwrap();
            block.push_str("  }\n");
            block.push_str("  result.id = id;\n");
            block.push_str("  result.status = 'active';\n");
            block.push_str("  result.createdAt = new Date(timestamp).toISOString();\n");
            block.push_str("  result.updatedAt = new Date(timestamp).toISOString();\n");
            for line in 0..18 {
                writeln!(
                    &mut block,
                    "  result.field_{line} = String(input).slice(0, {});",
                    10 + line * 3
                )
                .unwrap();
            }
            block.push_str("  return result;\n};\n");
            block
        })
        .collect();

    let dupe_file_count = file_count * 2 / 5;
    for i in 0..file_count {
        let mut content = String::new();
        writeln!(
            &mut content,
            "export const unique_{i} = (v: string): string => `${{v}}_{i}`;\n"
        )
        .unwrap();
        if i < dupe_file_count && !blocks.is_empty() {
            let group = i % blocks.len();
            content.push_str(&blocks[group]);
            content.push('\n');
        }
        writeln!(&mut content, "export const helper_{i} = {i};").unwrap();
        std::fs::write(root.join(format!("src/module{i}.ts")), content).unwrap();
    }

    std::fs::write(root.join("src/index.ts"), "export const main = true;\n").unwrap();

    let config = make_config(root);
    (temp_dir, config)
}

fn create_dupes_input(name: &str, file_count: usize) -> DupesInput {
    let (temp_dir, resolved_config) = create_dupe_project(name, file_count);
    let files = fallow_core::discover::discover_files(&resolved_config);
    DupesInput {
        _temp_dir: temp_dir,
        root: resolved_config.root,
        files,
        config: fallow_config::DuplicatesConfig::default(),
    }
}

fn bench_dupes_pipeline(c: &mut Criterion) {
    let mut group = c.benchmark_group("dupes_pipeline");

    group.bench_function("dupes_full_pipeline_1000_files", |bencher| {
        bencher.iter_batched_ref(
            || create_dupes_input("1000", 1000),
            |input| {
                fallow_engine::duplicates::find_duplicates(&input.root, &input.files, &input.config)
            },
            BatchSize::LargeInput,
        );
    });

    group.bench_function("dupes_full_pipeline_5000_files", |bencher| {
        bencher.iter_batched_ref(
            || create_dupes_input("5000", 5000),
            |input| {
                fallow_engine::duplicates::find_duplicates(&input.root, &input.files, &input.config)
            },
            BatchSize::LargeInput,
        );
    });

    group.finish();
}

criterion_group!(benches, bench_dupes_pipeline);
criterion_main!(benches);
