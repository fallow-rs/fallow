//! `circularDependencies.ignoreLazyImports` (issue #2936) drops the import
//! edges that load their target on demand or on another thread from the cycle
//! graph. Eager edges stay: static imports, a top-level `await import()`, and
//! an edge that also carries a static import.

use super::common::{create_config, fixture_path};
use fallow_core::graph::ModuleNode;
use fallow_types::extract::ImportLoadKind;

const FIXTURE: &str = "cycles-ignore-lazy-imports";

/// Each reported cycle as its sorted list of fixture-relative paths.
fn cycle_paths(ignore_lazy_imports: bool) -> Vec<Vec<String>> {
    let root = fixture_path(FIXTURE);
    let mut config = create_config(root.clone());
    config.circular_dependencies.ignore_lazy_imports = ignore_lazy_imports;
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let mut cycles: Vec<Vec<String>> = results
        .circular_dependencies
        .iter()
        .map(|finding| {
            let mut files: Vec<String> = finding
                .cycle
                .files
                .iter()
                .map(|path| {
                    path.strip_prefix(&root)
                        .unwrap_or(path)
                        .to_string_lossy()
                        .replace('\\', "/")
                })
                .collect();
            files.sort();
            files
        })
        .collect();
    cycles.sort();
    cycles
}

fn has_cycle(cycles: &[Vec<String>], files: &[&str]) -> bool {
    cycles.iter().any(|cycle| cycle == files)
}

#[test]
fn default_reports_lazy_cycles_unchanged() {
    let cycles = cycle_paths(false);
    for pair in [
        ["src/dynamic/a.ts", "src/dynamic/b.ts"],
        ["src/static/a.ts", "src/static/b.ts"],
        ["src/mixed/a.ts", "src/mixed/b.ts"],
        ["src/toplevel/a.ts", "src/toplevel/b.ts"],
        ["src/pattern/a.ts", "src/pattern/b.ts"],
        ["src/worker/a.ts", "src/worker/b.ts"],
    ] {
        assert!(
            has_cycle(&cycles, &pair),
            "expected {pair:?} in {cycles:#?}"
        );
    }
    assert!(
        !cycles
            .iter()
            .any(|cycle| cycle[0].starts_with("src/typeonly/")),
        "type-only edges never form a cycle: {cycles:#?}"
    );
}

#[test]
fn ignore_lazy_imports_keeps_only_eager_cycles() {
    let cycles = cycle_paths(true);
    let expected: Vec<Vec<String>> = [
        vec!["src/cap/hub.ts", "src/cap/p.ts", "src/cap/q.ts"],
        vec!["src/mixed/a.ts", "src/mixed/b.ts"],
        vec!["src/static/a.ts", "src/static/b.ts"],
        vec!["src/toplevel/a.ts", "src/toplevel/b.ts"],
    ]
    .into_iter()
    .map(|cycle| cycle.into_iter().map(str::to_owned).collect())
    .collect();
    assert_eq!(cycles, expected);
}

#[test]
fn lazy_cycles_no_longer_crowd_out_a_static_cycle() {
    let static_cycle = ["src/cap/hub.ts", "src/cap/p.ts", "src/cap/q.ts"];
    let default_cycles = cycle_paths(false);
    let lazy_cap_cycles = default_cycles
        .iter()
        .filter(|cycle| cycle[0].starts_with("src/cap/"))
        .count();
    assert_eq!(
        lazy_cap_cycles, 20,
        "the lazy importers fill the per-SCC cap by default: {default_cycles:#?}"
    );
    assert!(
        !has_cycle(&default_cycles, &static_cycle),
        "by default the lazy cycles hide the static cycle: {default_cycles:#?}"
    );
    assert!(has_cycle(&cycle_paths(true), &static_cycle));
}

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

#[test]
fn top_level_await_import_loads_eagerly() {
    let output = fallow_core::analyze_with_trace(&create_config(fixture_path(FIXTURE)))
        .expect("analysis should succeed");
    let graph = output
        .graph
        .as_ref()
        .expect("analyze_with_trace retains the graph");
    let kinds = |from: &str, to: &str| -> Vec<ImportLoadKind> {
        let source = module_named(&graph.modules, from).file_id;
        let target = module_named(&graph.modules, to).file_id;
        graph
            .outgoing_symbol_edges(source)
            .filter(|(edge_target, _)| *edge_target == target)
            .flat_map(|(_, symbols)| symbols.iter().map(|symbol| symbol.load_kind()))
            .collect()
    };
    assert_eq!(
        kinds("src/toplevel/b.ts", "src/toplevel/a.ts"),
        vec![ImportLoadKind::Static]
    );
    assert_eq!(
        kinds("src/dynamic/b.ts", "src/dynamic/a.ts"),
        vec![ImportLoadKind::Dynamic]
    );
}

#[test]
fn mixed_edge_anchors_on_the_static_import() {
    let root = fixture_path(FIXTURE);
    let mut config = create_config(root.clone());
    config.circular_dependencies.ignore_lazy_imports = true;
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let mixed_b = root.join("src/mixed/b.ts");
    let anchor = results
        .circular_dependencies
        .iter()
        .flat_map(|finding| finding.cycle.edges.iter())
        .find(|edge| edge.path == mixed_b)
        .expect("the mixed cycle has an edge from b.ts");
    assert_eq!(anchor.line, 1, "the anchor is the static import line");
}
