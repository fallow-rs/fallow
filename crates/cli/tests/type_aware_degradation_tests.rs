#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests and benches use unwrap and expect to keep fixture setup concise"
)]

//! A type-aware pass that cannot run must not fail the whole command under the
//! default `best-effort` policy. Syntactic analysis reports a superset of the
//! refined findings, so continuing is the conservative outcome. Only
//! `--type-aware-require complete` still exits 2.

#[path = "common/mod.rs"]
mod common;

use common::{CommandOutput, fixture_path, parse_json, run_fallow_raw_with_env};

const FIXTURE: &str = "type-aware-unused-export-refinement";
const DEGRADED_NOTICE: &str = "showing conservative syntactic findings";

/// Point the sidecar resolution at a path that does not exist, which is the
/// same failure class the transport reports for a timeout or a crash.
fn run_with_missing_companion(args: &[&str]) -> CommandOutput {
    let root = fixture_path(FIXTURE);
    let missing = root.join("missing-type-aware-companion");
    let missing_arg = missing.to_string_lossy().to_string();
    let root_arg = root.to_string_lossy().to_string();
    let mut full = vec![args[0], "--root", root_arg.as_str()];
    full.extend_from_slice(&args[1..]);
    run_fallow_raw_with_env(&full, &[("FALLOW_TYPE_AWARE_BIN", &missing_arg)])
}

#[test]
fn check_degrades_to_syntactic_findings_under_best_effort() {
    let output = run_with_missing_companion(&[
        "dead-code",
        "--type-aware",
        "--unused-exports",
        "--format",
        "json",
        "--quiet",
    ]);

    assert_ne!(output.code, 2, "stderr: {}", output.stderr);
    let json = parse_json(&output);
    let warnings = json["_meta"]["type_aware"]["warnings"]
        .as_array()
        .expect("degraded run records the reason in _meta.type_aware.warnings");
    assert_eq!(json["_meta"]["type_aware"]["warning_count"], 1);
    assert_eq!(json["_meta"]["type_aware"]["executed"], false);
    assert!(
        warnings[0].as_str().unwrap().contains(DEGRADED_NOTICE),
        "warnings: {warnings:?}"
    );
    assert!(
        json["unused_exports"]
            .as_array()
            .is_some_and(|v| !v.is_empty()),
        "syntactic findings should survive: {}",
        output.stdout
    );
}

#[test]
fn check_still_fails_closed_under_require_complete() {
    let output = run_with_missing_companion(&[
        "dead-code",
        "--type-aware",
        "--type-aware-require",
        "complete",
        "--unused-exports",
        "--format",
        "json",
        "--quiet",
    ]);

    assert_eq!(output.code, 2, "stdout: {}", output.stdout);
    assert!(
        output.stdout.contains("Type-aware analysis failed"),
        "stdout: {}",
        output.stdout
    );
}

/// `fix` is the one command that does not degrade. The reporting commands can
/// fall back to the syntactic set because it is a superset, so a gate only gets
/// stricter. Applying that same superset from a command that removes code would
/// delete the entries a working type-aware pass would have proven live, which is
/// the opposite of conservative.
#[test]
fn fix_dry_run_fails_closed_even_under_best_effort() {
    let output = run_with_missing_companion(&[
        "fix",
        "--type-aware",
        "--dry-run",
        "--format",
        "json",
        "--quiet",
    ]);

    assert_eq!(output.code, 2, "stdout: {}", output.stdout);
    assert!(
        output.stdout.contains("Type-aware analysis failed"),
        "stdout: {}",
        output.stdout
    );
}

#[test]
fn fix_dry_run_still_fails_closed_under_require_complete() {
    let output = run_with_missing_companion(&[
        "fix",
        "--type-aware",
        "--type-aware-require",
        "complete",
        "--dry-run",
        "--format",
        "json",
        "--quiet",
    ]);

    assert_eq!(output.code, 2, "stdout: {}", output.stdout);
    assert!(
        output.stdout.contains("Type-aware analysis failed"),
        "stdout: {}",
        output.stdout
    );
}

#[test]
fn health_type_coupling_degrades_under_best_effort() {
    let output = run_with_missing_companion(&[
        "health",
        "--type-aware",
        "--type-coupling",
        "--format",
        "json",
        "--quiet",
    ]);

    assert_ne!(output.code, 2, "stderr: {}", output.stderr);
    let json = parse_json(&output);
    let warnings = json["_meta"]["type_aware"]["warnings"]
        .as_array()
        .expect("degraded coupling records the reason in _meta.type_aware.warnings");
    assert!(
        warnings[0].as_str().unwrap().contains(DEGRADED_NOTICE),
        "warnings: {warnings:?}"
    );
}

#[test]
fn health_type_coupling_still_fails_closed_under_require_complete() {
    let output = run_with_missing_companion(&[
        "health",
        "--type-aware",
        "--type-coupling",
        "--type-aware-require",
        "complete",
        "--format",
        "json",
        "--quiet",
    ]);

    assert_eq!(output.code, 2, "stdout: {}", output.stdout);
    assert!(
        output.stdout.contains("Type-aware coupling failed"),
        "stdout: {}",
        output.stdout
    );
}

#[test]
fn combined_run_degrades_under_best_effort() {
    let output = run_with_missing_companion(&[
        "--type-aware",
        "--only",
        "health",
        "--format",
        "json",
        "--quiet",
    ]);

    assert_ne!(output.code, 2, "stderr: {}", output.stderr);
    assert!(
        !output.stdout.contains("Type-aware coupling failed"),
        "stdout: {}",
        output.stdout
    );
}

#[test]
fn combined_run_still_fails_closed_under_require_complete() {
    let output = run_with_missing_companion(&[
        "--type-aware",
        "--type-aware-require",
        "complete",
        "--only",
        "health",
        "--format",
        "json",
        "--quiet",
    ]);

    assert_eq!(output.code, 2, "stdout: {}", output.stdout);
    assert!(
        output.stdout.contains("Type-aware coupling failed"),
        "stdout: {}",
        output.stdout
    );
}
