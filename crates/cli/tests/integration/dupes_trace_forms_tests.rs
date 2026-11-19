//! `fallow dupes --trace` accepts a location (`FILE:LINE`) and a clone group
//! fingerprint (`dup:<fingerprint>`). The help and the input error name both
//! forms (issue #4830).

use crate::common::{run_fallow, run_fallow_raw};

#[test]
fn dupes_trace_help_shows_both_accepted_forms() {
    let help = run_fallow_raw(&["dupes", "--help"]).stdout;

    assert!(
        help.contains("--trace <FILE:LINE|dup:<fingerprint>>"),
        "{help}"
    );
    assert!(help.contains("`dup:f42424e8`"), "{help}");
}

#[test]
fn dupes_trace_input_error_names_both_forms() {
    let output = run_fallow(
        "dupes",
        "basic-project",
        &["--trace", "src/index.ts", "--format", "json", "--quiet"],
    );

    assert_eq!(output.code, 2, "stderr: {}", output.stderr);
    assert!(output.stdout.contains("FILE:LINE"), "{}", output.stdout);
    assert!(
        output.stdout.contains("dup:<fingerprint>"),
        "{}",
        output.stdout
    );
}
