use super::common::{create_config, fixture_path};

fn unused_dev_dependencies(results: &fallow_core::results::AnalysisResults) -> Vec<String> {
    results
        .unused_dev_dependencies
        .iter()
        .map(|dep| dep.dep.package_name.clone())
        .collect()
}

fn unused_files(
    results: &fallow_core::results::AnalysisResults,
    root: &std::path::Path,
) -> Vec<String> {
    let mut files: Vec<String> = results
        .unused_files
        .iter()
        .map(|file| {
            file.file
                .path
                .strip_prefix(root)
                .unwrap_or(&file.file.path)
                .to_string_lossy()
                .replace('\\', "/")
        })
        .collect();
    files.sort();
    files
}

#[test]
fn vite_plus_config_blocks_credit_dependencies_and_entries() {
    let root = fixture_path("vite-plus-config");
    let config = create_config(root.clone());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_deps = unused_dev_dependencies(&results);
    for credited in [
        // `lint.jsPlugins` and `lint.overrides[].jsPlugins`.
        "@e18e/eslint-plugin",
        "eslint-plugin-override-only",
        // A `staged` command, a `run.tasks` command and a `.vite-hooks` hook.
        "staged-tool",
        "task-tool",
        "hook-tool",
        // `vp exec tsx` and the `vp` binary.
        "tsx",
        "vite-plus",
    ] {
        assert!(
            !unused_deps.iter().any(|dep| dep == credited),
            "{credited} must be credited by the Vite+ config, got {unused_deps:?}"
        );
    }
    assert!(
        unused_deps
            .iter()
            .any(|dep| dep == "eslint-plugin-unused-control"),
        "the control dependency must stay unused, got {unused_deps:?}"
    );

    let files = unused_files(&results, &root);
    for used in [
        // `pack` entries: object map form and array form.
        "src/index.ts",
        "src/gitlab/index.ts",
        "src/azure/index.ts",
        // `run.tasks` commands.
        "scripts/task-only.ts",
        "scripts/task-short.ts",
        // `vp exec tsx <file>`.
        "scripts/gen.ts",
        // A local `lint.jsPlugins` specifier.
        "lint/local-plugin.js",
        // `vp test -c <config>` reads the config as a Vitest config.
        "e2e/app.e2e.ts",
    ] {
        assert!(
            !files.iter().any(|file| file == used),
            "{used} must be used through the Vite+ config, got {files:?}"
        );
    }
    for unused in [
        "src/dead.ts",
        // `vp lint` and `vp fmt` read their file arguments but do not run them.
        "scripts/lint-target.ts",
        "scripts/fmt-target.ts",
    ] {
        assert!(
            files.iter().any(|file| file == unused),
            "{unused} must stay unused, got {files:?}"
        );
    }
}

#[test]
fn vite_plus_dependency_enables_vitest_test_and_bench_entries() {
    let root = fixture_path("vite-plus-enabler");
    let config = create_config(root.clone());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let files = unused_files(&results, &root);
    assert!(
        files.is_empty(),
        "the test and bench files of a Vite+ project are entries, got {files:?}"
    );
    let unused_deps = unused_dev_dependencies(&results);
    assert!(
        unused_deps.is_empty(),
        "`vp` credits vite-plus without node_modules, got {unused_deps:?}"
    );
}

#[test]
fn plain_vite_config_pack_block_is_not_read() {
    let root = fixture_path("vite-plus-plain-vite");
    let config = create_config(root.clone());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let files = unused_files(&results, &root);
    assert!(
        files.iter().any(|file| file == "src/pack-only.ts"),
        "a plain Vite project has no Vite+ pack entries, got {files:?}"
    );
}
