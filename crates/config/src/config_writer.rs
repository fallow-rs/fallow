use std::error::Error;
use std::fmt;
use std::io::Write;
use std::path::Path;

use jsonc_parser::cst::{CstInputValue, CstRootNode};
use rustc_hash::FxHashSet;
use tempfile::NamedTempFile;
use toml_edit::{Array, ArrayOfTables, DocumentMut, InlineTable, Item, Table, Value};

use crate::IgnoreExportRule;

#[derive(Debug)]
pub enum ConfigWriteError {
    Io(std::io::Error),
    JsonParse(jsonc_parser::errors::ParseError),
    TomlParse(toml_edit::TomlError),
    InvalidShape(String),
}

impl fmt::Display for ConfigWriteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "{e}"),
            Self::JsonParse(e) => write!(f, "{e}"),
            Self::TomlParse(e) => write!(f, "{e}"),
            Self::InvalidShape(msg) => f.write_str(msg),
        }
    }
}

impl Error for ConfigWriteError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            Self::JsonParse(e) => Some(e),
            Self::TomlParse(e) => Some(e),
            Self::InvalidShape(_) => None,
        }
    }
}

impl From<std::io::Error> for ConfigWriteError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

pub type ConfigWriteResult<T> = Result<T, ConfigWriteError>;

/// Atomically write content to a file via a temporary file and rename.
///
/// Resolves symlinks at the target path before persisting so the rename
/// writes through to the symlink's target file rather than replacing the
/// symlink itself with a regular file (common when configs are mounted into
/// containers via symlinks).
pub fn atomic_write(path: &Path, content: &[u8]) -> std::io::Result<()> {
    let resolved = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let dir = resolved.parent().unwrap_or_else(|| Path::new("."));
    let mut tmp = NamedTempFile::new_in(dir)?;
    tmp.write_all(content)?;
    tmp.as_file().sync_all()?;
    tmp.persist(&resolved).map_err(|e| e.error)?;
    Ok(())
}

/// Append `ignoreExports` rules to an existing fallow config file.
///
/// Existing entries keep their order and exact formatting. New entries are
/// appended only when no existing entry has the same `file` value.
pub fn add_ignore_exports_rule(path: &Path, entries: &[IgnoreExportRule]) -> ConfigWriteResult<()> {
    if entries.is_empty() {
        return Ok(());
    }
    let content = std::fs::read_to_string(path)?;
    let rendered = add_ignore_exports_rule_to_string(path, &content, entries)?;
    atomic_write(path, rendered.as_bytes())?;
    Ok(())
}

fn add_ignore_exports_rule_to_string(
    path: &Path,
    content: &str,
    entries: &[IgnoreExportRule],
) -> ConfigWriteResult<String> {
    let had_bom = content.starts_with(BOM);
    let body = content.strip_prefix(BOM).unwrap_or(content);
    let config_dir = path.parent().unwrap_or_else(|| Path::new(""));
    let rendered = if is_json_config(path) {
        append_json_ignore_exports(body, entries, config_dir)?
    } else {
        append_toml_ignore_exports(body, entries, config_dir)?
    };
    let with_endings = preserve_line_endings(&rendered, body);
    Ok(if had_bom {
        let mut out = String::with_capacity(with_endings.len() + BOM.len_utf8());
        out.push(BOM);
        out.push_str(&with_endings);
        out
    } else {
        with_endings
    })
}

const BOM: char = '\u{FEFF}';

fn is_json_config(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|ext| ext.to_str()),
        Some("json" | "jsonc")
    )
}

fn append_json_ignore_exports(
    content: &str,
    entries: &[IgnoreExportRule],
    config_dir: &Path,
) -> ConfigWriteResult<String> {
    let root = CstRootNode::parse(content, &crate::jsonc::parse_options())
        .map_err(ConfigWriteError::JsonParse)?;
    let object = root.object_value_or_create().ok_or_else(|| {
        ConfigWriteError::InvalidShape("fallow config root must be an object".into())
    })?;
    let array = object
        .array_value_or_create("ignoreExports")
        .ok_or_else(|| {
            ConfigWriteError::InvalidShape("ignoreExports must be an array in fallow config".into())
        })?;

    let mut seen = FxHashSet::default();
    for element in array.elements() {
        if let Some(file) = element.to_serde_value().and_then(|value| {
            value
                .get("file")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        }) {
            record_existing_file(&mut seen, &file, config_dir);
        }
    }

    for entry in entries {
        if seen.insert(entry.file.clone()) {
            array.append(CstInputValue::Object(vec![
                ("file".to_owned(), CstInputValue::String(entry.file.clone())),
                (
                    "exports".to_owned(),
                    CstInputValue::Array(
                        entry
                            .exports
                            .iter()
                            .cloned()
                            .map(CstInputValue::String)
                            .collect(),
                    ),
                ),
            ]));
        }
    }
    Ok(root.to_string())
}

fn append_toml_ignore_exports(
    content: &str,
    entries: &[IgnoreExportRule],
    config_dir: &Path,
) -> ConfigWriteResult<String> {
    let mut doc = content
        .parse::<DocumentMut>()
        .map_err(ConfigWriteError::TomlParse)?;
    match doc
        .as_table_mut()
        .entry("ignoreExports")
        .or_insert(Item::None)
    {
        Item::None => {
            let mut tables = ArrayOfTables::new();
            let mut seen = FxHashSet::default();
            append_to_array_of_tables(&mut tables, entries, &mut seen);
            doc.as_table_mut()
                .insert("ignoreExports", Item::ArrayOfTables(tables));
        }
        Item::ArrayOfTables(tables) => {
            let mut seen = files_from_array_of_tables(tables, config_dir);
            append_to_array_of_tables(tables, entries, &mut seen);
        }
        Item::Value(Value::Array(array)) => {
            let mut seen = files_from_inline_array(array, config_dir);
            append_to_inline_array(array, entries, &mut seen);
        }
        _ => {
            return Err(ConfigWriteError::InvalidShape(
                "ignoreExports must be an array of tables or inline array in fallow config".into(),
            ));
        }
    }
    Ok(doc.to_string())
}

fn files_from_array_of_tables(tables: &ArrayOfTables, config_dir: &Path) -> FxHashSet<String> {
    let mut seen = FxHashSet::default();
    for table in tables {
        if let Some(file) = table.get("file").and_then(Item::as_str) {
            record_existing_file(&mut seen, file, config_dir);
        }
    }
    seen
}

fn append_to_array_of_tables(
    tables: &mut ArrayOfTables,
    entries: &[IgnoreExportRule],
    seen: &mut FxHashSet<String>,
) {
    for entry in entries {
        if seen.insert(entry.file.clone()) {
            tables.push(toml_ignore_export_table(entry));
        }
    }
}

fn toml_ignore_export_table(entry: &IgnoreExportRule) -> Table {
    let mut table = Table::new();
    table.insert("file", toml_edit::value(entry.file.clone()));
    table.insert("exports", Item::Value(Value::Array(exports_array(entry))));
    table
}

fn files_from_inline_array(array: &Array, config_dir: &Path) -> FxHashSet<String> {
    let mut seen = FxHashSet::default();
    for value in array {
        if let Some(file) = value
            .as_inline_table()
            .and_then(|table| table.get("file"))
            .and_then(Value::as_str)
        {
            record_existing_file(&mut seen, file, config_dir);
        }
    }
    seen
}

/// Insert an existing-entry path into the dedupe set under its canonical key.
///
/// The canonical key is the entry as written. When the existing entry is an
/// absolute path that resolves under the config dir, also insert the
/// dir-relative form so a new entry emitted by the action builder (which is
/// always config-dir-relative) is recognised as a duplicate.
fn record_existing_file(seen: &mut FxHashSet<String>, file: &str, config_dir: &Path) {
    seen.insert(file.to_owned());
    let path = Path::new(file);
    if path.is_absolute()
        && let Ok(relative) = path.strip_prefix(config_dir)
    {
        seen.insert(relative.to_string_lossy().replace('\\', "/"));
    }
}

fn append_to_inline_array(
    array: &mut Array,
    entries: &[IgnoreExportRule],
    seen: &mut FxHashSet<String>,
) {
    for entry in entries {
        if seen.insert(entry.file.clone()) {
            array.push(Value::InlineTable(toml_ignore_export_inline_table(entry)));
        }
    }
}

fn toml_ignore_export_inline_table(entry: &IgnoreExportRule) -> InlineTable {
    let mut table = InlineTable::new();
    table.insert("file", Value::from(entry.file.clone()));
    table.insert("exports", Value::Array(exports_array(entry)));
    table
}

fn exports_array(entry: &IgnoreExportRule) -> Array {
    let mut exports = Array::new();
    for export in &entry.exports {
        exports.push(export.as_str());
    }
    exports
}

fn preserve_line_endings(rendered: &str, original: &str) -> String {
    if original.contains("\r\n") {
        rendered.replace("\r\n", "\n").replace('\n', "\r\n")
    } else {
        rendered.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(file: &str) -> IgnoreExportRule {
        IgnoreExportRule {
            file: file.to_owned(),
            exports: vec!["*".to_owned()],
        }
    }

    #[test]
    fn appends_json_ignore_exports() {
        let output = add_ignore_exports_rule_to_string(
            Path::new(".fallowrc.json"),
            "{\n}\n",
            &[rule("src/index.ts")],
        )
        .unwrap();
        assert!(output.contains("\"ignoreExports\": ["));
        assert!(output.contains("\"file\": \"src/index.ts\""));
        assert!(output.ends_with('\n'));
    }

    #[test]
    fn appends_jsonc_preserving_comments() {
        let input = "{\n  // keep this\n  \"rules\": {}\n}\n";
        let output = add_ignore_exports_rule_to_string(
            Path::new(".fallowrc.jsonc"),
            input,
            &[rule("src/a.ts")],
        )
        .unwrap();
        assert!(output.contains("// keep this"));
        assert!(output.contains("\"rules\": {}"));
        assert!(output.contains("\"file\": \"src/a.ts\""));
    }

    #[test]
    fn merges_existing_json_ignore_exports_without_reordering_or_replacing() {
        let input = "{\n  \"ignoreExports\": [\n    { \"file\": \"src/a.ts\", \"exports\": [\"*\"] }\n  ],\n  \"rules\": {}\n}\n";
        let output = add_ignore_exports_rule_to_string(
            Path::new(".fallowrc.json"),
            input,
            &[rule("src/a.ts"), rule("src/b.ts")],
        )
        .unwrap();
        assert_eq!(output.matches("\"file\": \"src/a.ts\"").count(), 1);
        assert!(output.find("\"file\": \"src/a.ts\"") < output.find("\"file\": \"src/b.ts\""));
        assert!(output.contains("\"rules\": {}"));
    }

    #[test]
    fn appends_toml_ignore_exports() {
        let output = add_ignore_exports_rule_to_string(
            Path::new("fallow.toml"),
            "production = true\n",
            &[rule("src/index.ts")],
        )
        .unwrap();
        assert!(output.contains("production = true"));
        assert!(output.contains("[[ignoreExports]]"));
        assert!(output.contains("file = \"src/index.ts\""));
        assert!(output.contains("exports = [\"*\"]"));
    }

    #[test]
    fn appends_dot_fallow_toml_ignore_exports() {
        let output = add_ignore_exports_rule_to_string(
            Path::new(".fallow.toml"),
            "",
            &[rule("src/index.ts")],
        )
        .unwrap();
        assert!(output.contains("[[ignoreExports]]"));
        assert!(output.contains("file = \"src/index.ts\""));
    }

    #[test]
    fn merges_existing_toml_ignore_exports() {
        let input = "[[ignoreExports]]\nfile = \"src/a.ts\"\nexports = [\"*\"]\n";
        let output = add_ignore_exports_rule_to_string(
            Path::new("fallow.toml"),
            input,
            &[rule("src/a.ts"), rule("src/b.ts")],
        )
        .unwrap();
        assert_eq!(output.matches("file = \"src/a.ts\"").count(), 1);
        assert!(output.contains("file = \"src/b.ts\""));
    }

    #[test]
    fn preserves_crlf_line_endings() {
        let input = "{\r\n  \"rules\": {}\r\n}\r\n";
        let output = add_ignore_exports_rule_to_string(
            Path::new(".fallowrc.json"),
            input,
            &[rule("src/a.ts")],
        )
        .unwrap();
        assert!(output.contains("\r\n"));
        assert!(!output.contains("\r\r"));
        assert!(!output.replace("\r\n", "").contains('\n'));
    }

    #[test]
    fn preserves_toml_crlf_line_endings_without_double_carriage_returns() {
        let input = "production = true\r\n";
        let output =
            add_ignore_exports_rule_to_string(Path::new("fallow.toml"), input, &[rule("src/a.ts")])
                .unwrap();
        assert!(output.contains("\r\n"));
        assert!(!output.contains("\r\r"));
        assert!(!output.replace("\r\n", "").contains('\n'));
    }

    #[test]
    fn preserves_utf8_bom_on_json_config() {
        let input = "\u{FEFF}{\n  \"rules\": {}\n}\n";
        let output = add_ignore_exports_rule_to_string(
            Path::new(".fallowrc.json"),
            input,
            &[rule("src/a.ts")],
        )
        .unwrap();
        assert!(output.starts_with('\u{FEFF}'), "BOM stripped from output");
        assert!(output.matches('\u{FEFF}').count() == 1, "BOM duplicated");
        assert!(output.contains("\"file\": \"src/a.ts\""));
    }

    #[test]
    fn preserves_utf8_bom_on_toml_config() {
        let input = "\u{FEFF}production = true\n";
        let output =
            add_ignore_exports_rule_to_string(Path::new("fallow.toml"), input, &[rule("src/a.ts")])
                .unwrap();
        assert!(output.starts_with('\u{FEFF}'), "BOM stripped from output");
        assert!(output.matches('\u{FEFF}').count() == 1, "BOM duplicated");
        assert!(output.contains("[[ignoreExports]]"));
    }

    #[test]
    fn no_bom_added_when_input_had_none() {
        let input = "{\n}\n";
        let output = add_ignore_exports_rule_to_string(
            Path::new(".fallowrc.json"),
            input,
            &[rule("src/a.ts")],
        )
        .unwrap();
        assert!(!output.starts_with('\u{FEFF}'));
    }

    #[test]
    fn dedupes_existing_absolute_paths_against_relative_emissions() {
        let config_dir = Path::new("/project");
        let config_path = config_dir.join(".fallowrc.json");
        let input = "{\n  \"ignoreExports\": [\n    { \"file\": \"/project/src/a.ts\", \"exports\": [\"*\"] }\n  ]\n}\n";
        let output =
            add_ignore_exports_rule_to_string(&config_path, input, &[rule("src/a.ts")]).unwrap();
        assert_eq!(
            output.matches("\"src/a.ts\"").count(),
            0,
            "writer must not add a relative duplicate of an existing absolute entry"
        );
        assert_eq!(
            output.matches("\"/project/src/a.ts\"").count(),
            1,
            "existing absolute entry must remain"
        );
    }

    #[test]
    fn dedupes_existing_absolute_paths_against_relative_emissions_toml() {
        let config_dir = Path::new("/project");
        let config_path = config_dir.join("fallow.toml");
        let input = "[[ignoreExports]]\nfile = \"/project/src/a.ts\"\nexports = [\"*\"]\n";
        let output =
            add_ignore_exports_rule_to_string(&config_path, input, &[rule("src/a.ts")]).unwrap();
        assert_eq!(
            output.matches("file = \"src/a.ts\"").count(),
            0,
            "writer must not add a relative duplicate of an existing absolute TOML entry"
        );
        assert_eq!(output.matches("file = \"/project/src/a.ts\"").count(), 1);
    }
}
