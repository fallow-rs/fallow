//! Sets one value in a fallow config document and renders a new TOML config.

use jsonc_parser::cst::{CstInputValue, CstRootNode};
use toml_edit::{Array, ArrayOfTables, DocumentMut, InlineTable, Item, Table, Value};

use crate::config_writer::{
    BOM, ConfigWriteError, ConfigWriteResult, parse_toml_document, preserve_line_endings,
};

/// Syntax of a fallow config file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigFormat {
    /// JSON or JSONC (`.fallowrc.json`, `.fallowrc.jsonc`).
    Json,
    /// TOML (`fallow.toml`, `.fallow.toml`).
    Toml,
}

/// Render `content` with `value` at `key_path`.
///
/// Replaces an existing value and creates missing parent objects or tables.
/// Keeps comments, the UTF-8 BOM and the line endings of all other parts of
/// the document.
///
/// # Errors
///
/// Returns a parse error when `content` is not valid for `format`. Returns
/// [`ConfigWriteError::InvalidShape`] when `key_path` is empty, when a parent
/// key holds a value that is not an object or table, or when TOML cannot hold
/// `value`.
pub fn set_config_value_in_string(
    content: &str,
    format: ConfigFormat,
    key_path: &[&str],
    value: &serde_json::Value,
) -> ConfigWriteResult<String> {
    let Some((last, parents)) = key_path.split_last() else {
        return Err(ConfigWriteError::InvalidShape(
            "config key path must not be empty".into(),
        ));
    };
    let had_bom = content.starts_with(BOM);
    let body = content.strip_prefix(BOM).unwrap_or(content);
    let rendered = match format {
        ConfigFormat::Json => set_json_value(body, parents, last, value)?,
        ConfigFormat::Toml => set_toml_value(body, parents, last, value)?,
    };
    let with_endings = preserve_line_endings(&rendered, body);
    Ok(if had_bom {
        format!("{BOM}{with_endings}")
    } else {
        with_endings
    })
}

/// Render a JSON object as a new TOML document.
///
/// Each line of `header_comment` becomes a `#` comment line before the first
/// key. An object becomes a table, and an array of objects becomes an array of
/// tables.
///
/// # Errors
///
/// Returns [`ConfigWriteError::InvalidShape`] when `config` is not an object
/// or when TOML cannot hold a value in it.
pub fn render_toml_config(
    config: &serde_json::Value,
    header_comment: Option<&str>,
) -> ConfigWriteResult<String> {
    let serde_json::Value::Object(map) = config else {
        return Err(ConfigWriteError::InvalidShape(
            "config must be an object".into(),
        ));
    };
    let mut doc = DocumentMut::new();
    for (key, member) in map {
        doc.as_table_mut().insert(key, json_to_toml_item(member)?);
    }
    let mut out = String::new();
    if let Some(header) = header_comment {
        for line in header.lines() {
            out.push_str("# ");
            out.push_str(line);
            out.push('\n');
        }
        out.push('\n');
    }
    out.push_str(&doc.to_string());
    Ok(out)
}

/// Render strings as one TOML inline array, with TOML string escapes.
#[must_use]
pub fn toml_string_array(values: &[&str]) -> String {
    let mut array = Array::new();
    for value in values {
        array.push(*value);
    }
    array.to_string()
}

fn set_json_value(
    body: &str,
    parents: &[&str],
    last: &str,
    value: &serde_json::Value,
) -> ConfigWriteResult<String> {
    let root = CstRootNode::parse(body, &crate::jsonc::parse_options())
        .map_err(ConfigWriteError::JsonParse)?;
    let mut object = root.object_value_or_create().ok_or_else(|| {
        ConfigWriteError::InvalidShape("fallow config root must be an object".into())
    })?;
    for key in parents {
        if let Some(prop) = object.get(key)
            && prop
                .value()
                .is_some_and(|node| node.as_null_keyword().is_some())
        {
            prop.set_value(CstInputValue::Object(Vec::new()));
        }
        object = object.object_value_or_create(key).ok_or_else(|| {
            ConfigWriteError::InvalidShape(format!("{key} must be an object in fallow config"))
        })?;
    }
    let input = json_to_cst_input(value);
    match object.get(last) {
        Some(prop) => prop.set_value(input),
        None => {
            object.append(last, input);
        }
    }
    Ok(root.to_string())
}

fn json_to_cst_input(value: &serde_json::Value) -> CstInputValue {
    match value {
        serde_json::Value::Null => CstInputValue::Null,
        serde_json::Value::Bool(flag) => CstInputValue::Bool(*flag),
        serde_json::Value::Number(number) => CstInputValue::Number(number.to_string()),
        serde_json::Value::String(text) => CstInputValue::String(text.clone()),
        serde_json::Value::Array(items) => {
            CstInputValue::Array(items.iter().map(json_to_cst_input).collect())
        }
        serde_json::Value::Object(map) => CstInputValue::Object(
            map.iter()
                .map(|(key, member)| (key.clone(), json_to_cst_input(member)))
                .collect(),
        ),
    }
}

fn set_toml_value(
    body: &str,
    parents: &[&str],
    last: &str,
    value: &serde_json::Value,
) -> ConfigWriteResult<String> {
    let mut doc = parse_toml_document(body)?;
    set_in_table(doc.as_table_mut(), parents, last, value)?;
    Ok(doc.to_string())
}

fn not_a_table(key: &str) -> ConfigWriteError {
    ConfigWriteError::InvalidShape(format!("{key} must be a table in fallow config"))
}

fn set_in_table(
    table: &mut Table,
    parents: &[&str],
    last: &str,
    value: &serde_json::Value,
) -> ConfigWriteResult<()> {
    let Some((key, rest)) = parents.split_first() else {
        let mut item = json_to_toml_item(value)?;
        // The replaced section keeps its place and the comment lines above its header.
        if let (Some(new), Some(old)) = (
            item.as_table_mut(),
            table.get(last).and_then(Item::as_table),
        ) {
            new.set_position(old.position());
            *new.decor_mut() = old.decor().clone();
        }
        table.insert(last, item);
        return Ok(());
    };
    if !table.contains_key(key) {
        let mut parent = Table::new();
        parent.set_implicit(true);
        table.insert(key, Item::Table(parent));
    }
    match table.get_mut(key) {
        Some(Item::Table(child)) => set_in_table(child, rest, last, value),
        Some(Item::Value(Value::InlineTable(child))) => {
            set_in_inline_table(child, rest, last, value)
        }
        _ => Err(not_a_table(key)),
    }
}

fn set_in_inline_table(
    table: &mut InlineTable,
    parents: &[&str],
    last: &str,
    value: &serde_json::Value,
) -> ConfigWriteResult<()> {
    let Some((key, rest)) = parents.split_first() else {
        table.insert(last, json_to_toml_value(value)?);
        return Ok(());
    };
    if !table.contains_key(key) {
        table.insert(*key, Value::InlineTable(InlineTable::new()));
    }
    match table.get_mut(key) {
        Some(Value::InlineTable(child)) => set_in_inline_table(child, rest, last, value),
        _ => Err(not_a_table(key)),
    }
}

/// Convert a JSON value for a table position: an object becomes a table, and
/// an array of objects becomes an array of tables.
fn json_to_toml_item(value: &serde_json::Value) -> ConfigWriteResult<Item> {
    match value {
        serde_json::Value::Object(map) => {
            let mut table = Table::new();
            for (key, member) in map {
                table.insert(key, json_to_toml_item(member)?);
            }
            Ok(Item::Table(table))
        }
        serde_json::Value::Array(items)
            if !items.is_empty() && items.iter().all(serde_json::Value::is_object) =>
        {
            let mut tables = ArrayOfTables::new();
            for element in items {
                if let Item::Table(table) = json_to_toml_item(element)? {
                    tables.push(table);
                }
            }
            Ok(Item::ArrayOfTables(tables))
        }
        other => Ok(Item::Value(json_to_toml_value(other)?)),
    }
}

/// Convert a JSON value for an inline position: an object becomes an inline table.
fn json_to_toml_value(value: &serde_json::Value) -> ConfigWriteResult<Value> {
    match value {
        serde_json::Value::Null => Err(ConfigWriteError::InvalidShape(
            "TOML cannot hold a null value".into(),
        )),
        serde_json::Value::Bool(flag) => Ok(Value::from(*flag)),
        serde_json::Value::Number(number) => {
            if let Some(integer) = number.as_i64() {
                Ok(Value::from(integer))
            } else if number.is_f64()
                && let Some(float) = number.as_f64()
            {
                Ok(Value::from(float))
            } else {
                Err(ConfigWriteError::InvalidShape(format!(
                    "TOML cannot hold the number {number}"
                )))
            }
        }
        serde_json::Value::String(text) => Ok(Value::from(text.as_str())),
        serde_json::Value::Array(items) => {
            let mut array = Array::new();
            for element in items {
                array.push(json_to_toml_value(element)?);
            }
            Ok(Value::Array(array))
        }
        serde_json::Value::Object(map) => {
            let mut table = InlineTable::new();
            for (key, member) in map {
                table.insert(key.as_str(), json_to_toml_value(member)?);
            }
            Ok(Value::InlineTable(table))
        }
    }
}

#[cfg(test)]
#[path = "config_value_writer_tests.rs"]
mod tests;
