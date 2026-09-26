//! Each graph edge symbol records when its target loads: at import time
//! (static), on demand (`import()` or a pattern), or on another thread or
//! process (workers, forks). The startup weight report reads this kind, so
//! every import form in the fixture must map to exactly one expected kind.

use super::common::{create_config, fixture_path};
use fallow_core::graph::{ModuleGraph, ModuleNode};
use fallow_types::extract::ImportLoadKind;

const FIXTURE: &str = "startup-import-weight";

fn module_named<'a>(modules: &'a [ModuleNode], suffix: &str) -> &'a ModuleNode {
    modules
        .iter()
        .find(|m| {
            m.path
                .to_string_lossy()
                .replace('\\', "/")
                .ends_with(suffix)
        })
        .unwrap_or_else(|| panic!("{suffix} must be part of the module graph"))
}

/// The load kinds on the edge from `from` to `to`, sorted and deduplicated.
fn edge_kinds(graph: &ModuleGraph, from: &str, to: &str) -> Vec<ImportLoadKind> {
    let source = module_named(&graph.modules, from).file_id;
    let target = module_named(&graph.modules, to).file_id;
    let mut kinds: Vec<ImportLoadKind> = graph
        .outgoing_symbol_edges(source)
        .filter(|(edge_target, _)| *edge_target == target)
        .flat_map(|(_, symbols)| symbols.iter().map(|symbol| symbol.load_kind()))
        .collect();
    kinds.sort_unstable();
    kinds.dedup();
    kinds
}

#[test]
fn every_import_form_records_its_load_kind() {
    let output = fallow_core::analyze_with_trace(&create_config(fixture_path(FIXTURE)))
        .expect("analysis should succeed");
    let graph = output
        .graph
        .as_ref()
        .expect("analyze_with_trace retains the graph");

    let cases: [(&str, &str, ImportLoadKind); 14] = [
        ("src/index.ts", "src/heavy/view.ts", ImportLoadKind::Static),
        ("src/index.ts", "src/types.ts", ImportLoadKind::Static),
        ("src/index.ts", "src/styles.css", ImportLoadKind::Static),
        ("src/index.ts", "src/reexported.ts", ImportLoadKind::Static),
        ("src/index.ts", "src/legacy.js", ImportLoadKind::Static),
        ("src/index.ts", "src/lazy.ts", ImportLoadKind::Dynamic),
        (
            "src/index.ts",
            "src/pages/home.ts",
            ImportLoadKind::DynamicPattern,
        ),
        ("src/index.ts", "src/eager/one.ts", ImportLoadKind::Static),
        (
            "src/index.ts",
            "src/lazy-glob/two.ts",
            ImportLoadKind::DynamicPattern,
        ),
        ("src/index.ts", "src/worker.ts", ImportLoadKind::OutOfThread),
        ("src/index.ts", "src/child.js", ImportLoadKind::OutOfThread),
        ("src/lazy.ts", "src/shared.ts", ImportLoadKind::Static),
        (
            "src/worker.ts",
            "src/worker-only.ts",
            ImportLoadKind::Static,
        ),
        (
            "src/heavy/view.ts",
            "src/heavy/chart-data.ts",
            ImportLoadKind::Static,
        ),
    ];

    let failures: Vec<String> = cases
        .iter()
        .filter_map(|&(from, to, expected)| {
            let kinds = edge_kinds(graph, from, to);
            (kinds != [expected])
                .then(|| format!("{from} -> {to}: expected {expected:?}, got {kinds:?}"))
        })
        .collect();
    assert!(
        failures.is_empty(),
        "every import form must map to one load kind:\n{}",
        failures.join("\n")
    );
}
