//! Migration of the knip `cycles` section to fallow `circularDependencies`.

use serde_json::{Map, Value};

use super::MigrationWarning;

type JsonMap = Map<String, Value>;

const FIELD: &str = "cycles";

/// Migrate knip `cycles.allow` to `circularDependencies.allow`.
///
/// Both lists hold the files of one cycle in import order, relative to the
/// project root, without the repeat of the first file. Fallow also accepts a
/// rotation of the entry, so each knip entry keeps its meaning. An entry that
/// is not a list of strings gets a warning. `cycles.dynamicImports: true` is
/// the fallow default. Any other `dynamicImports` value and an unknown key
/// get a warning, because fallow has no exact equivalent.
pub(super) fn migrate_cycles(
    value: &Value,
    config: &mut JsonMap,
    warnings: &mut Vec<MigrationWarning>,
) {
    let Some(obj) = value.as_object() else {
        warnings.push(warning(
            FIELD,
            "expected an object".to_owned(),
            Some("write cycles as {\"allow\": [[\"src/a.ts\", \"src/b.ts\"]]}"),
        ));
        return;
    };
    for (key, value) in obj {
        match key.as_str() {
            "allow" => migrate_allow(value, config, warnings),
            "dynamicImports" if value.as_bool() == Some(true) => {}
            "dynamicImports" => warnings.push(warning(
                "cycles.dynamicImports",
                "fallow follows import() edges when it looks for cycles; not migrated".to_owned(),
                Some(
                    "set circularDependencies.ignoreLazyImports to skip lazy import() and \
                     worker edges",
                ),
            )),
            _ => warnings.push(warning(
                &format!("cycles.{key}"),
                format!("unknown knip cycles key `{key}`; not migrated"),
                None,
            )),
        }
    }
}

fn migrate_allow(value: &Value, config: &mut JsonMap, warnings: &mut Vec<MigrationWarning>) {
    let Some(entries) = value.as_array() else {
        warnings.push(warning(
            "cycles.allow",
            "expected an array of file lists".to_owned(),
            None,
        ));
        return;
    };
    let mut migrated = Vec::new();
    for entry in entries {
        match file_list(entry) {
            Some(files) => migrated.push(Value::Array(files)),
            None => warnings.push(warning(
                "cycles.allow",
                format!("entry {entry} is not a non-empty list of file paths; not migrated"),
                None,
            )),
        }
    }
    if migrated.is_empty() {
        return;
    }
    let section = config
        .entry("circularDependencies".to_owned())
        .or_insert_with(|| Value::Object(Map::new()));
    if let Some(section) = section.as_object_mut() {
        section.insert("allow".to_owned(), Value::Array(migrated));
    }
}

fn file_list(entry: &Value) -> Option<Vec<Value>> {
    let files = entry.as_array()?;
    if files.is_empty() || !files.iter().all(Value::is_string) {
        return None;
    }
    Some(files.clone())
}

fn warning(field: &str, message: String, suggestion: Option<&str>) -> MigrationWarning {
    MigrationWarning {
        source: "knip",
        field: field.to_owned(),
        message,
        suggestion: suggestion.map(str::to_owned),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn migrate(knip_cycles: &str) -> (JsonMap, Vec<MigrationWarning>) {
        let value: Value = serde_json::from_str(knip_cycles).expect("valid JSON");
        let mut config = JsonMap::new();
        let mut warnings = Vec::new();
        migrate_cycles(&value, &mut config, &mut warnings);
        (config, warnings)
    }

    #[test]
    fn allow_becomes_circular_dependencies_allow() {
        let (config, warnings) = migrate(r#"{"allow": [["src/a.ts", "src/b.ts"]]}"#);
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(
            config["circularDependencies"]["allow"],
            serde_json::json!([["src/a.ts", "src/b.ts"]])
        );
    }

    #[test]
    fn an_invalid_allow_entry_warns_and_the_others_stay() {
        let (config, warnings) = migrate(r#"{"allow": [["src/a.ts", "src/b.ts"], [], "x"]}"#);
        assert_eq!(warnings.len(), 2, "{warnings:?}");
        assert!(warnings.iter().all(|w| w.field == "cycles.allow"));
        assert_eq!(
            config["circularDependencies"]["allow"],
            serde_json::json!([["src/a.ts", "src/b.ts"]])
        );
    }

    #[test]
    fn dynamic_imports_true_is_the_default_and_needs_no_warning() {
        let (config, warnings) = migrate(r#"{"dynamicImports": true}"#);
        assert!(config.is_empty());
        assert!(warnings.is_empty(), "{warnings:?}");
    }

    #[test]
    fn dynamic_imports_false_and_unknown_keys_warn() {
        let (config, warnings) = migrate(r#"{"dynamicImports": false, "other": 1}"#);
        assert!(config.is_empty());
        let fields: Vec<&str> = warnings.iter().map(|w| w.field.as_str()).collect();
        assert_eq!(fields, vec!["cycles.dynamicImports", "cycles.other"]);
    }

    #[test]
    fn a_non_object_warns() {
        let (config, warnings) = migrate("true");
        assert!(config.is_empty());
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].field, "cycles");
    }
}
