//! A Vitest config that exports `mergeConfig(inlineBase, defineConfig({ test }))`
//! keeps its `test.setupFiles` in the second argument. Fallow read only the
//! inline base object, so it reported each setup file as unused.

use super::common::{create_config, fixture_path};

#[test]
fn merge_config_override_setup_files_are_used() {
    let root = fixture_path("vitest-merge-config-setup-files");
    let config = create_config(root.clone());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused: Vec<String> = results
        .unused_files
        .iter()
        .map(|f| {
            f.file
                .path
                .strip_prefix(&root)
                .unwrap_or(&f.file.path)
                .to_string_lossy()
                .replace('\\', "/")
        })
        .collect();
    for setup in ["src/test-utils/git.setup.ts", "src/test-utils/env.setup.ts"] {
        assert!(
            !unused.iter().any(|path| path == setup),
            "{setup} is a setup file of the merged config, unused files: {unused:?}"
        );
    }
    assert!(
        unused.iter().any(|path| path == "src/test-utils/orphan.ts"),
        "a file that no config names stays unused, unused files: {unused:?}"
    );
}
