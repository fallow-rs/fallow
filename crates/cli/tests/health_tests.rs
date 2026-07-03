#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests and benches use unwrap and expect to keep fixture setup concise"
)]

#[path = "common/mod.rs"]
mod common;

use common::{
    fixture_path, parse_json, redact_all, run_fallow, run_fallow_combined, run_fallow_in_root,
};
use std::path::Path;
use tempfile::tempdir;

fn write_file(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("create parent directories");
    }
    std::fs::write(path, contents).expect("write file");
}

fn copy_dir_recursive(src: &Path, dst: &Path) {
    std::fs::create_dir_all(dst).expect("create destination directory");
    for entry in std::fs::read_dir(src).expect("read source directory") {
        let entry = entry.expect("read source entry");
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        let file_type = entry.file_type().expect("read source entry type");
        if file_type.is_dir() {
            copy_dir_recursive(&src_path, &dst_path);
        } else if !file_type.is_dir() {
            std::fs::copy(&src_path, &dst_path).expect("copy file");
        }
    }
}

fn git(root: &Path, args: &[&str]) {
    let status = std::process::Command::new("git")
        .args(args)
        .current_dir(root)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .status()
        .expect("run git");
    assert!(status.success(), "git {args:?} should succeed");
}

#[test]
fn health_json_output_is_valid() {
    let output = run_fallow(
        "health",
        "complexity-project",
        &["--max-crap", "10000", "--format", "json", "--quiet"],
    );
    assert_eq!(output.code, 0, "health should succeed");
    let json = parse_json(&output);
    assert!(json.is_object(), "health JSON output should be an object");
}

#[test]
fn health_min_score_zero_exits_zero_with_findings() {
    let plain = run_fallow("health", "complexity-project", &["--quiet"]);
    assert_eq!(plain.code, 1, "plain health should still fail on findings");

    let gated = run_fallow(
        "health",
        "complexity-project",
        &["--min-score", "0", "--quiet"],
    );
    assert_eq!(
        gated.code, 0,
        "--min-score 0 must exit 0. stderr: {}",
        gated.stderr
    );
}

#[test]
fn health_min_score_below_threshold_fails() {
    let output = run_fallow(
        "health",
        "complexity-project",
        &["--min-score", "100", "--quiet"],
    );
    assert_eq!(
        output.code, 1,
        "--min-score 100 should fail the score gate. stderr: {}",
        output.stderr
    );
}

#[test]
fn health_min_score_demotes_rendered_findings_with_informational_note() {
    let output = run_fallow(
        "health",
        "complexity-project",
        &["--complexity", "--min-score", "0"],
    );
    assert_eq!(output.code, 0, "stderr: {}", output.stderr);
    assert!(
        output
            .stderr
            .contains("Findings above are informational: --min-score gates on the score"),
        "expected informational note in stderr, got: {}",
        output.stderr
    );
}

#[test]
fn health_report_only_never_fails() {
    let output = run_fallow(
        "health",
        "complexity-project",
        &["--report-only", "--quiet"],
    );
    assert_eq!(
        output.code, 0,
        "--report-only must exit 0 even with findings. stderr: {}",
        output.stderr
    );
}

#[test]
fn health_report_only_rejects_gate_flags() {
    let output = run_fallow(
        "health",
        "complexity-project",
        &[
            "--report-only",
            "--min-score",
            "80",
            "--format",
            "json",
            "--quiet",
        ],
    );
    assert_eq!(
        output.code, 2,
        "--report-only with --min-score should be rejected. stderr: {}",
        output.stderr
    );
    let json = parse_json(&output);
    assert_eq!(json["error"], serde_json::json!(true));
    let message = json["message"].as_str().expect("message should be present");
    assert!(
        message.contains("--report-only cannot be combined with"),
        "unexpected error message: {message}"
    );
}

#[test]
fn health_rejects_relative_coverage_root() {
    let output = run_fallow(
        "health",
        "complexity-project",
        &["--coverage-root", "src", "--format", "json", "--quiet"],
    );
    assert_eq!(
        output.code, 2,
        "relative --coverage-root should be rejected before health runs. stderr: {}",
        output.stderr
    );
    let json = parse_json(&output);
    assert_eq!(json["error"], serde_json::json!(true));
    let message = json["message"].as_str().expect("message should be present");
    assert!(
        message.contains("--coverage-root expects an absolute path")
            && message.contains("got 'src'"),
        "unexpected error message: {message}"
    );
}

#[test]
fn health_istanbul_matches_multiline_typed_async_arrow_signature() {
    let dir = tempdir().unwrap();
    write_file(
        &dir.path().join("package.json"),
        r#"{"name":"issue-370-coverage","type":"module"}"#,
    );
    let source_path = dir.path().join("src/actor.ts");
    write_file(
        &source_path,
        "type AnyLocator = unknown;
const resolveLocator = null as unknown as (locator: AnyLocator) => Promise<HTMLElement | HTMLElement[] | null>;
const isMissingElementError = null as unknown as (error: unknown) => boolean;
export const elementsFrom = async (
  locator: AnyLocator,
  options?: { missingAsEmpty?: boolean },
): Promise<HTMLElement[]> => {
  try {
    const result = await resolveLocator(locator);
    if (Array.isArray(result)) return result;
    return result ? [result] : [];
  } catch (error) {
    if (options?.missingAsEmpty === true && isMissingElementError(error)) return [];
    throw error;
  }
};
",
    );
    let coverage_path = dir.path().join("coverage/coverage-final.json");
    let mut coverage = serde_json::Map::new();
    coverage.insert(
        source_path.to_string_lossy().into_owned(),
        serde_json::json!({
            "path": source_path.to_string_lossy().into_owned(),
            "statementMap": {},
            "fnMap": {
                "0": {
                    "name": "(anonymous_0)",
                    "line": 7,
                    "decl": {
                        "start": { "line": 4, "column": 28 },
                        "end": { "line": 7, "column": 26 }
                    },
                    "loc": {
                        "start": { "line": 7, "column": 27 },
                        "end": { "line": 16, "column": 1 }
                    }
                }
            },
            "branchMap": {},
            "s": {},
            "f": { "0": 642 },
            "b": {}
        }),
    );
    write_file(
        &coverage_path,
        &serde_json::to_string(&coverage).expect("serialize coverage"),
    );

    let output = run_fallow_in_root(
        "health",
        dir.path(),
        &[
            "--complexity",
            "--coverage",
            "coverage/coverage-final.json",
            "--max-cyclomatic",
            "9999",
            "--max-cognitive",
            "9999",
            "--max-crap",
            "1",
            "--format",
            "json",
            "--quiet",
        ],
    );
    assert_eq!(
        output.code, 1,
        "low CRAP threshold should surface the covered function"
    );
    let json = parse_json(&output);
    assert_eq!(json["summary"]["istanbul_matched"].as_u64(), Some(1));

    let findings = json["findings"].as_array().expect("findings array");
    let finding = findings
        .iter()
        .find(|finding| finding["name"] == "elementsFrom")
        .unwrap_or_else(|| panic!("expected elementsFrom finding, got: {findings:#?}"));

    assert_eq!(finding["line"].as_u64(), Some(4));
    assert_eq!(finding["coverage_pct"].as_f64(), Some(100.0));
    assert_eq!(finding["coverage_tier"].as_str(), Some("high"));
    assert_eq!(finding["crap"].as_f64(), Some(7.0));
}

#[test]
fn health_json_has_findings() {
    let output = run_fallow(
        "health",
        "complexity-project",
        &["--complexity", "--format", "json", "--quiet"],
    );
    let json = parse_json(&output);
    assert!(
        json.get("findings").is_some(),
        "health JSON should have findings key"
    );
}

fn write_threshold_override_fixture(root: &Path, config: &str, source: &str) {
    write_file(
        &root.join("package.json"),
        r#"{"name":"threshold-override-fixture","type":"module","main":"src/legacy.ts"}"#,
    );
    write_file(&root.join(".fallowrc.json"), config);
    write_file(&root.join("src/legacy.ts"), source);
}

fn complex_threshold_override_source() -> &'static str {
    r"export function legacyFlow(input: number): number {
  let score = 0;
  if (input > 0) score += 1;
  if (input > 1) score += 1;
  if (input > 2) score += 1;
  if (input > 3) score += 1;
  if (input > 4) score += 1;
  if (input > 5) score += 1;
  return score;
}
"
}

#[test]
fn health_threshold_override_uses_local_ceiling() {
    let dir = tempdir().expect("create temp dir");
    write_threshold_override_fixture(
        dir.path(),
        r#"{
  "health": {
    "thresholdOverrides": [
      {
        "files": ["src/legacy.ts"],
        "functions": ["legacyFlow"],
        "maxCyclomatic": 20,
        "maxCognitive": 20,
        "reason": "legacy migration"
      }
    ]
  }
}
"#,
        complex_threshold_override_source(),
    );

    let output = run_fallow_in_root(
        "health",
        dir.path(),
        &[
            "--complexity",
            "--max-cyclomatic",
            "3",
            "--max-cognitive",
            "3",
            "--max-crap",
            "10000",
            "--format",
            "json",
            "--quiet",
        ],
    );
    assert_eq!(output.code, 0, "stderr: {}", output.stderr);
    let json = parse_json(&output);
    assert!(
        json["findings"].as_array().is_none_or(Vec::is_empty),
        "override should suppress local finding: {}",
        output.stdout
    );
    let states = json["threshold_overrides"]
        .as_array()
        .expect("threshold_overrides array");
    let state = states.first().expect("active override state");
    assert_eq!(state["status"].as_str(), Some("active"));
    assert_eq!(state["function"].as_str(), Some("legacyFlow"));
    assert_eq!(state["reason"].as_str(), Some("legacy migration"));
}

#[test]
fn health_threshold_override_reports_stale_when_under_global_threshold() {
    let dir = tempdir().expect("create temp dir");
    write_threshold_override_fixture(
        dir.path(),
        r#"{
  "health": {
    "thresholdOverrides": [
      { "files": ["src/legacy.ts"], "functions": ["legacyFlow"], "maxCyclomatic": 20 }
    ]
  }
}
"#,
        "export function legacyFlow(input: number): number {\n  return input + 1;\n}\n",
    );

    let output = run_fallow_in_root(
        "health",
        dir.path(),
        &[
            "--complexity",
            "--max-cyclomatic",
            "3",
            "--max-cognitive",
            "3",
            "--max-crap",
            "10000",
            "--format",
            "json",
            "--quiet",
        ],
    );
    assert_eq!(output.code, 0, "stderr: {}", output.stderr);
    let json = parse_json(&output);
    let states = json["threshold_overrides"]
        .as_array()
        .expect("threshold_overrides array");
    let state = states.first().expect("stale override state");
    assert_eq!(state["status"].as_str(), Some("stale"));
    assert_eq!(state["function"].as_str(), Some("legacyFlow"));
}

#[test]
fn health_threshold_override_omits_no_match_state_for_scoped_run() {
    let dir = tempdir().expect("create temp dir");
    write_threshold_override_fixture(
        dir.path(),
        r#"{
  "health": {
    "thresholdOverrides": [
      { "files": ["src/missing.ts"], "maxCyclomatic": 20 }
    ]
  }
}
"#,
        complex_threshold_override_source(),
    );
    git(dir.path(), &["init"]);
    git(dir.path(), &["config", "user.name", "Test User"]);
    git(dir.path(), &["config", "user.email", "test@example.com"]);
    git(dir.path(), &["add", "."]);
    git(dir.path(), &["commit", "-m", "initial"]);

    let output = run_fallow_in_root(
        "health",
        dir.path(),
        &[
            "--complexity",
            "--changed-since",
            "HEAD",
            "--max-cyclomatic",
            "50",
            "--max-cognitive",
            "50",
            "--max-crap",
            "10000",
            "--format",
            "json",
            "--quiet",
        ],
    );
    assert_eq!(output.code, 0, "stderr: {}", output.stderr);
    let json = parse_json(&output);
    assert!(
        json["threshold_overrides"]
            .as_array()
            .is_none_or(Vec::is_empty),
        "scoped run should not report no-match override state: {}",
        output.stdout
    );
}

#[test]
fn health_complexity_breakdown_gates_and_reconstructs_contributions() {
    // Without the flag, no `contributions` key is emitted on any finding.
    let without = parse_json(&run_fallow(
        "health",
        "complexity-project",
        &[
            "--complexity",
            "--max-cyclomatic",
            "1",
            "--format",
            "json",
            "--quiet",
        ],
    ));
    let findings = without
        .get("findings")
        .and_then(serde_json::Value::as_array)
        .expect("findings array");
    assert!(!findings.is_empty(), "fixture should produce findings");
    for f in findings {
        assert!(
            f.get("contributions").is_none(),
            "contributions must be omitted without --complexity-breakdown"
        );
    }

    // With the flag, each finding carries a breakdown that reconstructs the
    // aggregate metrics exactly (sum of weights, +1 for cyclomatic).
    let with = parse_json(&run_fallow(
        "health",
        "complexity-project",
        &[
            "--complexity",
            "--complexity-breakdown",
            "--max-cyclomatic",
            "1",
            "--format",
            "json",
            "--quiet",
        ],
    ));
    let findings = with
        .get("findings")
        .and_then(serde_json::Value::as_array)
        .expect("findings array");
    let mut saw_contributions = false;
    for f in findings {
        let Some(contribs) = f.get("contributions").and_then(serde_json::Value::as_array) else {
            continue;
        };
        saw_contributions = true;
        let sum = |metric: &str| -> u64 {
            contribs
                .iter()
                .filter(|c| c["metric"] == metric)
                .map(|c| c["weight"].as_u64().unwrap_or(0))
                .sum()
        };
        assert_eq!(
            sum("cyclomatic") + 1,
            f["cyclomatic"].as_u64().unwrap(),
            "cyclomatic contributions reconstruct the aggregate"
        );
        assert_eq!(
            sum("cognitive"),
            f["cognitive"].as_u64().unwrap(),
            "cognitive contributions reconstruct the aggregate"
        );
    }
    assert!(
        saw_contributions,
        "at least one finding should carry contributions with the flag"
    );
}

#[test]
fn health_reports_angular_template_complexity() {
    let output = run_fallow(
        "health",
        "angular-template-complexity",
        &[
            "--complexity",
            "--max-cyclomatic",
            "3",
            "--max-cognitive",
            "3",
            "--max-crap",
            "10000",
            "--format",
            "json",
            "--quiet",
        ],
    );
    let json = parse_json(&output);
    let findings = json["findings"].as_array().expect("findings array");
    let template = findings
        .iter()
        .find(|finding| {
            finding["name"] == "<template>"
                && finding["path"]
                    .as_str()
                    .is_some_and(|path| path.ends_with("permissions.component.html"))
        })
        .unwrap_or_else(|| panic!("expected template complexity finding, got: {findings:#?}"));

    assert!(
        template["cyclomatic"].as_u64().unwrap_or_default() > 3,
        "template should exceed cyclomatic threshold: {template:#?}"
    );
    assert!(
        template["cognitive"].as_u64().unwrap_or_default() > 3,
        "template should exceed cognitive threshold: {template:#?}"
    );
    let actions = template["actions"].as_array().expect("actions array");
    let suppress = actions
        .iter()
        .find(|action| action["type"] == "suppress-file")
        .unwrap_or_else(|| panic!("expected HTML suppress action, got: {actions:#?}"));
    assert_eq!(
        suppress["comment"],
        "<!-- fallow-ignore-file complexity -->"
    );
}

#[test]
fn health_emits_component_rollup_for_angular_component() {
    let output = run_fallow(
        "health",
        "angular-component-rollup",
        &[
            "--complexity",
            "--max-cyclomatic",
            "3",
            "--max-cognitive",
            "3",
            "--max-crap",
            "10000",
            "--format",
            "json",
            "--quiet",
        ],
    );
    let json = parse_json(&output);
    let findings = json["findings"].as_array().expect("findings array");

    let class_fn = findings
        .iter()
        .find(|finding| {
            finding["name"] == "handleClick"
                && finding["path"]
                    .as_str()
                    .is_some_and(|p| p.ends_with("host-game.component.ts"))
        })
        .unwrap_or_else(|| panic!("expected class function finding, got: {findings:#?}"));
    let class_cyc = class_fn["cyclomatic"].as_u64().expect("class cyclomatic");
    let class_cog = class_fn["cognitive"].as_u64().expect("class cognitive");

    let template = findings
        .iter()
        .find(|finding| {
            finding["name"] == "<template>"
                && finding["path"]
                    .as_str()
                    .is_some_and(|p| p.ends_with("host-game.component.html"))
        })
        .unwrap_or_else(|| panic!("expected template finding, got: {findings:#?}"));
    let template_cyc = template["cyclomatic"]
        .as_u64()
        .expect("template cyclomatic");
    let template_cog = template["cognitive"].as_u64().expect("template cognitive");

    let rollup = findings
        .iter()
        .find(|finding| {
            finding["name"] == "<component>"
                && finding["path"]
                    .as_str()
                    .is_some_and(|p| p.ends_with("host-game.component.ts"))
        })
        .unwrap_or_else(|| panic!("expected <component> rollup, got: {findings:#?}"));
    assert_eq!(
        rollup["cyclomatic"].as_u64().unwrap(),
        class_cyc + template_cyc,
        "rollup cyclomatic must equal worst class cyc + template cyc"
    );
    assert_eq!(
        rollup["cognitive"].as_u64().unwrap(),
        class_cog + template_cog,
        "rollup cognitive must equal worst class cog + template cog"
    );

    let breakdown = rollup["component_rollup"]
        .as_object()
        .unwrap_or_else(|| panic!("expected component_rollup payload, got: {rollup:#?}"));
    assert_eq!(
        breakdown["class_worst_function"].as_str().unwrap(),
        "handleClick"
    );
    assert_eq!(breakdown["class_cyclomatic"].as_u64().unwrap(), class_cyc);
    assert_eq!(
        breakdown["template_cyclomatic"].as_u64().unwrap(),
        template_cyc
    );
    let template_path = breakdown["template_path"]
        .as_str()
        .expect("template_path field");
    assert!(
        template_path.ends_with("host-game.component.html"),
        "template_path must point at the .html template, got: {template_path:?}"
    );
    assert!(
        !template_path.starts_with('/') && !template_path.contains("/var/folders/"),
        "template_path must be project-relative (no absolute prefix), got: {template_path:?}"
    );

    let actions = rollup["actions"].as_array().expect("rollup actions array");
    let suppress = actions
        .iter()
        .find(|a| a["type"] == "suppress-line")
        .unwrap_or_else(|| panic!("expected suppress-line on rollup, got: {actions:#?}"));
    assert_eq!(
        suppress["placement"].as_str().unwrap(),
        "above-component-worst-method",
        "rollup suppression must declare its placement so consumers can render the right hint"
    );
}

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "test fixture; linear setup/assert, length is not a maintainability concern"
)]
fn health_angular_template_crap_inherits_from_component_ts() {
    let dir = tempdir().unwrap();
    let fixture = fixture_path("angular-template-complexity");
    copy_dir_recursive(&fixture, dir.path());

    write_file(
        &dir.path().join("package.json"),
        r#"{
            "name": "issue-186-tier1-inherit",
            "main": "src/main.ts",
            "dependencies": {
                "@angular/core": "^19.0.0",
                "@angular/platform-browser": "^19.0.0"
            },
            "devDependencies": {
                "jest": "^29.0.0"
            }
        }"#,
    );

    write_file(
        &dir.path().join("src/permissions.component.spec.ts"),
        "import { PermissionsComponent } from './permissions.component';\n\
         describe('PermissionsComponent', () => {\n  \
           it('exists', () => { expect(PermissionsComponent).toBeDefined(); });\n\
         });\n",
    );

    let component_ts = dir.path().join("src/permissions.component.ts");
    let coverage_path = dir.path().join("coverage/coverage-final.json");
    let mut coverage = serde_json::Map::new();
    coverage.insert(
        component_ts.to_string_lossy().into_owned(),
        serde_json::json!({
            "path": component_ts.to_string_lossy().into_owned(),
            "statementMap": {},
            "fnMap": {},
            "branchMap": {},
            "s": {},
            "f": {},
            "b": {}
        }),
    );
    write_file(
        &coverage_path,
        &serde_json::to_string(&coverage).expect("serialize coverage"),
    );

    let output = run_fallow_in_root(
        "health",
        dir.path(),
        &[
            "--complexity",
            "--coverage",
            "coverage/coverage-final.json",
            "--max-cyclomatic",
            "3",
            "--max-cognitive",
            "3",
            "--max-crap",
            "30",
            "--format",
            "json",
            "--quiet",
        ],
    );
    let json = parse_json(&output);
    let findings = json["findings"].as_array().expect("findings array");
    let template = findings
        .iter()
        .find(|finding| {
            finding["name"] == "<template>"
                && finding["path"]
                    .as_str()
                    .is_some_and(|p| p.ends_with("permissions.component.html"))
        })
        .unwrap_or_else(|| panic!("expected <template> finding, got: {findings:#?}"));

    let coverage_source = template["coverage_source"]
        .as_str()
        .unwrap_or_else(|| panic!("expected coverage_source field, got: {template:#?}"));
    assert_eq!(
        coverage_source, "estimated_component_inherited",
        "<template> finding must carry the inherit-from-component discriminator (regression guard for #186 tier 1): {template:#?}"
    );

    let inherited_from = template["inherited_from"]
        .as_str()
        .unwrap_or_else(|| panic!("expected inherited_from field, got: {template:#?}"));
    assert!(
        inherited_from.ends_with("permissions.component.ts"),
        "inherited_from must point at the owning component .ts, got: {inherited_from:?}"
    );

    let tier = template["coverage_tier"]
        .as_str()
        .unwrap_or_else(|| panic!("expected coverage_tier field, got: {template:#?}"));
    assert!(
        matches!(tier, "partial" | "high"),
        "<template> coverage_tier inherited from the tested component .ts must be partial or high, got: {tier:?}"
    );

    let actions = template["actions"]
        .as_array()
        .expect("actions array present on health finding");
    let coverage_action = actions
        .iter()
        .find(|a| a["type"] == "increase-coverage")
        .unwrap_or_else(|| panic!("expected an increase-coverage action, got: {actions:#?}"));
    let target_path = coverage_action["target_path"].as_str().unwrap_or_else(|| {
        panic!("expected target_path on increase-coverage action, got: {coverage_action:#?}")
    });
    assert!(
        target_path.ends_with("permissions.component.ts"),
        "increase-coverage action's target_path must point at the owning .ts, got: {target_path:?}"
    );
}

#[test]
fn health_angular_template_inherit_rejects_non_component_owner() {
    let dir = tempdir().unwrap();
    write_file(
        &dir.path().join("package.json"),
        r#"{"name":"issue-186-negative","main":"src/main.ts"}"#,
    );
    write_file(
        &dir.path().join("src/main.ts"),
        "import \"./template.html\";\nexport const tag = \"plain\";\n",
    );
    write_file(
        &dir.path().join("src/template.html"),
        "@if (user) {\n  @if (user.isAdmin) {\n    @for (item of user.permissions; track item.id) {\n      @switch (item.status) {\n        @case ('active') { <a/> }\n        @case ('pending') { <b/> }\n        @default { <c/> }\n      }\n    }\n  }\n}\n",
    );

    let output = run_fallow_in_root(
        "health",
        dir.path(),
        &[
            "--complexity",
            "--max-cyclomatic",
            "3",
            "--max-cognitive",
            "3",
            "--max-crap",
            "30",
            "--format",
            "json",
            "--quiet",
        ],
    );
    let json = parse_json(&output);
    let findings = json["findings"].as_array().expect("findings array");
    let template = findings
        .iter()
        .find(|finding| {
            finding["name"] == "<template>"
                && finding["path"]
                    .as_str()
                    .is_some_and(|p| p.ends_with("template.html"))
        })
        .unwrap_or_else(|| panic!("expected <template> finding, got: {findings:#?}"));

    let source = template
        .get("coverage_source")
        .and_then(|v| v.as_str())
        .unwrap_or("none");
    assert_ne!(
        source, "estimated_component_inherited",
        "plain main.ts importing the template must not be credited as an Angular component owner: {template:#?}"
    );
    assert!(
        template.get("inherited_from").is_none()
            || template.get("inherited_from") == Some(&serde_json::Value::Null),
        "inherited_from must be absent when the owner is not an Angular component: {template:#?}"
    );
}

#[test]
fn health_reports_angular_inline_template_complexity() {
    let output = run_fallow(
        "health",
        "angular-inline-template-complexity",
        &[
            "--complexity",
            "--max-cyclomatic",
            "3",
            "--max-cognitive",
            "3",
            "--max-crap",
            "10000",
            "--format",
            "json",
            "--quiet",
        ],
    );
    let json = parse_json(&output);
    let findings = json["findings"].as_array().expect("findings array");
    let template = findings
        .iter()
        .find(|finding| {
            finding["name"] == "<template>"
                && finding["path"]
                    .as_str()
                    .is_some_and(|path| path.ends_with("host-game.component.ts"))
        })
        .unwrap_or_else(|| {
            panic!("expected inline template complexity finding, got: {findings:#?}")
        });

    assert!(
        template["cyclomatic"].as_u64().unwrap_or_default() > 3,
        "inline template should exceed cyclomatic threshold: {template:#?}"
    );
    assert!(
        template["cognitive"].as_u64().unwrap_or_default() > 3,
        "inline template should exceed cognitive threshold: {template:#?}"
    );
    assert_eq!(
        template["line"].as_u64(),
        Some(16),
        "inline template finding should anchor at the @Component decorator: {template:#?}"
    );
    let actions = template["actions"].as_array().expect("actions array");
    assert!(
        actions
            .iter()
            .any(|action| action["type"] == "suppress-line"),
        "inline template finding should expose a suppress-line action: {actions:#?}"
    );
    let suppress_line = actions
        .iter()
        .find(|action| action["type"] == "suppress-line")
        .expect("suppress-line action");
    assert_eq!(
        suppress_line["placement"].as_str(),
        Some("above-angular-decorator"),
        "inline template suppress-line should point at the decorator: {actions:#?}"
    );
    assert!(
        actions
            .iter()
            .all(|action| action["type"] != "suppress-file"),
        "inline template finding should not emit the HTML suppress-file action: {actions:#?}"
    );
}

#[test]
fn health_inline_template_complexity_can_be_suppressed() {
    let dir = tempdir().unwrap();
    let fixture = fixture_path("angular-inline-template-complexity");
    copy_dir_recursive(&fixture, dir.path());

    let component_path = dir.path().join("src/host-game.component.ts");
    let original = std::fs::read_to_string(&component_path).expect("read component");
    let prefixed = original.replacen(
        "@Component({",
        "// fallow-ignore-next-line complexity\n@Component({",
        1,
    );
    assert_ne!(
        original, prefixed,
        "fixture should contain a @Component decorator"
    );
    std::fs::write(&component_path, prefixed).expect("write suppressed component");

    let output = run_fallow_in_root(
        "health",
        dir.path(),
        &[
            "--complexity",
            "--max-cyclomatic",
            "3",
            "--max-cognitive",
            "3",
            "--max-crap",
            "10000",
            "--format",
            "json",
            "--quiet",
        ],
    );
    assert_eq!(
        output.code, 0,
        "suppressed inline template should not fail health"
    );
    let json = parse_json(&output);
    let findings = json["findings"].as_array();
    assert!(
        findings.is_none_or(|arr| arr.iter().all(|f| f["name"] != "<template>")),
        "suppressed inline template should not emit a <template> finding: {json:#?}"
    );
}

#[test]
fn health_html_template_complexity_can_be_suppressed() {
    let dir = tempdir().unwrap();
    let fixture = fixture_path("angular-template-complexity");
    copy_dir_recursive(&fixture, dir.path());

    let template_path = dir.path().join("src/permissions.component.html");
    let original = std::fs::read_to_string(&template_path).expect("read template");
    std::fs::write(
        &template_path,
        format!("<!-- fallow-ignore-file complexity -->\n{original}"),
    )
    .expect("write suppressed template");

    let output = run_fallow_in_root(
        "health",
        dir.path(),
        &[
            "--complexity",
            "--max-cyclomatic",
            "3",
            "--max-cognitive",
            "3",
            "--max-crap",
            "10000",
            "--format",
            "json",
            "--quiet",
        ],
    );
    assert_eq!(output.code, 0, "suppressed template should not fail health");
    let json = parse_json(&output);
    assert!(
        json["findings"].as_array().is_none_or(Vec::is_empty),
        "suppressed template should not emit findings: {json:#?}"
    );
}

#[test]
fn health_save_baseline_creates_parent_directory() {
    let dir = tempdir().unwrap();
    write_file(
        &dir.path().join("package.json"),
        r#"{"name":"health-save","version":"1.0.0"}"#,
    );
    write_file(
        &dir.path().join("src/index.ts"),
        r"export function alpha(value: number): number {
  if (value > 10) return value * 2;
  return value + 1;
}
",
    );

    let baseline_path = dir.path().join("fallow-baselines/health.json");
    let output = run_fallow_in_root(
        "health",
        dir.path(),
        &[
            "--targets",
            "--save-baseline",
            baseline_path.to_str().unwrap(),
            "--format",
            "json",
            "--quiet",
        ],
    );
    let rendered = redact_all(&format!("{}\n{}", output.stdout, output.stderr), dir.path());
    assert_eq!(
        output.code, 0,
        "health save baseline should succeed: {rendered}"
    );
    assert!(
        baseline_path.exists(),
        "health save baseline should create nested file: {rendered}"
    );
}

#[test]
fn health_exits_0_below_threshold() {
    let output = run_fallow(
        "health",
        "complexity-project",
        &[
            "--max-cyclomatic",
            "50",
            "--max-crap",
            "10000",
            "--complexity",
            "--format",
            "json",
            "--quiet",
        ],
    );
    assert_eq!(
        output.code, 0,
        "health should exit 0 when complexity below threshold"
    );
}

#[test]
fn health_exits_1_when_threshold_exceeded() {
    let output = run_fallow(
        "health",
        "complexity-project",
        &[
            "--max-cyclomatic",
            "3",
            "--complexity",
            "--fail-on-issues",
            "--format",
            "json",
            "--quiet",
        ],
    );
    assert_eq!(
        output.code, 1,
        "health should exit 1 when complexity exceeds threshold"
    );
}

/// With a high `--max-crap`, no function should trigger a CRAP finding and the
/// summary's `max_crap_threshold` must reflect the CLI override.
#[test]
fn health_exits_0_when_crap_below_threshold() {
    let output = run_fallow(
        "health",
        "complexity-project",
        &[
            "--max-cyclomatic",
            "99",
            "--max-crap",
            "10000",
            "--complexity",
            "--format",
            "json",
            "--quiet",
        ],
    );
    assert_eq!(
        output.code, 0,
        "health should exit 0 when CRAP stays below a very high threshold"
    );
    let json: serde_json::Value = serde_json::from_str(&output.stdout).unwrap();
    assert_eq!(
        json["summary"]["max_crap_threshold"].as_f64(),
        Some(10_000.0),
        "summary should echo the CLI-supplied threshold"
    );
}

/// With a very low `--max-crap`, every nontrivial function should become a
/// finding and the command must exit 1.
#[test]
fn health_exits_1_when_crap_threshold_exceeded() {
    let output = run_fallow(
        "health",
        "complexity-project",
        &[
            "--max-cyclomatic",
            "9999",
            "--max-cognitive",
            "9999",
            "--max-crap",
            "1",
            "--complexity",
            "--fail-on-issues",
            "--format",
            "json",
            "--quiet",
        ],
    );
    assert_eq!(
        output.code, 1,
        "health should exit 1 when any function has CRAP >= 1"
    );
    let json: serde_json::Value = serde_json::from_str(&output.stdout).unwrap();
    let findings = json["findings"].as_array().expect("findings array");
    assert!(
        !findings.is_empty(),
        "crap-triggered run should emit at least one finding"
    );
    let any_crap = findings
        .iter()
        .any(|f| f.get("crap").and_then(|v| v.as_f64()).is_some());
    assert!(
        any_crap,
        "at least one finding should carry a populated `crap` score when --max-crap triggered"
    );
}

#[test]
fn health_score_flag_shows_score() {
    let output = run_fallow(
        "health",
        "complexity-project",
        &["--score", "--format", "json", "--quiet"],
    );
    let json = parse_json(&output);
    assert!(
        json.get("score").is_some() || json.get("health_score").is_some(),
        "health --score should include score data"
    );
    let penalties = json["health_score"]["penalties"]
        .as_object()
        .expect("health --score should include penalty breakdown");
    assert!(
        !penalties.contains_key("hotspots"),
        "health --score should not run churn-backed hotspot analysis unless --hotspots is requested"
    );
    assert!(
        json.get("file_scores").is_none(),
        "health --score should not render file_scores"
    );
    assert!(
        json.get("coverage_gaps").is_none(),
        "health --score should not render coverage_gaps"
    );
    assert!(
        json.get("hotspot_summary").is_none(),
        "health --score should not render hotspot summaries"
    );
    assert!(
        json.get("vital_signs").is_none(),
        "health --score should not render vital signs"
    );
}

#[test]
fn health_vital_signs_carry_render_fan_in_on_react_project() {
    // Descriptive component render fan-in (the component-graph analogue of module
    // fan-in) is computed whenever React is declared and surfaced under the
    // existing `vital_signs` block (no flag, no rule).
    let output = run_fallow("health", "render-fan-in", &["--format", "json", "--quiet"]);
    let json = parse_json(&output);
    let vital = json
        .get("vital_signs")
        .expect("health renders vital_signs by default");
    assert_eq!(
        vital
            .get("max_render_fan_in")
            .and_then(serde_json::Value::as_u64),
        Some(3),
        "the headline is the honest DISTINCT-PARENTS count (Button = 3 parents), \
         not the inflated render-site count (6): {vital}"
    );
    // The located top-N is sorted by distinct_parents descending: Button (3) leads.
    let top = vital
        .get("top_render_fan_in")
        .and_then(serde_json::Value::as_array)
        .expect("top_render_fan_in present on a React project");
    let first = &top[0];
    assert_eq!(
        first.get("component").and_then(serde_json::Value::as_str),
        Some("Button"),
        "the top entry is sorted by distinct_parents desc (Button leads): {vital}"
    );
    assert_eq!(
        first
            .get("distinct_parents")
            .and_then(serde_json::Value::as_u64),
        Some(3),
        "Button's headline axis is distinct_parents = 3: {vital}"
    );
    assert_eq!(
        first
            .get("render_sites")
            .and_then(serde_json::Value::as_u64),
        Some(6),
        "render_sites is kept as secondary 'incl. repeats' context (6): {vital}"
    );
    // No test-file component (the fixture's __tests__/Button.test.tsx Page) leaks
    // into the located list.
    assert!(
        !top.iter()
            .any(|c| c.get("component").and_then(serde_json::Value::as_str) == Some("Page")),
        "a component defined in a test file must not appear in top_render_fan_in: {vital}"
    );
    assert!(
        vital.get("p95_render_fan_in").is_some(),
        "p95_render_fan_in is present on a React project: {vital}"
    );
    let high_pct = vital
        .get("render_fan_in_high_pct")
        .and_then(serde_json::Value::as_f64)
        .expect("render_fan_in_high_pct present on a React project");
    assert!(
        high_pct.is_finite() && (0.0..=100.0).contains(&high_pct),
        "render_fan_in_high_pct is a finite percentage: {high_pct}"
    );
}

#[test]
fn health_vital_signs_omit_render_fan_in_on_non_react_project() {
    // A non-React project computes no render fan-in; the three fields are
    // skip_serializing_if-omitted so the JSON contract is unchanged.
    let output = run_fallow(
        "health",
        "complexity-project",
        &["--format", "json", "--quiet"],
    );
    let json = parse_json(&output);
    let vital = json
        .get("vital_signs")
        .expect("health renders vital_signs by default");
    assert!(
        vital.get("max_render_fan_in").is_none(),
        "max_render_fan_in is omitted on a non-React project: {vital}"
    );
    assert!(
        vital.get("p95_render_fan_in").is_none(),
        "p95_render_fan_in is omitted on a non-React project: {vital}"
    );
    assert!(
        vital.get("render_fan_in_high_pct").is_none(),
        "render_fan_in_high_pct is omitted on a non-React project: {vital}"
    );
}

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "test fixture; linear setup/assert, length is not a maintainability concern"
)]
fn health_score_save_snapshot_keeps_hotspot_vital_signs() {
    let temp = tempdir().expect("create temp dir");
    let root = temp.path();
    write_file(
        &root.join("package.json"),
        r#"{"name":"health-score-snapshot","version":"1.0.0","type":"module"}"#,
    );
    write_file(
        &root.join("src/index.ts"),
        "export function risky(x: number) { if (x > 1) { if (x > 2) { if (x > 3) { if (x > 4) { if (x > 5) { return x; } } } } } return 0; }\n",
    );
    git(root, &["init"]);
    git(root, &["config", "user.email", "review@example.test"]);
    git(root, &["config", "user.name", "Review"]);
    git(root, &["add", "."]);
    git(root, &["commit", "-m", "initial"]);
    write_file(
        &root.join("src/index.ts"),
        "export function risky(x: number) { if (x > 1) { if (x > 2) { if (x > 3) { if (x > 4) { if (x > 5) { if (x > 6) { return x; } } } } } } return 0; }\n",
    );
    git(root, &["add", "."]);
    git(root, &["commit", "-m", "increase churn"]);

    let score_only = run_fallow_in_root(
        "health",
        root,
        &[
            "--score",
            "--min-commits",
            "1",
            "--since",
            "10y",
            "--format",
            "json",
            "--quiet",
        ],
    );
    let score_json = parse_json(&score_only);
    assert!(
        !score_json["health_score"]["penalties"]
            .as_object()
            .expect("score penalties")
            .contains_key("hotspots"),
        "plain --score should not compute churn-backed hotspot penalties"
    );

    let snapshot = run_fallow_in_root(
        "health",
        root,
        &[
            "--score",
            "--save-snapshot",
            "--min-commits",
            "1",
            "--since",
            "10y",
            "--format",
            "json",
            "--quiet",
        ],
    );
    let snapshot_json = parse_json(&snapshot);
    assert!(
        snapshot_json["health_score"]["penalties"]
            .as_object()
            .expect("snapshot score penalties")
            .contains_key("hotspots"),
        "snapshot score should include the hotspot penalty when hotspot vitals were computed"
    );

    let snapshot_dir = root.join(".fallow/snapshots");
    let snapshot_path = std::fs::read_dir(&snapshot_dir)
        .expect("read snapshot dir")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .find(|path| path.extension().is_some_and(|ext| ext == "json"))
        .expect("snapshot json should be saved");
    let saved_snapshot: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(snapshot_path).expect("read snapshot"))
            .expect("parse snapshot json");
    assert_eq!(
        saved_snapshot["vital_signs"]["hotspot_count"].as_u64(),
        Some(1),
        "--score --save-snapshot should still save hotspot vital signs"
    );

    let trend = run_fallow_in_root(
        "health",
        root,
        &[
            "--trend",
            "--min-commits",
            "1",
            "--since",
            "10y",
            "--format",
            "json",
            "--quiet",
        ],
    );
    let trend_json = parse_json(&trend);
    let trend_metrics = trend_json["health_trend"]["metrics"]
        .as_array()
        .expect("trend metrics");
    assert!(
        trend_metrics
            .iter()
            .any(|metric| metric["name"] == "hotspot_count"),
        "--trend should compare hotspot counts from complete snapshot data"
    );
}

#[test]
fn health_score_flag_with_config_does_not_render_coverage_gaps() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let config_path = dir.path().join("fallow.json");
    write_file(
        &config_path,
        r#"{
  "rules": {
    "coverage-gaps": "warn"
  }
}"#,
    );

    let root = fixture_path("production-mode");
    let output = common::run_fallow_in_root(
        "health",
        &root,
        &[
            "--config",
            config_path.to_str().expect("config path should be utf-8"),
            "--score",
            "--format",
            "json",
            "--quiet",
        ],
    );
    assert_eq!(output.code, 0, "health --score should still succeed");

    let json = parse_json(&output);
    assert!(
        json.get("coverage_gaps").is_none(),
        "config-enabled coverage gaps should not override explicit section selection"
    );
}

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "test fixture; linear setup/assert, length is not a maintainability concern"
)]
fn health_baseline_partial_overflow_does_not_emit_stale_baseline_warning() {
    let dir = tempfile::tempdir().expect("create temp dir");
    write_file(
        &dir.path().join("package.json"),
        r#"{"name":"baseline-health-repro","type":"module"}"#,
    );
    write_file(
        &dir.path().join("tsconfig.json"),
        r#"{"compilerOptions":{"target":"ES2020","module":"ES2020","strict":true},"include":["src"]}"#,
    );
    write_file(
        &dir.path().join("src/index.ts"),
        r#"export function alpha(items: number[]): string {
  let result = "";
  for (let i = 0; i < items.length; i++) {
    if (items[i] % 2 === 0) {
      if (items[i] % 3 === 0) {
        if (items[i] % 5 === 0) { result += "fizzbuzz"; }
        else { result += "fizz"; }
      } else if (items[i] % 5 === 0) { result += "buzz"; }
      else { result += String(items[i]); }
    } else {
      if (items[i] % 7 === 0) { result += "lucky"; }
      else if (items[i] > 50) {
        if (items[i] < 75) { result += "mid"; }
        else { result += "high"; }
      } else { result += "low"; }
    }
  }
  return result;
}"#,
    );

    let baseline_path = dir.path().join("health-baseline.json");
    let baseline_path_str = baseline_path
        .to_str()
        .expect("baseline path should be valid UTF-8");

    let save = run_fallow_in_root(
        "health",
        dir.path(),
        &[
            "--complexity",
            "--max-cyclomatic",
            "3",
            "--max-cognitive",
            "3",
            "--save-baseline",
            baseline_path_str,
        ],
    );
    let save_output = redact_all(&format!("{}\n{}", save.stdout, save.stderr), dir.path());
    assert!(
        save.code == 0 || save.code == 1,
        "save baseline should not crash: {save_output}"
    );
    assert!(
        baseline_path.exists(),
        "save baseline should create the baseline file: {save_output}"
    );
    assert!(
        save_output.contains("Saved health baseline to"),
        "save baseline should confirm the write: {save_output}"
    );

    write_file(
        &dir.path().join("src/index.ts"),
        r#"export function alpha(items: number[]): string {
  let result = "";
  for (let i = 0; i < items.length; i++) {
    if (items[i] % 2 === 0) {
      if (items[i] % 3 === 0) {
        if (items[i] % 5 === 0) { result += "fizzbuzz"; }
        else { result += "fizz"; }
      } else if (items[i] % 5 === 0) { result += "buzz"; }
      else { result += String(items[i]); }
    } else {
      if (items[i] % 7 === 0) { result += "lucky"; }
      else if (items[i] > 50) {
        if (items[i] < 75) { result += "mid"; }
        else { result += "high"; }
      } else { result += "low"; }
    }
  }
  return result;
}

export function beta(items: number[]): string {
  let result = "";
  for (let i = 0; i < items.length; i++) {
    if (items[i] % 2 === 0) {
      if (items[i] % 3 === 0) {
        if (items[i] % 5 === 0) { result += "fizzbuzz"; }
        else { result += "fizz"; }
      } else if (items[i] % 5 === 0) { result += "buzz"; }
      else { result += String(items[i]); }
    } else {
      if (items[i] % 7 === 0) { result += "lucky"; }
      else if (items[i] > 50) {
        if (items[i] < 75) { result += "mid"; }
        else { result += "high"; }
      } else { result += "low"; }
    }
  }
  return result;
}"#,
    );

    let load = run_fallow_in_root(
        "health",
        dir.path(),
        &[
            "--complexity",
            "--max-cyclomatic",
            "3",
            "--max-cognitive",
            "3",
            "--baseline",
            baseline_path_str,
        ],
    );
    let combined = redact_all(&format!("{}\n{}", load.stdout, load.stderr), dir.path());
    assert_eq!(
        load.code, 1,
        "baseline load should still report the overflowing findings: {combined}"
    );
    assert!(
        combined.contains("alpha") && combined.contains("beta"),
        "expected overflow run to still report both functions: {combined}"
    );
    assert!(
        !combined.contains("Warning: health baseline has"),
        "partial-overflow baseline should not look stale: {combined}"
    );
}

#[test]
fn health_score_flag_with_config_error_fails_without_rendering_coverage_gaps() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let config_path = dir.path().join("fallow.json");
    write_file(
        &config_path,
        r#"{
  "rules": {
    "coverage-gaps": "error"
  }
}
"#,
    );

    let root = fixture_path("production-mode");
    let output = common::run_fallow_in_root(
        "health",
        &root,
        &[
            "--config",
            config_path.to_str().expect("config path should be utf-8"),
            "--score",
            "--format",
            "json",
            "--quiet",
        ],
    );
    assert_eq!(
        output.code, 1,
        "coverage-gaps=error should still fail score-only health runs"
    );

    let json = parse_json(&output);
    assert!(
        json.get("coverage_gaps").is_none(),
        "gate-only coverage gaps should not be rendered in score-only output"
    );
}

#[test]
fn health_file_scores_flag() {
    let output = run_fallow(
        "health",
        "complexity-project",
        &["--file-scores", "--format", "json", "--quiet"],
    );
    let json = parse_json(&output);
    assert!(
        json.get("file_scores").is_some(),
        "health --file-scores should include file_scores"
    );
}

#[test]
fn health_file_scores_include_vue_sfc_files() {
    let output = run_fallow(
        "health",
        "vue-split-type-value-export",
        &["--file-scores", "--format", "json", "--quiet"],
    );
    assert_eq!(output.code, 0, "health should score Vue SFC files");

    let json = parse_json(&output);
    let file_scores = json["file_scores"]
        .as_array()
        .expect("health --file-scores should include file_scores");

    assert!(
        file_scores.iter().any(|score| {
            score.get("path").and_then(serde_json::Value::as_str) == Some("src/App.vue")
        }),
        "Vue SFC files should be included in file_scores: {file_scores:?}"
    );
}

#[test]
fn health_complexity_reports_vue_sfc_functions() {
    let output = run_fallow(
        "health",
        "vue-split-type-value-export",
        &[
            "--complexity",
            "--max-cyclomatic",
            "0",
            "--max-crap",
            "10000",
            "--format",
            "json",
            "--quiet",
        ],
    );
    assert_eq!(
        output.code, 1,
        "health should report Vue SFC complexity findings"
    );

    let json = parse_json(&output);
    let findings = json["findings"]
        .as_array()
        .expect("health --complexity should include findings");

    assert!(
        findings.iter().any(|finding| {
            finding.get("path").and_then(serde_json::Value::as_str) == Some("src/App.vue")
                && finding.get("name").and_then(serde_json::Value::as_str) == Some("isStatus")
        }),
        "Vue SFC functions should surface as health findings: {findings:?}"
    );
}

#[test]
fn health_coverage_gaps_flag_reports_runtime_gaps() {
    let output = run_fallow(
        "health",
        "coverage-gaps",
        &["--coverage-gaps", "--format", "json", "--quiet"],
    );
    assert_eq!(
        output.code, 0,
        "health --coverage-gaps defaults to warn severity (exit 0)"
    );

    let json = parse_json(&output);
    let coverage = json
        .get("coverage_gaps")
        .expect("health --coverage-gaps should include coverage_gaps");
    let files = coverage["files"]
        .as_array()
        .expect("coverage_gaps.files should be an array");
    let exports = coverage["exports"]
        .as_array()
        .expect("coverage_gaps.exports should be an array");

    let file_names: Vec<String> = files
        .iter()
        .filter_map(|item| item.get("path").and_then(serde_json::Value::as_str))
        .map(|p| p.replace('\\', "/"))
        .collect();
    assert!(
        file_names
            .iter()
            .any(|path| path.ends_with("src/setup-only.ts")),
        "setup-only.ts should remain untested even when referenced by test setup: {file_names:?}"
    );
    assert!(
        file_names
            .iter()
            .any(|path| path.ends_with("src/fixture-only.ts")),
        "fixture-only.ts should remain untested even when referenced by a fixture: {file_names:?}"
    );
    assert!(
        !file_names
            .iter()
            .any(|path| path.ends_with("src/covered.ts")),
        "covered.ts should not be reported as an untested file: {file_names:?}"
    );

    let export_names: Vec<_> = exports
        .iter()
        .filter_map(|item| item.get("export_name").and_then(serde_json::Value::as_str))
        .collect();
    assert!(
        !export_names.contains(&"covered"),
        "covered should not be reported as an untested export: {export_names:?}"
    );
    assert!(
        !export_names.contains(&"indirectlyCovered"),
        "exports already reported as dead code should be excluded from coverage gaps: {export_names:?}"
    );
}

#[test]
fn health_coverage_gaps_config_error_enforces_without_flag() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let config_path = dir.path().join("fallow.json");
    write_file(
        &config_path,
        r#"{
  "rules": {
    "coverage-gaps": "error"
  }
}
"#,
    );

    let root = fixture_path("production-mode");
    let output = common::run_fallow_in_root(
        "health",
        &root,
        &[
            "--config",
            config_path.to_str().expect("config path should be utf-8"),
            "--format",
            "json",
            "--quiet",
        ],
    );
    assert_eq!(
        output.code, 1,
        "coverage-gaps=error should fail health even without --coverage-gaps"
    );

    let json = parse_json(&output);
    assert!(
        json.get("coverage_gaps").is_some(),
        "config-enabled coverage gaps should be present in the report"
    );
}

#[test]
fn health_coverage_gaps_production_excludes_dead_test_helpers() {
    let output = run_fallow(
        "health",
        "production-mode",
        &[
            "--production",
            "--coverage-gaps",
            "--format",
            "json",
            "--quiet",
        ],
    );
    assert_eq!(
        output.code, 0,
        "runtime coverage gaps default to warn severity (exit 0)"
    );

    let json = parse_json(&output);
    let coverage = json["coverage_gaps"]
        .as_object()
        .expect("runtime coverage_gaps should be an object");

    let export_names: Vec<_> = coverage["exports"]
        .as_array()
        .expect("coverage_gaps.exports should be an array")
        .iter()
        .filter_map(|item| item.get("export_name").and_then(serde_json::Value::as_str))
        .collect();
    assert!(
        !export_names.contains(&"testHelper"),
        "exports already reported as dead code should not also be reported as coverage gaps: {export_names:?}"
    );
    assert!(
        export_names.contains(&"app") && export_names.contains(&"helper"),
        "runtime coverage gaps should still report runtime exports lacking test reachability: {export_names:?}"
    );

    let summary = coverage["summary"]
        .as_object()
        .expect("coverage_gaps.summary should be an object");
    assert_eq!(
        summary["untested_exports"].as_u64(),
        Some(2),
        "runtime coverage gaps should exclude dead exports from the export count"
    );
}

#[test]
fn health_coverage_gaps_suppressed_file_excluded() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let root = dir.path();
    copy_dir_recursive(&fixture_path("coverage-gaps"), root);

    write_file(
        &root.join("src/setup-only.ts"),
        r#"// fallow-ignore-file coverage-gaps
export function viaSetup(): string {
  return "setup";
}
"#,
    );

    let output = common::run_fallow_in_root(
        "health",
        root,
        &["--coverage-gaps", "--format", "json", "--quiet"],
    );

    let json = parse_json(&output);
    let coverage = json
        .get("coverage_gaps")
        .expect("coverage_gaps should be present");
    let file_paths: Vec<String> = coverage["files"]
        .as_array()
        .expect("files array")
        .iter()
        .filter_map(|item| item.get("path").and_then(serde_json::Value::as_str))
        .map(|p| p.replace('\\', "/"))
        .collect();

    assert!(
        !file_paths
            .iter()
            .any(|path| path.ends_with("src/setup-only.ts")),
        "setup-only.ts should be excluded when suppressed with fallow-ignore-file: {file_paths:?}"
    );

    let export_names: Vec<_> = coverage["exports"]
        .as_array()
        .expect("exports array")
        .iter()
        .filter_map(|item| item.get("export_name").and_then(serde_json::Value::as_str))
        .collect();
    assert!(
        !export_names.contains(&"viaSetup"),
        "viaSetup export should be excluded when file is suppressed: {export_names:?}"
    );
}

#[test]
fn health_coverage_gaps_workspace_scope_limits_results() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let root = dir.path();

    write_file(
        &root.join("package.json"),
        r#"{
  "name": "coverage-gaps-workspace",
  "private": true,
  "workspaces": ["packages/*"],
  "dependencies": {
    "vitest": "^3.2.4"
  }
}"#,
    );

    write_file(
        &root.join("packages/app/package.json"),
        r#"{
  "name": "app",
  "main": "src/main.ts"
}"#,
    );
    write_file(
        &root.join("packages/app/src/main.ts"),
        r#"import { covered } from "./covered";
import { appGap } from "./app-gap";

export const app = `${covered()}:${appGap()}`;
"#,
    );
    write_file(
        &root.join("packages/app/src/covered.ts"),
        r#"export function covered(): string {
  return "covered";
}
"#,
    );
    write_file(
        &root.join("packages/app/src/app-gap.ts"),
        r#"export function appGap(): string {
  return "app-gap";
}
"#,
    );
    write_file(
        &root.join("packages/app/tests/covered.test.ts"),
        r#"import { describe, expect, it } from "vitest";
import { covered } from "../src/covered";

describe("covered", () => {
  it("covers app runtime code selectively", () => {
    expect(covered()).toBe("covered");
  });
});
"#,
    );

    write_file(
        &root.join("packages/shared/package.json"),
        r#"{
  "name": "shared",
  "main": "src/index.ts"
}"#,
    );
    write_file(
        &root.join("packages/shared/src/index.ts"),
        r#"import { sharedGap } from "./shared-gap";

export const shared = sharedGap();
"#,
    );
    write_file(
        &root.join("packages/shared/src/shared-gap.ts"),
        r#"export function sharedGap(): string {
  return "shared-gap";
}
"#,
    );

    let output = common::run_fallow_in_root(
        "health",
        root,
        &[
            "--coverage-gaps",
            "--workspace",
            "app",
            "--format",
            "json",
            "--quiet",
        ],
    );
    assert_eq!(
        output.code, 0,
        "workspace-scoped health --coverage-gaps defaults to warn severity (exit 0)"
    );

    let json = parse_json(&output);
    let coverage = json["coverage_gaps"]
        .as_object()
        .expect("workspace-scoped coverage_gaps should be an object");

    let file_paths: Vec<String> = coverage["files"]
        .as_array()
        .expect("coverage_gaps.files should be an array")
        .iter()
        .filter_map(|item| item.get("path").and_then(serde_json::Value::as_str))
        .map(|p| p.replace('\\', "/"))
        .collect();
    assert!(
        file_paths.iter().all(|path| path.contains("packages/app/")),
        "workspace scope should only report app package files: {file_paths:?}"
    );
    assert!(
        file_paths
            .iter()
            .any(|path| path.ends_with("packages/app/src/app-gap.ts")),
        "app gap should be reported in workspace scope: {file_paths:?}"
    );
    assert!(
        !file_paths
            .iter()
            .any(|path| path.contains("packages/shared")),
        "shared package gaps should be excluded from app workspace scope: {file_paths:?}"
    );
}

#[test]
fn health_workspace_scopes_vital_signs_and_health_score() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let root = dir.path();

    write_file(
        &root.join("package.json"),
        r#"{
  "name": "ws-health-scope",
  "private": true,
  "workspaces": ["packages/*"]
}"#,
    );
    write_file(
        &root.join(".fallowrc.json"),
        r#"{"duplicates":{"min_tokens":10,"min_lines":3}}"#,
    );
    write_file(
        &root.join("packages/app/package.json"),
        r#"{ "name": "app", "main": "src/index.ts" }"#,
    );
    write_file(
        &root.join("packages/app/src/index.ts"),
        r"export const greet = (name: string): string => `hello ${name}`;
",
    );
    write_file(
        &root.join("packages/lib/package.json"),
        r#"{ "name": "lib", "main": "src/index.ts" }"#,
    );
    for i in 0..5 {
        write_file(
            &root.join(format!("packages/lib/src/util_{i}.ts")),
            &format!("export const fn_{i} = (a: number, b: number): number => a + b + {i};\n"),
        );
    }
    write_file(
        &root.join("packages/lib/src/index.ts"),
        r#"export * from "./util_0";
export * from "./util_1";
export * from "./util_2";
export * from "./util_3";
export * from "./util_4";
"#,
    );
    let duplicated_lib_function = r"export function duplicated(input: number): number {
  const first = input + 1;
  const second = first * 2;
  const third = second - 3;
  const fourth = third / 4;
  const fifth = fourth + 5;
  return fifth;
}
";
    write_file(
        &root.join("packages/lib/src/dup_a.ts"),
        duplicated_lib_function,
    );
    write_file(
        &root.join("packages/lib/src/dup_b.ts"),
        duplicated_lib_function,
    );

    git(root, &["init"]);
    git(root, &["config", "user.name", "Test User"]);
    git(root, &["config", "user.email", "test@example.com"]);
    git(root, &["add", "."]);
    git(root, &["commit", "-m", "initial"]);

    let monorepo = common::run_fallow_in_root(
        "health",
        root,
        &[
            "--score",
            "--complexity",
            "--file-scores",
            "--format",
            "json",
            "--quiet",
        ],
    );
    assert_eq!(monorepo.code, 0, "monorepo health run should succeed");
    let monorepo_json = parse_json(&monorepo);

    let snapshot_path = root.join(".fallow/app-snapshot.json");
    let snapshot_arg = snapshot_path.to_string_lossy().to_string();
    let scoped = common::run_fallow_in_root(
        "health",
        root,
        &[
            "--score",
            "--complexity",
            "--file-scores",
            "--workspace",
            "app",
            "--save-snapshot",
            &snapshot_arg,
            "--format",
            "json",
            "--quiet",
        ],
    );
    assert_eq!(scoped.code, 0, "workspace-scoped health run should succeed");
    let scoped_json = parse_json(&scoped);

    let monorepo_files = monorepo_json["summary"]["files_analyzed"]
        .as_u64()
        .expect("monorepo summary.files_analyzed");
    let scoped_files = scoped_json["summary"]["files_analyzed"]
        .as_u64()
        .expect("scoped summary.files_analyzed");
    assert!(
        scoped_files < monorepo_files,
        "summary.files_analyzed must scope to workspace (monorepo: {monorepo_files}, scoped: {scoped_files})"
    );

    let monorepo_loc = monorepo_json["vital_signs"]["total_loc"]
        .as_u64()
        .expect("monorepo vital_signs.total_loc");
    let scoped_loc = scoped_json["vital_signs"]["total_loc"]
        .as_u64()
        .expect("scoped vital_signs.total_loc");
    assert!(
        scoped_loc < monorepo_loc,
        "vital_signs.total_loc must scope to workspace (monorepo: {monorepo_loc}, scoped: {scoped_loc})"
    );

    let monorepo_duplication = monorepo_json["vital_signs"]["duplication_pct"]
        .as_f64()
        .expect("monorepo vital_signs.duplication_pct");
    let scoped_duplication = scoped_json["vital_signs"]["duplication_pct"]
        .as_f64()
        .expect("scoped vital_signs.duplication_pct");
    assert!(
        monorepo_duplication > scoped_duplication,
        "workspace score must not inherit duplication from another workspace (monorepo: {monorepo_duplication}, scoped: {scoped_duplication})"
    );
    assert!(
        scoped_duplication.abs() < f64::EPSILON,
        "app workspace has no duplicates, so scoped duplication should be zero"
    );
    assert_eq!(
        scoped_json["health_score"]["penalties"]["duplication"].as_f64(),
        Some(0.0),
        "app health score should not carry lib's duplication penalty"
    );

    let snapshot: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(&snapshot_path).expect("read saved app snapshot"),
    )
    .expect("parse saved app snapshot");
    assert_eq!(
        snapshot["counts"]["total_lines"], scoped_json["vital_signs"]["counts"]["total_lines"],
        "snapshot count totals must use the same workspace scope as JSON vital signs"
    );
}

#[test]
fn health_group_by_package_emits_per_workspace_envelope() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let root = dir.path();

    write_file(
        &root.join("package.json"),
        r#"{
  "name": "ws-grouped",
  "private": true,
  "workspaces": ["packages/*"]
}"#,
    );
    write_file(
        &root.join(".fallowrc.json"),
        r#"{"duplicates":{"min_tokens":10,"min_lines":3}}"#,
    );
    write_file(
        &root.join("packages/alpha/package.json"),
        r#"{ "name": "alpha", "main": "src/index.ts" }"#,
    );
    write_file(
        &root.join("packages/alpha/src/index.ts"),
        "export const a = (n: number): number => n * 2;\n",
    );
    write_file(
        &root.join("packages/beta/package.json"),
        r#"{ "name": "beta", "main": "src/index.ts" }"#,
    );
    write_file(
        &root.join("packages/beta/src/index.ts"),
        "export const b = (n: number): number => n + 1;\n",
    );
    let duplicated_beta_function = r"export function duplicated(input: number): number {
  const first = input + 1;
  const second = first * 2;
  const third = second - 3;
  const fourth = third / 4;
  const fifth = fourth + 5;
  return fifth;
}
";
    write_file(
        &root.join("packages/alpha/src/cross_group_dup.ts"),
        duplicated_beta_function,
    );
    write_file(
        &root.join("packages/beta/src/dup_a.ts"),
        duplicated_beta_function,
    );
    write_file(
        &root.join("packages/beta/src/dup_b.ts"),
        duplicated_beta_function,
    );

    git(root, &["init"]);
    git(root, &["config", "user.name", "Test User"]);
    git(root, &["config", "user.email", "test@example.com"]);
    git(root, &["add", "."]);
    git(root, &["commit", "-m", "initial"]);

    let output = common::run_fallow_in_root(
        "health",
        root,
        &[
            "--score",
            "--complexity",
            "--file-scores",
            "--group-by",
            "package",
            "--format",
            "json",
            "--quiet",
        ],
    );
    assert_eq!(output.code, 0, "grouped health run should succeed");
    let json = parse_json(&output);

    assert_eq!(
        json["grouped_by"].as_str(),
        Some("package"),
        "grouped_by should be 'package'"
    );
    let groups = json["groups"]
        .as_array()
        .expect("groups should be an array");
    let keys: Vec<&str> = groups.iter().filter_map(|g| g["key"].as_str()).collect();
    assert!(
        keys.contains(&"alpha"),
        "groups must include alpha workspace: {keys:?}"
    );
    assert!(
        keys.contains(&"beta"),
        "groups must include beta workspace: {keys:?}"
    );

    for group in groups {
        let key = group["key"].as_str().unwrap_or("?");
        assert!(
            group.get("vital_signs").is_some(),
            "group {key} must carry per-group vital_signs"
        );
        assert!(
            group.get("health_score").is_some(),
            "group {key} must carry per-group health_score"
        );
        assert!(
            group["files_analyzed"].as_u64().is_some(),
            "group {key} must report files_analyzed"
        );
    }
    let alpha = groups
        .iter()
        .find(|g| g["key"] == "alpha")
        .expect("alpha group");
    let beta = groups
        .iter()
        .find(|g| g["key"] == "beta")
        .expect("beta group");
    assert_eq!(
        alpha["vital_signs"]["duplication_pct"].as_f64(),
        Some(0.0),
        "alpha must not inherit beta's duplicate-code score input"
    );
    assert!(
        beta["vital_signs"]["duplication_pct"]
            .as_f64()
            .unwrap_or(0.0)
            > 0.0,
        "beta should carry its own duplicate-code score input"
    );
    assert_eq!(
        alpha["health_score"]["penalties"]["duplication"].as_f64(),
        Some(0.0),
        "alpha health score should not be penalized for beta duplication"
    );
    assert!(
        beta["health_score"]["penalties"]["duplication"]
            .as_f64()
            .unwrap_or(0.0)
            > 0.0,
        "beta health score should include its duplicate-code penalty"
    );

    assert!(
        json["vital_signs"].is_object(),
        "top-level vital_signs must remain populated alongside groups"
    );
    assert!(
        json["health_score"].is_object(),
        "top-level health_score must remain populated alongside groups"
    );
}

#[test]
fn health_group_by_package_tags_sarif_results_with_group() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let root = dir.path();

    write_file(
        &root.join("package.json"),
        r#"{
  "name": "ws-grouped-sarif",
  "private": true,
  "workspaces": ["packages/*"]
}"#,
    );
    write_file(
        &root.join("packages/alpha/package.json"),
        r#"{ "name": "alpha", "main": "src/index.ts" }"#,
    );
    write_file(
        &root.join("packages/alpha/src/index.ts"),
        r"export const branchy = (n: number): number => {
  if (n > 0) return 1;
  if (n < 0) return -1;
  if (n === 42) return 42;
  return 0;
};
",
    );
    write_file(
        &root.join("packages/beta/package.json"),
        r#"{ "name": "beta", "main": "src/index.ts" }"#,
    );
    write_file(
        &root.join("packages/beta/src/index.ts"),
        r"export const branchy = (n: number): number => {
  if (n > 0) return 1;
  if (n < 0) return -1;
  if (n === 42) return 42;
  return 0;
};
",
    );

    let sarif = common::run_fallow_in_root(
        "health",
        root,
        &[
            "--complexity",
            "--max-cyclomatic",
            "1",
            "--group-by",
            "package",
            "--format",
            "sarif",
            "--quiet",
        ],
    );
    let sarif_json = parse_json(&sarif);
    let runs = sarif_json["runs"]
        .as_array()
        .expect("SARIF runs should be an array");
    let mut sarif_groups: rustc_hash::FxHashSet<String> = rustc_hash::FxHashSet::default();
    let mut sarif_results = 0usize;
    for run in runs {
        if let Some(results) = run["results"].as_array() {
            for r in results {
                sarif_results += 1;
                if let Some(g) = r["properties"]["group"].as_str() {
                    sarif_groups.insert(g.to_owned());
                }
            }
        }
    }
    assert!(
        sarif_results > 0,
        "SARIF should contain at least one result"
    );
    assert!(
        sarif_groups.contains("alpha") && sarif_groups.contains("beta"),
        "SARIF results should tag alpha and beta groups: {sarif_groups:?}"
    );

    let cc = common::run_fallow_in_root(
        "health",
        root,
        &[
            "--complexity",
            "--max-cyclomatic",
            "1",
            "--group-by",
            "package",
            "--format",
            "codeclimate",
            "--quiet",
        ],
    );
    let cc_json = parse_json(&cc);
    let issues = cc_json
        .as_array()
        .expect("CodeClimate output should be an array");
    let mut cc_groups: rustc_hash::FxHashSet<String> = rustc_hash::FxHashSet::default();
    for issue in issues {
        if let Some(g) = issue["group"].as_str() {
            cc_groups.insert(g.to_owned());
        }
    }
    assert!(
        !issues.is_empty(),
        "CodeClimate should emit at least one issue"
    );
    assert!(
        cc_groups.contains("alpha") && cc_groups.contains("beta"),
        "CodeClimate issues should tag alpha and beta groups: {cc_groups:?}"
    );
}

#[test]
fn health_group_by_non_monorepo_emits_single_json_error() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let root = dir.path();

    write_file(
        &root.join("package.json"),
        r#"{ "name": "single", "main": "src/index.ts" }"#,
    );
    write_file(&root.join("src/index.ts"), "export const x = 1;\n");

    let output = common::run_fallow_in_root(
        "health",
        root,
        &["--group-by", "package", "--format", "json", "--quiet"],
    );
    assert_ne!(
        output.code, 0,
        "non-monorepo --group-by package should fail"
    );

    let parsed: serde_json::Value =
        serde_json::from_str(&output.stdout).expect("stdout should be a single valid JSON object");
    assert_eq!(parsed["error"], serde_json::json!(true));
    let msg = parsed["message"]
        .as_str()
        .expect("error message should be a string");
    assert!(
        msg.contains("monorepo"),
        "error message should mention 'monorepo': {msg}"
    );
}

#[test]
fn health_coverage_gaps_changed_since_scopes_results() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let root = dir.path();
    copy_dir_recursive(&fixture_path("coverage-gaps"), root);

    git(root, &["init"]);
    git(root, &["config", "user.name", "Test User"]);
    git(root, &["config", "user.email", "test@example.com"]);
    git(root, &["add", "."]);
    git(root, &["commit", "-m", "initial"]);

    write_file(
        &root.join("src/fixture-only.ts"),
        r#"export function viaFixture(): string {
  return "fixture-only-updated";
}
"#,
    );
    git(root, &["add", "src/fixture-only.ts"]);
    git(root, &["commit", "-m", "update fixture gap"]);

    let output = common::run_fallow_in_root(
        "health",
        root,
        &[
            "--coverage-gaps",
            "--changed-since",
            "HEAD~1",
            "--format",
            "json",
            "--quiet",
        ],
    );
    assert_eq!(
        output.code, 0,
        "changed-since coverage gaps defaults to warn severity (exit 0)"
    );

    let json = parse_json(&output);
    let coverage = json["coverage_gaps"]
        .as_object()
        .expect("changed-since coverage_gaps should be an object");

    let file_paths: Vec<String> = coverage["files"]
        .as_array()
        .expect("coverage_gaps.files should be an array")
        .iter()
        .filter_map(|item| item.get("path").and_then(serde_json::Value::as_str))
        .map(|p| p.replace('\\', "/"))
        .collect();
    assert_eq!(
        file_paths.len(),
        1,
        "changed-since should limit file gaps to changed files: {file_paths:?}"
    );
    assert!(
        file_paths[0].ends_with("src/fixture-only.ts"),
        "changed-since should report the changed fixture-only file, got: {file_paths:?}"
    );

    let summary = coverage["summary"]
        .as_object()
        .expect("coverage_gaps.summary should be an object");
    assert_eq!(
        summary["runtime_files"].as_u64(),
        Some(1),
        "changed-since should recompute runtime scope summary for changed files only"
    );
}

#[test]
fn health_human_output_snapshot() {
    let output = run_fallow(
        "health",
        "complexity-project",
        &["--complexity", "--max-cyclomatic", "10", "--quiet"],
    );
    let root = fixture_path("complexity-project");
    let redacted = redact_all(&output.stdout, &root);
    insta::assert_snapshot!("health_human_complexity", redacted);
}

#[test]
fn health_file_scores_include_plugin_scoped_hidden_dirs_for_react_router() {
    let output = run_fallow(
        "health",
        "react-router-conventions",
        &["--file-scores", "--format", "json", "--quiet"],
    );
    assert_eq!(output.code, 0, "stderr was: {}", output.stderr);

    let json = parse_json(&output);
    let files_analyzed = json["summary"]["files_analyzed"]
        .as_u64()
        .expect("files_analyzed is a number");
    assert!(
        files_analyzed >= 5,
        "expected files_analyzed >= 5 (root + routes + .client + .server), got {files_analyzed}"
    );

    let scored_paths: Vec<&str> = json["file_scores"]
        .as_array()
        .expect("file_scores array")
        .iter()
        .filter_map(|fs| fs["path"].as_str())
        .collect();
    assert!(
        scored_paths.contains(&"app/.client/analytics.ts"),
        "expected app/.client/analytics.ts in file_scores: {scored_paths:?}"
    );
}

/// Count occurrences of a literal substring in `haystack`.
fn count_occurrences(haystack: &str, needle: &str) -> usize {
    if needle.is_empty() {
        return 0;
    }
    let mut count = 0;
    let mut start = 0;
    while let Some(pos) = haystack[start..].find(needle) {
        count += 1;
        start += pos + needle.len();
    }
    count
}

/// Regression test for issue #557: `fallow --score` (bare combined mode) must
/// render the health score in human output, and must render it EXACTLY ONCE.
/// The score has always been present in JSON / SARIF / CodeClimate; only the
/// terminal renderer was missing the call into `render_health_score`. The
/// "exactly once" assertion guards the second half of the fix: a naive
/// orientation-header render would double the line because the downstream
/// Complexity section's own `print_health_human` call would render it again.
#[test]
fn combined_score_renders_health_score_exactly_once() {
    let output = run_fallow_combined("complexity-project", &["--score"]);
    assert!(
        output.code == 0 || output.code == 1,
        "combined --score should not crash: stdout={}\nstderr={}",
        output.stdout,
        output.stderr
    );
    let count = count_occurrences(&output.stderr, "Health score:");
    assert_eq!(
        count, 1,
        "combined --score must render the Health score line exactly once \
         (no duplicate from the downstream Complexity section), got {count}:\n{}",
        output.stderr
    );
}

/// Control: without `--score`, the bare `fallow` invocation must NOT render a
/// Health score line. This guards against accidentally always rendering the
/// score regardless of the flag.
#[test]
fn combined_without_score_omits_health_score_line() {
    let output = run_fallow_combined("complexity-project", &[]);
    assert!(
        output.code == 0 || output.code == 1,
        "bare combined run should not crash: stdout={}\nstderr={}",
        output.stdout,
        output.stderr
    );
    assert!(
        !output.stderr.contains("Health score:"),
        "bare `fallow` (no --score) must NOT render a Health score line, got:\n{}",
        output.stderr
    );
}

/// Standalone `fallow health --score` must keep rendering the score inline:
/// it has no upstream orientation header to absorb the responsibility. Pins
/// the second half of the `skip_score_and_trend` contract (combined skips,
/// standalone does not).
#[test]
fn standalone_health_score_still_renders_inline() {
    let output = run_fallow("health", "complexity-project", &["--score"]);
    assert!(
        output.code == 0 || output.code == 1,
        "fallow health --score should not crash: stdout={}\nstderr={}",
        output.stdout,
        output.stderr
    );
    let combined = format!("{}{}", output.stdout, output.stderr);
    let count = count_occurrences(&combined, "Health score:");
    assert_eq!(
        count, 1,
        "fallow health --score must render the Health score line exactly once, got {count}:\nstdout={}\nstderr={}",
        output.stdout, output.stderr
    );
}

/// `--min-score` is a `fallow health` (subcommand) flag, not a combined-mode
/// flag. Pin that the standalone exit-code gate still fires when the score is
/// below threshold, independent of where the human renderer emits the score.
#[test]
fn health_min_score_gate_fails_below_threshold() {
    let output = run_fallow(
        "health",
        "complexity-project",
        &["--score", "--min-score", "100"],
    );
    assert_ne!(
        output.code, 0,
        "fallow health --score --min-score 100 should fail the gate: stdout={}\nstderr={}",
        output.stdout, output.stderr
    );
}

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "test fixture; linear setup/assert, length is not a maintainability concern"
)]
fn health_churn_file_powers_hotspots_and_ownership_without_git() {
    let dir = tempdir().unwrap();
    write_file(
        &dir.path().join("package.json"),
        r#"{"name":"churn-import","type":"module"}"#,
    );
    // A genuinely complex function so the file can rank as a hotspot.
    write_file(
        &dir.path().join("src/hot.ts"),
        r#"export function classify(n: number, mode: string): string {
  let out = "";
  if (mode === "a") { if (n > 10) out = "big"; else if (n > 5) out = "mid"; else out = "small"; }
  else if (mode === "b") { for (let i = 0; i < n; i++) { if (i % 2 === 0) out += "x"; else out += "y"; } }
  else if (mode === "c") { switch (n) { case 1: out = "one"; break; case 2: out = "two"; break; default: out = "z"; } }
  else { out = n > 0 ? (n > 100 ? "huge" : "pos") : "neg"; }
  return out;
}
"#,
    );

    // Timestamps relative to now so the recency window stays valid as the
    // calendar moves (no hardcoded absolute dates).
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let day = 86_400;
    let churn = serde_json::json!({
        "schema": "fallow-churn/v1",
        "events": [
            { "path": "src/hot.ts", "timestamp": now - day, "author": "alice@corp", "added": 40, "deleted": 12 },
            { "path": "src/hot.ts", "timestamp": now - 2 * day, "author": "alice@corp", "added": 20, "deleted": 5 },
            { "path": "src/hot.ts", "timestamp": now - 4 * day, "author": "alice@corp", "added": 10, "deleted": 3 },
            { "path": "src/hot.ts", "timestamp": now - 3 * day, "author": "bob@corp", "added": 15, "deleted": 8 }
        ]
    });
    write_file(
        &dir.path().join("churn.json"),
        &serde_json::to_string(&churn).unwrap(),
    );

    // Imported churn powers hotspots on a directory with NO .git.
    let output = run_fallow_in_root(
        "health",
        dir.path(),
        &[
            "--hotspots",
            "--ownership",
            "--churn-file",
            "churn.json",
            "--format",
            "json",
        ],
    );
    assert_eq!(output.code, 0, "stderr: {}", output.stderr);
    let json = parse_json(&output);
    let hotspots = json["hotspots"].as_array().expect("hotspots array");
    assert!(
        !hotspots.is_empty(),
        "imported churn should produce hotspots: {}",
        output.stdout
    );
    assert!(
        hotspots[0]["path"]
            .as_str()
            .unwrap()
            .replace('\\', "/")
            .ends_with("src/hot.ts"),
        "top hotspot should be hot.ts: {}",
        output.stdout
    );
    // Header reflects the imported window, not a git "--since" duration.
    assert_eq!(
        json["hotspot_summary"]["since"].as_str(),
        Some("imported churn")
    );
    // Ownership / bus-factor derives from the imported authors.
    let ownership = &hotspots[0]["ownership"];
    assert_eq!(ownership["bus_factor"].as_u64(), Some(1));
    assert_eq!(ownership["contributor_count"].as_u64(), Some(2));

    // Neuter check: WITHOUT --churn-file the same non-git dir skips hotspots,
    // proving the imported data (not git) is what lit them up.
    let no_churn = run_fallow_in_root(
        "health",
        dir.path(),
        &["--hotspots", "--ownership", "--format", "json"],
    );
    assert_eq!(no_churn.code, 0, "stderr: {}", no_churn.stderr);
    let no_churn_json = parse_json(&no_churn);
    let absent_or_empty = no_churn_json
        .get("hotspots")
        .and_then(|v| v.as_array())
        .is_none_or(|a| a.is_empty());
    assert!(
        absent_or_empty,
        "no git + no churn-file should skip hotspots: {}",
        no_churn.stdout
    );

    // A malformed churn file is a hard error (exit 2), not a silent skip.
    write_file(
        &dir.path().join("bad.json"),
        r#"{ "schema": "nope", "events": [] }"#,
    );
    let bad = run_fallow_in_root(
        "health",
        dir.path(),
        &["--hotspots", "--churn-file", "bad.json", "--format", "json"],
    );
    assert_eq!(
        bad.code, 2,
        "malformed churn file should exit 2: stdout={} stderr={}",
        bad.stdout, bad.stderr
    );
    assert_eq!(parse_json(&bad)["error"].as_bool(), Some(true));

    // Inert: with no churn-consuming section (--score only), the same malformed
    // file is never validated, so it does not fail the run. The gate is
    // validate-iff-consume.
    let inert = run_fallow_in_root(
        "health",
        dir.path(),
        &["--score", "--churn-file", "bad.json", "--format", "json"],
    );
    assert_eq!(
        inert.code, 0,
        "churn-file is inert without a churn-consuming section: stdout={} stderr={}",
        inert.stdout, inert.stderr
    );
}

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "test fixture; linear setup/assert, length is not a maintainability concern"
)]
fn health_css_flag_surfaces_css_analytics() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    write_file(
        &root.join("package.json"),
        r#"{"name":"css-fixture","version":"1.0.0"}"#,
    );
    write_file(&root.join("src/index.ts"), "export const x = 1;\n");
    // An id selector (specificity a=1) and an over-complex compound selector are
    // both structurally notable; a plain class rule is not.
    write_file(
        &root.join("src/styles.css"),
        ":root { --brand: 4px; --unused-token: 0; }\n\
         #main { color: red; z-index: 5; }\n\
         .a.b.c.d.e { color: blue; font-size: 12px; }\n\
         .themed { width: var(--brand); }\n\
         @keyframes spin { from {} to {} }\n\
         @keyframes dead-anim { from {} }\n\
         .spinner { animation-name: spin; }\n\
         .plain { color: green; }\n",
    );

    // Without --css the section is absent (default output unchanged).
    let plain = run_fallow_in_root(
        "health",
        root,
        &["--max-crap", "10000", "--format", "json", "--quiet"],
    );
    let plain_json = parse_json(&plain);
    assert!(
        plain_json.get("css_analytics").is_none(),
        "css_analytics must be absent without --css: {}",
        plain.stdout
    );

    // With --css the section reports the stylesheet and its notable rules.
    let out = run_fallow_in_root(
        "health",
        root,
        &[
            "--css",
            "--max-crap",
            "10000",
            "--format",
            "json",
            "--quiet",
        ],
    );
    let json = parse_json(&out);
    let css = json
        .get("css_analytics")
        .expect("css_analytics present with --css");
    assert_eq!(css["summary"]["files_analyzed"], 1);
    let files = css["files"].as_array().expect("files array");
    assert!(!files.is_empty(), "notable rules surface the stylesheet");
    let notable: Vec<_> = files
        .iter()
        .flat_map(|f| f["analytics"]["notable_rules"].as_array().unwrap().iter())
        .collect();
    assert!(
        notable.iter().any(|r| r["specificity_a"] == 1),
        "the #main id selector contributes specificity a=1"
    );
    assert!(
        notable
            .iter()
            .any(|r| r["complexity"].as_u64().unwrap() > 4),
        "the five-class compound selector is over-complex"
    );

    // Design-token sprawl: three distinct colors, one font size, one z-index.
    let summary = &css["summary"];
    assert_eq!(summary["unique_colors"], 3, "summary: {summary}");
    assert_eq!(summary["unique_font_sizes"], 1, "summary: {summary}");
    assert_eq!(summary["unique_z_indexes"], 1, "summary: {summary}");

    // Deadness candidates: --unused-token and @keyframes dead-anim are defined
    // but never referenced in CSS.
    assert_eq!(
        summary["custom_properties_defined"], 2,
        "summary: {summary}"
    );
    assert_eq!(
        summary["custom_properties_unreferenced"], 1,
        "summary: {summary}"
    );
    assert_eq!(summary["keyframes_defined"], 2, "summary: {summary}");
    assert_eq!(summary["keyframes_unreferenced"], 1, "summary: {summary}");
    assert_eq!(summary["notable_truncated_files"], 0, "summary: {summary}");

    // The unreferenced @keyframes is LOCATED (name + path), not just counted.
    let keyframes = css["unreferenced_keyframes"]
        .as_array()
        .expect("unreferenced_keyframes located list");
    assert_eq!(keyframes.len(), 1);
    assert_eq!(keyframes[0]["name"], "dead-anim");
    assert_eq!(keyframes[0]["path"], "src/styles.css");

    // Located cleanup candidates carry a read-only verify action so agents have
    // a machine-readable next step (parity with every other health finding).
    let kf_actions = keyframes[0]["actions"]
        .as_array()
        .expect("keyframe actions array");
    assert_eq!(kf_actions[0]["type"], "verify-unused");
    assert_eq!(kf_actions[0]["auto_fixable"], false);
    assert!(
        kf_actions[0]["command"]
            .as_str()
            .is_some_and(|c| c.contains("dead-anim")),
        "keyframe verify action should carry a read-only search for the name: {kf_actions:#?}"
    );

    // No false positives on a healthy fixture: every var() and animation-name
    // resolves to a definition, so both undefined directions are zero and the
    // located undefined list is omitted.
    assert_eq!(
        summary["custom_properties_undefined"], 0,
        "summary: {summary}"
    );
    assert_eq!(summary["keyframes_undefined"], 0, "summary: {summary}");
    assert!(
        css.get("undefined_keyframes").is_none(),
        "undefined_keyframes omitted when every animation resolves: {}",
        out.stdout
    );
}

/// Assert the styling-health confidence marker shape: a `high`/`low` enum, with
/// the prose reason present iff `low` and omitted iff `high`.
fn assert_styling_confidence_shape(styling: &serde_json::Value) {
    let confidence = styling["confidence"].as_str();
    assert!(
        matches!(confidence, Some("high" | "low")),
        "styling_health carries a confidence marker: {styling}"
    );
    if confidence == Some("low") {
        assert!(
            styling
                .get("confidence_reason")
                .and_then(|r| r.as_str())
                .is_some_and(|r| r.contains("declaration")),
            "a low-confidence grade names the declaration count: {styling}"
        );
    } else {
        assert!(
            styling.get("confidence_reason").is_none(),
            "a high-confidence grade omits the reason: {styling}"
        );
    }
}

#[test]
fn health_css_flag_surfaces_styling_health_axis() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    write_file(
        &root.join("package.json"),
        r#"{"name":"styling-axis","version":"1.0.0"}"#,
    );
    write_file(&root.join("src/index.ts"), "export const x = 1;\n");
    // A stylesheet with a dead @font-face and an unreferenced custom property:
    // the styling-health axis should dock points for the dead styling surface.
    write_file(
        &root.join("src/styles.css"),
        ":root { --brand: 4px; --unused-token: 0; }\n\
         .themed { width: var(--brand); }\n\
         @font-face { font-family: \"DeadFont\"; src: url(dead.woff2); }\n\
         .plain { color: green; }\n",
    );

    // Without --css the styling-health axis is absent (default output unchanged).
    let plain = run_fallow_in_root(
        "health",
        root,
        &[
            "--score",
            "--max-crap",
            "10000",
            "--format",
            "json",
            "--quiet",
        ],
    );
    let plain_json = parse_json(&plain);
    assert!(
        plain_json.get("styling_health").is_none(),
        "styling_health must be absent without --css: {}",
        plain.stdout
    );
    let code_score_without_css = plain_json
        .get("health_score")
        .expect("code health_score present with --score")
        .clone();

    // With --css the styling-health axis is a separate score + grade object.
    let out = run_fallow_in_root(
        "health",
        root,
        &[
            "--css",
            "--score",
            "--max-crap",
            "10000",
            "--format",
            "json",
            "--quiet",
        ],
    );
    let json = parse_json(&out);
    let styling = json
        .get("styling_health")
        .expect("styling_health present with --css");
    assert!(
        styling.get("formula_version").is_some(),
        "styling_health carries a formula_version: {styling}"
    );
    assert!(
        styling["score"]
            .as_f64()
            .is_some_and(|s| (0.0..=100.0).contains(&s)),
        "styling_health score is in [0, 100]: {styling}"
    );
    assert!(
        matches!(styling["grade"].as_str(), Some("A" | "B" | "C" | "D" | "F")),
        "styling_health carries a letter grade: {styling}"
    );
    assert!(
        styling
            .get("penalties")
            .and_then(|p| p.get("dead_surface"))
            .is_some(),
        "styling_health carries the per-category penalty breakdown: {styling}"
    );
    // The dead @font-face docks the dead-surface category, so the styling score
    // is below a clean 100.
    assert!(
        styling["score"].as_f64().is_some_and(|s| s < 100.0),
        "a dead @font-face should dock the styling score: {styling}"
    );
    assert_styling_confidence_shape(styling);

    // The CODE health_score is byte-identical with and without --css: the styling
    // axis is additive and never folds into the code score.
    let code_score_with_css = json
        .get("health_score")
        .expect("code health_score present with --score");
    assert_eq!(
        code_score_with_css, &code_score_without_css,
        "code health_score must be unchanged by the styling axis"
    );
}

/// The HUMAN render of a low-confidence styling grade: the grade is prefixed
/// with `~` and a plain-text `Low confidence:` caveat (naming the declaration
/// count) sits before the `Deductions:` line. The JSON tests assert the
/// `confidence` shape; this guards the two human-render branches in
/// `report/human/health.rs` (the `~` prefix and the caveat line), which would
/// otherwise regress silently while the JSON tests stay green.
#[test]
fn health_css_low_confidence_renders_human_caveat() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    write_file(
        &root.join("package.json"),
        r#"{"name":"sparse-css","version":"1.0.0"}"#,
    );
    write_file(&root.join("src/index.ts"), "export const x = 1;\n");
    // A handful of declarations: well below the 50-declaration confidence floor.
    write_file(
        &root.join("src/styles.css"),
        ".a { color: green; }\n.b { width: 4px; }\n",
    );
    let out = run_fallow_in_root("health", root, &["--css", "--score", "--quiet"]);
    assert!(
        out.stdout.contains("Low confidence:"),
        "low-confidence grade shows the caveat label: {}",
        out.stdout
    );
    assert!(
        out.stdout.contains("graded from only") && out.stdout.contains("declaration"),
        "the caveat names the declaration count: {}",
        out.stdout
    );
    // The grade itself is prefixed with `~` to signal an approximate sample. The
    // `~` sits on the styling line (the only `~` fallow emits in this output), so
    // assert it follows the styling label rather than just appearing somewhere.
    let styling_line = out
        .stdout
        .lines()
        .find(|l| l.contains("Styling health:"))
        .unwrap_or_else(|| panic!("a Styling health line is rendered: {}", out.stdout));
    assert!(
        styling_line.contains('~'),
        "a low-confidence grade is prefixed with ~: {styling_line}"
    );
}

#[test]
fn health_css_flags_undefined_keyframe_and_var() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    write_file(
        &root.join("package.json"),
        r#"{"name":"css-undef","version":"1.0.0"}"#,
    );
    write_file(&root.join("src/index.ts"), "export const x = 1;\n");
    // `wobble` references a @keyframes defined nowhere (undefined). `spin` is
    // defined in a DIFFERENT stylesheet, so cross-file resolution must NOT flag
    // it. `--ghost` has no definition (undefined); `--brand` is defined in b.css.
    write_file(
        &root.join("src/a.css"),
        ".x { animation-name: wobble; color: var(--ghost); }\n\
         .y { animation-name: spin; color: var(--brand); }\n",
    );
    write_file(
        &root.join("src/b.css"),
        ":root { --brand: red; }\n@keyframes spin { from {} to {} }\n",
    );

    let out = run_fallow_in_root(
        "health",
        root,
        &[
            "--css",
            "--max-crap",
            "10000",
            "--format",
            "json",
            "--quiet",
        ],
    );
    let json = parse_json(&out);
    let css = json
        .get("css_analytics")
        .expect("css_analytics present with --css");
    let summary = &css["summary"];

    // `spin` and `--brand` resolve across the two stylesheets, so only `wobble`
    // and `--ghost` are undefined (used-but-defined-nowhere).
    assert_eq!(summary["keyframes_undefined"], 1, "summary: {summary}");
    assert_eq!(
        summary["custom_properties_undefined"], 1,
        "summary: {summary}"
    );

    // The undefined @keyframes is LOCATED (name + first referencing file).
    let undefined = css["undefined_keyframes"]
        .as_array()
        .expect("undefined_keyframes located list");
    assert_eq!(undefined.len(), 1);
    assert_eq!(undefined[0]["name"], "wobble");
    assert_eq!(undefined[0]["path"], "src/a.css");
    assert!(
        !undefined.iter().any(|kf| kf["name"] == "spin"),
        "spin resolves cross-file and must not be undefined: {undefined:#?}"
    );

    // It carries a distinct verify-undefined action (a CSS-in-JS @keyframes the
    // parser cannot see is the residual non-typo case).
    let actions = undefined[0]["actions"]
        .as_array()
        .expect("undefined keyframe actions array");
    assert_eq!(actions[0]["type"], "verify-undefined");
    assert_eq!(actions[0]["auto_fixable"], false);
    assert!(
        actions[0]["command"]
            .as_str()
            .is_some_and(|c| c.contains("wobble")),
        "verify action carries a read-only token search: {actions:#?}"
    );

    // Undefined custom properties are COUNT-ONLY (per panel review): no located
    // list, because a var() with no CSS definition is dominated by JS-set tokens.
    assert!(
        css.get("undefined_custom_properties").is_none(),
        "undefined custom properties are count-only, never located"
    );
}

#[test]
fn health_css_undefined_keyframe_renders_in_human() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    write_file(
        &root.join("package.json"),
        r#"{"name":"css-undef-human","version":"1.0.0"}"#,
    );
    write_file(&root.join("src/index.ts"), "export const x = 1;\n");
    write_file(
        &root.join("src/styles.css"),
        ".x { animation-name: wobble; }\n",
    );

    let out = run_fallow_in_root("health", root, &["--css", "--max-crap", "10000", "--quiet"]);
    assert!(
        out.stdout.contains("undefined @keyframes")
            && out.stdout.contains("wobble")
            && out.stdout.contains("CSS-in-JS"),
        "human output renders the located undefined keyframe with CSS-in-JS framing: stdout={:?}",
        out.stdout
    );
}

/// Helper: run `fallow health --css --format json` and return the parsed
/// `css_analytics.unreferenced_css_classes` array (empty when absent).
fn unreferenced_classes(root: &std::path::Path) -> Vec<serde_json::Value> {
    let out = run_fallow_in_root(
        "health",
        root,
        &[
            "--css",
            "--max-crap",
            "10000",
            "--format",
            "json",
            "--quiet",
        ],
    );
    let json = parse_json(&out);
    json.get("css_analytics")
        .and_then(|c| c.get("unreferenced_css_classes"))
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default()
}

/// Names in a `css_analytics` list field for a `fallow health --css` run.
fn css_list_names(root: &std::path::Path, field: &str, name_key: &str) -> Vec<String> {
    let out = run_fallow_in_root(
        "health",
        root,
        &[
            "--css",
            "--max-crap",
            "10000",
            "--format",
            "json",
            "--quiet",
        ],
    );
    let mut names: Vec<String> = parse_json(&out)
        .get("css_analytics")
        .and_then(|c| c.get(field))
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|x| x.get(name_key).and_then(|s| s.as_str()).map(str::to_owned))
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

#[test]
fn health_css_class_typo_credits_astro_style_blocks() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    write_file(&root.join("package.json"), r#"{"name":"astro-style"}"#);
    write_file(
        &root.join("src/Page.astro"),
        "<p class=\"ssr-only\">SSR only content</p>\n<style>\n.sr-only { position: absolute; }\n.ssr-only { color: red; }\n</style>\n",
    );

    assert!(
        css_list_names(root, "unresolved_class_references", "class").is_empty(),
        "classes defined in Astro style blocks must suppress typo candidates"
    );
}

#[test]
fn health_css_class_typo_credits_sfc_scss_style_blocks() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    write_file(&root.join("package.json"), r#"{"name":"sfc-scss-style"}"#);
    write_file(
        &root.join("src/Component.svelte"),
        "<h1 class=\"svelte-scss\">Svelte Scoped Scss</h1>\n<style lang=\"scss\">\n.svelte-css { color: blue; }\n.svelte-scss { color: red; }\n</style>\n",
    );

    assert!(
        css_list_names(root, "unresolved_class_references", "class").is_empty(),
        "classes defined in SFC SCSS style blocks must suppress typo candidates"
    );
}

#[test]
fn health_css_class_typo_credits_sass_stylesheets() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    write_file(&root.join("package.json"), r#"{"name":"sass-style"}"#);
    write_file(
        &root.join("src/suggest.css"),
        ".react-scss-title { color: blue; }\n",
    );
    write_file(
        &root.join("src/styles.sass"),
        ".react-sass-title\n  color: red\n",
    );
    write_file(
        &root.join("src/App.jsx"),
        "export const App = () => <h1 className=\"react-sass-title\">Sass</h1>;\n",
    );

    assert!(
        css_list_names(root, "unresolved_class_references", "class").is_empty(),
        "classes defined in Sass stylesheets must suppress typo candidates"
    );
}

#[test]
fn health_css_keyframe_credited_by_tailwind_animate_and_js() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    write_file(&root.join("package.json"), r#"{"name":"kf"}"#);
    // `arb` applied via an `animate-[arb_...]` arbitrary value, `util` via an
    // `animate-util` named utility, `jsanim` via a JS inline-style `animation:`
    // string. Only `dead` (referenced by nothing) is flagged.
    write_file(
        &root.join("src/anim.css"),
        "@keyframes arb{from{opacity:0}to{opacity:1}}\n@keyframes util{from{}to{}}\n@keyframes jsanim{from{}to{}}\n@keyframes dead{from{}to{}}\n",
    );
    write_file(
        &root.join("src/App.tsx"),
        "export const A = () => (<div className=\"animate-[arb_0.5s_ease] animate-util\" style={{ animation: 'jsanim 1s linear' }} />);\n",
    );

    assert_eq!(
        css_list_names(root, "unreferenced_keyframes", "name"),
        vec!["dead".to_string()],
        "only the genuinely-dead keyframe is flagged"
    );
}

#[test]
fn health_css_unreferenced_class_credits_dynamic_string_and_dependency() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    write_file(
        &root.join("package.json"),
        r#"{"name":"cls","dependencies":{"maplibre-gl":"^4.0.0"}}"#,
    );
    // `.stagger-1` applied dynamically (`stagger-${i}`), `.toast-skin` via a
    // config-object string (`className: 'toast-skin'`), `.maplibregl-popup`
    // styles a third-party library applied at runtime. Only `.really-dead-class`
    // (referenced by nothing) is flagged.
    write_file(
        &root.join("src/g.css"),
        ".stagger-1{}\n.toast-skin{}\n.maplibregl-popup{}\n.really-dead-class{}\n",
    );
    write_file(
        &root.join("src/App.tsx"),
        "export const A = ({ i }: { i: number }) => { const cfg = { className: 'toast-skin' }; return <div className={`stagger-${i}`} data-cfg={cfg.className} />; };\n",
    );

    assert_eq!(
        css_list_names(root, "unreferenced_css_classes", "class"),
        vec!["really-dead-class".to_string()],
        "dynamic / string-literal / third-party classes are credited"
    );
}

#[test]
fn health_css_unreferenced_class_credits_markdown_class_attributes() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    write_file(&root.join("package.json"), r#"{"name":"slidev-css"}"#);
    write_file(
        &root.join("style.css"),
        ".cover-sub{}\n.prompt-card{}\n.really-dead-class{}\n",
    );
    write_file(
        &root.join("slides.md"),
        r#"<p class="cover-sub">vibe coding to validate</p>

<div class="prompt-card">
  <p>Prompt</p>
</div>
"#,
    );

    assert_eq!(
        css_list_names(root, "unreferenced_css_classes", "class"),
        vec!["really-dead-class".to_string()],
        "Markdown and Slidev class attributes must credit CSS reachability"
    );
}

#[test]
fn health_css_unreferenced_class_credits_dynamic_status_literals() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    write_file(&root.join("package.json"), r#"{"name":"dynamic-status"}"#);
    write_file(
        &root.join("src/app.css"),
        ".realtime-refresh.stale{}\n.realtime-refresh.unavailable{}\n.helper-only{}\n.really-dead-class{}\n",
    );
    write_file(
        &root.join("src/status.ts"),
        "export type Status = 'connected' | 'stale' | 'unavailable';\nexport const helper = 'helper-only';\nexport const label = (status: Status) => status === 'stale' ? 'Stale' : 'Ready';\n",
    );
    write_file(
        &root.join("src/App.tsx"),
        "import type { Status } from './status';\nexport const A = ({ status }: { status: Status }) => <div className={`live-refresh realtime-refresh ${status}`}>x</div>;\n",
    );

    assert_eq!(
        css_list_names(root, "unreferenced_css_classes", "class"),
        vec!["helper-only".to_string(), "really-dead-class".to_string()],
        "status literal classes interpolated through className must be credited"
    );
}

#[test]
fn health_css_font_face_credited_by_custom_property_value() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    write_file(&root.join("package.json"), r#"{"name":"ff"}"#);
    // `LiveFont` is referenced only via a `--font-display` custom property inside
    // a `@theme` block (which lightningcss skips); `GhostFont` is referenced by
    // nothing. Only `GhostFont` is flagged. The fonts' own `@font-face` blocks are
    // masked so they do not self-credit.
    write_file(
        &root.join("src/fonts.css"),
        "@font-face{font-family:\"LiveFont\";src:url(/l.woff2)}\n@font-face{font-family:\"GhostFont\";src:url(/g.woff2)}\n@theme{--font-display:\"LiveFont\",sans-serif}\n.x{font-family:var(--font-display)}\n",
    );
    write_file(&root.join("src/App.tsx"), "export const A = () => null;\n");

    assert_eq!(
        css_list_names(root, "unused_font_faces", "family"),
        vec!["GhostFont".to_string()],
        "a font referenced via a --font-* custom property is not flagged"
    );
}

#[test]
fn health_css_global_override_class_not_unreferenced_candidate() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    write_file(
        &root.join("package.json"),
        r#"{"name":"mod","dependencies":{"antd":"^5.0.0"}}"#,
    );
    // A CSS Module that styles an antd modal: `.dialog` is the local class (applied
    // via `styles.dialog`), `:global(.ant-modal-header)` is antd's runtime DOM the
    // module overrides, and `.dead-local` is a genuinely-unused local class. Only
    // `dead-local` is flagged: the `:global(...)` override is never an unreferenced
    // candidate (the project markup never authors it; antd applies it at runtime),
    // even though `antd` normalizes too short for the dependency-prefix abstain.
    write_file(
        &root.join("src/Dialog.module.css"),
        ".dialog :global(.ant-modal-header) { color: red; }\n.dialog { padding: 1rem; }\n.dead-local { display: none; }\n",
    );
    write_file(
        &root.join("src/Dialog.tsx"),
        "import styles from './Dialog.module.css';\nexport const D = () => <div className={styles.dialog} />;\n",
    );

    assert_eq!(
        css_list_names(root, "unreferenced_css_classes", "class"),
        vec!["dead-local".to_string()],
        "a :global(...) override is not an unreferenced-class candidate"
    );
}

#[test]
fn health_css_module_camel_case_property_reference_credits_class() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    write_file(
        &root.join("package.json"),
        r#"{"name":"mod-camel","version":"1.0.0"}"#,
    );
    write_file(
        &root.join("src/Card.module.css"),
        ".dialog-panel { padding: 1rem; }\n.dead-local { display: none; }\n",
    );
    write_file(
        &root.join("src/Card.tsx"),
        "import styles from './Card.module.css';\nexport const Card = () => <section className={styles.dialogPanel} />;\n",
    );

    assert_eq!(
        css_list_names(root, "unreferenced_css_classes", "class"),
        vec!["dead-local".to_string()],
        "CSS Modules camelCase property references must credit dashed classes"
    );
}

#[test]
fn health_css_unreferenced_classes_include_sass_and_less_when_not_dominant() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    write_file(
        &root.join("package.json"),
        r#"{"name":"sass-less-classes","version":"1.0.0"}"#,
    );
    write_file(&root.join("src/base.css"), ".base-used { color: black; }\n");
    write_file(
        &root.join("src/extra.css"),
        ".extra-used { color: gray; }\n",
    );
    write_file(
        &root.join("src/theme.less"),
        ".used-less { color: red; }\n.dead-less { color: blue; }\n",
    );
    write_file(
        &root.join("src/theme.sass"),
        ".used-sass\n  color: red\n.dead-sass\n  color: blue\n",
    );
    write_file(
        &root.join("src/App.tsx"),
        "export const App = () => <div className=\"base-used extra-used used-less used-sass\" />;\n",
    );

    assert_eq!(
        css_list_names(root, "unreferenced_css_classes", "class"),
        vec!["dead-less".to_string(), "dead-sass".to_string()],
        "Sass and Less stylesheet classes should join the located unreferenced-class pass"
    );
}

#[test]
fn health_css_flags_unreferenced_global_class() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    write_file(
        &root.join("package.json"),
        r#"{"name":"unref","version":"1.0.0"}"#,
    );
    // The sheet is locally consumed (3 of 4 classes used in markup), so it is not
    // a published surface; `.legacy-promo-banner` is referenced nowhere.
    write_file(
        &root.join("src/app.css"),
        ".app-header { color: red; }\n.profile-card { padding: 1rem; }\n.nav-link { color: blue; }\n.legacy-promo-banner { display: none; }\n",
    );
    write_file(
        &root.join("src/App.jsx"),
        "export const C = () => (<div className=\"app-header\"><span className=\"profile-card\"><a className=\"nav-link\">x</a></span></div>);\n",
    );

    let refs = unreferenced_classes(root);
    assert_eq!(refs.len(), 1, "only the dead class is flagged: {refs:#?}");
    assert_eq!(refs[0]["class"], "legacy-promo-banner");
    assert_eq!(refs[0]["path"], "src/app.css");
    assert_eq!(refs[0]["line"], 4);
    let action = &refs[0]["actions"][0];
    assert_eq!(action["type"], "verify-unused");
    assert!(
        action["description"]
            .as_str()
            .is_some_and(|d| d.contains("CMS") && d.contains("server")),
        "verify action names the unscanned surfaces: {action:#?}"
    );
}

#[test]
fn health_css_unreferenced_abstains_on_preprocessor_dominant() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    write_file(&root.join("package.json"), r#"{"name":"scssheavy"}"#);
    write_file(&root.join("src/app.css"), ".used { color: red; }\n");
    write_file(&root.join("src/a.scss"), ".dead-banner { color: blue; }\n");
    write_file(&root.join("src/b.scss"), ".other-dead { color: green; }\n");
    write_file(
        &root.join("src/App.jsx"),
        "export const C = () => <div className=\"used\">x</div>;\n",
    );
    // 2 scss vs 1 css -> preprocessor-dominant -> abstain entirely.
    let css = css_analytics(root);
    assert!(unreferenced_classes(root).is_empty());
    assert_eq!(css["summary"]["preprocessor_stylesheets"].as_u64(), Some(2));
    assert_eq!(
        css["summary"]["preprocessor_reachability_abstained"].as_bool(),
        Some(true)
    );
    let out = run_fallow_in_root("health", root, &["--css", "--max-crap", "10000", "--quiet"]);
    assert!(
        out.stdout.contains("Sass/Less reachability skipped"),
        "human CSS output should explain preprocessor abstain: {}",
        out.stdout
    );
}

#[test]
fn health_css_unreferenced_abstains_published_and_dynamic() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    // `dist/lib.css` is a published entry (package.json `style`), so its classes
    // are consumed externally and must not be flagged.
    write_file(
        &root.join("package.json"),
        r#"{"name":"pub","style":"dist/lib.css"}"#,
    );
    write_file(
        &root.join("dist/lib.css"),
        ".lib-button { color: red; }\n.lib-unused-public { color: blue; }\n",
    );
    // `app.css` is locally consumed; `.feature-modal` is only ever assembled
    // dynamically (substring of a clsx call), so it must NOT be flagged.
    write_file(
        &root.join("src/app.css"),
        ".sidebar { color: red; }\n.feature-modal { color: blue; }\n",
    );
    write_file(
        &root.join("src/App.jsx"),
        "export const C = ({on}) => <div className={clsx(\"sidebar\", on && \"feature-modal\")}>x</div>;\n",
    );
    let refs = unreferenced_classes(root);
    assert!(
        !refs.iter().any(|r| r["class"] == "lib-unused-public"),
        "published-surface class must not be flagged: {refs:#?}"
    );
    assert!(
        !refs.iter().any(|r| r["class"] == "feature-modal"),
        "dynamically-assembled class (substring of clsx arg) must not be flagged: {refs:#?}"
    );
}

#[test]
fn health_css_unreferenced_renders_in_human() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    write_file(&root.join("package.json"), r#"{"name":"unrefhuman"}"#);
    write_file(
        &root.join("src/app.css"),
        ".header-bar { color: red; }\n.orphaned-widget { display: none; }\n",
    );
    write_file(
        &root.join("src/App.jsx"),
        "export const C = () => <div className=\"header-bar\">x</div>;\n",
    );
    let out = run_fallow_in_root("health", root, &["--css", "--max-crap", "10000", "--quiet"]);
    assert!(
        out.stdout.contains("referenced by no in-project markup")
            && out.stdout.contains("orphaned-widget")
            && out.stdout.contains("CMS"),
        "human output renders the unreferenced class with the unscanned-surface disclosure: stdout={:?}",
        out.stdout
    );
}

#[test]
fn health_css_flags_unused_font_face() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    write_file(&root.join("package.json"), r#"{"name":"deadfont"}"#);
    // `DeadFont` is declared + downloaded but applied by nothing; `LiveFont` is
    // declared AND applied via `.title`.
    write_file(
        &root.join("src/fonts.css"),
        "@font-face { font-family: \"DeadFont\"; src: url(./dead.woff2); }\n@font-face { font-family: \"LiveFont\"; src: url(./live.woff2); }\n.title { font-family: LiveFont, sans-serif; }\n",
    );
    write_file(
        &root.join("src/App.jsx"),
        "export const C = () => <h1 className=\"title\">x</h1>;\n",
    );

    let out = run_fallow_in_root(
        "health",
        root,
        &[
            "--css",
            "--max-crap",
            "10000",
            "--format",
            "json",
            "--quiet",
        ],
    );
    let json = parse_json(&out);
    let css = json
        .get("css_analytics")
        .expect("css_analytics present with --css");
    assert_eq!(css["summary"]["unused_font_faces"], 1);
    let ff = css["unused_font_faces"]
        .as_array()
        .expect("unused_font_faces located list");
    assert_eq!(ff.len(), 1, "only the dead font is flagged: {ff:#?}");
    assert_eq!(ff[0]["family"], "DeadFont");
    assert_eq!(ff[0]["path"], "src/fonts.css");
    assert!(
        !ff.iter().any(|f| f["family"] == "LiveFont"),
        "an applied @font-face must not be flagged: {ff:#?}"
    );
    assert_eq!(ff[0]["actions"][0]["type"], "verify-unused");
}

#[test]
fn health_css_unused_font_face_abstains_when_used_outside_css() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    write_file(&root.join("package.json"), r#"{"name":"jsfont"}"#);
    // `CanvasFont` is declared in CSS but applied only from JavaScript (a canvas
    // `fontFamily` assignment), which the CSS-only scan cannot see. The source
    // substring check must keep it out of the dead set.
    write_file(
        &root.join("src/fonts.css"),
        "@font-face { font-family: \"CanvasFont\"; src: url(./c.woff2); }\n.x { color: red; }\n",
    );
    write_file(
        &root.join("src/canvas.ts"),
        "export const setup = (ctx: CanvasRenderingContext2D) => { ctx.font = '16px CanvasFont'; };\n",
    );
    write_file(&root.join("src/App.jsx"), "export const C = () => null;\n");

    let css = parse_json(&run_fallow_in_root(
        "health",
        root,
        &[
            "--css",
            "--max-crap",
            "10000",
            "--format",
            "json",
            "--quiet",
        ],
    ));
    let count = css
        .get("css_analytics")
        .and_then(|c| c.get("summary"))
        .and_then(|s| s.get("unused_font_faces"))
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0);
    assert_eq!(
        count, 0,
        "a font applied from JS must not be flagged: {css}"
    );
}

#[test]
fn health_css_unused_font_face_matches_family_case_insensitively() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    write_file(&root.join("package.json"), r#"{"name":"casefont"}"#);
    // CSS font-family names are case-insensitive: `BrandFont` declared with one
    // casing and applied with another (`brandfont`) is LIVE and must not flag.
    // The declared casing is preserved for display, so only the genuinely-dead
    // `GhostFont` (no reference at any casing) is reported.
    write_file(
        &root.join("src/fonts.css"),
        "@font-face { font-family: \"BrandFont\"; src: url(./b.woff2); }\n@font-face { font-family: \"GhostFont\"; src: url(./g.woff2); }\n.title { font-family: brandfont, sans-serif; }\n",
    );
    write_file(
        &root.join("src/App.jsx"),
        "export const C = () => <h1 className=\"title\">x</h1>;\n",
    );

    let css = parse_json(&run_fallow_in_root(
        "health",
        root,
        &[
            "--css",
            "--max-crap",
            "10000",
            "--format",
            "json",
            "--quiet",
        ],
    ));
    let analytics = css
        .get("css_analytics")
        .expect("css_analytics present with --css");
    assert_eq!(analytics["summary"]["unused_font_faces"], 1);
    let ff = analytics["unused_font_faces"]
        .as_array()
        .expect("unused_font_faces located list");
    assert_eq!(ff.len(), 1, "only the truly dead font is flagged: {ff:#?}");
    assert_eq!(ff[0]["family"], "GhostFont");
    assert!(
        !ff.iter().any(|f| f["family"] == "BrandFont"),
        "a font applied with different casing must not be flagged: {ff:#?}"
    );
}

/// Run `fallow health --css` and return the `css_analytics` node (or `Null`).
fn css_analytics(root: &std::path::Path) -> serde_json::Value {
    let out = run_fallow_in_root(
        "health",
        root,
        &[
            "--css",
            "--max-crap",
            "10000",
            "--format",
            "json",
            "--quiet",
        ],
    );
    parse_json(&out)
        .get("css_analytics")
        .cloned()
        .unwrap_or(serde_json::Value::Null)
}

/// The flagged `unused_theme_tokens` token strings, sorted.
fn flagged_theme_tokens(css: &serde_json::Value) -> Vec<String> {
    css.get("unused_theme_tokens")
        .and_then(serde_json::Value::as_array)
        .map(|a| {
            let mut v: Vec<String> = a
                .iter()
                .filter_map(|t| t["token"].as_str().map(str::to_owned))
                .collect();
            v.sort();
            v
        })
        .unwrap_or_default()
}

/// A v4 `package.json` declaring the `tailwindcss` dependency (the v4 gate).
const TW_PKG: &str = r#"{"name":"twtheme","devDependencies":{"tailwindcss":"^4.0.0"}}"#;

#[test]
fn health_css_flags_unused_theme_token() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    write_file(&root.join("package.json"), TW_PKG);
    // `--color-brand` generates `bg-brand` (used); `--shadow-glow` generates
    // `shadow-glow` (used by nothing): only the latter is a dead design token.
    write_file(
        &root.join("src/theme.css"),
        "@theme {\n  --color-brand: #f05a28;\n  --shadow-glow: 0 0 8px red;\n}\n",
    );
    write_file(
        &root.join("src/App.tsx"),
        "export const C = () => <div className=\"bg-brand p-4\" />;\n",
    );

    let css = css_analytics(root);
    assert_eq!(css["summary"]["unused_theme_tokens"], 1, "{css:#?}");
    let tokens = css["unused_theme_tokens"]
        .as_array()
        .expect("unused_theme_tokens located list");
    assert_eq!(
        tokens.len(),
        1,
        "only the dead token is flagged: {tokens:#?}"
    );
    assert_eq!(tokens[0]["token"], "--shadow-glow");
    assert_eq!(tokens[0]["namespace"], "shadow");
    assert_eq!(tokens[0]["path"], "src/theme.css");
    assert_eq!(tokens[0]["line"], 3);
    assert_eq!(tokens[0]["actions"][0]["type"], "verify-unused");
    assert_eq!(tokens[0]["actions"][0]["auto_fixable"], false);
    // The verify command embeds the LITERAL qualified search terms.
    let command = tokens[0]["actions"][0]["command"]
        .as_str()
        .expect("verify command");
    assert!(command.contains("-glow"), "command: {command}");
    assert!(command.contains("--shadow-glow"), "command: {command}");
}

#[test]
fn health_css_theme_token_credited_by_apply_and_var() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    write_file(&root.join("package.json"), TW_PKG);
    // `--radius-card` is applied only via `@apply rounded-card` in plain CSS;
    // `--color-brand` is read only via `var()`, INCLUDING by another `@theme`
    // token (`--color-button` backs onto it). Both must be credited. `--font-x`
    // is genuinely dead.
    write_file(
        &root.join("src/theme.css"),
        "@theme {\n  --radius-card: 12px;\n  --color-brand: #f05a28;\n  --color-button: var(--color-brand);\n  --font-x: \"X\";\n}\n.panel { @apply rounded-card; }\n.btn { background: var(--color-button); }\n",
    );
    write_file(&root.join("src/App.tsx"), "export const C = () => null;\n");

    let css = css_analytics(root);
    assert_eq!(
        flagged_theme_tokens(&css),
        vec!["--font-x".to_string()],
        "@apply, var(), and token-backs-token must all credit usage: {css:#?}"
    );
}

#[test]
fn health_css_theme_token_credited_by_arbitrary_and_qualified_js() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    write_file(&root.join("package.json"), TW_PKG);
    // `--radius-pill` used only via an arbitrary value `rounded-[--radius-pill]`;
    // `--color-ring` used only via a qualified JS reference (`bg-ring` in a clsx
    // string). `--text-ghost` is genuinely dead.
    write_file(
        &root.join("src/theme.css"),
        "@theme {\n  --radius-pill: 9999px;\n  --color-ring: #00f;\n  --text-ghost: 8px;\n}\n",
    );
    write_file(
        &root.join("src/App.tsx"),
        "import clsx from 'clsx';\nexport const C = () => (<div className={clsx('bg-ring')}><span className=\"rounded-[--radius-pill]\" /></div>);\n",
    );

    let css = css_analytics(root);
    assert_eq!(
        flagged_theme_tokens(&css),
        vec!["--text-ghost".to_string()],
        "arbitrary value and qualified JS usage must credit: {css:#?}"
    );
}

#[test]
fn health_css_theme_token_default_override_not_flagged() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    write_file(&root.join("package.json"), TW_PKG);
    // Overriding a default shade is credited the moment its utility appears.
    write_file(
        &root.join("src/theme.css"),
        "@theme {\n  --color-red-500: #e00;\n}\n",
    );
    write_file(
        &root.join("src/App.tsx"),
        "export const C = () => <p className=\"text-red-500\" />;\n",
    );

    let css = css_analytics(root);
    assert!(
        flagged_theme_tokens(&css).is_empty(),
        "a default override used in markup must not be flagged: {css:#?}"
    );
}

#[test]
fn health_css_theme_token_excludes_bare_default_and_reset() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    write_file(&root.join("package.json"), TW_PKG);
    // Bare `--spacing` (multiplier base), `--default-*` (generates no utility),
    // and the `--color-*: initial` reset are NEVER candidates. `--blur-soft` is
    // a genuine dead token, present so the report is emitted.
    write_file(
        &root.join("src/theme.css"),
        "@theme {\n  --spacing: 0.25rem;\n  --default-transition-duration: 150ms;\n  --color-*: initial;\n  --blur-soft: 4px;\n}\n",
    );
    write_file(&root.join("src/App.tsx"), "export const C = () => null;\n");

    let css = css_analytics(root);
    assert_eq!(
        flagged_theme_tokens(&css),
        vec!["--blur-soft".to_string()],
        "bare / default / reset forms must never be candidates: {css:#?}"
    );
}

#[test]
fn health_css_theme_token_dictionary_word_not_credited_by_bare_word() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    write_file(&root.join("package.json"), TW_PKG);
    // The make-or-break: a token named `brand` / `card` must NOT be credited
    // merely because the WORD appears in source (branding, discard, cardboard).
    // Only a real `-<name>` utility suffix credits it.
    write_file(
        &root.join("src/theme.css"),
        "@theme {\n  --color-brand: #f05a28;\n  --radius-card: 8px;\n}\n",
    );
    write_file(
        &root.join("src/App.tsx"),
        "// brand card branding discard cardboard rebrand\nexport const brand = 'brand';\nconst card = { cardboard: true };\nexport const cls = 'flex p-4 unrelated-thing';\n",
    );

    let css = css_analytics(root);
    assert_eq!(
        flagged_theme_tokens(&css),
        vec!["--color-brand".to_string(), "--radius-card".to_string()],
        "bare dictionary words must NOT credit a theme token: {css:#?}"
    );
}

#[test]
fn health_css_theme_token_abstains_on_plugin_published_and_nontailwind() {
    // (a) @plugin directive -> abstain.
    let dir = tempdir().unwrap();
    let root = dir.path();
    write_file(&root.join("package.json"), TW_PKG);
    write_file(
        &root.join("src/theme.css"),
        "@plugin \"daisyui\";\n@theme {\n  --color-dead: #000;\n}\n",
    );
    write_file(&root.join("src/App.tsx"), "export const C = () => null;\n");
    assert!(
        flagged_theme_tokens(&css_analytics(root)).is_empty(),
        "a @plugin project must abstain"
    );

    // (b) the @theme stylesheet is a published package surface -> abstain.
    let dir = tempdir().unwrap();
    let root = dir.path();
    write_file(
        &root.join("package.json"),
        r#"{"name":"lib","devDependencies":{"tailwindcss":"^4.0.0"},"exports":{"./styles":"./src/theme.css"}}"#,
    );
    write_file(
        &root.join("src/theme.css"),
        "@theme {\n  --color-dead: #000;\n}\n",
    );
    write_file(&root.join("src/App.tsx"), "export const C = () => null;\n");
    assert!(
        flagged_theme_tokens(&css_analytics(root)).is_empty(),
        "a published-library @theme must abstain"
    );

    // (c) no tailwindcss dependency -> abstain.
    let dir = tempdir().unwrap();
    let root = dir.path();
    write_file(&root.join("package.json"), r#"{"name":"plain"}"#);
    write_file(
        &root.join("src/theme.css"),
        "@theme {\n  --color-dead: #000;\n}\n",
    );
    write_file(&root.join("src/App.tsx"), "export const C = () => null;\n");
    assert!(
        flagged_theme_tokens(&css_analytics(root)).is_empty(),
        "a non-Tailwind project must abstain"
    );
}

#[test]
fn health_css_theme_token_property_modifier_not_flagged() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    write_file(&root.join("package.json"), TW_PKG);
    // Tailwind v4 `--<token>--<property>` modifier configures an option on a
    // token (here `font-feature-settings` on `font-sans`); it generates no
    // standalone utility, so it must NOT be flagged (real-world smoke FP on the
    // Tailwind docs site). `--shadow-dead` is a genuine dead token so the report
    // is still emitted.
    write_file(
        &root.join("src/theme.css"),
        "@theme {\n  --font-sans: \"Inter\", sans-serif;\n  --font-sans--font-feature-settings: \"cv02\", \"cv03\";\n  --shadow-dead: 0 0 1px red;\n}\n",
    );
    write_file(
        &root.join("src/App.tsx"),
        "export const C = () => <p className=\"font-sans\" />;\n",
    );

    let css = css_analytics(root);
    assert_eq!(
        flagged_theme_tokens(&css),
        vec!["--shadow-dead".to_string()],
        "a token-property modifier must never be flagged: {css:#?}"
    );
}

#[test]
fn health_css_unused_theme_token_renders_in_human() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    write_file(&root.join("package.json"), TW_PKG);
    write_file(
        &root.join("src/theme.css"),
        "@theme {\n  --shadow-glow: 0 0 8px red;\n}\n",
    );
    write_file(&root.join("src/App.tsx"), "export const C = () => null;\n");

    let out = run_fallow_in_root("health", root, &["--css", "--max-crap", "10000", "--quiet"]);
    assert!(
        out.stdout.contains("@theme token") && out.stdout.contains("--shadow-glow"),
        "human output should list the unused @theme token: {}",
        out.stdout
    );
}

#[test]
fn health_css_flags_font_size_unit_mix_above_floor() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    write_file(&root.join("package.json"), r#"{"name":"unitmix"}"#);
    // A type scale split across px and rem, above the floor (>= 6 distinct sizes,
    // 2 units): the unit-mix candidate fires with a per-unit breakdown.
    write_file(
        &root.join("src/type.css"),
        ".a{font-size:12px}.b{font-size:14px}.c{font-size:16px}.d{font-size:1rem}.e{font-size:1.25rem}.f{font-size:1.5rem}\n",
    );
    write_file(&root.join("src/App.jsx"), "export const C = () => null;\n");

    let css = parse_json(&run_fallow_in_root(
        "health",
        root,
        &[
            "--css",
            "--max-crap",
            "10000",
            "--format",
            "json",
            "--quiet",
        ],
    ));
    let analytics = css
        .get("css_analytics")
        .expect("css_analytics present with --css");
    assert_eq!(analytics["summary"]["font_size_units_used"], 2);
    let mix = analytics
        .get("font_size_unit_mix")
        .expect("font_size_unit_mix candidate present above the floor");
    let notations = mix["notations"].as_array().expect("notations array");
    assert_eq!(notations.len(), 2, "px + rem: {mix:#?}");
    let units: Vec<&str> = notations
        .iter()
        .filter_map(|n| n["notation"].as_str())
        .collect();
    assert!(units.contains(&"px") && units.contains(&"rem"), "{units:?}");
    assert_eq!(mix["actions"][0]["type"], "standardize");
}

#[test]
fn health_css_font_size_unit_mix_abstains_below_floor() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    write_file(&root.join("package.json"), r#"{"name":"smallscale"}"#);
    // Two units but only three distinct sizes: below the floor, so no candidate
    // (a tiny stylesheet is not yet a type scale).
    write_file(
        &root.join("src/type.css"),
        ".a{font-size:12px}.b{font-size:1rem}.c{font-size:1.5rem}\n",
    );
    write_file(&root.join("src/App.jsx"), "export const C = () => null;\n");

    let css = parse_json(&run_fallow_in_root(
        "health",
        root,
        &[
            "--css",
            "--max-crap",
            "10000",
            "--format",
            "json",
            "--quiet",
        ],
    ));
    let analytics = css
        .get("css_analytics")
        .expect("css_analytics present with --css");
    assert!(
        analytics.get("font_size_unit_mix").is_none(),
        "below floor: no unit-mix candidate: {analytics}"
    );
}

#[test]
fn health_css_flags_unresolved_class_typo() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    write_file(
        &root.join("package.json"),
        r#"{"name":"css-class-typo","version":"1.0.0"}"#,
    );
    // `card-title` and `btn-primary` are authored CSS classes.
    write_file(
        &root.join("src/styles.css"),
        ".card-title { color: red; }\n.btn-primary { color: blue; }\n",
    );
    // `card-tite` is a one-edit typo of `card-title` (flag + suggest).
    // `btn-primary` matches a definition (NOT flagged).
    // `flex` is a Tailwind utility, not one edit from any class (NOT flagged).
    // `xy` is too short to typo-check (NOT flagged).
    write_file(
        &root.join("src/App.jsx"),
        "export const C = () => (\n  <div className=\"card-tite flex\">\n    <span className=\"btn-primary xy\">ok</span>\n  </div>\n);\n",
    );

    let out = run_fallow_in_root(
        "health",
        root,
        &[
            "--css",
            "--max-crap",
            "10000",
            "--format",
            "json",
            "--quiet",
        ],
    );
    let json = parse_json(&out);
    let css = json
        .get("css_analytics")
        .expect("css_analytics present with --css");
    assert_eq!(css["summary"]["unresolved_class_references"], 1);

    let refs = css["unresolved_class_references"]
        .as_array()
        .expect("unresolved_class_references located list");
    assert_eq!(refs.len(), 1, "only the typo is flagged: {refs:#?}");
    assert_eq!(refs[0]["class"], "card-tite");
    assert_eq!(refs[0]["suggestion"], "card-title");
    assert_eq!(refs[0]["path"], "src/App.jsx");
    assert!(
        !refs.iter().any(|r| r["class"] == "btn-primary"),
        "a correctly-spelled class must not be flagged: {refs:#?}"
    );
    assert!(
        !refs
            .iter()
            .any(|r| r["class"] == "flex" || r["class"] == "xy"),
        "Tailwind utilities and short tokens must not be flagged: {refs:#?}"
    );

    let actions = refs[0]["actions"]
        .as_array()
        .expect("unresolved class actions array");
    assert_eq!(actions[0]["type"], "verify-undefined");
    assert_eq!(actions[0]["auto_fixable"], false);
    assert!(
        actions[0]["command"]
            .as_str()
            .is_some_and(|c| c.contains("card-tite")),
        "verify action carries a read-only token search: {actions:#?}"
    );
}

#[test]
fn health_css_no_unresolved_class_without_authored_css() {
    // A project with no authored CSS classes (Tailwind-only) emits no typo
    // candidates: with an empty target set every token would look unresolved,
    // so the feature abstains entirely.
    let dir = tempdir().unwrap();
    let root = dir.path();
    write_file(
        &root.join("package.json"),
        r#"{"name":"tw-only","version":"1.0.0","dependencies":{"tailwindcss":"4"}}"#,
    );
    write_file(
        &root.join("src/App.jsx"),
        "export const C = () => <div className=\"flex items-center gap-4\">x</div>;\n",
    );

    let out = run_fallow_in_root(
        "health",
        root,
        &[
            "--css",
            "--max-crap",
            "10000",
            "--format",
            "json",
            "--quiet",
        ],
    );
    let json = parse_json(&out);
    if let Some(css) = json.get("css_analytics") {
        assert_eq!(
            css["summary"]["unresolved_class_references"], 0,
            "no authored CSS -> no typo candidates: {css}"
        );
        assert!(
            css.get("unresolved_class_references").is_none()
                || css["unresolved_class_references"]
                    .as_array()
                    .is_some_and(std::vec::Vec::is_empty),
            "no authored CSS -> empty list"
        );
    }
}

#[test]
fn health_css_unresolved_abstains_on_preprocessor_dominant() {
    // When .scss/.sass/.less files outnumber plain .css, the parser cannot
    // expand preprocessor loops/mixins, so the defined-class set is unreliable
    // (a generated class looks unresolved). The feature abstains entirely, even
    // when a token would otherwise be a near-miss. Caught by real-world smoke on
    // Bootstrap (a SCSS framework), where the bare near-miss produced 117 FPs.
    let dir = tempdir().unwrap();
    let root = dir.path();
    write_file(
        &root.join("package.json"),
        r#"{"name":"scss-heavy","version":"1.0.0"}"#,
    );
    write_file(&root.join("src/a.css"), ".sidebar-nav { color: red; }\n");
    write_file(
        &root.join("src/b.scss"),
        "$x: 1;\n.thing { color: blue; }\n",
    );
    write_file(
        &root.join("src/c.scss"),
        "$y: 2;\n.other { color: green; }\n",
    );
    // `sidebar-nev` is one edit from the defined `.sidebar-nav`, but the project
    // is preprocessor-dominant (2 scss vs 1 css), so nothing is flagged.
    write_file(
        &root.join("src/App.jsx"),
        "export const C = () => <nav className=\"sidebar-nev\">x</nav>;\n",
    );

    let out = run_fallow_in_root(
        "health",
        root,
        &[
            "--css",
            "--max-crap",
            "10000",
            "--format",
            "json",
            "--quiet",
        ],
    );
    let json = parse_json(&out);
    if let Some(css) = json.get("css_analytics") {
        assert_eq!(
            css["summary"]["unresolved_class_references"], 0,
            "preprocessor-dominant project must abstain: {css}"
        );
    }
}

#[test]
fn health_css_unreferenced_class_not_credited_by_custom_property_substring() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    write_file(
        &root.join("package.json"),
        r#"{"name":"css-custom-property-substring","dependencies":{"tailwindcss":"^4.0.0"}}"#,
    );
    write_file(
        &root.join("src/app.css"),
        r#"@import "tailwindcss";
:root {
  --btn-primary-bg: var(--color-teal-900);
  --btn-primary-shadow: var(--color-teal-900);
}

@layer components {
  .btn-primary {
    @apply bg-teal-900 shadow-[var(--btn-primary-shadow)]/20;
  }

  .btn-primary-bg {
    @apply bg-teal-800;
  }

  .btn-secondary {
    @apply bg-gold-400;
  }

  .btn-ghost {
    @apply bg-white/60;
  }

  .used-card {
    @apply rounded-xl;
  }
}
"#,
    );
    write_file(
        &root.join("src/Button.tsx"),
        r#"const cn = (...parts: Array<string | false>) => parts.filter(Boolean).join(" ");

export const Button = () => (
  <button className={cn("used-card", "bg-[var(--btn-primary-bg)]", "shadow-[var(--btn-primary-shadow)]/20")}>
    Pay
  </button>
);
"#,
    );

    assert_eq!(
        css_list_names(root, "unreferenced_css_classes", "class"),
        vec![
            "btn-ghost".to_string(),
            "btn-primary".to_string(),
            "btn-primary-bg".to_string(),
            "btn-secondary".to_string()
        ],
        "custom property names inside arbitrary values must not credit similarly named CSS classes"
    );
}

#[test]
fn health_css_important_reset_findings_are_verify_first() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    write_file(
        &root.join("package.json"),
        r#"{"name":"css-important-reset","dependencies":{"tailwindcss":"^4.0.0"}}"#,
    );
    write_file(
        &root.join("src/app.css"),
        r#"@import "tailwindcss";

@media (prefers-reduced-motion: reduce) {
  *,
  *::before,
  *::after {
    animation-duration: 0.01ms !important;
    animation-iteration-count: 1 !important;
    transition-duration: 0.01ms !important;
    scroll-behavior: auto !important;
  }
}
"#,
    );
    write_file(
        &root.join("src/App.tsx"),
        "export const App = () => null;\n",
    );

    let out = run_fallow_in_root(
        "health",
        root,
        &[
            "--css",
            "--report-only",
            "--format",
            "json",
            "--quiet",
            "--no-cache",
        ],
    );
    let json = parse_json(&out);
    let findings = json["styling_findings"]
        .as_array()
        .expect("styling findings");
    assert!(
        findings.iter().any(|finding| {
            finding["code"] == "css-selector-complexity"
                && finding["sub_kind"] == "important-density"
                && finding["confidence"] == "low"
                && finding["agent_disposition"] == "verify-first"
        }),
        "accessibility reset important usage should be verify-first: {findings:#?}"
    );
}

#[test]
fn health_css_third_party_important_overrides_are_verify_first() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    write_file(
        &root.join("package.json"),
        r#"{"name":"css-third-party-important","dependencies":{"tailwindcss":"^4.0.0"}}"#,
    );
    write_file(
        &root.join("src/app.css"),
        r#"@import "tailwindcss";

[data-sonner-toast].app-toast {
  background: rgb(255 255 255 / 0.6) !important;
  border: 1px solid rgb(231 229 228) !important;
  font-family: var(--font-sans) !important;
}
"#,
    );
    write_file(
        &root.join("src/App.tsx"),
        "export const App = () => null;\n",
    );

    let out = run_fallow_in_root(
        "health",
        root,
        &[
            "--css",
            "--report-only",
            "--format",
            "json",
            "--quiet",
            "--no-cache",
        ],
    );
    let json = parse_json(&out);
    let findings = json["styling_findings"]
        .as_array()
        .expect("styling findings");
    assert!(
        findings.iter().any(|finding| {
            finding["code"] == "css-selector-complexity"
                && finding["sub_kind"] == "important-density"
                && finding["confidence"] == "low"
                && finding["agent_disposition"] == "verify-first"
                && finding["fix_hint"]
                    .as_str()
                    .is_some_and(|hint| hint.contains("cleanup is not proven"))
        }),
        "third-party widget important usage should be verify-first: {findings:#?}"
    );
}

#[test]
fn health_css_keeps_tokenless_raw_values_out_of_styling_findings() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    write_file(
        &root.join("package.json"),
        r#"{"name":"css-tokenless-raw-style","version":"1.0.0"}"#,
    );
    write_file(
        &root.join("src/app.css"),
        ".card { color: #123456; font-size: 17px; }\n",
    );
    write_file(
        &root.join("src/App.tsx"),
        "export const App = () => null;\n",
    );

    let out = run_fallow_in_root(
        "health",
        root,
        &[
            "--css",
            "--report-only",
            "--format",
            "json",
            "--quiet",
            "--no-cache",
        ],
    );
    let json = parse_json(&out);
    assert!(
        json["css_analytics"]["raw_style_values"]
            .as_array()
            .is_some_and(|raw| !raw.is_empty()),
        "raw style values should stay available in css analytics: {}",
        out.stdout
    );
    let findings = json["styling_findings"]
        .as_array()
        .map_or(&[][..], Vec::as_slice);
    assert!(
        !findings
            .iter()
            .any(|finding| finding["sub_kind"] == "raw-style-value"),
        "tokenless raw values should not be promoted to styling findings: {findings:#?}"
    );
}

#[test]
fn health_css_unresolved_class_renders_in_human() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    write_file(
        &root.join("package.json"),
        r#"{"name":"css-class-typo-human","version":"1.0.0"}"#,
    );
    write_file(
        &root.join("src/styles.css"),
        ".sidebar-nav { color: red; }\n",
    );
    write_file(
        &root.join("src/App.jsx"),
        "export const C = () => <nav className=\"sidebar-nev\">x</nav>;\n",
    );

    let out = run_fallow_in_root("health", root, &["--css", "--max-crap", "10000", "--quiet"]);
    assert!(
        out.stdout.contains("likely class typo")
            && out.stdout.contains("sidebar-nev")
            && out.stdout.contains("did you mean")
            && out.stdout.contains("sidebar-nav"),
        "human output renders the typo with a suggestion: stdout={:?}",
        out.stdout
    );
}

#[test]
fn health_css_flags_duplicate_declaration_blocks() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    write_file(
        &root.join("package.json"),
        r#"{"name":"css-dup","version":"1.0.0"}"#,
    );
    write_file(&root.join("src/index.ts"), "export const x = 1;\n");
    // `.card` and `.panel` share an identical 4-declaration block in a different
    // order (one group). `.small` / `.other` share a 3-declaration block (below
    // the 4-floor, not reported). `.unique` is a 4-declaration block appearing
    // once (not reported).
    write_file(
        &root.join("src/a.css"),
        ".card { padding: 8px; margin: 4px; border-radius: 4px; color: red; }\n\
         .small { gap: 1px; width: 2px; height: 3px; }\n",
    );
    write_file(
        &root.join("src/b.css"),
        ".panel { color: red; border-radius: 4px; padding: 8px; margin: 4px; }\n\
         .other { gap: 1px; width: 2px; height: 3px; }\n\
         .unique { top: 1px; left: 2px; right: 3px; bottom: 4px; }\n",
    );

    let out = run_fallow_in_root(
        "health",
        root,
        &[
            "--css",
            "--max-crap",
            "10000",
            "--format",
            "json",
            "--quiet",
        ],
    );
    let json = parse_json(&out);
    let css = json
        .get("css_analytics")
        .expect("css_analytics present with --css");
    let summary = &css["summary"];

    // Only the 4-declaration `.card`/`.panel` block is a group (order-insensitive).
    assert_eq!(
        summary["duplicate_declaration_blocks"], 1,
        "summary: {summary}"
    );
    // savings = (2 occurrences - 1) * 4 declarations = 4.
    assert_eq!(
        summary["duplicate_declarations_total"], 4,
        "summary: {summary}"
    );

    let groups = css["duplicate_declaration_blocks"]
        .as_array()
        .expect("duplicate_declaration_blocks array");
    assert_eq!(groups.len(), 1);
    let g = &groups[0];
    assert_eq!(g["declaration_count"], 4);
    assert_eq!(g["occurrence_count"], 2);
    assert_eq!(g["estimated_savings"], 4);
    let occ = g["occurrences"].as_array().expect("occurrences array");
    assert_eq!(occ.len(), 2);
    // Sorted by (path, line): a.css before b.css.
    assert_eq!(occ[0]["path"], "src/a.css");
    assert_eq!(occ[1]["path"], "src/b.css");

    // Agent parity: a consolidate action (guidance-only, no command).
    let actions = g["actions"].as_array().expect("actions array");
    assert_eq!(actions[0]["type"], "consolidate");
    assert_eq!(actions[0]["auto_fixable"], false);
    assert!(
        actions[0].get("command").is_none(),
        "consolidate is guidance-only, no command: {actions:#?}"
    );

    // Human output renders the located group with savings + occurrences.
    let human = run_fallow_in_root("health", root, &["--css", "--max-crap", "10000", "--quiet"]);
    assert!(
        human.stdout.contains("duplicate declaration blocks")
            && human.stdout.contains("4 declarations in 2 rules")
            && human.stdout.contains("src/a.css:1"),
        "human output renders the duplicate-block group: stdout={:?}",
        human.stdout
    );
}

#[test]
fn health_css_preprocessor_virtual_stylesheets_feed_structural_analytics() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    write_file(
        &root.join("package.json"),
        r#"{"name":"preprocessor-analytics","version":"1.0.0"}"#,
    );
    write_file(&root.join("src/index.ts"), "export const x = 1;\n");
    write_file(
        &root.join("src/a.scss"),
        "$brand: #f00;\n\
         .card {\n\
           .body {\n\
             .title {\n\
               &:hover {\n\
                 color: $brand;\n\
                 padding: 8px;\n\
                 margin: 4px;\n\
                 border-radius: 4px;\n\
               }\n\
             }\n\
           }\n\
         }\n",
    );
    write_file(
        &root.join("src/b.less"),
        "@brand: #f00;\n\
         .panel {\n\
           color: @brand;\n\
           border-radius: 4px;\n\
           padding: 8px;\n\
           margin: 4px;\n\
         }\n",
    );

    let out = run_fallow_in_root(
        "health",
        root,
        &[
            "--css",
            "--max-crap",
            "10000",
            "--format",
            "json",
            "--quiet",
        ],
    );
    let json = parse_json(&out);
    let css = json
        .get("css_analytics")
        .expect("css_analytics present with --css");
    let summary = &css["summary"];
    assert_eq!(summary["files_analyzed"], 2, "summary: {summary}");
    assert_eq!(summary["preprocessor_stylesheets"], 2, "summary: {summary}");
    assert!(
        summary["max_nesting_depth"].as_u64().unwrap_or(0) >= 3,
        "nested SCSS should feed structural nesting metrics: {summary}"
    );
    assert_eq!(
        summary["duplicate_declaration_blocks"], 1,
        "SCSS and Less matching declaration blocks should be grouped: {summary}"
    );

    let groups = css["duplicate_declaration_blocks"]
        .as_array()
        .expect("duplicate_declaration_blocks array");
    let paths: Vec<_> = groups[0]["occurrences"]
        .as_array()
        .expect("occurrences array")
        .iter()
        .map(|occ| occ["path"].as_str().unwrap_or_default())
        .collect();
    assert!(
        paths.contains(&"src/a.scss") && paths.contains(&"src/b.less"),
        "duplicate block should point back to preprocessor sources: {groups:?}"
    );
}

#[test]
fn health_css_sfc_preprocessor_blocks_feed_structural_analytics() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    write_file(
        &root.join("package.json"),
        r#"{"name":"sfc-preprocessor-analytics","version":"1.0.0"}"#,
    );
    write_file(&root.join("src/index.ts"), "export const x = 1;\n");
    write_file(
        &root.join("src/Component.svelte"),
        "<script>export let title = '';</script>\n\
         <h1 class=\"title\">{title}</h1>\n\
         <style lang=\"scss\">\n\
         .card {\n\
           .body {\n\
             .title {\n\
               &:hover { color: $brand; }\n\
             }\n\
           }\n\
         }\n\
         </style>\n",
    );

    let out = run_fallow_in_root(
        "health",
        root,
        &[
            "--css",
            "--max-crap",
            "10000",
            "--format",
            "json",
            "--quiet",
        ],
    );
    let json = parse_json(&out);
    let css = json
        .get("css_analytics")
        .expect("css_analytics present with --css");
    let summary = &css["summary"];
    assert_eq!(summary["files_analyzed"], 1, "summary: {summary}");
    assert!(
        summary["max_nesting_depth"].as_u64().unwrap_or(0) >= 3,
        "SFC SCSS nesting should feed structural metrics: {summary}"
    );
}

#[test]
fn health_css_counts_shadow_radius_lineheight_sprawl() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    write_file(
        &root.join("package.json"),
        r#"{"name":"css-sprawl","version":"1.0.0"}"#,
    );
    write_file(&root.join("src/index.ts"), "export const x = 1;\n");
    // Distinct shadows {0 1px 2px #000, 0 2px 4px #111} = 2; radii {4px, 8px} = 2;
    // line-heights {1.5, 2} = 2. Each rule has 3 declarations (below the
    // duplicate-block floor, so no interference).
    write_file(
        &root.join("src/styles.css"),
        ".a { box-shadow: 0 1px 2px #000; border-radius: 4px; line-height: 1.5; }\n\
         .b { box-shadow: 0 2px 4px #111; border-radius: 4px; line-height: 1.5; }\n\
         .c { box-shadow: 0 1px 2px #000; border-radius: 8px; line-height: 2; }\n",
    );

    let out = run_fallow_in_root(
        "health",
        root,
        &[
            "--css",
            "--max-crap",
            "10000",
            "--format",
            "json",
            "--quiet",
        ],
    );
    let json = parse_json(&out);
    let s = &json["css_analytics"]["summary"];
    assert_eq!(s["unique_box_shadows"], 2, "summary: {s}");
    assert_eq!(s["unique_border_radii"], 2, "summary: {s}");
    assert_eq!(s["unique_line_heights"], 2, "summary: {s}");

    // The human "(cont.)" line surfaces them only when present.
    let human = run_fallow_in_root("health", root, &["--css", "--max-crap", "10000", "--quiet"]);
    assert!(
        human.stdout.contains("value sprawl (cont.)") && human.stdout.contains("shadow"),
        "human renders shadow/radius/line-height sprawl: stdout={:?}",
        human.stdout
    );
}

#[test]
fn health_css_flags_tailwind_arbitrary_values() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    // The tailwindcss dependency gates the arbitrary-value markup scan on.
    write_file(
        &root.join("package.json"),
        r#"{"name":"tw","version":"1.0.0","devDependencies":{"tailwindcss":"^3.4.0"}}"#,
    );
    write_file(&root.join("src/index.ts"), "export const x = 1;\n");
    write_file(&root.join("src/app.css"), ".x { color: red; }\n");
    write_file(
        &root.join("src/Button.tsx"),
        "export const B = () => <div className=\"w-[13px] bg-[#abc] w-[13px]\">x</div>;\n",
    );
    write_file(
        &root.join("src/Card.tsx"),
        "export const C = () => <div className=\"top-[7px]\">y</div>;\n",
    );

    let out = run_fallow_in_root(
        "health",
        root,
        &[
            "--css",
            "--max-crap",
            "10000",
            "--format",
            "json",
            "--quiet",
        ],
    );
    let json = parse_json(&out);
    let css = json
        .get("css_analytics")
        .expect("css_analytics present with --css");
    let s = &css["summary"];
    // Distinct tokens: w-[13px], bg-[#abc], top-[7px] = 3; total uses = 4.
    assert_eq!(s["tailwind_arbitrary_values"], 3, "summary: {s}");
    assert_eq!(s["tailwind_arbitrary_value_uses"], 4, "summary: {s}");

    let arb = css["tailwind_arbitrary_values"]
        .as_array()
        .expect("tailwind_arbitrary_values array");
    // Sorted by use count descending: w-[13px] (2x) first, located at its first file.
    assert_eq!(arb[0]["value"], "w-[13px]");
    assert_eq!(arb[0]["count"], 2);
    assert_eq!(arb[0]["path"], "src/Button.tsx");

    // Each entry carries a replace-with-token action with a find-all search.
    let actions = arb[0]["actions"].as_array().expect("actions array");
    assert_eq!(actions[0]["type"], "replace-with-token");
    assert_eq!(actions[0]["auto_fixable"], false);
    assert!(
        actions[0]["command"]
            .as_str()
            .is_some_and(|c| c.contains("grep -rnF 'w-[13px]'")),
        "action carries a fixed-string search for the token: {actions:#?}"
    );

    // Human output surfaces the bypass section.
    let human = run_fallow_in_root("health", root, &["--css", "--max-crap", "10000", "--quiet"]);
    assert!(
        human.stdout.contains("Tailwind arbitrary values")
            && human.stdout.contains("w-[13px] (2x)"),
        "human renders the Tailwind arbitrary-value section: stdout={:?}",
        human.stdout
    );
}

#[test]
fn health_css_emits_selector_and_dead_surface_styling_findings() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    write_file(
        root.join("package.json").as_path(),
        r#"{"name":"css-findings"}"#,
    );
    write_file(root.join("src/index.ts").as_path(), "export const x = 1;\n");
    write_file(
        root.join("src/styles.css").as_path(),
        "#app .card .title { color: red; }\n",
    );
    write_file(
        root.join("src/Card.vue").as_path(),
        "<template><div class=\"used\">x</div></template>\n\
         <style scoped>\n\
         .used { color: green; }\n\
         .dead { color: red; }\n\
         </style>\n",
    );

    let out = run_fallow_in_root(
        "health",
        root,
        &[
            "--css",
            "--max-crap",
            "10000",
            "--format",
            "json",
            "--quiet",
        ],
    );
    let json = parse_json(&out);
    let findings = json["styling_findings"]
        .as_array()
        .expect("styling findings array");
    assert!(
        findings.iter().any(|finding| {
            finding["code"] == "css-selector-complexity"
                && finding["sub_kind"] == "high-specificity"
                && finding["path"] == "src/styles.css"
        }),
        "selector-complexity finding present: {findings:#?}"
    );
    assert!(
        findings.iter().any(|finding| {
            finding["code"] == "css-dead-surface"
                && finding["sub_kind"] == "scoped-unused-class"
                && finding["path"] == "src/Card.vue"
        }),
        "dead-surface finding present: {findings:#?}"
    );
}

#[test]
fn health_css_tailwind_scan_gated_on_tailwind_dependency() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    // No tailwindcss dependency: the scan does not run even though the markup
    // contains a bracket token (the gate avoids false positives off-Tailwind).
    write_file(
        &root.join("package.json"),
        r#"{"name":"no-tw","version":"1.0.0"}"#,
    );
    write_file(&root.join("src/index.ts"), "export const x = 1;\n");
    write_file(&root.join("src/app.css"), "#main { color: red; }\n");
    write_file(
        &root.join("src/Button.tsx"),
        "export const B = () => <div className=\"w-[13px]\">x</div>;\n",
    );

    let out = run_fallow_in_root(
        "health",
        root,
        &[
            "--css",
            "--max-crap",
            "10000",
            "--format",
            "json",
            "--quiet",
        ],
    );
    let json = parse_json(&out);
    let css = json
        .get("css_analytics")
        .expect("css_analytics present from the .css file");
    assert_eq!(
        css["summary"]["tailwind_arbitrary_values"], 0,
        "no tailwind dep => no arbitrary-value scan"
    );
    assert!(
        css.get("tailwind_arbitrary_values").is_none(),
        "located list omitted when empty"
    );
}

#[test]
fn health_css_flags_unused_at_rules() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    write_file(
        &root.join("package.json"),
        r#"{"name":"atrules","version":"1.0.0"}"#,
    );
    write_file(&root.join("src/index.ts"), "export const x = 1;\n");
    // `--used` is registered AND referenced; `--orphan` is registered but never
    // var()'d -> unused @property. `base` is declared and populated; `utilities`
    // is declared but never populated -> unused @layer.
    write_file(
        &root.join("src/styles.css"),
        "@property --used { syntax: \"<color>\"; inherits: false; initial-value: red; }\n\
         @property --orphan { syntax: \"<length>\"; inherits: false; initial-value: 0px; }\n\
         @layer base, utilities;\n\
         @layer base { .a { color: var(--used); } }\n",
    );

    let out = run_fallow_in_root(
        "health",
        root,
        &[
            "--css",
            "--max-crap",
            "10000",
            "--format",
            "json",
            "--quiet",
        ],
    );
    let json = parse_json(&out);
    let css = json
        .get("css_analytics")
        .expect("css_analytics present with --css");
    let s = &css["summary"];
    assert_eq!(s["unused_property_registrations"], 1, "summary: {s}");
    assert_eq!(s["unused_layers"], 1, "summary: {s}");

    let entries = css["unused_at_rules"]
        .as_array()
        .expect("unused_at_rules array");
    assert_eq!(entries.len(), 2);
    let prop = entries
        .iter()
        .find(|e| e["type"] == "property-registration")
        .expect("property-registration entry");
    assert_eq!(prop["name"], "--orphan");
    assert_eq!(prop["path"], "src/styles.css");
    assert_eq!(prop["actions"][0]["type"], "verify-unused");
    let layer = entries
        .iter()
        .find(|e| e["type"] == "layer")
        .expect("layer entry");
    assert_eq!(layer["name"], "utilities");
}

#[test]
fn health_css_markdown_section_present() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    write_file(
        &root.join("package.json"),
        r#"{"name":"md","version":"1.0.0"}"#,
    );
    write_file(&root.join("src/index.ts"), "export const x = 1;\n");
    write_file(
        &root.join("src/styles.css"),
        "#main { color: red; }\n.spinner { animation-name: ghost; }\n",
    );

    let out = run_fallow_in_root(
        "health",
        root,
        &[
            "--css",
            "--max-crap",
            "10000",
            "--format",
            "markdown",
            "--quiet",
        ],
    );
    assert!(
        out.stdout.contains("## CSS Health")
            && out.stdout.contains("Value sprawl")
            && out.stdout.contains("Candidates")
            && out.stdout.contains("ghost"),
        "markdown CSS Health section renders summary + undefined keyframe: stdout={:?}",
        out.stdout
    );
}

#[test]
fn health_css_flags_unused_scoped_vue_class() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    write_file(
        &root.join("package.json"),
        r#"{"name":"vue-fixture","version":"1.0.0","dependencies":{"vue":"^3.4.0"}}"#,
    );
    write_file(&root.join("src/index.ts"), "export const x = 1;\n");
    // `.used` is referenced in the template; `.dead` is referenced nowhere.
    write_file(
        &root.join("src/App.vue"),
        "<template><div class=\"used\"></div></template>\n\
         <style scoped>.used { color: red; } .dead { color: blue; }</style>\n",
    );

    let out = run_fallow_in_root(
        "health",
        root,
        &[
            "--css",
            "--max-crap",
            "10000",
            "--format",
            "json",
            "--quiet",
        ],
    );
    let json = parse_json(&out);
    let css = json
        .get("css_analytics")
        .expect("css_analytics present with --css and a scoped-dead class");
    assert_eq!(
        css["summary"]["scoped_unused_classes"], 1,
        "stdout: {}",
        out.stdout
    );
    let scoped = css["scoped_unused"]
        .as_array()
        .expect("scoped_unused array");
    assert_eq!(scoped.len(), 1);
    assert_eq!(scoped[0]["classes"][0], "dead");

    // The scoped candidate also carries a verify action, but with no command:
    // the component-scoped scan already covers static uses, so the residual
    // check (dynamic string bindings) is manual.
    let scoped_actions = scoped[0]["actions"]
        .as_array()
        .expect("scoped_unused actions array");
    assert_eq!(scoped_actions[0]["type"], "verify-unused");
    assert!(
        scoped_actions[0].get("command").is_none(),
        "scoped verify action omits a command: {scoped_actions:#?}"
    );

    // The SFC `<style>` block is folded into the metric path, so the component's
    // styles are analyzed (red + blue), not silently excluded.
    assert_eq!(
        css["summary"]["files_analyzed"], 1,
        "stdout: {}",
        out.stdout
    );
    assert!(
        css["summary"]["unique_colors"].as_u64().unwrap() >= 2,
        "the SFC <style> colors are counted: {}",
        out.stdout
    );
}

#[test]
fn health_css_human_section_survives_empty_complexity_section() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    write_file(
        &root.join("package.json"),
        r#"{"name":"css-human","version":"1.0.0"}"#,
    );
    // Trivial code: no complexity finding, so the `--complexity` section is empty.
    write_file(&root.join("src/index.ts"), "export const x = 1;\n");
    write_file(&root.join("src/styles.css"), "#main { color: red; }\n");

    // `--complexity` selects an (empty) section without forcing a score. Without
    // including css_analytics in the empty-report early-return, the human
    // renderer would print the green "no findings" line and drop the CSS
    // section. This is the human-output path the JSON tests do not cover.
    let out = run_fallow_in_root(
        "health",
        root,
        &["--css", "--complexity", "--max-crap", "10000", "--quiet"],
    );
    assert!(
        out.stdout.contains("CSS health"),
        "human output must keep the CSS health section: stdout={:?} stderr={:?}",
        out.stdout,
        out.stderr
    );
}

/// CSS-in-JS first-class, Phase 3b: `fallow health --css` lifts styled-components
/// / emotion tagged-template CSS into the styling analytics so a CSS-in-JS app
/// gets non-null `css_analytics` + `styling_health` instead of `null`, with
/// cross-file duplicate styled blocks surfaced and notable-rule line numbers
/// mapped back to the styled template. Also pins the dep gate: a project with no
/// CSS-in-JS library never analyzes its JS/TS files (no `files_analyzed`
/// inflation).
#[test]
#[allow(
    clippy::too_many_lines,
    reason = "test fixture; linear setup/assert, length is not a maintainability concern. \
              #[allow] not #[expect] because the body sits on the 100-line threshold, so \
              whether the lint fires is clippy-version dependent and an #[expect] is \
              unfulfilled on some toolchains"
)]
fn health_css_lifts_css_in_js_tagged_templates() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    write_file(
        &root.join("package.json"),
        r#"{"name":"css-in-js-health-fixture","version":"1.0.0","dependencies":{"styled-components":"^6.1.0"}}"#,
    );
    // Two files share an identical 4-declaration styled block (cross-file
    // design-system erosion); a third file has a notable `!important` rule and an
    // interpolation-heavy template.
    write_file(
        &root.join("src/CardA.tsx"),
        "import styled from 'styled-components';\n\
         export const CardA = styled.div`\n\
         display: flex;\n\
         align-items: center;\n\
         justify-content: space-between;\n\
         padding: 16px;\n\
         `;\n",
    );
    write_file(
        &root.join("src/CardB.tsx"),
        "import styled from 'styled-components';\n\
         export const CardB = styled.section`\n\
         display: flex;\n\
         align-items: center;\n\
         justify-content: space-between;\n\
         padding: 16px;\n\
         `;\n",
    );
    write_file(
        &root.join("src/Misc.tsx"),
        "import styled from 'styled-components';\n\
         export const Danger = styled.button`\n\
         color: red !important;\n\
         `;\n\
         export const Interp = styled.div`\n\
         color: ${theme.primary};\n\
         margin: ${x}px ${y}px;\n\
         `;\n",
    );

    let out = run_fallow_in_root(
        "health",
        root,
        &[
            "--css",
            "--max-crap",
            "10000",
            "--format",
            "json",
            "--quiet",
        ],
    );
    let json = parse_json(&out);

    // (criterion 6) css_analytics is non-null and analyzed the styled files.
    let css = json
        .get("css_analytics")
        .expect("css_analytics present for a CSS-in-JS project");
    let files_analyzed = css["summary"]["files_analyzed"].as_u64().unwrap();
    assert!(
        files_analyzed >= 3,
        "the three styled files should be analyzed: {css}"
    );

    // (criterion 7) styling_health is non-null (score + grade + confidence).
    let sh = json
        .get("styling_health")
        .expect("styling_health present for a CSS-in-JS project");
    assert!(
        sh.get("grade").is_some(),
        "styling_health has a grade: {sh}"
    );
    assert!(
        sh.get("confidence").is_some(),
        "styling_health has confidence: {sh}"
    );

    // (criterion 8) the duplicated 4-declaration styled block surfaces across the
    // two files.
    let dups = css["duplicate_declaration_blocks"].as_array().unwrap();
    assert!(
        dups.iter()
            .any(|d| d["occurrence_count"].as_u64().unwrap() >= 2
                && d["declaration_count"].as_u64().unwrap() >= 4),
        "the cross-file duplicate styled block should surface: {css}"
    );

    // (criterion 9) the authored `!important` rule is notable and its line maps
    // back onto the styled template in the source (not line 1).
    let files = css["files"].as_array().unwrap();
    let notable: Vec<_> = files
        .iter()
        .flat_map(|f| f["analytics"]["notable_rules"].as_array().unwrap().iter())
        .collect();
    assert!(
        notable
            .iter()
            .any(|r| r["important_count"].as_u64().unwrap_or(0) >= 1),
        "the authored !important declaration is a notable rule: {css}"
    );
    assert!(
        notable.iter().all(|r| r["line"].as_u64().unwrap_or(0) >= 2),
        "lifted rule line numbers map onto the styled template, not line 1: {css}"
    );

    // (criterion 11) the interpolation-heavy template did not invent !important
    // beyond the one authored, and did not blow up the parse (css_analytics is
    // present, asserted above). Exactly one authored !important across the corpus.
    assert_eq!(
        css["summary"]["important_declarations"].as_u64().unwrap(),
        1,
        "only the one authored !important is counted, masking invents none: {css}"
    );
}

/// (criterion 10) A project with no CSS-in-JS library never analyzes its JS/TS
/// files for styling analytics: `css_analytics` stays absent, so the JS/TS arm
/// adds zero `files_analyzed` and the output is byte-identical to pre-3b.
#[test]
fn health_css_skips_js_ts_without_css_in_js_dep() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    write_file(
        &root.join("package.json"),
        r#"{"name":"no-css-in-js-fixture","version":"1.0.0","dependencies":{"react":"^18.3.0"}}"#,
    );
    // A .tsx file that LOOKS like CSS-in-JS but the project declares no CSS-in-JS
    // library, so the JS/TS arm of the CSS walk is gated off.
    write_file(
        &root.join("src/App.tsx"),
        "const Btn = styled.button`color: red;`;\nexport const x = 1;\n",
    );

    let out = run_fallow_in_root(
        "health",
        root,
        &[
            "--css",
            "--max-crap",
            "10000",
            "--format",
            "json",
            "--quiet",
        ],
    );
    let json = parse_json(&out);
    assert!(
        json.get("css_analytics").is_none(),
        "no CSS-in-JS dep means no JS/TS styling analytics: {}",
        out.stdout
    );
}
