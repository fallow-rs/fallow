//! Knip settings that name file paths, for the root and for each workspace.
//!
//! Fallow config paths are project-root-relative. A knip workspace section is
//! relative to its workspace key, so the migration puts the key in front of
//! each pattern. A glob key such as `packages/*` becomes the `packages/*/`
//! prefix, which matches the same package roots.

use serde_json::{Map, Value};

use super::knip_fields::{
    IssueTypeEffect, migrate_ignore, migrate_ignore_deps, migrate_namespace_issue_type,
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

/// Migrate knip `entry` patterns to fallow `entry`.
///
/// Fallow `entry` files are runtime roots in every mode, so the knip
/// production marker (a trailing `!`) is dropped. A section that mixes marked
/// and unmarked patterns gets one warning, because fallow has no
/// production-only entry: `--production` only removes files that match the
/// production exclusion patterns. Fallow `entry` has no exclusion form, so a
/// negated pattern is reported and not written: as a literal glob it would
/// match no file.
pub(super) fn migrate_entry(
    value: &Value,
    scope: &KnipScope,
    config: &mut JsonMap,
    warnings: &mut Vec<MigrationWarning>,
) {
    let mut migrated = Vec::new();
    let mut marked = false;
    let mut unmarked = false;
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
        let body = match pattern.strip_suffix(PRODUCTION_MARKER) {
            Some(body) => {
                marked = true;
                body
            }
            None => {
                unmarked = true;
                pattern.as_str()
            }
        };
        match scope.rebase(body) {
            Some(rebased) => migrated.push(rebased),
            None => warnings.push(traversal_warning(scope.field("entry"), &pattern)),
        }
    }
    if marked && unmarked {
        warnings.push(MigrationWarning {
            source: "knip",
            field: scope.field("entry"),
            message: "production markers (a trailing `!`) removed; fallow has no production-only entry, so the unmarked entries now also count in production mode".to_string(),
            suggestion: Some(
                "check the `--production` findings; remove an unmarked entry if it must not keep production code reachable"
                    .to_string(),
            ),
        });
    }
    append_unique(config, "entry", migrated);
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
                            IssueTypeEffect::Off,
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

/// Migrate the knip `workspaces` map.
///
/// `root_include_entry_exports` is the effective root value of
/// `includeEntryExports`. A workspace that sets another value gets a warning,
/// because fallow has only the global setting.
pub(super) fn migrate_workspaces(
    workspaces: &Value,
    root_include_entry_exports: bool,
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
        for (name, value) in section {
            migrate_workspace_field(
                name,
                value,
                &scope,
                root_include_entry_exports,
                config,
                warnings,
            );
        }
    }
}

fn migrate_workspace_field(
    name: &str,
    value: &Value,
    scope: &KnipScope,
    root_include_entry_exports: bool,
    config: &mut JsonMap,
    warnings: &mut Vec<MigrationWarning>,
) {
    match name {
        "entry" => migrate_entry(value, scope, config, warnings),
        "ignore" => migrate_ignore(value, scope, config, warnings),
        "ignoreIssues" => migrate_ignore_issues(value, scope, config, warnings),
        "ignoreDependencies" => {
            if migrate_ignore_deps(value, &scope.field(name), config, warnings) {
                warnings.push(MigrationWarning {
                    source: "knip",
                    field: scope.field(name),
                    message: "merged into the global ignoreDependencies list; fallow has no per-workspace list, so the entries apply to every workspace".to_string(),
                    suggestion: None,
                });
            }
        }
        "includeEntryExports" => {
            if value.as_bool() != Some(root_include_entry_exports) {
                warnings.push(MigrationWarning {
                    source: "knip",
                    field: scope.field(name),
                    message: "per-workspace includeEntryExports not migrated; fallow has only the global includeEntryExports setting".to_string(),
                    suggestion: Some(
                        "set includeEntryExports at the top level to apply it to every workspace"
                            .to_string(),
                    ),
                });
            }
        }
        _ => warn_unmapped_workspace_field(name, scope, warnings),
    }
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
