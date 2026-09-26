//! The load closure of an entry splits the modules it reaches by when they
//! load, and the dominating imports name the single import edges that keep a
//! subtree of the eager closure on the startup path.

use super::common::{create_config, fixture_path};
use fallow_core::graph::{ModuleGraph, ModuleNode};
use fallow_types::discover::FileId;

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

fn relative(graph: &ModuleGraph, ids: &[FileId]) -> Vec<String> {
    let root = fixture_path(FIXTURE);
    ids.iter()
        .map(|id| {
            graph.modules[id.0 as usize]
                .path
                .strip_prefix(&root)
                .expect("module inside the fixture")
                .to_string_lossy()
                .replace('\\', "/")
        })
        .collect()
}

fn size_of(graph: &ModuleGraph, id: FileId) -> u64 {
    std::fs::metadata(&graph.modules[id.0 as usize].path).map_or(0, |metadata| metadata.len())
}

#[test]
fn entry_closure_splits_eager_deferred_and_out_of_thread_modules() {
    let output = fallow_core::analyze_with_trace(&create_config(fixture_path(FIXTURE)))
        .expect("analysis should succeed");
    let graph = output.graph.as_ref().expect("graph is retained");
    let entry = module_named(&graph.modules, "src/index.ts").file_id;

    let closure = graph.entry_load_closure(entry);

    let mut eager = relative(graph, &closure.eager);
    eager.sort();
    assert_eq!(
        eager,
        [
            "src/eager/one.ts",
            "src/heavy/chart-data.ts",
            "src/heavy/formatters.ts",
            "src/heavy/view.ts",
            "src/index.ts",
            "src/legacy.js",
            "src/reexported.ts",
            "src/shared.ts",
            "src/styles.css",
        ],
        "type-only imports, import() targets and worker targets stay off the eager path"
    );
    let mut deferred = relative(graph, &closure.deferred);
    deferred.sort();
    assert_eq!(
        deferred,
        [
            "src/lazy-glob/two.ts",
            "src/lazy-only.ts",
            "src/lazy.ts",
            "src/pages/home.ts",
        ],
        "a module that is also eager (shared.ts) is not deferred"
    );
    let mut out_of_thread = relative(graph, &closure.out_of_thread);
    out_of_thread.sort();
    assert_eq!(
        out_of_thread,
        ["src/child.js", "src/worker-only.ts", "src/worker.ts"]
    );
}

#[test]
fn dominating_imports_rank_single_edges_by_exclusive_weight() {
    let output = fallow_core::analyze_with_trace(&create_config(fixture_path(FIXTURE)))
        .expect("analysis should succeed");
    let graph = output.graph.as_ref().expect("graph is retained");
    let entry = module_named(&graph.modules, "src/index.ts").file_id;
    let closure = graph.entry_load_closure(entry);

    let imports = graph.eager_dominating_imports(entry, &closure.eager, |id| size_of(graph, id));

    let ranked: Vec<(String, String, usize)> = imports
        .iter()
        .map(|import| {
            let names = relative(graph, &[import.importer, import.target]);
            (names[0].clone(), names[1].clone(), import.exclusive_modules)
        })
        .collect();
    assert_eq!(
        ranked,
        [
            (
                "src/index.ts".to_string(),
                "src/heavy/view.ts".to_string(),
                3
            ),
            (
                "src/heavy/view.ts".to_string(),
                "src/heavy/chart-data.ts".to_string(),
                1
            ),
            (
                "src/heavy/view.ts".to_string(),
                "src/heavy/formatters.ts".to_string(),
                1
            ),
            (
                "src/index.ts".to_string(),
                "src/reexported.ts".to_string(),
                1
            ),
            ("src/index.ts".to_string(), "src/legacy.js".to_string(), 1),
            ("src/index.ts".to_string(), "src/styles.css".to_string(), 1),
            (
                "src/index.ts".to_string(),
                "src/eager/one.ts".to_string(),
                1
            ),
        ],
        "shared.ts has two eager importers, so no single import dominates it"
    );

    let view = &imports[0];
    let expected_view_weight: u64 = [
        "src/heavy/view.ts",
        "src/heavy/chart-data.ts",
        "src/heavy/formatters.ts",
    ]
    .iter()
    .map(|path| size_of(graph, module_named(&graph.modules, path).file_id))
    .sum();
    assert_eq!(view.exclusive_weight, expected_view_weight);
    assert!(
        view.import_span_start.is_some(),
        "a static import carries the span of its binding"
    );
    assert!(
        imports[6].import_span_start.is_none(),
        "an eager glob match has no binding span"
    );
}
