//! Stylelint CSS linter plugin.
//!
//! Stylelint loads the modules that its config names in `extends`, `plugins`
//! and `customSyntax`, at the top level and in each `overrides` entry. A name
//! is a package, and a relative or absolute path is a local module. These
//! references have no import in the project, so the plugin credits the
//! packages and keeps the local modules reachable. Imports in a script
//! config are credited too.
//!
//! An `extends` target is a config too, so the plugin reads it the same way:
//! a local path, or the file that a workspace package loads. The plugin reads
//! only files inside the project root.
//!
//! The plugin reads `stylelint.config.{js,cjs,mjs,ts}`, `.stylelintrc` (JSON
//! or YAML form), `.stylelintrc.{json,yaml,yml,js,cjs,mjs}`, and the
//! `stylelint` key of `package.json`.

use std::path::{Path, PathBuf};

use oxc_ast::ast::{ArrayExpressionElement, Expression, ObjectExpression};

use super::config_parser;
use super::eslint::push_setup_file_once;
use super::extends_walk::{ExtendsWalk, workspace_package_dir, workspace_package_file};
use super::{Plugin, PluginResult};

const ENABLERS: &[&str] = &["stylelint"];

const CONFIG_PATTERNS: &[&str] = &[
    "stylelint.config.{js,cjs,mjs,ts}",
    ".stylelintrc",
    ".stylelintrc.{json,yaml,yml,js,cjs,mjs}",
];

const ALWAYS_USED: &[&str] = &[
    "stylelint.config.{js,cjs,mjs,ts}",
    ".stylelintrc",
    ".stylelintrc.{json,yaml,yml,js,cjs,mjs}",
];

const TOOLING_DEPENDENCIES: &[&str] = &[
    "stylelint",
    "stylelint-config-standard",
    "stylelint-config-recommended",
    "stylelint-order",
    "stylelint-scss",
    "postcss-scss",
];

const EXTENDS: &str = "extends";

/// The config keys whose string values Stylelint loads as modules.
const MODULE_KEYS: &[&str] = &[EXTENDS, "plugins", "customSyntax"];

define_plugin! {
    struct StylelintPlugin => "stylelint",
    enablers: ENABLERS,
    config_patterns: CONFIG_PATTERNS,
    always_used: ALWAYS_USED,
    tooling_dependencies: TOOLING_DEPENDENCIES,
    package_json_config_key: "stylelint",
    resolve_config(config_path, source, root) {
        resolve_stylelint_config(config_path, source, root, root)
    },
    resolve_config_in_project(config_path, source, root, project_root) {
        resolve_stylelint_config(config_path, source, root, project_root)
    },
}

/// Resolve a Stylelint config. Every config that its `extends` chain names
/// must be inside `project_root`.
fn resolve_stylelint_config(
    config_path: &Path,
    source: &str,
    root: &Path,
    project_root: &Path,
) -> PluginResult {
    let mut result = PluginResult::default();
    let mut walk = ExtendsWalk::new(project_root);
    credit_config(&mut result, config_path, source, root, &mut walk, 0);
    result
}

/// The number of configs that one walk reads at most, the config itself
/// included, as in the `ESLint` plugin. The visited set stops a cycle; the cap
/// stops a long chain of distinct files.
const MAX_EXTENDS_DEPTH: usize = 8;

/// Credit the packages that one config names and keep its local modules
/// reachable. A config that `extends` names is read the same way: a local
/// path, or the file that a workspace package loads.
fn credit_config(
    result: &mut PluginResult,
    config_path: &Path,
    source: &str,
    root: &Path,
    walk: &mut ExtendsWalk<'_>,
    depth: usize,
) {
    if depth >= MAX_EXTENDS_DEPTH || !walk.visited.insert(visit_key(config_path)) {
        return;
    }
    let modules = match config_format(config_path, source) {
        ConfigFormat::Script => {
            super::add_import_referenced_dependencies(result, source, config_path);
            extract_script_modules(source, config_path)
        }
        ConfigFormat::Json => extract_script_modules(
            &config_parser::wrap_json_as_expression(source),
            &config_path.with_extension("js"),
        ),
        ConfigFormat::Yaml => extract_yaml_modules(source),
    };

    let config_dir = config_path.parent().unwrap_or(root);
    for ConfigModule { key, specifier } in modules {
        if config_parser::is_local_module_specifier(&specifier) {
            if key == EXTENDS {
                let path = resolve_extends_path(config_dir, &specifier);
                if config_parser::is_inside_root(&path, walk.project_root()) {
                    push_setup_file_once(result, path.clone());
                    follow_extends(result, &path, root, walk, depth);
                }
            } else {
                let path = config_parser::resolve_local_module(config_dir, &specifier);
                push_setup_file_once(result, path);
            }
        } else if let Some(package) = config_parser::config_string_package_name(&specifier) {
            if key == EXTENDS
                && let Some(shared) =
                    workspace_shared_config_file(config_dir, &specifier, &package, root, walk)
            {
                push_setup_file_once(result, shared.clone());
                follow_extends(result, &shared, root, walk, depth);
            }
            result.referenced_dependencies.push(package);
        }
    }
}

/// Read a config that an `extends` entry names. The caller has checked that
/// the path is inside the project root.
fn follow_extends(
    result: &mut PluginResult,
    path: &Path,
    root: &Path,
    walk: &mut ExtendsWalk<'_>,
    depth: usize,
) {
    if let Ok(source) = std::fs::read_to_string(path) {
        credit_config(result, path, &source, root, walk, depth + 1);
    }
}

fn visit_key(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

/// The file that a local `extends` path names. Stylelint tries the path, the
/// path with `.js` and `.json`, then `index.js` and `index.json` in the
/// directory. Without a match, the joined path is returned.
fn resolve_extends_path(config_dir: &Path, specifier: &str) -> PathBuf {
    let base = config_parser::lexical_normalize(&config_dir.join(specifier));
    let with_suffix = |suffix: &str| {
        let mut path = base.clone().into_os_string();
        path.push(suffix);
        PathBuf::from(path)
    };
    let mut candidates = ["", ".js", ".json"]
        .map(with_suffix)
        .into_iter()
        .chain(["index.js", "index.json"].map(|index| base.join(index)));
    candidates.find(|path| path.is_file()).unwrap_or(base)
}

/// The file that an `extends` package loads, when the package is a workspace
/// package of the project. The package directory and the file must be inside
/// the project root. A registry package under `node_modules` gives `None`.
fn workspace_shared_config_file(
    config_dir: &Path,
    specifier: &str,
    package: &str,
    root: &Path,
    walk: &mut ExtendsWalk<'_>,
) -> Option<PathBuf> {
    let pkg_dir = workspace_package_dir(config_dir, package, walk)?;
    if !config_parser::is_inside_root(&pkg_dir, walk.project_root()) {
        return None;
    }
    let subpath = specifier.strip_prefix(package)?;
    workspace_package_file(&pkg_dir, subpath, root)
        .filter(|file| config_parser::is_inside_root(file, walk.project_root()))
}

/// One string value of a module key.
struct ConfigModule {
    key: &'static str,
    specifier: String,
}

enum ConfigFormat {
    Script,
    Json,
    Yaml,
}

/// The config format. An extensionless `.stylelintrc` holds JSON or YAML.
fn config_format(config_path: &Path, source: &str) -> ConfigFormat {
    match config_path.extension().and_then(|ext| ext.to_str()) {
        Some("json") => ConfigFormat::Json,
        Some("yaml" | "yml") => ConfigFormat::Yaml,
        Some(_) => ConfigFormat::Script,
        None if source.trim_start().starts_with('{') => ConfigFormat::Json,
        None => ConfigFormat::Yaml,
    }
}

fn extract_script_modules(source: &str, path: &Path) -> Vec<ConfigModule> {
    config_parser::extract_from_source(source, path, |program| {
        let config = config_parser::find_config_object(program)?;
        let mut modules = object_modules(config);
        if let Some(overrides) = config_parser::property_expr(config, "overrides")
            .and_then(config_parser::array_expression)
        {
            for entry in overrides
                .elements
                .iter()
                .filter_map(ArrayExpressionElement::as_expression)
                .filter_map(config_parser::object_expression)
            {
                modules.extend(object_modules(entry));
            }
        }
        Some(modules)
    })
    .unwrap_or_default()
}

/// The string values of the module keys of one config object. Each key takes
/// one string or an array.
fn object_modules(obj: &ObjectExpression<'_>) -> Vec<ConfigModule> {
    let mut modules = Vec::new();
    for &key in MODULE_KEYS {
        let Some(value) = config_parser::property_expr(obj, key) else {
            continue;
        };
        if matches!(
            value,
            Expression::StringLiteral(_) | Expression::ArrayExpression(_)
        ) {
            modules.extend(
                config_parser::collect_shallow_string_values(value)
                    .into_iter()
                    .map(|specifier| ConfigModule { key, specifier }),
            );
        }
    }
    modules
}

fn extract_yaml_modules(source: &str) -> Vec<ConfigModule> {
    let Ok(document) = fallow_config::yaml::parse(source) else {
        return Vec::new();
    };
    let root = document.root();
    let mut modules = yaml_object_modules(root);
    if let Some(overrides) = root.get("overrides").and_then(|node| node.as_sequence()) {
        for entry in overrides {
            modules.extend(yaml_object_modules(entry));
        }
    }
    modules
}

fn yaml_object_modules(node: fallow_config::yaml::YamlNode<'_>) -> Vec<ConfigModule> {
    let mut modules = Vec::new();
    for &key in MODULE_KEYS {
        let Some(value) = node.get(key) else {
            continue;
        };
        let module = |text: &str| ConfigModule {
            key,
            specifier: text.to_string(),
        };
        if let Some(text) = value.as_str() {
            modules.push(module(text));
        } else if let Some(items) = value.as_sequence() {
            modules.extend(items.filter_map(|item| item.as_str()).map(module));
        }
    }
    modules
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn resolve(file: &str, source: &str) -> PluginResult {
        StylelintPlugin.resolve_config(
            &Path::new("/project").join(file),
            source,
            Path::new("/project"),
        )
    }

    #[test]
    fn script_config_credits_module_keys_and_overrides() {
        let result = resolve(
            "stylelint.config.mjs",
            r#"export default {
                extends: ["stylelint-config-example", "@acme/config-stylelint"],
                plugins: ["@acme/stylelint-plugin-tokens/rules"],
                customSyntax: "postcss-example",
                overrides: [
                    { files: ["**/*.html"], customSyntax: "postcss-example-html" },
                    { files: ["**/*.vue"], extends: "@acme/config-vue", plugins: ["./rules/local.js"] },
                ],
            };"#,
        );
        assert_eq!(
            result.referenced_dependencies,
            vec![
                "stylelint-config-example".to_string(),
                "@acme/config-stylelint".to_string(),
                "@acme/stylelint-plugin-tokens".to_string(),
                "postcss-example".to_string(),
                "postcss-example-html".to_string(),
                "@acme/config-vue".to_string(),
            ]
        );
        assert_eq!(
            result.setup_files,
            vec![PathBuf::from("/project/rules/local.js")]
        );
    }

    #[test]
    fn local_paths_are_setup_files_not_packages() {
        let result = resolve(
            "stylelint.config.js",
            r#"module.exports = {
                plugins: ["./base.js", "../plugins/local.js"],
            };"#,
        );
        assert!(result.referenced_dependencies.is_empty());
        assert_eq!(
            result.setup_files,
            vec![
                PathBuf::from("/project/base.js"),
                PathBuf::from("/plugins/local.js"),
            ]
        );
    }

    #[test]
    fn script_config_keeps_import_credits() {
        let result = resolve(
            "stylelint.config.mjs",
            r#"import order from "stylelint-config-example-order";
            export default { extends: [order] };"#,
        );
        assert_eq!(
            result.referenced_dependencies,
            vec!["stylelint-config-example-order".to_string()]
        );
    }

    #[test]
    fn json_config_credits_module_keys() {
        let source = r#"{
            "extends": "@acme/config-stylelint",
            "plugins": ["stylelint-plugin-example"],
            "overrides": [{ "files": ["*.scss"], "customSyntax": "postcss-example" }]
        }"#;
        let expected = vec![
            "@acme/config-stylelint".to_string(),
            "stylelint-plugin-example".to_string(),
            "postcss-example".to_string(),
        ];
        // `stylelint.config.json` stands in for the synthetic path that the
        // registry gives the `stylelint` key of `package.json`.
        for file in [".stylelintrc.json", ".stylelintrc", "stylelint.config.json"] {
            assert_eq!(
                resolve(file, source).referenced_dependencies,
                expected,
                "{file}"
            );
        }
    }

    #[test]
    fn yaml_config_credits_module_keys() {
        let source = "extends: \"@acme/config-stylelint\"\nplugins:\n  - stylelint-plugin-example\n  - ./local-plugin.js\noverrides:\n  - files: [\"*.html\"]\n    customSyntax: postcss-example-html\n";
        for file in [".stylelintrc.yaml", ".stylelintrc.yml", ".stylelintrc"] {
            let result = resolve(file, source);
            assert_eq!(
                result.referenced_dependencies,
                vec![
                    "@acme/config-stylelint".to_string(),
                    "stylelint-plugin-example".to_string(),
                    "postcss-example-html".to_string(),
                ],
                "{file}"
            );
            assert_eq!(
                result.setup_files,
                vec![PathBuf::from("/project/local-plugin.js")],
                "{file}"
            );
        }
    }

    fn write(path: &Path, contents: &str) {
        std::fs::create_dir_all(path.parent().expect("parent")).expect("create dir");
        std::fs::write(path, contents).expect("write file");
    }

    /// Resolve the config at `path` as the registry does: read it from disk,
    /// with `root` as the package root and `project_root` as the analysis
    /// root.
    fn resolve_file(path: &Path, root: &Path, project_root: &Path) -> PluginResult {
        let source = std::fs::read_to_string(path).expect("read config");
        StylelintPlugin.resolve_config_in_project(path, &source, root, project_root)
    }

    #[test]
    fn local_extends_cycle_reads_each_config_once() {
        let dir = tempfile::tempdir().expect("temp dir");
        let config = dir.path().join("stylelint.config.js");
        let base = dir.path().join("base.json");
        write(
            &config,
            r#"module.exports = { extends: ["./base.json"], plugins: ["stylelint-plugin-example"] };"#,
        );
        write(
            &base,
            r#"{ "extends": "./stylelint.config.js", "customSyntax": "postcss-example" }"#,
        );
        let result = resolve_file(&config, dir.path(), dir.path());
        assert_eq!(
            result.referenced_dependencies,
            vec![
                "postcss-example".to_string(),
                "stylelint-plugin-example".to_string(),
            ]
        );
        assert_eq!(result.setup_files, vec![base, config]);
    }

    #[test]
    fn shared_local_extends_target_is_one_setup_file() {
        let dir = tempfile::tempdir().expect("temp dir");
        let config = dir.path().join(".stylelintrc.json");
        write(&config, r#"{ "extends": ["./a.json", "./b.json"] }"#);
        write(
            &dir.path().join("a.json"),
            r#"{ "extends": "./shared.json" }"#,
        );
        write(
            &dir.path().join("b.json"),
            r#"{ "extends": "./shared.json" }"#,
        );
        write(
            &dir.path().join("shared.json"),
            r#"{ "customSyntax": "postcss-example" }"#,
        );
        let result = resolve_file(&config, dir.path(), dir.path());
        assert_eq!(
            result.referenced_dependencies,
            vec!["postcss-example".to_string()]
        );
        assert_eq!(
            result.setup_files,
            ["a.json", "shared.json", "b.json"].map(|file| dir.path().join(file))
        );
    }

    #[test]
    fn local_extends_chain_stops_at_the_depth_cap() {
        let dir = tempfile::tempdir().expect("temp dir");
        let config = |level: usize| {
            format!(
                r#"{{ "extends": "./level-{}.json", "customSyntax": "syntax-{level}" }}"#,
                level + 1
            )
        };
        write(&dir.path().join(".stylelintrc.json"), &config(0));
        for level in 1..=MAX_EXTENDS_DEPTH {
            write(
                &dir.path().join(format!("level-{level}.json")),
                &config(level),
            );
        }
        let result = resolve_file(
            &dir.path().join(".stylelintrc.json"),
            dir.path(),
            dir.path(),
        );
        let expected: Vec<String> = (0..MAX_EXTENDS_DEPTH)
            .map(|level| format!("syntax-{level}"))
            .collect();
        let mut credited = result.referenced_dependencies;
        credited.sort_by_key(|name| {
            name.strip_prefix("syntax-")
                .and_then(|level| level.parse::<usize>().ok())
                .unwrap_or(usize::MAX)
        });
        assert_eq!(credited, expected);
    }

    #[test]
    fn extensionless_local_extends_finds_the_file() {
        let dir = tempfile::tempdir().expect("temp dir");
        let config = dir.path().join(".stylelintrc.json");
        write(&config, r#"{ "extends": ["./base", "./shared"] }"#);
        write(
            &dir.path().join("base.json"),
            r#"{ "customSyntax": "postcss-example-base" }"#,
        );
        write(
            &dir.path().join("shared/index.js"),
            r#"module.exports = { customSyntax: "postcss-example-shared" };"#,
        );
        let result = resolve_file(&config, dir.path(), dir.path());
        assert_eq!(
            result.referenced_dependencies,
            vec![
                "postcss-example-base".to_string(),
                "postcss-example-shared".to_string(),
            ]
        );
        assert_eq!(
            result.setup_files,
            vec![
                dir.path().join("base.json"),
                dir.path().join("shared/index.js"),
            ]
        );
    }

    #[test]
    fn local_extends_is_read_only_inside_the_project_root() {
        let dir = tempfile::tempdir().expect("temp dir");
        let project = dir.path().join("project");
        let package = project.join("packages/a");
        write(
            &dir.path().join("outside.json"),
            r#"{ "customSyntax": "postcss-example-outside" }"#,
        );
        write(
            &project.join(".stylelintrc.json"),
            r#"{ "customSyntax": "postcss-example-root" }"#,
        );
        let config = package.join(".stylelintrc.json");
        write(
            &config,
            r#"{ "extends": ["../../.stylelintrc.json", "../../../outside.json"] }"#,
        );
        let result = resolve_file(&config, &package, &project);
        assert_eq!(
            result.referenced_dependencies,
            vec!["postcss-example-root".to_string()]
        );
        assert!(
            !result
                .setup_files
                .contains(&dir.path().join("outside.json")),
            "{:?}",
            result.setup_files
        );
    }

    #[test]
    fn workspace_package_main_outside_the_project_is_not_read() {
        let dir = tempfile::tempdir().expect("temp dir");
        let project = dir.path().join("project");
        write(
            &project.join("package.json"),
            r#"{ "private": true, "workspaces": ["packages/*"] }"#,
        );
        write(
            &project.join("packages/config/package.json"),
            r#"{ "name": "@acme/config-stylelint", "main": "../../../outside.js" }"#,
        );
        write(
            &dir.path().join("outside.js"),
            r#"module.exports = { customSyntax: "postcss-example-outside" };"#,
        );
        let config = project.join(".stylelintrc.json");
        write(&config, r#"{ "extends": "@acme/config-stylelint" }"#);
        let result = resolve_file(&config, &project, &project);
        assert_eq!(
            result.referenced_dependencies,
            vec!["@acme/config-stylelint".to_string()]
        );
        assert!(result.setup_files.is_empty(), "{:?}", result.setup_files);
    }

    #[test]
    fn workspace_package_extends_reads_its_main_file() {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = dir.path();
        write(
            &root.join("package.json"),
            r#"{ "private": true, "workspaces": ["packages/*"] }"#,
        );
        write(
            &root.join("packages/config/package.json"),
            r#"{ "name": "@acme/config-stylelint", "main": "index.js" }"#,
        );
        write(
            &root.join("packages/config/index.js"),
            r#"module.exports = { plugins: ["stylelint-plugin-example", "./local-plugin.js"] };"#,
        );
        let config = root.join("packages/web/.stylelintrc.json");
        write(&config, r#"{ "extends": "@acme/config-stylelint" }"#);
        let result = resolve_file(&config, &root.join("packages/web"), root);
        assert_eq!(
            result.referenced_dependencies,
            vec![
                "stylelint-plugin-example".to_string(),
                "@acme/config-stylelint".to_string(),
            ]
        );
        assert_eq!(
            result.setup_files,
            vec![
                root.join("packages/config/index.js"),
                root.join("packages/config/local-plugin.js"),
            ]
        );
    }

    #[cfg(unix)]
    #[test]
    fn linked_package_outside_the_project_is_not_read() {
        let dir = tempfile::tempdir().expect("temp dir");
        let outside = dir.path().join("outside");
        write(
            &outside.join("package.json"),
            r#"{ "name": "@acme/config-stylelint", "main": "index.js" }"#,
        );
        write(
            &outside.join("index.js"),
            r#"module.exports = { customSyntax: "postcss-example-outside" };"#,
        );
        let project = dir.path().join("project");
        std::fs::create_dir_all(project.join("node_modules/@acme")).expect("create dir");
        std::os::unix::fs::symlink(
            &outside,
            project.join("node_modules/@acme/config-stylelint"),
        )
        .expect("link package");
        let config = project.join(".stylelintrc.json");
        write(&config, r#"{ "extends": "@acme/config-stylelint" }"#);
        let result = resolve_file(&config, &project, &project);
        assert_eq!(
            result.referenced_dependencies,
            vec!["@acme/config-stylelint".to_string()]
        );
        assert!(result.setup_files.is_empty());
    }

    #[test]
    fn malformed_yaml_config_is_empty() {
        assert!(resolve(".stylelintrc.yaml", "extends: [unclosed").is_empty());
    }
}
