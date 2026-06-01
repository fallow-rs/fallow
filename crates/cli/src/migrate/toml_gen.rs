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

    for key in &["entry", "ignorePatterns", "ignoreDependencies"] {
        if let Some(value) = obj.get(*key)
            && let Some(arr) = value.as_array()
        {
            let items: Vec<String> = arr
                .iter()
                .filter_map(|v| v.as_str().map(|s| format!("\"{s}\"")))
                .collect();
            let _ = writeln!(output, "{key} = [{}]", items.join(", "));
        }
    }

    if let Some(value) = obj.get("ignoreExportsUsedInFile") {
        match value {
            serde_json::Value::Bool(enabled) => {
                let _ = writeln!(output, "ignoreExportsUsedInFile = {enabled}");
            }
            serde_json::Value::Object(kinds) => {
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
            _ => {}
        }
    }

    if let Some(rules) = obj.get("rules")
        && let Some(rules_obj) = rules.as_object()
        && !rules_obj.is_empty()
    {
        output.push_str("\n[rules]\n");
        for (key, value) in rules_obj {
            if let Some(s) = value.as_str() {
                let _ = writeln!(output, "{key} = \"{s}\"");
            }
        }
    }

    if let Some(dupes) = obj.get("duplicates")
        && let Some(dupes_obj) = dupes.as_object()
        && !dupes_obj.is_empty()
    {
        output.push_str("\n[duplicates]\n");
        for (key, value) in dupes_obj {
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
                    let items: Vec<String> = arr
                        .iter()
                        .filter_map(|v| v.as_str().map(|s| format!("\"{s}\"")))
                        .collect();
                    let _ = writeln!(output, "{key} = [{}]", items.join(", "));
                }
                _ => {}
            }
        }
    }

    output
}
