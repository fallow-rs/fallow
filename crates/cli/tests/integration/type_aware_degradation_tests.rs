#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests and benches use unwrap and expect to keep fixture setup concise"
)]

//! A type-aware pass that cannot run must not fail the whole command under the
//! default `best-effort` policy. Syntactic analysis reports a superset of the
//! refined findings, so continuing is the conservative outcome. Only
//! `--type-aware-require complete` still exits 2.

use crate::common::{CommandOutput, fixture_path, parse_json, run_fallow_raw_with_env};

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

/// The notice is the only indication that the report has no type-aware refinement.
#[test]
fn human_check_states_the_degradation_once_and_claims_no_success() {
    let output = run_with_missing_companion(&["dead-code", "--type-aware", "--unused-exports"]);
    let emitted = format!("{}{}", output.stdout, output.stderr);

    assert_eq!(
        emitted.matches(DEGRADED_NOTICE).count(),
        1,
        "stdout: {}\nstderr: {}",
        output.stdout,
        output.stderr
    );
    assert!(
        !emitted.contains("Type-aware refinement:"),
        "a pass that never ran must not report counts: {emitted}"
    );
}

#[test]
fn human_health_coupling_states_the_degradation_once() {
    let output = run_with_missing_companion(&["health", "--type-aware", "--type-coupling"]);
    let emitted = format!("{}{}", output.stdout, output.stderr);

    assert_eq!(
        emitted.matches(DEGRADED_NOTICE).count(),
        1,
        "stdout: {}\nstderr: {}",
        output.stdout,
        output.stderr
    );
}

/// `--quiet` has to silence the notice on every stream while the machine-facing
/// copy in `_meta` survives, so a JSON consumer still learns the report is
/// unrefined.
#[test]
fn quiet_json_keeps_the_degradation_out_of_stderr() {
    let output = run_with_missing_companion(&[
        "dead-code",
        "--type-aware",
        "--unused-exports",
        "--format",
        "json",
        "--quiet",
    ]);

    assert!(
        !output.stderr.contains(DEGRADED_NOTICE),
        "stderr: {}",
        output.stderr
    );
    let json = parse_json(&output);
    assert!(
        json["_meta"]["type_aware"]["warnings"][0]
            .as_str()
            .is_some_and(|warning| warning.contains(DEGRADED_NOTICE)),
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
    // `fix` is the only surface that can still stop, so it is the only place a
    // user can get stuck, and the error has to name both ways out.
    assert!(
        output.stdout.contains("FALLOW_TYPE_AWARE_TIMEOUT_SECS"),
        "stdout: {}",
        output.stdout
    );
    assert!(
        output.stdout.contains("--no-type-aware"),
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

/// An unknown backend selection is an input error. The run must name the known
/// families before it looks for a companion, so a missing companion does not
/// hide the cause.
#[test]
fn unknown_backend_selection_fails_before_companion_lookup() {
    let root = fixture_path(FIXTURE);
    let missing = root.join("missing-type-aware-companion");
    let missing_arg = missing.to_string_lossy().to_string();
    let root_arg = root.to_string_lossy().to_string();
    let output = run_fallow_raw_with_env(
        &[
            "dead-code",
            "--root",
            root_arg.as_str(),
            "--type-aware",
            "--type-aware-require",
            "complete",
            "--unused-exports",
            "--format",
            "json",
            "--quiet",
        ],
        &[
            ("FALLOW_TYPE_AWARE_BIN", &missing_arg),
            ("FALLOW_TYPE_AWARE_BACKEND", "tsc"),
        ],
    );

    assert_eq!(output.code, 2, "stdout: {}", output.stdout);
    assert!(
        output.stdout.contains(
            "unknown FALLOW_TYPE_AWARE_BACKEND value `tsc`; expected one of: typescript-go, tsrs"
        ),
        "stdout: {}",
        output.stdout
    );
    assert!(
        !output.stdout.contains("missing-type-aware-companion"),
        "stdout: {}",
        output.stdout
    );
}

const MISSING_SIDECAR_FIXTURE: &str = "type-aware-missing-sidecar";

/// Run `dead-code` on the fixture with no `FALLOW_TYPE_AWARE_BIN`, so the
/// discovery looks only next to the test binary, where no sidecar exists.
fn run_without_sidecar(extra_env: &[(&str, &str)], args: &[&str]) -> CommandOutput {
    let mut full = vec!["dead-code"];
    full.extend_from_slice(args);
    run_in_root_without_sidecar(&fixture_path(MISSING_SIDECAR_FIXTURE), extra_env, &full)
}

/// Run Fallow with `--root root` and then `args`, with no sidecar. `args`
/// starts with the subcommand, or with a flag for the bare run.
fn run_in_root_without_sidecar(
    root: &std::path::Path,
    extra_env: &[(&str, &str)],
    args: &[&str],
) -> CommandOutput {
    let mut cmd = std::process::Command::new(crate::common::fallow_bin());
    cmd.env("RUST_LOG", "").env("NO_COLOR", "1");
    crate::common::scrub_analysis_env(&mut cmd);
    cmd.env_remove("FALLOW_TYPE_AWARE_BIN")
        .env_remove("FALLOW_TYPE_AWARE_SCRIPT")
        .envs(extra_env.iter().copied())
        .arg("--root")
        .arg(root)
        .args(args);
    let output = cmd.output().expect("failed to run fallow binary");
    CommandOutput {
        stdout: String::from_utf8_lossy(&output.stdout).to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        code: output.status.code().unwrap_or(-1),
    }
}

fn class_member_severities(json: &serde_json::Value) -> Vec<String> {
    json["unused_class_members"]
        .as_array()
        .expect("unused_class_members array")
        .iter()
        .map(|finding| {
            finding["effective_severity"]
                .as_str()
                .unwrap_or_default()
                .to_owned()
        })
        .collect()
}

/// A missing sidecar must not fail the gate on a finding that only the
/// type-aware pass can disprove (#4606). The finding stays in the report at
/// `warn`, and `gate_outcomes` states why the findings did not gate.
#[test]
fn missing_sidecar_reports_refinable_findings_as_warn() {
    let output = run_without_sidecar(&[], &["--format", "json", "--quiet"]);

    assert_eq!(
        output.code, 0,
        "stdout: {}\nstderr: {}",
        output.stdout, output.stderr
    );
    let json = parse_json(&output);
    assert_eq!(json["_meta"]["type_aware"]["executed"], false);
    assert!(
        json["_meta"]["type_aware"]["warnings"][0]
            .as_str()
            .is_some_and(|warning| warning.contains("was not found next to the active Fallow")),
        "{}",
        output.stdout
    );
    assert_eq!(class_member_severities(&json), vec!["warn".to_owned()]);
    let fallback = &json["gate_outcomes"]["type-aware-fallback"];
    assert_eq!(fallback["status"], "skipped", "{}", json["gate_outcomes"]);
    assert_eq!(fallback["enforced"], false);
    assert_eq!(
        json["gate_outcomes"]["error-severity-findings"]["status"],
        "pass"
    );
}

/// A sidecar that starts and then fails is not a missing sidecar. The run keeps
/// the conservative syntactic findings at their configured severity.
#[cfg(unix)]
#[test]
fn failed_sidecar_keeps_refinable_findings_at_error() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().expect("temp dir");
    let sidecar = dir.path().join("fallow-type-aware");
    std::fs::write(&sidecar, "#!/bin/sh\nexit 3\n").expect("write sidecar");
    std::fs::set_permissions(&sidecar, std::fs::Permissions::from_mode(0o755))
        .expect("chmod sidecar");
    let sidecar_arg = sidecar.to_string_lossy().to_string();

    let output = run_without_sidecar(
        &[("FALLOW_TYPE_AWARE_BIN", &sidecar_arg)],
        &["--format", "json", "--quiet"],
    );

    assert_eq!(
        output.code, 1,
        "stdout: {}\nstderr: {}",
        output.stdout, output.stderr
    );
    let json = parse_json(&output);
    assert_eq!(json["_meta"]["type_aware"]["executed"], false);
    assert_eq!(class_member_severities(&json), vec!["error".to_owned()]);
    assert!(
        json["gate_outcomes"].get("type-aware-fallback").is_none(),
        "{}",
        json["gate_outcomes"]
    );
}

/// `require: complete` keeps the hard failure for a missing sidecar.
#[test]
fn missing_sidecar_still_fails_under_require_complete() {
    let output = run_without_sidecar(
        &[],
        &[
            "--type-aware-require",
            "complete",
            "--format",
            "json",
            "--quiet",
        ],
    );
    assert_eq!(output.code, 2, "stdout: {}", output.stdout);
}

/// A copy of the missing-sidecar fixture with an unused file and the given
/// `rules`. The unused file is a finding of a normal kind, so a test can
/// check that the `warn` cap does not change other rules.
fn missing_sidecar_copy(rules: &str) -> tempfile::TempDir {
    let dir = crate::common::copy_fixture(MISSING_SIDECAR_FIXTURE);
    std::fs::write(
        dir.path().join(".fallowrc.json"),
        format!(
            r#"{{ "typeAware": {{ "enabled": true, "projects": ["tsconfig.json"] }}, "rules": {rules} }}"#
        ),
    )
    .expect("write config");
    std::fs::write(
        dir.path().join("src/orphan.ts"),
        "export const orphan = 1;\n",
    )
    .expect("write orphan file");
    dir
}

fn severities_of(json: &serde_json::Value, key: &str) -> Vec<String> {
    json[key]
        .as_array()
        .unwrap_or_else(|| panic!("{key} array"))
        .iter()
        .map(|finding| {
            finding["effective_severity"]
                .as_str()
                .unwrap_or_default()
                .to_owned()
        })
        .collect()
}

/// `--fail-on-issues` promotes `warn` findings to `error`. Without a sidecar,
/// the `warn` cap of the kinds that the type-aware pass can disprove wins over
/// that promotion. Every other `warn` rule is still promoted.
#[test]
fn missing_sidecar_cap_wins_over_fail_on_issues() {
    let dir = missing_sidecar_copy(r#"{ "unused-files": "off" }"#);
    let output = run_in_root_without_sidecar(
        dir.path(),
        &[],
        &[
            "dead-code",
            "--format",
            "json",
            "--quiet",
            "--fail-on-issues",
        ],
    );
    assert_eq!(
        output.code, 0,
        "stdout: {}\nstderr: {}",
        output.stdout, output.stderr
    );
    let json = parse_json(&output);
    assert_eq!(
        severities_of(&json, "unused_class_members"),
        vec!["warn".to_owned()]
    );

    let dir = missing_sidecar_copy(r#"{ "unused-files": "warn" }"#);
    let output = run_in_root_without_sidecar(
        dir.path(),
        &[],
        &[
            "dead-code",
            "--format",
            "json",
            "--quiet",
            "--fail-on-issues",
        ],
    );
    assert_eq!(
        output.code, 1,
        "a normal warn rule is still promoted: {}",
        output.stdout
    );
    let json = parse_json(&output);
    assert_eq!(
        severities_of(&json, "unused_class_members"),
        vec!["warn".to_owned()]
    );
    assert_eq!(
        severities_of(&json, "unused_files"),
        vec!["error".to_owned()]
    );
    assert_eq!(
        json["gate_outcomes"]["error-severity-findings"]["observed"],
        1.0
    );
}

/// `--ci` selects SARIF and implies `--fail-on-issues` on the bare run.
#[test]
fn missing_sidecar_cap_wins_over_ci_on_the_bare_run() {
    let dir = missing_sidecar_copy(r#"{ "unused-files": "off" }"#);
    let output = run_in_root_without_sidecar(dir.path(), &[], &["--ci"]);
    assert_eq!(
        output.code, 0,
        "stdout: {}\nstderr: {}",
        output.stdout, output.stderr
    );

    let dir = missing_sidecar_copy(r#"{ "unused-files": "warn" }"#);
    let output = run_in_root_without_sidecar(dir.path(), &[], &["--ci"]);
    assert_eq!(
        output.code, 1,
        "a normal warn rule is still promoted: {}",
        output.stdout
    );
}

/// The type-aware pass removes each private-type-leak candidate that the
/// checker does not confirm, so a missing sidecar lowers these findings too.
#[test]
fn missing_sidecar_lowers_private_type_leaks() {
    let dir = missing_sidecar_copy(r#"{ "unused-files": "off", "private-type-leaks": "error" }"#);
    std::fs::write(
        dir.path().join("src/leak.ts"),
        "type Hidden = { value: string };\n\nexport function makeHidden(): Hidden {\n  return { value: \"x\" };\n}\n",
    )
    .expect("write leak file");
    std::fs::write(
        dir.path().join("src/index.ts"),
        format!(
            "export {{ makeHidden }} from \"./leak\";\n{}",
            std::fs::read_to_string(dir.path().join("src/index.ts")).expect("read index")
        ),
    )
    .expect("write index");

    let output = run_in_root_without_sidecar(
        dir.path(),
        &[],
        &["dead-code", "--format", "json", "--quiet"],
    );
    assert_eq!(
        output.code, 0,
        "stdout: {}\nstderr: {}",
        output.stdout, output.stderr
    );
    let json = parse_json(&output);
    assert_eq!(
        severities_of(&json, "private_type_leaks"),
        vec!["warn".to_owned()]
    );
}
