//! A listed package that a used dependency declares as a peer, required or
//! optional, is credited by the unused-dependency check, and
//! `--trace-dependency` names the used package that lists it. An optional
//! peer passes credit on to its own peers only when the project lists it.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests and benches use unwrap and expect to keep fixture setup concise"
)]

use crate::common::{parse_json, run_fallow};
use serde_json::{Value, json};

const FIXTURE: &str = "optional-peer-of-used-dependency";
const CHAIN_FIXTURE: &str = "peer-chain-through-unlisted-peer";
const WORKSPACE_CHAIN_FIXTURE: &str = "peer-chain-through-workspace-peer";
const ROOT_CHAIN_FIXTURE: &str = "peer-chain-through-root-peer";
const IGNORED_WORKSPACE_FIXTURE: &str = "peer-chain-through-ignored-workspace";
const REQUIRED_CHAIN_FIXTURE: &str = "peer-chain-through-required-peer";

fn trace(package_name: &str) -> Value {
    trace_in(FIXTURE, package_name)
}

fn trace_in(fixture: &str, package_name: &str) -> Value {
    parse_json(&run_fallow(
        "dead-code",
        fixture,
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
    assert_eq!(
        unused_dependencies(FIXTURE),
        vec!["peer-of-unused", "unused-host"]
    );
}

fn unused_dependencies(fixture: &str) -> Vec<String> {
    let output = parse_json(&run_fallow(
        "dead-code",
        fixture,
        &["--format", "json", "--quiet", "--no-cache"],
    ));
    let mut unused: Vec<String> = output["unused_dependencies"]
        .as_array()
        .expect("unused_dependencies array")
        .iter()
        .map(|dep| {
            dep["package_name"]
                .as_str()
                .expect("package_name")
                .to_string()
        })
        .collect();
    unused.sort_unstable();
    unused
}

/// `host` is used. Its optional peer `unlisted-mid` is installed but not
/// listed, so the project does not turn it on and its peers `leaf-a`
/// (optional) and `leaf-b` (required) stay reported. The listed optional peer
/// `listed-mid` and the required peer `req-mid` still pass credit on.
#[test]
fn an_unlisted_optional_peer_does_not_credit_its_own_peers() {
    assert_eq!(unused_dependencies(CHAIN_FIXTURE), vec!["leaf-a", "leaf-b"]);

    for leaf in ["leaf-a", "leaf-b"] {
        let uncredited = trace_in(CHAIN_FIXTURE, leaf);
        assert_eq!(uncredited["is_used"], false, "{uncredited:#}");
        assert!(uncredited.get("peer_of").is_none(), "{uncredited:#}");
    }
    assert_eq!(
        trace_in(CHAIN_FIXTURE, "leaf-c")["peer_of"],
        json!(["listed-mid"])
    );
    assert_eq!(
        trace_in(CHAIN_FIXTURE, "leaf-d")["peer_of"],
        json!(["req-mid"])
    );
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

/// A workspace that installs the host in its own `node_modules` credits the
/// optional peer in its manifest. The trace runs the same closure per
/// workspace and names the host, and it names the manifest that the report
/// still flags.
#[test]
fn the_trace_follows_a_host_installed_in_a_workspace() {
    let fixture = "optional-peer-in-workspace-install";
    let output = parse_json(&run_fallow(
        "dead-code",
        fixture,
        &["--format", "json", "--quiet", "--no-cache"],
    ));
    let unused: Vec<&str> = output["unused_dependencies"]
        .as_array()
        .expect("unused_dependencies array")
        .iter()
        .map(|dep| dep["path"].as_str().expect("path"))
        .collect();
    assert_eq!(unused, vec!["packages/b/package.json"], "{output:#}");

    let traced = parse_json(&run_fallow(
        "dead-code",
        fixture,
        &[
            "--trace-dependency",
            "opt-peer",
            "--format",
            "json",
            "--quiet",
            "--no-cache",
        ],
    ));
    assert_eq!(traced["is_used"], true, "{traced:#}");
    assert_eq!(traced["peer_of"], json!(["host"]), "{traced:#}");
    assert_eq!(
        traced["unused_in"],
        json!(["packages/b/package.json"]),
        "{traced:#}"
    );
}

/// The workspace `app` lists `host` and its optional peer `mid`, so `mid` is
/// turned on for the shared install. `mid` declares `leaf` as a required
/// peer, so the root `leaf` gets credit through `host` and `mid`.
#[test]
fn an_optional_peer_listed_by_a_workspace_credits_a_root_peer() {
    let unused = unused_dependencies(WORKSPACE_CHAIN_FIXTURE);
    assert!(!unused.iter().any(|name| name == "leaf"), "{unused:?}");
    assert_eq!(
        trace_in(WORKSPACE_CHAIN_FIXTURE, "leaf")["peer_of"],
        json!(["mid"])
    );
}

/// The root `package.json` lists the optional peer `mid` of `host`. The
/// workspace `app` lists and imports `host` and lists `leaf`. The root turns
/// `mid` on for the shared install, so the workspace check credits `leaf`
/// through `host` and `mid`, and the trace agrees.
#[test]
fn an_optional_peer_listed_by_the_root_credits_a_workspace_peer() {
    let unused = unused_dependencies(ROOT_CHAIN_FIXTURE);
    assert!(unused.is_empty(), "{unused:?}");
    assert_eq!(
        trace_in(ROOT_CHAIN_FIXTURE, "leaf")["peer_of"],
        json!(["mid"])
    );
}

/// Only the workspace `packages/ignored` lists the optional peer `mid` of
/// `host`, and `ignorePatterns` hides that workspace. The ignored manifest
/// does not turn `mid` on, so the root `leaf` stays reported, and the trace
/// agrees.
#[test]
fn an_ignored_workspace_does_not_turn_on_an_optional_peer() {
    assert_eq!(unused_dependencies(IGNORED_WORKSPACE_FIXTURE), vec!["leaf"]);

    let uncredited = trace_in(IGNORED_WORKSPACE_FIXTURE, "leaf");
    assert_eq!(uncredited["is_used"], false, "{uncredited:#}");
    assert!(uncredited.get("peer_of").is_none(), "{uncredited:#}");
}

/// `host` declares `mid` as a required peer, and `mid` declares `leaf` as an
/// optional peer. The required peer `mid` is always expanded, so the listed
/// optional peer `leaf` gets credit through it.
#[test]
fn a_required_peer_credits_its_own_optional_peer() {
    assert!(unused_dependencies(REQUIRED_CHAIN_FIXTURE).is_empty());
    assert_eq!(
        trace_in(REQUIRED_CHAIN_FIXTURE, "leaf")["peer_of"],
        json!(["mid"])
    );
}
