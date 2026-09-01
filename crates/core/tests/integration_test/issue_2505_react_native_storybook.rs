//! Integration test for React Native Storybook's `.rnstorybook` convention
//! (issue #2505).
//!
//! Fixture `tests/fixtures/rn-storybook/` is a conventional React Native
//! Storybook setup: entry-point swapping makes `.rnstorybook/index.js` the
//! application entry, that entry imports the generated
//! `.rnstorybook/storybook.requires.ts`, and `.rnstorybook/main.ts` declares
//! its story glob plus on-device addons under `deviceAddons`.
//!
//! Without plugin support the whole directory sits outside the source graph:
//! `.rnstorybook` is dropped by the hidden-directory filter, so its files are
//! never parsed and the on-device addon packages read as unused.

use std::path::Path;

use fallow_config::{WorkspaceDiagnostic, WorkspaceDiagnosticKind};

use super::common::{create_config, fixture_path};

const FIXTURE: &str = "rn-storybook";

fn rel(path: &Path, root: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn skipped_dotdirs(root: &Path) -> Vec<WorkspaceDiagnostic> {
    fallow_config::workspace_diagnostics_for(root)
        .into_iter()
        .filter(|diagnostic| {
            matches!(
                diagnostic.kind,
                WorkspaceDiagnosticKind::SkippedSourceDotdir
            )
        })
        .collect()
}

#[test]
fn react_native_storybook_config_directory_is_part_of_the_source_graph() {
    let root = fixture_path(FIXTURE);
    let config = create_config(root.clone());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let files: Vec<String> = fallow_core::discover::discover_files_with_plugin_scopes(&config)
        .iter()
        .map(|file| rel(&file.path, &config.root))
        .collect();
    for expected in [
        ".rnstorybook/main.ts",
        ".rnstorybook/index.js",
        ".rnstorybook/preview.tsx",
        ".rnstorybook/storybook.requires.ts",
    ] {
        assert!(
            files.contains(&expected.to_string()),
            "{expected} should be discovered, found {files:?}"
        );
    }

    let unused_files: Vec<String> = results
        .unused_files
        .iter()
        .map(|finding| rel(&finding.file.path, &config.root))
        .collect();
    assert!(
        unused_files.is_empty(),
        "a conventional React Native Storybook tree has no unused files, found {unused_files:?}"
    );

    let unused_exports: Vec<String> = results
        .unused_exports
        .iter()
        .map(|finding| {
            format!(
                "{}#{}",
                rel(&finding.export.path, &config.root),
                finding.export.export_name
            )
        })
        .collect();
    assert!(
        unused_exports.is_empty(),
        "story and config exports are framework entry points, found {unused_exports:?}"
    );

    let unused_deps: Vec<&str> = results
        .unused_dependencies
        .iter()
        .map(|finding| finding.dep.package_name.as_str())
        .chain(
            results
                .unused_dev_dependencies
                .iter()
                .map(|finding| finding.dep.package_name.as_str()),
        )
        .collect();
    assert!(
        unused_deps.is_empty(),
        "on-device addons and the React Native Storybook package are used, found {unused_deps:?}"
    );

    let diagnostics = skipped_dotdirs(&root);
    assert!(
        diagnostics.is_empty(),
        "`.rnstorybook` is traversed, so nothing reports as a skipped source dotdir: \
         {diagnostics:?}"
    );
}

/// Storybook regenerates `storybook.requires` on every bundle, so projects
/// routinely leave it out of version control. Then `deviceAddons` in
/// `.rnstorybook/main.ts` is the only place an on-device addon is named, and
/// the config parse is what has to credit it.
#[test]
fn device_addons_are_credited_without_the_generated_requires_module() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let root = tmp.path();
    std::fs::create_dir_all(root.join(".rnstorybook")).expect("create config dir");
    std::fs::create_dir_all(root.join("src")).expect("create src dir");
    std::fs::write(
        root.join("package.json"),
        r#"{
  "name": "rn-storybook-generated-file-absent",
  "devDependencies": {
    "@storybook/addon-ondevice-actions": "^9.0.0",
    "@storybook/react-native": "^9.0.0"
  }
}
"#,
    )
    .expect("write package.json");
    std::fs::write(
        root.join(".rnstorybook/main.ts"),
        r"import type { StorybookConfig } from '@storybook/react-native';

const config: StorybookConfig = {
  stories: ['../src/**/*.stories.tsx'],
  deviceAddons: ['@storybook/addon-ondevice-actions'],
};

export default config;
",
    )
    .expect("write main.ts");
    std::fs::write(
        root.join("src/Button.stories.tsx"),
        "export default { title: 'Button' };\nexport const Primary = {};\n",
    )
    .expect("write story");

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_deps: Vec<&str> = results
        .unused_dev_dependencies
        .iter()
        .map(|finding| finding.dep.package_name.as_str())
        .collect();
    assert!(
        !unused_deps.contains(&"@storybook/addon-ondevice-actions"),
        "an addon named only by deviceAddons is referenced, found {unused_deps:?}"
    );
}
