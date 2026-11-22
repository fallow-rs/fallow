//! `fallow trace <file>:<symbol> --callers / --callees` chain output.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "integration tests keep fixture setup concise"
)]

use crate::common::{parse_json, run_fallow};
use serde_json::Value;

const FIXTURE: &str = "trace-chain-depth";

fn trace(target: &str, extra: &[&str]) -> Value {
    let mut args = vec![target, "--format", "json", "--quiet", "--no-cache"];
    args.extend_from_slice(extra);
    parse_json(&run_fallow("trace", FIXTURE, &args))
}

/// Each hop as `depth file local_name`.
fn hops(value: &Value, key: &str) -> Vec<String> {
    value[key]
        .as_array()
        .unwrap_or_else(|| panic!("{key} must be an array, got {value}"))
        .iter()
        .map(|hop| {
            format!(
                "{} {} {}",
                hop["depth"],
                hop["file"].as_str().unwrap().replace('\\', "/"),
                hop["local_name"].as_str().unwrap()
            )
        })
        .collect()
}

/// `start.ts` imports `shared.ts` directly and through `a.ts`. The direct
/// edge puts `shared.ts` at depth 1, so its own import of `leaf.ts` is at
/// depth 2 and inside the depth limit.
#[test]
fn callees_visit_each_module_at_its_shortest_depth() {
    let value = trace("src/start.ts:start", &["--callees", "--depth", "2"]);
    let callees = hops(&value, "callees");
    assert!(
        callees.contains(&"2 src/leaf.ts leaf".to_string()),
        "leaf.ts is two hops from start.ts, got {callees:?}"
    );
}

/// The chains of a name that the file does not export are empty, as the
/// `symbol_found` contract says. A namespace importer does not count as a
/// caller of a missing name.
#[test]
fn missing_symbol_gives_empty_chains() {
    let value = trace(
        "src/hub.ts:hubValu",
        &["--callers", "--callees", "--depth", "2"],
    );
    assert_eq!(value["symbol_found"], false, "got {value}");
    assert_eq!(hops(&value, "callers"), Vec::<String>::new());
    let value = trace("src/shared.ts:sharedd", &["--callees", "--depth", "2"]);
    assert_eq!(value["symbol_found"], false, "got {value}");
    assert_eq!(hops(&value, "callees"), Vec::<String>::new());
}
