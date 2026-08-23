#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests and benches use unwrap and expect to keep fixture setup concise"
)]

#[path = "common/mod.rs"]
mod common;

use common::{
    fixture_path, parse_json, redact_all, redact_paths, run_fallow, run_fallow_combined,
    run_fallow_in_root, run_fallow_raw, run_fallow_raw_with_env,
    run_fallow_raw_with_type_aware_sidecar,
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
        message.contains("no parseable text lockfile")
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
        human.stderr.contains("no parseable text lockfile")
            && human.stderr.contains("bun install --save-text-lockfile"),
        "human run warns about the skip on stderr; stderr: {}",
        human.stderr
    );
}

/// Issue #2367: a bun repo that pins versions under Yarn-style `resolutions`
/// gets unused-override findings in JSON output, sourced to `package.json`
/// with the bun hint naming `resolutions`.
#[test]
fn bun_resolutions_surface_as_unused_overrides_in_json_output() {
    let output = run_fallow(
        "dead-code",
        "issue-2367-bun-resolutions",
        &["--format", "json", "--quiet", "--no-cache"],
    );
    let json = parse_json(&output);
    let findings = json["unused_dependency_overrides"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let mut keys: Vec<&str> = findings
        .iter()
        .filter_map(|finding| finding["raw_key"].as_str())
        .collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        vec!["**/trim-newlines", "left-pad"],
        "the two unresolved resolutions pins are reported: {findings:?}"
    );
    for finding in &findings {
        assert_eq!(finding["source"], "package.json");
        assert_eq!(finding["path"], "package.json");
        let hint = finding["hint"].as_str().unwrap_or_default();
        assert!(
            hint.contains("resolutions") && hint.contains("bun install --frozen-lockfile"),
            "the bun hint names the resolutions origin: {hint}"
        );
    }
    assert!(
        json["workspace_diagnostics"]
            .as_array()
            .is_none_or(Vec::is_empty),
        "a parseable bun.lock resolves normally: {}",
        json["workspace_diagnostics"]
    );
}

/// The #2371 probe: `src/impl.ts` exports a value whose only consumer is the
/// bound `import type` in `src/index.ts`.
fn write_type_only_import_probe(root: &std::path::Path) {
    std::fs::create_dir_all(root.join("src")).expect("create source directory");
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"probe","type":"module","main":"src/index.ts"}"#,
    )
    .expect("write package.json");
    std::fs::write(
        root.join("src/index.ts"),
        "import type { helper } from './impl';\nexport type T = typeof helper;\n",
    )
    .expect("write index.ts");
    std::fs::write(
        root.join("src/impl.ts"),
        "export const helper = (): number => 1;\n",
    )
    .expect("write impl.ts");
}

/// A `tsconfig.json` the sidecar can select for a probe under `src`.
fn write_probe_tsconfig(root: &std::path::Path) {
    std::fs::write(
        root.join("tsconfig.json"),
        r#"{"compilerOptions":{"strict":true,"module":"ESNext","moduleResolution":"Bundler","target":"ES2022","noEmit":true},"include":["src"]}"#,
    )
    .expect("write tsconfig.json");
}

/// Issue #2371: a value-only export whose only credit is a bound
/// `import type` is not reported by dead-code, and the trace must say so
/// through the type namespace instead of contradicting the verdict.
#[test]
fn trace_reports_the_type_lane_credit_of_a_value_only_export() {
    let dir = tempfile::tempdir().expect("temporary project");
    let root = dir.path();
    write_type_only_import_probe(root);

    let verdict = parse_json(&run_fallow_in_root(
        "dead-code",
        root,
        &["--format", "json", "--quiet", "--no-cache"],
    ));
    assert_eq!(
        verdict["unused_exports"].as_array().map(Vec::len),
        Some(0),
        "the type-only import credits the value export: {}",
        verdict["unused_exports"]
    );

    let trace = parse_json(&run_fallow_in_root(
        "dead-code",
        root,
        &[
            "--trace",
            "src/impl.ts:helper",
            "--format",
            "json",
            "--quiet",
            "--no-cache",
        ],
    ));
    assert_eq!(trace["kind"], "trace");
    assert_eq!(trace["namespace"], "type");
    assert_eq!(trace["is_used"], true);
    assert_eq!(trace["direct_references"][0]["from_file"], "src/index.ts");
    assert_eq!(trace["direct_references"][0]["kind"], "named import");
    assert_eq!(trace["reason"], "Used by 1 file(s)");

    let human = run_fallow_in_root(
        "dead-code",
        root,
        &["--trace", "src/impl.ts:helper", "--quiet", "--no-cache"],
    );
    // The human renderer prints native paths, so normalize separators before
    // matching: on Windows the same lines read `src\impl.ts`.
    let stderr = redact_paths(&human.stderr, root);
    assert!(
        stderr.contains("USED helper in src/impl.ts")
            && stderr.contains("Namespace: type")
            && stderr.contains("-> src/index.ts (named import)"),
        "human trace reports the type-lane credit; stderr: {stderr}"
    );
}

/// Issue #2371: the checker proof beside the syntactic trace covers the lane
/// the declaration occupies, and the sidecar does not model a cross-lane
/// import as a reference. The payload must stay readable across that gap: the
/// root trace reports the credit, `semantic.target.namespace` names the lane
/// the narrower proof covers, and the human proof line says so.
#[test]
fn type_aware_trace_scopes_the_proof_that_misses_the_type_lane_credit() {
    let dir = tempfile::tempdir().expect("temporary project");
    let root = dir.path();
    write_type_only_import_probe(root);
    write_probe_tsconfig(root);
    let root_arg = root.to_string_lossy();

    let output = run_fallow_raw_with_type_aware_sidecar(&[
        "dead-code",
        "--root",
        &root_arg,
        "--trace",
        "src/impl.ts:helper",
        "--type-aware",
        "--format",
        "json",
        "--quiet",
        "--no-cache",
    ]);

    let trace = parse_json(&output);
    assert_eq!(trace["namespace"], "type", "stderr: {}", output.stderr);
    assert_eq!(trace["is_used"], true);
    assert_eq!(trace["direct_references"][0]["from_file"], "src/index.ts");
    assert_eq!(
        trace["semantic"]["target"]["namespace"], "value",
        "the proof covers the declaration's own lane: {}",
        trace["semantic"]
    );
    // Deliberate negative control: the sidecar does not credit a cross-lane
    // import, so the proof is narrower than the graph here. It pins the known
    // gap, not a behaviour this change introduces.
    assert_eq!(trace["semantic"]["assertion"], "no-references-found");

    let human = run_fallow_raw_with_type_aware_sidecar(&[
        "dead-code",
        "--root",
        &root_arg,
        "--trace",
        "src/impl.ts:helper",
        "--type-aware",
        "--quiet",
        "--no-cache",
    ]);
    let stderr = redact_paths(&human.stderr, root);
    assert!(
        stderr.contains("Type-aware proof: no-references-found (complete, value namespace only)"),
        "the proof line names the lane it covers; stderr: {stderr}"
    );
}

fn combined_root_diagnostics_of_kind(
    json: &serde_json::Value,
    kind: &str,
) -> Vec<serde_json::Value> {
    json["workspace_diagnostics"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter(|diagnostic| diagnostic["kind"] == kind)
        .collect()
}

/// Write a project whose `pnpm-workspace.yaml` does not parse, the second
/// analysis-stage diagnostic kind, and return its temp dir.
fn malformed_pnpm_workspace_project() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        dir.path().join("package.json"),
        r#"{"name":"issue-2366-malformed-pnpm-workspace-yaml","private":true}"#,
    )
    .expect("write package.json");
    std::fs::write(
        dir.path().join("pnpm-workspace.yaml"),
        "catalog:\n  react: ^18.2.0\n{this is\nnot: valid: yaml: at: all\n",
    )
    .expect("write malformed pnpm-workspace.yaml");
    std::fs::create_dir_all(dir.path().join("src")).expect("create src");
    std::fs::write(
        dir.path().join("src/index.ts"),
        "export const greet = (name: string): string => `hello ${name}`;\n",
    )
    .expect("write source");
    dir
}

/// Issue #2366: the bare combined run (`fallow --format json`) must carry the
/// analysis-stage workspace diagnostics that `dead-code --format json` carries.
/// The combined root is the single carrier, so no section repeats the array.
/// Both kinds the analyze stage records are covered: the bun.lockb override
/// skip and a malformed `pnpm-workspace.yaml`.
#[test]
fn combined_json_root_carries_analysis_stage_workspace_diagnostics() {
    let output = run_fallow_combined(
        "issue-2358-bun-lockb-diagnostic",
        &["--format", "json", "--quiet", "--no-cache"],
    );
    let json = parse_json(&output);
    let skips = combined_root_diagnostics_of_kind(&json, "bun-lockb-override-resolution-skipped");
    assert_eq!(
        skips.len(),
        1,
        "exactly one bun.lockb skip diagnostic on the combined root: {}",
        json["workspace_diagnostics"]
    );
    assert_eq!(skips[0]["path"], "package.json");

    let dir = malformed_pnpm_workspace_project();
    let output = run_fallow_raw(&[
        "--root",
        dir.path().to_str().expect("temp path is UTF-8"),
        "--format",
        "json",
        "--quiet",
        "--no-cache",
    ]);
    let json = parse_json(&output);
    let malformed = combined_root_diagnostics_of_kind(&json, "malformed-pnpm-workspace-yaml");
    assert_eq!(
        malformed.len(),
        1,
        "exactly one malformed yaml diagnostic on the combined root: {}",
        json["workspace_diagnostics"]
    );
    assert_eq!(malformed[0]["path"], "pnpm-workspace.yaml");
    assert!(
        json["check"].is_object() && json["dupes"].is_object() && json["health"].is_object(),
        "all three sections ran, so the absence checks below are not vacuous: {json}"
    );
    assert!(
        json["check"].get("workspace_diagnostics").is_none()
            && json["dupes"].get("workspace_diagnostics").is_none()
            && json["health"].get("workspace_diagnostics").is_none(),
        "the root is the only carrier; no section repeats the array: {json}"
    );
}

/// Issue #2366: the carrier is unconditional, so a combined run that drops the
/// `check` section still reports what its analyses recorded. `--skip check`
/// and `--only health` both still run a dead-code analyze pass, which is what
/// records the analysis-stage kinds and warns on stderr.
#[test]
fn combined_json_carries_workspace_diagnostics_without_a_check_section() {
    for section_flags in [
        ["--skip", "check"].as_slice(),
        ["--only", "health"].as_slice(),
    ] {
        let mut args = vec!["--format", "json", "--quiet", "--no-cache"];
        args.extend_from_slice(section_flags);
        let output = run_fallow_combined("issue-2358-bun-lockb-diagnostic", &args);
        let json = parse_json(&output);
        assert!(
            json.get("check").is_none(),
            "{section_flags:?} drops the check section: {json}"
        );
        let skips =
            combined_root_diagnostics_of_kind(&json, "bun-lockb-override-resolution-skipped");
        assert_eq!(
            skips.len(),
            1,
            "{section_flags:?} still carries the skip diagnostic: {}",
            json["workspace_diagnostics"]
        );
        assert_eq!(skips[0]["path"], "package.json");
    }
}

/// Issue #2366: `--only dupes` runs no dead-code analyze pass, so it records no
/// analysis-stage kind, but the workspace-discovery diagnostics config load
/// records still reach the combined root.
#[test]
fn combined_json_only_dupes_carries_workspace_discovery_diagnostics() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::create_dir_all(dir.path().join("packages/no-manifest/src")).expect("create packages");
    std::fs::create_dir_all(dir.path().join("src")).expect("create src");
    std::fs::write(
        dir.path().join("package.json"),
        r#"{"name":"issue-2366-only-dupes","private":true,"main":"src/index.ts","workspaces":["packages/*"]}"#,
    )
    .expect("write package.json");
    std::fs::write(dir.path().join("src/index.ts"), "export const value = 1;\n")
        .expect("write source");
    std::fs::write(
        dir.path().join("packages/no-manifest/src/a.ts"),
        "export const other = 2;\n",
    )
    .expect("write workspace source");

    let output = run_fallow_raw(&[
        "--root",
        dir.path().to_str().expect("temp path is UTF-8"),
        "--format",
        "json",
        "--quiet",
        "--no-cache",
        "--only",
        "dupes",
    ]);
    let json = parse_json(&output);
    assert!(
        json["dupes"].is_object() && json.get("check").is_none() && json.get("health").is_none(),
        "only the dupes section ran: {json}"
    );
    let unmatched = combined_root_diagnostics_of_kind(&json, "glob-matched-no-package-json");
    assert_eq!(
        unmatched.len(),
        1,
        "the workspace glob diagnostic reaches a dupes-only combined run: {}",
        json["workspace_diagnostics"]
    );
    assert_eq!(unmatched[0]["path"], "packages/no-manifest");
}

/// Write a project whose test file is over the `--max-file-size 1` ceiling, so
/// a NON-production walk records `skipped-large-file` for it and a production
/// walk (which excludes test files) never sees it. `production_config` is the
/// `.fallowrc.json` body that splits the per-analysis production modes.
fn split_production_large_test_file_project(production_config: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        dir.path().join("package.json"),
        r#"{"name":"issue-2366-split-production","private":true,"main":"src/index.ts"}"#,
    )
    .expect("write package.json");
    std::fs::write(dir.path().join(".fallowrc.json"), production_config)
        .expect("write .fallowrc.json");
    std::fs::create_dir_all(dir.path().join("src")).expect("create src");
    std::fs::write(dir.path().join("src/index.ts"), "export const value = 1;\n")
        .expect("write source");
    std::fs::write(
        dir.path().join("src/huge.test.ts"),
        "// filler\n".repeat(150_000),
    )
    .expect("write oversized test file");
    dir
}

/// Issue #2366: a combined run walks the project once per analysis, and a
/// per-analysis `production` mode gives those walks different file sets, so
/// each walk records a different source-discovery list and clears the previous
/// one. The combined root must report the UNION of what the run recorded, in
/// both directions of the split, otherwise the answer depends on which walk
/// happened to run last and the root contradicts the standalone `dead-code`
/// envelope of the same project.
#[test]
fn combined_json_root_unions_workspace_diagnostics_across_split_production_modes() {
    for production_config in [
        r#"{"production":{"deadCode":true,"health":false,"dupes":false}}"#,
        r#"{"production":{"deadCode":false,"health":true,"dupes":true}}"#,
    ] {
        let dir = split_production_large_test_file_project(production_config);
        let root = dir.path().to_str().expect("temp path is UTF-8");
        let output = run_fallow_raw(&[
            "--root",
            root,
            "--max-file-size",
            "1",
            "--format",
            "json",
            "--quiet",
            "--no-cache",
        ]);
        let json = parse_json(&output);
        assert!(
            json["check"].is_object() && json["dupes"].is_object() && json["health"].is_object(),
            "all three sections ran under {production_config}: {json}"
        );
        let skipped = combined_root_diagnostics_of_kind(&json, "skipped-large-file");
        assert_eq!(
            skipped.len(),
            1,
            "the combined root reports the oversized file under {production_config}: {}",
            json["workspace_diagnostics"]
        );
        assert_eq!(skipped[0]["path"], "src/huge.test.ts");
    }
}

/// Issue #2366: the combined root and the programmatic combined envelope are
/// built from different inputs (the CLI folds the process registry per
/// analysis phase, the programmatic route folds the typed sections' own
/// lists), so pin that a non-production dead-code pass under a production
/// health/dupes split reaches the standalone envelope and the combined root
/// alike. Without the union the combined root is empty here while
/// `dead-code --format json` on the same project reports the entry.
#[test]
fn combined_json_root_matches_standalone_dead_code_under_a_production_split() {
    let dir = split_production_large_test_file_project(
        r#"{"production":{"deadCode":false,"health":true,"dupes":true}}"#,
    );
    let root = dir.path().to_str().expect("temp path is UTF-8");
    let standalone = parse_json(&run_fallow_raw(&[
        "dead-code",
        "--root",
        root,
        "--max-file-size",
        "1",
        "--format",
        "json",
        "--quiet",
        "--no-cache",
    ]));
    let combined = parse_json(&run_fallow_raw(&[
        "--root",
        root,
        "--max-file-size",
        "1",
        "--format",
        "json",
        "--quiet",
        "--no-cache",
    ]));
    assert_eq!(
        standalone["workspace_diagnostics"], combined["workspace_diagnostics"],
        "the combined root carries the standalone dead-code list: standalone {} vs combined {}",
        standalone["workspace_diagnostics"], combined["workspace_diagnostics"]
    );
    assert_eq!(
        combined_root_diagnostics_of_kind(&combined, "skipped-large-file").len(),
        1,
        "the comparison above is not vacuous: {}",
        combined["workspace_diagnostics"]
    );
}

/// Issue #2366: with `--production-dead-code --production-health` the only
/// analysis that walks the full file set is duplication, so the oversized test
/// file is recorded by the dupes walk alone and neither the dead-code nor the
/// health section's own list carries it. The combined root must still report
/// it, from the registry read that closes the fold.
#[test]
fn combined_json_root_carries_a_diagnostic_only_the_dupes_walk_recorded() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        dir.path().join("package.json"),
        r#"{"name":"issue-2366-dupes-only-carrier","private":true,"main":"src/index.ts"}"#,
    )
    .expect("write package.json");
    std::fs::create_dir_all(dir.path().join("src")).expect("create src");
    std::fs::write(dir.path().join("src/index.ts"), "export const value = 1;\n")
        .expect("write source");
    std::fs::write(
        dir.path().join("src/huge.test.ts"),
        "// filler\n".repeat(150_000),
    )
    .expect("write oversized test file");

    let json = parse_json(&run_fallow_raw(&[
        "--root",
        dir.path().to_str().expect("temp path is UTF-8"),
        "--max-file-size",
        "1",
        "--production-dead-code",
        "--production-health",
        "--format",
        "json",
        "--quiet",
        "--no-cache",
    ]));
    assert!(json["dupes"].is_object(), "the dupes section ran: {json}");
    let skipped = combined_root_diagnostics_of_kind(&json, "skipped-large-file");
    assert_eq!(
        skipped.len(),
        1,
        "the combined root reports what only the dupes walk saw: {}",
        json["workspace_diagnostics"]
    );
    assert_eq!(skipped[0]["path"], "src/huge.test.ts");
}
