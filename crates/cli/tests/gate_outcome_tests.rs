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
    assert!(
        exceeded,
        "the fixture grew from 1 unused file to 5, so the gate must have tripped: {}",
        envelope["regression"]
    );
    assert_eq!(output.code, 1, "and the run must fail: {}", output.stderr);
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
    assert_eq!(
        entry["status"], "fail",
        "the fixture holds a critical-severity finding: {}",
        envelope["summary"]
    );
    assert_eq!(
        output.code, 1,
        "the published verdict and the process status agree: {}",
        output.stderr
    );
    assert_eq!(
        entry["threshold_label"], "critical",
        "the floor is recoverable from the entry alone"
    );
    assert!(
        entry["observed"].as_f64().is_some_and(|count| count >= 1.0),
        "the entry reports how many findings reached the floor"
    );
}

/// `--report-only` is an explicit request never to fail, and it returns before
/// any gate is consulted. The verdict still has to be published, or the flag
/// silently removes the only channel a consumer has, and every entry has to be
/// unenforced, or a job following the published contract fails a green build
/// whose own stderr says the gate stood down.
///
/// The gate has to be ARMED for this to mean anything: `--report-only` is
/// mutually exclusive with `--min-score` and `--min-severity`, so a bare
/// `--report-only` run arms nothing and emits no object at all. A rotted
/// baseline is the one gate that composes with it.
#[test]
fn report_only_publishes_every_verdict_and_enforces_none() {
    let project = complex_project();
    let root = project.path();
    let baseline = root.join("health-baseline.json");
    let baseline_arg = baseline.to_str().expect("utf8");
    let saved = run(&[
        "health",
        "--root",
        root.to_str().expect("utf8"),
        "--format",
        "json",
        "--quiet",
        "--complexity",
        "--save-baseline",
        baseline_arg,
    ]);
    assert!(
        saved.code == 0 || saved.code == 1,
        "saving a health baseline should not error: {}",
        saved.stderr
    );

    // Remove the findings the baseline recorded, so every entry goes stale.
    std::fs::write(
        root.join("src/complex.ts"),
        "export const classify = (): string => 'none';\n",
    )
    .expect("simplify the project");

    let output = run(&[
        "health",
        "--root",
        root.to_str().expect("utf8"),
        "--format",
        "json",
        "--quiet",
        "--complexity",
        "--baseline",
        baseline_arg,
        "--fail-on-stale-baseline",
        "--report-only",
    ]);
    assert_eq!(
        output.code, 0,
        "--report-only never fails, whatever the gate concluded: {}",
        output.stderr
    );
    let envelope = parse_json(&output);
    let entry = gate(&envelope, "stale-baseline");
    assert_eq!(
        entry["status"], "fail",
        "the verdict is published rather than hidden"
    );
    assert_eq!(
        entry["enforced"],
        Value::Bool(false),
        "a run told never to fail enforces nothing, so a consumer gating on \
         `status == fail && enforced` cannot fail this green build"
    );
    for (name, outcome) in envelope["gate_outcomes"]
        .as_object()
        .expect("the object is present")
    {
        assert_eq!(
            outcome["enforced"],
            Value::Bool(false),
            "no entry escapes the --report-only clamp: {name}"
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

/// The combined machine renderers collapse every gate but stale-baseline and
/// regression to exit 0. An entry claiming `enforced: true` on that path states
/// an exit the run cannot produce, which is the exact disagreement this object
/// was added to remove.
#[test]
fn the_combined_json_path_does_not_claim_an_exit_it_cannot_produce() {
    let project = cloned_project();
    let root = root_arg(&project);
    let output = run(&[
        "--root",
        root,
        "--format",
        "json",
        "--quiet",
        "--dupes-threshold",
        "1",
    ]);
    assert_eq!(
        output.code, 0,
        "the combined JSON path has never exited non-zero for duplication: {}",
        output.stderr
    );
    let combined = parse_json(&output);
    let entry = gate(&combined, "duplication-threshold");
    assert_eq!(entry["status"], "fail", "the threshold was still exceeded");
    assert_eq!(
        entry["enforced"],
        Value::Bool(false),
        "and the entry says so, so a consumer gating on `status == fail && enforced` \
         cannot fail this passing run"
    );

    // The standalone command does enforce the same gate, so the two differ in
    // `enforced` and agree on `status`.
    let standalone = run(&[
        "dupes",
        "--root",
        root,
        "--format",
        "json",
        "--quiet",
        "--threshold",
        "1",
    ]);
    assert_eq!(standalone.code, 1, "{}", standalone.stderr);
    let standalone_envelope = parse_json(&standalone);
    let standalone_entry = gate(&standalone_envelope, "duplication-threshold");
    assert_eq!(standalone_entry["status"], entry["status"]);
    assert_eq!(standalone_entry["enforced"], Value::Bool(true));
}

/// One envelope must not produce two surfaces that state opposite verdicts. The
/// pull-request comment is the one a reviewer reads, and it used to assert a
/// passing quality gate while the job summary rendered from the same file said
/// a gate had failed.
#[test]
fn the_pull_request_comment_carries_the_same_verdict_as_the_job_summary() {
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
    let saved = root.join("results.json");
    std::fs::write(&saved, &produced.stdout).expect("save the envelope");

    for format in [
        "github-summary",
        "github-annotations",
        "pr-comment-github",
        "pr-comment-gitlab",
        // Both review targets reach the conclusion-less arm on a dead-code
        // envelope, which used to drop the line entirely.
        "review-github",
        "review-gitlab",
    ] {
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
            "every render exits 0: {}",
            rendered.stderr
        );
        assert!(
            rendered.stdout.contains("Gate outcomes:") && rendered.stdout.contains("regression"),
            "{format} states the outcome the producing run reached: {}",
            rendered.stdout
        );
    }
}

/// The annotations stream is capped by the consumer (`head -n "$MAX"` in the
/// action), so a verdict appended after the findings is the first thing a noisy
/// run drops.
#[test]
fn the_annotation_verdict_comes_before_the_findings() {
    let project = orphan_project(6);
    let root = project.path();
    let output = run(&[
        "dead-code",
        "--root",
        root.to_str().expect("utf8"),
        "--format",
        "json",
        "--quiet",
        "--fail-on-issues",
    ]);
    let saved = root.join("results.json");
    std::fs::write(&saved, &output.stdout).expect("save the envelope");
    let rendered = run(&[
        "report",
        "--from",
        saved.to_str().expect("utf8"),
        "--root",
        root.to_str().expect("utf8"),
        "--quiet",
        "--format",
        "github-annotations",
    ]);
    let first = rendered
        .stdout
        .lines()
        .next()
        .expect("annotations rendered");
    assert!(
        first.starts_with("::notice::Fallow: Gate outcomes:"),
        "the verdict is the first line, so a cap cannot drop it: {first}"
    );
}

/// `scope_reasons` and `change_scoped` are one predicate's two projections, so
/// the array is non-empty exactly when the boolean is true. A consumer that
/// reads the array to decide whether the narrowing is removable would otherwise
/// be deciding from a different answer than the one that suppressed the gate.
fn assert_scope_projection_agrees(staleness: &Value, expected: &[&str]) {
    let reasons = staleness["scope_reasons"]
        .as_array()
        .map(|reasons| {
            reasons
                .iter()
                .map(|reason| reason.as_str().expect("a reason is a string"))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    assert_eq!(reasons, expected, "staleness was {staleness}");
    assert_eq!(
        staleness["change_scoped"],
        Value::Bool(!reasons.is_empty()),
        "change_scoped must follow the reason set: {staleness}"
    );
}

#[test]
fn dead_code_names_every_channel_that_narrowed_the_run() {
    let project = orphan_project(3);
    let root = project.path();
    let baseline = root.join("baseline.json");
    let baseline_arg = baseline.to_str().expect("utf8");
    let saved = run(&[
        "dead-code",
        "--root",
        root_arg(&project),
        "--format",
        "json",
        "--quiet",
        "--save-baseline",
        baseline_arg,
    ]);
    assert!(
        saved.code == 0 || saved.code == 1,
        "saving a baseline should not error: {}",
        saved.stderr
    );

    let unscoped = parse_json(&run(&[
        "dead-code",
        "--root",
        root_arg(&project),
        "--format",
        "json",
        "--quiet",
        "--baseline",
        baseline_arg,
    ]));
    assert_scope_projection_agrees(&unscoped["baseline_staleness"], &[]);
    assert!(
        unscoped["baseline_staleness"]
            .as_object()
            .expect("staleness is an object")
            .get("scope_reasons")
            .is_none(),
        "a whole-project run keeps the member off the wire entirely: {}",
        unscoped["baseline_staleness"]
    );

    let narrowed = parse_json(&run(&[
        "dead-code",
        "--root",
        root_arg(&project),
        "--format",
        "json",
        "--quiet",
        "--baseline",
        baseline_arg,
        "--production",
        "--unused-exports",
    ]));
    assert_scope_projection_agrees(
        &narrowed["baseline_staleness"],
        &["issue-type-filter", "production"],
    );
}

/// `dupes` compares and saves before the report-narrowing filters run, so its
/// predicate sees a resolved changed-file set and production mode and nothing
/// else. It reports `changed-files` rather than the flag that produced it,
/// because by then the flag is gone.
#[test]
fn dupes_reports_the_two_channels_that_narrow_its_comparison() {
    let project = cloned_project();
    let baseline = project.path().join("dupes-baseline.json");
    let baseline_arg = baseline.to_str().expect("utf8");
    let saved = run(&[
        "dupes",
        "--root",
        root_arg(&project),
        "--format",
        "json",
        "--quiet",
        "--save-baseline",
        baseline_arg,
    ]);
    assert!(
        saved.code == 0 || saved.code == 1,
        "saving a duplication baseline should not error: {}",
        saved.stderr
    );

    let unscoped = parse_json(&run(&[
        "dupes",
        "--root",
        root_arg(&project),
        "--format",
        "json",
        "--quiet",
        "--baseline",
        baseline_arg,
    ]));
    assert_scope_projection_agrees(&unscoped["baseline_staleness"], &[]);

    let narrowed = parse_json(&run(&[
        "dupes",
        "--root",
        root_arg(&project),
        "--format",
        "json",
        "--quiet",
        "--baseline",
        baseline_arg,
        "--production",
    ]));
    assert_scope_projection_agrees(&narrowed["baseline_staleness"], &["production"]);
}

/// `health` runs its predicate after the flags were resolved, so workspace
/// roots and a changed-file set have already lost the flag they came from.
#[test]
fn health_reports_the_coarser_channels_its_predicate_can_see() {
    let project = complex_project();
    let root = project.path();
    let baseline = root.join("health-baseline.json");
    let baseline_arg = baseline.to_str().expect("utf8");
    let saved = run(&[
        "health",
        "--root",
        root_arg(&project),
        "--format",
        "json",
        "--quiet",
        "--complexity",
        "--save-baseline",
        baseline_arg,
    ]);
    assert!(
        saved.code == 0 || saved.code == 1,
        "saving a health baseline should not error: {}",
        saved.stderr
    );

    let unscoped = parse_json(&run(&[
        "health",
        "--root",
        root_arg(&project),
        "--format",
        "json",
        "--quiet",
        "--complexity",
        "--baseline",
        baseline_arg,
    ]));
    assert_scope_projection_agrees(&unscoped["summary"]["baseline_staleness"], &[]);

    let narrowed = parse_json(&run(&[
        "health",
        "--root",
        root_arg(&project),
        "--format",
        "json",
        "--quiet",
        "--complexity",
        "--baseline",
        baseline_arg,
        "--production",
    ]));
    assert_scope_projection_agrees(&narrowed["summary"]["baseline_staleness"], &["production"]);
}

/// A rotted baseline on a project with nothing left to report is the run where
/// the advisory and the gate are both silent by construction, so the read-only
/// pointer has to survive the zero-finding early return.
#[test]
fn a_narrowed_run_with_no_findings_still_points_at_the_unscoped_recheck() {
    let project = orphan_project(2);
    let root = project.path();
    let baseline = root.join("baseline.json");
    let baseline_arg = baseline.to_str().expect("utf8");
    let saved = run(&[
        "dead-code",
        "--root",
        root_arg(&project),
        "--format",
        "json",
        "--quiet",
        "--save-baseline",
        baseline_arg,
    ]);
    assert!(
        saved.code == 0 || saved.code == 1,
        "saving a baseline should not error: {}",
        saved.stderr
    );

    // The positional path narrows to the entry point, which the baseline never
    // recorded, so the run reports nothing and matches nothing: the shape where
    // both the advisory and the gate are silent by construction.
    let output = run(&[
        "dead-code",
        "src/index.ts",
        "--root",
        root_arg(&project),
        "--format",
        "json",
        "--quiet",
        "--baseline",
        baseline_arg,
    ]);
    let envelope = parse_json(&output);
    assert_scope_projection_agrees(&envelope["baseline_staleness"], &["scope"]);

    let steps = envelope["next_steps"]
        .as_array()
        .unwrap_or_else(|| panic!("a narrowed baseline run offers a pointer: {envelope}"));
    let recheck = steps
        .iter()
        .find(|step| step["id"] == "recheck-baseline")
        .unwrap_or_else(|| panic!("expected a recheck-baseline entry, got {envelope}"));
    let command = recheck["command"].as_str().expect("command is a string");
    assert!(
        command.starts_with("fallow dead-code --baseline "),
        "the pointer names the command whose baseline it is: {command}"
    );
    assert!(
        command.ends_with("baseline.json"),
        "the pointer names the loaded baseline, root-relative like every other \
         path on the envelope: {command}"
    );
    assert!(
        !command.contains("--save-baseline"),
        "next_steps is a read-only contract: {command}"
    );
    assert!(
        recheck["reason"]
            .as_str()
            .expect("reason is a string")
            .contains("(scope)"),
        "the reason names the published scope_reasons: {recheck}"
    );
}

/// A baseline with no entries this command recognises suppresses nothing, and
/// every verdict that follows is green and honest: the advisory has nothing to
/// judge, so `gate_trips` is false and the gate reports `pass`. A repository
/// that pointed `--baseline` at a baseline another command saved, or at an
/// empty file, would gate on it forever and never be told.
///
/// Each command has its own format, and the three do not agree on what a
/// foreign file means: `dupes` and `health` give every field a serde default,
/// so any JSON object loads as zero entries, while `dead-code` rejects one
/// today because five of its fields carry no default. That asymmetry is not a
/// kind check, so the note is asserted on the commands that accept the file.
#[test]
fn a_dupes_run_says_so_when_the_baseline_is_another_commands() {
    let project = orphan_project(2);
    let dead_code_baseline = project.path().join("dead-code-baseline.json");
    let baseline_arg = dead_code_baseline.to_str().expect("utf8");
    let saved = run(&[
        "dead-code",
        "--root",
        root_arg(&project),
        "--format",
        "json",
        "--quiet",
        "--save-baseline",
        baseline_arg,
    ]);
    assert!(
        saved.code == 0 || saved.code == 1,
        "saving a dead-code baseline should not error: {}",
        saved.stderr
    );

    let output = run(&[
        "dupes",
        "--root",
        root_arg(&project),
        "--format",
        "json",
        "--quiet",
        "--baseline",
        baseline_arg,
        "--fail-on-stale-baseline",
    ]);
    let envelope = parse_json(&output);
    assert_eq!(
        envelope["baseline_staleness"]["baseline_entries"], 0,
        "the wrong-kind file loads as zero entries: {envelope}"
    );
    assert_eq!(
        gate(&envelope, "stale-baseline")["status"],
        "pass",
        "a baseline with nothing in it cannot report a stale entry: {envelope}"
    );
    assert!(
        output
            .stderr
            .contains("has no entries this command recognises"),
        "the fact must reach stderr even under --quiet, because it appears in \
         no human report and --ci implies --quiet: {}",
        output.stderr
    );
    assert_eq!(
        output.code, 0,
        "a legitimately empty baseline is a real state, so the exit code does \
         not move: {}",
        output.stderr
    );
}

#[test]
fn a_health_run_says_so_when_the_baseline_is_another_commands() {
    let project = cloned_project();
    let dupes_baseline = project.path().join("dupes-baseline.json");
    let baseline_arg = dupes_baseline.to_str().expect("utf8");
    let saved = run(&[
        "dupes",
        "--root",
        root_arg(&project),
        "--format",
        "json",
        "--quiet",
        "--save-baseline",
        baseline_arg,
    ]);
    assert!(
        saved.code == 0 || saved.code == 1,
        "saving a duplication baseline should not error: {}",
        saved.stderr
    );

    let output = run(&[
        "health",
        "--root",
        root_arg(&project),
        "--format",
        "json",
        "--quiet",
        "--complexity",
        "--baseline",
        baseline_arg,
    ]);
    let envelope = parse_json(&output);
    assert_eq!(
        envelope["summary"]["baseline_staleness"]["baseline_entries"], 0,
        "the wrong-kind file loads as zero entries: {envelope}"
    );
    assert!(
        output
            .stderr
            .contains("has no entries this command recognises"),
        "{}",
        output.stderr
    );
}

/// The note is about the baseline, not about this run, so a baseline that does
/// carry entries never earns it however the run turned out.
#[test]
fn a_baseline_with_entries_never_earns_the_zero_entry_note() {
    let project = orphan_project(3);
    let baseline = project.path().join("baseline.json");
    let baseline_arg = baseline.to_str().expect("utf8");
    let saved = run(&[
        "dead-code",
        "--root",
        root_arg(&project),
        "--format",
        "json",
        "--quiet",
        "--save-baseline",
        baseline_arg,
    ]);
    assert!(
        saved.code == 0 || saved.code == 1,
        "saving a baseline should not error: {}",
        saved.stderr
    );

    let output = run(&[
        "dead-code",
        "--root",
        root_arg(&project),
        "--format",
        "json",
        "--quiet",
        "--baseline",
        baseline_arg,
    ]);
    assert!(
        !output.stderr.contains("has no entries"),
        "a populated baseline says nothing about recognition: {}",
        output.stderr
    );
}

/// Save a dead-code baseline over `project`, then remove what it recorded, so
/// every entry goes unmatched with no current finding to compare against.
///
/// That pair is the shape #2675 is about: the advisory stays silent because a
/// cleaned project and a rotted baseline look identical from the counts, while
/// `--fail-on-stale-baseline` asks for exactly that case and its rule holds.
fn rotted_dead_code_baseline(project: &TempDir, orphans: usize) -> String {
    let root = project.path();
    let baseline = root.join("baseline.json");
    let baseline_arg = baseline.to_str().expect("utf8").to_owned();
    let saved = run(&[
        "dead-code",
        "--root",
        root_arg(project),
        "--format",
        "json",
        "--quiet",
        "--save-baseline",
        &baseline_arg,
    ]);
    assert!(
        saved.code == 0 || saved.code == 1,
        "saving a baseline should not error: {}",
        saved.stderr
    );
    for index in 0..orphans {
        std::fs::remove_file(root.join(format!("src/orphan{index}.ts")))
            .expect("clean the project");
    }
    baseline_arg
}

fn run_with_env(args: &[&str], env: &[(&str, &str)]) -> CommandOutput {
    let mut command = std::process::Command::new(common::fallow_bin());
    command.env("RUST_LOG", "").env("NO_COLOR", "1");
    common::scrub_coverage_env(&mut command);
    for arg in args {
        command.arg(arg);
    }
    for (name, value) in env {
        command.env(name, value);
    }
    let output = command.output().expect("run fallow");
    CommandOutput {
        stdout: String::from_utf8_lossy(&output.stdout).to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        code: output.status.code().unwrap_or(-1),
    }
}

fn decision_sidecar(path: &Path) -> Value {
    serde_json::from_slice(&std::fs::read(path).expect("read the decision sidecar"))
        .expect("parse the decision sidecar")
}

fn decision_gate<'a>(sidecar: &'a Value, id: &str) -> &'a Value {
    sidecar["gates"]
        .as_array()
        .expect("the gates array is present")
        .iter()
        .find(|gate| gate["id"] == id)
        .unwrap_or_else(|| panic!("expected a `{id}` row, got {}", sidecar["gates"]))
}

/// The headline ask of #2675: an armed and tripped baseline gate reaches the
/// Check Run as a named gate, and the sticky comment says what went stale.
#[test]
fn a_tripped_stale_baseline_gate_reaches_the_comment_and_the_decision_surface() {
    let project = orphan_project(3);
    let baseline_arg = rotted_dead_code_baseline(&project, 3);
    let sidecar_path = project.path().join("decision.json");

    let output = run_with_env(
        &[
            "dead-code",
            "--root",
            root_arg(&project),
            "--format",
            "pr-comment-github",
            "--quiet",
            "--baseline",
            &baseline_arg,
            "--fail-on-stale-baseline",
        ],
        &[(
            "FALLOW_PR_DECISION_FILE",
            sidecar_path.to_str().expect("utf8"),
        )],
    );

    assert!(
        output
            .stdout
            .contains("**Baseline has stale entries.** 3 of 3 saved entries matched nothing"),
        "the artefact a reviewer reads must say what went stale: {}",
        output.stdout
    );
    assert!(
        output
            .stdout
            .contains("Gate outcomes: failed stale-baseline"),
        "the gate inventory keeps its place beside the advisory: {}",
        output.stdout
    );

    let sidecar = decision_sidecar(&sidecar_path);
    let row = decision_gate(&sidecar, "stale-baseline");
    assert_eq!(row["label"], "Stale baseline");
    assert_eq!(row["status"], "failure");
    assert_eq!(
        row["scope"], "this run",
        "a baseline verdict is not scoped to the change and must not claim to be"
    );
    assert_eq!(
        sidecar["gates"][0]["id"], "dead-code",
        "the command row stays first"
    );
}

/// Published without the flag that arms it, the same verdict must not paint a
/// red gate: `enforced` is the CLI's statement about its own exit code, and a
/// repository that never asked for the gate has not configured a failure.
#[test]
fn an_unarmed_stale_baseline_gate_reaches_the_decision_surface_as_neutral() {
    let project = orphan_project(2);
    let baseline_arg = rotted_dead_code_baseline(&project, 2);
    let sidecar_path = project.path().join("decision.json");

    let output = run_with_env(
        &[
            "dead-code",
            "--root",
            root_arg(&project),
            "--format",
            "pr-comment-gitlab",
            "--quiet",
            "--baseline",
            &baseline_arg,
        ],
        &[(
            "FALLOW_PR_DECISION_FILE",
            sidecar_path.to_str().expect("utf8"),
        )],
    );
    assert_eq!(output.code, 0, "no gate was armed: {}", output.stderr);

    let sidecar = decision_sidecar(&sidecar_path);
    assert_eq!(
        decision_gate(&sidecar, "stale-baseline")["status"],
        "neutral"
    );
    assert!(
        output.stdout.contains("**Baseline has stale entries.**"),
        "the advisory is about the baseline, not about the exit code: {}",
        output.stdout
    );
}

/// A run whose baseline is fresh says nothing about staleness, so the clause is
/// conditional rather than always present.
#[test]
fn a_fresh_baseline_adds_no_advisory_to_the_comment() {
    let project = orphan_project(2);
    let baseline = project.path().join("baseline.json");
    let baseline_arg = baseline.to_str().expect("utf8");
    let saved = run(&[
        "dead-code",
        "--root",
        root_arg(&project),
        "--format",
        "json",
        "--quiet",
        "--save-baseline",
        baseline_arg,
    ]);
    assert!(saved.code == 0 || saved.code == 1, "{}", saved.stderr);

    let output = run(&[
        "dead-code",
        "--root",
        root_arg(&project),
        "--format",
        "pr-comment-github",
        "--quiet",
        "--baseline",
        baseline_arg,
    ]);

    assert!(
        !output.stdout.contains("Baseline"),
        "a baseline that matched everything earns no advisory: {}",
        output.stdout
    );
}

/// The check-run `conclusion` is a documented non-blocker. Appending a failing
/// gate row must not turn a combined run that armed a gate into a merge
/// blocker for every consumer with a required check.
#[test]
fn a_tripped_gate_row_does_not_move_the_combined_check_run_conclusion() {
    let project = orphan_project(3);
    let baseline_arg = rotted_dead_code_baseline(&project, 3);
    let sidecar_path = project.path().join("decision.json");

    let output = run_with_env(
        &[
            "--root",
            root_arg(&project),
            "--format",
            "pr-comment-github",
            "--quiet",
            "--baseline",
            &baseline_arg,
            "--fail-on-stale-baseline",
        ],
        &[(
            "FALLOW_PR_DECISION_FILE",
            sidecar_path.to_str().expect("utf8"),
        )],
    );
    assert!(
        output.code == 0 || output.code == 1,
        "combined should not error: {}",
        output.stderr
    );

    let sidecar = decision_sidecar(&sidecar_path);
    assert_eq!(
        decision_gate(&sidecar, "stale-baseline")["status"],
        "failure"
    );
    assert_ne!(
        sidecar["conclusion"], "failure",
        "the conclusion stays derived from the per-area rows: {}",
        sidecar["gates"]
    );
}
