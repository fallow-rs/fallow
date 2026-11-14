//! `audit --gate new-only` attribution of an unused file or an unused export
//! outside the changed files.
//!
//! A change can make code unused without a change to that code: it removes
//! the last import of a file or of an export, or a config change removes an
//! entry point. The base did not report that finding, so the audit reports it
//! as introduced. A finding outside the changed files that the base also
//! reported stays out of the report.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests and benches use unwrap and expect to keep fixture setup concise"
)]

use std::fs;
use std::path::Path;

use tempfile::TempDir;

use crate::common::{commit_all, git, parse_json, run_fallow_raw_in_dir};

const BASE_INDEX: &str = "import { used, helper } from \"./lib\";\nimport { keyVault } from \"./keyvault\";\nconsole.log(used(), helper(), keyVault());\n";
const HEAD_INDEX: &str = "import { used } from \"./lib\";\nconsole.log(used());\n";
const LIB: &str = "export const used = (): number => 1;\nexport const helper = (): number => 2;\nexport const stale = (): number => 3;\n";

/// A project with `src/index.ts` as the only entry. At the base, the entry
/// uses `helper` and `src/keyvault.ts`. `src/old.ts` and the `stale` export
/// are unused at the base already.
fn fixture(config: &str) -> TempDir {
    let tmp = TempDir::new().expect("failed to create temp dir");
    let dir = tmp.path();
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::create_dir_all(dir.join("scripts")).unwrap();
    fs::write(
        dir.join("package.json"),
        r#"{"name": "orphan", "version": "1.0.0", "private": true}"#,
    )
    .unwrap();
    fs::write(dir.join(".fallowrc.json"), config).unwrap();
    fs::write(dir.join("src/index.ts"), BASE_INDEX).unwrap();
    fs::write(dir.join("src/lib.ts"), LIB).unwrap();
    fs::write(
        dir.join("src/keyvault.ts"),
        "export const keyVault = (): string => \"kv\";\n",
    )
    .unwrap();
    fs::write(
        dir.join("src/old.ts"),
        "export const old = (): number => 4;\n",
    )
    .unwrap();
    fs::write(
        dir.join("scripts/extra.ts"),
        "export const run = (): number => 5;\nconsole.log(run());\n",
    )
    .unwrap();
    git(dir, &["init", "-b", "main"]);
    git(dir, &["config", "core.autocrlf", "false"]);
    commit_all(dir, "baseline");
    git(dir, &["checkout", "-b", "head"]);
    tmp
}

const ENTRY_INDEX_AND_EXTRA: &str = r#"{"entry": ["src/index.ts", "scripts/extra.ts"]}"#;
const ENTRY_INDEX: &str = r#"{"entry": ["src/index.ts"]}"#;

struct Audit {
    code: i32,
    verdict: String,
    /// `(path, introduced)` of each unused file.
    files: Vec<(String, bool)>,
    /// `(path:name, introduced)` of each unused export.
    exports: Vec<(String, bool)>,
}

fn audit(dir: &Path, extra: &[&str]) -> Audit {
    audit_with_root(dir, dir.to_str().unwrap(), extra)
}

/// Run the audit in the working directory `dir` with `--root root`.
fn audit_with_root(dir: &Path, root: &str, extra: &[&str]) -> Audit {
    let mut args = vec![
        "audit", "--root", root, "--base", "main", "--format", "json", "--quiet", "--no-css",
    ];
    args.extend_from_slice(extra);
    let output = run_fallow_raw_in_dir(dir, &args);
    assert!(
        output.code == 0 || output.code == 1,
        "audit failed. stdout: {}\nstderr: {}",
        output.stdout,
        output.stderr
    );
    let json = parse_json(&output);
    let collect = |key: &str, label: &dyn Fn(&serde_json::Value) -> String| {
        let mut items: Vec<(String, bool)> = json["dead_code"][key]
            .as_array()
            .map(|items| {
                items
                    .iter()
                    .map(|item| (label(item), item["introduced"].as_bool().unwrap_or(false)))
                    .collect()
            })
            .unwrap_or_default();
        items.sort();
        items
    };
    let files = collect("unused_files", &|item| {
        item["path"].as_str().unwrap().replace('\\', "/")
    });
    let exports = collect("unused_exports", &|item| {
        format!(
            "{}:{}",
            item["path"].as_str().unwrap().replace('\\', "/"),
            item["export_name"].as_str().unwrap()
        )
    });
    Audit {
        code: output.code,
        verdict: json["verdict"].as_str().unwrap().to_string(),
        files,
        exports,
    }
}

fn owned(items: &[(&str, bool)]) -> Vec<(String, bool)> {
    items
        .iter()
        .map(|(label, introduced)| ((*label).to_string(), *introduced))
        .collect()
}

#[test]
fn removing_the_last_use_reports_the_orphaned_file_and_export() {
    let tmp = fixture(ENTRY_INDEX_AND_EXTRA);
    fs::write(tmp.path().join("src/index.ts"), HEAD_INDEX).unwrap();
    commit_all(tmp.path(), "drop callers");

    let result = audit(tmp.path(), &["--no-cache"]);

    assert_eq!(result.files, owned(&[("src/keyvault.ts", true)]));
    assert_eq!(result.exports, owned(&[("src/lib.ts:helper", true)]));
    assert_eq!(result.verdict, "fail");
    assert_eq!(result.code, 1);
}

#[test]
fn a_cached_base_snapshot_still_reports_the_orphaned_code() {
    let tmp = fixture(ENTRY_INDEX_AND_EXTRA);
    fs::write(tmp.path().join("src/index.ts"), HEAD_INDEX).unwrap();
    commit_all(tmp.path(), "drop callers");

    let cold = audit(tmp.path(), &[]);
    let warm = audit(tmp.path(), &[]);

    for result in [cold, warm] {
        assert_eq!(result.files, owned(&[("src/keyvault.ts", true)]));
        assert_eq!(result.exports, owned(&[("src/lib.ts:helper", true)]));
        assert_eq!(result.verdict, "fail");
    }
}

#[test]
fn a_config_only_change_reports_the_file_it_made_unreachable() {
    let tmp = fixture(ENTRY_INDEX_AND_EXTRA);
    fs::write(tmp.path().join(".fallowrc.json"), ENTRY_INDEX).unwrap();
    commit_all(tmp.path(), "drop an entry");

    let result = audit(tmp.path(), &["--no-cache"]);

    assert_eq!(result.files, owned(&[("scripts/extra.ts", true)]));
    assert!(result.exports.is_empty(), "{:?}", result.exports);
    assert_eq!(result.verdict, "fail");
    assert_eq!(result.code, 1);
}

#[test]
fn a_cached_base_snapshot_still_reports_the_file_a_config_change_made_unreachable() {
    let tmp = fixture(ENTRY_INDEX_AND_EXTRA);
    fs::write(tmp.path().join(".fallowrc.json"), ENTRY_INDEX).unwrap();
    commit_all(tmp.path(), "drop an entry");

    let cold = audit(tmp.path(), &[]);
    let warm = audit(tmp.path(), &[]);

    for result in [cold, warm] {
        assert_eq!(result.files, owned(&[("scripts/extra.ts", true)]));
        assert_eq!(result.verdict, "fail");
    }
}

#[test]
fn a_config_change_that_keeps_the_entries_orphans_nothing() {
    let tmp = fixture(ENTRY_INDEX_AND_EXTRA);
    fs::write(
        tmp.path().join(".fallowrc.json"),
        r#"{"entry": ["src/index.ts", "scripts/extra.ts"], "ignoreDependencies": ["left-pad"]}"#,
    )
    .unwrap();
    commit_all(tmp.path(), "edit the config");

    let result = audit(tmp.path(), &["--no-cache"]);

    assert!(result.files.is_empty(), "{:?}", result.files);
    assert!(result.exports.is_empty(), "{:?}", result.exports);
    assert_eq!(result.verdict, "pass");
}

#[test]
fn a_scope_path_bounds_the_orphaned_code() {
    let tmp = fixture(ENTRY_INDEX_AND_EXTRA);
    fs::write(tmp.path().join("src/index.ts"), HEAD_INDEX).unwrap();
    commit_all(tmp.path(), "drop callers");
    let scope = tmp.path().join("src/index.ts");

    let result = audit(tmp.path(), &["--no-cache", scope.to_str().unwrap()]);

    assert!(result.files.is_empty(), "{:?}", result.files);
    assert!(result.exports.is_empty(), "{:?}", result.exports);
    assert_eq!(result.verdict, "pass");
}

#[test]
fn a_change_that_orphans_nothing_keeps_the_report_to_the_changed_files() {
    let tmp = fixture(ENTRY_INDEX_AND_EXTRA);
    fs::write(
        tmp.path().join("src/index.ts"),
        format!("{BASE_INDEX}console.log(\"edited\");\n"),
    )
    .unwrap();
    commit_all(tmp.path(), "edit the entry");

    let result = audit(tmp.path(), &["--no-cache"]);

    assert!(result.files.is_empty(), "{:?}", result.files);
    assert!(result.exports.is_empty(), "{:?}", result.exports);
    assert_eq!(result.verdict, "pass");
}

#[test]
fn a_config_change_that_turns_a_rule_on_does_not_gate_old_findings() {
    let tmp = fixture(
        r#"{"entry": ["src/index.ts", "scripts/extra.ts"], "rules": {"unused-files": "off", "unused-exports": "off"}}"#,
    );
    fs::write(tmp.path().join(".fallowrc.json"), ENTRY_INDEX_AND_EXTRA).unwrap();
    commit_all(tmp.path(), "turn the rules on");

    let result = audit(tmp.path(), &["--no-cache"]);

    assert!(result.files.is_empty(), "{:?}", result.files);
    assert!(result.exports.is_empty(), "{:?}", result.exports);
    assert_eq!(result.verdict, "pass");
}

#[test]
fn the_all_gate_keeps_the_report_to_the_changed_files() {
    let tmp = fixture(ENTRY_INDEX_AND_EXTRA);
    fs::write(tmp.path().join("src/index.ts"), HEAD_INDEX).unwrap();
    commit_all(tmp.path(), "drop callers");

    let result = audit(tmp.path(), &["--no-cache", "--gate", "all"]);

    assert!(result.files.is_empty(), "{:?}", result.files);
    assert!(result.exports.is_empty(), "{:?}", result.exports);
}

#[test]
fn a_relative_config_path_still_reports_the_file_a_config_change_made_unreachable() {
    let tmp = fixture(ENTRY_INDEX_AND_EXTRA);
    fs::write(tmp.path().join(".fallowrc.json"), ENTRY_INDEX).unwrap();
    commit_all(tmp.path(), "drop an entry");

    let result = audit_with_root(tmp.path(), ".", &["--no-cache", "-c", ".fallowrc.json"]);

    assert_eq!(result.files, owned(&[("scripts/extra.ts", true)]));
    assert_eq!(result.verdict, "fail");
}

#[test]
fn a_config_path_through_a_symlink_still_reports_the_file_a_config_change_made_unreachable() {
    let tmp = fixture(ENTRY_INDEX_AND_EXTRA);
    fs::write(tmp.path().join(".fallowrc.json"), ENTRY_INDEX).unwrap();
    commit_all(tmp.path(), "drop an entry");
    let link_parent = TempDir::new().expect("failed to create temp dir");
    let link = link_parent.path().join("project");
    #[cfg(unix)]
    std::os::unix::fs::symlink(tmp.path(), &link).unwrap();
    #[cfg(windows)]
    std::os::windows::fs::symlink_dir(tmp.path(), &link).unwrap();
    let config = link.join(".fallowrc.json");

    let result = audit_with_root(
        tmp.path(),
        link.to_str().unwrap(),
        &["--no-cache", "-c", config.to_str().unwrap()],
    );

    assert_eq!(result.files, owned(&[("scripts/extra.ts", true)]));
    assert_eq!(result.verdict, "fail");
}

/// A config change that only stops ignoring code shows unused code that the
/// base already had, so the audit does not count it as introduced.
#[test]
fn a_config_change_that_stops_ignoring_code_does_not_gate_old_findings() {
    let base_configs = [
        r#"{"entry": ["src/index.ts", "scripts/extra.ts"], "ignorePatterns": ["src/old.ts"]}"#,
        r#"{"entry": ["src/index.ts", "scripts/extra.ts"], "ignoreFindings": ["src/old.ts", "src/lib.ts"]}"#,
        r#"{"entry": ["src/index.ts", "scripts/extra.ts"], "ignoreExports": [{"file": "src/lib.ts", "exports": ["stale"]}], "overrides": [{"files": ["src/old.ts"], "rules": {"unused-files": "off"}}]}"#,
    ];
    for base_config in base_configs {
        let tmp = fixture(base_config);
        fs::write(tmp.path().join(".fallowrc.json"), ENTRY_INDEX_AND_EXTRA).unwrap();
        commit_all(tmp.path(), "stop ignoring code");

        let result = audit(tmp.path(), &["--no-cache"]);

        let introduced = |items: &[(String, bool)]| items.iter().any(|(_, new)| *new);
        assert!(
            !introduced(&result.files),
            "{base_config}: {:?}",
            result.files
        );
        assert!(
            !introduced(&result.exports),
            "{base_config}: {:?}",
            result.exports
        );
        assert_eq!(result.verdict, "pass", "{base_config}");
    }
}

#[test]
fn a_config_change_that_stops_ignoring_code_and_drops_an_entry_reports_only_the_entry() {
    let tmp = fixture(
        r#"{"entry": ["src/index.ts", "scripts/extra.ts"], "ignorePatterns": ["src/old.ts"]}"#,
    );
    fs::write(tmp.path().join(".fallowrc.json"), ENTRY_INDEX).unwrap();
    commit_all(tmp.path(), "drop an entry and stop ignoring code");

    let result = audit(tmp.path(), &["--no-cache"]);

    let introduced: Vec<&str> = result
        .files
        .iter()
        .filter(|(_, new)| *new)
        .map(|(path, _)| path.as_str())
        .collect();
    assert_eq!(introduced, ["scripts/extra.ts"]);
    assert_eq!(result.verdict, "fail");
}

/// A base config that ignores the only importer of `src/legacy.ts` leaves
/// that file unreachable. When the change stops ignoring the importer, the
/// unused export of `src/legacy.ts` shows, but that code was dead at the base.
#[test]
fn a_config_change_that_makes_an_old_unused_file_reachable_does_not_gate_its_exports() {
    let tmp = fixture(
        r#"{"entry": ["src/index.ts", "scripts/extra.ts", "scripts/tool.ts"], "ignorePatterns": ["scripts/tool.ts"]}"#,
    );
    let dir = tmp.path();
    git(dir, &["checkout", "main"]);
    fs::write(
        dir.join("src/legacy.ts"),
        "export const one = (): number => 1;\nexport const two = (): number => 2;\n",
    )
    .unwrap();
    fs::write(
        dir.join("scripts/tool.ts"),
        "import { one } from \"../src/legacy\";\nconsole.log(one());\n",
    )
    .unwrap();
    commit_all(dir, "add a tool");
    git(dir, &["checkout", "-B", "head", "main"]);
    fs::write(
        dir.join(".fallowrc.json"),
        r#"{"entry": ["src/index.ts", "scripts/extra.ts", "scripts/tool.ts"]}"#,
    )
    .unwrap();
    commit_all(dir, "stop ignoring the tool");

    let result = audit(dir, &["--no-cache"]);

    let introduced = |items: &[(String, bool)]| items.iter().any(|(_, new)| *new);
    assert!(!introduced(&result.files), "{:?}", result.files);
    assert!(!introduced(&result.exports), "{:?}", result.exports);
    assert_eq!(result.verdict, "pass");
}
