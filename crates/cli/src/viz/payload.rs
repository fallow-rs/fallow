//! Transport encoding for the payload that `fallow viz` embeds in its HTML.
//!
//! The payload has two parts:
//!
//! - The core. It holds what the first paint needs: files, edges, the
//!   summary and the availability of each analysis.
//! - Lazy sections. Each one holds a large finding list. The frontend parses
//!   a section on the first read of its property, which is normally when its
//!   lens opens.
//!
//! Arrays of objects become tables: `{"$k": keys, "$r": rows}`. A `null`
//! cell or a missing trailing cell means that the key is absent. The encoder
//! keeps an array plain when the table is not shorter or when a member holds
//! an explicit `null`. When any key in the payload starts with `$`, the
//! encoder writes no tables at all and `tables` is false, so the page tells
//! the decoder to leave such objects as they are.
//!
//! `viz-frontend/src/payload.ts` owns the decoder. Both sides pin the same
//! fixture, so they cannot drift apart.

use rustc_hash::FxHashSet;
use serde_json::{Map, Value};

use fallow_engine::viz::VizData;

/// Sections that the first paint does not read. Each path names an object
/// key, with `.` between the levels.
const LAZY_SECTIONS: &[&str] = &[
    "health.files",
    "health.findings",
    "security.candidates",
    "security.blind_spots",
    "styling.findings",
    "dependencies.findings",
    "architecture.findings",
    "frameworks.findings",
    "feature_flags.findings",
    "clones",
];

/// The per-file function lists, sent as one array that aligns with `files`.
const FUNCTIONS_COLUMN: &str = "files.*.functions";

const TABLE_KEYS: &str = "$k";
const TABLE_ROWS: &str = "$r";
const RESERVED_PREFIX: char = '$';

/// The encoded payload: the core JSON and one JSON text per lazy section.
pub(super) struct EncodedPayload {
    pub core: String,
    pub lazy: Vec<(&'static str, String)>,
    /// Whether the texts can hold tables that the decoder must expand.
    pub tables: bool,
}

/// Split and encode the viz payload for the HTML page.
pub(super) fn encode_payload(data: &VizData) -> Result<EncodedPayload, serde_json::Error> {
    encode_value(serde_json::to_value(data)?)
}

fn encode_value(mut root: Value) -> Result<EncodedPayload, serde_json::Error> {
    let tables = !has_reserved_key(&root);
    let finish = |value: Value| {
        let value = if tables { encode_tables(value) } else { value };
        serde_json::to_string(&value)
    };

    let mut lazy = Vec::new();
    if let Some(column) = take_column(&mut root, "files", "functions") {
        lazy.push((FUNCTIONS_COLUMN, finish(column)?));
    }
    for path in LAZY_SECTIONS {
        if let Some(section) = take_path(&mut root, path) {
            lazy.push((*path, finish(section)?));
        }
    }
    Ok(EncodedPayload {
        core: finish(root)?,
        lazy,
        tables,
    })
}

/// Remove the value at a dotted object path and return it.
fn take_path(root: &mut Value, path: &str) -> Option<Value> {
    let (parent_path, key) = path.rsplit_once('.').unwrap_or(("", path));
    let mut parent = root;
    for segment in parent_path.split('.').filter(|segment| !segment.is_empty()) {
        parent = parent.get_mut(segment)?;
    }
    parent.as_object_mut()?.remove(key)
}

/// Remove `key` from each object in the `rows` array. Return one entry per
/// row (an empty array when the row had no value), or `None` when no row had
/// a value.
fn take_column(root: &mut Value, rows: &str, key: &str) -> Option<Value> {
    let rows = root.get_mut(rows)?.as_array_mut()?;
    let mut found = false;
    let column: Vec<Value> = rows
        .iter_mut()
        .map(|row| {
            let value = row.as_object_mut().and_then(|row| row.remove(key));
            found |= value.is_some();
            value.unwrap_or_else(|| Value::Array(Vec::new()))
        })
        .collect();
    found.then_some(Value::Array(column))
}

fn has_reserved_key(value: &Value) -> bool {
    match value {
        Value::Object(map) => map
            .iter()
            .any(|(key, value)| key.starts_with(RESERVED_PREFIX) || has_reserved_key(value)),
        Value::Array(items) => items.iter().any(has_reserved_key),
        _ => false,
    }
}

/// Encode each array of objects as a table when the table is shorter.
fn encode_tables(value: Value) -> Value {
    match value {
        Value::Object(map) => Value::Object(
            map.into_iter()
                .map(|(key, value)| (key, encode_tables(value)))
                .collect(),
        ),
        Value::Array(items) => {
            let items: Vec<Value> = items.into_iter().map(encode_tables).collect();
            match table_of(&items) {
                Some(table) if json_len(&table) < json_len_of_items(&items) => table,
                _ => Value::Array(items),
            }
        }
        other => other,
    }
}

fn table_of(items: &[Value]) -> Option<Value> {
    if items.len() < 2 {
        return None;
    }
    let mut keys: Vec<&str> = Vec::new();
    let mut seen: FxHashSet<&str> = FxHashSet::default();
    for item in items {
        let object = item.as_object()?;
        for (key, value) in object {
            // `null` marks an absent key in a row, so a real `null` cannot
            // travel in a table.
            if value.is_null() {
                return None;
            }
            if seen.insert(key.as_str()) {
                keys.push(key.as_str());
            }
        }
    }
    let rows = items
        .iter()
        .filter_map(Value::as_object)
        .map(|object| Value::Array(table_row(object, &keys)))
        .collect();
    let mut table = Map::new();
    table.insert(
        TABLE_KEYS.to_string(),
        Value::Array(
            keys.iter()
                .map(|key| Value::String((*key).to_string()))
                .collect(),
        ),
    );
    table.insert(TABLE_ROWS.to_string(), Value::Array(rows));
    Some(Value::Object(table))
}

fn table_row(object: &Map<String, Value>, keys: &[&str]) -> Vec<Value> {
    let mut row: Vec<Value> = keys
        .iter()
        .map(|key| object.get(*key).cloned().unwrap_or(Value::Null))
        .collect();
    while row.last().is_some_and(Value::is_null) {
        row.pop();
    }
    row
}

fn json_len(value: &Value) -> usize {
    serde_json::to_string(value).map_or(usize::MAX, |text| text.len())
}

fn json_len_of_items(items: &[Value]) -> usize {
    // `[` + items + `,` separators + `]`.
    items.iter().map(json_len).sum::<usize>() + items.len().saturating_sub(1) + 2
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    /// Mirror of `decodeTables` in `viz-frontend/src/payload.ts`.
    fn decode_tables(value: Value) -> Value {
        match value {
            Value::Array(items) => Value::Array(items.into_iter().map(decode_tables).collect()),
            Value::Object(mut map)
                if map.len() == 2
                    && map.get(TABLE_KEYS).is_some_and(Value::is_array)
                    && map.get(TABLE_ROWS).is_some_and(Value::is_array) =>
            {
                let Some(Value::Array(keys)) = map.remove(TABLE_KEYS) else {
                    unreachable!("checked above");
                };
                let Some(Value::Array(rows)) = map.remove(TABLE_ROWS) else {
                    unreachable!("checked above");
                };
                let rows = rows
                    .into_iter()
                    .map(|row| {
                        let Value::Array(cells) = row else {
                            panic!("table row is not an array");
                        };
                        let object: Map<String, Value> = keys
                            .iter()
                            .zip(cells)
                            .filter(|(_, cell)| !cell.is_null())
                            .map(|(key, cell)| {
                                (key.as_str().expect("key").to_string(), decode_tables(cell))
                            })
                            .collect();
                        Value::Object(object)
                    })
                    .collect();
                Value::Array(rows)
            }
            Value::Object(map) => Value::Object(
                map.into_iter()
                    .map(|(key, value)| (key, decode_tables(value)))
                    .collect(),
            ),
            other => other,
        }
    }

    /// Rebuild the full payload the way the frontend does after every lazy
    /// section has been read.
    fn hydrate(encoded: &EncodedPayload) -> Value {
        let decode = |text: &str| {
            let value: Value = serde_json::from_str(text).expect("payload JSON");
            if encoded.tables {
                decode_tables(value)
            } else {
                value
            }
        };
        let mut root = decode(&encoded.core);
        for (path, text) in &encoded.lazy {
            let section = decode(text);
            if *path == FUNCTIONS_COLUMN {
                let Value::Array(column) = section else {
                    panic!("function column is not an array");
                };
                let files = root["files"].as_array_mut().expect("files");
                for (file, functions) in files.iter_mut().zip(column) {
                    if functions.as_array().is_some_and(|list| !list.is_empty()) {
                        file.as_object_mut()
                            .expect("file")
                            .insert("functions".to_string(), functions);
                    }
                }
                continue;
            }
            let (parent_path, key) = path.rsplit_once('.').unwrap_or(("", path));
            let mut parent = &mut root;
            for segment in parent_path.split('.').filter(|segment| !segment.is_empty()) {
                parent = parent.get_mut(segment).expect("parent");
            }
            parent
                .as_object_mut()
                .expect("parent object")
                .insert((*key).to_string(), section);
        }
        root
    }

    fn sample() -> Value {
        json!({
            "root": "demo",
            "files": [
                {"path": "a.ts", "size": 10, "functions": [
                    {"name": "f", "line": 1, "cyclomatic": 2},
                    {"name": "g", "line": 9, "cyclomatic": 1},
                    {"name": "h", "line": 20, "cyclomatic": 4}
                ]},
                {"path": "b.ts", "size": 20, "zone": 1},
                {"path": "c.ts", "size": 30}
            ],
            "edges": [[0, 1, 0], [1, 2, 1]],
            "clones": [],
            "health": {
                "availability": {"state": "complete", "count": 2, "unit": "files"},
                "score": 90.5,
                "files": [],
                "findings": [
                    {"kind": "health-finding", "file": 0, "facts": [
                        {"label": "cyclomatic", "value": "30"},
                        {"label": "cognitive", "value": "40"}
                    ]},
                    {"kind": "health-finding", "file": 1, "severity": "high"}
                ]
            },
            "styling": {"availability": {"state": "disabled", "count": 0, "unit": "findings"},
                        "findings": [], "summary": {"tokens": [1, 2]}}
        })
    }

    #[test]
    fn encoded_table_matches_the_frontend_fixture() {
        // `viz-frontend/src/payload.test.ts` decodes this exact text.
        let rows = json!([
            {"line": 1, "name": "a", "props": 2},
            {"line": 2, "name": "b"},
            {"line": 3, "props": 4}
        ]);
        assert_eq!(
            serde_json::to_string(&encode_tables(rows)).expect("serialize"),
            r#"{"$k":["line","name","props"],"$r":[[1,"a",2],[2,"b"],[3,null,4]]}"#
        );
    }

    #[test]
    fn hydrated_payload_equals_the_original() {
        let original = sample();
        let encoded = encode_value(original.clone()).expect("encode");
        assert_eq!(hydrate(&encoded), original);
    }

    #[test]
    fn core_leaves_out_every_lazy_section() {
        let encoded = encode_value(sample()).expect("encode");
        let core: Value = serde_json::from_str(&encoded.core).expect("core JSON");
        assert!(core["health"].get("findings").is_none());
        assert!(core["health"].get("files").is_none());
        assert!(core.get("clones").is_none());
        assert_eq!(core["health"]["score"], json!(90.5));
        assert!(!encoded.core.contains("\"functions\""));
        let paths: Vec<&str> = encoded.lazy.iter().map(|(path, _)| *path).collect();
        assert_eq!(
            paths,
            [
                FUNCTIONS_COLUMN,
                "health.files",
                "health.findings",
                "styling.findings",
                "clones"
            ]
        );
    }

    #[test]
    fn function_column_is_absent_when_no_file_has_functions() {
        let mut value = sample();
        value["files"][0]
            .as_object_mut()
            .expect("file")
            .remove("functions");
        let encoded = encode_value(value.clone()).expect("encode");
        assert!(
            encoded
                .lazy
                .iter()
                .all(|(path, _)| *path != FUNCTIONS_COLUMN)
        );
        assert_eq!(hydrate(&encoded), value);
    }

    #[test]
    fn table_is_not_used_when_it_is_longer() {
        let rows = json!([{"a": 1}, {"a": 2}]);
        assert_eq!(encode_tables(rows.clone()), rows);
    }

    #[test]
    fn arrays_with_explicit_null_stay_plain() {
        let rows = json!([
            {"name": "alpha", "value": null},
            {"name": "beta", "value": 1},
            {"name": "gamma", "value": 2}
        ]);
        assert_eq!(encode_tables(rows.clone()), rows);
    }

    #[test]
    fn reserved_key_turns_table_encoding_off() {
        let mut value = sample();
        value["styling"]["summary"] = json!({"$k": ["x"], "$r": [[1]]});
        let encoded = encode_value(value.clone()).expect("encode");
        assert!(!encoded.tables);
        let core: Value = serde_json::from_str(&encoded.core).expect("core JSON");
        assert!(core["files"].is_array());
        assert_eq!(hydrate(&encoded), value);
    }
}
