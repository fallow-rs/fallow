#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests and benches use unwrap and expect to keep fixture setup concise"
)]

#[path = "common/mod.rs"]
mod common;

use common::{
    fixture_path, parse_json, redact_all, run_fallow, run_fallow_combined, run_fallow_in_root,
    run_fallow_raw, run_fallow_raw_with_env, run_fallow_raw_with_type_aware_sidecar,
};

#[test]
fn check_with_issues_exits_1() {
    let output = run_fallow("check", "basic-project", &["--format", "json", "--quiet"]);
    assert_eq!(
        output.code, 1,
        "check should exit 1 when error-severity issues found"
    );
}

#[test]
fn check_warn_severity_exits_0_without_fail_flag() {
    let output = run_fallow(
        "check",
        "config-file-project",
        &["--unused-files", "--format", "json", "--quiet"],
    );
    assert_eq!(
        output.code, 0,
        "check with only warn-severity issues should exit 0 without --fail-on-issues"
    );
    let json = parse_json(&output);
    assert!(
        json["total_issues"].as_u64().unwrap_or(0) > 0,
        "config-file-project should have warn-severity unused files"
    );
}

#[test]
fn check_warn_severity_exits_1_with_fail_on_issues() {
    let output = run_fallow(
        "check",
        "config-file-project",
        &[
            "--unused-files",
            "--fail-on-issues",
            "--format",
            "json",
            "--quiet",
        ],
    );
    assert_eq!(
        output.code, 1,
        "--fail-on-issues should promote warns to errors and exit 1"
    );
}

#[test]
fn check_ci_flag_implies_fail_on_issues() {
    let output = run_fallow("check", "basic-project", &["--ci", "--format", "json"]);
    assert_eq!(output.code, 1, "--ci should imply --fail-on-issues");
}

#[test]
fn check_json_format_produces_valid_json() {
    let output = run_fallow("check", "basic-project", &["--format", "json", "--quiet"]);
    let json = parse_json(&output);
    assert!(
        json.get("schema_version").is_some(),
        "JSON output should have schema_version"
    );
    assert!(json.is_object(), "JSON output should be an object");
}

#[test]
fn empty_type_aware_candidate_set_starts_no_companion() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"clean-type-aware","exports":"./src/index.ts"}"#,
    )
    .unwrap();
    std::fs::write(
        root.join("tsconfig.json"),
        r#"{"compilerOptions":{"strict":true,"noEmit":true},"include":["src/**/*.ts"]}"#,
    )
    .unwrap();
    std::fs::write(root.join("src/index.ts"), "export const live = 1;\n").unwrap();
    let root_arg = root.to_string_lossy();
    let missing_companion = root.join("missing-type-aware-companion");
    let missing_companion_arg = missing_companion.to_string_lossy();

    let output = run_fallow_raw_with_env(
        &[
            "dead-code",
            "--root",
            &root_arg,
            "--type-aware",
            "--unused-exports",
            "--format",
            "json",
            "--quiet",
        ],
        &[("FALLOW_TYPE_AWARE_BIN", &missing_companion_arg)],
    );

    assert_eq!(
        output.code, 0,
        "stdout: {}\nstderr: {}",
        output.stdout, output.stderr
    );
    let json = parse_json(&output);
    assert_eq!(json["total_issues"], 0);
    assert_eq!(json["_meta"]["type_aware"]["elapsed_ms"], 0);
    assert_eq!(
        json["_meta"]["type_aware"]["identity"]["capabilities"],
        serde_json::json!(["symbol-use"])
    );
}

#[test]
fn focused_type_aware_trace_rejects_unsupported_output_formats() {
    let output = run_fallow(
        "dead-code",
        "basic-project",
        &[
            "--type-aware",
            "--trace",
            "src/index.ts:anotherUnused3",
            "--format",
            "compact",
            "--quiet",
        ],
    );

    assert_eq!(output.code, 2);
    assert!(
        output
            .stderr
            .contains("focused trace and impact queries support human and JSON output"),
        "stderr: {}",
        output.stderr
    );
    assert!(output.stdout.is_empty(), "stdout: {}", output.stdout);
}

#[test]
fn configured_type_aware_accepts_gitlab_review_renderer() {
    let root = fixture_path("type-aware-unused-export-refinement");
    let root_arg = root.to_string_lossy();
    let config_dir = tempfile::tempdir().expect("type-aware config directory");
    let config_path = config_dir.path().join("fallow.json");
    std::fs::write(&config_path, r#"{"typeAware":{"enabled":true}}"#)
        .expect("write type-aware config");
    let config_arg = config_path.to_string_lossy();
    let output = run_fallow_raw_with_type_aware_sidecar(&[
        "dead-code",
        "--root",
        &root_arg,
        "--config",
        &config_arg,
        "--unused-exports",
        "--unused-types",
        "--format",
        "review-gitlab",
        "--quiet",
    ]);

    assert_eq!(output.code, 1, "stderr: {}", output.stderr);
    let envelope = parse_json(&output);
    assert_eq!(envelope["meta"]["schema"], "fallow-review-envelope/v3");
    let rendered = output.stdout;
    assert!(
        !rendered.contains("PublicApi")
            && !rendered.contains("PublicComplex")
            && !rendered.contains("PublicMerged"),
        "semantically used findings leaked into the review: {rendered}"
    );
    assert!(rendered.contains("actuallyUnused"), "review: {rendered}");
}

#[test]
fn explicit_type_aware_accepts_gitlab_sticky_comment_renderer() {
    let root = fixture_path("type-aware-unused-export-refinement");
    let root_arg = root.to_string_lossy();
    let output = run_fallow_raw_with_type_aware_sidecar(&[
        "dead-code",
        "--root",
        &root_arg,
        "--type-aware",
        "--unused-exports",
        "--unused-types",
        "--format",
        "pr-comment-gitlab",
        "--quiet",
    ]);

    assert_eq!(output.code, 1, "stderr: {}", output.stderr);
    assert!(output.stdout.contains("<!-- fallow-id: fallow-results -->"));
    for suppressed in ["PublicApi", "PublicComplex", "PublicMerged"] {
        assert!(
            !output.stdout.contains(suppressed),
            "{suppressed} leaked into the sticky comment: {}",
            output.stdout
        );
    }
    assert_eq!(output.stdout.matches("actuallyUnused").count(), 1);
}

#[test]
fn required_type_aware_review_fails_closed_when_companion_is_missing() {
    let root = fixture_path("type-aware-unused-export-refinement");
    let root_arg = root.to_string_lossy();
    let missing = root.join("missing-type-aware-companion");
    let missing_arg = missing.to_string_lossy();
    let output = run_fallow_raw_with_env(
        &[
            "dead-code",
            "--root",
            &root_arg,
            "--type-aware",
            "--type-aware-require",
            "complete",
            "--unused-exports",
            "--format",
            "review-gitlab",
            "--quiet",
        ],
        &[("FALLOW_TYPE_AWARE_BIN", &missing_arg)],
    );

    assert_eq!(output.code, 2);
    assert!(output.stdout.is_empty(), "stdout: {}", output.stdout);
    assert!(
        output.stderr.contains("Type-aware analysis failed")
            && !output.stderr.contains("Quality gate passed"),
        "stderr: {}",
        output.stderr
    );
}

#[test]
fn type_aware_class_method_impact_uses_exact_owner_identity() {
    let root = fixture_path("type-aware-class-method-impact");
    let root_arg = root.to_string_lossy();
    let output = run_fallow_raw_with_type_aware_sidecar(&[
        "dead-code",
        "--root",
        &root_arg,
        "--type-aware",
        "--symbol-impact",
        "src/repository.ts:UserRepository.save",
        "--format",
        "json",
        "--quiet",
    ]);

    assert_eq!(
        output.code, 0,
        "stdout: {}\nstderr: {}",
        output.stdout, output.stderr
    );
    let json = parse_json(&output);
    assert_eq!(json["target"]["owner"], serde_json::json!("UserRepository"));
    assert_eq!(json["target"]["local_name"], serde_json::json!("save"));
    let direct_consumers = json["direct_consumers"]
        .as_array()
        .expect("direct consumers");
    assert!(
        direct_consumers
            .iter()
            .any(|consumer| consumer["path"] == "src/service.ts")
    );
    assert!(
        direct_consumers
            .iter()
            .all(|consumer| consumer["path"] != "src/repository.ts"),
        "the same-named AuditRepository.save declaration is not a consumer"
    );

    let preview = run_fallow_raw_with_type_aware_sidecar(&[
        "fix",
        "--root",
        &root_arg,
        "--type-aware",
        "--dry-run",
        "--format",
        "json",
        "--quiet",
    ]);
    assert_eq!(
        preview.code, 0,
        "stdout: {}\nstderr: {}",
        preview.stdout, preview.stderr
    );
    let preview = parse_json(&preview);
    let fixes = preview["fixes"].as_array().expect("fix preview");
    assert!(fixes.iter().any(|fix| {
        fix["type"] == "remove_class_member"
            && fix["parent"] == "UserRepository"
            && fix["name"] == "purge"
            && fix["closed_world_eligible"] == true
    }));
    assert!(
        fixes
            .iter()
            .all(|fix| !(fix["parent"] == "UserRepository" && fix["name"] == "save")),
        "an exact call must prevent a class-member fix"
    );
}

#[test]
fn type_aware_refines_ambiguous_unused_exports_without_unsafe_fixes() {
    let root = fixture_path("type-aware-unused-export-refinement");
    let root_arg = root.to_string_lossy();
    let args = [
        "dead-code",
        "--root",
        &root_arg,
        "--unused-exports",
        "--unused-types",
        "--format",
        "json",
        "--quiet",
    ];

    let syntactic = parse_json(&run_fallow_raw(&args));
    let syntactic_exports = syntactic["unused_exports"]
        .as_array()
        .expect("unused exports");
    let syntactic_types = syntactic["unused_types"].as_array().expect("unused types");
    assert!(syntactic_exports.iter().any(|issue| {
        matches!(
            issue["export_name"].as_str(),
            Some("PublicApi" | "PublicMerged")
        )
    }));
    assert!(
        syntactic_types
            .iter()
            .any(|issue| { issue["export_name"] == "PublicComplex" })
    );

    let type_aware_args = [
        "dead-code",
        "--root",
        &root_arg,
        "--type-aware",
        "--unused-exports",
        "--unused-types",
        "--format",
        "json",
        "--quiet",
    ];
    let typed_output = run_fallow_raw_with_type_aware_sidecar(&type_aware_args);
    assert_ne!(
        typed_output.code, 2,
        "stdout: {}\nstderr: {}",
        typed_output.stdout, typed_output.stderr
    );
    let typed = parse_json(&typed_output);
    let typed_exports = typed["unused_exports"].as_array().expect("unused exports");
    let typed_types = typed["unused_types"].as_array().expect("unused types");

    for confirmed_used in ["PublicApi", "PublicComplex", "PublicMerged"] {
        assert!(
            typed_exports
                .iter()
                .chain(typed_types)
                .all(|issue| issue["export_name"] != confirmed_used),
            "{confirmed_used} should be removed after exact semantic use is confirmed"
        );
    }

    let runtime_only = typed_exports
        .iter()
        .find(|issue| issue["export_name"] == "RuntimeOnly")
        .expect("dynamic import candidate");
    assert_eq!(runtime_only["actions"][0]["auto_fixable"], false);

    let actually_unused = typed_exports
        .iter()
        .find(|issue| issue["export_name"] == "actuallyUnused")
        .expect("confirmed unused export");
    assert_eq!(actually_unused["actions"][0]["auto_fixable"], true);

    let decisions = typed["_meta"]["type_aware"]["candidate_decisions"]
        .as_array()
        .expect("candidate decisions");
    let runtime_decision = decisions
        .iter()
        .find(|decision| decision["subject"]["exported_name"] == "RuntimeOnly")
        .expect("dynamic import decision");
    assert_eq!(runtime_decision["decision"], "retained-abstained");
    assert_eq!(runtime_decision["reason_code"], "dynamic-behavior");

    let unused_decision = decisions
        .iter()
        .find(|decision| decision["subject"]["exported_name"] == "actuallyUnused")
        .expect("unused export decision");
    assert_eq!(
        unused_decision["decision"],
        "confirmed-no-static-references"
    );
}

#[test]
fn type_aware_framework_contract_requires_package_provenance() {
    let dir = tempfile::tempdir().expect("temporary project");
    let root = dir.path();
    std::fs::create_dir_all(root.join("src")).expect("create source directory");
    std::fs::create_dir_all(root.join("node_modules/lit")).expect("create fake lit package");
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"framework-contract","private":true,"type":"module","main":"src/index.ts","dependencies":{"lit":"1.0.0"}}"#,
    )
    .expect("write package");
    std::fs::write(
        root.join("tsconfig.json"),
        r#"{"compilerOptions":{"module":"nodenext","moduleResolution":"nodenext","strict":true},"include":["src/**/*.ts"]}"#,
    )
    .expect("write tsconfig");
    std::fs::write(
        root.join("node_modules/lit/package.json"),
        r#"{"name":"lit","version":"1.0.0","types":"index.d.ts"}"#,
    )
    .expect("write fake lit manifest");
    std::fs::write(
        root.join("node_modules/lit/index.d.ts"),
        "export declare class LitElement {}\n",
    )
    .expect("write fake lit declaration");
    std::fs::write(
        root.join("src/real.ts"),
        "import { LitElement } from \"lit\";\nexport class RealElement extends LitElement {\n  render(): unknown { return null; }\n}\n",
    )
    .expect("write package-backed class");
    std::fs::write(
        root.join("src/local.ts"),
        "class LitElement {}\nexport class LocalElement extends LitElement {\n  render(): unknown { return null; }\n}\n",
    )
    .expect("write local same-name class");
    std::fs::write(
        root.join("src/index.ts"),
        "import { RealElement } from \"./real.js\";\nimport { LocalElement } from \"./local.js\";\nnew RealElement();\nnew LocalElement();\n",
    )
    .expect("write entry point");

    let root_arg = root.to_string_lossy();
    let output = run_fallow_raw_with_type_aware_sidecar(&[
        "dead-code",
        "--root",
        &root_arg,
        "--unused-class-members",
        "--type-aware",
        "--format",
        "json",
        "--quiet",
    ]);

    assert_eq!(
        output.code, 1,
        "stdout: {}\nstderr: {}",
        output.stdout, output.stderr
    );
    let json = parse_json(&output);
    let decisions = json["_meta"]["type_aware"]["candidate_decisions"]
        .as_array()
        .unwrap_or_else(|| panic!("semantic decisions missing: {}", output.stdout));
    let real = decisions
        .iter()
        .find(|decision| decision["subject"]["owner"] == "RealElement")
        .expect("real framework method decision");
    assert_eq!(real["decision"], "contract-preserved");
    assert_eq!(real["framework_contract"]["package"], "lit");
    assert!(
        real["explanation"]
            .as_str()
            .is_some_and(|explanation| explanation.contains("lit contract"))
    );
    let local = decisions
        .iter()
        .find(|decision| decision["subject"]["owner"] == "LocalElement")
        .expect("local same-name method decision");
    assert_eq!(local["decision"], "confirmed-no-static-references");
    assert!(local.get("framework_contract").is_none());
    assert_eq!(local["closed_world_eligible"], true);
}

#[test]
fn duplicate_export_add_to_config_is_auto_fixable_with_explicit_config() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir_all(root.join("src/one")).unwrap();
    std::fs::create_dir_all(root.join("src/two")).unwrap();
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"explicit-config","main":"src/index.ts"}"#,
    )
    .unwrap();
    let config_path = root.join("custom.fallow.json");
    std::fs::write(&config_path, "{}\n").unwrap();
    std::fs::write(
        root.join("src/index.ts"),
        "export { Button } from './one';\nexport { Button as Button2 } from './two';\nconsole.log(Button2);\n",
    )
    .unwrap();
    std::fs::write(root.join("src/one/index.ts"), "export const Button = 1;\n").unwrap();
    std::fs::write(root.join("src/two/index.ts"), "export const Button = 2;\n").unwrap();

    let output = run_fallow_in_root(
        "dead-code",
        root,
        &[
            "--config",
            config_path.to_str().unwrap(),
            "--duplicate-exports",
            "--format",
            "json",
            "--quiet",
        ],
    );
    assert_eq!(
        output.code, 1,
        "duplicate export should be reported: stdout={}, stderr={}",
        output.stdout, output.stderr
    );

    let json = parse_json(&output);
    let actions = json["duplicate_exports"][0]["actions"].as_array().unwrap();
    assert_eq!(actions[0]["type"], "add-to-config");
    assert_eq!(actions[0]["auto_fixable"], true);
}

#[test]
fn combined_performance_includes_duplication_stage() {
    let output = run_fallow_combined(
        "duplicate-code",
        &["--only", "dead-code,dupes", "--performance", "--quiet"],
    );
    assert!(
        output.code == 0 || output.code == 1,
        "combined performance run should not crash: stdout={}\nstderr={}",
        output.stdout,
        output.stderr
    );
    assert!(
        output.stderr.contains("Pipeline Performance"),
        "combined --performance should print pipeline table: {}",
        output.stderr
    );
    assert!(
        output.stderr.contains("duplication:"),
        "pipeline table should include duplication stage: {}",
        output.stderr
    );
}

/// Combined mode runs check and dupes via `rayon::join`. Verify the parallel
/// scheduling does not leak nondeterminism into the rendered JSON: repeated
/// runs against the same fixture must produce byte-identical output once the
/// inherently nondeterministic wall-clock fields are stripped.
#[test]
fn combined_parallel_output_is_deterministic() {
    fn normalize(value: &mut serde_json::Value) {
        match value {
            serde_json::Value::Object(map) => {
                map.remove("elapsed_ms");
                if let Some(telemetry) = map
                    .get_mut("_meta")
                    .and_then(|meta| meta.get_mut("telemetry"))
                    .and_then(|telemetry| telemetry.as_object_mut())
                {
                    telemetry.remove("analysis_run_id");
                }
                for v in map.values_mut() {
                    normalize(v);
                }
            }
            serde_json::Value::Array(items) => {
                for v in items {
                    normalize(v);
                }
            }
            _ => {}
        }
    }

    let mut canonicalized: Vec<String> = std::iter::repeat_with(|| {
        let output = run_fallow_combined(
            "duplicate-code",
            &["--only", "dead-code,dupes", "--format", "json", "--quiet"],
        );
        assert!(
            output.code == 0 || output.code == 1,
            "combined run should not crash: stdout={}\nstderr={}",
            output.stdout,
            output.stderr
        );
        let mut value = parse_json(&output);
        normalize(&mut value);
        serde_json::to_string(&value).expect("re-serialize canonical json")
    })
    .take(3)
    .collect();

    let first = canonicalized.remove(0);
    for (idx, run) in canonicalized.iter().enumerate() {
        assert_eq!(
            &first,
            run,
            "combined parallel run #{} differed from run #0",
            idx + 1
        );
    }
}

#[test]
fn check_compact_format_has_no_ansi() {
    let output = run_fallow(
        "check",
        "basic-project",
        &["--format", "compact", "--quiet"],
    );
    assert!(
        !output.stdout.contains("\x1b["),
        "compact output should have no ANSI escape sequences"
    );
    assert!(
        !output.stdout.trim().is_empty(),
        "compact output should not be empty for project with issues"
    );
}

#[test]
fn check_sarif_format_has_schema() {
    let output = run_fallow("check", "basic-project", &["--format", "sarif", "--quiet"]);
    let json = parse_json(&output);
    assert!(
        json.get("$schema").is_some(),
        "SARIF output should have $schema key"
    );
}

#[test]
fn check_markdown_format_has_heading() {
    let output = run_fallow(
        "check",
        "basic-project",
        &["--format", "markdown", "--quiet"],
    );
    assert!(
        output.stdout.contains('#'),
        "markdown output should contain heading markers"
    );
}

#[test]
fn check_codeclimate_format_is_array() {
    let output = run_fallow(
        "check",
        "basic-project",
        &["--format", "codeclimate", "--quiet"],
    );
    let json: serde_json::Value = serde_json::from_str(&output.stdout).unwrap_or_else(|e| {
        panic!(
            "failed to parse codeclimate JSON: {e}\nstdout: {}",
            output.stdout
        )
    });
    assert!(json.is_array(), "codeclimate output should be a JSON array");
}

#[test]
fn check_gitlab_codequality_alias_is_array() {
    let output = run_fallow(
        "check",
        "basic-project",
        &["--format", "gitlab-codequality", "--quiet"],
    );
    let json: serde_json::Value = serde_json::from_str(&output.stdout).unwrap_or_else(|e| {
        panic!(
            "failed to parse gitlab-codequality JSON: {e}\nstdout: {}",
            output.stdout
        )
    });
    assert!(
        json.is_array(),
        "gitlab-codequality output should be a JSON array"
    );
}

#[test]
fn check_unused_files_filter_limits_output() {
    let output = run_fallow(
        "check",
        "basic-project",
        &["--unused-files", "--format", "json", "--quiet"],
    );
    let json = parse_json(&output);
    assert!(
        json.get("unused_files").is_some(),
        "should have unused_files when filtered"
    );
    let unused_exports = json["unused_exports"].as_array();
    assert!(
        unused_exports.is_none() || unused_exports.unwrap().is_empty(),
        "unused_exports should be empty when only --unused-files"
    );
}

#[test]
fn check_multiple_filters_combined() {
    let output = run_fallow(
        "check",
        "basic-project",
        &[
            "--unused-files",
            "--unused-exports",
            "--format",
            "json",
            "--quiet",
        ],
    );
    let json = parse_json(&output);
    assert!(
        json.get("unused_files").is_some(),
        "should have unused_files"
    );
    assert!(
        json.get("unused_exports").is_some(),
        "should have unused_exports"
    );
}

#[test]
fn check_unused_deps_filter() {
    let output = run_fallow(
        "check",
        "basic-project",
        &["--unused-deps", "--format", "json", "--quiet"],
    );
    let json = parse_json(&output);
    assert!(
        json.get("unused_dependencies").is_some(),
        "should have unused_dependencies"
    );
}

#[test]
fn check_json_has_total_issues() {
    let output = run_fallow("check", "basic-project", &["--format", "json", "--quiet"]);
    let json = parse_json(&output);
    assert!(
        json.get("total_issues").is_some(),
        "JSON should have total_issues"
    );
    assert!(
        json["total_issues"].as_u64().unwrap() > 0,
        "basic-project should have issues"
    );
}

#[test]
fn check_json_has_version_and_elapsed() {
    let output = run_fallow("check", "basic-project", &["--format", "json", "--quiet"]);
    let json = parse_json(&output);
    assert!(json.get("version").is_some(), "JSON should have version");
    assert!(
        json.get("elapsed_ms").is_some(),
        "JSON should have elapsed_ms"
    );
}

#[test]
fn check_invalid_root_exits_2() {
    let output = run_fallow_raw(&["check", "--root", "/nonexistent/path/xyz", "--quiet"]);
    assert_eq!(output.code, 2, "invalid root should exit with code 2");
}

#[test]
fn check_json_error_format() {
    let output = run_fallow_raw(&[
        "check",
        "--root",
        "/nonexistent/path/xyz",
        "--format",
        "json",
        "--quiet",
    ]);
    assert_eq!(output.code, 2);
    let json: serde_json::Value = serde_json::from_str(&output.stdout).unwrap_or_else(|e| {
        panic!(
            "error output should be valid JSON: {e}\nstdout: {}",
            output.stdout
        )
    });
    assert!(
        json.get("error").is_some(),
        "error JSON should have 'error' field"
    );
}

#[test]
fn check_human_output_unused_files_only() {
    let output = run_fallow("check", "basic-project", &["--unused-files", "--quiet"]);
    let root = fixture_path("basic-project");
    let redacted = redact_all(&output.stdout, &root);
    insta::assert_snapshot!("check_human_unused_files_only", redacted);
}

#[test]
fn check_human_output_unused_exports_only() {
    let output = run_fallow("check", "basic-project", &["--unused-exports", "--quiet"]);
    let root = fixture_path("basic-project");
    let redacted = redact_all(&output.stdout, &root);
    insta::assert_snapshot!("check_human_unused_exports_only", redacted);
}

fn combined_check_unused_export_names(json: &serde_json::Value) -> Vec<String> {
    json["check"]["unused_exports"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|v| v["export_name"].as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn include_entry_exports_works_in_combined_mode() {
    let output = run_fallow_combined(
        "entry-export-validation",
        &["--include-entry-exports", "--format", "json", "--quiet"],
    );
    assert!(
        !output.stderr.contains("unexpected argument")
            && !output.stderr.contains("error: unrecognized argument"),
        "combined mode must accept --include-entry-exports; stderr: {}",
        output.stderr
    );
    let json = parse_json(&output);
    let names = combined_check_unused_export_names(&json);
    assert!(
        names.iter().any(|n| n == "meatdata"),
        "meatdata typo should be flagged in combined mode with --include-entry-exports, got: {names:?}"
    );
}

#[test]
fn include_entry_exports_via_config_file_in_combined_mode() {
    let output = run_fallow_combined(
        "entry-export-validation-config",
        &["--format", "json", "--quiet"],
    );
    let json = parse_json(&output);
    let names = combined_check_unused_export_names(&json);
    assert!(
        names.iter().any(|n| n == "meatdata"),
        "meatdata should be flagged via includeEntryExports in config, got: {names:?}"
    );
}

#[test]
fn check_human_output_unused_deps_has_content() {
    let output = run_fallow("check", "basic-project", &["--unused-deps", "--quiet"]);
    assert!(
        output.stdout.contains("Unused dependencies"),
        "unused-deps output should contain section header"
    );
    assert!(
        output.stdout.contains("unused-dep"),
        "should list unused-dep"
    );
}

/// Build a project with one unused file under `src/legacy` and one under `src`,
/// plus the given `ignoreFindings` patterns.
fn ignore_findings_project(patterns: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("temporary project");
    let root = dir.path();
    std::fs::create_dir_all(root.join("src/legacy")).expect("create source directories");
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"ignore-findings","private":true,"type":"module","main":"src/index.ts"}"#,
    )
    .expect("write package");
    std::fs::write(
        root.join(".fallowrc.json"),
        format!(r#"{{"ignoreFindings": {patterns}}}"#),
    )
    .expect("write config");
    std::fs::write(
        root.join("src/index.ts"),
        "export const main = (): void => {};\n",
    )
    .expect("write entry point");
    std::fs::write(root.join("src/legacy/old.ts"), "export const old = 1;\n")
        .expect("write legacy file");
    std::fs::write(root.join("src/orphan.ts"), "export const orphan = 1;\n")
        .expect("write orphan file");
    dir
}

#[test]
fn ignore_findings_pattern_matching_nothing_prints_note() {
    let dir = ignore_findings_project(r#"["src/legacy/**", "src/legcy/**"]"#);
    let output = run_fallow_in_root("dead-code", dir.path(), &["--unused-files"]);

    assert!(
        output
            .stderr
            .contains("ignoreFindings pattern matched no finding")
            && output.stderr.contains("src/legcy/**"),
        "stderr should name the pattern that matched nothing; stderr: {}",
        output.stderr
    );
    assert!(
        !output.stderr.contains("src/legacy/**"),
        "the matching pattern should not be named; stderr: {}",
        output.stderr
    );
}

#[test]
fn ignore_findings_pattern_that_matches_prints_no_note() {
    let dir = ignore_findings_project(r#"["src/legacy/**"]"#);
    let output = run_fallow_in_root("dead-code", dir.path(), &["--unused-files"]);

    assert!(
        !output.stderr.contains("ignoreFindings"),
        "a matching pattern should stay silent; stderr: {}",
        output.stderr
    );
}

#[test]
fn no_ignore_findings_configuration_prints_no_note() {
    let dir = ignore_findings_project("[]");
    let output = run_fallow_in_root("dead-code", dir.path(), &["--unused-files"]);

    assert!(
        !output.stderr.contains("ignoreFindings"),
        "an empty configuration should stay silent; stderr: {}",
        output.stderr
    );
}

#[test]
fn ignore_findings_note_stays_out_of_json_output() {
    let dir = ignore_findings_project(r#"["src/legacy/**", "src/legcy/**"]"#);
    let output = run_fallow_in_root(
        "dead-code",
        dir.path(),
        &["--unused-files", "--format", "json"],
    );

    assert!(
        !output.stdout.contains("ignoreFindings") && !output.stderr.contains("ignoreFindings"),
        "json output must not carry the human note; stdout: {}\nstderr: {}",
        output.stdout,
        output.stderr
    );
}

/// Issue #2358: a bun.lockb-only repo with overrides gets no unused-override
/// findings (resolution is unreadable); the JSON envelope must explain the
/// skip through `workspace_diagnostics[]` and the human run must warn.
#[test]
fn bun_lockb_only_override_skip_surfaces_in_json_and_human_output() {
    let output = run_fallow(
        "dead-code",
        "issue-2358-bun-lockb-diagnostic",
        &["--format", "json", "--quiet", "--no-cache"],
    );
    let json = parse_json(&output);
    assert!(
        json["unused_dependency_overrides"]
            .as_array()
            .is_none_or(Vec::is_empty),
        "the unused-override check must stay skipped: {}",
        json["unused_dependency_overrides"]
    );
    let diagnostics = json["workspace_diagnostics"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let skips: Vec<&serde_json::Value> = diagnostics
        .iter()
        .filter(|diagnostic| diagnostic["kind"] == "bun-lockb-override-resolution-skipped")
        .collect();
    assert_eq!(
        skips.len(),
        1,
        "exactly one skip diagnostic for the root manifest: {diagnostics:?}"
    );
    assert_eq!(skips[0]["path"], "package.json");
    let message = skips[0]["message"].as_str().unwrap_or_default();
    assert!(
        message.contains("only bun.lockb was found")
            && message.contains("bun install --save-text-lockfile"),
        "message states the cause and the text-lockfile next step: {message}"
    );

    let root = fixture_path("issue-2358-bun-lockb-diagnostic");
    let human = run_fallow_raw_with_env(
        &[
            "dead-code",
            "--root",
            root.to_str().expect("fixture path is UTF-8"),
            "--no-cache",
        ],
        &[("RUST_LOG", "warn")],
    );
    assert!(
        human.stderr.contains("only bun.lockb was found")
            && human.stderr.contains("bun install --save-text-lockfile"),
        "human run warns about the skip on stderr; stderr: {}",
        human.stderr
    );
}
