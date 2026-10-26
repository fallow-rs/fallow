//! Component health warnings stay advisory, while explicit errors enforce gates.

use crate::common::{
    commit_all, copy_fixture, git, parse_json, run_fallow_in_root, run_fallow_raw,
};
use serde_json::{Value, json};

fn project(rule: &str, severity: &str) -> tempfile::TempDir {
    let project = copy_fixture(rule);
    let mut rules = serde_json::to_value(fallow_config::RulesConfig::default()).unwrap();
    for value in rules.as_object_mut().unwrap().values_mut() {
        *value = json!("off");
    }
    rules[rule] = json!(severity);
    std::fs::write(
        project.path().join(".fallowrc.json"),
        json!({"rules": rules, "duplicates": {"enabled": false}}).to_string(),
    )
    .unwrap();
    project
}

#[test]
fn component_errors_enforce_dead_code_and_combined_gates() {
    for (rule, collection, findings) in [
        ("thin-wrapper", "thin_wrappers", 1),
        ("duplicate-prop-shape", "duplicate_prop_shapes", 3),
    ] {
        for (severity, expected_total, expected_code) in
            [("error", findings, 1), ("warn", 0, 0), ("off", 0, 0)]
        {
            let project = project(rule, severity);
            let root = project.path().to_str().unwrap();
            for strict in [false, true] {
                let mut args = vec!["--quiet", "--no-cache", "--format", "json"];
                if strict {
                    args.push("--fail-on-issues");
                }
                let output = run_fallow_in_root("dead-code", project.path(), &args);
                let report = parse_json(&output);
                assert_eq!(
                    output.code, expected_code,
                    "{rule}={severity}: {}",
                    output.stdout
                );
                assert_eq!(report["total_issues"], expected_total);
                assert_eq!(
                    report["gate_outcomes"]["error-severity-findings"]["observed"],
                    json!(f64::from(u32::try_from(expected_total).unwrap()))
                );
                if severity == "off" {
                    assert!(report[collection].as_array().is_none_or(Vec::is_empty));
                } else {
                    let issues = report[collection].as_array().unwrap();
                    assert_eq!(issues.len(), findings);
                    for finding in issues {
                        assert_eq!(finding["effective_severity"], severity);
                        assert_eq!(finding["actions"][0]["type"], "suppress-line");
                    }
                }
                let mut args = vec![
                    "--root",
                    root,
                    "--quiet",
                    "--no-cache",
                    "--format",
                    "json",
                    "--ci",
                    "--only",
                    "dead-code",
                ];
                if strict {
                    args.push("--fail-on-issues");
                }
                let output = run_fallow_raw(&args);
                assert_eq!(
                    output.code, expected_code,
                    "{rule}={severity}: {}",
                    output.stdout
                );
                assert_eq!(parse_json(&output)["check"]["total_issues"], expected_total);
            }
        }
    }
}

fn dead_code(root: &std::path::Path, extra: &[&str]) -> crate::common::CommandOutput {
    let mut args = vec!["--quiet", "--no-cache", "--format", "json"];
    args.extend_from_slice(extra);
    run_fallow_in_root("dead-code", root, &args)
}

#[test]
fn component_source_overrides_control_enforcement_and_detection() {
    for (rule, collection, findings) in [
        ("thin-wrapper", "thin_wrappers", 1),
        ("duplicate-prop-shape", "duplicate_prop_shapes", 3),
    ] {
        for (base, source, expected_code, expected_findings) in [
            ("off", "error", 1, findings),
            ("error", "warn", 0, findings),
            ("error", "off", 0, 0),
        ] {
            let project = project(rule, base);
            let path = project.path().join(".fallowrc.json");
            let mut config: Value =
                serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
            config["overrides"] = json!([{"files": ["src/**"], "rules": {rule: source}}]);
            std::fs::write(path, config.to_string()).unwrap();
            let output = dead_code(project.path(), &["--fail-on-issues"]);
            assert_eq!(
                output.code, expected_code,
                "{rule} {base}/{source}: {}",
                output.stdout
            );
            let json = parse_json(&output);
            assert_eq!(
                json[collection].as_array().map_or(0, Vec::len),
                expected_findings
            );
            if expected_findings > 0 {
                assert_eq!(json[collection][0]["effective_severity"], source);
            }
        }
    }
    let project = project("duplicate-prop-shape", "warn");
    let path = project.path().join(".fallowrc.json");
    let mut config: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    config["overrides"] =
        json!([{"files": ["src/form.tsx"], "rules": {"duplicate-prop-shape": "error"}}]);
    std::fs::write(path, config.to_string()).unwrap();
    let output = dead_code(project.path(), &["--fail-on-issues"]);
    let report = parse_json(&output);
    assert_eq!(output.code, 1, "{}", output.stdout);
    assert_eq!(report["total_issues"], 1);
    assert_eq!(report["summary"]["duplicate_prop_shapes"], 1);
    assert_eq!(
        report["duplicate_prop_shapes"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|finding| finding["effective_severity"] == "warn")
            .count(),
        2
    );
}

#[test]
fn component_baselines_filter_reviewed_findings() {
    for (rule, collection, expected) in [
        ("thin-wrapper", "thin_wrappers", 1),
        ("duplicate-prop-shape", "duplicate_prop_shapes", 3),
    ] {
        let project = project(rule, "error");
        let baseline = project.path().join("baseline.json");
        let baseline_arg = baseline.to_str().unwrap();
        let before = dead_code(project.path(), &["--save-baseline", baseline_arg]);
        assert_eq!(before.code, 1, "{}", before.stdout);
        let saved: Value =
            serde_json::from_str(&std::fs::read_to_string(&baseline).unwrap()).unwrap();
        assert_eq!(saved[collection].as_array().unwrap().len(), expected);
        let reviewed = dead_code(
            project.path(),
            &["--baseline", baseline_arg, "--fail-on-issues"],
        );
        assert_eq!(reviewed.code, 0, "{}", reviewed.stdout);
        assert_eq!(parse_json(&reviewed)["total_issues"], 0);
        assert!(
            parse_json(&reviewed)[collection]
                .as_array()
                .is_none_or(Vec::is_empty)
        );
    }
}

fn audit(root: &std::path::Path, gate: &str) -> crate::common::CommandOutput {
    run_fallow_in_root(
        "audit",
        root,
        &[
            "--base",
            "main",
            "--gate",
            gate,
            "--quiet",
            "--no-cache",
            "--format",
            "json",
        ],
    )
}

#[test]
fn component_audit_distinguishes_inherited_and_introduced_findings() {
    for (rule, collection, source, expected) in [
        ("thin-wrapper", "thin_wrappers", "src/App.tsx", 1),
        (
            "duplicate-prop-shape",
            "duplicate_prop_shapes",
            "src/form.tsx",
            1,
        ),
    ] {
        let project = project(rule, "error");
        git(project.path(), &["init", "-q", "-b", "main"]);
        commit_all(project.path(), "base");
        git(project.path(), &["checkout", "-q", "-b", "feat/change"]);
        let path = project.path().join(source);
        let original = std::fs::read_to_string(&path).unwrap();
        std::fs::write(
            &path,
            format!("{original}\n// Preserve existing component identities.\n"),
        )
        .unwrap();
        commit_all(project.path(), "touch source");
        for (gate, code) in [("new-only", 0), ("all", 1)] {
            let output = audit(project.path(), gate);
            let report = parse_json(&output);
            assert_eq!(output.code, code, "{rule}/{gate}: {}", output.stdout);
            assert_eq!(
                report["attribution"]["dead_code_introduced"], 0,
                "{rule}: inherited or advisory stage: {}",
                output.stdout
            );
            if gate == "new-only" {
                assert_eq!(
                    report["attribution"]["dead_code_inherited"], expected,
                    "{rule}/{gate}: inherited stage: {}",
                    output.stdout
                );
                assert_eq!(report["dead_code"][collection][0]["introduced"], false);
            } else {
                assert_eq!(report["summary"]["dead_code_has_errors"], true);
                assert_eq!(report["summary"]["dead_code_issues"], expected);
            }
        }
        let source = if rule == "thin-wrapper" {
            original
                .replace("const Wrapper", "const RenamedWrapper")
                .replace("<Wrapper ", "<RenamedWrapper ")
        } else {
            original.replace("FieldTextarea", "RenamedTextarea")
        };
        std::fs::write(path, source).unwrap();
        if rule == "duplicate-prop-shape" {
            let main = project.path().join("src/main.tsx");
            let source = std::fs::read_to_string(&main).unwrap();
            std::fs::write(main, source.replace("FieldTextarea", "RenamedTextarea")).unwrap();
        }
        commit_all(project.path(), "rename component");
        let output = audit(project.path(), "new-only");
        let report = parse_json(&output);
        assert_eq!(output.code, 1, "{rule}: {}", output.stdout);
        assert_eq!(
            report["attribution"]["dead_code_introduced"], 1,
            "{rule}: renamed component stage: {}",
            output.stdout
        );
        let config_path = project.path().join(".fallowrc.json");
        let mut config: Value =
            serde_json::from_str(&std::fs::read_to_string(&config_path).unwrap()).unwrap();
        config["rules"][rule] = json!("warn");
        std::fs::write(config_path, config.to_string()).unwrap();
        commit_all(project.path(), "advisory policy");
        for gate in ["all", "new-only"] {
            let output = audit(project.path(), gate);
            assert_eq!(output.code, 0, "{}", output.stdout);
            let report = parse_json(&output);
            assert_eq!(
                report["attribution"]["dead_code_introduced"], 0,
                "{rule}: inherited or advisory stage: {}",
                output.stdout
            );
            assert_eq!(report["attribution"]["dead_code_inherited"], 0);
            assert_eq!(
                report["dead_code"][collection][0]["effective_severity"],
                "warn"
            );
        }
    }
}

#[test]
fn component_ci_formats_and_saved_reports_keep_enforced_errors() {
    for (rule, collection, title) in [
        ("thin-wrapper", "thin_wrappers", "Thin wrapper"),
        (
            "duplicate-prop-shape",
            "duplicate_prop_shapes",
            "Duplicate prop shape",
        ),
    ] {
        let project = project(rule, "error");
        let saved = project.path().join("saved.json");
        let report = parse_json(&dead_code(project.path(), &[]));
        for historical in [false, true] {
            let mut archive = report.clone();
            if historical {
                archive["total_issues"] = json!(0);
                archive["summary"]["total_issues"] = json!(0);
                archive["summary"]
                    .as_object_mut()
                    .unwrap()
                    .remove(collection);
            }
            std::fs::write(&saved, archive.to_string()).unwrap();
            for format in [
                "sarif",
                "codeclimate",
                "github-annotations",
                "github-summary",
            ] {
                let direct = run_fallow_in_root(
                    "dead-code",
                    project.path(),
                    &["--quiet", "--no-cache", "--format", format],
                );
                assert_eq!(direct.code, 1, "{rule}/{format}: {}", direct.stdout);
                let restored = run_fallow_raw(&[
                    "report",
                    "--from",
                    saved.to_str().unwrap(),
                    "--root",
                    project.path().to_str().unwrap(),
                    "--quiet",
                    "--format",
                    format,
                ]);
                assert_eq!(restored.code, 0, "{rule}/{format}: {}", restored.stdout);
                for output in [&direct, &restored] {
                    assert_ci_error(output, format, rule, title);
                }
            }
        }
        let mut corrupt = report;
        corrupt["total_issues"] = json!(19);
        std::fs::write(&saved, corrupt.to_string()).unwrap();
        let output = run_fallow_raw(&[
            "report",
            "--from",
            saved.to_str().unwrap(),
            "--quiet",
            "--format",
            "github-summary",
        ]);
        assert_eq!(
            output.code, 2,
            "must reject a corrupt current counter: {}",
            output.stdout
        );
    }
}

fn assert_ci_error(output: &crate::common::CommandOutput, format: &str, rule: &str, title: &str) {
    match format {
        "sarif" => {
            let report = parse_json(output);
            let findings = report["runs"][0]["results"].as_array().unwrap();
            assert_eq!(findings[0]["ruleId"], format!("fallow/{rule}"));
            assert_eq!(findings[0]["level"], "error");
        }
        "codeclimate" => {
            let report = parse_json(output);
            assert_eq!(report[0]["check_name"], format!("fallow/{rule}"));
            assert_eq!(report[0]["severity"], "major");
            assert_eq!(
                report[0]["location"]["path"],
                if rule == "thin-wrapper" {
                    "src/App.tsx"
                } else {
                    "src/fields.tsx"
                }
            );
        }
        "github-annotations" => {
            assert!(
                output.stdout.contains("::error file=src/"),
                "{}",
                output.stdout
            );
            assert!(output.stdout.contains(title), "{}", output.stdout);
        }
        _ => {
            assert!(output.stdout.contains(title), "{}", output.stdout);
            assert!(output.stdout.contains("src/"), "{}", output.stdout);
        }
    }
}
