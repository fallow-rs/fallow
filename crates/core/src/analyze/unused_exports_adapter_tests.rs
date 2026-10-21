//! Duplicate-export tests for aliased-import adapters.

use std::path::PathBuf;

use rustc_hash::FxHashMap;

use super::find_duplicate_exports;
use super::tests::{build_duplicate_graph, test_config};
use crate::results::DuplicateExport;
use crate::suppress::SuppressionContext;

#[test]
fn duplicate_exports_direct_import_still_flagged() {
    let graph = build_duplicate_graph(
        &[
            ("/src/entry.ts", true),
            ("/src/a.ts", false),
            ("/src/b.ts", false),
        ],
        &[
            (1, "helper", false),
            (2, "helper", false),
            (2, "other", false),
        ],
        &[(0, 1, "helper", false), (1, 2, "other", false)],
    );
    let suppressions = SuppressionContext::empty();
    let config = test_config();
    let result = find_duplicate_exports(&graph, &config, &suppressions, &FxHashMap::default(), &[]);
    assert_eq!(
        result.len(),
        1,
        "directly connected files should still be flagged"
    );
}

fn duplicate_location_paths(duplicate: &DuplicateExport) -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = duplicate
        .locations
        .iter()
        .map(|location| location.path.clone())
        .collect();
    paths.sort();
    paths
}

#[test]
fn duplicate_exports_skip_aliased_import_wrapper() {
    let graph = build_duplicate_graph(
        &[
            ("/src/entry.ts", true),
            ("/src/adapter.ts", false),
            ("/src/core.ts", false),
        ],
        &[(1, "helper", false), (2, "helper", false)],
        &[(0, 1, "helper", false), (1, 2, "helper", false)],
    );
    let suppressions = SuppressionContext::empty();
    let config = test_config();
    let result = find_duplicate_exports(&graph, &config, &suppressions, &FxHashMap::default(), &[]);
    assert!(
        result.is_empty(),
        "an adapter that imports `helper` under an alias is not an independent origin"
    );
}

#[test]
fn duplicate_exports_wrapper_keeps_independent_pair() {
    let graph = build_duplicate_graph(
        &[
            ("/src/entry.ts", true),
            ("/src/adapter.ts", false),
            ("/src/core.ts", false),
            ("/src/other.ts", false),
        ],
        &[
            (1, "helper", false),
            (2, "helper", false),
            (3, "helper", false),
        ],
        &[
            (0, 1, "helper", false),
            (1, 2, "helper", false),
            (0, 2, "helper", false),
            (0, 3, "helper", false),
        ],
    );
    let suppressions = SuppressionContext::empty();
    let config = test_config();
    let result = find_duplicate_exports(&graph, &config, &suppressions, &FxHashMap::default(), &[]);
    assert_eq!(result.len(), 1);
    assert_eq!(
        duplicate_location_paths(&result[0]),
        vec![
            PathBuf::from("/src/core.ts"),
            PathBuf::from("/src/other.ts")
        ]
    );
}

#[test]
fn duplicate_exports_type_only_aliased_wrapper() {
    let graph = build_duplicate_graph(
        &[
            ("/src/entry.ts", true),
            ("/src/adapter.ts", false),
            ("/src/core.ts", false),
        ],
        &[(1, "Props", true), (2, "Props", true)],
        &[(0, 1, "Props", true), (1, 2, "Props", true)],
    );
    let suppressions = SuppressionContext::empty();
    let config = test_config();
    let result = find_duplicate_exports(&graph, &config, &suppressions, &FxHashMap::default(), &[]);
    assert!(
        result.is_empty(),
        "a type adapter that imports `Props` under an alias is not an independent origin"
    );
}

#[test]
fn duplicate_exports_keep_mutual_aliased_imports() {
    let graph = build_duplicate_graph(
        &[
            ("/src/entry.ts", true),
            ("/src/a.ts", false),
            ("/src/b.ts", false),
        ],
        &[(1, "helper", false), (2, "helper", false)],
        &[
            (0, 1, "helper", false),
            (0, 2, "helper", false),
            (1, 2, "helper", false),
            (2, 1, "helper", false),
        ],
    );
    let suppressions = SuppressionContext::empty();
    let config = test_config();
    let result = find_duplicate_exports(&graph, &config, &suppressions, &FxHashMap::default(), &[]);
    assert_eq!(
        result.len(),
        1,
        "two modules that import `helper` from each other have no origin to derive from"
    );
    assert_eq!(
        duplicate_location_paths(&result[0]),
        vec![PathBuf::from("/src/a.ts"), PathBuf::from("/src/b.ts")]
    );
}

#[test]
fn duplicate_exports_skip_adapter_cycle_that_reaches_origin() {
    let graph = build_duplicate_graph(
        &[
            ("/src/entry.ts", true),
            ("/src/a.ts", false),
            ("/src/b.ts", false),
            ("/src/core.ts", false),
        ],
        &[
            (1, "helper", false),
            (2, "helper", false),
            (3, "helper", false),
        ],
        &[
            (0, 1, "helper", false),
            (0, 2, "helper", false),
            (1, 2, "helper", false),
            (2, 1, "helper", false),
            (2, 3, "helper", false),
        ],
    );
    let suppressions = SuppressionContext::empty();
    let config = test_config();
    let result = find_duplicate_exports(&graph, &config, &suppressions, &FxHashMap::default(), &[]);
    assert!(
        result.is_empty(),
        "adapters whose links reach `core.ts` derive their export from it"
    );
}
