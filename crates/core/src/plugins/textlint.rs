//! textlint plugin.
//!
//! textlint loads each rule, filter rule and plugin of its config at runtime.
//! The config names a package by a short name, such as `no-todo` for
//! `textlint-rule-no-todo`, or names a local module by path. These references
//! have no import in the project, so the plugin credits the packages and keeps
//! the local modules reachable.
//!
//! The plugin reads `.textlintrc` (JSON or YAML form), `.textlintrc.json`,
//! `.textlintrc.{yaml,yml}`, `.textlintrc.js`, `.textlintrc.cjs`, the
//! `textlint` key of `package.json`, and a config that a
//! `textlint --config <file>` script names.

use std::path::{Path, PathBuf};

use oxc_ast::ast::{ArrayExpressionElement, Expression, ObjectExpression, ObjectPropertyKind};
use rustc_hash::FxHashSet;

use super::{Plugin, PluginResult, config_parser};

const ENABLERS: &[&str] = &["textlint"];

// The YAML forms stay activation-only: the extractor is a JS/JSON parser.
const CONFIG_PATTERNS: &[&str] = &[".textlintrc", ".textlintrc.{json,yaml,yml,js,cjs}"];

const ALWAYS_USED: &[&str] = &[".textlintrc", ".textlintrc.{json,yaml,yml,js,cjs}"];

const TOOLING_DEPENDENCIES: &[&str] = &["textlint"];

const RULE_PREFIX: &str = "textlint-rule-";
const PRESET_PREFIX: &str = "textlint-rule-preset-";
const FILTER_RULE_PREFIX: &str = "textlint-filter-rule-";
const PLUGIN_PREFIX: &str = "textlint-plugin-";

/// The manifest sections that declare a package.
const DEPENDENCY_SECTIONS: &[&str] = &[
    "dependencies",
    "devDependencies",
    "optionalDependencies",
    "peerDependencies",
];

pub struct TextlintPlugin;

impl Plugin for TextlintPlugin {
    fn name(&self) -> &'static str {
        "textlint"
    }

    fn enablers(&self) -> &'static [&'static str] {
        ENABLERS
    }

    fn config_patterns(&self) -> &'static [&'static str] {
        CONFIG_PATTERNS
    }

    fn always_used(&self) -> &'static [&'static str] {
        ALWAYS_USED
    }

    fn tooling_dependencies(&self) -> &'static [&'static str] {
        TOOLING_DEPENDENCIES
    }

    fn script_config_binaries(&self) -> &'static [&'static str] {
        ENABLERS
    }

    fn package_json_config_key(&self) -> Option<&'static str> {
        Some("textlint")
    }

    fn resolve_config(&self, config_path: &Path, source: &str, root: &Path) -> PluginResult {
        let mut result = PluginResult::default();
        let config = match config_format(config_path, source) {
            ConfigFormat::Script(parse_path) => {
                super::add_import_referenced_dependencies(&mut result, source, config_path);
                extract_modules(source, &parse_path)
            }
            ConfigFormat::Yaml => extract_yaml_modules(source),
        };
        let config_dir = config_path.parent().unwrap_or(root);
        let declared = declared_packages(config_dir, root);

        let modules = config
            .rules
            .iter()
            .map(|key| (ModuleKind::Rule, key))
            .chain(config.filters.iter().map(|key| (ModuleKind::Filter, key)))
            .chain(config.plugins.iter().map(|key| (ModuleKind::Plugin, key)));
        for (kind, key) in modules {
            match key {
                ModuleKey::Name(name) if is_local(name) => {
                    result.setup_files.push(resolve_local(config_dir, name));
                }
                ModuleKey::Name(name) => {
                    result
                        .referenced_dependencies
                        .push(package_name(kind, name, &declared));
                }
                ModuleKey::Path(path) => {
                    result.setup_files.push(resolve_local(config_dir, path));
                }
            }
        }

        result
    }
}

/// The config format. An extensionless `.textlintrc` holds JSON or YAML.
enum ConfigFormat {
    /// Parsed with the JS parser, which takes the file extension of this path.
    Script(PathBuf),
    Yaml,
}

fn config_format(config_path: &Path, source: &str) -> ConfigFormat {
    match config_path.extension().and_then(|ext| ext.to_str()) {
        Some("yml" | "yaml") => ConfigFormat::Yaml,
        Some(_) => ConfigFormat::Script(config_path.to_path_buf()),
        None if source.trim_start().starts_with('{') => {
            ConfigFormat::Script(config_path.with_file_name(".textlintrc.json"))
        }
        None => ConfigFormat::Yaml,
    }
}

#[derive(Clone, Copy)]
enum ModuleKind {
    Rule,
    Filter,
    Plugin,
}

/// One module that the config names.
#[derive(Debug, PartialEq, Eq)]
enum ModuleKey {
    /// A literal key: a package short name, a package name, or a path.
    Name(String),
    /// A computed key, such as `path.join(__dirname, "rules/x")`, as a path
    /// relative to the config directory.
    Path(String),
}

#[derive(Debug, Default)]
struct TextlintModules {
    rules: Vec<ModuleKey>,
    filters: Vec<ModuleKey>,
    plugins: Vec<ModuleKey>,
}

fn extract_modules(source: &str, path: &Path) -> TextlintModules {
    config_parser::extract_from_source(source, path, |program| {
        let obj = config_parser::find_config_object(program)?;
        Some(TextlintModules {
            rules: config_parser::property_object(obj, "rules")
                .map(|rules| object_keys(rules, true))
                .unwrap_or_default(),
            filters: config_parser::property_object(obj, "filters")
                .map(|filters| object_keys(filters, false))
                .unwrap_or_default(),
            plugins: config_parser::property_expr(obj, "plugins")
                .map(plugin_keys)
                .unwrap_or_default(),
        })
    })
    .unwrap_or_default()
}

fn extract_yaml_modules(source: &str) -> TextlintModules {
    let Ok(document) = fallow_config::yaml::parse(source) else {
        return TextlintModules::default();
    };
    let root = document.root();
    let keys = |field: &str, skip_disabled: bool| -> Vec<ModuleKey> {
        let Some(node) = root.get(field) else {
            return Vec::new();
        };
        if let Some(items) = node.as_sequence() {
            return items
                .filter_map(|item| item.as_str())
                .map(|name| ModuleKey::Name(name.to_string()))
                .collect();
        }
        node.as_mapping()
            .map(|mapping| {
                mapping
                    .iter()
                    .filter(|(_, value)| !(skip_disabled && is_yaml_falsy(*value)))
                    .filter_map(|(key, _)| key.as_str())
                    .map(|name| ModuleKey::Name(name.to_string()))
                    .collect()
            })
            .unwrap_or_default()
    };
    TextlintModules {
        rules: keys("rules", true),
        filters: keys("filters", false),
        plugins: keys("plugins", false),
    }
}

fn is_yaml_falsy(node: fallow_config::yaml::YamlNode<'_>) -> bool {
    node.is_null() || node.as_bool() == Some(false)
}

/// The keys of an object. textlint skips a rule whose options value is falsy,
/// so `skip_disabled` drops those keys.
fn object_keys(obj: &ObjectExpression<'_>, skip_disabled: bool) -> Vec<ModuleKey> {
    obj.properties
        .iter()
        .filter_map(|prop| {
            let ObjectPropertyKind::ObjectProperty(prop) = prop else {
                return None;
            };
            if skip_disabled && is_falsy(&prop.value) {
                return None;
            }
            if let Some(name) = prop.key.static_name() {
                return Some(ModuleKey::Name(name.to_string()));
            }
            let expr = prop.key.as_expression()?;
            config_parser::expression_to_path_string(expr).map(ModuleKey::Path)
        })
        .collect()
}

/// `plugins` is an array of names or an object keyed by name.
fn plugin_keys(expr: &Expression<'_>) -> Vec<ModuleKey> {
    match expr {
        Expression::ArrayExpression(array) => array
            .elements
            .iter()
            .filter_map(ArrayExpressionElement::as_expression)
            .filter_map(config_parser::expression_to_string)
            .map(ModuleKey::Name)
            .collect(),
        Expression::ObjectExpression(obj) => object_keys(obj, false),
        _ => Vec::new(),
    }
}

fn is_falsy(expr: &Expression<'_>) -> bool {
    match expr {
        Expression::BooleanLiteral(boolean) => !boolean.value,
        Expression::NullLiteral(_) => true,
        Expression::NumericLiteral(number) => number.value == 0.0,
        Expression::StringLiteral(string) => string.value.is_empty(),
        Expression::Identifier(id) => id.name == "undefined",
        _ => false,
    }
}

fn is_local(name: &str) -> bool {
    name.starts_with('.') || Path::new(name).is_absolute()
}

/// Resolve a local module the way Node resolves a request: the file itself,
/// the file with a source extension, then the index file of a directory.
fn resolve_local(config_dir: &Path, value: &str) -> PathBuf {
    let base = config_parser::lexical_normalize(&config_dir.join(value));
    if base.is_file() {
        return base;
    }
    let mut candidates = crate::discover::SOURCE_EXTENSIONS
        .iter()
        .map(|ext| {
            let mut with_ext = base.clone().into_os_string();
            with_ext.push(".");
            with_ext.push(ext);
            PathBuf::from(with_ext)
        })
        .chain(
            crate::discover::SOURCE_EXTENSIONS
                .iter()
                .map(|ext| base.join(format!("index.{ext}"))),
        );
    candidates.find(|path| path.is_file()).unwrap_or(base)
}

/// The packages that the nearest `package.json` at or above the config
/// directory declares. The search stops at the project root.
fn declared_packages(config_dir: &Path, root: &Path) -> FxHashSet<String> {
    let mut declared = FxHashSet::default();
    let manifest = config_dir
        .ancestors()
        .take_while(|dir| dir.starts_with(root))
        .map(|dir| dir.join("package.json"))
        .find(|path| path.is_file());
    let Some(manifest) = manifest else {
        return declared;
    };
    let Ok(content) = std::fs::read_to_string(&manifest) else {
        return declared;
    };
    let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) else {
        return declared;
    };
    for section in DEPENDENCY_SECTIONS {
        if let Some(deps) = json.get(section).and_then(serde_json::Value::as_object) {
            declared.extend(deps.keys().cloned());
        }
    }
    declared
}

/// The package that textlint loads for one config key.
///
/// textlint tries several package names for a key. The first candidate that
/// the manifest declares wins. Without a declared candidate, the plugin
/// credits the name that textlint tries first.
fn package_name(kind: ModuleKind, key: &str, declared: &FxHashSet<String>) -> String {
    let (prefix, name, candidates) = match kind {
        ModuleKind::Rule if is_preset_rule_key(key) => {
            let name = preset_short_name(key);
            let candidates = vec![
                format!("{PRESET_PREFIX}{name}"),
                name.clone(),
                full_package_name(PRESET_PREFIX, &name),
                key.to_string(),
            ];
            (PRESET_PREFIX, name, candidates)
        }
        ModuleKind::Rule => module_candidates(RULE_PREFIX, key),
        ModuleKind::Filter => module_candidates(FILTER_RULE_PREFIX, key),
        ModuleKind::Plugin => module_candidates(PLUGIN_PREFIX, key),
    };
    let package = candidates
        .into_iter()
        .find(|candidate| declared.contains(candidate))
        .unwrap_or_else(|| {
            if has_prefix(prefix, &name) {
                name
            } else {
                full_package_name(prefix, &name)
            }
        });
    crate::resolve::extract_package_name(&package)
}

fn module_candidates(prefix: &'static str, key: &str) -> (&'static str, String, Vec<String>) {
    let candidates = vec![full_package_name(prefix, key), key.to_string()];
    (prefix, key.to_string(), candidates)
}

/// Add the textlint prefix to a package name. A scoped name gets the prefix
/// after the scope: `@scope/x` becomes `@scope/<prefix>x`.
fn full_package_name(prefix: &str, name: &str) -> String {
    let Some((scope, rest)) = name
        .strip_prefix('@')
        .and_then(|scoped| scoped.split_once('/'))
    else {
        return format!("{prefix}{name}");
    };
    if rest == prefix.trim_end_matches('-') || rest.starts_with(prefix) {
        return name.to_string();
    }
    format!("@{scope}/{prefix}{rest}")
}

fn has_prefix(prefix: &str, name: &str) -> bool {
    let unscoped = if name.starts_with('@') {
        name.split_once('/').map_or(name, |(_, rest)| rest)
    } else {
        name
    };
    unscoped.starts_with(prefix)
}

fn is_preset_rule_key(key: &str) -> bool {
    key.starts_with("preset-")
        || key.starts_with(PRESET_PREFIX)
        || (key.starts_with('@')
            && (key.contains("/preset-") || key.contains(&format!("/{PRESET_PREFIX}"))))
}

/// Strip the `preset-` short-name marker: `preset-x` becomes `x` and
/// `@scope/preset-x` becomes `@scope/x`.
fn preset_short_name(key: &str) -> String {
    let name = key.strip_prefix("preset-").unwrap_or(key);
    if let Some(scoped) = name.strip_prefix('@')
        && let Some((scope, rest)) = scoped.split_once('/')
        && let Some(short) = rest.strip_prefix("preset-")
    {
        return format!("@{scope}/{short}");
    }
    name.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn declared(names: &[&str]) -> FxHashSet<String> {
        names.iter().map(ToString::to_string).collect()
    }

    fn rule(key: &str, names: &[&str]) -> String {
        package_name(ModuleKind::Rule, key, &declared(names))
    }

    #[test]
    fn expands_rule_short_names() {
        assert_eq!(rule("no-todo", &[]), "textlint-rule-no-todo");
        assert_eq!(
            rule("textlint-rule-max-comma", &[]),
            "textlint-rule-max-comma"
        );
        assert_eq!(
            rule("@textlint-rule/no-unmatched-pair", &[]),
            "@textlint-rule/textlint-rule-no-unmatched-pair"
        );
        assert_eq!(
            rule("@textlint-ja/no-synonyms", &[]),
            "@textlint-ja/textlint-rule-no-synonyms"
        );
    }

    #[test]
    fn expands_preset_names() {
        assert_eq!(
            rule("preset-ja-technical-writing", &[]),
            "textlint-rule-preset-ja-technical-writing"
        );
        assert_eq!(
            rule("textlint-rule-preset-smarthr", &[]),
            "textlint-rule-preset-smarthr"
        );
        assert_eq!(
            rule(
                "@textlint-ja/preset-ai-writing",
                &["@textlint-ja/textlint-rule-preset-ai-writing"]
            ),
            "@textlint-ja/textlint-rule-preset-ai-writing"
        );
    }

    #[test]
    fn declared_candidate_wins() {
        assert_eq!(rule("custom-rule", &["custom-rule"]), "custom-rule");
    }

    #[test]
    fn expands_filter_and_plugin_names() {
        let none = declared(&[]);
        assert_eq!(
            package_name(ModuleKind::Filter, "comments", &none),
            "textlint-filter-rule-comments"
        );
        assert_eq!(
            package_name(ModuleKind::Plugin, "@textlint/markdown", &none),
            "@textlint/textlint-plugin-markdown"
        );
        assert_eq!(
            package_name(ModuleKind::Plugin, "html", &none),
            "textlint-plugin-html"
        );
    }

    #[test]
    fn extracts_json_modules_and_skips_disabled_rules() {
        let source = r#"{
            "rules": { "no-todo": true, "write-good": false, "max-comma": { "max": 3 } },
            "filters": { "comments": true },
            "plugins": ["latex2e"]
        }"#;
        let modules = extract_modules(source, Path::new("/p/.textlintrc.json"));
        assert_eq!(
            modules.rules,
            vec![
                ModuleKey::Name("no-todo".to_string()),
                ModuleKey::Name("max-comma".to_string())
            ]
        );
        assert_eq!(
            modules.filters,
            vec![ModuleKey::Name("comments".to_string())]
        );
        assert_eq!(
            modules.plugins,
            vec![ModuleKey::Name("latex2e".to_string())]
        );
    }

    #[test]
    fn extracts_computed_local_rule_paths() {
        let source = r"
            const path = require('node:path');
            module.exports = {
                rules: {
                    [require.resolve('./rules/check-document.cjs')]: true,
                    [path.join(__dirname, 'rules/check-heading')]: true,
                },
            };
        ";
        let modules = extract_modules(source, Path::new("/p/.textlintrc.cjs"));
        assert_eq!(modules.rules.len(), 2);
        assert!(
            modules
                .rules
                .iter()
                .all(|key| matches!(key, ModuleKey::Path(path) if path.contains("rules/check-")))
        );
    }

    #[test]
    fn extensionless_rc_reads_json_and_yaml() {
        let rc = Path::new("/p/.textlintrc");
        assert!(matches!(
            config_format(rc, "{ \"rules\": {} }"),
            ConfigFormat::Script(_)
        ));
        assert!(matches!(
            config_format(rc, "rules:\n  no-todo: true\n"),
            ConfigFormat::Yaml
        ));
    }

    #[test]
    fn yaml_config_credits_enabled_modules() {
        let result = TextlintPlugin.resolve_config(
            Path::new("/project/.textlintrc.yml"),
            "rules:\n  no-todo: true\n  max-ten: false\n  ./rules/local.js: true\nfilters:\n  comments: true\nplugins:\n  - markdown\n",
            Path::new("/project"),
        );
        assert_eq!(
            result.referenced_dependencies,
            vec![
                "textlint-rule-no-todo".to_string(),
                "textlint-filter-rule-comments".to_string(),
                "textlint-plugin-markdown".to_string(),
            ]
        );
        assert_eq!(
            result.setup_files,
            vec![PathBuf::from("/project/rules/local.js")]
        );
    }

    #[test]
    fn malformed_yaml_config_is_empty() {
        let result = TextlintPlugin.resolve_config(
            Path::new("/project/.textlintrc.yaml"),
            "rules: [unclosed",
            Path::new("/project"),
        );
        assert!(result.referenced_dependencies.is_empty());
    }

    #[test]
    fn local_rule_name_is_a_setup_file() {
        let result = TextlintPlugin.resolve_config(
            Path::new("/project/.textlintrc.json"),
            r#"{ "rules": { "./rules/local.js": true } }"#,
            Path::new("/project"),
        );
        assert!(result.referenced_dependencies.is_empty());
        assert_eq!(
            result.setup_files,
            vec![PathBuf::from("/project/rules/local.js")]
        );
    }

    #[test]
    fn scoped_name_with_the_prefix_keeps_its_name() {
        assert_eq!(
            full_package_name(RULE_PREFIX, "@scope/textlint-rule-foo"),
            "@scope/textlint-rule-foo"
        );
        assert_eq!(
            full_package_name(RULE_PREFIX, "@scope/foo"),
            "@scope/textlint-rule-foo"
        );
    }
}
