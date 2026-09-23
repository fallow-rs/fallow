#![allow(
    clippy::expect_used,
    reason = "integration tests use expect to keep fixture setup concise"
)]

//! The CI formats state the same gate severity as the exit code.
//!
//! The fixture sets `unused-files` and `unused-exports` to `error` and lowers
//! both to `warn` for `src/legacy/**` through `overrides[].rules`. Each format
//! must show `error` for the findings outside `src/legacy/` and `warn` for the
//! findings inside it, in the direct run and in `fallow report --from`.

#[path = "common/mod.rs"]
mod common;

use std::path::Path;

use common::{CommandOutput, commit_all, git, parse_json, run_fallow_in_root, run_fallow_raw};
use serde_json::Value;

const HELPERS: &str = "src/helpers.ts";
const LEGACY_API: &str = "src/legacy/api.ts";
const ORPHAN: &str = "src/orphan.ts";
const LEGACY_OLD: &str = "src/legacy/old.ts";

const CONFIG: &str = r#"{
  "rules": { "unused-files": "error", "unused-exports": "error" },
  "overrides": [
    {
      "files": ["src/legacy/**"],
      "rules": { "unused-files": "warn", "unused-exports": "warn" }
    }
  ]
}
"#;

fn write(root: &Path, relative: &str, content: &str) {
    let path = root.join(relative);
    std::fs::create_dir_all(path.parent().expect("parent")).expect("create directory");
    std::fs::write(path, content).expect("write fixture file");
}

fn write_base(root: &Path) {
    write(
        root,
        "package.json",
        r#"{"name":"gate-severity","private":true,"main":"src/index.ts"}"#,
    );
    write(root, ".fallowrc.json", CONFIG);
    write(root, "src/index.ts", "export const entry = true;\n");
}

fn write_findings(root: &Path) {
    write(
        root,
        "src/index.ts",
        "import { used } from \"./helpers\";\nimport { legacyUsed } from \"./legacy/api\";\nexport const entry = [used, legacyUsed];\n",
    );
    write(
        root,
        HELPERS,
        "export const used = 1;\nexport const unusedHelper = 2;\n",
    );
    write(
        root,
        LEGACY_API,
        "export const legacyUsed = 1;\nexport const legacyUnused = 2;\n",
    );
    write(root, ORPHAN, "export const orphan = 1;\n");
    write(root, LEGACY_OLD, "export const old = 1;\n");
}

fn project() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("fixture tempdir");
    write_base(dir.path());
    write_findings(dir.path());
    dir
}

fn dead_code(root: &Path, format: &str, extra: &[&str]) -> CommandOutput {
    let mut args = vec!["--quiet", "--format", format];
    args.extend_from_slice(extra);
    let output = run_fallow_in_root("dead-code", root, &args);
    assert!(
        matches!(output.code, 0 | 1),
        "dead-code --format {format} failed: {}",
        output.stderr
    );
    output
}

fn report_from(root: &Path, saved: &Path, format: &str) -> CommandOutput {
    let output = run_fallow_raw(&[
        "report",
        "--from",
        saved.to_str().expect("utf-8 path"),
        "--root",
        root.to_str().expect("utf-8 path"),
        "--quiet",
        "--format",
        format,
    ]);
    assert_eq!(
        output.code, 0,
        "report --from --format {format} failed: {}",
        output.stderr
    );
    output
}

/// The workflow-command level (`error` or `warning`) of the first annotation
/// line whose `file=` property is `path`.
fn annotation_level<'a>(stdout: &'a str, path: &str, title: &str) -> &'a str {
    let needle = format!(" file={path}");
    let line = stdout
        .lines()
        .find(|line| line.contains(&needle) && line.contains(&format!("title={title}")))
        .unwrap_or_else(|| panic!("no {title} annotation for {path} in:\n{stdout}"));
    line.trim_start_matches("::")
        .split(' ')
        .next()
        .expect("annotation level")
}

fn sarif_level(sarif: &Value, rule_id: &str, path: &str) -> String {
    sarif["runs"][0]["results"]
        .as_array()
        .expect("SARIF results")
        .iter()
        .find(|result| {
            result["ruleId"] == rule_id
                && result["locations"][0]["physicalLocation"]["artifactLocation"]["uri"] == path
        })
        .unwrap_or_else(|| panic!("no SARIF {rule_id} result for {path}"))["level"]
        .as_str()
        .expect("SARIF level")
        .to_owned()
}

fn codeclimate_severity(issues: &Value, check_name: &str, path: &str) -> String {
    issues
        .as_array()
        .expect("CodeClimate issues")
        .iter()
        .find(|issue| issue["check_name"] == check_name && issue["location"]["path"] == path)
        .unwrap_or_else(|| panic!("no CodeClimate {check_name} issue for {path}"))["severity"]
        .as_str()
        .expect("CodeClimate severity")
        .to_owned()
}

fn finding<'a>(envelope: &'a Value, key: &str, path: &str) -> &'a Value {
    envelope[key]
        .as_array()
        .unwrap_or_else(|| panic!("{key} array"))
        .iter()
        .find(|item| item["path"] == path)
        .unwrap_or_else(|| panic!("no {key} entry for {path}"))
}

#[test]
fn json_findings_carry_the_effective_severity_after_overrides() {
    let dir = project();
    let envelope = parse_json(&dead_code(dir.path(), "json", &[]));

    assert_eq!(
        finding(&envelope, "unused_exports", HELPERS)["effective_severity"],
        "error"
    );
    assert_eq!(
        finding(&envelope, "unused_exports", LEGACY_API)["effective_severity"],
        "warn"
    );
    assert_eq!(
        finding(&envelope, "unused_files", ORPHAN)["effective_severity"],
        "error"
    );
    assert_eq!(
        finding(&envelope, "unused_files", LEGACY_OLD)["effective_severity"],
        "warn"
    );
}

#[test]
fn github_annotations_follow_the_effective_severity() {
    let dir = project();
    let stdout = dead_code(dir.path(), "github-annotations", &[]).stdout;

    assert_eq!(annotation_level(&stdout, HELPERS, "Unused export"), "error");
    assert_eq!(
        annotation_level(&stdout, LEGACY_API, "Unused export"),
        "warning"
    );
    assert_eq!(annotation_level(&stdout, ORPHAN, "Unused file"), "error");
    assert_eq!(
        annotation_level(&stdout, LEGACY_OLD, "Unused file"),
        "warning"
    );
}

#[test]
fn sarif_levels_follow_the_effective_severity() {
    let dir = project();
    let sarif = parse_json(&dead_code(dir.path(), "sarif", &[]));

    assert_eq!(
        sarif_level(&sarif, "fallow/unused-export", HELPERS),
        "error"
    );
    assert_eq!(
        sarif_level(&sarif, "fallow/unused-export", LEGACY_API),
        "warning"
    );
    assert_eq!(sarif_level(&sarif, "fallow/unused-file", ORPHAN), "error");
    assert_eq!(
        sarif_level(&sarif, "fallow/unused-file", LEGACY_OLD),
        "warning"
    );
}

#[test]
fn codeclimate_severities_follow_the_effective_severity() {
    let dir = project();
    let issues = parse_json(&dead_code(dir.path(), "codeclimate", &[]));

    assert_eq!(
        codeclimate_severity(&issues, "fallow/unused-export", HELPERS),
        "major"
    );
    assert_eq!(
        codeclimate_severity(&issues, "fallow/unused-export", LEGACY_API),
        "minor"
    );
    assert_eq!(
        codeclimate_severity(&issues, "fallow/unused-file", ORPHAN),
        "major"
    );
    assert_eq!(
        codeclimate_severity(&issues, "fallow/unused-file", LEGACY_OLD),
        "minor"
    );
}

fn assert_saved_parity(root: &Path, extra: &[&str]) {
    let json = dead_code(root, "json", extra);
    let saved_dir = tempfile::tempdir().expect("saved report tempdir");
    let saved = saved_dir.path().join("results.json");
    std::fs::write(&saved, &json.stdout).expect("write saved report");

    for format in ["sarif", "codeclimate", "github-annotations"] {
        let direct = dead_code(root, format, extra);
        let rendered = report_from(root, &saved, format);
        assert_eq!(
            rendered.stdout, direct.stdout,
            "report --from --format {format} {extra:?} must be byte-identical to the direct run"
        );
    }
}

#[test]
fn saved_reports_match_the_direct_run_with_a_severity_override() {
    let dir = project();
    assert_saved_parity(dir.path(), &[]);
}

#[test]
fn saved_grouped_reports_match_the_direct_run_with_a_severity_override() {
    let dir = project();
    assert_saved_parity(dir.path(), &["--group-by", "directory"]);
}

#[test]
fn grouped_json_findings_carry_the_effective_severity() {
    let dir = project();
    let envelope = parse_json(&dead_code(dir.path(), "json", &["--group-by", "directory"]));
    let findings: Vec<&Value> = envelope["groups"]
        .as_array()
        .expect("groups array")
        .iter()
        .flat_map(|group| {
            group["unused_exports"]
                .as_array()
                .into_iter()
                .flatten()
                .chain(group["unused_files"].as_array().into_iter().flatten())
        })
        .collect();

    assert!(!findings.is_empty(), "grouped envelope has findings");
    for item in findings {
        let expected = if item["path"]
            .as_str()
            .is_some_and(|path| path.starts_with("src/legacy/"))
        {
            "warn"
        } else {
            "error"
        };
        assert_eq!(item["effective_severity"], expected, "finding {item}");
    }
}

#[test]
fn saved_json_without_the_field_keeps_the_rule_based_levels() {
    let dir = project();
    let mut envelope = parse_json(&dead_code(dir.path(), "json", &[]));
    for key in ["unused_exports", "unused_files"] {
        for item in envelope[key].as_array_mut().expect("findings array") {
            item.as_object_mut()
                .expect("finding object")
                .remove("effective_severity");
        }
    }
    let saved_dir = tempfile::tempdir().expect("saved report tempdir");
    let saved = saved_dir.path().join("results.json");
    std::fs::write(&saved, envelope.to_string()).expect("write saved report");

    let stdout = report_from(dir.path(), &saved, "github-annotations").stdout;
    assert_eq!(
        annotation_level(&stdout, HELPERS, "Unused export"),
        "warning"
    );
    assert_eq!(annotation_level(&stdout, ORPHAN, "Unused file"), "warning");

    let sarif: Value =
        serde_json::from_str(&report_from(dir.path(), &saved, "sarif").stdout).expect("SARIF");
    assert_eq!(
        sarif_level(&sarif, "fallow/unused-export", LEGACY_API),
        "error"
    );
}

#[test]
fn fail_on_issues_raises_every_warn_finding_to_error() {
    let dir = project();
    let stdout = dead_code(dir.path(), "github-annotations", &["--fail-on-issues"]).stdout;

    assert_eq!(
        annotation_level(&stdout, LEGACY_API, "Unused export"),
        "error"
    );
    assert_eq!(
        annotation_level(&stdout, LEGACY_OLD, "Unused file"),
        "error"
    );
}

#[test]
fn audit_and_dead_code_annotations_agree() {
    let dir = tempfile::tempdir().expect("audit tempdir");
    let root = dir.path();
    git(root, &["init", "-q", "-b", "main"]);
    write_base(root);
    commit_all(root, "base");
    write_findings(root);

    let audit = run_fallow_in_root(
        "audit",
        root,
        &[
            "--base",
            "main",
            "--quiet",
            "--format",
            "github-annotations",
        ],
    );
    assert!(
        matches!(audit.code, 0 | 1),
        "audit failed: {}",
        audit.stderr
    );
    let dead = dead_code(root, "github-annotations", &[]);

    for (path, title) in [
        (HELPERS, "Unused export"),
        (LEGACY_API, "Unused export"),
        (ORPHAN, "Unused file"),
        (LEGACY_OLD, "Unused file"),
    ] {
        assert_eq!(
            annotation_level(&audit.stdout, path, title),
            annotation_level(&dead.stdout, path, title),
            "audit and dead-code disagree on {title} in {path}"
        );
    }
    assert_eq!(
        annotation_level(&audit.stdout, HELPERS, "Unused export"),
        "error"
    );
    assert_eq!(
        annotation_level(&audit.stdout, LEGACY_OLD, "Unused file"),
        "warning"
    );
}

#[test]
fn a_severity_change_keeps_baseline_keys_and_fingerprints() {
    let dir = project();
    let root = dir.path();
    let baseline_dir = tempfile::tempdir().expect("baseline tempdir");
    let baseline = baseline_dir.path().join("baseline.json");
    let baseline = baseline.to_str().expect("utf-8 path");
    dead_code(root, "json", &["--save-baseline", baseline]);
    let fingerprints = |sarif: &Value| -> Vec<Value> {
        sarif["runs"][0]["results"]
            .as_array()
            .expect("SARIF results")
            .iter()
            .map(|result| result["partialFingerprints"].clone())
            .collect()
    };
    let before = fingerprints(&parse_json(&dead_code(root, "sarif", &[])));

    write(
        root,
        ".fallowrc.json",
        &CONFIG.replace(
            r#""unused-exports": "warn""#,
            r#""unused-exports": "error""#,
        ),
    );
    let envelope = parse_json(&dead_code(root, "json", &[]));
    assert_eq!(
        finding(&envelope, "unused_exports", LEGACY_API)["effective_severity"],
        "error",
        "the fixture change must raise the legacy export to error"
    );

    let against_baseline = parse_json(&dead_code(root, "json", &["--baseline", baseline]));
    assert_eq!(
        against_baseline["total_issues"], 0,
        "a severity change must not make a baselined finding new"
    );
    let after = fingerprints(&parse_json(&dead_code(root, "sarif", &[])));
    assert_eq!(
        before, after,
        "a severity change must not move SARIF fingerprints"
    );
}

/// A pnpm workspace whose head commit adds an empty catalog group and an
/// override for a package that no workspace depends on. Both findings sit on
/// `pnpm-workspace.yaml`, so an override on that file decides their severity.
fn catalog_project(base: &str, overridden: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("catalog tempdir");
    let root = dir.path();
    write(
        root,
        "package.json",
        r#"{"name":"catalog-gate","private":true,"workspaces":["packages/*"]}"#,
    );
    write(
        root,
        "packages/app/package.json",
        r#"{"name":"app","private":true,"main":"src/index.ts","dependencies":{"vue":"catalog:vue3"}}"#,
    );
    write(
        root,
        "packages/app/src/index.ts",
        "import { ref } from 'vue';\nconsole.log(ref);\n",
    );
    write(
        root,
        "pnpm-workspace.yaml",
        "packages:\n  - 'packages/*'\n\ncatalogs:\n  vue3:\n    vue: ^3.4.0\n",
    );
    write(
        root,
        ".fallowrc.json",
        &format!(
            r#"{{
  "rules": {{ "empty-catalog-groups": "{base}", "unused-dependency-overrides": "{base}" }},
  "overrides": [
    {{
      "files": ["pnpm-workspace.yaml"],
      "rules": {{ "empty-catalog-groups": "{overridden}", "unused-dependency-overrides": "{overridden}" }}
    }}
  ]
}}
"#
        ),
    );
    git(root, &["init", "-q", "-b", "main"]);
    commit_all(root, "base");
    write(
        root,
        "pnpm-workspace.yaml",
        "packages:\n  - 'packages/*'\n\ncatalogs:\n  legacy: {}\n  vue3:\n    vue: ^3.4.0\n\noverrides:\n  axios: ^1.6.0\n",
    );
    dir
}

fn assert_catalog_gate(base: &str, overridden: &str, expected_exit: i32, expected_level: &str) {
    let dir = catalog_project(base, overridden);
    let root = dir.path();
    let audit = run_fallow_in_root(
        "audit",
        root,
        &[
            "--base",
            "main",
            "--quiet",
            "--no-cache",
            "--format",
            "github-annotations",
        ],
    );
    let dead = run_fallow_in_root(
        "dead-code",
        root,
        &["--quiet", "--no-cache", "--format", "github-annotations"],
    );
    for (label, output) in [("audit", &audit), ("dead-code", &dead)] {
        assert_eq!(
            output.code, expected_exit,
            "{label} exit with base {base}, override {overridden}: {}\n{}",
            output.stdout, output.stderr
        );
        for title in ["Empty catalog group", "Unused dependency override"] {
            assert_eq!(
                annotation_level(&output.stdout, "pnpm-workspace.yaml", title),
                expected_level,
                "{label} {title} with base {base}, override {overridden}"
            );
        }
    }
}

#[test]
fn a_per_file_override_raises_catalog_findings_to_error_everywhere() {
    assert_catalog_gate("warn", "error", 1, "error");
}

#[test]
fn a_per_file_override_lowers_catalog_findings_to_warn_everywhere() {
    assert_catalog_gate("error", "warn", 0, "warning");
}

#[test]
fn an_unknown_severity_in_a_saved_report_reads_as_absent() {
    let dir = project();
    let mut envelope = parse_json(&dead_code(dir.path(), "json", &[]));
    for item in envelope["unused_exports"]
        .as_array_mut()
        .expect("findings array")
    {
        item["effective_severity"] = Value::from("info");
    }
    let saved_dir = tempfile::tempdir().expect("saved report tempdir");
    let saved = saved_dir.path().join("results.json");
    std::fs::write(&saved, envelope.to_string()).expect("write saved report");

    let stdout = report_from(dir.path(), &saved, "github-annotations").stdout;
    assert_eq!(
        annotation_level(&stdout, HELPERS, "Unused export"),
        "warning"
    );
    let sarif: Value =
        serde_json::from_str(&report_from(dir.path(), &saved, "sarif").stdout).expect("SARIF");
    assert_eq!(
        sarif_level(&sarif, "fallow/unused-export", LEGACY_API),
        "error",
        "an unknown value falls back to the rule-based level"
    );
}
