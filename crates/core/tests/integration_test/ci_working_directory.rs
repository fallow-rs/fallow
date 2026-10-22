//! A GitHub Actions step resolves its file arguments in its
//! `working-directory`, and a workspace anchor such as `$GITHUB_WORKSPACE/`
//! resolves from the project root. Composite actions count as CI files.

use super::common::{create_config, fixture_path};

fn unused_file_paths(results: &fallow_types::results::AnalysisResults) -> Vec<String> {
    results
        .unused_files
        .iter()
        .map(|f| f.file.path.to_string_lossy().replace('\\', "/"))
        .collect()
}

#[test]
fn ci_file_arguments_resolve_in_the_working_directory() {
    let root = fixture_path("ci-working-directory");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let paths = unused_file_paths(&results);

    for file in [
        "packages/web/scripts/direct.js",
        "apps/desktop/scripts/default.ts",
        "ecosystem-ci/patch.ts",
        "scripts/anchored.ts",
        "ci/quoted.ts",
    ] {
        assert!(
            !paths.iter().any(|p| p.ends_with(file)),
            "{file} runs from a CI step and must be an entry. Got: {paths:?}"
        );
    }
    for file in ["scripts/direct.js", "inner.ts", "verify.ts"] {
        assert!(
            paths
                .iter()
                .any(|p| p.ends_with(file) && !p.ends_with(&format!("web/{file}"))),
            "{file} is relative to another working directory and must stay unused. Got: {paths:?}"
        );
    }
}

#[test]
fn composite_action_step_credits_its_binary() {
    let root = fixture_path("ci-working-directory");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let unused: Vec<&str> = results
        .unused_dev_dependencies
        .iter()
        .map(|finding| finding.dep.package_name.as_str())
        .collect();
    assert!(
        !unused.contains(&"@napi-rs/cli"),
        "`napi build` in a composite action must credit @napi-rs/cli: {unused:?}"
    );
}
