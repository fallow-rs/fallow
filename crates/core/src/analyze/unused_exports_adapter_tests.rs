//! Duplicate-export tests for aliased-import wrappers and star re-export
//! overlays.

use std::path::PathBuf;

use rustc_hash::FxHashMap;

use super::find_duplicate_exports;
use super::tests::{build_duplicate_graph, test_config};
use crate::discover::FileId;
use crate::graph::{ModuleGraph, ReExportEdge};
use crate::results::DuplicateExport;
use crate::suppress::SuppressionContext;

/// Add a declared `export * from` edge for each `(module, source)` pair.
fn add_star_re_exports(graph: &mut ModuleGraph, pairs: &[(usize, u32)]) {
    for &(module, source) in pairs {
        graph.modules[module].re_exports.push(ReExportEdge {
            source_file: FileId(source),
            imported_name: "*".to_string(),
            exported_name: "*".to_string(),
            is_type_only: false,
            span: oxc_span::Span::new(1, 30),
        });
    }
}

fn find(graph: &ModuleGraph) -> Vec<DuplicateExport> {
    let suppressions = SuppressionContext::empty();
    let config = test_config();
    find_duplicate_exports(graph, &config, &suppressions, &FxHashMap::default(), &[])
}

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
fn duplicate_exports_flag_aliased_import_wrapper() {
    let graph = build_duplicate_graph(
        &[
            ("/src/entry.ts", true),
            ("/src/wrapper.ts", false),
            ("/src/core.ts", false),
        ],
        &[(1, "helper", false), (2, "helper", false)],
        &[
            (0, 1, "helper", false),
            (0, 2, "helper", false),
            (1, 2, "helper", false),
        ],
    );
    let result = find(&graph);
    assert_eq!(
        result.len(),
        1,
        "a wrapper that imports `helper` under an alias declares a second `helper`"
    );
    assert_eq!(
        duplicate_location_paths(&result[0]),
        vec![
            PathBuf::from("/src/core.ts"),
            PathBuf::from("/src/wrapper.ts")
        ]
    );
}

#[test]
fn duplicate_exports_skip_star_re_export_overlay() {
    let mut graph = build_duplicate_graph(
        &[
            ("/src/entry.ts", true),
            ("/src/overlay.ts", false),
            ("/src/core.ts", false),
        ],
        &[(1, "helper", false), (2, "helper", false)],
        &[(0, 1, "helper", false), (1, 2, "helper", false)],
    );
    add_star_re_exports(&mut graph, &[(1, 2)]);
    assert!(
        find(&graph).is_empty(),
        "an overlay that star re-exports `core.ts` and replaces `helper` is not an independent origin"
    );
}

#[test]
fn duplicate_exports_flag_wrapper_that_star_re_exports_another_module() {
    let mut graph = build_duplicate_graph(
        &[
            ("/src/entry.ts", true),
            ("/src/wrapper.ts", false),
            ("/src/core.ts", false),
            ("/src/unrelated.ts", false),
        ],
        &[
            (1, "helper", false),
            (2, "helper", false),
            (3, "other", false),
        ],
        &[(0, 1, "helper", false), (1, 2, "helper", false)],
    );
    add_star_re_exports(&mut graph, &[(1, 3)]);
    assert_eq!(
        find(&graph).len(),
        1,
        "a star re-export of a different module does not make the wrapper an overlay"
    );
}

#[test]
fn duplicate_exports_synthesized_star_edge_is_not_an_overlay() {
    let mut graph = build_duplicate_graph(
        &[
            ("/src/entry.ts", true),
            ("/src/wrapper.ts", false),
            ("/src/core.ts", false),
        ],
        &[(1, "helper", false), (2, "helper", false)],
        &[
            (0, 1, "helper", false),
            (0, 2, "helper", false),
            (1, 2, "helper", false),
        ],
    );
    graph.modules[1].re_exports.push(ReExportEdge {
        source_file: FileId(2),
        imported_name: "*".to_string(),
        exported_name: "*".to_string(),
        is_type_only: false,
        span: oxc_span::Span::default(),
    });
    assert_eq!(
        find(&graph).len(),
        1,
        "a star edge without a declaration span does not make the wrapper an overlay"
    );
}

#[test]
fn duplicate_exports_namespace_re_export_is_not_an_overlay() {
    let mut graph = build_duplicate_graph(
        &[
            ("/src/entry.ts", true),
            ("/src/wrapper.ts", false),
            ("/src/core.ts", false),
        ],
        &[(1, "helper", false), (2, "helper", false)],
        &[
            (0, 1, "helper", false),
            (0, 2, "helper", false),
            (1, 2, "helper", false),
        ],
    );
    graph.modules[1].re_exports.push(ReExportEdge {
        source_file: FileId(2),
        imported_name: "*".to_string(),
        exported_name: "core".to_string(),
        is_type_only: false,
        span: oxc_span::Span::new(1, 30),
    });
    assert_eq!(
        find(&graph).len(),
        1,
        "`export * as core from` does not re-export the full surface of `core.ts`"
    );
}

#[test]
fn duplicate_exports_overlay_keeps_independent_pair() {
    let mut graph = build_duplicate_graph(
        &[
            ("/src/entry.ts", true),
            ("/src/overlay.ts", false),
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
    add_star_re_exports(&mut graph, &[(1, 2)]);
    let result = find(&graph);
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
fn duplicate_exports_type_only_star_re_export_overlay() {
    let mut graph = build_duplicate_graph(
        &[
            ("/src/entry.ts", true),
            ("/src/overlay.ts", false),
            ("/src/core.ts", false),
        ],
        &[(1, "Props", true), (2, "Props", true)],
        &[(0, 1, "Props", true), (1, 2, "Props", true)],
    );
    add_star_re_exports(&mut graph, &[(1, 2)]);
    assert!(
        find(&graph).is_empty(),
        "a type overlay that star re-exports `core.ts` is not an independent origin"
    );
}

#[test]
fn duplicate_exports_keep_mutual_star_re_export_overlays() {
    let mut graph = build_duplicate_graph(
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
    add_star_re_exports(&mut graph, &[(1, 2), (2, 1)]);
    let result = find(&graph);
    assert_eq!(
        result.len(),
        1,
        "two modules that overlay each other have no origin to derive from"
    );
    assert_eq!(
        duplicate_location_paths(&result[0]),
        vec![PathBuf::from("/src/a.ts"), PathBuf::from("/src/b.ts")]
    );
}

#[test]
fn duplicate_exports_skip_overlay_cycle_that_reaches_origin() {
    let mut graph = build_duplicate_graph(
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
    add_star_re_exports(&mut graph, &[(1, 2), (2, 1), (2, 3)]);
    assert!(
        find(&graph).is_empty(),
        "overlays whose links reach `core.ts` derive their export from it"
    );
}
