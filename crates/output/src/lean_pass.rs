//! The `--lean` JSON profile.
//!
//! The pass runs on the serialized document, after every typed step, so the
//! default output and the typed API stay byte-identical. It acts only on the
//! envelopes in [`LEAN_PROFILE_KINDS`]. Other documents pass through unchanged.

use crate::lean_profile::{LEAN_PROFILE, LEAN_PROFILE_KINDS};
use serde_json::{Map, Value};

/// Default maximum rows per health table in a lean document.
pub const LEAN_TABLE_CAP: usize = 10;

/// Health tables that the lean profile caps. The engine sorts each one by
/// importance before output (hotspots by score, file scores worst first,
/// large functions by line count), so the first rows are the ones to keep.
const CAPPED_HEALTH_TABLES: [&str; 3] = ["hotspots", "file_scores", "large_functions"];

/// Action fields that hold text for a person or an agent to read.
const ACTION_TEXT_FIELDS: [&str; 3] = ["description", "note", "comment"];

/// A shared text must save at least one copy to be worth a `_meta` entry.
const MIN_SHARED_TEXT_USES: usize = 2;

/// Apply the lean profile to a serialized JSON document.
///
/// `table_cap` is the maximum number of rows per health table; `None` keeps
/// every row.
pub fn apply_lean_profile(document: &mut Value, table_cap: Option<usize>) {
    let Some(root) = document.as_object_mut() else {
        return;
    };
    let Some(kind) = root
        .get("kind")
        .and_then(Value::as_str)
        .filter(|kind| LEAN_PROFILE_KINDS.contains(kind))
        .map(str::to_owned)
    else {
        return;
    };
    match kind.as_str() {
        "dead-code" => trim_dead_code_body(root),
        "dead-code-grouped" => {
            drop_empty_arrays(root);
            for_each_group(root, drop_empty_arrays);
        }
        "health" => {
            trim_health_body(root, table_cap);
            for_each_group(root, |group| trim_health_body(group, table_cap));
        }
        "combined" => trim_combined(root, table_cap),
        // A kind in `LEAN_PROFILE_KINDS` without an arm here keeps its full
        // shape; `every_lean_kind_has_an_arm` fails first.
        _ => return,
    }
    group_workspace_diagnostics(root);
    move_shared_action_text(root);
    root.shift_insert(1, "profile".to_string(), Value::from(LEAN_PROFILE));
}

fn trim_dead_code_body(body: &mut Map<String, Value>) {
    drop_empty_arrays(body);
    if let Some(Value::Object(summary)) = body.get_mut("summary") {
        summary.retain(|_, count| !is_zero(count));
    }
}

fn trim_health_body(body: &mut Map<String, Value>, table_cap: Option<usize>) {
    if let Some(cap) = table_cap {
        for table in CAPPED_HEALTH_TABLES {
            cap_table(body, table, cap);
        }
    }
    drop_empty_arrays(body);
}

fn trim_combined(root: &mut Map<String, Value>, table_cap: Option<usize>) {
    drop_empty_arrays(root);
    if let Some(Value::Object(check)) = root.get_mut("check") {
        trim_dead_code_body(check);
        group_workspace_diagnostics(check);
    }
    if let Some(Value::Object(dupes)) = root.get_mut("dupes") {
        drop_empty_arrays(dupes);
        group_workspace_diagnostics(dupes);
    }
    if let Some(Value::Object(health)) = root.get_mut("health") {
        trim_health_body(health, table_cap);
        group_workspace_diagnostics(health);
    }
}

fn for_each_group(root: &mut Map<String, Value>, mut trim: impl FnMut(&mut Map<String, Value>)) {
    if let Some(Value::Array(groups)) = root.get_mut("groups") {
        for group in groups.iter_mut().filter_map(Value::as_object_mut) {
            trim(group);
        }
    }
}

fn drop_empty_arrays(object: &mut Map<String, Value>) {
    object.retain(|_, value| !matches!(value, Value::Array(items) if items.is_empty()));
}

fn is_zero(value: &Value) -> bool {
    value.as_f64() == Some(0.0)
}

/// Keep the first `cap` rows of `table` and write the number of dropped rows
/// to `<table>_omitted`, directly after the table.
fn cap_table(body: &mut Map<String, Value>, table: &str, cap: usize) {
    let Some(Value::Array(rows)) = body.get_mut(table) else {
        return;
    };
    if rows.len() <= cap {
        return;
    }
    let omitted = rows.len() - cap;
    rows.truncate(cap);
    let position = body
        .keys()
        .position(|key| key == table)
        .unwrap_or(body.len());
    body.shift_insert(
        position + 1,
        format!("{table}_omitted"),
        Value::from(omitted),
    );
}

/// Replace the diagnostics array with one entry per `kind`, in first-seen
/// order. Each entry is the first diagnostic of its kind plus a `count`, and
/// `degrades_analysis` is true when any diagnostic of the kind degrades it.
fn group_workspace_diagnostics(object: &mut Map<String, Value>) {
    let Some(Value::Array(entries)) = object.get_mut("workspace_diagnostics") else {
        return;
    };
    let mut groups: Vec<(Option<String>, Value, usize, bool)> = Vec::new();
    for entry in entries.drain(..) {
        let kind = entry.get("kind").and_then(Value::as_str).map(str::to_owned);
        let degrades = entry.get("degrades_analysis") == Some(&Value::Bool(true));
        // An entry without a kind cannot be grouped, so it stays on its own.
        let existing = kind.as_ref().and_then(|kind| {
            groups
                .iter_mut()
                .find(|(group_kind, ..)| group_kind.as_ref() == Some(kind))
        });
        match existing {
            Some((_, _, count, group_degrades)) => {
                *count += 1;
                *group_degrades |= degrades;
            }
            None => groups.push((kind, entry, 1, degrades)),
        }
    }
    for (kind, mut first, count, degrades) in groups {
        if let (Some(_), Value::Object(fields)) = (kind, &mut first) {
            if degrades {
                fields.insert("degrades_analysis".to_string(), Value::Bool(true));
            }
            // An absent count means 1, so a kind with one diagnostic costs
            // no more bytes than in the full document.
            if count > 1 {
                fields.insert("count".to_string(), Value::from(count));
            }
        }
        entries.push(first);
    }
}

/// How often one action type carries one text field, and with which values.
#[derive(Default)]
struct FieldTally {
    /// Actions of the type that have this field as text.
    present: usize,
    /// Each distinct text with its number of uses, in first-seen order.
    values: Vec<(String, usize)>,
}

struct TypeTally {
    action_type: String,
    /// All actions of the type.
    actions: usize,
    fields: [FieldTally; ACTION_TEXT_FIELDS.len()],
}

/// Move the most common text of each (action type, text field) pair to
/// `_meta.actions[type][field]` and remove it from the actions that carry it.
///
/// The move is lossless: `action.field ?? _meta.actions[type].field` gives
/// the original text. A field moves only when every action of the type has
/// it, because an action without the field would otherwise read the shared
/// text as its own.
fn move_shared_action_text(root: &mut Map<String, Value>) {
    let mut tallies: Vec<TypeTally> = Vec::new();
    for_each_action(root, &mut |action| tally_action(&mut tallies, action));

    let mut shared = Map::new();
    for tally in &tallies {
        let mut fields = Map::new();
        for (field, field_tally) in ACTION_TEXT_FIELDS.iter().zip(&tally.fields) {
            if field_tally.present != tally.actions {
                continue;
            }
            if let Some(text) = most_common(&field_tally.values) {
                fields.insert((*field).to_string(), Value::from(text));
            }
        }
        if !fields.is_empty() {
            shared.insert(tally.action_type.clone(), Value::Object(fields));
        }
    }
    if shared.is_empty() {
        return;
    }

    for_each_action(root, &mut |action| {
        let Some(action_type) = action.get("type").and_then(Value::as_str) else {
            return;
        };
        let Some(Value::Object(defaults)) = shared.get(action_type) else {
            return;
        };
        let moved: Vec<String> = defaults
            .iter()
            .filter(|(field, text)| action.get(field.as_str()) == Some(*text))
            .map(|(field, _)| field.clone())
            .collect();
        for field in moved {
            action.shift_remove(&field);
        }
    });

    let meta = root
        .entry("_meta")
        .or_insert_with(|| Value::Object(Map::new()));
    if let Value::Object(meta) = meta {
        meta.insert("actions".to_string(), Value::Object(shared));
    }
}

fn tally_action(tallies: &mut Vec<TypeTally>, action: &Map<String, Value>) {
    let Some(action_type) = action.get("type").and_then(Value::as_str) else {
        return;
    };
    let index = match tallies
        .iter()
        .position(|tally| tally.action_type == action_type)
    {
        Some(index) => index,
        None => {
            tallies.push(TypeTally {
                action_type: action_type.to_string(),
                actions: 0,
                fields: Default::default(),
            });
            tallies.len() - 1
        }
    };
    let tally = &mut tallies[index];
    tally.actions += 1;
    for (field, field_tally) in ACTION_TEXT_FIELDS.iter().zip(tally.fields.iter_mut()) {
        let Some(text) = action.get(*field).and_then(Value::as_str) else {
            continue;
        };
        field_tally.present += 1;
        match field_tally
            .values
            .iter_mut()
            .find(|(value, _)| value == text)
        {
            Some((_, uses)) => *uses += 1,
            None => field_tally.values.push((text.to_string(), 1)),
        }
    }
}

/// The text with the most uses; the first seen wins a tie.
fn most_common(values: &[(String, usize)]) -> Option<&str> {
    let mut best: Option<&(String, usize)> = None;
    for candidate in values {
        if best.is_none_or(|(_, uses)| candidate.1 > *uses) {
            best = Some(candidate);
        }
    }
    best.filter(|(_, uses)| *uses >= MIN_SHARED_TEXT_USES)
        .map(|(text, _)| text.as_str())
}

/// Visit every action object in every `actions` array, outside root `_meta`.
fn for_each_action(root: &mut Map<String, Value>, visit: &mut impl FnMut(&mut Map<String, Value>)) {
    for (key, value) in root.iter_mut() {
        if key != "_meta" {
            visit_actions(key, value, visit);
        }
    }
}

fn visit_actions(key: &str, value: &mut Value, visit: &mut impl FnMut(&mut Map<String, Value>)) {
    match value {
        Value::Array(items) if key == "actions" => {
            for action in items.iter_mut().filter_map(Value::as_object_mut) {
                visit(action);
            }
        }
        Value::Array(items) => {
            for item in items {
                visit_actions("", item, visit);
            }
        }
        Value::Object(map) => {
            for (child_key, child) in map.iter_mut() {
                visit_actions(child_key, child, visit);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn lean(mut document: Value, cap: Option<usize>) -> Value {
        apply_lean_profile(&mut document, cap);
        document
    }

    #[test]
    fn every_lean_kind_has_an_arm() {
        for kind in LEAN_PROFILE_KINDS {
            let document = lean(json!({ "kind": kind, "schema_version": 1 }), None);
            assert_eq!(document["profile"], LEAN_PROFILE, "kind {kind}");
        }
    }

    #[test]
    fn lean_kinds_match_the_schema_enum() {
        let kinds: Vec<&str> = LEAN_PROFILE_KINDS.to_vec();
        let schema_kinds: Vec<String> = [
            crate::LeanOutputKind::DeadCode,
            crate::LeanOutputKind::DeadCodeGrouped,
            crate::LeanOutputKind::Health,
            crate::LeanOutputKind::Combined,
        ]
        .iter()
        .map(|kind| {
            serde_json::to_value(kind)
                .expect("a lean kind serializes")
                .as_str()
                .expect("a lean kind is a string")
                .to_owned()
        })
        .collect();
        assert_eq!(kinds, schema_kinds);
    }

    #[test]
    fn other_kinds_stay_unchanged() {
        let document = json!({
            "kind": "dupes",
            "schema_version": 3,
            "clone_groups": [],
            "stats": { "clone_groups": 0 }
        });
        assert_eq!(lean(document.clone(), Some(LEAN_TABLE_CAP)), document);
        let no_kind = json!({ "error": true, "items": [] });
        assert_eq!(lean(no_kind.clone(), Some(LEAN_TABLE_CAP)), no_kind);
    }

    #[test]
    fn dead_code_drops_empty_arrays_and_zero_summary_counts() {
        let document = lean(
            json!({
                "kind": "dead-code",
                "schema_version": 7,
                "version": "3.0.0",
                "elapsed_ms": 0,
                "total_issues": 0,
                "summary": { "total_issues": 0, "unused_files": 0, "unused_exports": 2 },
                "unused_files": [],
                "unused_exports": [{ "name": "a" }, { "name": "b" }],
                "entry_points": { "total": 0 }
            }),
            Some(LEAN_TABLE_CAP),
        );
        assert_eq!(
            document,
            json!({
                "kind": "dead-code",
                "profile": "lean",
                "schema_version": 7,
                "version": "3.0.0",
                "elapsed_ms": 0,
                "total_issues": 0,
                "summary": { "unused_exports": 2 },
                "unused_exports": [{ "name": "a" }, { "name": "b" }],
                "entry_points": { "total": 0 }
            })
        );
    }

    #[test]
    fn grouped_dead_code_drops_empty_arrays_in_each_group() {
        let document = lean(
            json!({
                "kind": "dead-code-grouped",
                "schema_version": 7,
                "total_issues": 1,
                "groups": [
                    { "key": "src", "total_issues": 1, "unused_files": [{ "path": "a" }], "unused_exports": [] },
                    { "key": "lib", "total_issues": 0, "unused_files": [], "unused_exports": [] }
                ]
            }),
            Some(LEAN_TABLE_CAP),
        );
        assert_eq!(
            document["groups"],
            json!([
                { "key": "src", "total_issues": 1, "unused_files": [{ "path": "a" }] },
                { "key": "lib", "total_issues": 0 }
            ])
        );
    }

    #[test]
    fn workspace_diagnostics_group_by_kind_in_first_seen_order() {
        let document = lean(
            json!({
                "kind": "dead-code",
                "schema_version": 7,
                "workspace_diagnostics": [
                    { "path": "a", "kind": "redundant", "message": "first", "pattern": "x" },
                    { "path": ".", "kind": "missing", "message": "m", "degrades_analysis": true },
                    { "path": "b", "kind": "redundant", "message": "second", "degrades_analysis": true },
                    { "path": "c", "kind": "redundant", "message": "third" }
                ]
            }),
            Some(LEAN_TABLE_CAP),
        );
        assert_eq!(
            document["workspace_diagnostics"],
            json!([
                { "path": "a", "kind": "redundant", "message": "first", "pattern": "x", "degrades_analysis": true, "count": 3 },
                { "path": ".", "kind": "missing", "message": "m", "degrades_analysis": true }
            ])
        );
    }

    #[test]
    fn health_tables_keep_the_first_rows_and_count_the_rest() {
        let rows = |count: usize| -> Value {
            Value::Array((0..count).map(|index| json!({ "row": index })).collect())
        };
        let document = lean(
            json!({
                "kind": "health",
                "schema_version": 12,
                "findings": rows(4),
                "file_scores": rows(5),
                "hotspots": rows(2),
                "large_functions": rows(3),
                "targets": rows(5),
                "groups": [{ "key": "src", "file_scores": rows(4), "findings": [] }]
            }),
            Some(3),
        );
        assert_eq!(document["findings"], rows(4), "findings stay complete");
        assert_eq!(
            document["targets"],
            rows(5),
            "targets are not a capped table"
        );
        assert_eq!(document["file_scores"], rows(3));
        assert_eq!(document["file_scores_omitted"], 2);
        assert_eq!(document["hotspots"], rows(2));
        assert!(document.get("hotspots_omitted").is_none());
        assert_eq!(document["large_functions"], rows(3));
        assert!(document.get("large_functions_omitted").is_none());
        assert_eq!(
            document["groups"],
            json!([{ "key": "src", "file_scores": rows(3), "file_scores_omitted": 1 }])
        );
        let keys: Vec<&String> = document.as_object().expect("object").keys().collect();
        let scores = keys.iter().position(|key| *key == "file_scores");
        let omitted = keys.iter().position(|key| *key == "file_scores_omitted");
        assert_eq!(
            omitted,
            scores.map(|index| index + 1),
            "the omitted count follows its table"
        );
    }

    #[test]
    fn health_tables_stay_whole_without_a_cap() {
        let rows = Value::Array((0..20).map(|index| json!({ "row": index })).collect());
        let document = lean(
            json!({ "kind": "health", "schema_version": 12, "hotspots": rows }),
            None,
        );
        assert_eq!(document["hotspots"], rows);
        assert!(document.get("hotspots_omitted").is_none());
    }

    #[test]
    fn combined_trims_each_section() {
        let rows = Value::Array((0..12).map(|index| json!({ "row": index })).collect());
        let document = lean(
            json!({
                "kind": "combined",
                "schema_version": 1,
                "_meta": { "telemetry": { "analysis_run_id": "run_1" } },
                "check": {
                    "total_issues": 0,
                    "summary": { "unused_files": 0, "unused_exports": 0 },
                    "unused_files": []
                },
                "dupes": { "clone_groups": [], "stats": { "clone_groups": 0 } },
                "health": { "findings": [], "summary": { "files_analyzed": 0 }, "hotspots": rows },
                "workspace_diagnostics": [
                    { "path": ".", "kind": "missing", "message": "m" },
                    { "path": ".", "kind": "missing", "message": "n" }
                ]
            }),
            Some(10),
        );
        assert_eq!(document["profile"], "lean");
        assert_eq!(
            document["check"],
            json!({ "total_issues": 0, "summary": {} })
        );
        assert_eq!(document["dupes"], json!({ "stats": { "clone_groups": 0 } }));
        assert_eq!(
            document["health"]["summary"],
            json!({ "files_analyzed": 0 }),
            "health summary zeros are real values"
        );
        assert!(document["health"].get("findings").is_none());
        assert_eq!(
            document["health"]["hotspots"].as_array().map(Vec::len),
            Some(10)
        );
        assert_eq!(document["health"]["hotspots_omitted"], 2);
        assert_eq!(
            document["workspace_diagnostics"],
            json!([{ "path": ".", "kind": "missing", "message": "m", "count": 2 }])
        );
        assert_eq!(
            document["_meta"]["telemetry"],
            json!({ "analysis_run_id": "run_1" }),
            "existing _meta keys stay"
        );
    }

    #[test]
    fn repeated_action_text_moves_to_meta_actions() {
        let document = lean(
            json!({
                "kind": "dead-code",
                "schema_version": 7,
                "unused_exports": [
                    { "name": "a", "actions": [
                        { "type": "remove-export", "auto_fixable": true, "description": "Remove it" },
                        { "type": "suppress-line", "auto_fixable": false, "description": "Suppress", "comment": "// x" }
                    ] },
                    { "name": "b", "actions": [
                        { "type": "remove-export", "auto_fixable": true, "description": "Remove it" },
                        { "type": "suppress-line", "auto_fixable": false, "description": "Suppress", "comment": "// x" }
                    ] },
                    { "name": "c", "actions": [
                        { "type": "remove-export", "auto_fixable": true, "description": "Other text" },
                        { "type": "suppress-line", "auto_fixable": false, "description": "Unique", "comment": "// y" }
                    ] }
                ]
            }),
            Some(LEAN_TABLE_CAP),
        );
        assert_eq!(
            document["_meta"]["actions"],
            json!({
                "remove-export": { "description": "Remove it" },
                "suppress-line": { "description": "Suppress", "comment": "// x" }
            })
        );
        assert_eq!(
            document["unused_exports"][0]["actions"],
            json!([
                { "type": "remove-export", "auto_fixable": true },
                { "type": "suppress-line", "auto_fixable": false }
            ])
        );
        assert_eq!(
            document["unused_exports"][2]["actions"],
            json!([
                { "type": "remove-export", "auto_fixable": true, "description": "Other text" },
                { "type": "suppress-line", "auto_fixable": false, "description": "Unique", "comment": "// y" }
            ]),
            "a different text stays on its action"
        );
    }

    #[test]
    fn action_text_stays_when_moving_it_would_lose_data() {
        let document = lean(
            json!({
                "kind": "health",
                "schema_version": 12,
                "findings": [
                    { "actions": [{ "type": "refactor-function", "description": "Refactor a", "note": "Split" }] },
                    { "actions": [{ "type": "refactor-function", "description": "Refactor b", "note": "Split" }] },
                    { "actions": [{ "type": "refactor-function", "description": "Refactor c" }] }
                ]
            }),
            Some(LEAN_TABLE_CAP),
        );
        // One action has no note. A shared note would give it one, so the
        // note stays on each action. Each description occurs once, so no
        // description moves either.
        assert!(document.get("_meta").is_none(), "{document}");
        assert_eq!(
            document["findings"][2]["actions"][0],
            json!({ "type": "refactor-function", "description": "Refactor c" })
        );
    }

    #[test]
    fn meta_actions_ignores_text_that_occurs_once() {
        let document = lean(
            json!({
                "kind": "dead-code",
                "schema_version": 7,
                "unused_files": [{ "actions": [{ "type": "delete-file", "auto_fixable": false, "description": "Delete" }] }]
            }),
            Some(LEAN_TABLE_CAP),
        );
        assert!(document.get("_meta").is_none());
        assert_eq!(
            document["unused_files"][0]["actions"][0]["description"],
            "Delete"
        );
    }
}
