#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests and benches use unwrap and expect to keep fixture setup concise"
)]

use crate::common::run_fallow_combined;

#[test]
fn combined_human_output_hides_internal_info_logs_when_rust_log_is_empty() {
    let output = run_fallow_combined("basic-project", &["--summary"]);
    assert_ne!(
        output.code, 2,
        "combined run should not hard-fail: stdout={} stderr={}",
        output.stdout, output.stderr
    );

    let combined = format!("{}\n{}", output.stdout, output.stderr);
    assert!(
        !combined.contains("active plugins"),
        "human output should not leak plugin tracing: {combined}"
    );
    assert!(
        !combined.contains("incremental cache stats"),
        "human output should not leak cache tracing: {combined}"
    );
    assert!(
        !combined.contains(" INFO ")
            && !combined.contains(" DEBUG ")
            && !combined.contains(" TRACE "),
        "human output should stay free of tracing levels: {combined}"
    );
    assert!(
        output.stderr.contains("Dead Code") || output.stderr.contains("■ Metrics"),
        "expected the normal combined human report on stderr: {}",
        output.stderr
    );
}

#[test]
fn combined_human_summary_logs_loaded_config_once() {
    let output = run_fallow_combined("config-file-project", &["--summary"]);
    assert_ne!(
        output.code, 2,
        "combined run should not hard-fail: stdout={} stderr={}",
        output.stdout, output.stderr
    );

    let combined = format!("{}\n{}", output.stdout, output.stderr);
    assert_eq!(
        combined.matches("loaded config:").count(),
        1,
        "combined mode should mention the loaded config once: {combined}"
    );
}

#[test]
fn combined_human_summary_uses_section_headers_without_duplicate_summary_titles() {
    let output = run_fallow_combined("config-file-project", &["--summary"]);
    assert_ne!(
        output.code, 2,
        "combined run should not hard-fail: stdout={} stderr={}",
        output.stdout, output.stderr
    );

    assert!(
        output.stderr.contains("── Dead Code"),
        "combined summary should keep the section header: {}",
        output.stderr
    );
    assert!(
        !output.stdout.contains("Dead Code Summary"),
        "combined summary should not duplicate the section title in stdout: {}",
        output.stdout
    );
}

/// Editing a file and re-running is the ordinary way to use fallow, so the
/// cache miss it causes must not be a warning.
///
/// Every edit-then-run cycle printed `WARN Graph cache decoded but not reused:
/// at least one file changed`, `fallow watch` printed it once per save, and
/// `--quiet` did not suppress it. Both content-drift reasons stay reported by
/// `fallow doctor` and the `--performance` table.
#[test]
fn a_content_change_between_runs_is_not_warned_about() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"cache-drift","main":"src/index.ts"}"#,
    )
    .unwrap();
    std::fs::write(
        root.join("src/index.ts"),
        "import { used } from './lib';\nused();\n",
    )
    .unwrap();
    std::fs::write(
        root.join("src/lib.ts"),
        "export const used = (): number => 1;\n",
    )
    .unwrap();

    let warm = crate::common::run_fallow_in_root("dead-code", root, &["--quiet"]);
    assert_ne!(warm.code, 2, "stderr:\n{}", warm.stderr);

    std::fs::write(
        root.join("src/lib.ts"),
        "export const used = (): number => 2;\n",
    )
    .unwrap();
    let after_edit = crate::common::run_fallow_in_root("dead-code", root, &["--quiet"]);
    assert_ne!(after_edit.code, 2, "stderr:\n{}", after_edit.stderr);

    assert!(
        !after_edit.stderr.contains("decoded but not reused"),
        "a cache doing exactly what it should must not warn; stderr:\n{}",
        after_edit.stderr
    );
    assert!(
        !after_edit.stderr.contains("WARN"),
        "an edit-then-run cycle produces no warning at all; stderr:\n{}",
        after_edit.stderr
    );

    // Silencing the warning must not lose the measurement: the reason stays
    // where a reader goes looking for it.
    std::fs::write(
        root.join("src/lib.ts"),
        "export const used = (): number => 3;\n",
    )
    .unwrap();
    let perf = crate::common::run_fallow_in_root("dead-code", root, &["--performance"]);
    assert!(
        perf.stderr
            .contains("graph cache not reused: at least one file changed"),
        "the performance table must still name the refusal; stderr:\n{}",
        perf.stderr
    );
}
