#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests and benches use unwrap and expect to keep fixture setup concise"
)]

//! `workspaces.settings`: per-workspace `ignoreDependencies` and
//! `includeEntryExports`.

use std::path::Path;

use crate::common::{
    CommandOutput, fallow_bin, parse_json, run_fallow_in_root, scrub_analysis_env,
};
use tempfile::TempDir;

fn write(root: &Path, relative: &str, contents: &str) {
    let path = root.join(relative);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, contents).unwrap();
}

/// A pnpm monorepo with two packages and one app:
///
/// - each package declares the unused dependency `left-pad`;
/// - each package entry exports `used` (imported by the app) and `stray`;
/// - the app imports the unlisted package `chalk`.
fn monorepo(config: &str) -> TempDir {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path();
    write(
        root,
        "package.json",
        r#"{ "name": "mono", "private": true, "workspaces": ["packages/*", "apps/*"] }"#,
    );
    for name in ["a", "b"] {
        write(
            root,
            &format!("packages/{name}/package.json"),
            &format!(
                r#"{{ "name": "@mono/{name}", "main": "src/index.js", "dependencies": {{ "left-pad": "1.3.0" }} }}"#
            ),
        );
        write(
            root,
            &format!("packages/{name}/src/index.js"),
            "export const used = 1;\nexport const stray = 2;\n",
        );
    }
    write(
        root,
        "apps/web/package.json",
        r#"{ "name": "web", "main": "src/main.js", "dependencies": { "@mono/a": "workspace:*", "@mono/b": "workspace:*" } }"#,
    );
    write(
        root,
        "apps/web/src/main.js",
        "import chalk from 'chalk';\nimport { used } from '@mono/a';\nimport { used as b } from '@mono/b';\nconsole.log(chalk, used, b);\n",
    );
    write(root, ".fallowrc.json", config);
    dir
}

fn dead_code(root: &Path) -> serde_json::Value {
    let output = run_fallow_in_root("dead-code", root, &["--format", "json", "--quiet"]);
    assert!(
        output.code == 0 || output.code == 1,
        "dead-code should not crash. stdout: {}\nstderr: {}",
        output.stdout,
        output.stderr
    );
    parse_json(&output)
}

fn names(json: &serde_json::Value, key: &str, field: &str) -> Vec<String> {
    json[key]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| {
            format!(
                "{}:{}",
                item["path"].as_str().unwrap_or_default(),
                item[field].as_str().unwrap()
            )
        })
        .collect()
}

fn unlisted(json: &serde_json::Value) -> Vec<String> {
    json["unlisted_dependencies"]
        .as_array()
        .unwrap()
        .iter()
        .map(|dep| dep["package_name"].as_str().unwrap().to_string())
        .collect()
}

#[test]
fn without_settings_every_workspace_reports() {
    let json = dead_code(monorepo("{}").path());
    let deps = names(&json, "unused_dependencies", "package_name");
    assert!(
        deps.contains(&"packages/a/package.json:left-pad".to_string()),
        "{deps:?}"
    );
    assert!(
        deps.contains(&"packages/b/package.json:left-pad".to_string()),
        "{deps:?}"
    );
    assert_eq!(unlisted(&json), vec!["chalk"]);
    let exports = names(&json, "unused_exports", "export_name");
    assert!(
        exports.is_empty(),
        "entry exports are skipped by default: {exports:?}"
    );
}

#[test]
fn workspace_ignore_dependencies_applies_only_to_the_selected_workspace() {
    let dir = monorepo(
        r#"{ "workspaces": { "settings": {
            "packages/a": { "ignoreDependencies": ["left-pad"] },
            "apps/*": { "ignoreDependencies": ["chalk"] }
        } } }"#,
    );
    let json = dead_code(dir.path());
    let deps = names(&json, "unused_dependencies", "package_name");
    assert!(
        !deps.contains(&"packages/a/package.json:left-pad".to_string()),
        "the key of packages/a hides its unused dependency: {deps:?}"
    );
    assert!(
        deps.contains(&"packages/b/package.json:left-pad".to_string()),
        "packages/b keeps its finding: {deps:?}"
    );
    assert!(
        unlisted(&json).is_empty(),
        "the app key hides the unlisted import"
    );
}

#[test]
fn workspace_ignore_dependencies_does_not_hide_an_import_of_another_workspace() {
    let dir = monorepo(
        r#"{ "workspaces": { "settings": {
            "packages/*": { "ignoreDependencies": ["chalk"] }
        } } }"#,
    );
    let json = dead_code(dir.path());
    assert_eq!(
        unlisted(&json),
        vec!["chalk"],
        "the app imports chalk, not a package"
    );
}

#[test]
fn workspace_include_entry_exports_reports_entry_exports_of_selected_workspaces() {
    let dir = monorepo(
        r#"{ "workspaces": { "settings": {
            "packages/*": { "includeEntryExports": true },
            "packages/b": { "includeEntryExports": false }
        } } }"#,
    );
    let json = dead_code(dir.path());
    let exports = names(&json, "unused_exports", "export_name");
    assert_eq!(
        exports,
        vec!["packages/a/src/index.js:stray".to_string()],
        "the glob turns the check on and the exact key of packages/b turns it off"
    );
}

#[test]
fn cli_flag_wins_over_a_workspace_false() {
    let dir = monorepo(
        r#"{ "workspaces": { "settings": { "packages/b": { "includeEntryExports": false } } } }"#,
    );
    let output = run_fallow_in_root(
        "dead-code",
        dir.path(),
        &["--format", "json", "--quiet", "--include-entry-exports"],
    );
    let json = parse_json(&output);
    let exports = names(&json, "unused_exports", "export_name");
    assert!(
        exports.contains(&"packages/b/src/index.js:stray".to_string()),
        "--include-entry-exports turns the check on everywhere: {exports:?}"
    );
}

/// Run with the ambient `RUST_LOG` removed, so the CLI default filter decides
/// which warnings reach stderr.
fn run_with_default_log(root: &Path) -> CommandOutput {
    let mut cmd = std::process::Command::new(fallow_bin());
    cmd.env("NO_COLOR", "1")
        .env_remove("RUST_LOG")
        .env_remove("FALLOW_QUIET");
    scrub_analysis_env(&mut cmd);
    let output = cmd
        .args(["dead-code", "--no-cache", "--root"])
        .arg(root)
        .output()
        .expect("run fallow");
    CommandOutput {
        stdout: String::from_utf8_lossy(&output.stdout).to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        code: output.status.code().unwrap_or(-1),
    }
}

#[test]
fn a_key_that_selects_no_workspace_warns() {
    let dir = monorepo(
        r#"{ "workspaces": { "settings": { "packages/missing": { "includeEntryExports": true } } } }"#,
    );
    let output = run_with_default_log(dir.path());
    assert!(
        output
            .stderr
            .contains("workspaces.settings key 'packages/missing' selects no workspace"),
        "stderr: {}",
        output.stderr
    );
}

#[test]
fn an_unknown_workspaces_key_warns_and_an_unknown_settings_field_fails() {
    let dir = monorepo(r#"{ "workspaces": { "setting": {} } }"#);
    let output = run_with_default_log(dir.path());
    assert!(
        output.stderr.contains("unknown key 'workspaces.setting'"),
        "stderr: {}",
        output.stderr
    );

    let dir = monorepo(
        r#"{ "workspaces": { "settings": { "packages/a": { "ignoreDependency": ["x"] } } } }"#,
    );
    let output = run_fallow_in_root("dead-code", dir.path(), &["--format", "json"]);
    assert_eq!(output.code, 2, "stdout: {}", output.stdout);
}
