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

use std::path::Path;

use crate::common::{
    CommandOutput, commit_all, git, parse_json, run_fallow_in_root, run_fallow_raw,
};
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

fn prop_project(severity: &str) -> tempfile::TempDir {
    let project = crate::common::copy_fixture("prop-drilling");
    let mut rules = serde_json::to_value(fallow_config::RulesConfig::default()).unwrap();
    for rule in rules.as_object_mut().unwrap().values_mut() {
        *rule = Value::String("off".to_owned());
    }
    rules["prop-drilling"] = Value::String(severity.to_owned());
    write(
        project.path(),
        ".fallowrc.json",
        &serde_json::json!({"rules": rules}).to_string(),
    );
    project
}

#[test]
fn prop_drilling_error_gates_and_warnings_remain_advisory_in_strict_runs() {
    for (severity, extra, expected_code, expected_total) in [
        ("error", vec![], 1, 1),
        ("warn", vec![], 0, 0),
        ("warn", vec!["--fail-on-issues"], 0, 0),
        ("off", vec!["--fail-on-issues"], 0, 0),
    ] {
        let project = prop_project(severity);
        let output = dead_code(project.path(), "json", &extra);
        let report = parse_json(&output);
        assert_eq!(output.code, expected_code, "{severity}: {}", output.stdout);
        assert_eq!(report["total_issues"], expected_total);
        if severity == "off" {
            assert_eq!(
                report["prop_drilling_chains"]
                    .as_array()
                    .map_or(0, Vec::len),
                0
            );
        } else {
            assert_eq!(report["prop_drilling_chains"][0]["prop"], "user");
            assert_eq!(
                report["prop_drilling_chains"][0]["effective_severity"],
                severity
            );
        }
    }
}

#[test]
fn prop_drilling_source_override_can_enable_and_suppress_enforcement() {
    for (base, source, expected_code, expected_chains) in [
        ("off", "error", 1, 1),
        ("error", "warn", 0, 1),
        ("error", "off", 0, 0),
    ] {
        let project = prop_project(base);
        let path = project.path().join(".fallowrc.json");
        let mut config: Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        config["overrides"] = serde_json::json!([
            {"files": ["src/Page.tsx"], "rules": {"prop-drilling": source}},
            {"files": ["src/Profile.tsx"], "rules": {"prop-drilling": "error"}}
        ]);
        std::fs::write(path, config.to_string()).unwrap();
        let output = dead_code(project.path(), "json", &["--fail-on-issues"]);
        let report = parse_json(&output);
        assert_eq!(
            output.code, expected_code,
            "{base}/{source}: {}",
            output.stdout
        );
        assert_eq!(
            report["prop_drilling_chains"]
                .as_array()
                .map_or(0, Vec::len),
            expected_chains
        );
        if expected_chains > 0 {
            assert_eq!(
                report["prop_drilling_chains"][0]["effective_severity"],
                source
            );
        }
    }
    let project = prop_project("error");
    assert_eq!(dead_code(project.path(), "json", &[]).code, 1);
    let path = project.path().join("src/Page.tsx");
    let text = std::fs::read_to_string(&path).unwrap();
    std::fs::write(
        path,
        text.replace(
            "export const Page",
            "// fallow-ignore-next-line prop-drilling\nexport const Page",
        ),
    )
    .unwrap();
    let suppressed = dead_code(project.path(), "json", &[]);
    assert_eq!(suppressed.code, 0);
    assert_eq!(parse_json(&suppressed)["total_issues"], 0);
}

fn add_prop_source(root: &Path) {
    let page = root.join("src/Page.tsx");
    let source = std::fs::read_to_string(&page).unwrap();
    std::fs::write(&page, format!("{source}\nexport const OtherPage = ({{ user }}: {{ user: {{ name: string }} }}) => <Layout user={{user}} />;\n")).unwrap();
    let main = root.join("src/main.tsx");
    let source = std::fs::read_to_string(&main).unwrap();
    std::fs::write(
        main,
        source.replace("{ Page }", "{ Page, OtherPage }").replace(
            "<Page user={user} />",
            "<><Page user={user} /><OtherPage user={user} /></>",
        ),
    )
    .unwrap();
}

#[test]
fn prop_drilling_baseline_roundtrip_and_prune_preserve_occurrences() {
    let project = prop_project("error");
    let baseline = project.path().join("baseline.json");
    let baseline_arg = baseline.to_str().unwrap();
    let original = dead_code(project.path(), "json", &["--save-baseline", baseline_arg]);
    assert_eq!(
        parse_json(&original)["prop_drilling_chains"][0]["prop"],
        "user"
    );
    let saved: Value = serde_json::from_str(&std::fs::read_to_string(&baseline).unwrap()).unwrap();
    assert_eq!(
        saved["prop_drilling_chains"].as_array().map_or(0, Vec::len),
        1
    );
    let reviewed = dead_code(project.path(), "json", &["--baseline", baseline_arg]);
    assert_eq!(reviewed.code, 0);
    assert_eq!(parse_json(&reviewed)["total_issues"], 0);

    let sidebar = project.path().join("src/Sidebar.tsx");
    let text = std::fs::read_to_string(&sidebar).unwrap();
    add_prop_source(project.path());
    let branch = dead_code(project.path(), "json", &["--baseline", baseline_arg]);
    assert_eq!(branch.code, 1);
    assert_eq!(parse_json(&branch)["total_issues"], 1);

    std::fs::write(
        &sidebar,
        text.replace("user={user}", "user={{ name: user.name }}"),
    )
    .unwrap();
    let stale = dead_code(
        project.path(),
        "json",
        &["--baseline", baseline_arg, "--fail-on-stale-baseline"],
    );
    assert_eq!(stale.code, 1);
    let pruned = run_fallow_in_root(
        "baselines",
        project.path(),
        &[
            "prune",
            "--dead-code-baseline",
            baseline_arg,
            "--quiet",
            "--format",
            "json",
        ],
    );
    assert_eq!(pruned.code, 0, "{}", pruned.stderr);
    let saved: Value = serde_json::from_str(&std::fs::read_to_string(&baseline).unwrap()).unwrap();
    assert!(saved["prop_drilling_chains"].as_array().unwrap().is_empty());
}

#[test]
fn prop_drilling_enforcement_retains_downstream_changes_and_source_anchor() {
    let project = prop_project("error");
    git(project.path(), &["init", "-q", "-b", "main"]);
    commit_all(project.path(), "base");
    let profile = project.path().join("src/Profile.tsx");
    let text = std::fs::read_to_string(&profile).unwrap();
    std::fs::write(
        &profile,
        text.replace("user.name", "user.name.toUpperCase()"),
    )
    .unwrap();
    let changed = dead_code(project.path(), "json", &["--changed-since", "HEAD"]);
    assert_eq!(changed.code, 1, "{}", changed.stdout);
    assert_eq!(
        parse_json(&changed)["prop_drilling_chains"][0]["hops"][0]["file"],
        "src/Page.tsx"
    );
    let diff = project.path().join("change.diff");
    std::fs::write(&diff, "diff --git a/src/Profile.tsx b/src/Profile.tsx\n--- a/src/Profile.tsx\n+++ b/src/Profile.tsx\n@@ -3,1 +3,1 @@\n-old\n+new\n").unwrap();
    let line_scoped = dead_code(
        project.path(),
        "json",
        &["--diff-file", diff.to_str().unwrap()],
    );
    assert_eq!(line_scoped.code, 1);
    assert_eq!(parse_json(&line_scoped)["total_issues"], 1);
    std::fs::write(&diff, "diff --git a/src/main.tsx b/src/main.tsx\n--- a/src/main.tsx\n+++ b/src/main.tsx\n@@ -1,1 +1,1 @@\n-old\n+new\n").unwrap();
    let irrelevant = dead_code(
        project.path(),
        "json",
        &["--diff-file", diff.to_str().unwrap()],
    );
    assert_eq!(irrelevant.code, 0);
    assert_eq!(parse_json(&irrelevant)["total_issues"], 0);
    let config_path = project.path().join(".fallowrc.json");
    let mut config: Value =
        serde_json::from_str(&std::fs::read_to_string(&config_path).unwrap()).unwrap();
    config["rules"]["prop-drilling"] = serde_json::json!("warn");
    std::fs::write(config_path, config.to_string()).unwrap();
    let advisory = dead_code(
        project.path(),
        "json",
        &["--changed-since", "HEAD", "--fail-on-issues"],
    );
    assert_eq!(advisory.code, 0);
    assert!(
        parse_json(&advisory)["prop_drilling_chains"]
            .as_array()
            .is_none_or(Vec::is_empty)
    );
}

#[test]
fn prop_drilling_errors_are_actionable_in_ci_formats_and_saved_reports() {
    let project = prop_project("error");
    let saved = project.path().join("saved.json");
    let output = dead_code(project.path(), "json", &[]);
    std::fs::write(&saved, &output.stdout).unwrap();
    for format in [
        "sarif",
        "codeclimate",
        "github-annotations",
        "github-summary",
    ] {
        let direct = dead_code(project.path(), format, &[]);
        assert_eq!(direct.code, 1, "{format}: {}", direct.stdout);
        let rendered = report_from(project.path(), &saved, format);
        match format {
            "sarif" => {
                for result in [&direct, &rendered] {
                    let json = parse_json(result);
                    assert_eq!(json["runs"][0]["results"][0]["level"], "error");
                    assert_eq!(
                        json["runs"][0]["results"][0]["locations"][0]["physicalLocation"]["artifactLocation"]
                            ["uri"],
                        "src/Page.tsx"
                    );
                }
            }
            "codeclimate" => {
                for result in [&direct, &rendered] {
                    let json = parse_json(result);
                    assert_eq!(json[0]["check_name"], "fallow/prop-drilling");
                    assert_eq!(json[0]["severity"], "major");
                    assert_eq!(json[0]["location"]["path"], "src/Page.tsx");
                }
            }
            "github-annotations" => {
                assert!(
                    direct.stdout.contains("::error file=src/Page.tsx,line=4"),
                    "{}",
                    direct.stdout
                );
                assert!(rendered.stdout.contains("Prop drilling"));
            }
            "github-summary" => {
                assert!(direct.stdout.contains("Prop drilling"), "{}", direct.stdout);
                assert!(rendered.stdout.contains("src/Page.tsx"));
            }
            _ => unreachable!(),
        }
    }
}

#[test]
fn prop_drilling_explain_defines_enforced_counter_for_flat_and_grouped_output() {
    for severity in ["error", "warn"] {
        let project = prop_project(severity);
        for args in [
            vec!["--explain"],
            vec!["--summary", "--explain"],
            vec!["--group-by", "directory", "--explain"],
        ] {
            let report = parse_json(&dead_code(project.path(), "json", &args));
            if args.contains(&"--group-by") {
                assert_eq!(report["total_issues"], i32::from(severity == "error"));
            } else if severity == "error" {
                assert_eq!(report["summary"]["prop_drilling_chains"], 1);
            } else {
                assert!(report["summary"].get("prop_drilling_chains").is_none());
            }
            let metric = &report["_meta"]["metrics"]["summary.prop_drilling_chains"];
            assert_eq!(metric["name"], "Enforced prop-drilling chains");
            assert_eq!(metric["range"], "Non-negative integer");
            let definition = metric["description"]
                .as_str()
                .expect("emitted counter needs a definition");
            assert!(
                definition.contains("effective_severity")
                    && definition.contains("error")
                    && definition.contains("Warning chains are excluded")
                    && definition.contains("omitted when zero")
            );
            assert!(
                metric["interpretation"]
                    .as_str()
                    .unwrap()
                    .contains("total_issues")
            );
        }
    }
}

#[test]
fn other_component_signals_remain_advisory_under_strict_flags() {
    for (fixture, rule, key) in [
        ("thin-wrapper", "thin-wrapper", "thin_wrappers"),
        (
            "duplicate-prop-shape",
            "duplicate-prop-shape",
            "duplicate_prop_shapes",
        ),
    ] {
        let project = crate::common::copy_fixture(fixture);
        let mut rules = serde_json::to_value(fallow_config::RulesConfig::default()).unwrap();
        for rule in rules.as_object_mut().unwrap().values_mut() {
            *rule = Value::String("off".to_owned());
        }
        rules[rule] = Value::String("error".to_owned());
        write(
            project.path(),
            ".fallowrc.json",
            &serde_json::json!({"rules":rules}).to_string(),
        );
        let output = dead_code(project.path(), "json", &["--fail-on-issues"]);
        let report = parse_json(&output);
        assert!(!report[key].as_array().unwrap().is_empty(), "{fixture}");
        assert_eq!(output.code, 0, "{fixture}: {}", output.stdout);
        assert_eq!(report["total_issues"], 0);
        assert_eq!(
            parse_json(&dead_code(project.path(), "sarif", &["--fail-on-issues"]))["runs"][0]["results"]
                [0]["level"],
            "warning"
        );
    }
}

// Captured from the pre-enforcement binary on the tracked prop-drilling fixture.
// These archives genuinely excluded saved error chains from their issue totals.
const LEGACY_PROP_REPORTS: [&str; 3] = [
    include_str!("fixtures/prop-drilling-advisory-error.json"),
    include_str!("fixtures/prop-drilling-advisory-grouped.json"),
    include_str!("fixtures/prop-drilling-advisory-combined.json"),
];

#[test]
fn historical_prop_error_reports_render_after_count_policy_migration() {
    let project = prop_project("off");
    let saved = project.path().join("historical.json");
    for archive in LEGACY_PROP_REPORTS {
        let json: Value = serde_json::from_str(archive).unwrap();
        assert_eq!(
            json.pointer("/total_issues")
                .or_else(|| json.pointer("/check/total_issues")),
            Some(&serde_json::json!(0))
        );
        std::fs::write(&saved, archive).unwrap();
        for format in [
            "sarif",
            "codeclimate",
            "github-annotations",
            "github-summary",
            "markdown",
        ] {
            let output = report_from(project.path(), &saved, format);
            match format {
                "sarif" => assert!(
                    parse_json(&output)["runs"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .flat_map(|run| run["results"].as_array().into_iter().flatten())
                        .any(|finding| finding["ruleId"] == "fallow/prop-drilling"
                            && finding["level"] == "error")
                ),
                "codeclimate" => assert!(
                    parse_json(&output)
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|finding| finding["check_name"] == "fallow/prop-drilling"
                            && finding["severity"] == "major")
                ),
                "github-annotations" => {
                    assert!(output.stdout.contains("::error file=src/Page.tsx,line=4"));
                }
                _ => {
                    assert!(
                        output.stdout.contains("Prop drilling"),
                        "{format}: {}",
                        output.stdout
                    );
                    if json["kind"] != "combined" {
                        assert!(
                            output.stdout.contains("src/Page.tsx"),
                            "{format}: {}",
                            output.stdout
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn historical_prop_audit_section_renders_with_archived_gate_outcome() {
    let project = prop_project("off");
    git(project.path(), &["init", "-q", "-b", "main"]);
    commit_all(project.path(), "base");
    let output = run_fallow_in_root(
        "audit",
        project.path(),
        &[
            "--base", "main", "--gate", "all", "--quiet", "--format", "json",
        ],
    );
    assert_eq!(output.code, 0, "{}", output.stderr);
    let mut archive = parse_json(&output);
    archive["dead_code"] = serde_json::from_str(LEGACY_PROP_REPORTS[0]).unwrap();
    let saved = project.path().join("historical-audit.json");
    std::fs::write(&saved, archive.to_string()).unwrap();
    let sarif = parse_json(&report_from(project.path(), &saved, "sarif"));
    assert!(
        sarif["runs"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|run| run["results"].as_array().into_iter().flatten())
            .any(|finding| finding["ruleId"] == "fallow/prop-drilling"
                && finding["level"] == "error")
    );
    let cc = parse_json(&report_from(project.path(), &saved, "codeclimate"));
    assert!(
        cc.as_array()
            .unwrap()
            .iter()
            .any(|finding| finding["check_name"] == "fallow/prop-drilling"
                && finding["severity"] == "major")
    );
    assert!(
        report_from(project.path(), &saved, "github-annotations")
            .stdout
            .contains("::error file=src/Page.tsx,line=4")
    );
    let summary = report_from(project.path(), &saved, "github-summary");
    assert!(summary.stdout.contains("Prop drilling"));
    assert!(summary.stdout.contains("passed"), "{}", summary.stdout);
}

#[test]
fn historical_prop_count_migration_preserves_corruption_guards() {
    let project = prop_project("off");
    let original: Value = serde_json::from_str(LEGACY_PROP_REPORTS[0]).unwrap();
    let grouped: Value = serde_json::from_str(LEGACY_PROP_REPORTS[1]).unwrap();
    let mut corrupt = Vec::new();
    let mut altered = original.clone();
    altered["total_issues"] = serde_json::json!(3);
    corrupt.push(altered);
    let mut altered = original.clone();
    altered.as_object_mut().unwrap().remove("unused_files");
    corrupt.push(altered);
    let mut altered = original.clone();
    altered["summary"]["prop_drilling_chains"] = serde_json::json!(1);
    corrupt.push(altered);
    let mut altered = original.clone();
    altered["summary"]["total_issues"] = serde_json::json!(99);
    corrupt.push(altered);
    let mut altered = grouped.clone();
    altered["total_issues"] = serde_json::json!(1);
    corrupt.push(altered);
    let mut altered = grouped.clone();
    altered["groups"][0]["total_issues"] = serde_json::json!(1);
    corrupt.push(altered);
    let mut altered = grouped.clone();
    altered["groups"][0].as_object_mut().unwrap().remove("key");
    corrupt.push(altered);
    let mut altered = grouped.clone();
    altered["summary"] = serde_json::json!({"prop_drilling_chains": 1});
    corrupt.push(altered);
    let mut altered = grouped;
    let mut current_group = altered["groups"][0].clone();
    current_group["key"] = serde_json::json!("other");
    current_group["total_issues"] = serde_json::json!(1);
    altered["groups"]
        .as_array_mut()
        .unwrap()
        .push(current_group);
    altered["total_issues"] = serde_json::json!(1);
    corrupt.push(altered);
    let saved = project.path().join("corrupt.json");
    for archive in corrupt {
        std::fs::write(&saved, archive.to_string()).unwrap();
        let output = run_fallow_raw(&[
            "report",
            "--from",
            saved.to_str().unwrap(),
            "--root",
            project.path().to_str().unwrap(),
            "--quiet",
            "--format",
            "github-summary",
        ]);
        assert_eq!(output.code, 2, "accepted corrupt archive: {archive}");
    }
    for severity in [
        serde_json::json!("warn"),
        Value::Null,
        serde_json::json!("future-severity"),
    ] {
        let mut archive = original.clone();
        archive["prop_drilling_chains"][0]["effective_severity"] = severity;
        std::fs::write(&saved, archive.to_string()).unwrap();
        assert_eq!(
            parse_json(&report_from(project.path(), &saved, "sarif"))["runs"][0]["results"][0]["level"],
            "warning"
        );
        assert!(
            parse_json(&report_from(project.path(), &saved, "codeclimate"))
                .as_array()
                .unwrap()
                .is_empty()
        );
    }
}

#[test]
fn prop_drilling_empty_saved_hops_do_not_invent_ci_locations() {
    let project = prop_project("error");
    let mut json = parse_json(&dead_code(project.path(), "json", &[]));
    assert_eq!(
        json["prop_drilling_chains"][0]["hops"][0]["component"],
        "Page"
    );
    json["prop_drilling_chains"][0]["hops"] = serde_json::json!([]);
    let saved = project.path().join("empty-hops.json");
    std::fs::write(&saved, json.to_string()).unwrap();
    let sarif = parse_json(&report_from(project.path(), &saved, "sarif"));
    let result = &sarif["runs"][0]["results"][0];
    assert_eq!(result["level"], "error");
    assert!(result["locations"].as_array().is_none_or(Vec::is_empty));
    assert!(
        parse_json(&report_from(project.path(), &saved, "codeclimate"))
            .as_array()
            .unwrap()
            .is_empty()
    );
    let annotations = report_from(project.path(), &saved, "github-annotations");
    assert!(!annotations.stdout.contains("file="));
    let summary = report_from(project.path(), &saved, "github-summary");
    assert!(!summary.stdout.contains("src/Page.tsx"));
}

#[test]
fn prop_drilling_warnings_keep_advisory_native_placement_and_bare_counts() {
    let project = prop_project("warn");
    let json = parse_json(&dead_code(project.path(), "json", &["--fail-on-issues"]));
    assert_eq!(
        json["prop_drilling_chains"][0]["effective_severity"],
        "warn"
    );
    assert!(
        parse_json(&dead_code(
            project.path(),
            "codeclimate",
            &["--fail-on-issues"]
        ))
        .as_array()
        .unwrap()
        .is_empty()
    );
    assert!(
        !dead_code(project.path(), "github-annotations", &["--fail-on-issues"])
            .stdout
            .contains("Prop drilling")
    );
    assert!(
        !dead_code(project.path(), "github-summary", &["--fail-on-issues"])
            .stdout
            .contains("Prop drilling")
    );
    assert_eq!(
        parse_json(&dead_code(project.path(), "sarif", &["--fail-on-issues"]))["runs"][0]["results"]
            [0]["level"],
        "warning"
    );
    let bare = run_fallow_raw(&[
        "--root",
        project.path().to_str().unwrap(),
        "--quiet",
        "--format",
        "json",
        "--fail-on-issues",
    ]);
    assert_eq!(bare.code, 0, "{}", bare.stderr);
    let json = parse_json(&bare);
    assert_eq!(json["check"]["total_issues"], 0);
    assert_eq!(
        json["check"]["prop_drilling_chains"][0]["effective_severity"],
        "warn"
    );
    for severity in ["warn", "error"] {
        let project = prop_project(severity);
        let bare = run_fallow_raw(&[
            "--root",
            project.path().to_str().unwrap(),
            "--quiet",
            "--format",
            "json",
            "--ci",
        ]);
        let expected = i32::from(severity == "error");
        assert_eq!(bare.code, expected, "{}", bare.stderr);
        assert_eq!(parse_json(&bare)["check"]["total_issues"], expected);
    }
}

#[test]
fn prop_drilling_audit_distinguishes_reviewed_and_introduced_sources() {
    let project = prop_project("error");
    git(project.path(), &["init", "-q", "-b", "main"]);
    commit_all(project.path(), "base");
    git(project.path(), &["checkout", "-q", "-b", "feat/change"]);
    let path = project.path().join("src/Profile.tsx");
    let source = std::fs::read_to_string(&path).unwrap();
    std::fs::write(
        &path,
        source.replace("user.name", "user.name.toUpperCase()"),
    )
    .unwrap();
    commit_all(project.path(), "touch consumer");
    let args = ["--base", "main", "--quiet", "--format", "json"];
    let inherited = run_fallow_in_root("audit", project.path(), &args);
    let report = parse_json(&inherited);
    assert_eq!(inherited.code, 0, "{}", inherited.stdout);
    assert_eq!(
        report["dead_code"]["prop_drilling_chains"][0]["introduced"],
        false
    );
    assert_eq!(report["attribution"]["dead_code_inherited"], 1);
    let all = run_fallow_in_root(
        "audit",
        project.path(),
        &[
            "--base", "main", "--gate", "all", "--quiet", "--format", "json",
        ],
    );
    assert_eq!(all.code, 1);
    add_prop_source(project.path());
    commit_all(project.path(), "add source");
    let introduced = run_fallow_in_root("audit", project.path(), &args);
    let report = parse_json(&introduced);
    assert_eq!(introduced.code, 1, "{}", introduced.stdout);
    assert_eq!(report["attribution"]["dead_code_introduced"], 1);
    assert_eq!(report["attribution"]["dead_code_inherited"], 1);
    let source = report["dead_code"]["prop_drilling_chains"]
        .as_array()
        .unwrap()
        .iter()
        .find(|finding| finding["hops"][0]["component"] == "OtherPage")
        .unwrap();
    assert_eq!(source["introduced"], true);
    for format in ["github-annotations", "github-summary"] {
        let output = run_fallow_in_root(
            "audit",
            project.path(),
            &["--base", "main", "--quiet", "--format", format],
        );
        assert!(
            output.stdout.contains("Prop drilling"),
            "{format}: {}",
            output.stdout
        );
        assert!(output.stdout.contains("src/Page.tsx"));
    }
    let config_path = project.path().join(".fallowrc.json");
    let mut config: Value =
        serde_json::from_str(&std::fs::read_to_string(&config_path).unwrap()).unwrap();
    config["rules"]["prop-drilling"] = serde_json::json!("warn");
    std::fs::write(config_path, config.to_string()).unwrap();
    commit_all(project.path(), "advisory policy");
    for gate in ["new-only", "all"] {
        let advisory = run_fallow_in_root(
            "audit",
            project.path(),
            &[
                "--base", "main", "--gate", gate, "--quiet", "--format", "json",
            ],
        );
        assert_eq!(advisory.code, 0, "{}", advisory.stderr);
        let json = parse_json(&advisory);
        assert_eq!(json["attribution"]["dead_code_introduced"], 0);
        assert_eq!(json["attribution"]["dead_code_inherited"], 0);
        assert_eq!(
            json["dead_code"]["prop_drilling_chains"][0]["effective_severity"],
            "warn"
        );
    }
}

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
