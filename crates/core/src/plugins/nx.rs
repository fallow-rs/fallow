//! Nx monorepo plugin.
//!
//! Detects Nx projects and marks workspace config files as always used.
//! Parses `project.json` to extract executor references as tooling dependencies
//! and `options.main` as entry points.
//!
//! Nx loads the files of a workspace-local plugin at runtime, so no source file
//! imports them. The plugin makes these files entry points:
//!
//! - the `implementation`, `batchImplementation`, `factory` and `hasher` files
//!   of each entry in an `executors.json` or `generators.json` collection;
//! - a local plugin path in the `plugins` array of `nx.json`, and the `main`
//!   file of its `package.json`;
//! - the collections that the `package.json` of a local plugin or a local
//!   executor path (`./tools/plugin:build`) names in its `executors`,
//!   `builders`, `generators` or `schematics` field.

use std::path::Path;

use super::config_parser;
use super::{Plugin, PluginResult};

define_plugin!(
    struct NxPlugin => "nx",
    enablers: &["nx"],
    config_patterns: &[
        "nx.json",
        "**/project.json",
        "**/executors.json",
        "**/generators.json",
    ],
    always_used: &["nx.json", "**/project.json"],
    tooling_dependencies: &[
        "nx",
        "@nx/workspace",
        "@nx/js",
        "@nx/react",
        "@nx/next",
        "@nx/node",
        "@nx/web",
        "@nx/vite",
        "@nx/jest",
        "@nx/eslint",
        "@nx/angular",
        "@nx/storybook",
        "@nx/webpack",
        "@nx/cypress",
        "@nx/playwright",
        "@nx/rollup",
        "@nx/esbuild",
        "@nx/rspack",
        "@nx/express",
        "@nx/nest",
    ],
    resolve_config(config_path, source, _root) {
        let file_name = config_path.file_name().and_then(|name| name.to_str());
        match file_name {
            Some("nx.json") => return resolve_nx_json(config_path, source, _root),
            Some("executors.json" | "generators.json") => {
                let mut result = PluginResult::default();
                resolve_collection(config_path, source, _root, &mut result);
                return result;
            }
            _ => {}
        }

        let mut result = PluginResult::default();

        let executor_strings = config_parser::extract_config_object_nested_strings(
            source,
            config_path,
            &["targets"],
            &["executor"],
        );
        credit_executors(&executor_strings, _root, &mut result);

        let project_root_rel = config_path
            .parent()
            .and_then(|p| p.strip_prefix(_root).ok())
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default();

        for field in &["main", "browser"] {
            let mains = config_parser::extract_config_object_nested_strings(
                source,
                config_path,
                &["targets"],
                &["options", field],
            );
            for main in &mains {
                let expanded = expand_nx_tokens(main, &project_root_rel);
                let path = expanded.trim_start_matches("./");
                result.push_entry_pattern(path.to_string());
            }
        }

        for field in &["styles", "scripts"] {
            let entries = config_parser::extract_config_object_nested_string_or_array(
                source,
                config_path,
                &["targets"],
                &["options", field],
            );
            for entry in &entries {
                let expanded = expand_nx_tokens(entry, &project_root_rel);
                let path = expanded.trim_start_matches("./");
                result.push_entry_pattern(path.to_string());
            }
        }

        let tsconfigs = config_parser::extract_config_object_nested_strings(
            source,
            config_path,
            &["targets"],
            &["options", "tsConfig"],
        );
        for tsconfig in &tsconfigs {
            let expanded = expand_nx_tokens(tsconfig, &project_root_rel);
            let path = expanded.trim_start_matches("./");
            result.always_used_files.push(path.to_string());
        }

        let include_paths = config_parser::extract_config_object_nested_string_or_array(
            source,
            config_path,
            &["targets"],
            &["options", "stylePreprocessorOptions", "includePaths"],
        );
        for entry in &include_paths {
            let expanded = expand_nx_tokens(entry, &project_root_rel);
            let absolute = _root.join(expanded.trim_start_matches("./"));
            if absolute.is_dir() {
                result.scss_include_paths.push(absolute);
            }
        }

        result
    },
);

/// Read the workspace-level dependency references out of `nx.json`.
///
/// Nx cannot run a target whose plugin, executor, or task runner is missing, so
/// every package named here is load-bearing.
fn resolve_nx_json(config_path: &Path, source: &str, root: &Path) -> PluginResult {
    let mut result = PluginResult::default();

    let plugins = config_parser::extract_config_shallow_strings_or_object_property(
        source,
        config_path,
        "plugins",
        "plugin",
    );

    let executors = config_parser::extract_config_object_nested_strings(
        source,
        config_path,
        &["targetDefaults"],
        &["executor"],
    );

    // Nx also accepts an executor string as a `targetDefaults` key, which sets
    // defaults for every target that runs it. A plain target name has no colon.
    let executor_keys: Vec<String> =
        config_parser::extract_config_object_keys(source, config_path, &["targetDefaults"])
            .into_iter()
            .filter(|key| key.contains(':'))
            .collect();

    let runners = config_parser::extract_config_object_nested_strings(
        source,
        config_path,
        &["tasksRunnerOptions"],
        &["runner"],
    );

    for plugin in &plugins {
        match local_reference_path(plugin) {
            Some(path) => resolve_local_plugin(path, root, &mut result),
            None => credit_package(plugin, &mut result),
        }
    }
    credit_executors(executors.iter().chain(&executor_keys), root, &mut result);
    for runner in &runners {
        credit_package(runner, &mut result);
    }

    result
}

/// The keys of an executor or generator collection. Nx reads `builders` and
/// `schematics` as the Angular names of `executors` and `generators`.
const COLLECTION_KEYS: &[&str] = &["executors", "builders", "generators", "schematics"];

/// The fields of a collection entry that name a module that Nx loads.
const COLLECTION_MODULE_FIELDS: &[&str] =
    &["implementation", "batchImplementation", "factory", "hasher"];

/// Make the files of each entry in an `executors.json` or `generators.json`
/// collection entry points. The paths are relative to the collection file.
fn resolve_collection(config_path: &Path, source: &str, root: &Path, result: &mut PluginResult) {
    for key in COLLECTION_KEYS {
        for field in COLLECTION_MODULE_FIELDS {
            let modules = config_parser::extract_config_object_nested_strings(
                source,
                config_path,
                &[key],
                &[field],
            );
            for module in &modules {
                // `./src/executor#buildExecutor` names an export of the module.
                let module = module.split('#').next().unwrap_or_default();
                if let Some(path) = config_parser::normalize_config_path(module, config_path, root)
                {
                    push_module_path(path, result);
                }
            }
        }
        let schemas = config_parser::extract_config_object_nested_strings(
            source,
            config_path,
            &[key],
            &["schema"],
        );
        for schema in &schemas {
            if let Some(path) = config_parser::normalize_config_path(schema, config_path, root) {
                result.always_used_files.push(path);
            }
        }
    }
}

/// Credit each executor reference: a package for `<package>:<name>`, and the
/// collections of the local plugin for `./<path>:<name>`.
fn credit_executors<'a>(
    executors: impl IntoIterator<Item = &'a String>,
    root: &Path,
    result: &mut PluginResult,
) {
    for executor in executors {
        match local_reference_path(executor) {
            Some(path) => {
                let plugin_dir = path.split(':').next().unwrap_or_default();
                resolve_local_manifest(plugin_dir, root, false, result);
            }
            None => credit_package(executor, result),
        }
    }
}

fn credit_package(reference: &str, result: &mut PluginResult) {
    if let Some(package) = nx_reference_package(reference) {
        result.referenced_dependencies.push(package);
    }
}

/// Make a local `nx.json` plugin an entry point. The path names a module file,
/// or a directory with an index file or a `package.json`.
fn resolve_local_plugin(path: &str, root: &Path, result: &mut PluginResult) {
    let Some(relative) = workspace_relative(path, root) else {
        return;
    };
    push_module_path(relative.clone(), result);
    resolve_local_manifest(&relative, root, true, result);
}

/// Read the `package.json` in the local plugin directory `path` and credit the
/// collections that it names. With `include_main`, its `main` file is an entry
/// point too.
fn resolve_local_manifest(path: &str, root: &Path, include_main: bool, result: &mut PluginResult) {
    let Some(relative) = workspace_relative(path, root) else {
        return;
    };
    let manifest_path = root.join(&relative).join("package.json");
    if !config_parser::is_inside_root(&manifest_path, root) {
        return;
    }
    let Some(manifest) = std::fs::read_to_string(&manifest_path)
        .ok()
        .and_then(|source| serde_json::from_str::<serde_json::Value>(&source).ok())
    else {
        return;
    };

    if include_main
        && let Some(main) = manifest.get("main").and_then(serde_json::Value::as_str)
        && let Some(main) = config_parser::normalize_config_path(main, &manifest_path, root)
    {
        push_module_path(main, result);
    }

    for key in COLLECTION_KEYS {
        let Some(collection) = manifest.get(*key).and_then(serde_json::Value::as_str) else {
            continue;
        };
        let Some(collection) =
            config_parser::normalize_config_path_buf(collection, &manifest_path, root)
        else {
            continue;
        };
        let collection_path = root.join(collection);
        if !config_parser::is_inside_root(&collection_path, root) {
            continue;
        }
        if let Ok(source) = std::fs::read_to_string(&collection_path) {
            resolve_collection(&collection_path, &source, root, result);
        }
    }
}

/// Register a module path that Nx loads with `require`. Nx does not expand
/// globs, so a value with glob syntax names no file and must not become a
/// pattern that makes many files entry points.
fn push_module_path(path: String, result: &mut PluginResult) {
    if !super::module_request::has_glob_syntax(&path) {
        result.push_entry_path(path);
    }
}

/// Nx resolves a local plugin or executor path from the workspace root.
fn workspace_relative(path: &str, root: &Path) -> Option<String> {
    config_parser::normalize_filesystem_config_path(path, &root.join("nx.json"), root)
}

/// The path of a workspace-local plugin or executor reference, which Nx marks
/// with a leading `./` or `/`.
fn local_reference_path(reference: &str) -> Option<&str> {
    (reference.starts_with("./") || reference.starts_with('/')).then_some(reference)
}

/// Resolve an Nx plugin, executor, or task-runner reference to its npm package.
///
/// Executors are `<package>:<executor>` and a plugin may name a subpath such as
/// `@nx/vite/plugin`. A workspace-local plugin is addressed by path, which is a
/// file rather than a dependency.
fn nx_reference_package(reference: &str) -> Option<String> {
    if reference.starts_with("./") || reference.starts_with('/') {
        return None;
    }
    let package = reference.split(':').next()?;
    if package.is_empty() {
        return None;
    }
    Some(crate::resolve::extract_package_name(package))
}

/// Expand Nx workspace tokens in a path string.
///
/// - `{projectRoot}` → the project's root directory relative to the workspace root
/// - `{workspaceRoot}` → empty string (paths are already resolved from workspace root)
///
/// See: <https://nx.dev/concepts/how-caching-works#runtime-hash-inputs>
fn expand_nx_tokens(path: &str, project_root_rel: &str) -> String {
    if !path.contains('{') {
        return path.to_string();
    }
    let result = if project_root_rel.is_empty() {
        path.replace("{projectRoot}/", "")
            .replace("{projectRoot}", "")
    } else {
        path.replace("{projectRoot}", project_root_rel)
    };
    result
        .replace("{workspaceRoot}/", "")
        .replace("{workspaceRoot}", "")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn has_entry_pattern(result: &PluginResult, pattern: &str) -> bool {
        result
            .entry_patterns
            .iter()
            .any(|entry_pattern| entry_pattern.pattern == pattern)
    }

    #[test]
    fn resolve_config_extracts_executor() {
        let source = r#"{
            "targets": {
                "build": {
                    "executor": "@angular/build:application"
                },
                "test": {
                    "executor": "@nx/vite:test"
                }
            }
        }"#;
        let plugin = NxPlugin;
        let result =
            plugin.resolve_config(Path::new("project.json"), source, Path::new("/project"));
        assert!(
            result
                .referenced_dependencies
                .contains(&"@angular/build".to_string())
        );
        assert!(
            result
                .referenced_dependencies
                .contains(&"@nx/vite".to_string())
        );
    }

    #[test]
    fn resolve_config_extracts_main() {
        let source = r#"{
            "targets": {
                "build": {
                    "executor": "@angular/build:application",
                    "options": {
                        "main": "apps/client/src/main.ts"
                    }
                }
            }
        }"#;
        let plugin = NxPlugin;
        let result =
            plugin.resolve_config(Path::new("project.json"), source, Path::new("/project"));
        assert!(has_entry_pattern(&result, "apps/client/src/main.ts"));
    }

    #[test]
    fn resolve_config_extracts_browser_as_entry() {
        let source = r#"{
            "targets": {
                "build": {
                    "executor": "@angular/build:application",
                    "options": {
                        "browser": "apps/client/src/main.ts"
                    }
                }
            }
        }"#;
        let plugin = NxPlugin;
        let result =
            plugin.resolve_config(Path::new("project.json"), source, Path::new("/project"));
        assert!(has_entry_pattern(&result, "apps/client/src/main.ts"));
    }

    #[test]
    fn resolve_config_extracts_styles_as_entry() {
        let source = r#"{
            "targets": {
                "build": {
                    "executor": "@angular/build:application",
                    "options": {
                        "styles": [
                            "src/styles.scss",
                            "apps/client/src/theme.css"
                        ]
                    }
                }
            }
        }"#;
        let plugin = NxPlugin;
        let result =
            plugin.resolve_config(Path::new("project.json"), source, Path::new("/project"));
        assert!(has_entry_pattern(&result, "src/styles.scss"));
        assert!(has_entry_pattern(&result, "apps/client/src/theme.css"));
    }

    #[test]
    fn resolve_config_extracts_styles_object_form() {
        let source = r#"{
            "targets": {
                "build": {
                    "executor": "@angular/build:application",
                    "options": {
                        "styles": [
                            "src/styles.scss",
                            { "input": "src/theme.scss", "bundleName": "theme", "inject": false }
                        ]
                    }
                }
            }
        }"#;
        let plugin = NxPlugin;
        let result =
            plugin.resolve_config(Path::new("project.json"), source, Path::new("/project"));
        assert!(has_entry_pattern(&result, "src/styles.scss"));
        assert!(
            has_entry_pattern(&result, "src/theme.scss"),
            "object-form entry `input` must be extracted as entry pattern"
        );
    }

    #[test]
    fn resolve_config_extracts_scripts_as_entry() {
        let source = r#"{
            "targets": {
                "build": {
                    "executor": "@angular/build:application",
                    "options": {
                        "scripts": ["src/analytics.ts"]
                    }
                }
            }
        }"#;
        let plugin = NxPlugin;
        let result =
            plugin.resolve_config(Path::new("project.json"), source, Path::new("/project"));
        assert!(has_entry_pattern(&result, "src/analytics.ts"));
    }

    #[test]
    fn resolve_config_expands_project_root_in_styles() {
        let source = r#"{
            "targets": {
                "build": {
                    "options": {
                        "styles": ["{projectRoot}/src/styles.scss"]
                    }
                }
            }
        }"#;
        let plugin = NxPlugin;
        let result = plugin.resolve_config(
            Path::new("/repo/apps/client/project.json"),
            source,
            Path::new("/repo"),
        );
        assert!(has_entry_pattern(&result, "apps/client/src/styles.scss"));
    }

    #[test]
    fn resolve_config_extracts_tsconfig() {
        let source = r#"{
            "targets": {
                "build": {
                    "executor": "@angular/build:application",
                    "options": {
                        "tsConfig": "apps/client/tsconfig.app.json"
                    }
                }
            }
        }"#;
        let plugin = NxPlugin;
        let result =
            plugin.resolve_config(Path::new("project.json"), source, Path::new("/project"));
        assert!(
            result
                .always_used_files
                .contains(&"apps/client/tsconfig.app.json".to_string())
        );
    }

    #[test]
    fn resolve_config_extracts_scss_include_paths() {
        let tmp = tempfile::tempdir().expect("create temp dir");
        let root = tmp.path();
        std::fs::create_dir_all(root.join("libs/shared/scss")).unwrap();

        let source = r#"{
            "targets": {
                "build": {
                    "executor": "@angular/build:application",
                    "options": {
                        "stylePreprocessorOptions": {
                            "includePaths": ["libs/shared/scss", "missing/dir"]
                        }
                    }
                }
            }
        }"#;
        let plugin = NxPlugin;
        let result = plugin.resolve_config(Path::new("project.json"), source, root);
        assert_eq!(result.scss_include_paths.len(), 1);
        assert_eq!(result.scss_include_paths[0], root.join("libs/shared/scss"));
    }

    #[test]
    fn resolve_config_empty_targets() {
        let source = r#"{ "targets": {} }"#;
        let plugin = NxPlugin;
        let result =
            plugin.resolve_config(Path::new("project.json"), source, Path::new("/project"));
        assert!(result.referenced_dependencies.is_empty());
        assert!(result.entry_patterns.is_empty());
    }

    #[test]
    fn resolve_config_expands_project_root_in_main() {
        let source = r#"{
            "targets": {
                "build": {
                    "executor": "@angular/build:application",
                    "options": {
                        "main": "{projectRoot}/src/main.ts"
                    }
                }
            }
        }"#;
        let plugin = NxPlugin;
        let result = plugin.resolve_config(
            Path::new("/workspace/apps/myapp/project.json"),
            source,
            Path::new("/workspace"),
        );
        assert!(has_entry_pattern(&result, "apps/myapp/src/main.ts"));
    }

    #[test]
    fn resolve_config_expands_project_root_in_tsconfig() {
        let source = r#"{
            "targets": {
                "build": {
                    "executor": "@angular/build:application",
                    "options": {
                        "tsConfig": "{projectRoot}/tsconfig.app.json"
                    }
                }
            }
        }"#;
        let plugin = NxPlugin;
        let result = plugin.resolve_config(
            Path::new("/workspace/apps/myapp/project.json"),
            source,
            Path::new("/workspace"),
        );
        assert!(
            result
                .always_used_files
                .contains(&"apps/myapp/tsconfig.app.json".to_string())
        );
    }

    #[test]
    fn resolve_config_expands_project_root_token() {
        let tmp = tempfile::tempdir().expect("create temp dir");
        let root = tmp.path();
        std::fs::create_dir_all(root.join("src/style-paths")).unwrap();

        let source = r#"{
            "targets": {
                "build": {
                    "executor": "@angular/build:application",
                    "options": {
                        "stylePreprocessorOptions": {
                            "includePaths": ["{projectRoot}/src/style-paths"]
                        }
                    }
                }
            }
        }"#;
        let plugin = NxPlugin;
        let result = plugin.resolve_config(root.join("project.json").as_path(), source, root);
        assert_eq!(result.scss_include_paths.len(), 1);
        assert_eq!(result.scss_include_paths[0], root.join("src/style-paths"));
    }

    #[test]
    fn resolve_config_expands_project_root_token_in_subproject() {
        let tmp = tempfile::tempdir().expect("create temp dir");
        let root = tmp.path();
        std::fs::create_dir_all(root.join("apps/myapp/src/styles")).unwrap();

        let source = r#"{
            "targets": {
                "build": {
                    "executor": "@angular/build:application",
                    "options": {
                        "stylePreprocessorOptions": {
                            "includePaths": ["{projectRoot}/src/styles"]
                        }
                    }
                }
            }
        }"#;
        let plugin = NxPlugin;
        let config_path = root.join("apps/myapp/project.json");
        let result = plugin.resolve_config(config_path.as_path(), source, root);
        assert_eq!(result.scss_include_paths.len(), 1);
        assert_eq!(
            result.scss_include_paths[0],
            root.join("apps/myapp/src/styles")
        );
    }

    #[test]
    fn resolve_config_expands_workspace_root_token() {
        let tmp = tempfile::tempdir().expect("create temp dir");
        let root = tmp.path();
        std::fs::create_dir_all(root.join("shared/styles")).unwrap();

        let source = r#"{
            "targets": {
                "build": {
                    "executor": "@angular/build:application",
                    "options": {
                        "stylePreprocessorOptions": {
                            "includePaths": ["{workspaceRoot}/shared/styles"]
                        }
                    }
                }
            }
        }"#;
        let plugin = NxPlugin;
        let result = plugin.resolve_config(root.join("project.json").as_path(), source, root);
        assert_eq!(result.scss_include_paths.len(), 1);
        assert_eq!(result.scss_include_paths[0], root.join("shared/styles"));
    }

    #[test]
    fn expand_nx_tokens_no_braces_unchanged() {
        assert_eq!(expand_nx_tokens("src/styles", "apps/myapp"), "src/styles");
    }

    #[test]
    fn expand_nx_tokens_project_root_replaced() {
        assert_eq!(
            expand_nx_tokens("{projectRoot}/src/styles", "apps/myapp"),
            "apps/myapp/src/styles"
        );
    }

    #[test]
    fn expand_nx_tokens_workspace_root_replaced() {
        assert_eq!(
            expand_nx_tokens("{workspaceRoot}/shared/styles", ""),
            "shared/styles"
        );
    }

    #[test]
    fn expand_nx_tokens_empty_project_root() {
        assert_eq!(
            expand_nx_tokens("{projectRoot}/src/styles", ""),
            "src/styles"
        );
    }

    #[test]
    fn resolve_nx_json_credits_plugins_target_defaults_and_runner() {
        let source = r#"{
            "plugins": [
                "nx-stylelint",
                { "plugin": "@monodon/rust/plugin", "options": {} },
                "./tools/local-plugin"
            ],
            "targetDefaults": {
                "@jscutlery/semver:version": { "cache": true },
                "build": { "executor": "@nx/vite:build" }
            },
            "tasksRunnerOptions": {
                "default": { "runner": "nx-cloud" }
            }
        }"#;
        let result = NxPlugin.resolve_config(Path::new("nx.json"), source, Path::new("/project"));
        let deps = &result.referenced_dependencies;
        assert!(deps.contains(&"nx-stylelint".to_string()), "{deps:?}");
        assert!(
            deps.contains(&"@monodon/rust".to_string()),
            "a plugin subpath must credit its scoped package, not the subpath: {deps:?}"
        );
        assert!(deps.contains(&"@jscutlery/semver".to_string()), "{deps:?}");
        assert!(deps.contains(&"@nx/vite".to_string()), "{deps:?}");
        assert!(deps.contains(&"nx-cloud".to_string()), "{deps:?}");
    }

    #[test]
    fn resolve_nx_json_skips_local_paths_and_plain_target_names() {
        let source = r#"{
            "plugins": ["./tools/local-plugin", "/abs/plugin"],
            "targetDefaults": {
                "build": { "cache": true },
                "e2e": { "executor": "./tools/executors/e2e:run" }
            }
        }"#;
        let result = NxPlugin.resolve_config(Path::new("nx.json"), source, Path::new("/project"));
        assert!(
            result.referenced_dependencies.is_empty(),
            "workspace-local paths and plain target names are not packages: {:?}",
            result.referenced_dependencies
        );
    }

    #[test]
    fn resolve_config_project_json_ignores_nx_json_keys() {
        let source = r#"{
            "plugins": ["nx-stylelint"],
            "tasksRunnerOptions": { "default": { "runner": "nx-cloud" } },
            "targets": { "build": { "executor": "@nx/vite:build" } }
        }"#;
        let result = NxPlugin.resolve_config(
            Path::new("apps/app/project.json"),
            source,
            Path::new("/project"),
        );
        let deps = &result.referenced_dependencies;
        assert!(deps.contains(&"@nx/vite".to_string()), "{deps:?}");
        assert!(
            !deps.contains(&"nx-stylelint".to_string()),
            "nx.json-only keys must not be read from project.json: {deps:?}"
        );
        assert!(!deps.contains(&"nx-cloud".to_string()), "{deps:?}");
    }

    fn write(root: &Path, path: &str, contents: &str) {
        let path = root.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, contents).unwrap();
    }

    #[test]
    fn project_json_local_executor_reads_plugin_collections() {
        let tmp = tempfile::tempdir().expect("create temp dir");
        let root = tmp.path();
        write(
            root,
            "tools/local-executors/package.json",
            r#"{"builders":"./custom-builders.json"}"#,
        );
        write(
            root,
            "tools/local-executors/custom-builders.json",
            r#"{"builders":{"e2e":{"implementation":"./impl/e2e.ts#runE2e","schema":"./impl/schema.json"}}}"#,
        );
        let source = r#"{"targets":{"e2e":{"executor":"./tools/local-executors:e2e"}}}"#;
        let result = NxPlugin.resolve_config(&root.join("apps/web/project.json"), source, root);
        assert!(
            has_entry_pattern(&result, "tools/local-executors/impl/e2e.ts"),
            "{:?}",
            result.entry_patterns
        );
        assert!(
            result
                .always_used_files
                .contains(&"tools/local-executors/impl/schema.json".to_string()),
            "{:?}",
            result.always_used_files
        );
        assert!(
            result.referenced_dependencies.is_empty(),
            "a local executor path is not a package: {:?}",
            result.referenced_dependencies
        );
    }

    #[test]
    fn collection_credits_modules_and_schemas_relative_to_the_file() {
        let source = r#"{
            "executors": {
                "build": {
                    "implementation": "./src/executors/build/executor#buildExecutor",
                    "batchImplementation": "./src/executors/build/batch",
                    "schema": "./src/executors/build/schema.json"
                },
                "alias": "@nx/js:tsc"
            },
            "generators": {
                "lib": { "factory": "./src/generators/lib/generator", "schema": "./src/generators/lib/schema.json" },
                "app": { "implementation": "./src/generators/app/generator.ts", "schema": "./schema.json" }
            },
            "schematics": {
                "old": { "factory": "./src/schematics/old/index", "schema": "./old.json" }
            }
        }"#;
        let result = NxPlugin.resolve_config(
            Path::new("/repo/tools/plugin/generators.json"),
            source,
            Path::new("/repo"),
        );
        for pattern in [
            "tools/plugin/src/executors/build/executor",
            "tools/plugin/src/executors/build/batch",
            "tools/plugin/src/generators/lib/generator",
            "tools/plugin/src/generators/app/generator.ts",
            "tools/plugin/src/schematics/old/index",
        ] {
            assert!(
                has_entry_pattern(&result, pattern),
                "{pattern}: {:?}",
                result.entry_patterns
            );
        }
        assert!(
            result
                .always_used_files
                .contains(&"tools/plugin/src/executors/build/schema.json".to_string()),
            "{:?}",
            result.always_used_files
        );
    }

    #[test]
    fn collection_drops_paths_outside_the_root() {
        let source = r#"{"executors":{"x":{"implementation":"../../../outside/impl","schema":"../../../outside/schema.json"}}}"#;
        let result = NxPlugin.resolve_config(
            Path::new("/repo/tools/plugin/executors.json"),
            source,
            Path::new("/repo"),
        );
        assert!(
            result.entry_patterns.is_empty(),
            "{:?}",
            result.entry_patterns
        );
        assert!(
            result.always_used_files.is_empty(),
            "{:?}",
            result.always_used_files
        );
    }

    #[test]
    fn nx_json_local_plugin_paths_are_entry_points() {
        let source = r#"{
            "plugins": [
                "./tools/file-plugin.ts",
                { "plugin": "./tools/dir-plugin", "options": {} },
                "@nx/vite/plugin"
            ]
        }"#;
        let result =
            NxPlugin.resolve_config(Path::new("/repo/nx.json"), source, Path::new("/repo"));
        assert!(
            has_entry_pattern(&result, "tools/file-plugin.ts"),
            "{:?}",
            result.entry_patterns
        );
        assert!(
            has_entry_pattern(
                &result,
                "tools/dir-plugin/index.{ts,tsx,mts,cts,gts,js,jsx,mjs,cjs,gjs,vue,svelte,astro,mdx}"
            ),
            "{:?}",
            result.entry_patterns
        );
        assert_eq!(result.referenced_dependencies, vec!["@nx/vite".to_string()]);
    }

    #[test]
    fn collection_skips_glob_values_and_empty_export_names() {
        let source = r#"{"executors":{
            "all": {"implementation": "./**"},
            "dynamic": {"implementation": "./src/[id]/executor"},
            "empty": {"implementation": "./src/empty/executor#"}
        }}"#;
        let result = NxPlugin.resolve_config(
            Path::new("/repo/tools/plugin/executors.json"),
            source,
            Path::new("/repo"),
        );
        assert!(
            result
                .entry_patterns
                .iter()
                .all(|rule| !rule.pattern.contains("**") && !rule.pattern.contains("[id]")),
            "a glob value names no module: {:?}",
            result.entry_patterns
        );
        assert!(
            has_entry_pattern(&result, "tools/plugin/src/empty/executor"),
            "{:?}",
            result.entry_patterns
        );
    }
}
