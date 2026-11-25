//! The hints that lead from a withheld fix to the config change that unblocks
//! it (issue #5127).
//!
//! The fixture has two degrading diagnostics. A hidden folder with module
//! sources (`skipped-source-dotdir`) leaves imports unseen, so `fallow fix`
//! withholds removals. An undeclared workspace folder degrades the analysis
//! too, but it does not withhold a fix. The root declares `packages/*`, so
//! fallow looks for undeclared workspaces at all. The fix output must name the first
//! diagnostic and not the second, and `fallow workspaces` must show both.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests use unwrap and expect to keep fixture setup concise"
)]

use std::path::Path;

use crate::common::{parse_json, run_fallow_in_root};

/// The hidden folder of the fixture. It is not `.claude`, because a global
/// gitignore of the developer machine can ignore that name and hide the
/// diagnostic for a reason that has no relation to fallow.
const DOTDIR: &str = ".tooling";

fn write_project(root: &Path) {
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::create_dir_all(root.join(DOTDIR).join("hooks")).unwrap();
    std::fs::create_dir_all(root.join("scripts/gate")).unwrap();
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"hint-chain","private":true,"type":"module","main":"src/index.ts","workspaces":["packages/*"]}"#,
    )
    .unwrap();
    std::fs::create_dir_all(root.join("packages/app")).unwrap();
    std::fs::write(
        root.join("packages/app/package.json"),
        r#"{"name":"app","private":true}"#,
    )
    .unwrap();
    std::fs::write(
        root.join("src/index.ts"),
        "import { used } from './lib';\nused();\n",
    )
    .unwrap();
    std::fs::write(
        root.join("src/lib.ts"),
        "export const used = (): void => {};\nexport const hookOnly = (): void => {};\n",
    )
    .unwrap();
    std::fs::write(
        root.join(DOTDIR).join("hooks/run.mjs"),
        "import { hookOnly } from '../../src/lib.ts';\nhookOnly();\n",
    )
    .unwrap();
    std::fs::write(
        root.join("scripts/gate/package.json"),
        r#"{"name":"gate","private":true}"#,
    )
    .unwrap();
    std::fs::write(root.join("scripts/gate/index.ts"), "export {};\n").unwrap();
}

#[test]
fn fix_dry_run_json_names_the_diagnostic_that_withholds_the_fix() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write_project(root);

    let output = run_fallow_in_root("fix", root, &["--dry-run", "--format", "json", "--quiet"]);
    assert_eq!(output.code, 0, "stderr: {}", output.stderr);
    let json = parse_json(&output);

    let blocking = json["incomplete_analysis_diagnostics"]
        .as_array()
        .unwrap_or_else(|| {
            panic!(
                "the envelope names the blocking diagnostics: {}",
                output.stdout
            )
        });
    let kinds: Vec<(&str, &str)> = blocking
        .iter()
        .map(|entry| {
            (
                entry["kind"].as_str().unwrap(),
                entry["path"].as_str().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        kinds,
        vec![("skipped-source-dotdir", DOTDIR)],
        "only the diagnostic that leaves imports unseen withholds a fix; the undeclared workspace does not"
    );
    assert!(
        blocking[0]["action"].as_str().is_some_and(
            |action| action.contains("`!.tooling/**`") && action.contains("ignorePatterns")
        ),
        "each blocking diagnostic carries a concrete action: {}",
        output.stdout
    );

    let withheld: Vec<&serde_json::Value> = json["fixes"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|fix| fix["skip_reason"] == "low_confidence_incomplete_analysis")
        .collect();
    assert!(
        !withheld.is_empty(),
        "the fix is withheld: {}",
        output.stdout
    );
    for fix in withheld {
        let help = fix["help"].as_str().unwrap();
        assert!(
            help.contains("`!.tooling/**`") && !help.contains("Resolve the workspace diagnostics"),
            "the help names the folder and the config change, not a generic step: {help}"
        );
    }
}

#[test]
fn fix_dry_run_human_output_names_the_diagnostic_that_withholds_the_fix() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write_project(root);

    let output = run_fallow_in_root("fix", root, &["--dry-run", "--quiet"]);
    assert_eq!(output.code, 0, "stderr: {}", output.stderr);
    assert!(
        output.stderr.contains(".tooling (skipped-source-dotdir)")
            && output.stderr.contains("`!.tooling/**`"),
        "quiet human output lists the blocking diagnostic and its action: {}",
        output.stderr
    );
    assert!(
        !output
            .stderr
            .contains("scripts/gate (undeclared-workspace)"),
        "a diagnostic that does not withhold a fix is not listed as blocking: {}",
        output.stderr
    );
}

#[test]
fn applied_fix_after_the_config_change_has_no_blocking_diagnostics() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write_project(root);
    std::fs::write(
        root.join(".fallowrc.json"),
        r#"{"ignorePatterns":["!.tooling/**"]}"#,
    )
    .unwrap();

    let output = run_fallow_in_root("fix", root, &["--dry-run", "--format", "json", "--quiet"]);
    assert_eq!(output.code, 0, "stderr: {}", output.stderr);
    let json = parse_json(&output);
    assert!(
        json.get("incomplete_analysis_diagnostics").is_none(),
        "a run with no withheld fix omits the field: {}",
        output.stdout
    );
    assert!(
        !json["fixes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|fix| fix["skip_reason"] == "low_confidence_incomplete_analysis"),
        "the action of the hint unblocks the fix: {}",
        output.stdout
    );
}

#[test]
fn workspaces_reports_the_source_walk_diagnostics() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write_project(root);

    let output = run_fallow_in_root("workspaces", root, &["--format", "json", "--quiet"]);
    assert_eq!(output.code, 0, "stderr: {}", output.stderr);
    let json = parse_json(&output);
    let kinds: Vec<(&str, &str)> = json["workspace_diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| {
            (
                entry["kind"].as_str().unwrap(),
                entry["path"].as_str().unwrap(),
            )
        })
        .collect();
    assert!(
        kinds.contains(&("skipped-source-dotdir", DOTDIR)),
        "`fallow workspaces` shows the skipped hidden folder: {kinds:?}"
    );
    assert!(
        kinds.contains(&("undeclared-workspace", "scripts/gate")),
        "`fallow workspaces` still shows the undeclared workspace: {kinds:?}"
    );
}

#[test]
fn undeclared_workspace_hint_names_the_fallow_config_option() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write_project(root);

    let output = run_fallow_in_root("workspaces", root, &["--format", "json", "--quiet"]);
    let json = parse_json(&output);
    let message = json["workspace_diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["kind"] == "undeclared-workspace")
        .expect("the undeclared workspace is reported")["message"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(
        message.contains("`workspaces.patterns`"),
        "the hint names the fallow config option next to package.json: {message}"
    );

    std::fs::write(
        root.join(".fallowrc.json"),
        r#"{"workspaces":{"patterns":["scripts/gate"]}}"#,
    )
    .unwrap();
    let output = run_fallow_in_root("workspaces", root, &["--format", "json", "--quiet"]);
    let json = parse_json(&output);
    assert!(
        !json["workspace_diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .any(|entry| entry["kind"] == "undeclared-workspace"),
        "the named option clears the diagnostic: {}",
        output.stdout
    );
}
