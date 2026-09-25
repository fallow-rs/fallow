#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests and benches use unwrap and expect to keep fixture setup concise"
)]

use crate::common::{
    canonical_report, canonical_report_without_gate_outcomes, fixture_path, parse_json, redact_all,
    redact_paths, run_fallow, run_fallow_combined, run_fallow_in_root, run_fallow_raw,
    run_fallow_raw_with_env, run_fallow_raw_with_type_aware_sidecar,
};

#[test]
fn check_with_issues_exits_1() {
    let output = run_fallow("check", "basic-project", &["--format", "json", "--quiet"]);
    assert_eq!(
        output.code, 1,
        "check should exit 1 when error-severity issues found"
    );
    let json = parse_json(&output);
    assert!(
        json.get("schema_version").is_some(),
        "JSON output should have schema_version"
    );
    assert!(
        json["total_issues"].as_u64().unwrap() > 0,
        "basic-project should have issues"
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

/// The fixture from issue #2445: an import from the `domain` zone into the
/// `adapter` zone that both allow-nothing rules forbid, optionally with a
/// per-path override that matches neither side of the import.
fn boundary_violation_project(with_unrelated_override: bool) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("temporary project");
    let root = dir.path();
    std::fs::create_dir_all(root.join("src/domain")).expect("create domain directory");
    std::fs::create_dir_all(root.join("src/adapter")).expect("create adapter directory");
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"boundary-override-repro","private":true,"type":"module"}"#,
    )
    .expect("write package");
    let overrides = if with_unrelated_override {
        r#""overrides": [{ "files": ["src/other/**"], "rules": { "unused-exports": "off" } }],"#
    } else {
        ""
    };
    std::fs::write(
        root.join(".fallowrc.json"),
        format!(
            r#"{{
  "entry": ["src/main.ts"],
  "rules": {{ "boundary-violation": "error" }},
  {overrides}
  "boundaries": {{
    "zones": [
      {{ "name": "domain", "patterns": ["src/domain/**"] }},
      {{ "name": "adapter", "patterns": ["src/adapter/**"] }}
    ],
    "rules": [
      {{ "from": "domain", "allow": [] }},
      {{ "from": "adapter", "allow": [] }}
    ]
  }}
}}"#
        ),
    )
    .expect("write config");
    std::fs::write(root.join("src/main.ts"), "import \"./domain/value.ts\";\n")
        .expect("write entry point");
    std::fs::write(
        root.join("src/domain/value.ts"),
        "import { adapterValue } from \"../adapter/value.ts\";\nconsole.log(adapterValue);\n",
    )
    .expect("write domain file");
    std::fs::write(
        root.join("src/adapter/value.ts"),
        "export const adapterValue = 1;\n",
    )
    .expect("write adapter file");
    dir
}

const BOUNDARY_FAIL_ARGS: &[&str] = &[
    "--boundary-violations",
    "--fail-on-issues",
    "--no-cache",
    "--format",
    "json",
    "--quiet",
];

#[test]
fn boundary_violation_fails_on_issues_with_unrelated_override() {
    let dir = boundary_violation_project(true);
    let output = run_fallow_in_root("dead-code", dir.path(), BOUNDARY_FAIL_ARGS);
    let json = parse_json(&output);
    assert_eq!(
        json["boundary_violations"].as_array().map(Vec::len),
        Some(1),
        "the domain -> adapter import should be reported; stdout: {}",
        output.stdout
    );
    assert_eq!(
        output.code, 1,
        "an error-severity boundary violation must fail the run even when an unrelated per-path override exists"
    );
}

#[test]
fn boundary_violation_fails_on_issues_without_override() {
    let dir = boundary_violation_project(false);
    let output = run_fallow_in_root("dead-code", dir.path(), BOUNDARY_FAIL_ARGS);
    assert_eq!(
        output.code, 1,
        "an error-severity boundary violation must fail the run"
    );
}

/// A `warn` rule plus a per-path override that matches nothing relevant.
fn warn_with_unrelated_override_project() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("temporary project");
    let root = dir.path();
    std::fs::create_dir_all(root.join("src")).expect("create source directory");
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"warn-override-repro","private":true,"type":"module"}"#,
    )
    .expect("write package");
    std::fs::write(
        root.join(".fallowrc.json"),
        r#"{
  "entry": ["src/main.ts"],
  "rules": { "unused-exports": "warn" },
  "overrides": [{ "files": ["src/other/**"], "rules": { "unused-files": "off" } }]
}"#,
    )
    .expect("write config");
    std::fs::write(
        root.join("src/main.ts"),
        "import { used } from \"./lib.ts\";\nconsole.log(used);\n",
    )
    .expect("write entry point");
    std::fs::write(
        root.join("src/lib.ts"),
        "export const used = 1;\nexport const unused = 2;\n",
    )
    .expect("write library file");
    dir
}

#[test]
fn warn_rule_with_unrelated_override_exits_1_with_fail_on_issues() {
    let dir = warn_with_unrelated_override_project();
    let output = run_fallow_in_root(
        "dead-code",
        dir.path(),
        &[
            "--unused-exports",
            "--fail-on-issues",
            "--no-cache",
            "--format",
            "json",
            "--quiet",
        ],
    );
    assert_eq!(
        output.code, 1,
        "--fail-on-issues must promote a warn rule to error even when overrides exist; stdout: {}",
        output.stdout
    );
}

#[test]
fn warn_rule_with_unrelated_override_exits_0_without_fail_on_issues() {
    let dir = warn_with_unrelated_override_project();
    let output = run_fallow_in_root(
        "dead-code",
        dir.path(),
        &[
            "--unused-exports",
            "--no-cache",
            "--format",
            "json",
            "--quiet",
        ],
    );
    let json = parse_json(&output);
    assert!(
        json["total_issues"].as_u64().unwrap_or(0) > 0,
        "the unused export should be reported; stdout: {}",
        output.stdout
    );
    assert_eq!(
        output.code, 0,
        "a warn-only run without --fail-on-issues exits 0"
    );
}

/// A pnpm project with one unused dependency override in `package.json`. The
/// base rule has `base` severity, and an `overrides` entry for `package.json`
/// sets `manifest` severity.
fn dependency_override_severity_project(base: &str, manifest: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("temporary project");
    let root = dir.path().canonicalize().expect("canonical root");
    std::fs::create_dir_all(root.join("src")).expect("create source directory");
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"override-severity","private":true,"type":"module","main":"src/index.ts","pnpm":{"overrides":{"@scope/legacy-pkg":"^1.0.0"}}}"#,
    )
    .expect("write package");
    std::fs::write(
        root.join(".fallowrc.json"),
        format!(
            r#"{{
  "rules": {{ "unused-dependency-overrides": "{base}" }},
  "overrides": [{{ "files": ["package.json"], "rules": {{ "unused-dependency-overrides": "{manifest}" }} }}]
}}"#
        ),
    )
    .expect("write config");
    std::fs::write(root.join("src/index.ts"), "export const a = 1;\n").expect("write entry point");
    dir
}

fn run_dependency_override_severity(base: &str, manifest: &str) -> (i32, serde_json::Value) {
    let dir = dependency_override_severity_project(base, manifest);
    let root = dir.path().canonicalize().expect("canonical root");
    let output = run_fallow_in_root(
        "dead-code",
        &root,
        &["--no-cache", "--format", "json", "--quiet"],
    );
    let json = parse_json(&output);
    let overrides: Vec<&str> = json["unused_dependency_overrides"]
        .as_array()
        .expect("unused_dependency_overrides array")
        .iter()
        .filter_map(|finding| finding["target_package"].as_str())
        .collect();
    assert_eq!(
        overrides,
        vec!["@scope/legacy-pkg"],
        "the unused override is reported; stdout: {}",
        output.stdout
    );
    (output.code, json)
}

#[test]
fn dependency_override_rule_override_to_warn_exits_0() {
    let (code, json) = run_dependency_override_severity("error", "warn");
    assert_eq!(
        code, 0,
        "the `overrides` entry sets `warn` for package.json, so the base `error` does not fail the run: {}",
        json["gate_outcomes"]
    );
}

#[test]
fn dependency_override_rule_override_to_error_exits_1() {
    let (code, json) = run_dependency_override_severity("warn", "error");
    assert_eq!(
        code, 1,
        "the `overrides` entry sets `error` for package.json, so the run fails: {}",
        json["gate_outcomes"]
    );
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

/// Type-aware reconciliation appends a private-type leak the syntactic pass
/// never produced, so the CLI resolves rule severities again over the refined
/// set. Without that second pass a path whose override turns the rule off is
/// still reported, while the editor suppresses it.
#[test]
fn type_aware_reconciliation_respects_per_path_rule_overrides() {
    let root = fixture_path("type-aware-private-type-leak-overrides");
    let root_arg = root.to_string_lossy();
    let output = run_fallow_raw_with_type_aware_sidecar(&[
        "dead-code",
        "--root",
        &root_arg,
        "--type-aware",
        "--private-type-leaks",
        "--format",
        "json",
        "--quiet",
        "--no-cache",
    ]);

    let json = parse_json(&output);
    let paths: Vec<&str> = json["private_type_leaks"]
        .as_array()
        .expect("private type leaks array")
        .iter()
        .map(|leak| leak["path"].as_str().expect("leak path"))
        .collect();

    assert!(
        paths.contains(&"src/lib/util.ts"),
        "the control leak outside the override must stay reported: {paths:?} (stderr: {})",
        output.stderr
    );
    assert!(
        !paths.contains(&"src/ui/kit.ts"),
        "a leak added by reconciliation on an overridden path must be dropped: {paths:?}"
    );
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
fn type_aware_preserves_reachable_aliases_and_reports_actual_unused_export() {
    let root = fixture_path("type-aware-unused-export-refinement");
    let root_arg = root.to_string_lossy();
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

    for reachable_alias in ["PublicApi", "PublicComplex", "PublicMerged"] {
        assert!(
            typed_exports
                .iter()
                .chain(typed_types)
                .all(|issue| issue["export_name"] != reachable_alias),
            "{reachable_alias} has a reachable exact consumer"
        );
    }

    let actually_unused = typed_exports
        .iter()
        .find(|issue| issue["export_name"] == "actuallyUnused")
        .expect("confirmed unused export");
    assert_eq!(actually_unused["actions"][0]["auto_fixable"], true);

    assert!(
        typed_exports
            .iter()
            .any(|issue| issue["export_name"] == "mixedNonCrediting"),
        "mixed unreachable-read and bare-re-export evidence must not hide dead code: {typed_exports:?}"
    );

    let decisions = typed["_meta"]["type_aware"]["candidate_decisions"]
        .as_array()
        .expect("candidate decisions");
    let unused_decision = decisions
        .iter()
        .find(|decision| decision["subject"]["exported_name"] == "actuallyUnused")
        .expect("unused export decision");
    assert_eq!(
        unused_decision["decision"],
        "confirmed-no-static-references"
    );

    let mixed_decision = decisions
        .iter()
        .find(|decision| decision["subject"]["exported_name"] == "mixedNonCrediting")
        .expect("mixed non-crediting evidence decision");
    assert_eq!(mixed_decision["decision"], "retained-abstained");
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

/// Read the `--performance` timings object that a JSON run writes to stderr.
fn performance_timings(output: &common::CommandOutput) -> serde_json::Value {
    output
        .stderr
        .lines()
        .filter(|line| line.trim_start().starts_with('{'))
        .find_map(|line| {
            serde_json::from_str::<serde_json::Value>(line)
                .ok()
                .filter(|value| value.get("total_ms").is_some())
        })
        .unwrap_or_else(|| panic!("no performance timings on stderr:\n{}", output.stderr))
}

fn cold_dead_code_counters(fixture: &str, threads: &str) -> serde_json::Value {
    let output = run_fallow(
        "dead-code",
        fixture,
        &[
            "--performance",
            "--no-cache",
            "--threads",
            threads,
            "--format",
            "json",
            "--quiet",
        ],
    );
    assert!(
        output.code == 0 || output.code == 1,
        "dead-code --performance should not crash: stdout={}\nstderr={}",
        output.stdout,
        output.stderr
    );
    performance_timings(&output)["counters"].clone()
}

/// The work counters are exact, so a change that repeats work fails here.
/// Update a number only when the work changed on purpose, and say why in the
/// commit.
#[test]
fn performance_counters_are_exact_on_pinned_fixtures() {
    let counters = |files_read: u64, bytes: u64, calls: u64, unique: u64, oxc: u64| {
        serde_json::json!({
            "files_read": files_read,
            "source_bytes_read": bytes,
            "parse_cache_bytes_read": 0,
            "resolve_specifier_calls": calls,
            "unique_specifiers": unique,
            "oxc_resolve_calls": oxc,
            "canonicalize_calls": 0,
        })
    };
    // basic-project: `import { anotherUnused2, usedFunction } from "./utils"`
    // asks twice for one specifier, so calls exceed unique specifiers.
    // barrel-exports: two bindings of `./barrel` plus four re-exports.
    // cjs-project: one `require('./utils')`.
    let cases = [
        ("basic-project", counters(4, 1176, 3, 2, 3)),
        ("barrel-exports", counters(5, 479, 6, 5, 6)),
        ("cjs-project", counters(3, 195, 1, 1, 1)),
    ];
    for (fixture, expected) in cases {
        assert_eq!(
            cold_dead_code_counters(fixture, "2"),
            expected,
            "work counters for {fixture}"
        );
    }
}

/// A warm run reports the exact size of the parse cache file that it read.
#[test]
fn performance_counters_report_the_parse_cache_bytes_read() {
    let project = common::copy_fixture("basic-project");
    // A local test run can leave a cache in the fixture, and the copy takes it.
    let _ = std::fs::remove_dir_all(project.path().join(".fallow"));
    let args = ["--performance", "--format", "json", "--quiet"];
    let cold = run_fallow_in_root("dead-code", project.path(), &args);
    assert_eq!(
        performance_timings(&cold)["counters"]["parse_cache_bytes_read"],
        0,
        "a first run has no cache to read: {}",
        cold.stderr
    );
    let cache_bytes = std::fs::metadata(project.path().join(".fallow/cache.bin"))
        .expect("the cold run writes the parse cache")
        .len();

    let warm = run_fallow_in_root("dead-code", project.path(), &args);
    assert_eq!(
        performance_timings(&warm)["counters"]["parse_cache_bytes_read"],
        cache_bytes
    );
}

/// Counters must not depend on scheduling: one worker and many workers do the
/// same work.
#[test]
fn performance_counters_do_not_depend_on_the_thread_count() {
    let one = cold_dead_code_counters("basic-project", "1");
    let many = cold_dead_code_counters("basic-project", "8");
    assert!(one.is_object(), "counters missing: {one}");
    assert_eq!(one, many);
}

fn span<'a>(spans: &'a [serde_json::Value], name: &str) -> &'a serde_json::Value {
    spans
        .iter()
        .find(|span| span["name"] == name)
        .unwrap_or_else(|| panic!("span {name} missing: {spans:#?}"))
}

/// The process clock covers the time outside the pipeline TOTAL, and the span
/// tree says which span holds which. The test checks structure and ordering
/// of the clocks, never a millisecond value.
#[test]
fn dead_code_performance_reports_the_process_clock_and_span_tree() {
    let output = run_fallow(
        "dead-code",
        "basic-project",
        &["--performance", "--format", "json", "--quiet"],
    );
    let timings = performance_timings(&output);
    let process = &timings["process"];
    let ms = |value: &serde_json::Value, key: &str| -> f64 {
        value[key]
            .as_f64()
            .unwrap_or_else(|| panic!("{key} missing: {value}"))
    };
    let children = [
        "startup_ms",
        "config_ms",
        "git_ms",
        "analysis_ms",
        "post_analysis_ms",
        "output_ms",
    ];
    let children_sum: f64 = children.iter().map(|key| ms(process, key)).sum();
    assert!(
        children_sum <= ms(process, "wall_ms") + 0.01,
        "the process spans are disjoint parts of the wall clock: {process}"
    );
    assert!(ms(process, "thread_pool_ms") <= ms(process, "startup_ms") + 0.01);
    let pipeline_sum: f64 = [
        "workspaces_ms",
        "discover_files_ms",
        "parse_extract_ms",
        "cache_update_ms",
        "total_ms",
    ]
    .iter()
    .map(|key| ms(&timings, key))
    .sum();
    assert!(
        pipeline_sum <= ms(process, "analysis_ms") + 0.05,
        "the pipeline stages run inside the analysis span: {timings}"
    );

    let spans = timings["spans"].as_array().expect("spans array");
    let roots: Vec<_> = spans
        .iter()
        .filter(|span| span["parent"].is_null())
        .collect();
    assert_eq!(roots.len(), 1, "one root span: {spans:#?}");
    assert_eq!(roots[0]["name"], "process");
    assert_eq!(span(spans, "pipeline")["parent"], "analysis");
    assert_eq!(span(spans, "parse_extract")["parent"], "analysis");
    assert_eq!(span(spans, "resolve_imports")["parent"], "pipeline");
    assert_eq!(span(spans, "output")["parent"], "process");
}

/// The human table closes with the process rows and a WALL row.
#[test]
fn dead_code_human_performance_shows_the_wall_row() {
    let output = run_fallow("dead-code", "basic-project", &["--performance", "--quiet"]);
    for row in ["startup:", "config:", "analysis:", "output:", "WALL:"] {
        assert!(
            output.stderr.contains(row),
            "{row} missing:\n{}",
            output.stderr
        );
    }
}

/// Combined mode marks duplication as a span of its own, with its concurrency.
#[test]
fn combined_performance_json_has_a_duplication_span() {
    let output = run_fallow_combined(
        "duplicate-code",
        &[
            "--only",
            "dead-code,dupes",
            "--performance",
            "--format",
            "json",
            "--quiet",
        ],
    );
    let timings = performance_timings(&output);
    let spans = timings["spans"].as_array().expect("spans array");
    let duplication = span(spans, "duplication");
    assert_eq!(duplication["parent"], "process");
    assert!(duplication["concurrent"].is_boolean(), "{duplication}");
}

/// Combined mode runs check and dupes via `rayon::join`. Verify the parallel
/// scheduling does not leak nondeterminism into the rendered JSON: repeated
/// runs against the same fixture must produce byte-identical output once the
/// inherently nondeterministic wall-clock fields are stripped.
#[test]
fn combined_parallel_output_is_deterministic() {
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
        canonical_report(&output)
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

/// GitHub code scanning keys an alert on `partialFingerprints`, so two results
/// that share one value are one alert and the second finding is never shown. A
/// compact `package.json` puts every dependency on one line, which used to give
/// them the same rule id, URI, and source snippet, and that is the whole of the
/// fingerprint. CodeClimate never had it: it keys on the package name.
#[test]
fn check_sarif_gives_each_finding_its_own_fingerprint() {
    let dir = tempfile::tempdir().expect("temporary project");
    let root = dir.path();
    std::fs::create_dir_all(root.join("src")).expect("create src");
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"compact","version":"1.0.0","main":"src/index.ts","dependencies":{"lodash":"^4.17.21","chalk":"^5.3.0"}}"#,
    )
    .expect("write manifest");
    std::fs::write(
        root.join("src/barrel.ts"),
        "export { alpha, beta } from './m';
",
    )
    .expect("write barrel");
    std::fs::write(
        root.join("src/m.ts"),
        "export const alpha = 1;
export const beta = 2;
",
    )
    .expect("write module");
    std::fs::write(
        root.join("src/index.ts"),
        "import './barrel';

export const run = (): void => {};
",
    )
    .expect("write entry module");

    let output = run_fallow_in_root(
        "check",
        root,
        &["--format", "sarif", "--quiet", "--no-cache"],
    );
    let sarif = parse_json(&output);
    let results = sarif
        .pointer("/runs/0/results")
        .and_then(serde_json::Value::as_array)
        .expect("SARIF results");

    let fingerprints: Vec<&str> = results
        .iter()
        .map(|result| {
            result
                .pointer("/partialFingerprints/tools.fallow.fingerprint~1v1")
                .and_then(serde_json::Value::as_str)
                .expect("fingerprint")
        })
        .collect();
    assert!(
        fingerprints.len() >= 4,
        "the fixture must still report the unused dependencies and the barrel re-exports: {}",
        output.stdout
    );
    let unique: std::collections::BTreeSet<&&str> = fingerprints.iter().collect();
    assert_eq!(
        unique.len(),
        fingerprints.len(),
        "two findings sharing a fingerprint are one GitHub alert: {}",
        output.stdout
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
    // A parseable bun.lock resolves normally, so no lockfile diagnostic is
    // recorded. Environment and unconfigured-detector diagnostics are a
    // different channel and a bare fixture always carries some, so this asserts
    // the absence of the bun kinds rather than an empty array.
    let bun_kind = json["workspace_diagnostics"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default()
        .iter()
        .filter_map(|entry| entry["kind"].as_str())
        .find(|kind| kind.starts_with("bun-"));
    assert!(
        bun_kind.is_none(),
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

/// Issue #2371: the checker proof beside the syntactic trace follows the local
/// alias read through a type-only import. The proof stays scoped to the value
/// declaration occupied by `helper`, while the root graph trace reports its
/// type-lane credit.
#[test]
fn type_aware_trace_proves_typeof_read_through_type_only_import() {
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
    assert_eq!(trace["semantic"]["assertion"], "references-found");

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
        stderr.contains("Type-aware proof: references-found (complete)")
            && stderr.contains("src/index.ts:2:23 (value-reference, Value)"),
        "the proof lists the value read through the local alias; stderr: {stderr}"
    );
}

#[test]
fn filtered_type_aware_dead_code_keeps_unreachable_only_export_evidence() {
    let root = fixture_path("issue-2390-trace-consistency");
    let root_arg = root.to_string_lossy();
    let output = run_fallow_raw_with_type_aware_sidecar(&[
        "dead-code",
        "--root",
        &root_arg,
        "--type-aware",
        "--unused-exports",
        "--format",
        "json",
        "--quiet",
        "--no-cache",
    ]);

    let report = parse_json(&output);
    let helper = report["unused_exports"]
        .as_array()
        .expect("unused exports")
        .iter()
        .find(|finding| finding["path"] == "src/lonely.ts" && finding["export_name"] == "helper")
        .unwrap_or_else(|| {
            panic!("unreachable-only checker evidence must not hide helper: {report}")
        });
    assert_eq!(helper["semantic"]["decision"], "retained-abstained");
}

#[test]
fn type_aware_trace_does_not_credit_an_unreachable_consumer() {
    let root = fixture_path("issue-2390-trace-consistency");
    let root_arg = root.to_string_lossy();
    let output = run_fallow_raw_with_type_aware_sidecar(&[
        "dead-code",
        "--root",
        &root_arg,
        "--trace",
        "src/lonely.ts:helper",
        "--type-aware",
        "--format",
        "json",
        "--quiet",
        "--no-cache",
    ]);

    let trace = parse_json(&output);
    assert_eq!(trace["is_used"], false);
    assert_eq!(
        trace["semantic"]["assertion"],
        "references-only-in-unreachable-files"
    );
    assert_eq!(trace["semantic"]["status"], "partial");
    assert_eq!(trace["semantic"]["references"][0]["path"], "src/orphan.ts");
}

/// Kinds only a combined run can carry, because the standalone `dead-code`
/// envelope never runs the health pipeline that records them (issue #2689).
const HEALTH_STAGE_KINDS: [&str; 7] = [
    "file-scores-unavailable",
    "hotspots-skipped",
    "shallow-clone",
    "unpinned-clock",
    "ownership-unavailable",
    "trend-snapshot-unreadable",
    "coverage-auto-detected",
];

/// Assert the combined root is never NARROWER than the standalone `dead-code`
/// envelope: it carries every entry that envelope carries, in the same order,
/// and anything extra comes from a section the standalone run never executed.
///
/// Equality was the right assertion while only the walks contributed. The
/// health section now records its own degraded inputs, so an extra entry is
/// expected and a MISSING one is still a failure.
fn assert_combined_root_covers_standalone(
    standalone: &serde_json::Value,
    combined: &serde_json::Value,
    context: &str,
) {
    let standalone_entries = standalone["workspace_diagnostics"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let combined_entries = combined["workspace_diagnostics"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let shared: Vec<serde_json::Value> = combined_entries
        .iter()
        .filter(|entry| !HEALTH_STAGE_KINDS.contains(&entry["kind"].as_str().unwrap_or_default()))
        .cloned()
        .collect();
    assert_eq!(
        standalone_entries, shared,
        "{context}: standalone {} vs combined {}",
        standalone["workspace_diagnostics"], combined["workspace_diagnostics"]
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

/// Issue #2366: one glob declared in both `package.json` and
/// `pnpm-workspace.yaml`, in the two spellings those files conventionally use,
/// is ONE diagnostic per matched directory.
///
/// Config load expands both manifests and records a diagnostic from each, so
/// the process registry the standalone envelopes read verbatim held the same
/// finding twice, with `./pkgs/*` and `pkgs/*` as its `pattern`. Keying the
/// fold on the typed payload would have kept both spellings as distinct
/// entries, doubling the array on a real monorepo, so the recorded pattern
/// drops the no-op `./`, the recorded path drops the matching no-op `.`
/// component, and workspace discovery folds its own list before returning it.
/// `list_tests` covers the same shape on the workspace listing envelope, which
/// reads that list instead of the registry.
#[test]
fn one_glob_declared_in_two_manifests_reports_one_diagnostic() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    std::fs::create_dir_all(root.join("pkgs/aaa")).expect("create package-less dir");
    std::fs::create_dir_all(root.join("src")).expect("create src");
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"issue-2366-two-manifests","private":true,"main":"src/index.ts","workspaces":["./pkgs/*"]}"#,
    )
    .expect("write package.json");
    std::fs::write(
        root.join("pnpm-workspace.yaml"),
        "packages:\n  - \"pkgs/*\"\n",
    )
    .expect("write pnpm-workspace.yaml");
    std::fs::write(root.join("src/index.ts"), "export const value = 1;\n").expect("write source");
    std::fs::write(root.join("pkgs/aaa/readme.txt"), "no package.json here\n")
        .expect("write filler");

    for args in [["dead-code"].as_slice(), ["list"].as_slice(), [].as_slice()] {
        let mut argv = args.to_vec();
        argv.extend_from_slice(&[
            "--root",
            root.to_str().expect("temp path is UTF-8"),
            "--format",
            "json",
            "--quiet",
            "--no-cache",
        ]);
        let json = parse_json(&run_fallow_raw(&argv));
        let patterns: Vec<String> = json["workspace_diagnostics"]
            .as_array()
            .map(|diagnostics| {
                diagnostics
                    .iter()
                    .filter(|entry| entry["kind"] == "glob-matched-no-package-json")
                    .map(|entry| entry["pattern"].as_str().unwrap_or_default().to_owned())
                    .collect()
            })
            .unwrap_or_default();
        assert_eq!(
            patterns,
            ["pkgs/*"],
            "`fallow {args:?}` reports the directory once: {}",
            json["workspace_diagnostics"]
        );
    }
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

/// Two oversized files, one production and one test, plus both analysis-stage
/// diagnostic kinds: under a `production` split every walk in the run skips a
/// different pair, so each analysis contributes its own source-discovery list
/// and the union has entries from more than one observation point.
fn split_production_two_large_files_project(production_config: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        dir.path().join("package.json"),
        r#"{"name":"issue-2366-repeat-runs","private":true,"main":"src/index.ts","overrides":{"ws":"^8.21.0"},"dependencies":{"ws":"^8.18.0"}}"#,
    )
    .expect("write package.json");
    std::fs::write(
        dir.path().join("bun.lockb"),
        b"\x00binary lockfile placeholder\x00",
    )
    .expect("write bun.lockb placeholder");
    std::fs::write(
        dir.path().join("pnpm-workspace.yaml"),
        "catalog:\n  react: ^18.2.0\n{this is\nnot: valid: yaml: at: all\n",
    )
    .expect("write malformed pnpm-workspace.yaml");
    std::fs::write(dir.path().join(".fallowrc.json"), production_config)
        .expect("write .fallowrc.json");
    std::fs::create_dir_all(dir.path().join("src")).expect("create src");
    std::fs::write(dir.path().join("src/index.ts"), "export const value = 1;\n")
        .expect("write source");
    std::fs::write(
        dir.path().join("src/huge.prod.ts"),
        "// filler\n".repeat(150_000),
    )
    .expect("write oversized production file");
    std::fs::write(
        dir.path().join("src/huge.test.ts"),
        "// filler\n".repeat(150_000),
    )
    .expect("write oversized test file");
    dir
}

/// Write a project whose only source outside `src/` sits in a dot-prefixed
/// directory discovery does not traverse, so a walk records exactly one
/// `skipped-source-dotdir` (issue #461).
fn skipped_source_dotdir_project() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        dir.path().join("package.json"),
        r#"{"name":"issue-461-skipped-source-dotdir","private":true,"main":"src/index.ts"}"#,
    )
    .expect("write package.json");
    std::fs::create_dir_all(dir.path().join("src")).expect("create src");
    std::fs::write(dir.path().join("src/index.ts"), "export const value = 1;\n")
        .expect("write source");
    std::fs::create_dir_all(dir.path().join(".claude/hooks")).expect("create .claude/hooks");
    std::fs::write(
        dir.path().join(".claude/hooks/probe.mjs"),
        "export const hook = () => 1;\n",
    )
    .expect("write hook");
    dir
}

/// Issue #461: the diagnostic has to survive combined mode's per-analysis
/// config reload to reach the JSON envelope, which is exactly what
/// `WorkspaceDiagnosticKind::is_source_discovery` decides. A kind left out of
/// that classifier passes every unit test in the workspace and is still wiped
/// from the combined root before serialization (the issue #1086 failure).
#[test]
fn combined_json_root_carries_the_skipped_source_dotdir_diagnostic() {
    let dir = skipped_source_dotdir_project();
    let root = dir.path().to_str().expect("temp path is UTF-8");
    let json = parse_json(&run_fallow_raw(&[
        "--root",
        root,
        "--format",
        "json",
        "--quiet",
        "--no-cache",
    ]));

    let reported = combined_root_diagnostics_of_kind(&json, "skipped-source-dotdir");
    assert_eq!(
        reported.len(),
        1,
        "the combined root reports the skip once: {}",
        json["workspace_diagnostics"]
    );
    assert_eq!(reported[0]["path"], ".claude");
    assert!(
        reported[0]["message"]
            .as_str()
            .is_some_and(|message| message.contains("ignoreExports")
                && message.contains("does not fix this run")),
        "the message names the remedy for the false positive in this run: {}",
        reported[0]["message"]
    );
}

/// Issue #461 plus issue #2366: the diagnostic must also be classified as
/// walk-recorded, so a concurrent walk's dotdir set is never folded into
/// another analysis's list. A kind missing from
/// `WorkspaceDiagnosticKind::is_source_walk_recorded` makes the combined
/// root's array order depend on which walk wrote last.
#[test]
fn combined_json_root_workspace_diagnostics_stay_byte_identical_with_a_skipped_dotdir() {
    let dir = skipped_source_dotdir_project();
    let root = dir.path().to_str().expect("temp path is UTF-8");
    let mut observed: Vec<serde_json::Value> = Vec::new();
    for _ in 0..6 {
        let json = parse_json(&run_fallow_raw(&[
            "--root",
            root,
            "--format",
            "json",
            "--quiet",
            "--no-cache",
        ]));
        observed.push(json["workspace_diagnostics"].clone());
    }
    for (index, run) in observed.iter().enumerate() {
        assert_eq!(
            run, &observed[0],
            "run {index} disagrees with the first run about the combined root's \
             workspace_diagnostics[]"
        );
    }
}

/// The two unconfigured-check diagnostics fire on every project that never
/// opted into boundaries or rule packs, which is the product's default state,
/// so a stderr warning for them is permanent noise on nearly every run. They
/// keep their `workspace_diagnostics[]` entries, where a consumer that wants to
/// tell "measured zero" from "measured nothing" can read them.
///
/// `node-modules-missing` in the same run is the control: it reports a real
/// degradation that changes results, so it stays on stderr and proves the
/// warning surface is live rather than filtered by the log level.
#[test]
fn unconfigured_check_diagnostics_stay_out_of_stderr_but_reach_json() {
    let root = fixture_path("basic-project");
    let output = run_fallow_raw_with_env(
        &[
            "--root",
            root.to_str().expect("fixture path is UTF-8"),
            "dead-code",
            "--format",
            "json",
            "--no-cache",
        ],
        &[("RUST_LOG", "warn")],
    );

    assert!(
        output.stderr.contains("node_modules"),
        "the degradation warning proves warnings reach stderr here: {}",
        output.stderr
    );
    assert!(
        !output
            .stderr
            .contains("No architecture boundaries are configured"),
        "an unconfigured boundary check must not warn: {}",
        output.stderr
    );
    assert!(
        !output.stderr.contains("No rule packs are configured"),
        "an unconfigured rule-pack check must not warn: {}",
        output.stderr
    );

    let json = parse_json(&output);
    let kinds: Vec<&str> = json["workspace_diagnostics"]
        .as_array()
        .expect("the envelope carries the array")
        .iter()
        .filter_map(|diagnostic| diagnostic["kind"].as_str())
        .collect();
    assert!(
        kinds.contains(&"boundaries-not-configured")
            && kinds.contains(&"rule-packs-not-configured"),
        "both stay in the structured array: {kinds:?}"
    );
}

/// Issue #2366: the combined root's union must be the same ARRAY on every run
/// of the same command, not just the same set.
///
/// Under this split the dead-code and duplication walks run under
/// `rayon::join` on one root, and each walk replaces the registry's
/// source-discovery set. While the dead-code analysis folded a live registry
/// read into its own list, whether the duplication walk had already written
/// decided whether its skip arrived inside the dead-code section's list or
/// later from the duplication section's, so the same command emitted two
/// different orders across repeat runs. Every analysis now carries its own
/// walk's skips by value and the live read drops walk-recorded entries, so the
/// order is fixed by section order alone.
#[test]
fn combined_json_root_workspace_diagnostics_are_byte_identical_across_repeat_runs() {
    let dir = split_production_two_large_files_project(
        r#"{"production":{"deadCode":true,"health":false,"dupes":false}}"#,
    );
    let root = dir.path().to_str().expect("temp path is UTF-8");
    let mut observed: Vec<serde_json::Value> = Vec::new();
    for _ in 0..6 {
        let json = parse_json(&run_fallow_raw(&[
            "--root",
            root,
            "--max-file-size",
            "1",
            "--format",
            "json",
            "--quiet",
            "--no-cache",
        ]));
        observed.push(json["workspace_diagnostics"].clone());
    }

    let entries: Vec<(String, String)> = observed[0]
        .as_array()
        .expect("the root carries the array")
        .iter()
        .map(|diagnostic| {
            (
                diagnostic["kind"].as_str().unwrap_or_default().to_owned(),
                diagnostic["path"].as_str().unwrap_or_default().to_owned(),
            )
        })
        .collect();
    assert_eq!(
        entries,
        [
            (
                "skipped-large-file".to_owned(),
                "src/huge.prod.ts".to_owned()
            ),
            ("node-modules-missing".to_owned(), "node_modules".to_owned()),
            (
                "bun-lockb-override-resolution-skipped".to_owned(),
                "package.json".to_owned()
            ),
            (
                "malformed-pnpm-workspace-yaml".to_owned(),
                "pnpm-workspace.yaml".to_owned()
            ),
            ("boundaries-not-configured".to_owned(), ".".to_owned()),
            ("rule-packs-not-configured".to_owned(), ".".to_owned()),
            (
                "skipped-large-file".to_owned(),
                "src/huge.test.ts".to_owned()
            ),
            // The health section's own degraded input, merged into its result
            // at finalize and unioned in last because health is the last
            // section (issue #2689). The fixture is a bare temporary directory,
            // so there is no repository for the churn analysis to read.
            ("hotspots-skipped".to_owned(), ".".to_owned()),
        ],
        "the union runs in section order: the dead-code analysis's own snapshot \
         (its production walk's skips and the config-load stash), then the \
         registry entries recorded after the session was built, sorted by path \
         and kind, then the skip only the full-file-set walks saw, then the \
         health section's own degraded inputs"
    );
    for (index, run) in observed.iter().enumerate() {
        assert_eq!(
            run, &observed[0],
            "run {index} disagrees with the first run about the combined root's \
             workspace_diagnostics[]"
        );
    }
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
/// built from different inputs (the CLI folds each analysis's captured list
/// plus a final registry read, the programmatic route folds the typed
/// sections' own lists), so pin that a non-production dead-code pass under a
/// production health/dupes split reaches the standalone envelope and the
/// combined root alike. Without the union the combined root is empty here
/// while `dead-code --format json` on the same project reports the entry.
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
    assert_combined_root_covers_standalone(
        &standalone,
        &combined,
        "the combined root carries the standalone dead-code list",
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
/// it, from the duplication section's own captured list.
///
/// This split is also the case that runs the dead-code and duplication walks
/// under `rayon::join`, so a registry read would answer "whichever walk wrote
/// last" and the assertion below would hold only on some runs.
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

/// Issues #2366 and #2396: every standalone analysis envelope carries the
/// workspace-discovery list captured by its own run, matching both the
/// workspace listing and the combined root.
fn undeclared_workspace_fixture() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::create_dir_all(dir.path().join("packages/inner/src")).expect("create inner package");
    std::fs::create_dir_all(dir.path().join("src")).expect("create src");
    std::fs::write(
        dir.path().join("package.json"),
        r#"{"name":"issue-2366-undeclared","private":true,"main":"src/index.ts","workspaces":["packages/declared"]}"#,
    )
    .expect("write package.json");
    std::fs::write(
        dir.path().join("packages/inner/package.json"),
        r#"{"name":"inner-pkg","version":"1.0.0"}"#,
    )
    .expect("write inner package.json");
    std::fs::write(dir.path().join("src/index.ts"), "export const value = 1;\n")
        .expect("write source");
    std::fs::write(
        dir.path().join("packages/inner/src/index.ts"),
        "export const inner = 2;\n",
    )
    .expect("write inner source");
    dir
}

fn assert_undeclared_workspace_diagnostic(output: &serde_json::Value, context: &str) {
    let diagnostics = output["workspace_diagnostics"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|diagnostic| diagnostic["kind"] == "undeclared-workspace")
        .collect::<Vec<_>>();
    assert_eq!(
        diagnostics.len(),
        1,
        "{context}: {}",
        output["workspace_diagnostics"]
    );
    assert_eq!(diagnostics[0]["path"], "packages/inner");
}

#[test]
fn analysis_envelopes_agree_with_the_workspace_listing_on_undeclared_workspaces() {
    let dir = undeclared_workspace_fixture();
    let root = dir.path().to_str().expect("temp path is UTF-8");

    let listing = parse_json(&run_fallow_raw(&[
        "list",
        "--workspaces",
        "--root",
        root,
        "--format",
        "json",
        "--quiet",
    ]));
    assert_undeclared_workspace_diagnostic(
        &listing,
        "the workspace listing reports the undeclared package",
    );

    let combined = parse_json(&run_fallow_raw(&[
        "--root",
        root,
        "--format",
        "json",
        "--quiet",
        "--no-cache",
    ]));
    assert_undeclared_workspace_diagnostic(
        &combined,
        "the combined root reports what the workspace listing reports",
    );

    for command in ["dead-code", "check", "health", "dupes"] {
        let output = parse_json(&run_fallow_raw(&[
            command,
            "--root",
            root,
            "--format",
            "json",
            "--quiet",
            "--no-cache",
        ]));
        assert_undeclared_workspace_diagnostic(
            &output,
            &format!("{command} reports the run-owned undeclared workspace"),
        );
    }

    for command in [
        vec![
            "security",
            "--root",
            root,
            "--format",
            "json",
            "--quiet",
            "--no-cache",
        ],
        vec![
            "security",
            "--root",
            root,
            "--summary",
            "--format",
            "json",
            "--quiet",
            "--no-cache",
        ],
        vec![
            "security",
            "--root",
            root,
            "--no-cache",
            "blind-spots",
            "--format",
            "json",
            "--quiet",
        ],
    ] {
        let output = parse_json(&run_fallow_raw(&command));
        assert_undeclared_workspace_diagnostic(
            &output,
            "security-family output reports its run-owned diagnostic",
        );
    }
}

/// Issue #2366: two overlapping workspace globs (`["pkgs/*", "pkgs/a*"]`, the
/// shape a monorepo gets from `["packages/*", "packages/*/*"]`) report the same
/// package-less directory twice, once per pattern. The union that builds the
/// combined root must keep both, otherwise the root is NARROWER than the
/// standalone `dead-code` envelope it unions.
#[test]
fn combined_json_root_keeps_both_overlapping_glob_diagnostics() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::create_dir_all(dir.path().join("pkgs/aaa")).expect("create package-less directory");
    std::fs::create_dir_all(dir.path().join("src")).expect("create src");
    std::fs::write(
        dir.path().join("package.json"),
        r#"{"name":"issue-2366-overlapping-globs","private":true,"main":"src/index.ts","workspaces":["pkgs/*","pkgs/a*"]}"#,
    )
    .expect("write package.json");
    std::fs::write(dir.path().join("src/index.ts"), "export const value = 1;\n")
        .expect("write source");
    std::fs::write(
        dir.path().join("pkgs/aaa/readme.txt"),
        "no package.json here\n",
    )
    .expect("write filler");
    let root = dir.path().to_str().expect("temp path is UTF-8");

    let standalone = parse_json(&run_fallow_raw(&[
        "dead-code",
        "--root",
        root,
        "--format",
        "json",
        "--quiet",
        "--no-cache",
    ]));
    let combined = parse_json(&run_fallow_raw(&[
        "--root",
        root,
        "--format",
        "json",
        "--quiet",
        "--no-cache",
    ]));

    let patterns: Vec<String> =
        combined_root_diagnostics_of_kind(&combined, "glob-matched-no-package-json")
            .iter()
            .map(|diagnostic| {
                diagnostic["pattern"]
                    .as_str()
                    .unwrap_or_default()
                    .to_owned()
            })
            .collect();
    assert_eq!(
        patterns,
        ["pkgs/*", "pkgs/a*"],
        "both globs matched the same directory and both are reported: {}",
        combined["workspace_diagnostics"]
    );
    assert_combined_root_covers_standalone(
        &standalone,
        &combined,
        "the combined root is never narrower than the standalone dead-code envelope",
    );
}

/// Rewrite the helper module so it exports `used` plus `unused_count` exports
/// that nothing consumes.
fn write_unused_exports(root: &std::path::Path, unused_count: u64) {
    let source: String = std::iter::once("export const used = () => 1;\n".to_owned())
        .chain((0..unused_count).map(|index| format!("export const unused{index} = () => 1;\n")))
        .collect();
    std::fs::write(root.join("src/helpers.ts"), source).expect("write helper module");
}

/// The fixture from issue #2627: a project whose dead-code baseline is saved
/// while `saved` exports are unused, and whose sources then keep only
/// `remaining` of them, so `saved - remaining` baseline entries match nothing
/// on the next run.
fn rotted_baseline_project(saved: u64, remaining: u64) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("temporary project");
    let root = dir.path();
    std::fs::create_dir_all(root.join("src")).expect("create source directory");
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"baseline-staleness-repro","private":true,"main":"src/index.ts"}"#,
    )
    .expect("write package");
    std::fs::write(
        root.join("src/index.ts"),
        "import { used } from \"./helpers\";\nexport const main = () => used();\n",
    )
    .expect("write entry point");
    write_unused_exports(root, saved);
    let save = run_fallow_raw(&[
        "dead-code",
        "--root",
        root.to_str().expect("temp path is UTF-8"),
        "--no-cache",
        "--quiet",
        "--format",
        "json",
        "--save-baseline",
        root.join("baseline.json")
            .to_str()
            .expect("temp path is UTF-8"),
    ]);
    let saved_entries = parse_json(&save)["total_issues"].as_u64().unwrap_or(0);
    assert_eq!(
        saved_entries, saved,
        "the saved baseline must hold exactly {saved} entries: {}",
        save.stdout
    );
    write_unused_exports(root, remaining);
    dir
}

/// Run `dead-code --baseline` against a prepared fixture with extra arguments.
fn run_with_baseline(root: &std::path::Path, extra: &[&str]) -> crate::common::CommandOutput {
    let baseline = root.join("baseline.json");
    let mut args = vec![
        "dead-code",
        "--root",
        root.to_str().expect("temp path is UTF-8"),
        "--no-cache",
        "--baseline",
        baseline.to_str().expect("temp path is UTF-8"),
    ];
    args.extend_from_slice(extra);
    run_fallow_raw(&args)
}

#[test]
fn partially_stale_dead_code_baseline_warns_on_human_output() {
    let project = rotted_baseline_project(4, 2);
    let output = run_with_baseline(project.path(), &[]);
    assert!(
        output.stderr.contains(
            "Warning: baseline is partially stale: 2 of 4 entries matched no current issue"
        ),
        "a half-rotten baseline must say so on stderr: {}",
        output.stderr
    );
    assert!(
        output.stderr.contains("--save-baseline"),
        "the warning must point at the re-save command: {}",
        output.stderr
    );
    assert_eq!(
        output.code, 0,
        "the warning must not change the exit code: {}",
        output.stderr
    );
}

#[test]
fn partially_stale_baseline_is_silent_below_threshold() {
    let project = rotted_baseline_project(5, 4);
    let output = run_with_baseline(project.path(), &[]);
    assert!(
        output.stderr.contains("Comparing against baseline"),
        "the baseline still loads: {}",
        output.stderr
    );
    assert!(
        !output.stderr.contains("partially stale"),
        "one stale entry out of five is below the warning threshold: {}",
        output.stderr
    );
}

/// A project that cleaned up everything the baseline described has nothing to
/// compare, so staleness cannot be judged and the advisory warning stays
/// silent, exactly as `health --baseline` already behaved.
#[test]
fn cleaned_project_does_not_warn_about_a_fully_stale_baseline() {
    let project = rotted_baseline_project(4, 0);
    let output = run_with_baseline(project.path(), &[]);
    assert!(
        output.stderr.contains("Comparing against baseline"),
        "the baseline still loads: {}",
        output.stderr
    );
    assert!(
        !output.stderr.contains("matched 0 current issues"),
        "a run with no findings cannot tell rot from success: {}",
        output.stderr
    );
    assert!(
        !output.stderr.contains("partially stale"),
        "neither staleness branch fires without findings to compare: {}",
        output.stderr
    );
    assert_eq!(
        output.code, 0,
        "a clean project stays green: {}",
        output.stderr
    );
}

/// The guard against over-suppressing: a run that produced findings and
/// matched none of the baseline still gets the zero-overlap advice, because
/// there the mismatch really is rot.
#[test]
fn zero_overlap_still_warns_when_the_run_has_findings() {
    let project = rotted_baseline_project(4, 4);
    let root = project.path();
    std::fs::rename(root.join("src/helpers.ts"), root.join("src/renamed.ts"))
        .expect("move the helper module");
    std::fs::write(
        root.join("src/index.ts"),
        "import { used } from \"./renamed\";\nexport const main = () => used();\n",
    )
    .expect("repoint the import");
    let output = run_with_baseline(root, &[]);
    assert!(
        output
            .stderr
            .contains("Warning: baseline has 4 entries but matched 0 current issues"),
        "the zero-overlap wording is unchanged: {}",
        output.stderr
    );
    assert!(
        !output.stderr.contains("partially stale"),
        "the two branches are mutually exclusive: {}",
        output.stderr
    );
    assert_eq!(
        output.code, 1,
        "the four unmatched findings fail the run, the warning does not: {}",
        output.stderr
    );
}

#[test]
fn fail_on_stale_baseline_exits_one_on_a_partially_stale_baseline() {
    let project = rotted_baseline_project(4, 2);
    let output = run_with_baseline(project.path(), &["--fail-on-stale-baseline"]);
    assert!(
        output
            .stderr
            .contains("Baseline gate failed: 2 of 4 entries"),
        "the gate names the stale share: {}",
        output.stderr
    );
    assert_eq!(
        output.code, 1,
        "the opt-in gate fails the run: {}",
        output.stderr
    );
}

/// The reason the flag exists: a handful of stale entries the advisory
/// threshold deliberately ignores still fails an opted-in build.
#[test]
fn fail_on_stale_baseline_exits_one_below_the_warning_threshold() {
    let project = rotted_baseline_project(5, 4);
    let output = run_with_baseline(project.path(), &["--fail-on-stale-baseline"]);
    assert!(
        output
            .stderr
            .contains("Baseline gate failed: 1 of 5 entries"),
        "one stale entry is enough for the opt-in gate: {}",
        output.stderr
    );
    assert!(
        !output.stderr.contains("partially stale"),
        "the advisory warning keeps its quarter threshold: {}",
        output.stderr
    );
    assert_eq!(output.code, 1, "the gate fails the run: {}", output.stderr);
}

#[test]
fn fail_on_stale_baseline_exits_one_on_a_cleaned_project() {
    let project = rotted_baseline_project(4, 0);
    let output = run_with_baseline(project.path(), &["--fail-on-stale-baseline"]);
    assert!(
        output
            .stderr
            .contains("Baseline gate failed: 4 of 4 entries"),
        "a dead baseline file is exactly the hygiene case the gate exists for: {}",
        output.stderr
    );
    assert!(
        !output.stderr.contains("matched 0 current issues"),
        "the advisory warning stays silent on a cleaned project: {}",
        output.stderr
    );
    assert_eq!(output.code, 1, "the gate fails the run: {}", output.stderr);
}

#[test]
fn fail_on_stale_baseline_is_green_on_a_fresh_baseline() {
    let project = rotted_baseline_project(4, 4);
    let output = run_with_baseline(project.path(), &["--fail-on-stale-baseline"]);
    assert!(
        !output.stderr.contains("Baseline gate failed"),
        "every entry still matches: {}",
        output.stderr
    );
    assert_eq!(
        output.code, 0,
        "a fresh baseline is green: {}",
        output.stderr
    );
}

/// A narrowed run cannot judge a whole-project baseline, but it must say so:
/// `--file`, `--changed-since` and `--production` are ordinary CI shapes, and
/// a job that opted into a build-failing gate would otherwise stay green
/// forever without ever judging the baseline.
#[test]
fn fail_on_stale_baseline_says_why_it_stood_down_on_a_scoped_run() {
    let project = rotted_baseline_project(4, 2);
    let output = run_with_baseline(
        project.path(),
        &["--file", "src/index.ts", "--fail-on-stale-baseline"],
    );
    assert!(
        !output.stderr.contains("Baseline gate failed"),
        "a narrowed run cannot judge a whole-project baseline: {}",
        output.stderr
    );
    assert!(
        output
            .stderr
            .contains("--fail-on-stale-baseline did not run"),
        "the run names the reason the gate stood down: {}",
        output.stderr
    );
    assert_eq!(
        output.code, 0,
        "the gate must stay usable in changed-file CI jobs: {}",
        output.stderr
    );
}

/// Production mode narrows through the config rather than a scope flag, and
/// reaches the same guard and the same note.
#[test]
fn fail_on_stale_baseline_says_why_it_stood_down_in_production_mode() {
    let project = rotted_baseline_project(4, 2);
    let output = run_with_baseline(
        project.path(),
        &["--production", "--quiet", "--fail-on-stale-baseline"],
    );
    assert!(
        output
            .stderr
            .contains("--fail-on-stale-baseline did not run"),
        "the note survives --quiet, like the gate line: {}",
        output.stderr
    );
    assert!(
        !output.stderr.contains("Baseline gate failed"),
        "production mode drops files, so the comparison is partial: {}",
        output.stderr
    );
    assert_eq!(
        output.code, 0,
        "a stood-down gate does not fail the run: {}",
        output.stderr
    );
}

#[test]
fn fail_on_stale_baseline_is_inert_without_a_baseline() {
    let project = rotted_baseline_project(4, 2);
    let output = run_fallow_raw(&[
        "dead-code",
        "--root",
        project.path().to_str().expect("temp path is UTF-8"),
        "--no-cache",
        "--fail-on-stale-baseline",
    ]);
    assert!(
        !output.stderr.contains("Baseline gate failed"),
        "there is no baseline to judge: {}",
        output.stderr
    );
    assert!(
        !output.stderr.contains("--fail-on-stale-baseline"),
        "with nothing loaded the flag has nothing to explain: {}",
        output.stderr
    );
    assert_ne!(
        output.code, 2,
        "the flag is accepted without a baseline: {}",
        output.stderr
    );
}

/// Unlike the score and findings gates, a stale baseline appears nowhere else
/// in the output, so a bare exit 1 under `--quiet` or `--ci` would be
/// unexplained.
#[test]
fn fail_on_stale_baseline_explains_itself_under_quiet() {
    let project = rotted_baseline_project(4, 2);
    let output = run_with_baseline(project.path(), &["--quiet", "--fail-on-stale-baseline"]);
    assert!(
        output
            .stderr
            .contains("Baseline gate failed: 2 of 4 entries"),
        "the gate line survives --quiet: {}",
        output.stderr
    );
    assert!(
        !output.stderr.contains("partially stale"),
        "--quiet still suppresses the advisory warning: {}",
        output.stderr
    );
    assert_eq!(output.code, 1, "the gate fails the run: {}", output.stderr);
}

#[test]
fn fail_on_stale_baseline_leaves_json_output_unchanged() {
    let project = rotted_baseline_project(4, 2);
    let without = run_with_baseline(project.path(), &["--format", "json", "--quiet"]);
    let with = run_with_baseline(
        project.path(),
        &["--format", "json", "--quiet", "--fail-on-stale-baseline"],
    );
    assert_eq!(
        canonical_report_without_gate_outcomes(&without),
        canonical_report_without_gate_outcomes(&with),
        "the gate moves nothing in the report but its own armed-ness"
    );
    // 3.26.0 promised the flag changed nothing but the exit code and one stderr
    // line, because no envelope carried a gate verdict then. `gate_outcomes`
    // carries one now, and whether a verdict is armed is part of it, so exactly
    // one member moves and `baseline_staleness` itself still does not.
    assert_eq!(
        parse_json(&without)["baseline_staleness"],
        parse_json(&with)["baseline_staleness"],
        "the staleness object stays flag-independent"
    );
    assert_eq!(
        parse_json(&without)["gate_outcomes"]["stale-baseline"]["enforced"],
        serde_json::json!(false),
        "the verdict is published unarmed without the flag"
    );
    assert_eq!(
        parse_json(&with)["gate_outcomes"]["stale-baseline"]["enforced"],
        serde_json::json!(true),
        "the flag arms it"
    );
    assert_eq!(without.code, 0, "the run is green without the flag");
    assert_eq!(with.code, 1, "the run fails with the flag");
}

/// Exit precedence puts the regression gate ahead of the baseline gate, but a
/// stale baseline appears in no report, so the line has to print anyway. A
/// run that returns on the first failing gate would exit 1 without a word
/// about the baseline the user explicitly gated on.
#[test]
fn fail_on_stale_baseline_still_prints_behind_the_regression_gate() {
    let project = rotted_baseline_project(4, 2);
    let regression = project.path().join("regression.json");
    let regression_path = regression.to_str().expect("temp path is UTF-8");
    // Saved while the baseline still filters everything, so the module added
    // below is the only regression.
    run_with_baseline(
        project.path(),
        &["--quiet", "--save-regression-baseline", regression_path],
    );
    std::fs::write(
        project.path().join("src/extra.ts"),
        "export const brandNew = () => 1;\n",
    )
    .expect("write an unreferenced module");

    let output = run_with_baseline(
        project.path(),
        &[
            "--regression-baseline",
            regression_path,
            "--fail-on-regression",
            "--fail-on-stale-baseline",
        ],
    );
    assert!(
        output.stderr.contains("Regression detected"),
        "the regression gate fires first: {}",
        output.stderr
    );
    assert!(
        output
            .stderr
            .contains("Baseline gate failed: 2 of 4 entries"),
        "the baseline gate still says what it found: {}",
        output.stderr
    );
    assert_eq!(output.code, 1, "the run fails: {}", output.stderr);
}

/// Run the bare combined command against a prepared fixture. The bare run is
/// the shape a repository gets from `fallow` with no subcommand, and its
/// machine renderers collapse every gate to exit 0, so the opt-in gate needs
/// its own coverage there.
fn run_bare_with_baseline(root: &std::path::Path, extra: &[&str]) -> crate::common::CommandOutput {
    let baseline = root.join("baseline.json");
    let mut args = vec![
        "--root",
        root.to_str().expect("temp path is UTF-8"),
        "--no-cache",
        "--baseline",
        baseline.to_str().expect("temp path is UTF-8"),
    ];
    args.extend_from_slice(extra);
    run_fallow_raw(&args)
}

/// CI reads `--format json`, so a gate that only fires on the human renderer
/// is a gate that never fires in the job that asked for it.
#[test]
fn fail_on_stale_baseline_gates_the_bare_run_in_json() {
    let project = rotted_baseline_project(4, 2);
    let without = run_bare_with_baseline(project.path(), &["--format", "json", "--quiet"]);
    let with = run_bare_with_baseline(
        project.path(),
        &["--format", "json", "--quiet", "--fail-on-stale-baseline"],
    );
    assert!(
        with.stderr.contains("Baseline gate failed: 2 of 4 entries"),
        "the bare run names the stale share on stderr: {}",
        with.stderr
    );
    assert_eq!(
        without.code, 0,
        "the bare JSON run is green without the flag: {}",
        without.stderr
    );
    assert_eq!(
        with.code, 1,
        "the opt-in gate fails the bare JSON run: {}",
        with.stderr
    );
    assert_eq!(
        canonical_report_without_gate_outcomes(&without),
        canonical_report_without_gate_outcomes(&with),
        "the gate moves nothing in the combined envelope but its own armed-ness"
    );
    assert_eq!(
        parse_json(&without)["check"]["baseline_staleness"],
        parse_json(&with)["check"]["baseline_staleness"],
        "the staleness object stays flag-independent"
    );
}

/// The same contract on the other machine renderers the bare run offers.
#[test]
fn fail_on_stale_baseline_gates_the_bare_run_in_every_machine_format() {
    let project = rotted_baseline_project(4, 2);
    for format in ["sarif", "codeclimate", "github-annotations"] {
        let output = run_bare_with_baseline(
            project.path(),
            &["--format", format, "--quiet", "--fail-on-stale-baseline"],
        );
        assert!(
            output
                .stderr
                .contains("Baseline gate failed: 2 of 4 entries"),
            "--format {format} must carry the gate line on stderr: {}",
            output.stderr
        );
        assert_eq!(
            output.code, 1,
            "--format {format} must carry the gate exit code: {}",
            output.stderr
        );
    }
}

#[test]
fn scoped_run_does_not_warn_about_baseline_staleness() {
    let project = rotted_baseline_project(4, 2);
    let output = run_with_baseline(project.path(), &["--file", "src/index.ts"]);
    assert!(
        output.stderr.contains("Comparing against baseline"),
        "the baseline still loads under a file scope: {}",
        output.stderr
    );
    assert!(
        !output.stderr.contains("partially stale"),
        "a narrowed run cannot judge a whole-project baseline: {}",
        output.stderr
    );
    assert!(
        !output.stderr.contains("matched 0 current issues"),
        "re-saving from a narrowed run would gut the baseline, so do not advise it: {}",
        output.stderr
    );
}

#[test]
fn production_run_does_not_warn_about_baseline_staleness() {
    let project = rotted_baseline_project(4, 2);
    let output = run_with_baseline(project.path(), &["--production"]);
    assert!(
        output.stderr.contains("Comparing against baseline"),
        "the baseline still loads in production mode: {}",
        output.stderr
    );
    assert!(
        !output.stderr.contains("partially stale"),
        "production mode drops test and dev files, so it cannot judge a whole-project baseline: {}",
        output.stderr
    );
    assert!(
        !output.stderr.contains("matched 0 current issues"),
        "re-saving from a production run would drop every non-production entry: {}",
        output.stderr
    );
}

#[test]
fn diff_scoped_run_does_not_warn_about_baseline_staleness() {
    let project = rotted_baseline_project(4, 2);
    let diff = project.path().join("scope.patch");
    std::fs::write(
        &diff,
        "diff --git a/src/index.ts b/src/index.ts\n\
         index 1111111..2222222 100644\n\
         --- a/src/index.ts\n\
         +++ b/src/index.ts\n\
         @@ -1,2 +1,2 @@\n\
          import { used } from \"./helpers\";\n\
         -export const main = () => used();\n\
         +export const main = () => used() + 0;\n",
    )
    .expect("write diff");
    let output = run_with_baseline(
        project.path(),
        &["--diff-file", diff.to_str().expect("temp path is UTF-8")],
    );
    assert!(
        !output.stderr.contains("partially stale"),
        "a diff-scoped run sees only the changed slice of the baseline: {}",
        output.stderr
    );
    assert!(
        !output.stderr.contains("matched 0 current issues"),
        "a diff-scoped run must not advise a re-save that would gut the baseline: {}",
        output.stderr
    );
}

#[test]
fn filtered_run_does_not_warn_about_baseline_staleness() {
    let project = rotted_baseline_project(4, 2);
    let output = run_with_baseline(project.path(), &["--circular-deps"]);
    assert!(
        !output.stderr.contains("partially stale"),
        "a run restricted to one issue type drops whole baseline categories: {}",
        output.stderr
    );
    assert!(
        !output.stderr.contains("matched 0 current issues"),
        "a filtered run must not advise a re-save either: {}",
        output.stderr
    );
}

#[test]
fn quiet_suppresses_the_partial_staleness_warning() {
    let project = rotted_baseline_project(4, 2);
    let output = run_with_baseline(project.path(), &["--quiet"]);
    assert!(
        !output.stderr.contains("Comparing against baseline"),
        "--quiet suppresses the baseline notice: {}",
        output.stderr
    );
    assert!(
        !output.stderr.contains("partially stale"),
        "--quiet suppresses the staleness warning too: {}",
        output.stderr
    );
}

#[test]
fn partially_stale_baseline_leaves_json_output_unchanged() {
    let project = rotted_baseline_project(4, 2);
    let output = run_with_baseline(project.path(), &["--format", "json", "--quiet"]);
    let json = parse_json(&output);
    assert_eq!(
        json["baseline"],
        serde_json::json!({ "entries": 4, "matched": 2 }),
        "the envelope keeps exactly the two counts it always carried: {}",
        json["baseline"]
    );
}

/// Write a project whose only source outside `src/` sits under a directory a
/// built-in discovery ignore pattern matches, with no `.gitignore` to hide it
/// first (issue #2638).
fn default_ignore_exclusion_project() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        dir.path().join("package.json"),
        r#"{"name":"issue-2638-default-ignore-exclusions","private":true,"main":"src/index.ts"}"#,
    )
    .expect("write package.json");
    std::fs::create_dir_all(dir.path().join("src")).expect("create src");
    std::fs::write(dir.path().join("src/index.ts"), "export const value = 1;\n")
        .expect("write source");
    std::fs::create_dir_all(dir.path().join("packages/web/build/src"))
        .expect("create excluded tree");
    for name in ["a.ts", "b.ts"] {
        std::fs::write(
            dir.path().join("packages/web/build/src").join(name),
            "export const excluded = 1;\n",
        )
        .expect("write excluded source");
    }
    dir
}

/// Issue #2638 (R3): `--explain-skipped` now reaches discovery, so the run says
/// which built-in pattern removed files and where they were.
#[test]
fn explain_skipped_names_the_built_in_pattern_that_excluded_source_files() {
    let dir = default_ignore_exclusion_project();
    let root = dir.path().to_str().expect("temp path is UTF-8");
    let output = run_fallow_raw(&[
        "dead-code",
        "--root",
        root,
        "--no-cache",
        "--explain-skipped",
    ]);

    assert!(
        output
            .stderr
            .contains("note: skipped 2 source files matching fallow's built-in discovery ignores:"),
        "the note header carries the total: {}",
        output.stderr
    );
    assert!(
        output
            .stderr
            .contains("    2  **/build/**  packages/web/build"),
        "the row carries the count, the pattern, and the directory: {}",
        output.stderr
    );
    assert!(
        output
            .stderr
            .contains("analyze that directory on its own: fallow --root <dir>"),
        "a directory-shaped pattern gets the remedy that works: {}",
        output.stderr
    );
}

/// Issue #2638: the `--root` remedy is true for a directory-shaped built-in
/// only. A file-name glob matches at every root, so the note must not hand the
/// reader a command that re-excludes the same file.
#[test]
fn a_file_shaped_built_in_pattern_is_not_offered_the_root_remedy() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        dir.path().join("package.json"),
        r#"{"name":"issue-2638-file-shaped","private":true,"main":"src/index.ts"}"#,
    )
    .expect("write package.json");
    std::fs::create_dir_all(dir.path().join("src")).expect("create src");
    std::fs::write(dir.path().join("src/index.ts"), "export const value = 1;\n")
        .expect("write source");
    std::fs::create_dir_all(dir.path().join("vendor")).expect("create vendor");
    std::fs::write(dir.path().join("vendor/lib.min.js"), "var a=1;\n").expect("write bundle");
    let root = dir.path().to_str().expect("temp path is UTF-8");

    let output = run_fallow_raw(&[
        "dead-code",
        "--root",
        root,
        "--no-cache",
        "--explain-skipped",
    ]);
    assert!(
        output.stderr.contains("**/*.min.js"),
        "the note still names the pattern: {}",
        output.stderr
    );
    assert!(
        !output.stderr.contains("fallow --root <dir>"),
        "re-rooting re-excludes the same file: {}",
        output.stderr
    );
    assert!(
        output.stderr.contains("rename first-party source"),
        "the note names the remedy that does work: {}",
        output.stderr
    );
}

/// Issue #2638, the headline case: the whole source tree sat under a matched
/// directory, so the run analyzed nothing. That run says why on stderr without
/// any flag, because a green "No issues found" is the misleading answer.
#[test]
fn a_run_whose_whole_source_tree_was_excluded_says_so_by_default() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        dir.path().join("package.json"),
        r#"{"name":"issue-2638-all-excluded","private":true}"#,
    )
    .expect("write package.json");
    std::fs::create_dir_all(dir.path().join("build/src")).expect("create excluded tree");
    for name in ["a.ts", "b.ts", "c.ts"] {
        std::fs::write(
            dir.path().join("build/src").join(name),
            "export const excluded = 1;\n",
        )
        .expect("write excluded source");
    }
    let root = dir.path().to_str().expect("temp path is UTF-8");

    let output = run_fallow_raw(&["dead-code", "--root", root, "--no-cache"]);
    assert!(
        output.stderr.contains(
            "No source files were analyzed. The built-in ignore pattern '**/build/**' excluded \
             3 files; run with --explain-skipped for the breakdown."
        ),
        "the default run states the outcome and the measured exclusion: {}",
        output.stderr
    );
}

/// The unflagged line fires whenever a run discovered nothing, and a built-in
/// exclusion is not always why. `--production` drops test-only source AFTER the
/// ignore check, so the tally never sees it: here the project is all tests and
/// the run would analyze nothing with or without `dist/`. The sentence must
/// still be true, which means it reports two facts and blames neither.
#[test]
fn a_production_run_with_no_source_left_states_facts_without_naming_a_cause() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        dir.path().join("package.json"),
        r#"{"name":"issue-2638-production","private":true}"#,
    )
    .expect("write package.json");
    std::fs::create_dir_all(dir.path().join("src")).expect("create src");
    std::fs::write(
        dir.path().join("src/app.test.ts"),
        "export const spec = 1;\n",
    )
    .expect("write test source");
    std::fs::create_dir_all(dir.path().join("dist")).expect("create dist");
    std::fs::write(dir.path().join("dist/gen.ts"), "export const gen = 1;\n")
        .expect("write generated source");
    let root = dir.path().to_str().expect("temp path is UTF-8");

    let output = run_fallow_raw(&["dead-code", "--root", root, "--no-cache", "--production"]);
    assert!(
        output.stderr.contains(
            "No source files were analyzed. The built-in ignore pattern '**/dist/**' excluded \
             1 file; run with --explain-skipped for the breakdown."
        ),
        "two measured facts, joined by a period: {}",
        output.stderr
    );
    assert!(
        !output.stderr.contains("were analyzed:"),
        "production mode emptied the file list here, so a causal colon would name the wrong \
         reason: {}",
        output.stderr
    );
}

/// Issue #2638: the anchor the note prints is the directory the `--root`
/// remedy names, so running that command has to recover the files. A nested
/// match is the case that proves it: anchoring at the outer `build` leaves the
/// inner one in the relative path and the built-in matches again.
#[test]
fn the_anchor_a_nested_match_reports_is_the_directory_that_recovers_the_files() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        dir.path().join("package.json"),
        r#"{"name":"issue-2638-nested","private":true,"main":"src/index.ts"}"#,
    )
    .expect("write package.json");
    std::fs::create_dir_all(dir.path().join("src")).expect("create src");
    std::fs::write(dir.path().join("src/index.ts"), "export const value = 1;\n")
        .expect("write source");
    std::fs::create_dir_all(dir.path().join("build/tools/build")).expect("create nested tree");
    std::fs::write(
        dir.path().join("build/tools/build/gen.ts"),
        "export const gen = 1;\n",
    )
    .expect("write excluded source");
    let root = dir.path().to_str().expect("temp path is UTF-8");

    let json = parse_json(&run_fallow_raw(&[
        "dead-code",
        "--root",
        root,
        "--format",
        "json",
        "--quiet",
        "--no-cache",
    ]));
    let reported = combined_root_diagnostics_of_kind(&json, "excluded-by-default-ignore");
    assert_eq!(reported.len(), 1, "{}", json["workspace_diagnostics"]);
    assert_eq!(
        reported[0]["path"], "build/tools/build",
        "the anchor is the deepest matched segment: {}",
        reported[0]
    );

    let anchor = dir.path().join("build/tools/build");
    let listed = parse_json(&run_fallow_raw(&[
        "list",
        "--files",
        "--root",
        anchor.to_str().expect("temp path is UTF-8"),
        "--format",
        "json",
        "--quiet",
        "--no-cache",
    ]));
    assert_eq!(
        listed["file_count"], 1,
        "the advertised remedy has to recover the files in one hop: {listed}"
    );
}

/// The guard that keeps the line above off every healthy project: a run that
/// discovered even one source file goes back to saying nothing by default.
#[test]
fn a_run_that_discovered_source_stays_silent_about_exclusions_by_default() {
    let dir = default_ignore_exclusion_project();
    let root = dir.path().to_str().expect("temp path is UTF-8");
    let output = run_fallow_raw(&["dead-code", "--root", root, "--no-cache"]);

    assert!(
        !output.stderr.contains("No source files were analyzed"),
        "one discovered file is enough to have something to report: {}",
        output.stderr
    );
}

/// Issue #2638 (AC4): the default run gains no noise. The note is a
/// presentation choice the flag owns, not a warning the walk emits.
#[test]
fn a_default_run_says_nothing_about_built_in_ignore_exclusions() {
    let dir = default_ignore_exclusion_project();
    let root = dir.path().to_str().expect("temp path is UTF-8");
    let output = run_fallow_raw(&["dead-code", "--root", root, "--no-cache"]);

    assert!(
        !output.stderr.contains("built-in ignore"),
        "no default stderr note: {}",
        output.stderr
    );
    assert!(
        !output.stderr.contains("**/build/**"),
        "no default stderr note: {}",
        output.stderr
    );
}

/// Issue #2638 (AC5): `--quiet` suppresses the note even with the flag.
#[test]
fn quiet_suppresses_the_built_in_ignore_exclusion_note() {
    let dir = default_ignore_exclusion_project();
    let root = dir.path().to_str().expect("temp path is UTF-8");
    let output = run_fallow_raw(&[
        "dead-code",
        "--root",
        root,
        "--no-cache",
        "--explain-skipped",
        "--quiet",
    ]);

    assert!(
        !output.stderr.contains("**/build/**"),
        "--quiet suppresses the note: {}",
        output.stderr
    );
}

/// Issue #2638 (R4, AC6): the typed entry is unconditional in JSON, with or
/// without the flag, and carries a project-relative forward-slash path.
#[test]
fn dead_code_json_carries_the_excluded_by_default_ignore_diagnostic() {
    let dir = default_ignore_exclusion_project();
    let root = dir.path().to_str().expect("temp path is UTF-8");
    for extra in [Vec::new(), vec!["--explain-skipped"]] {
        let mut args = vec![
            "dead-code",
            "--root",
            root,
            "--format",
            "json",
            "--quiet",
            "--no-cache",
        ];
        args.extend(extra.iter().copied());
        let json = parse_json(&run_fallow_raw(&args));

        let reported = combined_root_diagnostics_of_kind(&json, "excluded-by-default-ignore");
        assert_eq!(
            reported.len(),
            1,
            "one entry per excluding pattern: {}",
            json["workspace_diagnostics"]
        );
        assert_eq!(reported[0]["pattern"], "**/build/**");
        assert_eq!(reported[0]["file_count"], 2);
        assert_eq!(reported[0]["directory_count"], 1);
        assert_eq!(reported[0]["path"], "packages/web/build");
    }
}

/// Issue #2638: a built-in that matched a file sitting directly at the analysis
/// root anchors at the root, and `root.join("")` is the root itself. The
/// analysis envelopes' post-serialisation strip only removes a
/// `root + separator` prefix, so an absolute host path would reach JSON here.
#[test]
fn a_root_anchored_exclusion_reports_a_relative_path_and_a_real_location() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        dir.path().join("package.json"),
        r#"{"name":"issue-2638-root-anchored","private":true,"main":"src/index.ts"}"#,
    )
    .expect("write package.json");
    std::fs::create_dir_all(dir.path().join("src")).expect("create src");
    std::fs::write(dir.path().join("src/index.ts"), "export const value = 1;\n")
        .expect("write source");
    std::fs::write(dir.path().join("app.min.js"), "var a=1;\n").expect("write bundle");
    let root = dir.path().to_str().expect("temp path is UTF-8");

    let json = parse_json(&run_fallow_raw(&[
        "dead-code",
        "--root",
        root,
        "--format",
        "json",
        "--quiet",
        "--no-cache",
    ]));
    let reported = combined_root_diagnostics_of_kind(&json, "excluded-by-default-ignore");
    assert_eq!(reported.len(), 1, "{}", json["workspace_diagnostics"]);
    assert_eq!(
        reported[0]["path"], ".",
        "the contract is project-root-relative: {}",
        reported[0]
    );
    let message = reported[0]["message"].as_str().unwrap_or_default();
    assert!(
        message.starts_with("Skipped 1 source file under '.'"),
        "an empty string is not a location: {message}"
    );
    assert!(
        !message.contains(root),
        "no host path reaches the message either: {message}"
    );
}

/// Issue #2638 (AC6): combined mode's per-analysis config reloads must not wipe
/// the entry before the root envelope is built.
#[test]
fn combined_json_root_carries_the_excluded_by_default_ignore_diagnostic() {
    let dir = default_ignore_exclusion_project();
    let root = dir.path().to_str().expect("temp path is UTF-8");
    let json = parse_json(&run_fallow_raw(&[
        "--root",
        root,
        "--format",
        "json",
        "--quiet",
        "--no-cache",
    ]));

    let reported = combined_root_diagnostics_of_kind(&json, "excluded-by-default-ignore");
    assert_eq!(
        reported.len(),
        1,
        "the combined root reports the exclusion once: {}",
        json["workspace_diagnostics"]
    );
}

/// Issue #2638 (AC7): this is an advisory about project layout, not a finding.
/// Giving it a rule id would put it in a reviewer's annotations on every
/// monorepo, so the CI report formats must stay silent about it.
#[test]
fn ci_report_formats_say_nothing_about_built_in_ignore_exclusions() {
    let dir = default_ignore_exclusion_project();
    let root = dir.path().to_str().expect("temp path is UTF-8");
    for format in ["sarif", "codeclimate"] {
        let output = run_fallow_raw(&[
            "dead-code",
            "--root",
            root,
            "--format",
            format,
            "--quiet",
            "--no-cache",
        ]);
        assert!(
            !output.stdout.contains("excluded-by-default-ignore"),
            "{format} output must not carry the discovery advisory: {}",
            output.stdout
        );
    }
}

/// Issue #2638 (AC8): the exclusions are designed behavior, not a degraded run.
/// A caveat here would fire on nearly every project and make `fallow fix`
/// withhold `delete-file` and `remove-export` project-wide.
#[test]
fn a_built_in_ignore_exclusion_raises_no_reachability_caveat() {
    let dir = default_ignore_exclusion_project();
    let root = dir.path().to_str().expect("temp path is UTF-8");
    let json = parse_json(&run_fallow_raw(&[
        "dead-code",
        "--root",
        root,
        "--format",
        "json",
        "--quiet",
        "--no-cache",
    ]));

    assert!(
        !combined_root_diagnostics_of_kind(&json, "excluded-by-default-ignore").is_empty(),
        "fixture must actually trigger the diagnostic: {}",
        json["workspace_diagnostics"]
    );
    let caveated: Vec<&serde_json::Value> = json["unused_files"]
        .as_array()
        .map(|files| {
            files
                .iter()
                .filter(|file| {
                    file["reachability_caveats"]
                        .as_array()
                        .is_some_and(|caveats| !caveats.is_empty())
                })
                .collect()
        })
        .unwrap_or_default();
    assert!(
        caveated.is_empty(),
        "no finding may inherit a caveat from this advisory: {caveated:?}"
    );
}

// --- `baseline_staleness` on the dead-code envelope (issue #2673) ---------
//
// 3.26.0 published the staleness verdict on stderr only, which put it out of
// reach of every consumer that runs `--quiet --format json`. These pin the
// envelope half of the fix.

/// Read the dead-code envelope's `baseline_staleness` object.
fn envelope_staleness(output: &crate::common::CommandOutput) -> serde_json::Value {
    parse_json(output)["baseline_staleness"].clone()
}

#[test]
fn json_envelope_carries_baseline_staleness_on_a_whole_project_run() {
    let project = rotted_baseline_project(4, 2);
    let output = run_with_baseline(project.path(), &["--format", "json", "--quiet"]);
    let staleness = envelope_staleness(&output);
    assert_eq!(staleness["baseline_entries"], 4);
    assert_eq!(staleness["matched_entries"], 2);
    assert_eq!(staleness["stale_entries"], 2);
    assert_eq!(staleness["current_findings"], 2);
    assert_eq!(staleness["change_scoped"], false);
    assert_eq!(staleness["stale"], true);
    assert_eq!(staleness["warning"], "partial");
    assert_eq!(
        staleness["gate_trips"], true,
        "the gate's own rule, published so a CI integration reads one boolean: {}",
        output.stdout
    );
    assert_eq!(
        staleness["moved_entries"], 0,
        "dead-code matches entries by fingerprint and never follows a move: {}",
        output.stdout
    );
}

/// The reading that makes a jq derivation from `baseline.matched` alone wrong:
/// a narrowed run legitimately matches nothing while the baseline is healthy.
#[test]
fn scoped_run_publishes_change_scoped_so_zero_matches_cannot_be_misread() {
    let project = rotted_baseline_project(4, 2);
    let output = run_with_baseline(
        project.path(),
        &["--file", "src/index.ts", "--format", "json", "--quiet"],
    );
    let staleness = envelope_staleness(&output);
    assert_eq!(staleness["change_scoped"], true);
    assert_eq!(
        staleness["stale"], false,
        "a narrowed run cannot judge a whole-project baseline: {}",
        output.stdout
    );
    assert_eq!(
        staleness["gate_trips"], false,
        "and the gate stands down for the same reason: {}",
        output.stdout
    );
}

/// The case #2673 names as the reason the gate exists: the project is clean,
/// every baseline entry is dead, and the advisory is silent by design. A
/// consumer keying only on `stale` would stay green forever.
#[test]
fn a_cleaned_project_reports_a_silent_advisory_and_a_tripped_gate() {
    let project = rotted_baseline_project(4, 0);
    let output = run_with_baseline(project.path(), &["--format", "json", "--quiet"]);
    let envelope = parse_json(&output);
    assert_eq!(envelope["total_issues"], 0);
    let staleness = &envelope["baseline_staleness"];
    assert_eq!(staleness["current_findings"], 0);
    assert_eq!(staleness["stale"], false);
    assert_eq!(staleness["warning"], "none");
    assert_eq!(
        staleness["gate_trips"], true,
        "the gate is deliberately stricter than the advisory: {}",
        output.stdout
    );
}

#[test]
fn baseline_staleness_is_absent_without_a_baseline() {
    let project = rotted_baseline_project(4, 2);
    let output = run_fallow_raw(&[
        "dead-code",
        "--root",
        project.path().to_str().expect("temp path is UTF-8"),
        "--no-cache",
        "--format",
        "json",
        "--quiet",
    ]);
    let envelope = parse_json(&output);
    assert!(
        envelope.get("baseline_staleness").is_none(),
        "a run with no baseline keeps the wire byte-identical: {}",
        output.stdout
    );
}

/// `baseline` predates `baseline_staleness` and stays untouched, so anyone
/// gating on it is undisturbed. The two must never disagree.
#[test]
fn the_legacy_baseline_object_agrees_with_baseline_staleness() {
    let project = rotted_baseline_project(4, 2);
    let output = run_with_baseline(project.path(), &["--format", "json", "--quiet"]);
    let envelope = parse_json(&output);
    assert_eq!(
        envelope["baseline"]["entries"],
        envelope["baseline_staleness"]["baseline_entries"]
    );
    assert_eq!(
        envelope["baseline"]["matched"],
        envelope["baseline_staleness"]["matched_entries"]
    );
}

/// `gate_trips` is redundant with the three counts on purpose, so a consumer
/// reads one boolean instead of restating the rule. This pins the identity so
/// the redundancy cannot silently diverge from the engine.
#[test]
fn gate_trips_equals_the_rule_the_exit_gate_applies() {
    for (saved, remaining, extra) in [
        (4_u64, 2_u64, Vec::new()),
        (4, 0, Vec::new()),
        (4, 4, Vec::new()),
        (4, 2, vec!["--file", "src/index.ts"]),
    ] {
        let project = rotted_baseline_project(saved, remaining);
        let mut args = vec!["--format", "json", "--quiet"];
        args.extend_from_slice(&extra);
        let staleness = envelope_staleness(&run_with_baseline(project.path(), &args));
        let entries = staleness["baseline_entries"].as_u64().expect("entries");
        let matched = staleness["matched_entries"].as_u64().expect("matched");
        let change_scoped = staleness["change_scoped"].as_bool().expect("scoped");
        let unrecognised = staleness["unrecognised_format"]
            .as_bool()
            .unwrap_or_default();
        let expected = unrecognised || (!change_scoped && entries > 0 && matched < entries);
        assert_eq!(
            staleness["gate_trips"], expected,
            "gate_trips must equal unrecognised_format || (!change_scoped && entries > 0 && \
             matched < entries) for saved={saved} remaining={remaining} extra={extra:?}"
        );

        // And it must equal what the exit gate actually does.
        let mut gate_args = vec!["--format", "json", "--quiet", "--fail-on-stale-baseline"];
        gate_args.extend_from_slice(&extra);
        let gated = run_with_baseline(project.path(), &gate_args);
        assert_eq!(
            gated.code == 1,
            expected,
            "the published boolean and the exit code cannot disagree: {}",
            gated.stderr
        );
    }
}

/// `docs/backwards-compatibility.md` promises the flag changes nothing but the
/// exit code and the stderr line, so the object is emitted either way.
#[test]
fn baseline_staleness_does_not_depend_on_the_gate_flag() {
    let project = rotted_baseline_project(4, 2);
    let without = run_with_baseline(project.path(), &["--format", "json", "--quiet"]);
    let with = run_with_baseline(
        project.path(),
        &["--format", "json", "--quiet", "--fail-on-stale-baseline"],
    );
    assert_eq!(
        canonical_report_without_gate_outcomes(&without),
        canonical_report_without_gate_outcomes(&with),
        "the gate moves nothing in the report but its own armed-ness"
    );
    // 3.26.0 promised the flag changed nothing but the exit code and one stderr
    // line, because no envelope carried a gate verdict then. `gate_outcomes`
    // carries one now, and whether a verdict is armed is part of it, so exactly
    // one member moves and `baseline_staleness` itself still does not.
    assert_eq!(
        parse_json(&without)["baseline_staleness"],
        parse_json(&with)["baseline_staleness"],
        "the staleness object stays flag-independent"
    );
    assert_eq!(without.code, 0);
    assert_eq!(with.code, 1);
}

/// The new object is additive and absent by default, which is exactly the
/// condition `docs/backwards-compatibility.md` sets for not bumping a version.
/// Every envelope whose embedded shape changed is pinned, not only the two that
/// carry the object at their own root.
#[test]
fn adding_baseline_staleness_moved_no_schema_version() {
    let project = rotted_baseline_project(4, 2);
    let root = project.path().to_str().expect("temp path is UTF-8");
    let dead_code = run_with_baseline(project.path(), &["--format", "json", "--quiet"]);
    assert_eq!(parse_json(&dead_code)["schema_version"], 9);
    let dupes = run_fallow_raw(&[
        "dupes",
        "--root",
        root,
        "--no-cache",
        "--format",
        "json",
        "--quiet",
    ]);
    assert_eq!(parse_json(&dupes)["schema_version"], 10);
    let health = run_fallow_raw(&[
        "health",
        "--root",
        root,
        "--no-cache",
        "--format",
        "json",
        "--quiet",
    ]);
    assert_eq!(parse_json(&health)["schema_version"], 11);
    let combined = run_fallow_raw(&[
        "--root",
        project.path().to_str().expect("temp path is UTF-8"),
        "--no-cache",
        "--format",
        "json",
        "--quiet",
        "--baseline",
        project
            .path()
            .join("baseline.json")
            .to_str()
            .expect("temp path is UTF-8"),
    ]);
    assert_eq!(parse_json(&combined)["schema_version"], 12);
}

/// The bare combined run is the action's default shape.
#[test]
fn the_combined_envelope_carries_baseline_staleness_under_check() {
    let project = rotted_baseline_project(4, 2);
    let output = run_fallow_raw(&[
        "--root",
        project.path().to_str().expect("temp path is UTF-8"),
        "--no-cache",
        "--format",
        "json",
        "--quiet",
        "--baseline",
        project
            .path()
            .join("baseline.json")
            .to_str()
            .expect("temp path is UTF-8"),
    ]);
    let staleness = parse_json(&output)["check"]["baseline_staleness"].clone();
    assert_eq!(staleness["baseline_entries"], 4);
    assert_eq!(staleness["gate_trips"], true);
}

/// `--group-by` is a distinct envelope kind, and the MCP tools accept it, so an
/// agent can reach it. It must carry the same object as the flat envelope.
#[test]
fn the_grouped_dead_code_envelope_carries_baseline_staleness() {
    let project = rotted_baseline_project(4, 2);
    let output = run_with_baseline(
        project.path(),
        &["--format", "json", "--quiet", "--group-by", "directory"],
    );
    let envelope = parse_json(&output);
    assert_eq!(envelope["kind"], "dead-code-grouped");
    let staleness = &envelope["baseline_staleness"];
    assert_eq!(staleness["baseline_entries"], 4);
    assert_eq!(staleness["matched_entries"], 2);
    assert_eq!(staleness["gate_trips"], true);
    assert_eq!(staleness["moved_entries"], 0);
}

#[test]
fn the_grouped_dead_code_envelope_omits_baseline_staleness_without_a_baseline() {
    let project = rotted_baseline_project(4, 2);
    let output = run_fallow_raw(&[
        "dead-code",
        "--root",
        project.path().to_str().expect("temp path is UTF-8"),
        "--no-cache",
        "--format",
        "json",
        "--quiet",
        "--group-by",
        "directory",
    ]);
    assert!(
        parse_json(&output).get("baseline_staleness").is_none(),
        "a grouped run with no baseline keeps the wire byte-identical: {}",
        output.stdout
    );
}

/// A run without a diff source starts no git process for the diff filter.
/// The diff base candidates are needed only to place a diff, and resolving
/// them costs one `git rev-parse` at startup of every command.
#[cfg(unix)]
#[test]
fn a_run_without_a_diff_starts_no_git_process_for_the_diff_filter() {
    use std::os::unix::fs::PermissionsExt as _;

    let project = common::copy_fixture("basic-project");
    common::git(project.path(), &["init", "-q"]);
    let shim_dir = tempfile::tempdir().expect("shim directory");
    let log = shim_dir.path().join("git.log");
    let real_git = String::from_utf8(
        std::process::Command::new("sh")
            .args(["-c", "command -v git"])
            .output()
            .expect("locate git")
            .stdout,
    )
    .expect("git path is UTF-8");
    let shim = shim_dir.path().join("git");
    std::fs::write(
        &shim,
        format!(
            "#!/bin/sh\necho \"$@\" >> '{}'\nexec '{}' \"$@\"\n",
            log.display(),
            real_git.trim()
        ),
    )
    .expect("write git shim");
    std::fs::set_permissions(&shim, std::fs::Permissions::from_mode(0o755)).expect("chmod shim");
    let path = format!(
        "{}:{}",
        shim_dir.path().display(),
        std::env::var("PATH").unwrap_or_default()
    );

    let root = project.path().to_str().expect("UTF-8 root");
    let output = run_fallow_raw_with_env(
        &[
            "dead-code",
            "--root",
            root,
            "--format",
            "compact",
            "--quiet",
        ],
        &[("PATH", path.as_str()), ("FALLOW_DIFF_FILE", "")],
    );
    assert!(
        output.code == 0 || output.code == 1,
        "stderr: {}",
        output.stderr
    );
    let calls = std::fs::read_to_string(&log).unwrap_or_default();
    assert_eq!(calls, "", "unexpected git calls");
}
