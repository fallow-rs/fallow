//! `require.resolve('./relative/path')` hands the path of a project file to a
//! consumer that fallow cannot see, such as a webpack
//! `NormalModuleReplacementPlugin` in `next.config.js` or a worker spawn in
//! application code. The resolved file and its exports are in use. A
//! `require.resolve` call with a `paths` option resolves from other
//! directories, so fallow does not follow it. A target that is not on disk,
//! such as build output, is not reported as an unresolved import.

use super::common::{create_config, fixture_path};

fn file_name(path: &std::path::Path) -> String {
    path.to_string_lossy()
        .replace('\\', "/")
        .rsplit('/')
        .next()
        .unwrap_or("")
        .to_string()
}

#[cfg_attr(miri, ignore)]
#[test]
fn require_resolve_relative_path_references_the_file() {
    let root = fixture_path("require-resolve-relative");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_files: Vec<String> = results
        .unused_files
        .iter()
        .map(|f| file_name(&f.file.path))
        .collect();
    for referenced in ["rafShim.js", "worker.js", "template-target.js"] {
        assert!(
            !unused_files.contains(&referenced.to_string()),
            "{referenced} is referenced through require.resolve, got unused files: {unused_files:?}"
        );
    }
    assert!(
        unused_files.contains(&"orphan.js".to_string()),
        "orphan.js has no reference and must stay unused, got: {unused_files:?}"
    );
    assert!(
        unused_files.contains(&"searched.js".to_string()),
        "a require.resolve call with a paths option resolves from other directories, so searched.js must stay unused, got: {unused_files:?}"
    );

    let unused_exports: Vec<(String, String)> = results
        .unused_exports
        .iter()
        .map(|e| (file_name(&e.export.path), e.export.export_name.clone()))
        .collect();
    assert!(
        !unused_exports.iter().any(|(file, _)| matches!(
            file.as_str(),
            "rafShim.js" | "worker.js" | "template-target.js"
        )),
        "the consumer of a resolved path uses the whole module, got: {unused_exports:?}"
    );

    let unresolved: Vec<&str> = results
        .unresolved_imports
        .iter()
        .map(|u| u.import.specifier.as_str())
        .collect();
    assert!(
        unresolved.is_empty(),
        "require.resolve targets, including build output that is not on disk, must not become unresolved imports, got: {unresolved:?}"
    );
}

/// `require.resolve` returns a path and loads nothing, so its edge cannot
/// close a runtime cycle. `src/index.js` resolves `./loader.js`, and
/// `loader.js` requires `./index.js`. The default config must not report the
/// pair as a circular dependency.
#[cfg_attr(miri, ignore)]
#[test]
fn require_resolve_edge_does_not_close_a_cycle() {
    let root = fixture_path("require-resolve-relative");
    let config = create_config(root);
    assert!(!config.circular_dependencies.ignore_lazy_imports);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let cycles: Vec<Vec<String>> = results
        .circular_dependencies
        .iter()
        .map(|finding| finding.cycle.files.iter().map(|p| file_name(p)).collect())
        .collect();
    assert!(
        cycles.is_empty(),
        "a require.resolve edge must not take part in a cycle, got: {cycles:?}"
    );

    let unused_files: Vec<String> = results
        .unused_files
        .iter()
        .map(|f| file_name(&f.file.path))
        .collect();
    assert!(
        !unused_files.contains(&"loader.js".to_string()),
        "loader.js is referenced through require.resolve, got: {unused_files:?}"
    );
}

/// A `require.resolve` target does not load with the entry, so it is in no
/// part of the entry load closure: not eager, not deferred and not out of
/// thread. `--entry-weight` reads this closure.
#[cfg_attr(miri, ignore)]
#[test]
fn require_resolve_target_is_outside_the_entry_load_closure() {
    let output =
        fallow_core::analyze_with_trace(&create_config(fixture_path("require-resolve-relative")))
            .expect("analysis should succeed");
    let graph = output.graph.as_ref().expect("graph is retained");
    let entry = graph
        .modules
        .iter()
        .find(|m| {
            m.path
                .to_string_lossy()
                .replace('\\', "/")
                .ends_with("src/index.js")
        })
        .expect("src/index.js is part of the module graph")
        .file_id;

    let closure = graph.entry_load_closure(entry);
    for (part, ids) in [
        ("eager", &closure.eager),
        ("deferred", &closure.deferred),
        ("out_of_thread", &closure.out_of_thread),
    ] {
        let files: Vec<String> = ids
            .iter()
            .map(|id| file_name(&graph.modules[id.0 as usize].path))
            .collect();
        for target in ["worker.js", "loader.js", "template-target.js"] {
            assert!(
                !files.contains(&target.to_string()),
                "{target} is only a require.resolve target, but the {part} closure has it: {files:?}"
            );
        }
    }
}
