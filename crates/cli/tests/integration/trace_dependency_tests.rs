//! `--trace-dependency` lists each importing file once, however many import
//! specifiers or statements the file has, and marks a file as type-only only
//! when every import of the package in that file is type-only.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests and benches use unwrap and expect to keep fixture setup concise"
)]

use crate::common::{parse_json, run_fallow};
use serde_json::{Value, json};

const FIXTURE: &str = "trace-dependency-repeated-imports";

fn trace(package: &str) -> Value {
    parse_json(&run_fallow(
        "dead-code",
        FIXTURE,
        &[
            "--trace-dependency",
            package,
            "--format",
            "json",
            "--quiet",
            "--no-cache",
        ],
    ))
}

#[test]
fn each_importing_file_is_listed_once() {
    let trace = trace("state-kit");
    assert_eq!(
        trace["imported_by"],
        json!(["src/index.ts", "src/mixed.ts", "src/types-only.ts"]),
        "{trace:#}"
    );
    assert_eq!(trace["import_count"], 3, "{trace:#}");
}

#[test]
fn a_file_with_a_value_import_is_not_type_only() {
    let trace = trace("state-kit");
    assert_eq!(
        trace["type_only_imported_by"],
        json!(["src/types-only.ts"]),
        "{trace:#}"
    );
}
