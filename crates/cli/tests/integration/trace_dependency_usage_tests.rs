//! `fallow trace --dependency <PKG>` reports how the code uses each imported
//! name of a package. `fallow dead-code --trace-dependency <PKG>` keeps its
//! old output on the same fixture.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "integration tests keep fixture setup concise"
)]

use crate::common::{parse_json, run_fallow};
use serde_json::Value;

const FIXTURE: &str = "trace-dependency-usage";
const PACKAGE: &str = "react-redux";

/// Drop the run metadata, which changes on each run.
fn without_meta(mut value: Value) -> Value {
    if let Some(object) = value.as_object_mut() {
        object.remove("_meta");
    }
    value
}

fn dead_code_trace(extra: &[&str]) -> Value {
    let mut args = vec![
        "--trace-dependency",
        PACKAGE,
        "--format",
        "json",
        "--quiet",
        "--no-cache",
    ];
    args.extend_from_slice(extra);
    without_meta(parse_json(&run_fallow("dead-code", FIXTURE, &args)))
}

#[test]
fn dead_code_trace_dependency_output_is_unchanged() {
    let trace = dead_code_trace(&[]);
    insta::assert_snapshot!(
        "trace_dependency_usage_dead_code_json",
        serde_json::to_string_pretty(&trace).unwrap()
    );
}
