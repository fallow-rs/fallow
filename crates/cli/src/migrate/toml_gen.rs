use std::fmt::Write as _;

use super::{MigrationResult, source_head};

#[expect(
    clippy::expect_used,
    reason = "migrated config is always stored as a JSON object"
)]
pub(super) fn generate_toml(result: &MigrationResult) -> String {
    let mut output = String::new();
    let source_comment = result
        .sources
        .iter()
        .map(|s| source_head(s))
        .collect::<Vec<_>>()
        .join(", ");
    let _ = writeln!(output, "# Migrated from {source_comment}\n");

    let obj = result
        .config
        .as_object()
        .expect("config is always an Object");

    write_string_array_fields(&mut output, obj);
    write_ignore_exports_used_in_file(&mut output, obj);
    write_boolean_section(&mut output, obj, "audit");
    write_string_section(&mut output, obj, "rules");
    write_duplicates_section(&mut output, obj);

    output
}

fn write_boolean_section(
    output: &mut String,
    obj: &serde_json::Map<String, serde_json::Value>,
    section: &str,
) {
    let Some(section_obj) = obj.get(section).and_then(serde_json::Value::as_object) else {
        return;
    };
    if section_obj.is_empty() {
        return;
    }

    let _ = writeln!(output, "\n[{section}]");
    for (key, value) in section_obj {
        if let Some(enabled) = value.as_bool() {
            let _ = writeln!(output, "{key} = {enabled}");
        }
    }
}

fn write_string_array_fields(
    output: &mut String,
    obj: &serde_json::Map<String, serde_json::Value>,
) {
    for key in &["entry", "ignorePatterns", "ignoreDependencies"] {
        if let Some(value) = obj.get(*key)
            && let Some(arr) = value.as_array()
        {
            let _ = writeln!(output, "{key} = [{}]", quoted_items(arr).join(", "));
        }
    }
}

fn write_ignore_exports_used_in_file(
    output: &mut String,
    obj: &serde_json::Map<String, serde_json::Value>,
) {
    let Some(value) = obj.get("ignoreExportsUsedInFile") else {
        return;
    };

    match value {
        serde_json::Value::Bool(enabled) => {
            let _ = writeln!(output, "ignoreExportsUsedInFile = {enabled}");
        }
        serde_json::Value::Object(kinds) => {
            write_ignore_exports_used_in_file_kinds(output, kinds);
        }
        _ => {}
    }
}

fn write_ignore_exports_used_in_file_kinds(
    output: &mut String,
    kinds: &serde_json::Map<String, serde_json::Value>,
) {
    let entries: Vec<String> = ["type", "interface"]
        .into_iter()
        .filter_map(|key| {
            kinds
                .get(key)
                .and_then(serde_json::Value::as_bool)
                .map(|enabled| format!("{key} = {enabled}"))
        })
        .collect();

    if !entries.is_empty() {
        let _ = writeln!(
            output,
            "ignoreExportsUsedInFile = {{ {} }}",
            entries.join(", ")
        );
    }
}

fn write_string_section(
    output: &mut String,
    obj: &serde_json::Map<String, serde_json::Value>,
    section: &str,
) {
    let Some(section_obj) = obj.get(section).and_then(serde_json::Value::as_object) else {
        return;
    };
    if section_obj.is_empty() {
        return;
    }

    let _ = writeln!(output, "\n[{section}]");
    for (key, value) in section_obj {
        if let Some(s) = value.as_str() {
            let _ = writeln!(output, "{key} = \"{s}\"");
        }
    }
}

fn write_duplicates_section(output: &mut String, obj: &serde_json::Map<String, serde_json::Value>) {
    let Some(dupes_obj) = obj.get("duplicates").and_then(serde_json::Value::as_object) else {
        return;
    };
    if dupes_obj.is_empty() {
        return;
    }

    output.push_str("\n[duplicates]\n");
    for (key, value) in dupes_obj {
        write_duplicates_value(output, key, value);
    }
}

fn write_duplicates_value(output: &mut String, key: &str, value: &serde_json::Value) {
    match value {
        serde_json::Value::Number(n) => {
            let _ = writeln!(output, "{key} = {n}");
        }
        serde_json::Value::Bool(b) => {
            let _ = writeln!(output, "{key} = {b}");
        }
        serde_json::Value::String(s) => {
            let _ = writeln!(output, "{key} = \"{s}\"");
        }
        serde_json::Value::Array(arr) => {
            let _ = writeln!(output, "{key} = [{}]", quoted_items(arr).join(", "));
        }
        _ => {}
    }
}

fn quoted_items(arr: &[serde_json::Value]) -> Vec<String> {
    arr.iter()
        .filter_map(|v| v.as_str().map(|s| format!("\"{s}\"")))
        .collect()
}
