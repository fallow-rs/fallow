//! A GitHub Actions step resolves its file arguments in its
//! `working-directory`, and a workspace anchor such as `$GITHUB_WORKSPACE/`
//! resolves from the project root. Composite actions count as CI files.
//! Matrix values that a step runs, `actions/github-script` bodies, and step
//! `env` aliases of `${{ github.action_path }}` also name scripts.

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

/// A CI file can name a script outside a `run:` value: a matrix value that a
/// step runs, an `actions/github-script` body, or a step `env` variable that
/// holds `${{ github.action_path }}`.
#[test]
fn ci_scripts_named_outside_run_values_are_entries() {
    let root = fixture_path("ci-working-directory");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let paths = unused_file_paths(&results);

    for file in [
        "ecosystem-ci/matrix-command.ts",
        "scripts/matrix-list.ts",
        ".github/scripts/imported.mjs",
        ".github/scripts/required.js",
        ".github/actions/setup/prepare.mjs",
    ] {
        assert!(
            !paths.iter().any(|p| p.ends_with(file)),
            "{file} runs from a CI file and must be an entry. Got: {paths:?}"
        );
    }
    for file in [
        "scripts/matrix-unrelated.ts",
        ".github/scripts/relative-import.mjs",
        ".github/actions/setup/unaliased.mjs",
    ] {
        assert!(
            paths.iter().any(|p| p.ends_with(file)),
            "{file} is not run by a CI step and must stay unused. Got: {paths:?}"
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

#[test]
fn github_action_path_in_composite_action_marks_scripts_as_used() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir_all(root.join("lib")).unwrap();
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"action-test","private":true}"#,
    )
    .unwrap();
    std::fs::write(
        root.join("action.yml"),
        r#"name: a
runs:
  using: composite
  steps:
    - shell: bash
      run: node "${{ github.action_path }}/lib/expr.mjs"
    - shell: bash
      run: node "${GITHUB_ACTION_PATH}/lib/envvar.mjs"
    - shell: bash
      working-directory: packages/app
      run: node "$GITHUB_ACTION_PATH/lib/from_subdir.mjs"
"#,
    )
    .unwrap();
    for file in ["expr.mjs", "envvar.mjs", "from_subdir.mjs", "orphan.mjs"] {
        std::fs::write(root.join("lib").join(file), "console.log(1);\n").unwrap();
    }

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let paths = unused_file_paths(&results);

    for used in ["lib/expr.mjs", "lib/envvar.mjs", "lib/from_subdir.mjs"] {
        assert!(
            !paths.iter().any(|p| p.ends_with(used)),
            "{used} is invoked via action path anchor and must not be unused: {paths:?}"
        );
    }
    assert!(
        paths.iter().any(|p| p.ends_with("lib/orphan.mjs")),
        "lib/orphan.mjs is unreferenced and must remain unused: {paths:?}"
    );
}
