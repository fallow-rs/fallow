//! The typed filters select the same dependency kinds as `--unused-deps` and
//! `--misplaced-deps`.

#![expect(
    clippy::expect_used,
    reason = "tests use expect to keep fixture setup concise"
)]

use std::fs;
use std::path::Path;

use fallow_api::{AnalysisOptions, DeadCodeFilters, DeadCodeOptions, run_dead_code};

fn write(root: &Path, path: &str, content: &str) {
    let target = root.join(path);
    fs::create_dir_all(target.parent().expect("file has a parent")).expect("create directory");
    fs::write(target, content).expect("write file");
}

/// `unused-prod` is unused, `left-pad` is a dev dependency in production code,
/// and `test-only` is a production dependency that only a test file imports.
fn project() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    write(
        root,
        "package.json",
        r#"{"name":"api-deps","private":true,"type":"module","main":"src/index.ts","dependencies":{"unused-prod":"1.0.0","test-only":"1.0.0"},"devDependencies":{"left-pad":"1.3.0","vitest":"1.0.0"}}"#,
    );
    write(
        root,
        "src/index.ts",
        "import leftPad from 'left-pad';\n\nexport const label = leftPad('x', 4);\nconsole.log(label);\n",
    );
    write(
        root,
        "src/index.test.ts",
        "import { test } from 'vitest';\nimport helper from 'test-only';\n\ntest('label', () => helper());\n",
    );
    dir
}

/// The names of the unused kinds, then the names of the placement kinds.
fn names(root: &Path, filters: DeadCodeFilters) -> (Vec<String>, Vec<String>) {
    let results = run_dead_code(&DeadCodeOptions {
        analysis: AnalysisOptions {
            root: Some(root.to_path_buf()),
            no_cache: true,
            ..AnalysisOptions::default()
        },
        filters,
        ..DeadCodeOptions::default()
    })
    .expect("analysis runs");
    let results = results.results();
    let unused = results
        .unused_dependencies
        .iter()
        .map(|item| item.dep.package_name.clone())
        .collect();
    let mut placement: Vec<String> = results
        .test_only_dependencies
        .iter()
        .map(|item| item.dep.package_name.clone())
        .chain(
            results
                .dev_dependencies_in_production
                .iter()
                .map(|item| item.dep.package_name.clone()),
        )
        .collect();
    placement.sort();
    (unused, placement)
}

fn placement() -> Vec<String> {
    vec!["left-pad".to_owned(), "test-only".to_owned()]
}

#[test]
fn unused_deps_alone_keeps_only_the_unused_kinds() {
    let dir = project();
    let (unused, placed) = names(
        dir.path(),
        DeadCodeFilters {
            unused_deps: true,
            ..DeadCodeFilters::default()
        },
    );
    assert_eq!(unused, ["unused-prod"]);
    assert!(placed.is_empty(), "{placed:?}");
}

#[test]
fn misplaced_deps_alone_keeps_only_the_placement_kinds() {
    let dir = project();
    let (unused, placed) = names(
        dir.path(),
        DeadCodeFilters {
            misplaced_deps: true,
            ..DeadCodeFilters::default()
        },
    );
    assert!(unused.is_empty(), "{unused:?}");
    assert_eq!(placed, placement());
}

#[test]
fn both_filters_keep_the_full_dependency_set() {
    let dir = project();
    let (unused, placed) = names(
        dir.path(),
        DeadCodeFilters {
            unused_deps: true,
            misplaced_deps: true,
            ..DeadCodeFilters::default()
        },
    );
    assert_eq!(unused, ["unused-prod"]);
    assert_eq!(placed, placement());
}

#[test]
fn the_registry_selector_enables_the_matching_filter() {
    let mut filters = DeadCodeFilters::default();
    assert!(filters.enable_registry_selector("misplaced-deps"));
    assert!(filters.misplaced_deps);
    assert!(!filters.unused_deps);
}
