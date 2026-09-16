//! `gate_outcomes` is a projection, not a second rule.
//!
//! Every gate the envelope publishes is computed by the same code that decides
//! the exit code, so these tests pin the identity rather than the value: they
//! assert the published entry against the feature-local field beside it and
//! against the process status, on the same run. A drift between the two is the
//! defect this object exists to prevent, and it would be invisible to a test
//! that only checked the entry was present.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests and benches use unwrap and expect to keep fixture setup concise"
)]

#[path = "common/mod.rs"]
mod common;

use common::{CommandOutput, parse_json, run_fallow_raw};
use serde_json::Value;
use std::fmt::Write as _;
use std::path::Path;
use tempfile::TempDir;

/// A project with `count` unreferenced modules plus one reachable entry point.
fn orphan_project(count: usize) -> TempDir {
    let dir = TempDir::new().expect("temp project");
    let root = dir.path();
    std::fs::create_dir_all(root.join("src")).expect("src dir");
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"gate-fx","version":"1.0.0","private":true,"main":"src/index.js"}"#,
    )
    .expect("package.json");
    std::fs::write(
        root.join("src/index.ts"),
        "export const main = (): number => 1;\n",
    )
    .expect("entry");
    for index in 0..count {
        std::fs::write(
            root.join(format!("src/orphan{index}.ts")),
            format!("export const orphan{index} = (): number => {index};\n"),
        )
        .expect("orphan module");
    }
    dir
}

/// Two modules holding the same function body, so duplication is non-zero.
fn cloned_project() -> TempDir {
    let dir = TempDir::new().expect("temp project");
    let root = dir.path();
    std::fs::create_dir_all(root.join("src")).expect("src dir");
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"clone-fx","version":"1.0.0","private":true}"#,
    )
    .expect("package.json");
    let body = "export const NAME = (rows: number[]): number => {\n  let total = 0;\n  for (const row of rows) {\n    if (row > 0) {\n      total += row * 2;\n    } else {\n      total -= row;\n    }\n  }\n  if (total > 100) {\n    total = total / 2;\n  }\n  return Math.round(total);\n};\n";
    std::fs::write(root.join("src/a.ts"), body.replace("NAME", "totalsA")).expect("a.ts");
    std::fs::write(root.join("src/b.ts"), body.replace("NAME", "totalsB")).expect("b.ts");
    std::fs::write(
        root.join("src/index.ts"),
        "export { totalsA } from \"./a\";\nexport { totalsB } from \"./b\";\n",
    )
    .expect("index.ts");
    dir
}

/// One function complex enough to cross the default complexity thresholds.
fn complex_project() -> TempDir {
    let dir = TempDir::new().expect("temp project");
    let root = dir.path();
    std::fs::create_dir_all(root.join("src")).expect("src dir");
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"health-fx","version":"1.0.0","private":true}"#,
    )
    .expect("package.json");
    let mut body =
        String::from("export const classify = (a: number, b: number, c: string): string => {\n");
    for index in 0..12 {
        let _ = writeln!(
            body,
            "  if (a > {index} && b < {}) {{ if (c === 'k{index}') {{ return 'a{index}'; }} }}",
            index * 2
        );
    }
    body.push_str("  return 'none';\n};\n");
    std::fs::write(root.join("src/complex.ts"), body).expect("complex.ts");
    std::fs::write(
        root.join("src/index.ts"),
        "export { classify } from \"./complex\";\n",
    )
    .expect("index.ts");
    dir
}

fn run(args: &[&str]) -> CommandOutput {
    run_fallow_raw(args)
}

fn gate<'a>(envelope: &'a Value, name: &str) -> &'a Value {
    let entry = &envelope["gate_outcomes"][name];
    assert!(
        !entry.is_null(),
        "expected a `{name}` entry, got {}",
        envelope["gate_outcomes"]
    );
    entry
}

fn root_arg(dir: &TempDir) -> &str {
    dir.path().to_str().expect("temp path is UTF-8")
}

fn save_regression_baseline(root: &Path, file: &str) {
    let out = run(&[
        "dead-code",
        "--root",
        root.to_str().expect("utf8"),
        "--format",
        "json",
        "--quiet",
        "--save-regression-baseline",
        file,
    ]);
    assert!(
        out.code == 0 || out.code == 1,
        "saving a regression baseline should not error: {}",
        out.stderr
    );
}

/// The headline invariant: a run that arms no gate is byte-identical to one
/// produced before the object existed, on every command. Without this the
/// additive-field exemption in `docs/backwards-compatibility.md` would not
/// apply and six envelopes would owe a `schema_version` bump.
#[test]
fn a_run_that_arms_no_gate_emits_no_gate_outcomes_key() {
    let project = orphan_project(2);
    let root = root_arg(&project);
    for args in [
        vec!["dead-code", "--root", root, "--format", "json", "--quiet"],
        vec!["dupes", "--root", root, "--format", "json", "--quiet"],
        vec!["health", "--root", root, "--format", "json", "--quiet"],
        vec!["security", "--root", root, "--format", "json", "--quiet"],
        vec!["--root", root, "--format", "json", "--quiet"],
    ] {
        let envelope = parse_json(&run(&args));
        assert!(
            envelope.get("gate_outcomes").is_none(),
            "`{}` armed no gate and must carry no key: {}",
            args[0],
            envelope["gate_outcomes"]
        );
    }
}

/// `gate_outcomes["regression"].status` is `regression.exceeded`, not a second
/// reading of the same counts.
#[test]
fn the_regression_entry_agrees_with_the_regression_object_and_the_exit_code() {
    let project = orphan_project(1);
    let root = project.path();
    let baseline = root.join("regression.json");
    let baseline_arg = baseline.to_str().expect("utf8");
    save_regression_baseline(root, baseline_arg);

    for index in 1..5 {
        std::fs::write(
            root.join(format!("src/orphan{index}.ts")),
            format!("export const orphan{index} = (): number => {index};\n"),
        )
        .expect("more orphans");
    }

    let output = run(&[
        "dead-code",
        "--root",
        root.to_str().expect("utf8"),
        "--format",
        "json",
        "--quiet",
        "--fail-on-regression",
        "--tolerance",
        "0",
        "--regression-baseline",
        baseline_arg,
    ]);
    let envelope = parse_json(&output);
    let exceeded = envelope["regression"]["exceeded"]
        .as_bool()
        .expect("the regression object is published");
    let entry = gate(&envelope, "regression");
    assert_eq!(
        entry["status"],
        Value::from(if exceeded { "fail" } else { "pass" }),
        "the entry restates `regression.exceeded`"
    );
    assert_eq!(entry["enforced"], Value::Bool(true));
    assert_eq!(
        output.code,
        i32::from(exceeded),
        "the published verdict and the process status agree: {}",
        output.stderr
    );
}

/// The duplication gate had no envelope surface at all before this object, so
/// the entry is the only machine channel and its numbers must be the ones the
/// CLI compared.
#[test]
fn the_duplication_threshold_entry_carries_the_numbers_it_compared() {
    let project = cloned_project();
    let root = root_arg(&project);
    let output = run(&[
        "dupes",
        "--root",
        root,
        "--format",
        "json",
        "--quiet",
        "--threshold",
        "5",
    ]);
    let envelope = parse_json(&output);
    let entry = gate(&envelope, "duplication-threshold");
    let percentage = envelope["stats"]["duplication_percentage"]
        .as_f64()
        .expect("duplication percentage is published");
    assert_eq!(entry["threshold"], Value::from(5.0));
    assert_eq!(
        entry["observed"], envelope["stats"]["duplication_percentage"],
        "the entry reports the percentage the gate compared: {percentage}"
    );
    assert_eq!(entry["status"], Value::from("fail"));
    assert_eq!(output.code, 1, "the gate fails the run: {}", output.stderr);
}

/// A threshold of zero is the CLI's own spelling of "no limit", so it arms
/// nothing and must not publish a passing gate that was never evaluated.
#[test]
fn a_zero_threshold_arms_no_duplication_gate() {
    let project = cloned_project();
    let envelope = parse_json(&run(&[
        "dupes",
        "--root",
        root_arg(&project),
        "--format",
        "json",
        "--quiet",
        "--threshold",
        "0",
    ]));
    assert!(envelope.get("gate_outcomes").is_none());
}

/// `--min-score` is the gate #2682 filed: it had no envelope member at all, and
/// its verdict now has to carry the score and the threshold so a consumer can
/// say what happened without restating the comparison.
#[test]
fn the_health_score_entry_carries_the_score_and_the_threshold() {
    let project = complex_project();
    let root = root_arg(&project);
    let output = run(&[
        "health",
        "--root",
        root,
        "--format",
        "json",
        "--quiet",
        "--min-score",
        "101",
        "--complexity",
    ]);
    let envelope = parse_json(&output);
    let entry = gate(&envelope, "health-min-score");
    let score = envelope["health_score"]["score"]
        .as_f64()
        .expect("the score is published");
    assert_eq!(
        entry["observed"], envelope["health_score"]["score"],
        "the entry reports the score the gate compared: {score}"
    );
    assert_eq!(entry["threshold"], Value::from(101.0));
    assert_eq!(
        entry["status"],
        Value::from("fail"),
        "no project scores above 101"
    );
    assert_eq!(output.code, 1, "the gate fails the run: {}", output.stderr);
}

/// `--min-severity` produced no output of any kind before this: no stderr line
/// and a byte-identical envelope, so the exit code was its whole surface.
#[test]
fn the_health_severity_entry_appears_and_agrees_with_the_exit_code() {
    let project = complex_project();
    let root = root_arg(&project);
    let output = run(&[
        "health",
        "--root",
        root,
        "--format",
        "json",
        "--quiet",
        "--min-severity",
        "critical",
    ]);
    let envelope = parse_json(&output);
    let entry = gate(&envelope, "health-min-severity");
    let failed = entry["status"] == "fail";
    assert_eq!(
        output.code,
        i32::from(failed),
        "the published verdict and the process status agree: {}",
        output.stderr
    );
    assert!(
        entry["observed"].is_number(),
        "the entry reports how many findings reached the floor"
    );
}

/// `--report-only` is an explicit request never to fail. The verdict still has
/// to be published, or the flag silently removes the only channel a consumer
/// has; `enforced` is what keeps the two facts apart.
#[test]
fn report_only_publishes_the_verdict_and_enforces_nothing() {
    let project = complex_project();
    let root = root_arg(&project);
    let output = run(&[
        "health",
        "--root",
        root,
        "--format",
        "json",
        "--quiet",
        "--report-only",
    ]);
    assert_eq!(
        output.code, 0,
        "--report-only never fails: {}",
        output.stderr
    );
    let envelope = parse_json(&output);
    if let Some(entry) = envelope["gate_outcomes"].get("health-findings") {
        assert_eq!(
            entry["enforced"],
            Value::Bool(false),
            "a run told never to fail enforces nothing"
        );
    }
}

/// #2685's own acceptance criterion, as a Rust test rather than a shell one:
/// the invariant `crates/output/src/security.rs` documents had no test.
#[test]
fn the_security_entry_agrees_with_the_gate_verdict_and_exit_8() {
    let project = TempDir::new().expect("temp project");
    let root = project.path();
    std::fs::create_dir_all(root.join("src")).expect("src dir");
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"sec-fx","version":"1.0.0","private":true}"#,
    )
    .expect("package.json");
    std::fs::write(
        root.join("src/index.ts"),
        "export const boot = (): string => \"ok\";\n",
    )
    .expect("index.ts");
    let git = |args: &[&str]| {
        std::process::Command::new("git")
            .args(args)
            .current_dir(root)
            .output()
            .expect("git runs");
    };
    git(&["init", "-q"]);
    git(&["config", "user.email", "t@example.com"]);
    git(&["config", "user.name", "t"]);
    git(&["add", "-A"]);
    git(&["commit", "-qm", "base"]);
    let base = String::from_utf8(
        std::process::Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(root)
            .output()
            .expect("git runs")
            .stdout,
    )
    .expect("utf8");
    let base = base.trim();

    std::fs::write(
        root.join("src/danger.ts"),
        "import { execSync } from \"node:child_process\";\nexport const runIt = (userInput: string): string =>\n  execSync(`ls ${userInput}`).toString();\n",
    )
    .expect("danger.ts");
    std::fs::write(
        root.join("src/index.ts"),
        "export const boot = (): string => \"ok\";\nexport { runIt } from \"./danger\";\n",
    )
    .expect("index.ts");
    git(&["add", "-A"]);
    git(&["commit", "-qm", "head"]);

    let output = run(&[
        "security",
        "--root",
        root.to_str().expect("utf8"),
        "--format",
        "json",
        "--quiet",
        "--gate",
        "new",
        "--changed-since",
        base,
    ]);
    let envelope = parse_json(&output);
    let verdict = envelope["gate"]["verdict"]
        .as_str()
        .expect("the gate object is published on pass and fail");
    let entry = gate(&envelope, "security");
    assert_eq!(
        entry["status"],
        Value::from(verdict),
        "the entry restates `gate.verdict`"
    );
    if verdict == "fail" {
        assert_eq!(
            output.code, 8,
            "the security gate exits 8: {}",
            output.stderr
        );
    }
    assert_eq!(
        gate(&envelope, "security-advisory")["status"],
        Value::from("skipped"),
        "a configured gate returns before the advisory, so the advisory stood down"
    );
}

/// The grouped dead-code envelope carries no `regression` object at all, so for
/// a grouped run the index is the only channel the verdict has.
#[test]
fn the_grouped_envelope_carries_the_verdict_it_has_no_other_field_for() {
    let project = orphan_project(1);
    let root = project.path();
    let baseline = root.join("regression.json");
    let baseline_arg = baseline.to_str().expect("utf8");
    save_regression_baseline(root, baseline_arg);
    for index in 1..5 {
        std::fs::write(
            root.join(format!("src/orphan{index}.ts")),
            format!("export const orphan{index} = (): number => {index};\n"),
        )
        .expect("more orphans");
    }

    let envelope = parse_json(&run(&[
        "dead-code",
        "--root",
        root.to_str().expect("utf8"),
        "--format",
        "json",
        "--quiet",
        "--group-by",
        "directory",
        "--fail-on-regression",
        "--tolerance",
        "0",
        "--regression-baseline",
        baseline_arg,
    ]));
    assert_eq!(envelope["kind"], Value::from("dead-code-grouped"));
    assert!(
        envelope.get("regression").is_none(),
        "the grouped envelope has never carried the regression object"
    );
    assert_eq!(gate(&envelope, "regression")["status"], Value::from("fail"));
}

/// A run that analyzed nothing reported a clean green on every machine surface
/// (issue #2686). The diagnostic is recorded by discovery, so it reaches the
/// shared list every envelope and every MCP tool is built from.
#[test]
fn a_run_that_analyzed_nothing_says_so_in_the_envelope() {
    let project = TempDir::new().expect("temp project");
    let root = project.path();
    std::fs::create_dir_all(root.join("dist")).expect("dist dir");
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"nothing-fx","version":"1.0.0","private":true}"#,
    )
    .expect("package.json");
    for index in 0..3 {
        std::fs::write(
            root.join(format!("dist/bundle{index}.js")),
            format!("export const d{index} = () => {index};\n"),
        )
        .expect("built output");
    }

    let output = run(&[
        "dead-code",
        "--root",
        root.to_str().expect("utf8"),
        "--format",
        "json",
        "--quiet",
    ]);
    assert_eq!(output.code, 0, "the run itself is green");
    let envelope = parse_json(&output);
    let diagnostics = envelope["workspace_diagnostics"]
        .as_array()
        .expect("diagnostics are published");
    let entry = diagnostics
        .iter()
        .find(|d| d["kind"] == "no-source-files-analyzed")
        .expect("the run analyzed nothing and must say so");
    assert_eq!(
        entry["excluded_file_count"],
        Value::from(3),
        "the built-in-ignore contribution stays attributable"
    );
    assert_eq!(
        entry["degrades_analysis"],
        Value::Bool(true),
        "a consumer reads this instead of hardcoding a kind allowlist"
    );

    for kind in ["boundaries-not-configured", "rule-packs-not-configured"] {
        let advisory = diagnostics
            .iter()
            .find(|d| d["kind"] == kind)
            .expect("the unconfigured-check advisories are still published");
        assert!(
            advisory.get("degrades_analysis").is_none(),
            "{kind} fires in the product's default state and must not be flagged"
        );
    }
}

/// An empty project has no exclusion to blame, which is the half the human
/// sentence has never covered.
#[test]
fn the_empty_case_reports_without_an_exclusion_to_blame() {
    let project = TempDir::new().expect("temp project");
    let root = project.path();
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"empty-fx","version":"1.0.0","private":true}"#,
    )
    .expect("package.json");
    let envelope = parse_json(&run(&[
        "dead-code",
        "--root",
        root.to_str().expect("utf8"),
        "--format",
        "json",
        "--quiet",
    ]));
    let entry = envelope["workspace_diagnostics"]
        .as_array()
        .expect("diagnostics are published")
        .iter()
        .find(|d| d["kind"] == "no-source-files-analyzed")
        .expect("an empty project analyzed nothing");
    assert_eq!(entry["excluded_file_count"], Value::from(0));
}

/// #2684: the producing run's verdict survives the re-render, and its exit code
/// deliberately does not.
#[test]
fn report_from_states_the_verdict_and_still_exits_zero() {
    let project = orphan_project(1);
    let root = project.path();
    let baseline = root.join("regression.json");
    let baseline_arg = baseline.to_str().expect("utf8");
    save_regression_baseline(root, baseline_arg);
    for index in 1..5 {
        std::fs::write(
            root.join(format!("src/orphan{index}.ts")),
            format!("export const orphan{index} = (): number => {index};\n"),
        )
        .expect("more orphans");
    }
    let produced = run(&[
        "dead-code",
        "--root",
        root.to_str().expect("utf8"),
        "--format",
        "json",
        "--quiet",
        "--fail-on-regression",
        "--tolerance",
        "0",
        "--regression-baseline",
        baseline_arg,
    ]);
    assert_eq!(
        produced.code, 1,
        "the producing run fails: {}",
        produced.stderr
    );
    let saved = root.join("results.json");
    std::fs::write(&saved, &produced.stdout).expect("save the envelope");

    for format in ["github-annotations", "github-summary"] {
        let rendered = run(&[
            "report",
            "--from",
            saved.to_str().expect("utf8"),
            "--root",
            root.to_str().expect("utf8"),
            "--quiet",
            "--format",
            format,
        ]);
        assert_eq!(
            rendered.code, 0,
            "the caller owns the status, so every render exits 0: {}",
            rendered.stderr
        );
        assert!(
            rendered.stdout.contains("regression"),
            "the {format} re-render names the gate that failed: {}",
            rendered.stdout
        );
    }
}
