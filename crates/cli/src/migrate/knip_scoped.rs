//! Knip settings that name file paths, for the root and for each workspace.
//!
//! Fallow config paths are project-root-relative. A knip workspace section is
//! relative to its workspace key, so the migration puts the key in front of
//! each pattern. A glob key such as `packages/*` becomes the `packages/*/`
//! prefix, which matches the same package roots.

use serde_json::{Map, Value};

use super::knip_fields::{
    migrate_ignore, migrate_ignore_binaries, migrate_ignore_deps, migrate_namespace_issue_type,
    warn_unmapped_rule_key,
};
use super::knip_tables::{
    KNIP_PLUGIN_KEYS, KNIP_RULE_MAP, KNIP_UNMAPPABLE_FIELDS, KNIP_UNSUPPORTED_PLUGIN_KEYS,
};
use super::{MigrationWarning, string_or_array};

type JsonMap = Map<String, Value>;

/// Knip issue types that fallow reports per project, so a file glob in
/// `overrides` cannot scope them.
const PROJECT_LEVEL_ISSUE_TYPES: &[&str] = &["dependencies", "devDependencies", "unlisted"];

/// The suffix that marks a knip entry pattern as a production entry.
const PRODUCTION_MARKER: char = '!';

/// One knip config section: the root, or one entry of `workspaces`.
pub(super) struct KnipScope {
    /// Warning field prefix: empty for the root, `workspaces.<key>.` otherwise.
    field_prefix: String,
    /// Project-root-relative path prefix with a trailing `/`, empty for the root.
    path_prefix: String,
}

/// Whether any `entry` pattern of the knip config, at the root or in a
/// workspace, carries the production marker. Only then does the migration
/// split the entries into `entry` and `supportEntry`.
pub(super) fn config_marks_production_entry(obj: &JsonMap) -> bool {
    let marks = |section: &JsonMap| {
        section.get("entry").is_some_and(|value| {
            string_or_array(value)
                .iter()
                .any(|pattern| !pattern.starts_with('!') && pattern.ends_with(PRODUCTION_MARKER))
        })
    };
    marks(obj)
        || obj
            .get("workspaces")
            .and_then(Value::as_object)
            .is_some_and(|workspaces| workspaces.values().filter_map(Value::as_object).any(marks))
}

impl KnipScope {
    pub(super) fn root() -> Self {
        Self {
            field_prefix: String::new(),
            path_prefix: String::new(),
        }
    }

    /// The scope of one workspace key, or the reason why the key has no
    /// project-root-relative path.
    fn workspace(key: &str) -> Result<Self, &'static str> {
        let normalized = normalize_path(key.trim());
        let path = normalized.trim_end_matches('/');
        if path.starts_with('!') {
            return Err("negated workspace key; workspace config not migrated");
        }
        if has_parent_segment(path) {
            return Err("workspace path leaves the project root; workspace config not migrated");
        }
        let path_prefix = if path.is_empty() || path == "." {
            String::new()
        } else {
            format!("{path}/")
        };
        let field_prefix = if key == "." {
            r#"workspaces["."]."#.to_string()
        } else {
            format!("workspaces.{key}.")
        };
        Ok(Self {
            field_prefix,
            path_prefix,
        })
    }

    /// The warning field name of `name` in this scope.
    pub(super) fn field(&self, name: &str) -> String {
        format!("{}{name}", self.field_prefix)
    }

    /// The `workspaces.settings` key of this scope, or `None` for the root
    /// workspace, which uses the global settings.
    fn settings_key(&self) -> Option<&str> {
        let key = self.path_prefix.trim_end_matches('/');
        (!key.is_empty()).then_some(key)
    }

    /// Rewrite a scope-relative glob as a project-root-relative glob, or
    /// return `None` when the glob leaves the scope with a `..` segment.
    pub(super) fn rebase(&self, pattern: &str) -> Option<String> {
        let pattern = normalize_path(pattern);
        if has_parent_segment(&pattern) {
            return None;
        }
        // `packages/**/` already matches any depth, so a leading `**/` in the
        // pattern adds nothing.
        if self.path_prefix.ends_with("**/") {
            if pattern == "**" {
                return Some(self.path_prefix.trim_end_matches('/').to_string());
            }
            if let Some(rest) = pattern.strip_prefix("**/") {
                return Some(format!("{}{rest}", self.path_prefix));
            }
        }
        Some(format!("{}{pattern}", self.path_prefix))
    }
}

/// Use `/` as the separator and drop a leading `./` or `/`, because fallow
/// config paths are project-root-relative.
fn normalize_path(path: &str) -> String {
    let path = path.replace('\\', "/");
    path.trim_start_matches("./")
        .trim_start_matches('/')
        .to_string()
}

fn has_parent_segment(path: &str) -> bool {
    path.split(['/', '\\']).any(|segment| segment == "..")
}

fn traversal_warning(field: String, pattern: &str) -> MigrationWarning {
    MigrationWarning {
        source: "knip",
        field,
        message: format!("pattern `{pattern}` leaves the workspace; not migrated"),
        suggestion: Some("add a project-root-relative pattern by hand".to_string()),
    }
}

/// Append each value to the string array at `key`, and skip values that the
/// array already holds.
pub(super) fn append_unique(config: &mut JsonMap, key: &str, values: Vec<String>) {
    if values.is_empty() {
        return;
    }
    let slot = config
        .entry(key.to_string())
        .or_insert_with(|| Value::Array(Vec::new()));
    let Some(array) = slot.as_array_mut() else {
        return;
    };
    for value in values {
        if !array
            .iter()
            .any(|existing| existing.as_str() == Some(&value))
        {
            array.push(Value::String(value));
        }
    }
}

fn append_value(config: &mut JsonMap, key: &str, value: Value) {
    if let Some(array) = config
        .entry(key.to_string())
        .or_insert_with(|| Value::Array(Vec::new()))
        .as_array_mut()
    {
        array.push(value);
    }
}

/// Migrate knip `entry` patterns to fallow `entry` and `supportEntry`.
///
/// When `production_split` is set (the knip config marks at least one entry
/// with a trailing `!`), a marked pattern becomes an `entry` (a runtime root
/// in every mode) and an unmarked pattern becomes a `supportEntry` (left out
/// in production mode). Otherwise every pattern becomes an `entry`, so a
/// project that never used the marker keeps its entries in production mode.
/// Fallow `entry` has no exclusion form, so a negated pattern is reported and
/// not written: as a literal glob it would match no file.
pub(super) fn migrate_entry(
    value: &Value,
    scope: &KnipScope,
    production_split: bool,
    config: &mut JsonMap,
    warnings: &mut Vec<MigrationWarning>,
) {
    let mut entries = Vec::new();
    let mut support_entries = Vec::new();
    for pattern in string_or_array(value) {
        if pattern.starts_with('!') {
            warnings.push(MigrationWarning {
                source: "knip",
                field: scope.field("entry"),
                message: format!(
                    "negated entry pattern `{pattern}` not migrated; fallow `entry` cannot exclude files"
                ),
                suggestion: Some(
                    "narrow the positive entry patterns so that they do not match these files"
                        .to_string(),
                ),
            });
            continue;
        }
        let (body, marked) = match pattern.strip_suffix(PRODUCTION_MARKER) {
            Some(body) => (body, true),
            None => (pattern.as_str(), false),
        };
        let Some(rebased) = scope.rebase(body) else {
            warnings.push(traversal_warning(scope.field("entry"), &pattern));
            continue;
        };
        if production_split && !marked {
            support_entries.push(rebased);
        } else {
            entries.push(rebased);
        }
    }
    append_unique(config, "entry", entries);
    append_unique(config, "supportEntry", support_entries);
}

/// Migrate knip `ignoreIssues` (file glob to issue types) to fallow.
///
/// File-level issue types become an `overrides` entry that turns the rules
/// off for the glob. `duplicates` turns `duplicate-export-aliases` off for the
/// glob. It becomes an `ignoreExports` entry with `exports: ["*"]` instead
/// when the same glob also ignores `exports` and unused types are ignored
/// there too, because that rule also exempts the exports from
/// `unused-exports`, `unused-types` and `duplicate-export-aliases`.
pub(super) fn migrate_ignore_issues(
    value: &Value,
    scope: &KnipScope,
    config: &mut JsonMap,
    warnings: &mut Vec<MigrationWarning>,
) {
    let Some(issues) = value.as_object() else {
        warnings.push(MigrationWarning {
            source: "knip",
            field: scope.field("ignoreIssues"),
            message: "expected an object of file globs to issue types; not migrated".to_string(),
            suggestion: None,
        });
        return;
    };
    let types_off = config
        .get("rules")
        .and_then(|rules| rules.get("unused-types"))
        .and_then(Value::as_str)
        == Some("off");

    // A base issue type that the global rules turn off already hides its
    // namespace findings everywhere.
    let globally_off: Vec<&str> = KNIP_RULE_MAP
        .iter()
        .filter(|(_, rule)| {
            config
                .get("rules")
                .and_then(|rules| rules.get(*rule))
                .and_then(Value::as_str)
                == Some("off")
        })
        .map(|(name, _)| *name)
        .collect();
    for (glob, kinds) in issues {
        let field = scope.field(&format!("ignoreIssues.{glob}"));
        let Some(file) = scope.rebase(glob).filter(|_| !glob.starts_with('!')) else {
            warnings.push(MigrationWarning {
                source: "knip",
                field,
                message: format!("file glob `{glob}` not migrated"),
                suggestion: Some(
                    "use a positive glob inside the workspace for overrides[].files".to_string(),
                ),
            });
            continue;
        };
        let kinds = string_or_array(kinds);
        let has = |kind: &str| kinds.iter().any(|k| k == kind);
        let exempt_all_exports = has("duplicates") && has("exports") && (has("types") || types_off);

        let mut rules = Map::new();
        for kind in &kinds {
            match kind.as_str() {
                "exports" | "types" | "duplicates" if exempt_all_exports => {}
                "duplicates" => {
                    rules.insert(
                        "duplicate-export-aliases".to_string(),
                        Value::String("off".to_string()),
                    );
                }
                kind if PROJECT_LEVEL_ISSUE_TYPES.contains(&kind) => {
                    warnings.push(MigrationWarning {
                        source: "knip",
                        field: format!("{field}.{kind}"),
                        message: format!(
                            "issue type `{kind}` is reported per project in fallow; a file glob cannot scope it"
                        ),
                        suggestion: Some(
                            "add the package names to ignoreDependencies".to_string(),
                        ),
                    });
                }
                kind => match KNIP_RULE_MAP.iter().find(|(name, _)| *name == kind) {
                    Some((_, rule)) => {
                        rules.insert((*rule).to_string(), Value::String("off".to_string()));
                    }
                    None => {
                        if !migrate_namespace_issue_type(
                            &field,
                            kind,
                            |base| has(base) || globally_off.contains(&base),
                            warnings,
                        ) {
                            warn_unmapped_rule_key(&field, kind, warnings);
                        }
                    }
                },
            }
        }

        if exempt_all_exports {
            append_value(
                config,
                "ignoreExports",
                serde_json::json!({"file": file, "exports": ["*"]}),
            );
        }
        if !rules.is_empty() {
            append_value(
                config,
                "overrides",
                serde_json::json!({"files": [file], "rules": rules}),
            );
        }
    }
}

/// The `workspaces.settings` values of one knip workspace section.
#[derive(Default)]
struct SectionSettings {
    /// The project-root-relative key, as fallow writes it.
    key: String,
    /// The knip key, for warning field names.
    scope_field_prefix: String,
    ignore_dependencies: Vec<String>,
    /// The `includeEntryExports` value that the section sets, if any.
    include_entry_exports: Option<bool>,
}

/// The settings that the root and the workspace sections share.
pub(super) struct WorkspaceMigration {
    /// The effective root value of `includeEntryExports`.
    pub(super) root_include_entry_exports: bool,
    /// Whether unmarked entry patterns become `supportEntry` patterns.
    pub(super) production_split: bool,
}

/// Migrate the knip `workspaces` map.
///
/// File patterns become project-root-relative patterns. `ignoreDependencies`
/// and `includeEntryExports` become `workspaces.settings` keys, except for the
/// root workspace `.`, which uses the global settings.
pub(super) fn migrate_workspaces(
    workspaces: &Value,
    migration: &WorkspaceMigration,
    config: &mut JsonMap,
    warnings: &mut Vec<MigrationWarning>,
) {
    let Some(workspaces) = workspaces.as_object() else {
        warnings.push(MigrationWarning {
            source: "knip",
            field: "workspaces".to_string(),
            message: "expected an object; workspace config not migrated".to_string(),
            suggestion: None,
        });
        return;
    };

    let mut sections = Vec::new();
    for (key, section) in workspaces {
        let Some(section) = section.as_object() else {
            warnings.push(MigrationWarning {
                source: "knip",
                field: format!("workspaces.{key}"),
                message: "expected an object; workspace config not migrated".to_string(),
                suggestion: None,
            });
            continue;
        };
        let scope = match KnipScope::workspace(key) {
            Ok(scope) => scope,
            Err(reason) => {
                warnings.push(MigrationWarning {
                    source: "knip",
                    field: format!("workspaces.{key}"),
                    message: reason.to_string(),
                    suggestion: None,
                });
                continue;
            }
        };
        let mut settings = SectionSettings {
            key: scope.settings_key().unwrap_or_default().to_string(),
            scope_field_prefix: scope.field(""),
            ..SectionSettings::default()
        };
        for (name, value) in section {
            migrate_workspace_field(
                (name, value),
                &scope,
                migration,
                &mut settings,
                config,
                warnings,
            );
        }
        if scope.settings_key().is_some() {
            sections.push(settings);
        }
    }
    write_workspace_settings(
        &sections,
        migration.root_include_entry_exports,
        config,
        warnings,
    );
}

fn migrate_workspace_field(
    (name, value): (&str, &Value),
    scope: &KnipScope,
    migration: &WorkspaceMigration,
    settings: &mut SectionSettings,
    config: &mut JsonMap,
    warnings: &mut Vec<MigrationWarning>,
) {
    match name {
        "entry" => migrate_entry(value, scope, migration.production_split, config, warnings),
        "ignore" => migrate_ignore(value, scope, config, warnings),
        "ignoreIssues" => migrate_ignore_issues(value, scope, config, warnings),
        "ignoreDependencies" => {
            if scope.settings_key().is_none() {
                migrate_ignore_deps(value, &scope.field(name), config, warnings);
                return;
            }
            let mut scratch = JsonMap::new();
            migrate_ignore_deps(value, &scope.field(name), &mut scratch, warnings);
            settings.ignore_dependencies = scratch
                .get("ignoreDependencies")
                .map(string_or_array)
                .unwrap_or_default();
        }
        "ignoreBinaries" => {
            if migrate_ignore_binaries(value, &scope.field(name), config, warnings) {
                warnings.push(MigrationWarning {
                    source: "knip",
                    field: scope.field(name),
                    message: "merged into the global ignoreBinaries list; fallow has no per-workspace list, so the entries apply to every workspace".to_string(),
                    suggestion: None,
                });
            }
        }
        "includeEntryExports" => {
            let Some(include) = value.as_bool() else {
                warnings.push(MigrationWarning {
                    source: "knip",
                    field: scope.field(name),
                    message: "expected a boolean; not migrated".to_string(),
                    suggestion: None,
                });
                return;
            };
            if scope.settings_key().is_some() {
                settings.include_entry_exports = Some(include);
            } else if include != migration.root_include_entry_exports {
                warnings.push(MigrationWarning {
                    source: "knip",
                    field: scope.field(name),
                    message: "includeEntryExports of the root workspace not migrated; in fallow the root package uses the top-level value, which also applies to each workspace without its own value".to_string(),
                    suggestion: Some(
                        "set the top-level includeEntryExports, and set workspaces.settings values for the workspaces that need another value"
                            .to_string(),
                    ),
                });
            }
        }
        _ => warn_unmapped_workspace_field(name, scope, warnings),
    }
}

/// Write the `workspaces.settings` keys.
///
/// Knip applies only the most specific workspace section. Fallow merges the
/// `ignoreDependencies` lists of every matching key, and takes
/// `includeEntryExports` from the most specific key that sets it. So a more
/// specific section without `includeEntryExports` gets the root value
/// explicitly, and a glob list that also reaches a more specific section gets
/// a warning.
fn write_workspace_settings(
    sections: &[SectionSettings],
    root_include_entry_exports: bool,
    config: &mut JsonMap,
    warnings: &mut Vec<MigrationWarning>,
) {
    let mut settings = JsonMap::new();
    for section in sections {
        let overlapping: Vec<&SectionSettings> = sections
            .iter()
            .filter(|other| other.key != section.key && glob_key_selects(&other.key, &section.key))
            .collect();
        for other in &overlapping {
            if !other.ignore_dependencies.is_empty() {
                warnings.push(MigrationWarning {
                    source: "knip",
                    field: format!("{}ignoreDependencies", other.scope_field_prefix),
                    message: format!(
                        "fallow merges the ignoreDependencies of every matching workspaces.settings key, so `{}` now also ignores the entries of `{}`",
                        section.key, other.key
                    ),
                    suggestion: Some(format!(
                        "move the entries to the workspaces that need them if `{}` must report them",
                        section.key
                    )),
                });
            }
        }
        let inherits_other_value = overlapping.iter().any(|other| {
            other
                .include_entry_exports
                .is_some_and(|value| value != root_include_entry_exports)
        });
        let include = section
            .include_entry_exports
            .or_else(|| inherits_other_value.then_some(root_include_entry_exports))
            .filter(|value| *value != root_include_entry_exports || inherits_other_value);

        let mut value = JsonMap::new();
        if !section.ignore_dependencies.is_empty() {
            value.insert(
                "ignoreDependencies".to_string(),
                Value::from(section.ignore_dependencies.clone()),
            );
        }
        if let Some(include) = include {
            value.insert("includeEntryExports".to_string(), Value::Bool(include));
        }
        if !value.is_empty() {
            settings.insert(section.key.clone(), Value::Object(value));
        }
    }
    if settings.is_empty() {
        return;
    }
    let workspaces = config
        .entry("workspaces".to_string())
        .or_insert_with(|| Value::Object(JsonMap::new()));
    if let Some(workspaces) = workspaces.as_object_mut() {
        workspaces.insert("settings".to_string(), Value::Object(settings));
    }
}

/// Whether the glob key `glob` selects the literal key `key`.
fn glob_key_selects(glob: &str, key: &str) -> bool {
    let is_glob = |text: &str| text.contains(['*', '?', '[', '{']);
    if !is_glob(glob) || is_glob(key) {
        return false;
    }
    fallow_config::compile_path_glob(glob).is_ok_and(|glob| glob.compile_matcher().is_match(key))
}

fn warn_unmapped_workspace_field(
    name: &str,
    scope: &KnipScope,
    warnings: &mut Vec<MigrationWarning>,
) {
    let field = scope.field(name);
    let (message, suggestion) = if let Some((_, message, suggestion)) = KNIP_UNMAPPABLE_FIELDS
        .iter()
        .find(|(unmappable, _, _)| *unmappable == name)
    {
        ((*message).to_string(), suggestion.map(str::to_string))
    } else if KNIP_PLUGIN_KEYS.contains(&name) {
        (
            format!("plugin config `{name}` is auto-detected by fallow's built-in plugins"),
            Some(
                "remove this section, or add an entry pattern if a plugin misses a file"
                    .to_string(),
            ),
        )
    } else if KNIP_UNSUPPORTED_PLUGIN_KEYS.contains(&name) {
        (
            format!("plugin config `{name}` has no fallow plugin equivalent"),
            None,
        )
    } else {
        (
            format!("unknown knip workspace key `{name}`; not migrated"),
            None,
        )
    };
    warnings.push(MigrationWarning {
        source: "knip",
        field,
        message,
        suggestion,
    });
}
