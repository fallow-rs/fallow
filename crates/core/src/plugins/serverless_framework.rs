//! Serverless Framework plugin.
//!
//! The Serverless Framework reads `serverless.yml` (or a JS/TS config) and
//! loads each function `handler` as a module. A handler such as
//! `src/handler.main` names the export `main` of `src/handler.{js,ts}`. No
//! import reaches these files, so the plugin makes them entry points.
//!
//! The framework also loads each name in `plugins` as a package or as a local
//! module, and the esbuild build step loads each `inject` file. The plugin
//! credits these references too.
//!
//! The osls fork reads the same config. The `osls` plugin reuses the shared
//! items of this module.

use std::path::Path;

use fallow_config::yaml::{self, YamlNode};

use super::{Plugin, PluginResult, config_parser};

const ENABLERS: &[&str] = &["serverless"];

/// Config files that the framework and its forks read.
pub(super) const CONFIG_PATTERNS: &[&str] = &["serverless.{js,cjs,mjs,ts,cts,mts,yml,yaml}"];

/// The framework reads its config files by convention, so no import reaches
/// them.
pub(super) const ALWAYS_USED: &[&str] = CONFIG_PATTERNS;

/// Only a root config file gets the `default` export credit. A module such as
/// `src/lib/serverless.ts` is not a service config and stays reportable.
pub(super) const CONFIG_FILE_EXPORTS: &[(&str, &[&str])] =
    &[("serverless.{js,cjs,mjs,ts,cts,mts}", &["default"])];

const TOOLING_DEPENDENCIES: &[&str] = &["serverless"];

/// Extensions that the framework tries for a handler without an extension.
const HANDLER_EXTENSIONS: &str = "{js,mjs,cjs,ts,mts,cts}";

/// Source extensions that a handler file can already carry.
const SOURCE_EXTENSIONS: &[&str] = &[".js", ".mjs", ".cjs", ".ts", ".mts", ".cts", ".jsx", ".tsx"];

/// The package that the esbuild build step needs.
const ESBUILD_PACKAGE: &str = "esbuild";

/// The manifest that marks a nested directory as a service directory.
const PACKAGE_JSON: &str = "package.json";

/// The start of a `${file(path):selector}` variable.
const FILE_VARIABLE_PREFIX: &str = "${file(";

/// Root config files that activate the plugin without a dependency. The
/// framework CLI is often installed globally, so `package.json` does not list
/// `serverless`.
const ROOT_CONFIG_FILES: &[&str] = &[
    "serverless.yml",
    "serverless.yaml",
    "serverless.ts",
    "serverless.js",
    "serverless.cjs",
    "serverless.mjs",
    "serverless.cts",
    "serverless.mts",
];

/// The osls plugin reads the same config, so a project that depends on osls
/// leaves the config to that plugin.
const OSLS_PACKAGE: &str = "osls";

pub struct ServerlessFrameworkPlugin;

impl Plugin for ServerlessFrameworkPlugin {
    fn name(&self) -> &'static str {
        "serverless-framework"
    }

    fn enablers(&self) -> &'static [&'static str] {
        ENABLERS
    }

    fn is_enabled_with_deps(&self, deps: &[String], root: &Path) -> bool {
        if deps.iter().any(|dep| ENABLERS.contains(&dep.as_str())) {
            return true;
        }
        !deps.iter().any(|dep| dep == OSLS_PACKAGE)
            && ROOT_CONFIG_FILES
                .iter()
                .any(|file| root.join(file).is_file())
    }

    fn config_patterns(&self) -> &'static [&'static str] {
        CONFIG_PATTERNS
    }

    fn config_default_export_opt_out(&self) -> &'static [&'static str] {
        CONFIG_PATTERNS
    }

    fn config_file_exports(&self) -> &'static [(&'static str, &'static [&'static str])] {
        CONFIG_FILE_EXPORTS
    }

    fn always_used(&self) -> &'static [&'static str] {
        ALWAYS_USED
    }

    fn tooling_dependencies(&self) -> &'static [&'static str] {
        TOOLING_DEPENDENCIES
    }

    fn resolve_config(&self, config_path: &Path, source: &str, root: &Path) -> PluginResult {
        resolve_serverless_config(config_path, source, root)
    }
}

/// Read a Serverless Framework config and return the handler entries, the
/// credited plugin packages and the esbuild references.
pub(super) fn resolve_serverless_config(
    config_path: &Path,
    source: &str,
    root: &Path,
) -> PluginResult {
    let mut result = PluginResult::default();
    if !is_yaml_config(config_path) && !is_service_directory(config_path, root) {
        return result;
    }
    let references = if is_yaml_config(config_path) {
        yaml_references(config_path, source, root)
    } else {
        super::add_import_referenced_dependencies(&mut result, source, config_path);
        script_references(config_path, source)
    };
    references.apply(config_path, root, &mut result);
    result
}

/// True when a config file sits in a directory that the framework can run as
/// a service: the project or workspace root, or a directory with its own
/// `package.json`.
///
/// The framework reads `serverless.*` from the service directory only. Config
/// discovery also finds a module such as `src/lib/serverless.ts`, so a nested
/// JS or TS file outside a service directory is an ordinary module. A nested
/// `serverless.yml` is always a service config.
fn is_service_directory(config_path: &Path, root: &Path) -> bool {
    config_path
        .parent()
        .is_some_and(|dir| dir == root || dir.join(PACKAGE_JSON).is_file())
}

fn is_yaml_config(config_path: &Path) -> bool {
    matches!(
        config_path.extension().and_then(|ext| ext.to_str()),
        Some("yml" | "yaml")
    )
}

/// The raw values that a config gives, before path resolution.
#[derive(Debug, Default, PartialEq, Eq)]
struct References {
    handlers: Vec<String>,
    plugins: Vec<String>,
    injects: Vec<String>,
    uses_esbuild: bool,
}

impl References {
    fn apply(self, config_path: &Path, root: &Path, result: &mut PluginResult) {
        for handler in &self.handlers {
            if let Some(pattern) = handler_entry_pattern(handler, config_path, root) {
                result.push_entry_pattern(pattern);
            }
        }
        for plugin in &self.plugins {
            credit_plugin(plugin, config_path, root, result);
        }
        for inject in &self.injects {
            if let Some(path) = config_parser::normalize_config_path(inject, config_path, root) {
                result.push_entry_path(path);
            }
        }
        if self.uses_esbuild {
            result
                .referenced_dependencies
                .push(ESBUILD_PACKAGE.to_string());
        }
    }
}

/// The module file part of a handler value.
///
/// The framework splits a handler at the last dot: `src/handler.main` names
/// the export `main` of the module `src/handler`. A handler without a dot
/// names no module and gives `None`.
fn handler_module(handler: &str) -> Option<&str> {
    let (module, _export) = handler.trim().rsplit_once('.')?;
    (!module.is_empty()).then_some(module)
}

/// The project-relative entry pattern for a handler value.
///
/// A module path with a source extension is a file. A module path without one
/// matches each handler extension. The path is relative to the config file
/// directory.
fn handler_entry_pattern(handler: &str, config_path: &Path, root: &Path) -> Option<String> {
    let module = handler_module(handler)?;
    let path = config_parser::normalize_config_path(module, config_path, root)?;
    if SOURCE_EXTENSIONS.iter().any(|ext| path.ends_with(ext)) {
        Some(path)
    } else {
        Some(format!("{path}.{HANDLER_EXTENSIONS}"))
    }
}

/// Credit one value of the `plugins` list.
///
/// A relative or absolute path names a local plugin module, so the plugin
/// keeps that file reachable. Any other value names a package.
fn credit_plugin(plugin: &str, config_path: &Path, root: &Path, result: &mut PluginResult) {
    let plugin = plugin.trim();
    // An unresolved variable such as `${self:custom.plugin}` names no package.
    if plugin.is_empty() || plugin.contains("${") {
        return;
    }
    if plugin.starts_with('.') || Path::new(plugin).is_absolute() {
        if let Some(path) = config_parser::normalize_config_path(plugin, config_path, root) {
            result.push_entry_path(path);
        }
        return;
    }
    result
        .referenced_dependencies
        .push(crate::resolve::extract_package_name(plugin));
}

/// Read the references of a JS or TS config.
///
/// The reader takes static values only. A `${file(...)}` variable in a script
/// config is not resolved.
fn script_references(config_path: &Path, source: &str) -> References {
    let mut injects = Vec::new();
    let mut uses_esbuild = false;
    for section in ["custom", "build"] {
        injects.extend(config_parser::extract_config_string_or_array(
            source,
            config_path,
            &[section, "esbuild", "inject"],
        ));
        uses_esbuild |= config_parser::extract_config_truthy_bool_or_object(
            source,
            config_path,
            &[section, "esbuild"],
        );
    }
    References {
        handlers: config_parser::extract_config_object_nested_strings(
            source,
            config_path,
            &["functions"],
            &["handler"],
        ),
        plugins: config_parser::extract_config_string_array(source, config_path, &["plugins"]),
        injects,
        uses_esbuild,
    }
}

/// Read the references of a YAML config.
///
/// The `functions` value, each function group and each function can be a
/// `${file(path):selector}` variable. The plugin reads the YAML or JSON file
/// that the variable names, relative to the config file directory. A file
/// outside the project root is not read.
fn yaml_references(config_path: &Path, source: &str, root: &Path) -> References {
    let Ok(document) = yaml::parse(source) else {
        return References::default();
    };
    let config = document.root();
    let resolver = FileVariables { config_path, root };
    let mut references = References::default();

    if let Some(functions) = config.get("functions") {
        resolver.with_resolved(functions, &mut |functions| {
            collect_function_groups(functions, &resolver, &mut references.handlers);
        });
    }

    if let Some(plugins) = config.get("plugins") {
        resolver.with_resolved(plugins, &mut |plugins| {
            let Some(items) = plugins.as_sequence() else {
                return;
            };
            for item in items {
                resolver.with_resolved(item, &mut |item| {
                    references.plugins.extend(string_or_sequence(item));
                });
            }
        });
    }

    for section in ["custom", "build"] {
        let Some(esbuild) = config.get(section).and_then(|node| node.get("esbuild")) else {
            continue;
        };
        if is_truthy(esbuild) {
            references.uses_esbuild = true;
        }
        if let Some(inject) = esbuild.get("inject") {
            references.injects.extend(string_or_sequence(inject));
        }
    }

    references
}

/// Collect the handlers of a `functions` value. The value is a mapping of
/// functions, or a sequence of such mappings.
fn collect_function_groups(
    functions: YamlNode<'_>,
    resolver: &FileVariables<'_>,
    handlers: &mut Vec<String>,
) {
    let mut collect_group = |group: YamlNode<'_>| {
        resolver.with_resolved(group, &mut |group| {
            collect_functions(group, resolver, handlers);
        });
    };
    match functions.as_sequence() {
        Some(groups) => groups.for_each(&mut collect_group),
        None => collect_group(functions),
    }
}

fn collect_functions(
    group: YamlNode<'_>,
    resolver: &FileVariables<'_>,
    handlers: &mut Vec<String>,
) {
    let Some(functions) = group.as_mapping() else {
        return;
    };
    for function in functions.values() {
        resolver.with_resolved(function, &mut |function| {
            if let Some(handler) = function.get("handler").and_then(YamlNode::as_str) {
                handlers.push(handler.to_string());
            }
        });
    }
}

fn is_truthy(node: YamlNode<'_>) -> bool {
    node.as_mapping().is_some() || node.as_bool() == Some(true)
}

fn string_or_sequence(node: YamlNode<'_>) -> Vec<String> {
    if let Some(value) = node.as_str() {
        return vec![value.to_string()];
    }
    node.as_sequence()
        .map(|items| {
            items
                .filter_map(|item| item.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

/// A parsed `${file(path):selector}` variable.
#[derive(Debug, PartialEq, Eq)]
struct FileVariable<'a> {
    path: &'a str,
    selector: Option<&'a str>,
}

/// Parse a `${file(path)}` or `${file(path):selector}` variable at the start
/// of `value`. A fallback after the selector, such as `, self:custom.x`, is
/// ignored.
fn parse_file_variable(value: &str) -> Option<FileVariable<'_>> {
    let rest = value.strip_prefix(FILE_VARIABLE_PREFIX)?;
    let close = rest.find(')')?;
    let path = rest[..close].trim();
    if path.is_empty() || path.contains(['(', '$']) {
        return None;
    }
    let selector = rest[close + 1..]
        .strip_prefix(':')
        .map(|tail| {
            let end = tail
                .find(|c: char| c.is_whitespace() || c == ',' || c == '}')
                .unwrap_or(tail.len());
            &tail[..end]
        })
        .filter(|selector| !selector.is_empty());
    Some(FileVariable { path, selector })
}

/// Resolves `${file(...)}` variables relative to the config file directory.
struct FileVariables<'a> {
    config_path: &'a Path,
    root: &'a Path,
}

impl FileVariables<'_> {
    /// Call `visit` with the value that `node` stands for.
    ///
    /// A node that is not a file variable stands for itself. A file variable
    /// stands for the selected value of the file it names. When the file
    /// cannot be read, `visit` is not called.
    fn with_resolved(&self, node: YamlNode<'_>, visit: &mut dyn FnMut(YamlNode<'_>)) {
        let Some(variable) = node.as_str().and_then(parse_file_variable) else {
            visit(node);
            return;
        };
        let Some(relative) = config_parser::normalize_filesystem_config_path_buf(
            variable.path,
            self.config_path,
            self.root,
        ) else {
            return;
        };
        let Ok(source) = std::fs::read_to_string(self.root.join(relative)) else {
            return;
        };
        let Ok(document) = yaml::parse(&source) else {
            return;
        };
        let mut target = document.root();
        if let Some(selector) = variable.selector {
            for segment in selector.split('.') {
                let Some(next) = target.get(segment) else {
                    return;
                };
                target = next;
            }
        }
        visit(target);
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    /// Build a platform-absolute path from a logical `/`-rooted string, so
    /// path resolution behaves the same on Windows.
    fn abs(logical: &str) -> PathBuf {
        #[cfg(windows)]
        {
            PathBuf::from(format!("C:{}", logical.replace('/', "\\")))
        }
        #[cfg(not(windows))]
        {
            PathBuf::from(logical)
        }
    }

    fn entry_patterns(result: &PluginResult) -> Vec<String> {
        result
            .entry_patterns
            .iter()
            .map(|rule| rule.pattern.clone())
            .collect()
    }

    fn resolve(config: &str, source: &str) -> PluginResult {
        ServerlessFrameworkPlugin.resolve_config(&abs(config), source, &abs("/project"))
    }

    #[test]
    fn handler_module_splits_at_the_last_dot() {
        assert_eq!(handler_module("src/handler.main"), Some("src/handler"));
        assert_eq!(
            handler_module("function/handler.js.handler"),
            Some("function/handler.js")
        );
        assert_eq!(handler_module("handler"), None);
        assert_eq!(handler_module(".main"), None);
    }

    #[test]
    fn handler_entry_pattern_adds_extensions_only_when_missing() {
        let config = abs("/project/serverless.yml");
        let root = abs("/project");
        assert_eq!(
            handler_entry_pattern("src/handler.main", &config, &root).as_deref(),
            Some("src/handler.{js,mjs,cjs,ts,mts,cts}")
        );
        assert_eq!(
            handler_entry_pattern("function/handler.js.handler", &config, &root).as_deref(),
            Some("function/handler.js")
        );
    }

    #[test]
    fn handler_is_relative_to_the_config_directory() {
        let result = resolve(
            "/project/services/api/serverless.yml",
            "functions:\n  hello:\n    handler: src/hello.handler\n",
        );
        assert_eq!(
            entry_patterns(&result),
            vec!["services/api/src/hello.{js,mjs,cjs,ts,mts,cts}".to_string()]
        );
    }

    #[test]
    fn yaml_config_with_cloudformation_tags_gives_handlers_and_plugins() {
        let source = r"
service: TestService
provider:
  name: aws
  iamRoleStatements:
    - Effect: Allow
      Resource: !Sub 'arn:aws:s3:::${self:custom.bucket}/*'
plugins:
  - serverless-offline
  - '@scope/serverless-plugin/sub'
  - ./plugins/local-plugin
functions:
  First:
    handler: function/handler.js.handler
  Second:
    handler: function-2/function2.handler
    environment:
      FOO: !Ref SomeResource
  Third:
    image:
      command: dist/functions/third.handler
";
        let result = resolve("/project/serverless.yml", source);
        let entries = entry_patterns(&result);
        assert!(
            entries.contains(&"function/handler.js".to_string()),
            "{entries:?}"
        );
        assert!(
            entries.contains(&"function-2/function2.{js,mjs,cjs,ts,mts,cts}".to_string()),
            "{entries:?}"
        );
        assert!(
            entries.contains(&"plugins/local-plugin".to_string()),
            "{entries:?}"
        );
        assert!(
            !entries.iter().any(|entry| entry.contains("third")),
            "{entries:?}"
        );
        assert_eq!(
            result.referenced_dependencies,
            vec![
                "serverless-offline".to_string(),
                "@scope/serverless-plugin".to_string()
            ]
        );
    }

    #[test]
    fn yaml_esbuild_section_credits_esbuild_and_inject_files() {
        let source = r"
custom:
  esbuild:
    inject:
      - ./src/globals.ts
build:
  esbuild:
    inject: ./src/production.ts
";
        let result = resolve("/project/serverless.yaml", source);
        let entries = entry_patterns(&result);
        assert!(
            entries.contains(&"src/globals.ts".to_string()),
            "{entries:?}"
        );
        assert!(
            entries.contains(&"src/production.ts".to_string()),
            "{entries:?}"
        );
        assert_eq!(result.referenced_dependencies, vec!["esbuild".to_string()]);
    }

    #[test]
    fn disabled_esbuild_section_does_not_credit_esbuild() {
        let result = resolve("/project/serverless.yml", "build:\n  esbuild: false\n");
        assert!(result.referenced_dependencies.is_empty());
    }

    #[test]
    fn invalid_yaml_gives_an_empty_result() {
        let result = resolve("/project/serverless.yml", "functions: [unclosed\n");
        assert!(result.entry_patterns.is_empty());
        assert!(result.referenced_dependencies.is_empty());
    }

    #[test]
    fn typescript_config_gives_handlers_plugins_and_inject_files() {
        let source = r"
import type { AWS } from '@serverless/typescript';

const config: AWS = {
  service: 'demo',
  custom: {
    esbuild: { bundle: true, inject: ['./src/libs/commonjs-globals.ts'] },
  },
  plugins: ['serverless-esbuild', 'serverless-offline'],
  functions: {
    hello: { handler: 'src/functions/hello/handler.main' },
  },
};

export default config;
";
        let result = resolve("/project/serverless.ts", source);
        let entries = entry_patterns(&result);
        assert!(
            entries.contains(&"src/functions/hello/handler.{js,mjs,cjs,ts,mts,cts}".to_string()),
            "{entries:?}"
        );
        assert!(
            entries.contains(&"src/libs/commonjs-globals.ts".to_string()),
            "{entries:?}"
        );
        let deps = &result.referenced_dependencies;
        for dep in [
            "serverless-esbuild",
            "serverless-offline",
            "esbuild",
            "@serverless/typescript",
        ] {
            assert!(
                deps.contains(&dep.to_string()),
                "{dep} missing from {deps:?}"
            );
        }
    }

    #[test]
    fn nested_script_config_outside_a_service_directory_gives_an_empty_result() {
        let source = "export default { plugins: ['serverless-offline'], functions: { a: { handler: 'orphan.main' } } };";
        let result = resolve("/project/src/lib/serverless.ts", source);
        assert!(result.entry_patterns.is_empty());
        assert!(result.referenced_dependencies.is_empty());
    }

    #[test]
    fn nested_yaml_config_is_a_service_config() {
        let result = resolve(
            "/project/services/api/serverless.yml",
            "plugins:\n  - serverless-offline\nfunctions:\n  a:\n    handler: handler.main\n",
        );
        assert_eq!(
            entry_patterns(&result),
            vec!["services/api/handler.{js,mjs,cjs,ts,mts,cts}"]
        );
        assert_eq!(result.referenced_dependencies, vec!["serverless-offline"]);
    }

    #[test]
    fn parse_file_variable_reads_path_and_selector() {
        assert_eq!(
            parse_file_variable("${file(./functions.yml)}"),
            Some(FileVariable {
                path: "./functions.yml",
                selector: None
            })
        );
        assert_eq!(
            parse_file_variable("${file(./shared.yml):functions.hello}"),
            Some(FileVariable {
                path: "./shared.yml",
                selector: Some("functions.hello")
            })
        );
        assert_eq!(
            parse_file_variable("${file(./optional.yml):function, self:custom.fallback}"),
            Some(FileVariable {
                path: "./optional.yml",
                selector: Some("function")
            })
        );
        assert_eq!(parse_file_variable("${file(${self:custom.path})}"), None);
        assert_eq!(parse_file_variable("${self:custom.value}"), None);
        assert_eq!(parse_file_variable("src/handler.main"), None);
    }

    #[test]
    fn file_variables_resolve_against_files_on_disk() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        std::fs::write(
            root.join("functions.yml"),
            "users:\n  handler: src/users.handler\n",
        )
        .expect("write functions.yml");
        std::fs::write(
            root.join("shared.yml"),
            "plugins:\n  - serverless-offline\nfunctions:\n  hello:\n    handler: src/hello.handler\n",
        )
        .expect("write shared.yml");
        let source = "
plugins: ${file(./shared.yml):plugins}
functions:
  - ${file(./functions.yml)}
  - hello: ${file(./shared.yml):functions.hello}
  - optional: ${file(./optional.yml):function, self:custom.fallback}
";
        let result =
            ServerlessFrameworkPlugin.resolve_config(&root.join("serverless.yml"), source, root);
        let entries = entry_patterns(&result);
        assert!(
            entries.contains(&"src/users.{js,mjs,cjs,ts,mts,cts}".to_string()),
            "{entries:?}"
        );
        assert!(
            entries.contains(&"src/hello.{js,mjs,cjs,ts,mts,cts}".to_string()),
            "{entries:?}"
        );
        assert_eq!(entries.len(), 2, "{entries:?}");
        assert_eq!(
            result.referenced_dependencies,
            vec!["serverless-offline".to_string()]
        );
    }

    #[test]
    fn file_variable_outside_the_root_is_not_read() {
        let result = resolve(
            "/project/serverless.yml",
            "functions: ${file(/etc/functions.yml)}\n",
        );
        assert!(result.entry_patterns.is_empty());
    }

    #[test]
    fn root_config_activates_without_a_dependency() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("serverless.yml"), "service: app\n").expect("write");
        assert!(ServerlessFrameworkPlugin.is_enabled_with_deps(&[], dir.path()));
    }

    #[test]
    fn root_config_leaves_an_osls_project_to_the_osls_plugin() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("serverless.yml"), "service: app\n").expect("write");
        let deps = vec!["osls".to_string()];
        assert!(!ServerlessFrameworkPlugin.is_enabled_with_deps(&deps, dir.path()));
    }

    #[test]
    fn no_dependency_and_no_config_stays_inactive() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert!(!ServerlessFrameworkPlugin.is_enabled_with_deps(&[], dir.path()));
    }
}
