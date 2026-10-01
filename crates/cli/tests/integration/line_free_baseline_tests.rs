//! Line-free keys for dead-code baselines and the audit new-only gate.
//!
//! A baseline entry and an audit key name what a finding is about, never
//! where it is. A line shift above findings must not make them new, and a
//! new second finding with the same subject must be new.

use std::path::Path;

use serde_json::Value;

use crate::common::{commit_all, copy_fixture, git, parse_json, run_fallow_raw};

const BASIC: &str = "finding-ids-basic";
const SOURCES: &[&str] = &[
    "src/barrel.ts",
    "src/flags.ts",
    "src/index.ts",
    "src/lib.ts",
    "src/orphan.ts",
    "src/utils.ts",
];

fn root_arg(root: &Path) -> &str {
    root.to_str().expect("temp path is UTF-8")
}

fn read(root: &Path, relative: &str) -> String {
    std::fs::read_to_string(root.join(relative)).expect("read fixture file")
}

fn write(root: &Path, relative: &str, contents: &str) {
    std::fs::write(root.join(relative), contents).expect("write fixture file");
}

/// Add an unlisted dependency, so a finding with an import-site line exists.
fn import_unlisted_dependency(root: &Path) {
    let source = read(root, "src/index.ts");
    write(
        root,
        "src/index.ts",
        &format!("import chalk from \"chalk\";\nconsole.log(chalk);\n{source}"),
    );
}

/// Put blank lines and a comment above every finding in every source file.
fn shift_every_source(root: &Path) {
    for file in SOURCES {
        let source = read(root, file);
        write(root, file, &format!("\n\n\n// shifted\n\n{source}"));
    }
}

fn run_json(root: &Path, args: &[&str]) -> Value {
    let mut full = args.to_vec();
    full.extend_from_slice(&[
        "--root",
        root_arg(root),
        "--format",
        "json",
        "--quiet",
        "--no-cache",
    ]);
    let output = run_fallow_raw(&full);
    assert!(
        output.code == 0 || output.code == 1,
        "fallow {args:?} failed with {}: {}",
        output.code,
        output.stderr
    );
    parse_json(&output)
}

fn save_baseline(root: &Path, baseline: &Path) {
    let output = run_fallow_raw(&[
        "dead-code",
        "--root",
        root_arg(root),
        "--quiet",
        "--no-cache",
        "--save-baseline",
        root_arg(baseline),
    ]);
    assert!(baseline.exists(), "no baseline written: {}", output.stderr);
}

fn audit_json(root: &Path) -> Value {
    run_json(root, &["audit", "--base", "main", "--gate", "new-only"])
}

fn introduced_rows(audit: &Value) -> Vec<String> {
    let mut rows = Vec::new();
    let Some(section) = audit["dead_code"].as_object() else {
        return rows;
    };
    for (array, items) in section {
        for item in items.as_array().into_iter().flatten() {
            if item["introduced"] == true {
                rows.push(format!("{array} {item}"));
            }
        }
    }
    rows
}

fn init_repo(root: &Path) {
    git(root, &["init", "-q", "-b", "main"]);
    commit_all(root, "base");
}

#[test]
fn a_saved_baseline_hides_every_finding_after_a_line_shift() {
    let dir = copy_fixture(BASIC);
    import_unlisted_dependency(dir.path());
    let baseline = dir.path().join("fallow-baseline.json");
    save_baseline(dir.path(), &baseline);
    let saved: Value = serde_json::from_str(&read(dir.path(), "fallow-baseline.json")).unwrap();
    assert_eq!(saved["identity"], "dc1");

    shift_every_source(dir.path());
    let before = run_json(dir.path(), &["dead-code"]);
    let after = run_json(
        dir.path(),
        &["dead-code", "--baseline", root_arg(&baseline)],
    );

    assert!(
        before["total_issues"].as_u64().unwrap_or(0) > 0,
        "the fixture must report findings"
    );
    assert_eq!(
        after["total_issues"], 0,
        "a line shift made findings new: {after:#}"
    );
}

#[test]
fn a_legacy_baseline_still_matches_and_is_rewritten_on_save() {
    let dir = copy_fixture(BASIC);
    let baseline = dir.path().join("legacy-baseline.json");
    write(
        dir.path(),
        "legacy-baseline.json",
        r#"{
  "unused_files": ["src/orphan.ts"],
  "unused_exports": ["src/utils.ts:helper"],
  "unused_types": [],
  "unused_dependencies": ["package.json:left-pad"],
  "unused_dev_dependencies": []
}"#,
    );

    let json = run_json(
        dir.path(),
        &["dead-code", "--baseline", root_arg(&baseline)],
    );
    let names: Vec<&str> = json["unused_exports"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|item| item["export_name"].as_str())
        .collect();
    assert_eq!(
        json["unused_files"].as_array().unwrap().as_slice(),
        [] as [serde_json::Value; 0]
    );
    assert_eq!(
        json["unused_dependencies"].as_array().unwrap().as_slice(),
        [] as [serde_json::Value; 0]
    );
    assert!(
        !names.contains(&"helper"),
        "legacy key must still match: {names:?}"
    );
    assert!(
        names.contains(&"unusedFn"),
        "other findings stay: {names:?}"
    );

    save_baseline(dir.path(), &baseline);
    let saved: Value = serde_json::from_str(&read(dir.path(), "legacy-baseline.json")).unwrap();
    assert_eq!(saved["identity"], "dc1");
    assert!(
        saved["unused_exports"]
            .as_array()
            .unwrap()
            .contains(&Value::from("unused-export:src/utils.ts:helper")),
        "{saved:#}"
    );
}

#[test]
fn audit_new_only_keeps_findings_inherited_after_a_line_shift() {
    let dir = copy_fixture(BASIC);
    import_unlisted_dependency(dir.path());
    init_repo(dir.path());
    shift_every_source(dir.path());

    let audit = audit_json(dir.path());

    assert_eq!(
        introduced_rows(&audit),
        Vec::<String>::new(),
        "a line shift made audit findings introduced"
    );
    assert_eq!(audit["attribution"]["dead_code_introduced"], 0);
}

#[test]
fn audit_new_only_reports_a_second_occurrence_of_an_inherited_key() {
    let dir = copy_fixture(BASIC);
    init_repo(dir.path());
    let flags = read(dir.path(), "src/flags.ts");
    write(
        dir.path(),
        "src/flags.ts",
        &format!("{flags}// fallow-ignore-next-line unused-export\nexport const flagC = 3;\n"),
    );
    let index = read(dir.path(), "src/index.ts");
    write(
        dir.path(),
        "src/index.ts",
        &index
            .replace("flagA, flagB }", "flagA, flagB, flagC }")
            .replace("flagA, flagB);", "flagA, flagB, flagC);"),
    );

    let audit = audit_json(dir.path());
    let introduced = audit["dead_code"]["stale_suppressions"]
        .as_array()
        .expect("stale suppressions")
        .iter()
        .filter(|item| item["introduced"] == true)
        .count();

    assert_eq!(
        introduced, 1,
        "the third stale suppression is new: {:#}",
        audit["dead_code"]["stale_suppressions"]
    );
}

#[test]
fn audit_new_only_follows_a_git_rename() {
    let dir = copy_fixture(BASIC);
    init_repo(dir.path());
    git(dir.path(), &["mv", "src/utils.ts", "src/helpers.ts"]);
    let index = read(dir.path(), "src/index.ts");
    write(
        dir.path(),
        "src/index.ts",
        &index.replace("\"./utils\"", "\"./helpers\""),
    );
    let helpers = read(dir.path(), "src/helpers.ts");
    write(dir.path(), "src/helpers.ts", &format!("\n\n{helpers}"));

    let audit = audit_json(dir.path());

    assert_eq!(
        introduced_rows(&audit),
        Vec::<String>::new(),
        "a renamed file made findings introduced"
    );
}

const LEGACY_HINT: &str = "uses the old baseline key format";

fn write_legacy_baseline(root: &Path) -> std::path::PathBuf {
    write(
        root,
        "legacy-baseline.json",
        r#"{
  "unused_files": ["src/orphan.ts"],
  "unused_exports": [],
  "unused_types": [],
  "unused_dependencies": [],
  "unused_dev_dependencies": []
}"#,
    );
    root.join("legacy-baseline.json")
}

fn human_run(root: &Path, baseline: &Path) -> crate::common::CommandOutput {
    let output = run_fallow_raw(&[
        "dead-code",
        "--root",
        root_arg(root),
        "--no-cache",
        "--baseline",
        root_arg(baseline),
    ]);
    assert!(output.code == 0 || output.code == 1, "{}", output.stderr);
    output
}

#[test]
fn a_legacy_baseline_prints_a_hint_in_human_output_and_marks_the_json() {
    let dir = copy_fixture(BASIC);
    let baseline = write_legacy_baseline(dir.path());

    let human = human_run(dir.path(), &baseline);
    assert!(human.stderr.contains(LEGACY_HINT), "{}", human.stderr);
    assert!(human.stderr.contains("--save-baseline"), "{}", human.stderr);

    let json = run_fallow_raw(&[
        "dead-code",
        "--root",
        root_arg(dir.path()),
        "--no-cache",
        "--format",
        "json",
        "--baseline",
        root_arg(&baseline),
    ]);
    assert!(!json.stderr.contains(LEGACY_HINT), "{}", json.stderr);
    assert_eq!(parse_json(&json)["baseline_staleness"]["format"], "legacy");
}

#[test]
fn a_current_baseline_has_no_hint_and_no_format_field() {
    let dir = copy_fixture(BASIC);
    let baseline = dir.path().join("fallow-baseline.json");
    save_baseline(dir.path(), &baseline);

    let human = human_run(dir.path(), &baseline);
    assert!(!human.stderr.contains(LEGACY_HINT), "{}", human.stderr);
    let envelope = run_json(
        dir.path(),
        &["dead-code", "--baseline", root_arg(&baseline)],
    );
    assert!(envelope["baseline_staleness"].is_object(), "{envelope:#}");
    assert!(envelope["baseline_staleness"].get("format").is_none());
}
