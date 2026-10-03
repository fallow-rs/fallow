//! A listed package that a used dependency declares as a peer, required or
//! optional, is credited by the unused-dependency check, and
//! `--trace-dependency` names the used package that lists it.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests and benches use unwrap and expect to keep fixture setup concise"
)]

use crate::common::{parse_json, run_fallow};
use serde_json::{Value, json};

const FIXTURE: &str = "optional-peer-of-used-dependency";

fn trace(package_name: &str) -> Value {
    parse_json(&run_fallow(
        "dead-code",
        FIXTURE,
        &[
            "--trace-dependency",
            package_name,
            "--format",
            "json",
            "--quiet",
            "--no-cache",
        ],
    ))
}

#[test]
fn an_optional_peer_of_a_used_dependency_is_not_reported() {
    let output = parse_json(&run_fallow(
        "dead-code",
        FIXTURE,
        &["--format", "json", "--quiet", "--no-cache"],
    ));
    let mut unused: Vec<&str> = output["unused_dependencies"]
        .as_array()
        .expect("unused_dependencies array")
        .iter()
        .map(|dep| dep["package_name"].as_str().expect("package_name"))
        .collect();
    unused.sort_unstable();

    assert_eq!(unused, vec!["peer-of-unused", "unused-host"], "{output:#}");
}

#[test]
fn the_trace_names_the_used_package_that_lists_the_peer() {
    let credited = trace("opt-peer");
    assert_eq!(credited["is_used"], true, "{credited:#}");
    assert_eq!(credited["import_count"], 0, "{credited:#}");
    assert_eq!(credited["peer_of"], json!(["host"]), "{credited:#}");

    let uncredited = trace("peer-of-unused");
    assert_eq!(uncredited["is_used"], false, "{uncredited:#}");
    assert!(uncredited.get("peer_of").is_none(), "{uncredited:#}");
}
