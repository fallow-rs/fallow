//! Issue #3886: a package script that runs a project-local shell helper
//! credits the packages and seeds the entry files that the helper runs.

use super::common::{create_config, fixture_path};

#[test]
fn shell_helpers_run_by_package_scripts_credit_their_commands() {
    let root = fixture_path("issue-3886-shell-helper-scripts");
    let results =
        fallow_core::analyze(&create_config(root.clone())).expect("analysis should succeed");

    let unused_deps: Vec<&str> = results
        .unused_dev_dependencies
        .iter()
        .map(|finding| finding.dep.package_name.as_str())
        .collect();
    for used in ["report-tool", "nested-tool", "exec-tool", "cd-tool"] {
        assert!(
            !unused_deps.contains(&used),
            "{used} runs in a shell helper: {unused_deps:?}"
        );
    }
    assert!(
        unused_deps.contains(&"unused-tool"),
        "a comment or heredoc line is not a command: {unused_deps:?}"
    );

    let unused_files: Vec<String> = results
        .unused_files
        .iter()
        .map(|finding| {
            finding
                .file
                .path
                .strip_prefix(&root)
                .unwrap_or(&finding.file.path)
                .to_string_lossy()
                .replace('\\', "/")
        })
        .collect();
    assert!(
        !unused_files.contains(&"src/generate.ts".to_string()),
        "a file that a helper runs with tsx is an entry: {unused_files:?}"
    );
    assert!(
        unused_files.contains(&"src/after-cd.ts".to_string()),
        "a path after `cd` is relative to another directory: {unused_files:?}"
    );
}
