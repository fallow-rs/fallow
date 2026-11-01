//! `override-rule-ineffective`: the `overrides` rules that cannot change a
//! result (issue #4065).
//!
//! Each table row runs the rule three ways. A positive control proves that
//! the fixture has the finding. The override run proves that the override
//! changes nothing and that fallow reports it. The remedy run proves that the
//! mechanism that the message names clears the finding.

use std::path::Path;

use serde_json::{Value, json};

use crate::common::{CommandOutput, commit_all, git, parse_json, run_fallow_raw};

fn write(root: &Path, path: &str, content: &str) {
    let path = root.join(path);
    std::fs::create_dir_all(path.parent().expect("parent")).expect("create directory");
    std::fs::write(path, content).expect("write file");
}

fn set_config(root: &Path, config: &Value) {
    write(root, ".fallowrc.json", &config.to_string());
}

fn run(root: &Path, args: &[&str]) -> CommandOutput {
    let root = root.to_string_lossy();
    let mut all = vec![args[0], "--root", root.as_ref()];
    all.extend_from_slice(&args[1..]);
    run_fallow_raw(&all)
}

fn json_run(root: &Path, extra: &[&str]) -> Value {
    let mut args = vec!["dead-code", "--format", "json", "--quiet"];
    args.extend_from_slice(extra);
    parse_json(&run(root, &args))
}

fn count(envelope: &Value, key: &str) -> usize {
    envelope[key].as_array().map_or(0, Vec::len)
}

/// `(rule, files, reason)` of each `override-rule-ineffective` entry.
fn entries(envelope: &Value) -> Vec<(String, Vec<String>, String)> {
    envelope["workspace_diagnostics"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|diagnostic| diagnostic["kind"] == "override-rule-ineffective")
        .map(|diagnostic| {
            (
                diagnostic["rule"].as_str().expect("rule").to_owned(),
                diagnostic["files"]
                    .as_array()
                    .expect("files")
                    .iter()
                    .map(|file| file.as_str().expect("glob").to_owned())
                    .collect(),
                diagnostic["reason"].as_str().expect("reason").to_owned(),
            )
        })
        .collect()
}

fn entry(rule: &str, glob: &str, reason: &str) -> (String, Vec<String>, String) {
    (rule.to_owned(), vec![glob.to_owned()], reason.to_owned())
}

fn override_off(rule: &str, glob: &str) -> Value {
    json!({ "files": [glob], "rules": { rule: "off" } })
}

fn issue_project() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("temporary project");
    let root = dir.path();
    write(
        root,
        "package.json",
        r#"{"name":"repro","private":true,"type":"module","devDependencies":{"left-pad":"1.3.0"}}"#,
    );
    write(root, "src/index.js", "export const app = 1;\n");
    write(
        root,
        "tools/report.js",
        "import leftPad from 'left-pad';\n\nconsole.log(leftPad('x', 4));\n",
    );
    write(
        root,
        "node_modules/left-pad/package.json",
        r#"{"name":"left-pad","version":"1.3.0"}"#,
    );
    set_config(
        root,
        &json!({
            "entry": ["src/index.js", "tools/*.js"],
            "rules": { "dev-dependencies-in-production": "error" },
            "overrides": [override_off("dev-dependencies-in-production", "tools/**")],
        }),
    );
    dir
}

#[test]
fn issue_repro_reports_the_override_and_keeps_the_finding() {
    let dir = issue_project();
    let output = run(dir.path(), &["dead-code", "--format", "json", "--quiet"]);
    assert_eq!(output.code, 1, "the finding still fails: {}", output.stdout);
    let envelope = parse_json(&output);
    assert_eq!(count(&envelope, "dev_dependencies_in_production"), 1);
    assert_eq!(
        entries(&envelope),
        vec![entry(
            "dev-dependencies-in-production",
            "tools/**",
            "project-level"
        )]
    );
    let diagnostic = envelope["workspace_diagnostics"]
        .as_array()
        .expect("diagnostics")
        .iter()
        .find(|diagnostic| diagnostic["kind"] == "override-rule-ineffective")
        .expect("entry");
    assert_eq!(diagnostic["path"], ".");
    assert_ne!(diagnostic["degrades_analysis"], true);
    assert_eq!(
        diagnostic["message"],
        "overrides.rules.dev-dependencies-in-production has no effect for files matching \
         [tools/**]: dev-dependencies-in-production is a project-level rule, so fallow reads \
         only the top-level severity. To hide one package, add it to `ignoreDependencies`. That \
         entry applies to the whole project. To change the rule for all packages, set \
         `rules.dev-dependencies-in-production`."
    );
}

#[test]
fn issue_repro_prints_one_note_in_human_output() {
    let dir = issue_project();
    let output = run(dir.path(), &["dead-code"]);
    assert_eq!(output.code, 1);
    let notes: Vec<&str> = output
        .stderr
        .lines()
        .filter(|line| line.contains("had no effect") || line.contains("has no effect"))
        .collect();
    assert_eq!(
        notes,
        vec![
            "Note: override rule had no effect: dev-dependencies-in-production for [tools/**] \
             (project-level rule; add the package to ignoreDependencies, or set the top-level \
             rule)."
        ],
        "{}",
        output.stderr
    );
}

/// One finding for each project-level rule. `--production` shows
/// `type-only-dependencies`, so that row has its own test.
fn project_level_fixture() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("temporary project");
    let root = dir.path();
    write(
        root,
        "package.json",
        r#"{"name":"r","private":true,"type":"module","main":"src/index.ts",
            "dependencies":{"un-prod":"1.0.0","only-type":"1.0.0","only-test":"1.0.0"},
            "devDependencies":{"un-dev":"1.0.0","dev-in-prod":"1.0.0"},
            "optionalDependencies":{"un-opt":"1.0.0"}}"#,
    );
    write(
        root,
        "pnpm-workspace.yaml",
        "packages:\n  - .\ncatalog:\n  unused-cat: 1.0.0\n",
    );
    for package in [
        "un-prod",
        "only-type",
        "only-test",
        "un-dev",
        "dev-in-prod",
        "un-opt",
    ] {
        write(
            root,
            &format!("node_modules/{package}/package.json"),
            &format!(r#"{{"name":"{package}","version":"1.0.0"}}"#),
        );
    }
    write(
        root,
        "src/index.ts",
        "export * from './b1.js';\nimport './a/x.js'; import './a/y.js'; import './t.ts';\n\
         import d from 'dev-in-prod'; import l from 'not-listed'; console.log(d, l);\n",
    );
    write(
        root,
        "src/t.ts",
        "import type { T } from 'only-type'; export type U = T;\n",
    );
    write(
        root,
        "src/index.test.ts",
        "import t from 'only-test'; import { it } from 'node:test'; console.log(t, it);\n",
    );
    write(root, "src/b1.js", "export * from './b2.js';\n");
    write(root, "src/b2.js", "export * from './b1.js';\n");
    write(root, "src/a/x.js", "export const dup = 1;\n");
    write(root, "src/a/y.js", "export const dup = 2;\n");
    dir
}

/// A top-level `error` plus an override `off` keeps the finding and reports
/// one `project-level` entry. Setting the top-level rule to `off` clears it.
fn assert_project_level_row(root: &Path, rule: &str, key: &str, flags: &[&str]) {
    set_config(root, &json!({ "rules": { rule: "error" } }));
    let control = json_run(root, flags);
    assert_eq!(count(&control, key), 1, "control for {rule}: {control}");
    assert!(entries(&control).is_empty(), "{rule}: {control}");

    set_config(
        root,
        &json!({
            "rules": { rule: "error" },
            "overrides": [override_off(rule, "**")],
        }),
    );
    let overridden = json_run(root, flags);
    assert_eq!(count(&overridden, key), 1, "{rule}: {overridden}");
    assert_eq!(
        entries(&overridden),
        vec![entry(rule, "**", "project-level")],
        "{rule}"
    );

    set_config(root, &json!({ "rules": { rule: "off" } }));
    assert_eq!(count(&json_run(root, flags), key), 0, "{rule} top level");
}

#[test]
fn ignore_dependencies_clears_each_rule_that_names_it() {
    let dir = project_level_fixture();
    let root = dir.path();
    for (rule, key, package) in [
        ("unused-dependencies", "unused_dependencies", "un-prod"),
        (
            "unused-dev-dependencies",
            "unused_dev_dependencies",
            "un-dev",
        ),
        (
            "unused-optional-dependencies",
            "unused_optional_dependencies",
            "un-opt",
        ),
        (
            "unlisted-dependencies",
            "unlisted_dependencies",
            "not-listed",
        ),
        (
            "test-only-dependencies",
            "test_only_dependencies",
            "only-test",
        ),
        (
            "dev-dependencies-in-production",
            "dev_dependencies_in_production",
            "dev-in-prod",
        ),
    ] {
        assert_project_level_row(root, rule, key, &[]);
        set_config(
            root,
            &json!({ "rules": { rule: "error" }, "ignoreDependencies": [package] }),
        );
        let envelope = json_run(root, &[]);
        assert_eq!(count(&envelope, key), 0, "{rule}: {envelope}");
    }
}

/// `ignoreDependencies` clears this finding only with `--production`, and a
/// normal run then calls the entry redundant. The message names a mechanism
/// that works in both runs.
#[test]
fn moving_the_package_clears_type_only_dependencies() {
    let dir = project_level_fixture();
    let root = dir.path();
    let rule = "type-only-dependencies";
    let key = "type_only_dependencies";
    assert_project_level_row(root, rule, key, &["--production"]);

    set_config(
        root,
        &json!({
            "rules": { rule: "error" },
            "overrides": [override_off(rule, "**")],
        }),
    );
    let message = json_run(root, &["--production"])["workspace_diagnostics"]
        .as_array()
        .expect("diagnostics")
        .iter()
        .find(|diagnostic| diagnostic["kind"] == "override-rule-ineffective")
        .expect("entry")["message"]
        .as_str()
        .expect("message")
        .to_owned();
    assert_eq!(
        message,
        "overrides.rules.type-only-dependencies has no effect for files matching [**]: \
         type-only-dependencies is a project-level rule, so fallow reads only the top-level \
         severity. Move the package to `devDependencies`, or set \
         `rules.type-only-dependencies`."
    );

    set_config(root, &json!({ "rules": { rule: "error" } }));
    write(
        root,
        "package.json",
        r#"{"name":"r","private":true,"type":"module","main":"src/index.ts",
            "devDependencies":{"only-type":"1.0.0"}}"#,
    );
    let moved = json_run(root, &["--production"]);
    assert_eq!(count(&moved, key), 0, "{moved}");
}

#[test]
fn inline_comment_clears_unused_catalog_entries() {
    let dir = project_level_fixture();
    let root = dir.path();
    let rule = "unused-catalog-entries";
    let key = "unused_catalog_entries";
    assert_project_level_row(root, rule, key, &[]);

    set_config(
        root,
        &json!({
            "rules": { rule: "error" },
            "overrides": [override_off(rule, "pnpm-workspace.yaml")],
        }),
    );
    let envelope = json_run(root, &[]);
    assert_eq!(count(&envelope, key), 1, "{envelope}");
    let message = envelope["workspace_diagnostics"]
        .as_array()
        .expect("diagnostics")
        .iter()
        .find(|diagnostic| diagnostic["kind"] == "override-rule-ineffective")
        .expect("entry")["message"]
        .as_str()
        .expect("message");
    assert!(
        message.contains("add `# fallow-ignore-next-line unused-catalog-entry` above the entry"),
        "{message}"
    );
    assert!(!message.contains("ignoreDependencies"), "{message}");

    set_config(root, &json!({ "rules": { rule: "error" } }));
    write(
        root,
        "pnpm-workspace.yaml",
        "packages:\n  - .\ncatalog:\n  # fallow-ignore-next-line unused-catalog-entry\n  unused-cat: 1.0.0\n",
    );
    let suppressed = json_run(root, &[]);
    assert_eq!(count(&suppressed, key), 0, "{suppressed}");
}

#[test]
fn ignore_exports_clears_duplicate_exports() {
    let dir = project_level_fixture();
    let root = dir.path();
    let rule = "duplicate-exports";
    let key = "duplicate_exports";
    assert_project_level_row(root, rule, key, &[]);

    set_config(
        root,
        &json!({
            "rules": { rule: "error" },
            "ignoreExports": [{ "file": "src/a/*", "exports": ["dup"] }],
        }),
    );
    let envelope = json_run(root, &[]);
    assert_eq!(count(&envelope, key), 0, "{envelope}");
    assert!(
        !envelope["unused_exports"].to_string().contains("\"dup\""),
        "the message states this side effect: {envelope}"
    );
}

#[test]
fn file_comment_clears_re_export_cycle() {
    let dir = project_level_fixture();
    let root = dir.path();
    let rule = "re-export-cycle";
    let key = "re_export_cycles";
    assert_project_level_row(root, rule, key, &[]);

    set_config(root, &json!({ "rules": { rule: "error" } }));
    write(
        root,
        "src/b1.js",
        "// fallow-ignore-file re-export-cycle\nexport * from './b2.js';\n",
    );
    let envelope = json_run(root, &[]);
    assert_eq!(count(&envelope, key), 0, "{envelope}");
}

/// One finding for each rule whose detector needs the top-level rule on.
fn base_off_fixture() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("temporary project");
    let root = dir.path();
    write(
        root,
        "package.json",
        r#"{"name":"r","private":true,"type":"module","main":"src/index.ts"}"#,
    );
    write(
        root,
        "src/leak.ts",
        "type Hidden = { a: number };\nexport const leak = (): Hidden => ({ a: 1 });\n",
    );
    write(root, "src/orphan.ts", "export const z = 1;\n");
    write(
        root,
        "src/m.ts",
        "export enum E { A, B }\n\
         export class C { a() { return 1; } b() { return 2; } }\n\
         export const used = 1;\n\
         export const dead = 2;\n\
         export type DeadT = string;\n\
         /** @deprecated use other */\n\
         export const D = 3;\n",
    );
    write(root, "src/c1.ts", "import './c2.ts'; export const a = 1;\n");
    write(root, "src/c2.ts", "import './c1.ts'; export const b = 1;\n");
    write(
        root,
        "src/index.ts",
        "import { E, C, used, D } from './m.ts';\n\
         import { leak } from './leak.ts';\n\
         import './c1.ts';\n\
         import './missing-file.ts';\n\
         export const main = () => [E.A, new C().a(), used, D, leak()];\n",
    );
    dir
}

const BASE_OFF_RULES: [(&str, &str); 9] = [
    ("unused-files", "unused_files"),
    ("unused-exports", "unused_exports"),
    ("unused-types", "unused_types"),
    ("private-type-leaks", "private_type_leaks"),
    ("unused-enum-members", "unused_enum_members"),
    ("unused-class-members", "unused_class_members"),
    ("unresolved-imports", "unresolved_imports"),
    ("circular-dependencies", "circular_dependencies"),
    ("deprecated-exports-in-use", "deprecated_exports_in_use"),
];

#[test]
fn a_raised_override_of_a_top_level_off_rule_is_reported_and_the_remedy_works() {
    let dir = base_off_fixture();
    let root = dir.path();
    for (rule, key) in BASE_OFF_RULES {
        set_config(root, &json!({ "rules": { rule: "error" } }));
        let control = json_run(root, &[]);
        assert!(count(&control, key) >= 1, "control for {rule}: {control}");
        assert!(entries(&control).is_empty(), "{rule}: {control}");

        for severity in ["error", "warn"] {
            set_config(
                root,
                &json!({
                    "rules": { rule: "off" },
                    "overrides": [{ "files": ["src/**"], "rules": { rule: severity } }],
                }),
            );
            let raised = json_run(root, &[]);
            assert_eq!(count(&raised, key), 0, "{rule} {severity}: {raised}");
            assert_eq!(
                entries(&raised),
                vec![entry(rule, "src/**", "base-rule-off")],
                "{rule} {severity}"
            );
        }

        // The remedy: the top-level rule on, and `off` for the other paths.
        set_config(
            root,
            &json!({
                "rules": { rule: "error" },
                "overrides": [override_off(rule, "src/**")],
            }),
        );
        let scoped_off = json_run(root, &[]);
        assert_eq!(count(&scoped_off, key), 0, "{rule}: {scoped_off}");
        assert!(entries(&scoped_off).is_empty(), "{rule}: {scoped_off}");

        set_config(
            root,
            &json!({
                "rules": { rule: "error" },
                "overrides": [override_off(rule, "other/**")],
            }),
        );
        let other_path = json_run(root, &[]);
        assert!(count(&other_path, key) >= 1, "{rule}: {other_path}");
        assert!(entries(&other_path).is_empty(), "{rule}: {other_path}");
    }
}

/// A flag starts the detector after config load, so the override then works.
/// A static entry would be false in these runs.
#[test]
fn an_opt_in_flag_keeps_the_override_and_reports_no_entry() {
    let dir = base_off_fixture();
    let root = dir.path();
    for (rule, key, flag, severity) in [
        (
            "private-type-leaks",
            "private_type_leaks",
            "--private-type-leaks",
            "error",
        ),
        (
            "deprecated-exports-in-use",
            "deprecated_exports_in_use",
            "--deprecated-exports-in-use",
            "warn",
        ),
    ] {
        set_config(
            root,
            &json!({
                "rules": { rule: "off" },
                "overrides": [{ "files": ["src/**"], "rules": { rule: severity } }],
            }),
        );
        let without_flag = json_run(root, &[]);
        assert_eq!(
            entries(&without_flag),
            vec![entry(rule, "src/**", "base-rule-off")],
            "control for {flag}"
        );
        let with_flag = json_run(root, &[flag]);
        assert!(entries(&with_flag).is_empty(), "{flag}: {with_flag}");
        let findings = with_flag[key].as_array().expect("findings");
        assert_eq!(findings.len(), 1, "{flag}: {with_flag}");
        assert_eq!(findings[0]["effective_severity"], severity, "{flag}");
    }
}

/// An override that does work reports nothing. The `duplicate-exports` row is
/// the positive control in the same run.
#[test]
fn an_override_with_an_effect_reports_no_entry() {
    let dir = project_level_fixture();
    let root = dir.path();
    set_config(
        root,
        &json!({
            "rules": { "unused-exports": "error", "circular-dependencies": "error" },
            "overrides": [{
                "files": ["src/**"],
                "rules": {
                    "unused-exports": "off",
                    "circular-dependencies": "off",
                    "package-cycle": "off",
                    "duplicate-exports": "warn",
                },
            }],
        }),
    );
    assert_eq!(
        entries(&json_run(root, &[])),
        vec![entry("duplicate-exports", "src/**", "project-level")]
    );
}

#[test]
fn an_alias_key_reports_the_canonical_rule_name() {
    let dir = issue_project();
    let root = dir.path();
    set_config(
        root,
        &json!({
            "entry": ["src/index.js", "tools/*.js"],
            "overrides": [override_off("dev-dependency-in-production", "tools/**")],
        }),
    );
    assert_eq!(
        entries(&json_run(root, &[])),
        vec![entry(
            "dev-dependencies-in-production",
            "tools/**",
            "project-level"
        )]
    );
}

#[test]
fn equal_entries_report_once_and_keep_config_order() {
    let dir = project_level_fixture();
    let root = dir.path();
    set_config(
        root,
        &json!({
            "overrides": [
                override_off("re-export-cycle", "src/**"),
                override_off("duplicate-exports", "src/**"),
                override_off("re-export-cycle", "src/**"),
                override_off("re-export-cycle", "lib/**"),
            ],
        }),
    );
    assert_eq!(
        entries(&json_run(root, &[])),
        vec![
            entry("re-export-cycle", "src/**", "project-level"),
            entry("duplicate-exports", "src/**", "project-level"),
            entry("re-export-cycle", "lib/**", "project-level"),
        ]
    );
}

#[test]
fn a_run_without_dependency_findings_omits_the_dependency_rows() {
    let dir = project_level_fixture();
    let root = dir.path();
    set_config(
        root,
        &json!({
            "overrides": [
                override_off("unused-dependencies", "**"),
                override_off("type-only-dependencies", "**"),
                override_off("duplicate-exports", "**"),
            ],
        }),
    );
    let all = [
        entry("unused-dependencies", "**", "project-level"),
        entry("type-only-dependencies", "**", "project-level"),
        entry("duplicate-exports", "**", "project-level"),
    ];
    assert_eq!(entries(&json_run(root, &[])), all);
    assert_eq!(entries(&json_run(root, &["--production"])), all);
    assert_eq!(entries(&json_run(root, &["--unused-deps"])), all);
    assert_eq!(
        entries(&json_run(root, &["--unused-exports"])),
        all[2..].to_vec()
    );
    assert_eq!(
        entries(&json_run(root, &["--file", "src/index.ts"])),
        all[2..].to_vec()
    );
}

/// The old warning fired at config resolve, in every command. It must not
/// print next to the new entry.
#[test]
fn the_old_config_warning_is_gone() {
    let dir = project_level_fixture();
    let root = dir.path();
    set_config(
        root,
        &json!({
            "overrides": [
                override_off("duplicate-exports", "src/**"),
                override_off("re-export-cycle", "src/**"),
            ],
        }),
    );
    let output = run(root, &["dead-code"]);
    assert!(
        !output.stderr.contains("is an inter-file rule"),
        "{}",
        output.stderr
    );
    let notes = output
        .stderr
        .lines()
        .filter(|line| line.starts_with("Note: override rule had no effect"))
        .count();
    assert_eq!(notes, 2, "{}", output.stderr);
}

const RULE: &str = "duplicate-exports";
const GLOB: &str = "src/**";
const NOTE: &str = "Note: override rule had no effect: duplicate-exports for [src/**] \
     (project-level rule; add the file and export names to ignoreExports).";
const MARKDOWN_LINE: &str = "- `duplicate-exports` for `src/**`: project-level rule; add the file \
     and export names to ignoreExports";

fn parity_project() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("temporary project");
    let root = dir.path();
    write(
        root,
        "package.json",
        r#"{"name":"parity","private":true,"main":"src/index.ts"}"#,
    );
    write(
        root,
        "src/index.ts",
        "import './a.ts'; import './b.ts';\nexport const main = 1;\n",
    );
    write(root, "src/a.ts", "export const dup = 1;\n");
    write(root, "src/b.ts", "export const dup = 2;\n");
    set_config(root, &json!({ "overrides": [override_off(RULE, GLOB)] }));
    git(root, &["init", "-q"]);
    commit_all(root, "initial");
    write(
        root,
        "src/a.ts",
        "export const dup = 1;\nexport const second = 2;\n",
    );
    dir
}

fn assert_carried(output: &CommandOutput, format: &str, label: &str) {
    let context = format!(
        "{label} --format {format}\nstdout: {}\nstderr: {}",
        output.stdout, output.stderr
    );
    let any_note = output.stderr.contains("Note: override rule");
    match format {
        "json" => {
            let envelope = parse_json(output);
            let diagnostics = envelope
                .pointer("/dead_code/workspace_diagnostics")
                .or_else(|| envelope.get("workspace_diagnostics"))
                .expect("diagnostics");
            let found = diagnostics
                .as_array()
                .expect("array")
                .iter()
                .filter(|diagnostic| diagnostic["kind"] == "override-rule-ineffective")
                .count();
            assert_eq!(found, 1, "{context}");
            assert!(!any_note, "{context}");
        }
        "sarif" => {
            let sarif = parse_json(output);
            let notification = sarif["runs"]
                .as_array()
                .expect("runs")
                .iter()
                .filter_map(|run| run.pointer("/invocations/0/toolConfigurationNotifications"))
                .flat_map(|notifications| notifications.as_array().expect("array").iter())
                .find(|notification| {
                    notification["descriptor"]["id"] == "override-rule-ineffective"
                })
                .unwrap_or_else(|| panic!("no notification: {context}"));
            assert_eq!(notification["level"], "note", "{context}");
            assert_eq!(
                notification["properties"],
                json!({
                    "setting": "overrides",
                    "rule": RULE,
                    "files": [GLOB],
                    "reason": "project-level",
                }),
                "{context}"
            );
            assert!(!any_note, "{context}");
        }
        "markdown" | "github-summary" | "pr-comment-github" | "pr-comment-gitlab" => {
            assert!(
                output.stdout.contains("## Override rules with no effect"),
                "{context}"
            );
            assert!(output.stdout.contains(MARKDOWN_LINE), "{context}");
            assert!(
                !output.stdout.contains("Redundant config entries"),
                "{context}"
            );
            assert!(!any_note, "{context}");
        }
        "review-github" | "review-gitlab" => {
            let envelope = parse_json(output);
            let body = envelope["body"].as_str().unwrap_or_default();
            assert!(
                body.contains("## Override rules with no effect"),
                "{context}"
            );
            assert!(body.contains(MARKDOWN_LINE), "{context}");
            assert!(!any_note, "{context}");
        }
        _ => {
            assert!(output.stderr.contains(NOTE), "{context}");
            assert!(
                !output.stdout.contains("override rule had no effect"),
                "the note stays off stdout: {context}"
            );
        }
    }
}

fn command_args<'a>(command: &'a str, root: &'a str, format: &'a str) -> Vec<&'a str> {
    let mut args = match command {
        "combined" => Vec::new(),
        "audit" => vec!["audit", "--base", "HEAD"],
        other => vec![other],
    };
    args.extend(["--root", root, "--format", format]);
    args
}

#[test]
fn the_entry_reaches_every_live_format_of_every_command() {
    let dir = parity_project();
    let root = dir.path().to_string_lossy().into_owned();
    std::thread::scope(|scope| {
        for command in ["dead-code", "combined", "audit"] {
            let root = &root;
            scope.spawn(move || {
                let formats: &[&str] = if command == "dead-code" {
                    &[
                        "human",
                        "json",
                        "sarif",
                        "markdown",
                        "compact",
                        "codeclimate",
                        "github-annotations",
                        "github-summary",
                        "pr-comment-github",
                        "pr-comment-gitlab",
                        "review-github",
                        "review-gitlab",
                    ]
                } else {
                    &[
                        "human",
                        "json",
                        "sarif",
                        "codeclimate",
                        "github-annotations",
                        "github-summary",
                        "pr-comment-github",
                        "review-github",
                        "review-gitlab",
                    ]
                };
                for format in formats {
                    let output = run_fallow_raw(&command_args(command, root, format));
                    assert_carried(&output, format, command);
                }
            });
        }
    });
}

#[test]
fn the_entry_reaches_every_format_of_report_from() {
    let dir = parity_project();
    let root = dir.path().to_string_lossy().into_owned();
    for command in ["dead-code", "combined", "audit"] {
        let saved = run_fallow_raw(&command_args(command, &root, "json"));
        assert_carried(&saved, "json", command);
        let saved_path = dir.path().join(format!("{command}.json"));
        std::fs::write(&saved_path, &saved.stdout).expect("write saved envelope");
        let saved_path = saved_path.to_string_lossy().into_owned();
        let markdown: &[&str] = if command == "audit" {
            &[]
        } else {
            &["markdown"]
        };
        for format in [
            "sarif",
            "codeclimate",
            "github-annotations",
            "github-summary",
            "pr-comment-github",
            "pr-comment-gitlab",
            "review-github",
            "review-gitlab",
        ]
        .iter()
        .chain(markdown)
        {
            let output = run_fallow_raw(&[
                "report",
                "--from",
                &saved_path,
                "--root",
                &root,
                "--format",
                format,
            ]);
            assert_carried(&output, format, &format!("report --from {command}"));
        }
    }
}
