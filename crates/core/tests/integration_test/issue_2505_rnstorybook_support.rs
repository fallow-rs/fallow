//! React Native Storybook convention coverage for issue #2505.

use fallow_config::{WorkspaceDiagnosticKind, workspace_diagnostics_for};

use super::common::{create_config, fixture_path};

fn relative(path: &std::path::Path, root: &std::path::Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

#[test]
fn react_native_storybook_configuration_forms_one_reachable_graph() {
    let root = fixture_path("issue-2505-rnstorybook-support");
    let config = create_config(root.clone());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_files: Vec<_> = results
        .unused_files
        .iter()
        .map(|finding| relative(&finding.file.path, &root))
        .collect();
    for path in [
        ".rnstorybook/main.ts",
        ".rnstorybook/index.tsx",
        ".rnstorybook/storybook.requires.ts",
        ".rnstorybook/preview.ts",
        "src/mobile-case.tsx",
    ] {
        assert!(
            !unused_files.iter().any(|unused| unused == path),
            "{path} should be reachable through the React Native Storybook graph, found {unused_files:?}"
        );
    }

    let unused_exports: Vec<_> = results
        .unused_exports
        .iter()
        .map(|finding| {
            (
                relative(&finding.export.path, &root),
                finding.export.export_name.as_str(),
            )
        })
        .collect();
    assert!(
        !unused_exports.iter().any(|(path, _)| {
            path.starts_with(".rnstorybook/") || path == "src/mobile-case.tsx"
        }),
        "React Native Storybook exports should be consumed by the framework graph, found {unused_exports:?}"
    );

    let unused_dev_dependencies: Vec<_> = results
        .unused_dev_dependencies
        .iter()
        .map(|finding| finding.dep.package_name.as_str())
        .collect();
    for dependency in [
        "@storybook/react-native",
        "@storybook/addon-ondevice-actions",
    ] {
        assert!(
            !unused_dev_dependencies.contains(&dependency),
            "{dependency} should be credited by React Native Storybook, found {unused_dev_dependencies:?}"
        );
    }

    let skipped_hidden_dirs: Vec<_> = workspace_diagnostics_for(&root)
        .into_iter()
        .filter(|diagnostic| {
            matches!(
                diagnostic.kind,
                WorkspaceDiagnosticKind::SkippedSourceDotdir
            )
        })
        .map(|diagnostic| relative(&diagnostic.path, &root))
        .collect();
    assert!(
        !skipped_hidden_dirs.contains(&".rnstorybook".to_string()),
        ".rnstorybook should be discovered, found {skipped_hidden_dirs:?}"
    );
    assert!(
        skipped_hidden_dirs.contains(&".hidden-other".to_string()),
        "unrelated hidden source directories should remain skipped, found {skipped_hidden_dirs:?}"
    );
}
