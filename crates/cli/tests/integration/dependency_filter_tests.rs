//! `--unused-deps` selects the unused dependency kinds and `--misplaced-deps`
//! selects the placement kinds. Each flag must hold on every output format.

use std::collections::BTreeSet;
use std::path::Path;

use tempfile::TempDir;

use super::common::{parse_json, run_fallow_in_root};

const UNUSED_KEYS: [&str; 3] = [
    "unused_dependencies",
    "unused_dev_dependencies",
    "unused_optional_dependencies",
];
const PLACEMENT_KEYS: [&str; 3] = [
    "type_only_dependencies",
    "test_only_dependencies",
    "dev_dependencies_in_production",
];

fn write(root: &Path, path: &str, content: &str) {
    let target = root.join(path);
    std::fs::create_dir_all(target.parent().expect("file has a parent")).expect("create directory");
    std::fs::write(target, content).expect("write file");
}

/// One package for each of the six dependency kinds.
fn all_kinds_project() -> TempDir {
    let dir = tempfile::tempdir().expect("temporary project");
    let root = dir.path();
    write(
        root,
        "package.json",
        r#"{
  "name": "dependency-filters",
  "private": true,
  "type": "module",
  "main": "src/index.ts",
  "dependencies": {
    "unused-prod": "1.0.0",
    "test-only": "1.0.0",
    "type-only": "1.0.0"
  },
  "devDependencies": {
    "left-pad": "1.3.0",
    "unused-dev": "1.0.0",
    "vitest": "1.0.0"
  },
  "optionalDependencies": { "unused-optional": "1.0.0" }
}
"#,
    );
    write(
        root,
        "src/index.ts",
        "import leftPad from 'left-pad';\nimport type { Shape } from 'type-only';\n\nexport const label: Shape = leftPad('x', 4);\nconsole.log(label);\n",
    );
    write(
        root,
        "src/index.test.ts",
        "import { test } from 'vitest';\nimport helper from 'test-only';\n\ntest('label', () => helper());\n",
    );
    dir
}

/// The case from the issue: a used devDependency and no unused dependency.
fn dev_in_production_project(config: Option<&str>) -> TempDir {
    let dir = tempfile::tempdir().expect("temporary project");
    let root = dir.path();
    write(
        root,
        "package.json",
        r#"{"name":"repro","private":true,"type":"module","main":"src/index.js","devDependencies":{"left-pad":"1.3.0"}}"#,
    );
    write(
        root,
        "src/index.js",
        "import leftPad from 'left-pad';\n\nexport const label = leftPad('x', 4);\nconsole.log(label);\n",
    );
    if let Some(config) = config {
        write(root, ".fallowrc.json", config);
    }
    dir
}

fn package_names(json: &serde_json::Value, keys: &[&str]) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    for key in keys {
        for finding in json[*key].as_array().into_iter().flatten() {
            names.insert(
                finding["package_name"]
                    .as_str()
                    .expect("package_name is a string")
                    .to_owned(),
            );
        }
    }
    names
}

fn selected(root: &Path, flags: &[&str]) -> BTreeSet<String> {
    let mut args = flags.to_vec();
    args.extend(["--format", "json", "--quiet"]);
    let json = parse_json(&run_fallow_in_root("dead-code", root, &args));
    let mut keys = UNUSED_KEYS.to_vec();
    keys.extend(PLACEMENT_KEYS);
    package_names(&json, &keys)
}

fn set(names: &[&str]) -> BTreeSet<String> {
    names.iter().map(|name| (*name).to_owned()).collect()
}

#[test]
fn unused_deps_on_the_issue_repro_reports_nothing_and_exits_zero() {
    let dir = dev_in_production_project(None);
    let output = run_fallow_in_root(
        "dead-code",
        dir.path(),
        &["--unused-deps", "--format", "json", "--quiet"],
    );
    assert_eq!(output.code, 0, "stderr: {}", output.stderr);
    let json = parse_json(&output);
    assert_eq!(json["total_issues"], 0, "stdout: {}", output.stdout);
}

#[test]
fn unused_deps_with_a_gate_no_longer_fails_on_a_used_package() {
    let dir = dev_in_production_project(None);
    let output = run_fallow_in_root(
        "dead-code",
        dir.path(),
        &["--unused-deps", "--fail-on-issues", "--quiet"],
    );
    assert_eq!(output.code, 0, "stderr: {}", output.stderr);
}

#[test]
fn unused_deps_selects_only_the_unused_kinds_in_both_modes() {
    let dir = all_kinds_project();
    assert_eq!(
        selected(dir.path(), &["--unused-deps"]),
        set(&["unused-dev", "unused-optional", "unused-prod"]),
    );
    assert_eq!(
        selected(dir.path(), &["--unused-deps", "--production"]),
        set(&["test-only", "unused-prod"]),
    );
}

#[test]
fn misplaced_deps_selects_the_placement_kinds_for_each_mode() {
    let dir = all_kinds_project();
    let run = |flags: &[&str]| {
        let mut args = flags.to_vec();
        args.extend(["--format", "json", "--quiet"]);
        parse_json(&run_fallow_in_root("dead-code", dir.path(), &args))
    };

    let default_mode = run(&["--misplaced-deps"]);
    assert_eq!(
        package_names(&default_mode, &["test_only_dependencies"]),
        set(&["test-only"])
    );
    assert_eq!(
        package_names(&default_mode, &["dev_dependencies_in_production"]),
        set(&["left-pad"])
    );
    assert_eq!(
        package_names(&default_mode, &["type_only_dependencies"]),
        set(&[])
    );
    assert_eq!(package_names(&default_mode, &UNUSED_KEYS), set(&[]));

    let production = run(&["--misplaced-deps", "--production"]);
    assert_eq!(
        package_names(&production, &["type_only_dependencies"]),
        set(&["type-only"])
    );
    assert_eq!(
        package_names(&production, &["dev_dependencies_in_production"]),
        set(&["left-pad"])
    );
    assert_eq!(
        package_names(&production, &["test_only_dependencies"]),
        set(&[])
    );
    assert_eq!(package_names(&production, &UNUSED_KEYS), set(&[]));
}

#[test]
fn the_two_flags_together_select_the_full_dependency_set_in_both_modes() {
    let dir = all_kinds_project();
    assert_eq!(
        selected(dir.path(), &["--unused-deps", "--misplaced-deps"]),
        set(&[
            "left-pad",
            "test-only",
            "unused-dev",
            "unused-optional",
            "unused-prod"
        ]),
    );
    assert_eq!(
        selected(
            dir.path(),
            &["--unused-deps", "--misplaced-deps", "--production"]
        ),
        set(&["left-pad", "test-only", "type-only", "unused-prod"]),
    );
}

#[test]
fn another_filter_selects_none_of_the_six_kinds() {
    let dir = all_kinds_project();
    // The positive control is the set in the tests above: the project has all six kinds.
    assert_eq!(selected(dir.path(), &["--unused-files"]), set(&[]));
    assert_eq!(selected(dir.path(), &["--unlisted-deps"]), set(&[]));
}

#[test]
fn every_output_format_agrees_for_each_flag() {
    let dir = dev_in_production_project(None);
    let root = dir.path();
    let run = |flag: &str, format: &str| {
        run_fallow_in_root("dead-code", root, &[flag, "--format", format, "--quiet"]).stdout
    };

    for format in ["sarif", "codeclimate", "compact", "human"] {
        let selected = run("--misplaced-deps", format);
        let hidden = run("--unused-deps", format);
        // A SARIF report lists every rule id, so the test looks for the package name.
        assert!(
            selected.contains("left-pad"),
            "{format} under --misplaced-deps: {selected}"
        );
        assert!(
            !hidden.contains("left-pad"),
            "{format} under --unused-deps: {hidden}"
        );
    }
}

const GATE_NOTE: &str = "note: --unused-deps no longer selects misplaced dependencies. 1 hidden finding(s) would fail this gate. Add --misplaced-deps to keep the gate, or set the rule to \"off\" to remove this note.";

#[test]
fn the_note_names_a_gate_that_the_narrower_selection_passes() {
    let dir = dev_in_production_project(None);
    for gate in ["--fail-on-issues", "--ci"] {
        let output = run_fallow_in_root("dead-code", dir.path(), &["--unused-deps", gate]);
        assert_eq!(output.code, 0, "{gate} stderr: {}", output.stderr);
        assert!(
            output.stderr.contains(GATE_NOTE),
            "{gate} stderr: {}",
            output.stderr
        );
    }
}

#[test]
fn the_note_prints_under_quiet_on_stderr_and_keeps_json_clean() {
    let dir = dev_in_production_project(None);
    let output = run_fallow_in_root(
        "dead-code",
        dir.path(),
        &[
            "--unused-deps",
            "--fail-on-issues",
            "--format",
            "json",
            "--quiet",
        ],
    );
    assert!(output.stderr.contains(GATE_NOTE), "{}", output.stderr);
    assert!(!output.stdout.contains("no longer selects"));
    assert_eq!(parse_json(&output)["total_issues"], 0);
}

#[test]
fn the_note_follows_a_placement_rule_at_error() {
    let dir = dev_in_production_project(Some(
        r#"{"rules": {"dev-dependencies-in-production": "error"}}"#,
    ));
    let hidden = run_fallow_in_root("dead-code", dir.path(), &["--unused-deps"]);
    assert_eq!(hidden.code, 0, "stderr: {}", hidden.stderr);
    assert!(hidden.stderr.contains(GATE_NOTE), "{}", hidden.stderr);

    let selected = run_fallow_in_root("dead-code", dir.path(), &["--misplaced-deps"]);
    assert_eq!(selected.code, 1, "stderr: {}", selected.stderr);
}

#[test]
fn the_note_stays_out_of_runs_with_no_changed_exit_code() {
    let dir = dev_in_production_project(None);
    let plain = run_fallow_in_root("dead-code", dir.path(), &["--unused-deps"]);
    assert!(
        !plain.stderr.contains("no longer selects"),
        "{}",
        plain.stderr
    );

    let union = run_fallow_in_root(
        "dead-code",
        dir.path(),
        &["--unused-deps", "--misplaced-deps", "--fail-on-issues"],
    );
    assert_eq!(union.code, 1, "stderr: {}", union.stderr);
    assert!(
        !union.stderr.contains("no longer selects"),
        "{}",
        union.stderr
    );

    let off = dev_in_production_project(Some(
        r#"{"rules": {"dev-dependencies-in-production": "off"}}"#,
    ));
    let rule_off = run_fallow_in_root(
        "dead-code",
        off.path(),
        &["--unused-deps", "--fail-on-issues"],
    );
    assert_eq!(rule_off.code, 0, "stderr: {}", rule_off.stderr);
    assert!(
        !rule_off.stderr.contains("no longer selects"),
        "{}",
        rule_off.stderr
    );
}

#[test]
fn the_note_does_not_count_a_finding_that_the_baseline_absorbs() {
    let dir = dev_in_production_project(None);
    let baseline = dir.path().join("fallow-baseline.json");
    let baseline = baseline.to_str().expect("utf-8 path");
    let saved = run_fallow_in_root(
        "dead-code",
        dir.path(),
        &["--misplaced-deps", "--save-baseline", baseline, "--quiet"],
    );
    assert!(saved.code <= 1, "stderr: {}", saved.stderr);

    // The positive control: the same baseline run with the full set passes the gate.
    let full = run_fallow_in_root(
        "dead-code",
        dir.path(),
        &[
            "--misplaced-deps",
            "--baseline",
            baseline,
            "--fail-on-issues",
        ],
    );
    assert_eq!(full.code, 0, "stderr: {}", full.stderr);

    let narrowed = run_fallow_in_root(
        "dead-code",
        dir.path(),
        &["--unused-deps", "--baseline", baseline, "--fail-on-issues"],
    );
    assert!(
        !narrowed.stderr.contains("no longer selects"),
        "{}",
        narrowed.stderr
    );
}

#[test]
fn an_unmatched_ignore_dependencies_glob_is_reported_under_misplaced_deps() {
    let dir = dev_in_production_project(Some(r#"{"ignoreDependencies": ["@acm/*"]}"#));
    let output = run_fallow_in_root(
        "dead-code",
        dir.path(),
        &["--misplaced-deps", "--format", "json", "--quiet"],
    );
    assert!(
        output.stdout.contains("ignore-dependencies-glob-unmatched"),
        "stdout: {}",
        output.stdout
    );
}

#[test]
fn explain_resolves_the_misplaced_deps_selector() {
    let dir = dev_in_production_project(None);
    let output = run_fallow_in_root(
        "explain",
        dir.path(),
        &["misplaced-deps", "--format", "json", "--quiet"],
    );
    assert_eq!(output.code, 0, "stderr: {}", output.stderr);
    assert_eq!(
        parse_json(&output)["id"],
        "fallow/dev-dependency-in-production"
    );
}
