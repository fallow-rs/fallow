use super::common::{create_config, fixture_path};

fn relative(root: &std::path::Path, path: &std::path::Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

#[test]
fn alchemy_stacks_and_worker_mains_are_credited() {
    // Alchemy loads `alchemy.run.ts` by convention and bundles the file that
    // each resource names in `main`. No module imports either one.
    let root = fixture_path("alchemy-plugin");
    let config = create_config(root.clone());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_files: Vec<String> = results
        .unused_files
        .iter()
        .map(|finding| relative(&root, &finding.file.path))
        .collect();
    for path in [
        "alchemy.run.ts",
        "stacks/infra.ts",
        "src/api.ts",
        "src/async-worker.ts",
        "src/site/infra.ts",
        "src/site/entry.ts",
        "src/site/render.ts",
    ] {
        assert!(
            !unused_files.contains(&path.to_string()),
            "{path} should be reachable through the Alchemy stack, unused files: {unused_files:?}"
        );
    }
    for path in ["src/orphan.ts", "snippets/draft-worker.ts"] {
        assert!(
            unused_files.contains(&path.to_string()),
            "{path} is not deployed by a stack and should still report, unused files: {unused_files:?}"
        );
    }

    let unused_exports: Vec<String> = results
        .unused_exports
        .iter()
        .map(|finding| {
            format!(
                "{}:{}",
                relative(&root, &finding.export.path),
                finding.export.export_name
            )
        })
        .collect();
    for export in [
        "src/api.ts:Counter",
        "src/api.ts:default",
        "src/async-worker.ts:Room",
        "src/async-worker.ts:default",
        "src/site/entry.ts:default",
    ] {
        assert!(
            !unused_exports.contains(&export.to_string()),
            "{export} is read by the platform runtime, unused exports: {unused_exports:?}"
        );
    }
    for export in ["src/api.ts:LocalOnly", "src/api.ts:RequestFailed"] {
        assert!(
            unused_exports.contains(&export.to_string()),
            "{export} is not bound by the platform and should still report, unused exports: {unused_exports:?}"
        );
    }

    let unused_dependencies: Vec<&str> = results
        .unused_dependencies
        .iter()
        .map(|dep| dep.dep.package_name.as_str())
        .collect();
    for dep in ["@effect/platform-node", "@effect/platform-bun", "alchemy"] {
        assert!(
            !unused_dependencies.contains(&dep),
            "{dep} should be credited by the Alchemy plugin, unused deps: {unused_dependencies:?}"
        );
    }
    assert!(
        unused_dependencies.contains(&"unused-control"),
        "an unreferenced dependency should still report, unused deps: {unused_dependencies:?}"
    );
}
