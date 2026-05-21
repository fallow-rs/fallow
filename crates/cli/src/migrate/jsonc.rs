use std::fmt::Write as _;

use super::{MigrationResult, source_head};

pub(super) fn generate_jsonc(result: &MigrationResult) -> String {
    let mut output = String::new();
    output.push_str("{\n");
    output.push_str(
        "  \"$schema\": \"https://raw.githubusercontent.com/fallow-rs/fallow/main/schema.json\",\n",
    );

    let obj = result
        .config
        .as_object()
        .expect("config is always an Object");
    // Strip internal tool tags (`(knip config)`, `(jscpd config)`, ...) from
    // the comment so the committed config file does not carry implementation
    // detail. See issue #457.
    let source_comment = result
        .sources
        .iter()
        .map(|s| source_head(s))
        .collect::<Vec<_>>()
        .join(", ");
    let _ = writeln!(output, "  // Migrated from {source_comment}");

    let mut entries: Vec<(&String, &serde_json::Value)> = obj.iter().collect();
    // Sort keys for consistent output
    let key_order = [
        "entry",
        "ignorePatterns",
        "ignoreDependencies",
        "ignoreExportsUsedInFile",
        "rules",
        "duplicates",
    ];
    entries.sort_by_key(|(k, _)| {
        key_order
            .iter()
            .position(|o| *o == k.as_str())
            .unwrap_or(usize::MAX)
    });

    let total = entries.len();
    for (i, (key, value)) in entries.iter().enumerate() {
        let is_last = i == total - 1;
        let serialized = serde_json::to_string_pretty(value).unwrap_or_default();
        // Indent the serialized value by 2 spaces (but the first line is on the key line)
        let indented = indent_json_value(&serialized, 2);
        if is_last {
            let _ = writeln!(output, "  \"{key}\": {indented}");
        } else {
            let _ = writeln!(output, "  \"{key}\": {indented},");
        }
    }

    output.push_str("}\n");
    output
}

/// Indent a pretty-printed JSON value's continuation lines.
pub(super) fn indent_json_value(json: &str, spaces: usize) -> String {
    let indent = " ".repeat(spaces);
    let mut lines: Vec<&str> = json.lines().collect();
    if lines.len() <= 1 {
        return json.to_string();
    }
    // First line stays as-is, subsequent lines get indented
    let first = lines.remove(0);
    let rest: Vec<String> = lines.iter().map(|l| format!("{indent}{l}")).collect();
    let mut result = first.to_string();
    for line in rest {
        result.push('\n');
        result.push_str(&line);
    }
    result
}
