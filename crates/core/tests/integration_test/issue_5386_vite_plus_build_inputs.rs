//! Issue #5386: a Vite+ project can list only `vite-plus` in its manifest.
//! The Vite plugin must still read the build inputs of `vite.config.*`, or
//! each file that only a build input reaches is a false `unused-file`. The
//! `@lib` alias checks that the Vite and Vitest plugins, which both read
//! `resolve.alias`, keep the alias target reachable together.

use super::common::{create_config, fixture_path};

#[test]
fn vite_plus_dependency_reads_vite_build_inputs_and_aliases() {
    let root = fixture_path("issue-5386-vite-plus-build-inputs");
    let config = create_config(root.clone());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let mut unused: Vec<String> = results
        .unused_files
        .iter()
        .map(|file| {
            file.file
                .path
                .strip_prefix(&root)
                .unwrap_or(&file.file.path)
                .to_string_lossy()
                .replace('\\', "/")
        })
        .collect();
    unused.sort();
    assert_eq!(
        unused,
        vec!["src/orphan.ts".to_string()],
        "only the orphan is unused: the HTML inputs, the worker input and the alias target are reached"
    );

    let unresolved: Vec<&str> = results
        .unresolved_imports
        .iter()
        .map(|import| import.import.specifier.as_str())
        .collect();
    assert!(
        unresolved.is_empty(),
        "the `@lib` alias resolves. Got: {unresolved:?}"
    );
}
