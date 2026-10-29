//! Issue #3886: a package script or CI step that runs a project-local shell
//! helper credits the packages and seeds the entry files that the helper runs.

use super::common::{create_config, fixture_path};

#[test]
fn shell_helpers_run_by_scripts_and_ci_credit_their_commands() {
    let root = fixture_path("issue-3886-shell-helper-scripts");
    let results =
        fallow_core::analyze(&create_config(root.clone())).expect("analysis should succeed");

    let unused_deps: Vec<&str> = results
        .unused_dev_dependencies
        .iter()
        .map(|finding| finding.dep.package_name.as_str())
        .collect();
    for (used, why) in [
        ("report-tool", "a package script runs the helper with bash"),
        ("nested-tool", "a helper runs a nested helper"),
        ("exec-tool", "a package script runs the helper directly"),
        ("cd-tool", "a command after cd still credits its package"),
        (
            "ws-tool",
            "a workspace script runs a helper of the repository",
        ),
        (
            "ci-tool",
            "a CI step runs a helper in its working-directory",
        ),
    ] {
        assert!(
            !unused_deps.contains(&used),
            "{used}: {why}: {unused_deps:?}"
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
    // `src/generate.ts`, `src/release-entry.ts` (after `cd "$(dirname "$0")/.."`),
    // `packages/web/src/ws-entry.ts` and `tools/src/ci-entry.ts` are helper
    // entries. `chmod +x` and `shellcheck` take a helper path but add nothing.
    // `src/after-cd.ts` stays unused, because the helper runs it after
    // `cd packages`.
    assert_eq!(unused_files, vec!["src/after-cd.ts".to_string()]);
}
