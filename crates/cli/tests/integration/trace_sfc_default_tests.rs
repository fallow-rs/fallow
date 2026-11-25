//! `fallow dead-code --trace <file>.svelte:default` for the implicit default
//! export of a component file.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "integration tests keep fixture setup concise"
)]

use crate::common::{parse_json, run_fallow};
use serde_json::Value;

const FIXTURE: &str = "trace-sfc-implicit-default";

fn trace(target: &str) -> Value {
    parse_json(&run_fallow(
        "dead-code",
        FIXTURE,
        &[
            "--trace",
            target,
            "--format",
            "json",
            "--quiet",
            "--no-cache",
        ],
    ))
}

fn reference_files(value: &Value) -> Vec<String> {
    value["direct_references"]
        .as_array()
        .unwrap_or_else(|| panic!("direct_references must be an array, got {value}"))
        .iter()
        .map(|reference| {
            format!(
                "{} {}",
                reference["from_file"].as_str().unwrap().replace('\\', "/"),
                reference["kind"].as_str().unwrap()
            )
        })
        .collect()
}

/// A barrel re-exports the component and no file imports it through the
/// barrel, but `main.ts` imports the component file directly. Dead-code
/// reports no finding for the component, so the trace must say it is used
/// and list the direct importer.
#[test]
fn directly_imported_component_default_is_used() {
    let value = trace("src/lib/Card.svelte:default");
    assert_eq!(value["is_used"], true, "got {value}");
    assert_eq!(
        reference_files(&value),
        vec!["src/main.ts default import".to_string()],
        "got {value}"
    );
}

/// A `{ default as X }` import is a default import, and a namespace import
/// reads the default through the namespace object.
#[test]
fn named_default_and_namespace_imports_are_direct_references() {
    let panel = trace("src/lib/Panel.svelte:default");
    assert_eq!(
        reference_files(&panel),
        vec!["src/main.ts default import".to_string()],
        "got {panel}"
    );
    let dialog = trace("src/lib/Dialog.vue:default");
    assert_eq!(
        reference_files(&dialog),
        vec!["src/main.ts namespace import".to_string()],
        "got {dialog}"
    );
}

/// A component file that only a side-effect import reaches has no default
/// importer. Dead-code reports an unused component as an unused file, so a
/// reachable component never gets an unused-export verdict.
#[test]
fn reachable_component_without_default_importer_is_not_unused() {
    let value = trace("src/lib/Banner.vue:default");
    assert_eq!(value["file_reachable"], true, "got {value}");
    assert_eq!(value["is_used"], true, "got {value}");
    assert!(reference_files(&value).is_empty(), "got {value}");
}
