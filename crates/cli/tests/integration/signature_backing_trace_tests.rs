#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests use unwrap and expect to keep fixture setup concise"
)]

//! The trace of a type that only a same-file exported signature uses gives the
//! same verdict as the `unused-type` report (issue #5280).

use std::path::Path;

use crate::common::{copy_fixture, parse_json, run_fallow_in_root};

const FIXTURE: &str = "signature-backing-types";

fn unused_type_names(root: &Path, extra: &[&str]) -> Vec<String> {
    let mut args = vec!["--format", "json", "--quiet", "--no-cache"];
    args.extend_from_slice(extra);
    let json = parse_json(&run_fallow_in_root("dead-code", root, &args));
    let mut names: Vec<String> = json["unused_types"]
        .as_array()
        .expect("unused_types array")
        .iter()
        .map(|finding| finding["export_name"].as_str().unwrap().to_owned())
        .collect();
    names.sort();
    names
}

fn trace(root: &Path, target: &str, extra: &[&str]) -> serde_json::Value {
    let mut args = vec![
        "--trace",
        target,
        "--format",
        "json",
        "--quiet",
        "--no-cache",
    ];
    args.extend_from_slice(extra);
    parse_json(&run_fallow_in_root("dead-code", root, &args))
}

#[test]
fn reported_signature_type_traces_as_unused() {
    let dir = copy_fixture(FIXTURE);
    assert!(unused_type_names(dir.path(), &[]).contains(&"UsedInSignature".to_owned()));

    let json = trace(dir.path(), "src/lib.ts:UsedInSignature", &[]);
    assert_eq!(json["is_used"], false);
    assert!(json.get("kept_by_signature").is_none(), "{json}");
    assert_eq!(json["reason"], "No references found, export is unused");
}

#[test]
fn declaration_emit_keeps_the_type_in_report_and_trace() {
    let dir = copy_fixture(FIXTURE);
    std::fs::write(
        dir.path().join("tsconfig.json"),
        r#"{ "compilerOptions": { "declaration": true } }"#,
    )
    .expect("write tsconfig");
    assert!(!unused_type_names(dir.path(), &[]).contains(&"UsedAsGenericArg".to_owned()));

    let json = trace(dir.path(), "src/lib.ts:UsedAsGenericArg", &[]);
    assert_eq!(json["is_used"], false);
    assert_eq!(
        json["kept_by_signature"],
        serde_json::json!({
            "backers": ["usedGeneric"],
            "reason": { "kind": "declaration_emit", "config": "tsconfig.json" }
        })
    );
    assert_eq!(
        json["reason"],
        "No references found, but the signature of exported `usedGeneric` uses the type and \
         tsconfig.json emits declarations, so unused-type keeps the export"
    );
}

#[test]
fn private_type_leak_rule_keeps_the_type_in_report_and_trace() {
    let dir = copy_fixture(FIXTURE);
    std::fs::write(
        dir.path().join(".fallowrc.json"),
        r#"{ "rules": { "private-type-leaks": "warn" } }"#,
    )
    .expect("write config");
    assert!(!unused_type_names(dir.path(), &[]).contains(&"UsedInSignature".to_owned()));

    let json = trace(dir.path(), "src/lib.ts:UsedInSignature", &[]);
    assert_eq!(
        json["kept_by_signature"],
        serde_json::json!({
            "backers": ["used"],
            "reason": { "kind": "private_type_leak" }
        })
    );
}
