//! Typed rendering and saved-input validation for advisory shared-state observations.

use fallow_types::shared_state::{SharedStateInventory, StateSite};
use std::fmt::Write;

/// Render the inventory without representing observations as issue findings.
#[must_use]
pub fn markdown(inventory: &SharedStateInventory, root: &std::path::Path) -> String {
    let mut out = String::from(
        "\n## Shared-state inventory\n\nStatic sites only. Shared runtime and execution order are unknown.\n",
    );
    if !inventory.complete {
        out.push_str("\nInventory is incomplete. Parsing or retention limits omitted evidence.\n");
    }
    if inventory.imported_writes.is_empty() && inventory.globals.is_empty() {
        out.push_str("\nNo supported shared-state observations.\n");
        return out;
    }
    for write in &inventory.imported_writes {
        let property = serde_json::Value::from(write.property.clone()).to_string();
        let _ = writeln!(
            out,
            "\n- Imported write at {}: {} {} ({}); declaration {}.",
            location(&write.writer, root),
            crate::markdown_code_span(&write.binding),
            crate::markdown_code_span(&property),
            write.operation.label(),
            location(&write.declaration, root)
        );
    }
    for group in &inventory.globals {
        let _ = writeln!(
            out,
            "\n### {} {}\n",
            crate::markdown_code_span(&group.root),
            crate::markdown_code_span(&group.slot)
        );
        for (label, sites) in [("Read", &group.reads), ("Write", &group.writes)] {
            for access in sites {
                let property = serde_json::Value::from(access.property.clone()).to_string();
                let _ = writeln!(
                    out,
                    "- {label}: {} {} ({})",
                    location(&access.site, root),
                    crate::markdown_code_span(&property),
                    access.operation.label()
                );
            }
        }
    }
    out.push_str("\nReview ownership and initialization manually. Intentional sites can use `fallow-ignore-next-line shared-state`.\n");
    out
}

fn location(site: &StateSite, root: &std::path::Path) -> String {
    crate::markdown_code_span(&format!(
        "{}:{}:{}",
        site.path.strip_prefix(root).unwrap_or(&site.path).display(),
        site.line,
        site.column
    ))
}

/// Validate every saved inventory body, including an explicitly empty one.
/// Returns whether any inventory was present.
///
/// # Errors
/// Returns a shape/version error instead of silently dropping unsupported data.
pub fn validate_saved(value: &serde_json::Value) -> Result<bool, String> {
    let mut present = false;
    match value {
        serde_json::Value::Object(map) => {
            for (key, value) in map {
                if key == "shared_state" {
                    let inventory: SharedStateInventory = serde_json::from_value(value.clone())
                        .map_err(|error| {
                            format!("invalid saved shared_state inventory: {error}")
                        })?;
                    validate(&inventory)?;
                    present = true;
                } else {
                    present |= validate_saved(value)?;
                }
            }
            if present
                && (map.contains_key("grouped_by")
                    || map
                        .get("kind")
                        .and_then(serde_json::Value::as_str)
                        .is_some_and(|kind| kind.ends_with("-grouped")))
            {
                return Err(
                    "saved shared-state inventory does not support grouped reports".to_owned(),
                );
            }
        }
        serde_json::Value::Array(values) => {
            for value in values {
                present |= validate_saved(value)?;
            }
        }
        _ => {}
    }
    Ok(present)
}

fn validate(inventory: &SharedStateInventory) -> Result<(), String> {
    if inventory.version != 1 {
        return Err(format!(
            "unsupported shared_state version {}",
            inventory.version
        ));
    }
    if inventory.truncated && inventory.complete {
        return Err("truncated shared_state inventory cannot be complete".to_owned());
    }
    let mut actions = inventory
        .imported_writes
        .iter()
        .flat_map(|write| &write.actions)
        .chain(inventory.globals.iter().flat_map(|group| &group.actions));
    if actions.any(|action| action.auto_fixable) {
        return Err("shared_state actions must be manual".to_owned());
    }
    Ok(())
}

/// Normalize only source paths, preserving literal property keys and identities.
pub(crate) fn normalize_paths(value: &mut serde_json::Value, prefix: &str) {
    let Some(object) = value.as_object_mut() else {
        return;
    };
    for write in object
        .get_mut("imported_writes")
        .and_then(serde_json::Value::as_array_mut)
        .into_iter()
        .flatten()
    {
        for key in ["writer", "declaration"] {
            if let Some(path) = write.get_mut(key).and_then(|site| site.get_mut("path")) {
                normalize_source_path(path, prefix);
            }
        }
    }
    for group in object
        .get_mut("globals")
        .and_then(serde_json::Value::as_array_mut)
        .into_iter()
        .flatten()
    {
        for key in ["reads", "writes"] {
            for access in group
                .get_mut(key)
                .and_then(serde_json::Value::as_array_mut)
                .into_iter()
                .flatten()
            {
                if let Some(path) = access.get_mut("site").and_then(|site| site.get_mut("path")) {
                    normalize_source_path(path, prefix);
                }
            }
        }
    }
}

fn normalize_source_path(value: &mut serde_json::Value, prefix: &str) {
    let Some(path) = value.as_str() else { return };
    let path = path.replace('\\', "/");
    let prefix = prefix.replace('\\', "/");
    *value = serde_json::Value::String(path.strip_prefix(&prefix).unwrap_or(&path).to_owned());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saved_shared_state_validates_empty_bodies_and_preserves_literal_paths() {
        let mut value = serde_json::json!({"shared_state": {"version": 1, "complete": true, "truncated": false, "imported_writes": [], "globals": []}});
        assert_eq!(validate_saved(&value), Ok(true));
        assert_eq!(validate_saved(&serde_json::json!({})), Ok(false));
        value["shared_state"]["version"] = serde_json::json!(2);
        assert!(validate_saved(&value).is_err());
        value["shared_state"] = serde_json::Value::Null;
        assert!(validate_saved(&value).is_err());
        let mut paths = serde_json::json!({"shared_state": {"imported_writes": [{"writer": {"path": "/project/src/writer.ts"}, "declaration": {"path": "/project/src/owner.ts"}, "property": ["/project/a.b", "a.b"]}], "globals": []}});
        crate::strip_root_prefix(&mut paths, "/project/");
        assert_eq!(
            paths["shared_state"]["imported_writes"][0]["writer"]["path"],
            "src/writer.ts"
        );
        assert_eq!(
            paths["shared_state"]["imported_writes"][0]["property"],
            serde_json::json!(["/project/a.b", "a.b"])
        );
    }
}

#[cfg(test)]
mod path_tests {
    #[test]
    fn shared_state_windows_paths_preserve_literal_segments() {
        let mut report = serde_json::json!({"shared_state": {"imported_writes": [{"writer": {"path": "C:\\project\\src\\[id].ts"}, "declaration": {"path": "C:\\project\\src\\state.ts"}, "property": ["C:\\project\\literal"]}], "globals": []}});
        crate::strip_root_prefix(&mut report, "C:\\project\\");
        assert_eq!(
            report["shared_state"]["imported_writes"][0]["writer"]["path"],
            "src/[id].ts"
        );
        assert_eq!(
            report["shared_state"]["imported_writes"][0]["property"][0],
            "C:\\project\\literal"
        );
    }
}
