//! The removal of stale definitions from the committed output schema.

use serde_json::{Map, Value};

/// Committed definitions that no Rust type derives and that the merge keeps.
/// Each entry is a definition name and the reason that a person maintains it
/// by hand. The merge and the drift test share this list.
const HAND_MAINTAINED_ALLOW_LIST: &[(&str, &str)] = &[];

/// True when a person maintains the definition by hand.
pub fn is_hand_maintained(name: &str) -> bool {
    HAND_MAINTAINED_ALLOW_LIST
        .iter()
        .any(|(kept, _)| *kept == name)
}

/// Remove each committed definition that the derived map does not produce and
/// that the allow list does not name. Without this step a removed Rust type
/// leaves a stale definition in `docs/output-schema.json`, and only the drift
/// test finds it. Prints one line for each removed name.
pub fn prune_and_report(definitions: &mut Map<String, Value>, derived: &Map<String, Value>) {
    for name in prune_orphan_definitions(definitions, derived, is_hand_maintained) {
        eprintln!("fallow-schema-emit: pruned stale definition `{name}`");
    }
}

/// Returns the removed names in sorted order.
fn prune_orphan_definitions(
    definitions: &mut Map<String, Value>,
    derived: &Map<String, Value>,
    is_allowed: impl Fn(&str) -> bool,
) -> Vec<String> {
    let mut pruned: Vec<String> = Vec::new();
    // `retain` keeps the order of the other keys. `remove` can move the last
    // key into the gap, which reorders the committed file.
    definitions.retain(|name, _| {
        let kept = derived.contains_key(name.as_str()) || is_allowed(name);
        if !kept {
            pruned.push(name.clone());
        }
        kept
    });
    pruned.sort();
    pruned
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A definition that no Rust type derives is removed from the committed
    /// document, unless the allow list names it. A derived definition stays,
    /// and the order of the kept definitions does not change.
    #[test]
    fn the_merge_prunes_a_definition_that_no_type_derives() {
        let mut definitions = Map::new();
        for name in ["Kept", "Stale", "HandWritten", "Last"] {
            definitions.insert(name.to_string(), serde_json::json!({ "type": "string" }));
        }
        let mut derived = Map::new();
        for name in ["Kept", "Last"] {
            derived.insert(name.to_string(), serde_json::json!({ "type": "string" }));
        }

        let pruned =
            prune_orphan_definitions(&mut definitions, &derived, |name| name == "HandWritten");

        assert_eq!(pruned, vec!["Stale".to_string()]);
        let names: Vec<&str> = definitions.keys().map(String::as_str).collect();
        assert_eq!(names, vec!["Kept", "HandWritten", "Last"]);
    }

    /// The committed document carries no orphan after the merge, so
    /// `generate:contracts:check` fails on a stale entry before CI does.
    #[test]
    fn the_merged_document_has_no_orphan_definition() {
        let derived = super::super::derived_definitions();
        let merged = super::super::merge_with_committed(&derived).expect("merge must succeed");
        let definitions = merged["definitions"].as_object().expect("definitions");
        for name in definitions.keys() {
            assert!(
                derived.contains_key(name) || is_hand_maintained(name),
                "orphan `{name}` survived the merge"
            );
        }
    }
}
