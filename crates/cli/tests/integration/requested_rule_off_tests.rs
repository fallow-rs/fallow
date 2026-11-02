//! `requested-rule-off`: an explicit issue-type flag asks for a rule that the
//! config sets to `off`, so the requested check does not run (issue #4067).
//!
//! The note never changes a finding or an exit code. Each "no entry" test also
//! has a positive control, because a build without the note reports no entry
//! on every input.

use std::path::Path;

use serde_json::{Value, json};

use crate::common::{CommandOutput, parse_json, run_fallow_raw};

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

fn json_run(root: &Path, args: &[&str]) -> Value {
    let mut all = args.to_vec();
    all.extend_from_slice(&["--format", "json", "--quiet"]);
    parse_json(&run(root, &all))
}

/// `(rule, flag)` of each `requested-rule-off` entry.
fn entries(envelope: &Value) -> Vec<(String, String)> {
    envelope["workspace_diagnostics"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|diagnostic| diagnostic["kind"] == "requested-rule-off")
        .map(|diagnostic| {
            (
                diagnostic["rule"].as_str().expect("rule").to_owned(),
                diagnostic["flag"].as_str().expect("flag").to_owned(),
            )
        })
        .collect()
}

fn entry(rule: &str, flag: &str) -> (String, String) {
    (rule.to_owned(), flag.to_owned())
}

const NOTE: &str = "Note: unused-dependencies is off in config, so --unused-deps did not check \
                    it. Set the rule to \"error\" or \"warn\" to run this check.";

/// The project of the issue: `left-pad` is declared and nothing imports it.
fn issue_project() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("temporary project");
    let root = dir.path();
    write(
        root,
        "package.json",
        r#"{"name":"repro","private":true,"type":"module","main":"src/index.js",
            "dependencies":{"left-pad":"1.3.0"}}"#,
    );
    write(root, "src/index.js", "export const a = 1;\n");
    write(
        root,
        "node_modules/left-pad/package.json",
        r#"{"name":"left-pad","version":"1.3.0"}"#,
    );
    set_config(root, &json!({ "rules": { "unused-dependencies": "off" } }));
    dir
}

#[test]
fn issue_repro_prints_one_note_and_keeps_the_exit_code() {
    let dir = issue_project();
    let output = run(dir.path(), &["dead-code", "--unused-deps"]);
    assert_eq!(output.code, 0, "{}", output.stderr);
    let notes: Vec<&str> = output
        .stderr
        .lines()
        .filter(|line| line.contains("did not check"))
        .collect();
    assert_eq!(notes, vec![NOTE], "{}", output.stderr);

    let quiet = run(dir.path(), &["dead-code", "--unused-deps", "--quiet"]);
    assert_eq!(quiet.code, 0);
    assert!(!quiet.stderr.contains("did not check"), "{}", quiet.stderr);

    set_config(dir.path(), &json!({}));
    let control = run(dir.path(), &["dead-code", "--unused-deps"]);
    assert_eq!(control.code, 1, "the rule on finds the unused package");
    assert!(
        !control.stderr.contains("did not check"),
        "{}",
        control.stderr
    );
}

#[test]
fn issue_repro_reaches_the_json_envelope() {
    let dir = issue_project();
    let output = run(
        dir.path(),
        &["dead-code", "--unused-deps", "--format", "json", "--quiet"],
    );
    assert_eq!(output.code, 0, "{}", output.stderr);
    let envelope = parse_json(&output);
    assert_eq!(envelope["total_issues"], 0);
    let diagnostic = envelope["workspace_diagnostics"]
        .as_array()
        .expect("workspace_diagnostics")
        .iter()
        .find(|diagnostic| diagnostic["kind"] == "requested-rule-off")
        .expect("one requested-rule-off entry");
    assert_eq!(diagnostic["rule"], "unused-dependencies");
    assert_eq!(diagnostic["flag"], "--unused-deps");
    assert_eq!(diagnostic["path"], ".");
    assert_eq!(
        diagnostic["message"],
        "unused-dependencies is off in config, so --unused-deps did not check it. Set the rule \
         to \"error\" or \"warn\" to run this check."
    );
    assert!(
        diagnostic.get("degrades_analysis").is_none(),
        "the run measured what the config asked for: {diagnostic}"
    );
}

#[test]
fn a_run_without_a_flag_reports_no_entry() {
    let dir = issue_project();
    let root = dir.path();
    assert!(entries(&json_run(root, &["dead-code"])).is_empty());
    assert!(entries(&json_run(root, &["dead-code", "--unused-files"])).is_empty());
    let human = run(root, &["dead-code"]);
    assert!(!human.stderr.contains("did not check"), "{}", human.stderr);
    let combined = json_run(root, &["--only", "dead-code"]);
    assert!(
        entries(&combined).is_empty(),
        "the bare combined run selects no rule: {combined}"
    );
    assert_eq!(
        entries(&json_run(root, &["dead-code", "--unused-deps"])),
        vec![entry("unused-dependencies", "--unused-deps")],
        "positive control"
    );
}

#[test]
fn only_the_unused_rules_fire_for_unused_deps() {
    let dir = issue_project();
    let root = dir.path();
    set_config(
        root,
        &json!({ "rules": {
            "unused-dependencies": "off",
            "unused-dev-dependencies": "off",
            "unused-optional-dependencies": "off",
            "type-only-dependencies": "off",
            "test-only-dependencies": "off",
            "dev-dependencies-in-production": "off",
        }}),
    );
    assert_eq!(
        entries(&json_run(root, &["dead-code", "--unused-deps"])),
        vec![
            entry("unused-dependencies", "--unused-deps"),
            entry("unused-dev-dependencies", "--unused-deps"),
            entry("unused-optional-dependencies", "--unused-deps"),
        ]
    );
}

/// Setting a placement rule off is the documented way to narrow `--unused-deps`.
#[test]
fn the_narrowing_recipe_of_unused_deps_prints_no_note() {
    let dir = issue_project();
    let root = dir.path();
    set_config(
        root,
        &json!({ "rules": {
            "type-only-dependencies": "off",
            "test-only-dependencies": "off",
            "dev-dependencies-in-production": "off",
        }}),
    );
    let output = run(root, &["dead-code", "--unused-deps"]);
    assert_eq!(output.code, 1, "the unused package still fails the run");
    assert!(
        !output.stderr.contains("did not check"),
        "{}",
        output.stderr
    );
    assert!(entries(&json_run(root, &["dead-code", "--unused-deps"])).is_empty());

    set_config(root, &json!({ "rules": { "unused-dependencies": "off" } }));
    assert_eq!(
        entries(&json_run(root, &["dead-code", "--unused-deps"])).len(),
        1,
        "positive control"
    );
}

/// Production mode forces two rules off in the resolved config. The config
/// does not say that, and setting them on changes nothing, so a note is false.
#[test]
fn production_mode_stays_silent_about_the_rules_it_forces_off() {
    let dir = issue_project();
    let root = dir.path();
    set_config(root, &json!({}));
    let flag = ["dead-code", "--unused-deps", "--production"];
    assert!(entries(&json_run(root, &flag)).is_empty());

    set_config(root, &json!({ "production": { "deadCode": true } }));
    assert!(entries(&json_run(root, &["dead-code", "--unused-deps"])).is_empty());

    set_config(
        root,
        &json!({ "rules": {
            "unused-dependencies": "off",
            "unused-dev-dependencies": "off",
            "unused-optional-dependencies": "off",
        }}),
    );
    assert_eq!(
        entries(&json_run(root, &flag)),
        vec![entry("unused-dependencies", "--unused-deps")],
        "a rule that the user set off still fires"
    );

    set_config(
        root,
        &json!({
            "production": { "deadCode": true },
            "rules": { "unused-dependencies": "off", "unused-dev-dependencies": "off" },
        }),
    );
    assert_eq!(
        entries(&json_run(root, &["dead-code", "--unused-deps"])),
        vec![entry("unused-dependencies", "--unused-deps")],
    );
}

/// A `--file` run drops dependency findings, so a rule at `error` would still
/// check nothing. The note would give a false remedy there.
#[test]
fn a_file_run_skips_the_dependency_note_only() {
    let dir = issue_project();
    let root = dir.path();
    set_config(
        root,
        &json!({ "rules": { "unused-dependencies": "off", "unused-exports": "off" } }),
    );
    let file = ["--file", "src/index.js"];
    let mut args = vec!["dead-code", "--unused-deps", "--unused-exports"];
    assert_eq!(
        entries(&json_run(root, &args)),
        vec![
            entry("unused-exports", "--unused-exports"),
            entry("unused-dependencies", "--unused-deps"),
        ],
    );
    args.extend_from_slice(&file);
    assert_eq!(
        entries(&json_run(root, &args)),
        vec![entry("unused-exports", "--unused-exports")],
    );
}

fn suppression_project() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("temporary project");
    let root = dir.path();
    write(
        root,
        "package.json",
        r#"{"name":"repro","private":true,"type":"module","main":"src/index.js"}"#,
    );
    write(
        root,
        "src/index.js",
        "// fallow-ignore-next-line unused-exports\nexport const used = 1;\nconsole.log(used);\n",
    );
    dir
}

#[test]
fn stale_suppressions_fires_only_for_its_own_rule() {
    let dir = suppression_project();
    let root = dir.path();
    let flag = ["dead-code", "--stale-suppressions"];

    assert!(
        entries(&json_run(root, &flag)).is_empty(),
        "default rules: `require-suppression-reason` defaults to off and stays silent"
    );

    set_config(
        root,
        &json!({ "rules": {
            "stale-suppressions": "off",
            "require-suppression-reason": "error",
        }}),
    );
    assert_eq!(
        entries(&json_run(root, &flag)),
        vec![entry("stale-suppressions", "--stale-suppressions")],
    );

    set_config(root, &json!({ "rules": { "stale-suppressions": "off" } }));
    let human = run(root, &flag);
    assert_eq!(human.code, 0, "{}", human.stderr);
    assert!(
        human.stderr.contains(
            "Note: stale-suppressions is off in config, so --stale-suppressions did not check it."
        ),
        "{}",
        human.stderr
    );
}

#[test]
fn a_flag_that_activates_its_rule_reports_no_entry() {
    let dir = issue_project();
    let root = dir.path();
    set_config(
        root,
        &json!({ "rules": {
            "private-type-leaks": "off",
            "absent-component-props": "off",
            "deprecated-exports-in-use": "off",
            "unused-exports": "off",
        }}),
    );
    for flag in [
        "--private-type-leaks",
        "--absent-component-props",
        "--deprecated-exports-in-use",
    ] {
        assert!(
            entries(&json_run(root, &["dead-code", flag])).is_empty(),
            "{flag}"
        );
    }
    assert_eq!(
        entries(&json_run(root, &["dead-code", "--unused-exports"])),
        vec![entry("unused-exports", "--unused-exports")],
        "positive control"
    );
}

/// A top-level `off` stops the detector, also when an override raises the rule
/// for some files. So "did not check" stays true.
#[test]
fn a_raised_override_of_a_top_level_off_rule_keeps_the_note() {
    let dir = issue_project();
    let root = dir.path();
    write(root, "src/orphan.js", "export const lost = 1;\n");
    set_config(
        root,
        &json!({
            "rules": { "unused-files": "off" },
            "overrides": [{ "files": ["src/**"], "rules": { "unused-files": "error" } }],
        }),
    );
    let envelope = json_run(root, &["dead-code", "--unused-files"]);
    assert_eq!(envelope["unused_files"].as_array().map(Vec::len), Some(0));
    assert_eq!(
        entries(&envelope),
        vec![entry("unused-files", "--unused-files")]
    );
}

fn architecture_project() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("temporary project");
    let root = dir.path();
    write(
        root,
        "package.json",
        r#"{"name":"repro","private":true,"type":"module","main":"src/a.js"}"#,
    );
    write(
        root,
        "src/a.js",
        "import { b } from './b.js';\nexport const a = b;\n",
    );
    write(
        root,
        "src/b.js",
        "import { a } from './a.js';\nexport const b = a;\n",
    );
    dir
}

#[test]
fn architecture_fires_only_for_an_explicit_selector() {
    let dir = architecture_project();
    let root = dir.path();
    set_config(
        root,
        &json!({ "rules": {
            "circular-dependencies": "off",
            "boundary-violation": "off",
            "policy-violation": "off",
        }}),
    );
    assert!(
        entries(&json_run(root, &["architecture"])).is_empty(),
        "no selector asks for nothing"
    );
    assert_eq!(
        entries(&json_run(root, &["architecture", "--cycles"])),
        vec![entry("circular-dependencies", "--cycles")],
    );
    assert_eq!(
        entries(&json_run(root, &["architecture", "--boundaries"])),
        vec![entry("boundary-violation", "--boundaries")],
    );
    assert_eq!(
        entries(&json_run(root, &["architecture", "--policy"])),
        vec![entry("policy-violation", "--policy")],
    );
    let human = run(root, &["architecture", "--cycles"]);
    assert_eq!(human.code, 0, "{}", human.stderr);
    assert!(
        human.stderr.contains(
            "Note: circular-dependencies is off in config, so --cycles did not check it."
        ),
        "{}",
        human.stderr
    );

    set_config(root, &json!({}));
    let control = run(root, &["architecture", "--cycles"]);
    assert_eq!(control.code, 1, "the rule on finds the cycle");
}

#[test]
fn cycles_reports_each_rule_that_is_off() {
    let dir = architecture_project();
    let root = dir.path();
    set_config(
        root,
        &json!({ "rules": {
            "circular-dependencies": "off",
            "re-export-cycle": "off",
            "package-cycle": "off",
        }}),
    );
    assert_eq!(
        entries(&json_run(root, &["architecture", "--cycles"])),
        vec![
            entry("circular-dependencies", "--cycles"),
            entry("re-export-cycle", "--cycles"),
            entry("package-cycle", "--cycles"),
        ]
    );
}

#[test]
fn dead_code_flags_name_the_flag_that_the_user_typed() {
    let dir = architecture_project();
    let root = dir.path();
    set_config(
        root,
        &json!({ "rules": {
            "circular-dependencies": "off",
            "boundary-violation": "off",
        }}),
    );
    assert_eq!(
        entries(&json_run(root, &["dead-code", "--circular-deps"])),
        vec![entry("circular-dependencies", "--circular-deps")],
    );
    assert_eq!(
        entries(&json_run(root, &["dead-code", "--boundary-violations"])),
        vec![entry("boundary-violation", "--boundary-violations")],
        "the three boundary kinds share one config key"
    );
}

#[test]
fn grouped_envelopes_keep_the_entry() {
    let dir = architecture_project();
    let root = dir.path();
    set_config(
        root,
        &json!({ "rules": { "circular-dependencies": "off", "unused-exports": "off" } }),
    );
    for (args, expected) in [
        (
            vec!["dead-code", "--unused-exports", "--group-by", "directory"],
            entry("unused-exports", "--unused-exports"),
        ),
        (
            vec!["architecture", "--cycles", "--group-by", "directory"],
            entry("circular-dependencies", "--cycles"),
        ),
    ] {
        assert_eq!(entries(&json_run(root, &args)), vec![expected], "{args:?}");
    }
}

/// The documented lean contract groups a kind into its first entry and `count`.
#[test]
fn a_lean_run_keeps_the_first_entry_and_the_count() {
    let dir = issue_project();
    let root = dir.path();
    set_config(
        root,
        &json!({ "rules": {
            "unused-dependencies": "off",
            "unused-dev-dependencies": "off",
        }}),
    );
    let envelope = json_run(root, &["dead-code", "--unused-deps", "--lean"]);
    let diagnostics = envelope["workspace_diagnostics"]
        .as_array()
        .expect("workspace_diagnostics");
    let grouped: Vec<&Value> = diagnostics
        .iter()
        .filter(|diagnostic| diagnostic["kind"] == "requested-rule-off")
        .collect();
    assert_eq!(grouped.len(), 1, "{diagnostics:?}");
    assert_eq!(grouped[0]["rule"], "unused-dependencies");
    assert_eq!(grouped[0]["count"], 2);
}

fn formats_project() -> tempfile::TempDir {
    let dir = issue_project();
    set_config(
        dir.path(),
        &json!({ "rules": { "unused-dependencies": "off", "unused-dev-dependencies": "off" } }),
    );
    dir
}

#[test]
fn sarif_has_a_notification_and_no_result() {
    let dir = formats_project();
    let output = run(
        dir.path(),
        &["dead-code", "--unused-deps", "--format", "sarif", "--quiet"],
    );
    assert_eq!(output.code, 0, "{}", output.stderr);
    let sarif = parse_json(&output);
    let run_entry = &sarif["runs"][0];
    assert_eq!(run_entry["results"].as_array().map(Vec::len), Some(0));
    let notifications = run_entry["invocations"][0]["toolConfigurationNotifications"]
        .as_array()
        .expect("notifications");
    let found: Vec<&Value> = notifications
        .iter()
        .filter(|notification| notification["descriptor"]["id"] == "requested-rule-off")
        .collect();
    assert_eq!(found.len(), 2, "{notifications:?}");
    assert_eq!(found[0]["level"], "note");
    assert_eq!(found[0]["properties"]["rule"], "unused-dependencies");
    assert_eq!(found[0]["properties"]["flag"], "--unused-deps");
    assert!(
        found[0]["message"]["text"]
            .as_str()
            .expect("text")
            .contains("did not check it")
    );
}

const SECTION: &str = "## Requested checks that did not run";
const SECTION_LINE: &str =
    "- `unused-dependencies` is off in config, so `--unused-deps` did not check it.";
const SECTION_END: &str = "Set each rule to \"error\" or \"warn\" to run the check.";

fn assert_section(text: &str, label: &str) {
    assert!(text.contains(SECTION), "{label}: {text}");
    assert!(text.contains(SECTION_LINE), "{label}: {text}");
    assert!(text.contains(SECTION_END), "{label}: {text}");
    assert!(
        !text.contains("Note: unused-dependencies"),
        "{label}: one place per format: {text}"
    );
}

#[test]
fn markdown_formats_have_the_section() {
    let dir = formats_project();
    for format in [
        "markdown",
        "github-summary",
        "pr-comment-github",
        "pr-comment-gitlab",
        "review-github",
        "review-gitlab",
    ] {
        let output = run(
            dir.path(),
            &["dead-code", "--unused-deps", "--format", format],
        );
        assert_eq!(output.code, 0, "{format}: {}", output.stderr);
        let text = if format.starts_with("review-") {
            parse_json(&output)["body"]
                .as_str()
                .expect("review body")
                .to_owned()
        } else {
            output.stdout.clone()
        };
        assert_section(&text, format);
        assert!(!output.stderr.contains("did not check"), "{format}");
    }
}

#[test]
fn stdout_formats_keep_stdout_and_print_the_note_on_stderr() {
    let dir = formats_project();
    let root = dir.path();
    for format in ["codeclimate", "compact", "github-annotations"] {
        let output = run(root, &["dead-code", "--unused-deps", "--format", format]);
        assert_eq!(output.code, 0, "{format}: {}", output.stderr);
        assert!(!output.stdout.contains("did not check"), "{format}");
        assert!(output.stderr.contains(NOTE), "{format}: {}", output.stderr);
    }
    let code_climate = run(
        root,
        &["dead-code", "--unused-deps", "--format", "codeclimate"],
    );
    assert_eq!(code_climate.stdout.trim(), "[]");
}

#[test]
fn report_from_a_saved_envelope_states_the_same_entry() {
    let dir = formats_project();
    let root = dir.path();
    let saved = run(
        root,
        &["dead-code", "--unused-deps", "--format", "json", "--quiet"],
    );
    let from = root.join("saved.json");
    std::fs::write(&from, &saved.stdout).expect("write envelope");
    let from = from.to_string_lossy().into_owned();

    let markdown = run(root, &["report", "--from", &from, "--format", "markdown"]);
    assert_section(&markdown.stdout, "report markdown");

    let sarif = run(root, &["report", "--from", &from, "--format", "sarif"]);
    assert!(
        sarif.stdout.contains("requested-rule-off"),
        "{}",
        sarif.stdout
    );

    let stderr_format = run(
        root,
        &["report", "--from", &from, "--format", "codeclimate"],
    );
    assert!(
        stderr_format.stderr.contains(NOTE),
        "{}",
        stderr_format.stderr
    );
    let quiet = run(
        root,
        &[
            "report",
            "--from",
            &from,
            "--format",
            "codeclimate",
            "--quiet",
        ],
    );
    assert!(!quiet.stderr.contains("did not check"), "{}", quiet.stderr);
}

/// The typed route reads the same table as the CLI, so the same request gives
/// the same entries on both routes.
#[test]
fn the_cli_and_the_typed_route_report_the_same_entries() {
    let dir = issue_project();
    let root = dir.path();
    set_config(
        root,
        &json!({ "rules": {
            "unused-dependencies": "off",
            "unused-dev-dependencies": "off",
            "stale-suppressions": "off",
            "circular-dependencies": "off",
        }}),
    );
    let typed = |filters: fallow_api::DeadCodeFilters, production: bool| {
        entries(
            &fallow_api::serialize_dead_code_programmatic_json(
                fallow_api::run_dead_code(&fallow_api::DeadCodeOptions {
                    analysis: fallow_api::AnalysisOptions {
                        root: Some(root.to_path_buf()),
                        production_override: production.then_some(true),
                        ..fallow_api::AnalysisOptions::default()
                    },
                    filters,
                    ..fallow_api::DeadCodeOptions::default()
                })
                .expect("typed run"),
            )
            .expect("typed JSON"),
        )
    };
    let filters = |selector: &str| {
        let mut filters = fallow_api::DeadCodeFilters::default();
        assert!(filters.enable_registry_selector(selector), "{selector}");
        filters
    };
    for (selector, args, production) in [
        ("unused-deps", vec!["dead-code", "--unused-deps"], false),
        (
            "unused-deps",
            vec!["dead-code", "--unused-deps", "--production"],
            true,
        ),
        (
            "stale-suppressions",
            vec!["dead-code", "--stale-suppressions"],
            false,
        ),
        ("circular-deps", vec!["dead-code", "--circular-deps"], false),
    ] {
        let cli = entries(&json_run(root, &args));
        assert!(!cli.is_empty(), "{args:?}: positive control");
        assert_eq!(typed(filters(selector), production), cli, "{args:?}");
    }
    assert!(typed(fallow_api::DeadCodeFilters::default(), false).is_empty());

    let architecture = |cycles: bool| {
        entries(
            &fallow_api::serialize_architecture_programmatic_json(
                fallow_api::run_architecture(&fallow_api::ArchitectureOptions {
                    analysis: fallow_api::AnalysisOptions {
                        root: Some(root.to_path_buf()),
                        ..fallow_api::AnalysisOptions::default()
                    },
                    cycles,
                    ..fallow_api::ArchitectureOptions::default()
                })
                .expect("typed architecture run"),
            )
            .expect("typed architecture JSON"),
        )
    };
    assert_eq!(
        architecture(true),
        entries(&json_run(root, &["architecture", "--cycles"]))
    );
    assert_eq!(
        architecture(false),
        entries(&json_run(root, &["architecture"]))
    );
}
