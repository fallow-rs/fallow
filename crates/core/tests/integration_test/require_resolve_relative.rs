//! `require.resolve('./relative/path')` hands the path of a project file to a
//! consumer that fallow cannot see, such as a webpack
//! `NormalModuleReplacementPlugin` in `next.config.js` or a worker spawn in
//! application code. The resolved file and its exports are in use. A
//! `require.resolve` call with a `paths` option resolves from other
//! directories, so fallow does not follow it.

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
    for referenced in ["rafShim.js", "worker.js"] {
        assert!(
            !unused_files.contains(&referenced.to_string()),
            "{referenced} is referenced through require.resolve, got unused files: {unused_files:?}"
        );
    }
    assert!(
        unused_files.contains(&"orphan.js".to_string()),
        "orphan.js has no reference and must stay unused, got: {unused_files:?}"
    );

    let unused_exports: Vec<(String, String)> = results
        .unused_exports
        .iter()
        .map(|e| (file_name(&e.export.path), e.export.export_name.clone()))
        .collect();
    assert!(
        !unused_exports
            .iter()
            .any(|(file, _)| file == "rafShim.js" || file == "worker.js"),
        "the consumer of a resolved path uses the whole module, got: {unused_exports:?}"
    );

    let unresolved: Vec<&str> = results
        .unresolved_imports
        .iter()
        .map(|u| u.import.specifier.as_str())
        .collect();
    assert!(
        unresolved.is_empty(),
        "require.resolve targets must not become unresolved imports, got: {unresolved:?}"
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
