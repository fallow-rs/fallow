//! Entry-point discovery benchmark.
//!
//! Discovery is a serial, uncached stage between the parse phase and import
//! resolution, and it was the only pipeline stage with no benchmark at all.
//! Its cost is driven by filesystem probing, not by source size: every
//! `package.json` entry field is canonicalized and probed against the source
//! extensions until one exists. The fixture is therefore shaped like a
//! monorepo (many small packages, each with entry fields and scripts) rather
//! than like a few large files.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests and benches use unwrap and expect to keep fixture setup concise"
)]
#![allow(
    clippy::significant_drop_tightening,
    reason = "the external Criterion macro owns the benchmark lifecycle"
)]

use std::path::{Path, PathBuf};

use criterion::{Criterion, criterion_group, criterion_main};
use tempfile::TempDir;

mod helpers;

/// A monorepo small enough that discovery should stay under the noise floor.
const SMALL_PACKAGE_COUNT: usize = 64;
/// A monorepo at the size where discovery starts to dominate a warm run.
const LARGE_PACKAGE_COUNT: usize = 256;
/// Source files written per package.
const FILES_PER_PACKAGE: usize = 6;
/// A narrow plugin surface: a handful of frameworks contributing entry
/// patterns.
const FEW_PLUGIN_PATTERNS: usize = 32;
/// A broad plugin surface, the shape a real project with many detected
/// frameworks produces.
const MANY_PLUGIN_PATTERNS: usize = 128;

struct MonorepoFixture {
    _temp_dir: TempDir,
    config: fallow_config::ResolvedConfig,
    files: Vec<fallow_core::discover::DiscoveredFile>,
    workspace_roots: Vec<PathBuf>,
}

fn write_file(path: &Path, source: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, source).unwrap();
}

/// Build a monorepo whose packages exercise every filesystem-probing branch of
/// entry resolution: an exact `main`, an extensionless `module` that needs the
/// source-extension fallback, a directory `exports` target that needs the
/// index probe, a `bin` map, and runtime plus support scripts.
fn create_monorepo_fixture(package_count: usize) -> MonorepoFixture {
    let temp_dir = tempfile::Builder::new()
        .prefix("fallow-bench-entry-points-")
        .tempdir()
        .unwrap();
    let root = temp_dir.path().to_path_buf();

    write_file(
        &root.join("package.json"),
        r#"{"name":"bench-monorepo","private":true,"workspaces":["packages/*"],"main":"src/index.ts","scripts":{"build":"node scripts/build.mjs","start":"node src/server.ts"}}"#,
    );
    write_file(&root.join("src/index.ts"), "export const root = 1;\n");
    write_file(&root.join("src/server.ts"), "export const serve = 1;\n");
    write_file(&root.join("scripts/build.mjs"), "export const build = 1;\n");

    let mut workspace_roots = Vec::with_capacity(package_count);
    for index in 0..package_count {
        let pkg_dir = root.join(format!("packages/pkg-{index}"));
        write_file(
            &pkg_dir.join("package.json"),
            &format!(
                r#"{{"name":"@bench/pkg-{index}","main":"./src/index.ts","module":"./src/module","exports":{{".":"./src/index.ts","./sub":"./src/sub"}},"bin":{{"pkg-{index}":"./bin/cli.ts"}},"scripts":{{"start":"node ./src/main.ts","lint":"node ./scripts/lint.mjs"}}}}"#
            ),
        );
        write_file(&pkg_dir.join("src/index.ts"), "export const a = 1;\n");
        write_file(&pkg_dir.join("src/module.ts"), "export const b = 2;\n");
        write_file(&pkg_dir.join("src/sub/index.ts"), "export const c = 3;\n");
        write_file(&pkg_dir.join("src/main.ts"), "export const d = 4;\n");
        write_file(&pkg_dir.join("bin/cli.ts"), "export const cli = 5;\n");
        write_file(
            &pkg_dir.join("scripts/lint.mjs"),
            "export const lint = 6;\n",
        );
        for file in 0..FILES_PER_PACKAGE {
            write_file(
                &pkg_dir.join(format!("src/unit{file}.ts")),
                &format!("export const unit{file} = {file};\n"),
            );
        }
        workspace_roots.push(pkg_dir);
    }

    let config = helpers::make_config(root, true);
    let files = fallow_core::discover::discover_files(&config);
    MonorepoFixture {
        _temp_dir: temp_dir,
        config,
        files,
        workspace_roots,
    }
}

/// Root-package discovery: manual entry globs, root `package.json` fields, and
/// the nested `package.json` scan under the conventional monorepo directories.
fn root_discovery(fixture: &MonorepoFixture) -> usize {
    fallow_core::discover::discover_entry_points(&fixture.config, &fixture.files).len()
}

/// Per-workspace discovery over every package the fixture declares. The
/// pipeline fans these across rayon workers; the benchmark runs them serially
/// so the measurement reflects the work, not the machine's core count.
fn workspace_discovery(fixture: &MonorepoFixture) -> usize {
    fixture
        .workspace_roots
        .iter()
        .map(|ws_root| {
            fallow_core::discover::discover_workspace_entry_points(
                ws_root,
                &fixture.config,
                &fixture.files,
            )
            .len()
        })
        .sum()
}

fn entry_point_discovery_root(c: &mut Criterion) {
    let small = create_monorepo_fixture(SMALL_PACKAGE_COUNT);
    c.bench_function("entry_point_discovery_root_64_packages", |bencher| {
        bencher.iter(|| root_discovery(&small));
    });

    let large = create_monorepo_fixture(LARGE_PACKAGE_COUNT);
    c.bench_function("entry_point_discovery_root_256_packages", |bencher| {
        bencher.iter(|| root_discovery(&large));
    });
}

fn entry_point_discovery_workspaces(c: &mut Criterion) {
    let small = create_monorepo_fixture(SMALL_PACKAGE_COUNT);
    c.bench_function("entry_point_discovery_workspaces_64_packages", |bencher| {
        bencher.iter(|| workspace_discovery(&small));
    });

    let large = create_monorepo_fixture(LARGE_PACKAGE_COUNT);
    c.bench_function("entry_point_discovery_workspaces_256_packages", |bencher| {
        bencher.iter(|| workspace_discovery(&large));
    });
}

/// Plugin entry patterns compiled into a glob set and matched against every
/// discovered file.
///
/// This is the half of the discovery stage that measurement showed dominates
/// it on real projects, and it is the half that does no filesystem work at
/// all: it scales with active pattern count times file count, not with how
/// many `package.json` entry fields have to be probed. The two pattern counts
/// bracket a narrow and a broad plugin surface so a regression in either the
/// compile or the match half shows up.
fn plugin_result_with_patterns(
    pattern_count: usize,
) -> fallow_core::plugins::AggregatedPluginResult {
    let shapes = [
        "src/pages/**/*.ts",
        "app/**/route.ts",
        "src/routes/**/index.ts",
        "packages/*/src/index.ts",
    ];
    let entry_patterns = (0..pattern_count)
        .map(|index| {
            let shape = shapes[index % shapes.len()];
            let rule = fallow_core::plugins::PathRule {
                pattern: format!("{}{shape}", "sub".repeat(index / shapes.len())),
                exclude_globs: Vec::new(),
                exclude_regexes: Vec::new(),
                exclude_segment_regexes: Vec::new(),
                parent_relative: false,
            };
            (rule, format!("bench-plugin-{index}"))
        })
        .collect();
    fallow_core::plugins::AggregatedPluginResult {
        entry_patterns,
        ..fallow_core::plugins::AggregatedPluginResult::default()
    }
}

fn plugin_glob_discovery(
    fixture: &MonorepoFixture,
    plugin_result: &fallow_core::plugins::AggregatedPluginResult,
) -> usize {
    fallow_core::discover::discover_plugin_entry_points(
        plugin_result,
        &fixture.config,
        &fixture.files,
    )
    .len()
}

fn entry_point_discovery_plugin_globs(c: &mut Criterion) {
    let fixture = create_monorepo_fixture(LARGE_PACKAGE_COUNT);

    let few = plugin_result_with_patterns(FEW_PLUGIN_PATTERNS);
    c.bench_function(
        "entry_point_discovery_plugin_globs_32_patterns",
        |bencher| {
            bencher.iter(|| plugin_glob_discovery(&fixture, &few));
        },
    );

    let many = plugin_result_with_patterns(MANY_PLUGIN_PATTERNS);
    c.bench_function(
        "entry_point_discovery_plugin_globs_128_patterns",
        |bencher| {
            bencher.iter(|| plugin_glob_discovery(&fixture, &many));
        },
    );
}

criterion_group!(
    benches,
    entry_point_discovery_root,
    entry_point_discovery_workspaces,
    entry_point_discovery_plugin_globs
);
criterion_main!(benches);
