use super::common::{create_config, fixture_path};

/// An import with `with { type: "macro" }` runs its target at build time. The
/// bundler inlines the result, so a devDependency that only a macro module
/// imports is not a production dependency. A module that a runtime import also
/// reaches stays production code. See issue #5100.
#[test]
fn macro_import_target_is_build_time() {
    let config = create_config(fixture_path("macro-import-build-time"));
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let flagged: Vec<&str> = results
        .dev_dependencies_in_production
        .iter()
        .map(|finding| finding.dep.package_name.as_str())
        .collect();
    assert!(
        !flagged.contains(&"figlet"),
        "a devDependency that only a macro module imports is build time: {flagged:?}"
    );
    assert!(
        !flagged.contains(&"build-stamp"),
        "a package that a macro import runs is build time: {flagged:?}"
    );
    assert!(
        flagged.contains(&"kleur"),
        "a module that a runtime import also reaches stays production code: {flagged:?}"
    );

    assert!(
        !results
            .unused_files
            .iter()
            .any(|finding| finding.file.path.ends_with("src/title.macro.ts")),
        "the macro module stays used"
    );

    let unused_exports: Vec<&str> = results
        .unused_exports
        .iter()
        .map(|finding| finding.export.export_name.as_str())
        .collect();
    assert!(
        !unused_exports.contains(&"title"),
        "the macro export stays used: {unused_exports:?}"
    );

    let unused_dev: Vec<&str> = results
        .unused_dev_dependencies
        .iter()
        .map(|finding| finding.dep.package_name.as_str())
        .collect();
    assert!(
        !unused_dev.contains(&"figlet"),
        "the macro module still uses its devDependency: {unused_dev:?}"
    );
}

/// A macro module is reachable, but it never runs in the shipped program, so
/// it is not runtime reachable and no load closure of the entry holds it.
#[test]
fn macro_import_target_is_not_runtime_reachable() {
    let output =
        fallow_core::analyze_with_trace(&create_config(fixture_path("macro-import-build-time")))
            .expect("analysis should succeed");
    let graph = output.graph.as_ref().expect("graph is retained");
    let module = |suffix: &str| {
        graph
            .modules()
            .iter()
            .find(|module| {
                module
                    .path
                    .to_string_lossy()
                    .replace('\\', "/")
                    .ends_with(suffix)
            })
            .unwrap_or_else(|| panic!("{suffix} must be part of the module graph"))
    };

    let title = module("src/title.macro.ts");
    assert!(title.is_reachable());
    assert!(!title.is_runtime_reachable());
    assert!(module("src/paint.ts").is_runtime_reachable());

    let closure = graph.entry_load_closure(module("src/index.ts").file_id);
    assert!(closure.eager.contains(&module("src/paint.ts").file_id));
    for ids in [&closure.eager, &closure.deferred, &closure.out_of_thread] {
        assert!(!ids.contains(&title.file_id));
    }
}
