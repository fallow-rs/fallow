//! `--trace-dependency` agrees with the unused devDependency report on plugin
//! tooling: a dependency the report credits traces as used and names why, and
//! a dependency the report flags traces as unused.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests and benches use unwrap and expect to keep fixture setup concise"
)]

use crate::common::{fixture_path, parse_json, run_fallow, run_fallow_in_root};
use serde_json::{Value, json};

const FIXTURE: &str = "plugin-tooling-credit";

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
fn a_tooling_dependency_without_evidence_traces_as_unused() {
    let karma = trace("karma");
    assert_eq!(karma["is_used"], false, "{karma:#}");
    assert!(karma.get("tooling_credit").is_none(), "{karma:#}");
}

#[test]
fn a_plugin_config_credit_names_the_config_file() {
    let c8 = trace("c8");
    assert_eq!(c8["is_used"], true, "{c8:#}");
    assert_eq!(
        c8["tooling_credit"],
        json!({ "reason": "plugin-config", "plugin": "c8", "config": ".c8rc.json" }),
        "{c8:#}"
    );
}

#[test]
fn a_plugin_reference_credit_names_the_invoked_package() {
    let companion = trace("ts-mocha");
    assert_eq!(companion["is_used"], true, "{companion:#}");
    assert_eq!(
        companion["tooling_credit"],
        json!({ "reason": "plugin-reference", "plugin": "mocha", "reference": "mocha" }),
        "{companion:#}"
    );
}

#[test]
fn a_hook_invocation_traces_as_a_script_reference() {
    let syncpack = trace("syncpack");
    assert_eq!(syncpack["is_used"], true, "{syncpack:#}");
    assert_eq!(syncpack["used_in_scripts"], true, "{syncpack:#}");
    assert!(syncpack.get("tooling_credit").is_none(), "{syncpack:#}");
}

/// Every declared devDependency traces as unused exactly when the report
/// flags it.
#[test]
fn the_trace_agrees_with_the_report_for_every_dev_dependency() {
    let root = fixture_path(FIXTURE);
    let report = parse_json(&run_fallow_in_root(
        "dead-code",
        &root,
        &["--format", "json", "--quiet", "--no-cache"],
    ));
    let reported: Vec<&str> = report["unused_dev_dependencies"]
        .as_array()
        .expect("unused_dev_dependencies array")
        .iter()
        .map(|dep| dep["package_name"].as_str().expect("package name"))
        .collect();
    let manifest: Value = serde_json::from_str(
        &std::fs::read_to_string(root.join("package.json")).expect("read package.json"),
    )
    .expect("parse package.json");
    let declared = manifest["devDependencies"]
        .as_object()
        .expect("devDependencies object");
    assert!(!reported.is_empty(), "the fixture must report something");
    for name in declared.keys() {
        let traced = trace(name);
        assert_eq!(
            traced["is_used"].as_bool(),
            Some(!reported.contains(&name.as_str())),
            "trace and report disagree on {name}: reported {reported:?}, trace {traced:#}"
        );
    }
}
