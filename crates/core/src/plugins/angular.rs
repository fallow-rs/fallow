//! Angular framework plugin.
//!
//! Detects Angular projects and marks component, module, service, guard,
//! pipe, directive, resolver, and interceptor files as entry points.
//! Parses `angular.json` to extract styles, scripts, main, and polyfills
//! from build targets as additional entry points. For ng-packagr library
//! packages, parses `ng-package.json` / `ng-package.prod.json` and treats
//! `lib.entryFile` (default `src/public_api.ts`) as the package public API
//! entry point so the library surface stays reachable.

/// ng-packagr's documented default `lib.entryFile` when the field is omitted.
/// Matches ng-packagr's `ng-package.schema.json` (`"default": "src/public_api.ts"`,
/// underscore); modern libraries scaffolded with a hyphenated `public-api.ts` set
/// `entryFile` explicitly, so the default only applies to configs that omit it.
const NG_PACKAGE_DEFAULT_ENTRY_FILE: &str = "src/public_api.ts";

use std::path::Path;

use super::config_parser;
use super::{Plugin, PluginResult};

define_plugin!(
    struct AngularPlugin => "angular",
    enablers: &["@angular/core", "ng-packagr"],
    entry_patterns: &[
        "src/main.ts",
        "src/app/**/*.component.ts",
        "src/app/**/*.module.ts",
        "src/app/**/*.service.ts",
        "src/app/**/*.guard.ts",
        "src/app/**/*.pipe.ts",
        "src/app/**/*.directive.ts",
        "src/app/**/*.resolver.ts",
        "src/app/**/*.interceptor.ts",
        "**/src/main.ts",
        "**/src/app/**/*.component.ts",
        "**/src/app/**/*.module.ts",
        "**/src/app/**/*.service.ts",
        "**/src/app/**/*.guard.ts",
        "**/src/app/**/*.pipe.ts",
        "**/src/app/**/*.directive.ts",
        "**/src/app/**/*.resolver.ts",
        "**/src/app/**/*.interceptor.ts",
    ],
    config_patterns: &[
        "angular.json",
        ".angular.json",
        "ng-package.json",
        "ng-package.prod.json",
    ],
    always_used: &[
        "angular.json",
        ".angular.json",
        "src/polyfills.ts",
        "src/environments/**/*.ts",
        "src/app/app.config.ts",
        "src/app/app.config.server.ts",
    ],
    tooling_dependencies: &[
        "@angular/cli",
        "@angular-devkit/build-angular",
        "@angular/compiler-cli",
        "@angular/compiler",
        "@angular/build",
        "ng-packagr",
        "zone.js",
        "tslib",
        "rxjs",
        "@angular/common",
        "@angular/platform-browser",
        "@angular/platform-browser-dynamic",
    ],
    resolve_config(config_path, source, _root) {
        let mut result = PluginResult::default();

        if is_ng_package_config(config_path) {
            for entry in resolve_ng_package_entries(config_path, source, _root) {
                result.push_entry_pattern(entry);
            }
            return result;
        }

        let styles = config_parser::extract_config_object_nested_string_or_array(
            source,
            config_path,
            &["projects"],
            &["architect", "build", "options", "styles"],
        );
        for style in &styles {
            let path = style.trim_start_matches("./");
            result.push_entry_pattern(path.to_string());
        }

        let scripts = config_parser::extract_config_object_nested_string_or_array(
            source,
            config_path,
            &["projects"],
            &["architect", "build", "options", "scripts"],
        );
        for script in &scripts {
            let path = script.trim_start_matches("./");
            result.push_entry_pattern(path.to_string());
        }

        for field in &["main", "browser"] {
            let mains = config_parser::extract_config_object_nested_strings(
                source,
                config_path,
                &["projects"],
                &["architect", "build", "options", field],
            );
            for main in &mains {
                let path = main.trim_start_matches("./");
                result.push_entry_pattern(path.to_string());
            }
        }

        let polyfills = config_parser::extract_config_object_nested_string_or_array(
            source,
            config_path,
            &["projects"],
            &["architect", "build", "options", "polyfills"],
        );
        for polyfill in &polyfills {
            let trimmed = polyfill.trim_start_matches("./");
            if trimmed.contains('/') {
                result.push_entry_pattern(trimmed.to_string());
            }
        }

        let test_mains = config_parser::extract_config_object_nested_strings(
            source,
            config_path,
            &["projects"],
            &["architect", "test", "options", "main"],
        );
        for main in &test_mains {
            let path = main.trim_start_matches("./");
            result.push_entry_pattern(path.to_string());
        }

        let include_paths = config_parser::extract_config_object_nested_string_or_array(
            source,
            config_path,
            &["projects"],
            &[
                "architect",
                "build",
                "options",
                "stylePreprocessorOptions",
                "includePaths",
            ],
        );
        result
            .scss_include_paths
            .extend(resolve_scss_include_paths(&include_paths, _root));

        result
    },
);

/// Resolve each SCSS include path entry to an absolute directory.
///
/// Skips entries whose resolved directory does not exist on disk — a missing
/// include path cannot resolve anything and would only waste syscalls during
/// SCSS resolution.
fn resolve_scss_include_paths(entries: &[String], root: &Path) -> Vec<std::path::PathBuf> {
    entries
        .iter()
        .map(|entry| root.join(entry.trim_start_matches("./")))
        .filter(|path| path.is_dir())
        .collect()
}

/// Maximum subdirectory depth scanned for nested (secondary-entry-point)
/// ng-package configs, relative to the primary config's directory. ng-packagr
/// secondary entry points sit a level or two below the primary; the cap bounds
/// the walk on pathological trees.
const NG_PACKAGE_SCAN_MAX_DEPTH: usize = 6;

/// Directory names never traversed when scanning for nested ng-package configs.
const NG_PACKAGE_SCAN_SKIP_DIRS: &[&str] = &["node_modules", "dist", "out", "tmp", "coverage"];

/// Whether `name` is an ng-packagr library-descriptor file name.
fn is_ng_package_file_name(name: &str) -> bool {
    name == "ng-package.json" || name == "ng-package.prod.json"
}

/// Whether `config_path` is an ng-packagr library descriptor.
fn is_ng_package_config(config_path: &Path) -> bool {
    config_path
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(is_ng_package_file_name)
}

/// Resolve every ng-packagr entry file reachable from a primary ng-package
/// config: the primary `lib.entryFile` plus each nested secondary-entry-point
/// config in the package subtree. Each entry file is resolved relative to the
/// directory of the config that declares it, matching ng-packagr's per-entry
/// semantics. Returns deduped project-relative entry patterns (workspace runs
/// have them prefixed back by the registry).
fn resolve_ng_package_entries(config_path: &Path, source: &str, root: &Path) -> Vec<String> {
    let mut entries = Vec::new();
    if let Some(entry) = resolve_ng_package_entry_from_source(config_path, source, root) {
        entries.push(entry);
    }

    if let Some(base) = config_path.parent() {
        let mut nested = Vec::new();
        collect_nested_ng_package_configs(base, 0, &mut nested);
        for nested_path in nested {
            let Some(nested_source) =
                crate::discover::project_read::read_project_file(root, &nested_path)
            else {
                continue;
            };
            if let Some(entry) =
                resolve_ng_package_entry_from_source(&nested_path, &nested_source, root)
            {
                entries.push(entry);
            }
        }
    }

    entries.sort();
    entries.dedup();
    entries
}

/// Resolve a single ng-package config's `lib.entryFile` to a project-relative
/// entry pattern, falling back to ng-packagr's documented default
/// (`src/public_api.ts`) when omitted or empty. Returns `None` only when
/// normalization escapes the project root.
fn resolve_ng_package_entry_from_source(
    config_path: &Path,
    source: &str,
    root: &Path,
) -> Option<String> {
    let entry_file =
        config_parser::extract_config_string(source, config_path, &["lib", "entryFile"])
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| NG_PACKAGE_DEFAULT_ENTRY_FILE.to_string());

    config_parser::normalize_config_path(&entry_file, config_path, root)
}

/// Recursively collect nested ng-package config paths in subdirectories of
/// `dir`, bounded by depth and a skip-dir list. Configs in `dir` itself
/// (depth 0) are intentionally not collected: they are either the primary
/// config or same-directory siblings that config discovery already surfaces.
fn collect_nested_ng_package_configs(
    dir: &Path,
    depth: usize,
    found: &mut Vec<std::path::PathBuf>,
) {
    if depth > NG_PACKAGE_SCAN_MAX_DEPTH {
        return;
    }
    let Ok(read_dir) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in read_dir.flatten() {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        let path = entry.path();
        if file_type.is_dir() {
            let skip = path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| {
                    name.starts_with('.') || NG_PACKAGE_SCAN_SKIP_DIRS.contains(&name)
                });
            if !skip {
                collect_nested_ng_package_configs(&path, depth + 1, found);
            }
        } else if depth >= 1
            && path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(is_ng_package_file_name)
        {
            found.push(path);
        }
    }
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
    fn resolve_config_extracts_styles() {
        let source = r#"{
            "projects": {
                "my-app": {
                    "architect": {
                        "build": {
                            "options": {
                                "styles": ["src/styles.css", "src/theme.scss"]
                            }
                        }
                    }
                }
            }
        }"#;
        let plugin = AngularPlugin;
        let result =
            plugin.resolve_config(Path::new("angular.json"), source, Path::new("/project"));
        assert!(has_entry_pattern(&result, "src/styles.css"));
        assert!(has_entry_pattern(&result, "src/theme.scss"));
    }

    #[test]
    fn resolve_config_extracts_styles_object_form() {
        let source = r#"{
            "projects": {
                "my-app": {
                    "architect": {
                        "build": {
                            "options": {
                                "styles": [
                                    "src/styles.scss",
                                    { "input": "src/theme.scss", "bundleName": "theme", "inject": false }
                                ]
                            }
                        }
                    }
                }
            }
        }"#;
        let plugin = AngularPlugin;
        let result =
            plugin.resolve_config(Path::new("angular.json"), source, Path::new("/project"));
        assert!(has_entry_pattern(&result, "src/styles.scss"));
        assert!(
            has_entry_pattern(&result, "src/theme.scss"),
            "object-form entry `input` must be extracted as entry pattern"
        );
    }

    #[test]
    fn resolve_config_extracts_main() {
        let source = r#"{
            "projects": {
                "my-app": {
                    "architect": {
                        "build": {
                            "options": {
                                "main": "src/main.ts"
                            }
                        }
                    }
                }
            }
        }"#;
        let plugin = AngularPlugin;
        let result =
            plugin.resolve_config(Path::new("angular.json"), source, Path::new("/project"));
        assert!(has_entry_pattern(&result, "src/main.ts"));
    }

    #[test]
    fn resolve_config_extracts_scripts() {
        let source = r#"{
            "projects": {
                "my-app": {
                    "architect": {
                        "build": {
                            "options": {
                                "scripts": ["node_modules/some-lib/dist/script.js"]
                            }
                        }
                    }
                }
            }
        }"#;
        let plugin = AngularPlugin;
        let result =
            plugin.resolve_config(Path::new("angular.json"), source, Path::new("/project"));
        assert!(has_entry_pattern(
            &result,
            "node_modules/some-lib/dist/script.js"
        ));
    }

    #[test]
    fn resolve_config_multiple_projects() {
        let source = r#"{
            "projects": {
                "app-one": {
                    "architect": {
                        "build": {
                            "options": {
                                "styles": ["apps/one/src/styles.css"],
                                "main": "apps/one/src/main.ts"
                            }
                        }
                    }
                },
                "app-two": {
                    "architect": {
                        "build": {
                            "options": {
                                "styles": ["apps/two/src/styles.css"],
                                "main": "apps/two/src/main.ts"
                            }
                        }
                    }
                }
            }
        }"#;
        let plugin = AngularPlugin;
        let result =
            plugin.resolve_config(Path::new("angular.json"), source, Path::new("/project"));
        assert!(has_entry_pattern(&result, "apps/one/src/styles.css"));
        assert!(has_entry_pattern(&result, "apps/two/src/styles.css"));
        assert!(has_entry_pattern(&result, "apps/one/src/main.ts"));
        assert!(has_entry_pattern(&result, "apps/two/src/main.ts"));
    }

    #[test]
    fn resolve_config_extracts_scss_include_paths() {
        let tmp = tempfile::tempdir().expect("create temp dir");
        let root = tmp.path();
        std::fs::create_dir_all(root.join("src/styles")).unwrap();
        std::fs::create_dir_all(root.join("libs/shared/scss")).unwrap();

        let source = r#"{
            "projects": {
                "my-app": {
                    "architect": {
                        "build": {
                            "options": {
                                "stylePreprocessorOptions": {
                                    "includePaths": ["src/styles", "./libs/shared/scss"]
                                }
                            }
                        }
                    }
                }
            }
        }"#;
        let plugin = AngularPlugin;
        let result = plugin.resolve_config(Path::new("angular.json"), source, root);
        assert_eq!(result.scss_include_paths.len(), 2);
        assert!(result.scss_include_paths.contains(&root.join("src/styles")));
        assert!(
            result
                .scss_include_paths
                .contains(&root.join("libs/shared/scss"))
        );
    }

    #[test]
    fn resolve_config_scss_include_paths_skips_missing_dirs() {
        let tmp = tempfile::tempdir().expect("create temp dir");
        let root = tmp.path();
        std::fs::create_dir_all(root.join("src/styles")).unwrap();

        let source = r#"{
            "projects": {
                "my-app": {
                    "architect": {
                        "build": {
                            "options": {
                                "stylePreprocessorOptions": {
                                    "includePaths": ["src/styles", "missing/dir"]
                                }
                            }
                        }
                    }
                }
            }
        }"#;
        let plugin = AngularPlugin;
        let result = plugin.resolve_config(Path::new("angular.json"), source, root);
        assert_eq!(result.scss_include_paths.len(), 1);
        assert_eq!(result.scss_include_paths[0], root.join("src/styles"));
    }

    #[test]
    fn resolve_config_ng_package_entry_file() {
        let source = r#"{
            "$schema": "./node_modules/ng-packagr/ng-package.schema.json",
            "dest": "./dist",
            "lib": {
                "entryFile": "src/public-api.ts"
            }
        }"#;
        let plugin = AngularPlugin;
        let result = plugin.resolve_config(
            Path::new("/project/ng-package.json"),
            source,
            Path::new("/project"),
        );
        assert!(has_entry_pattern(&result, "src/public-api.ts"));
    }

    #[test]
    fn resolve_config_ng_package_entry_file_nested_dir() {
        let source = r#"{
            "lib": { "entryFile": "src/public-api.ts" }
        }"#;
        let plugin = AngularPlugin;
        let result = plugin.resolve_config(
            Path::new("/repo/packages/angular/ng-package.json"),
            source,
            Path::new("/repo"),
        );
        assert!(has_entry_pattern(
            &result,
            "packages/angular/src/public-api.ts"
        ));
    }

    #[test]
    fn resolve_config_ng_package_entry_file_default_when_omitted() {
        let source = r#"{ "dest": "./dist", "lib": {} }"#;
        let plugin = AngularPlugin;
        let result = plugin.resolve_config(
            Path::new("/project/ng-package.json"),
            source,
            Path::new("/project"),
        );
        assert!(has_entry_pattern(&result, "src/public_api.ts"));
    }

    #[test]
    fn resolve_config_ng_package_default_when_lib_absent() {
        let source = r#"{ "$schema": "x", "dest": "./dist" }"#;
        let plugin = AngularPlugin;
        let result = plugin.resolve_config(
            Path::new("/project/ng-package.json"),
            source,
            Path::new("/project"),
        );
        assert!(has_entry_pattern(&result, "src/public_api.ts"));
    }

    #[test]
    fn resolve_config_ng_package_prod_variant() {
        let source = r#"{ "lib": { "entryFile": "src/prod-api.ts" } }"#;
        let plugin = AngularPlugin;
        let result = plugin.resolve_config(
            Path::new("/project/ng-package.prod.json"),
            source,
            Path::new("/project"),
        );
        assert!(has_entry_pattern(&result, "src/prod-api.ts"));
    }

    #[test]
    fn resolve_config_ng_package_empty_entry_file_uses_default() {
        let source = r#"{ "lib": { "entryFile": "" } }"#;
        let plugin = AngularPlugin;
        let result = plugin.resolve_config(
            Path::new("/project/ng-package.json"),
            source,
            Path::new("/project"),
        );
        assert!(has_entry_pattern(&result, "src/public_api.ts"));
    }

    #[test]
    fn resolve_config_ng_package_malformed_does_not_panic() {
        let source = "{ this is not valid json";
        let plugin = AngularPlugin;
        let result = plugin.resolve_config(
            Path::new("/project/ng-package.json"),
            source,
            Path::new("/project"),
        );
        assert!(has_entry_pattern(&result, "src/public_api.ts"));
    }

    #[test]
    fn resolve_config_ng_package_collects_nested_secondary_entries() {
        let tmp = tempfile::tempdir().expect("temp dir");
        let root = tmp.path();
        std::fs::create_dir_all(root.join("client")).unwrap();
        std::fs::create_dir_all(root.join("server")).unwrap();
        std::fs::write(
            root.join("ng-package.json"),
            r#"{ "lib": { "entryFile": "src/public-api.ts" } }"#,
        )
        .unwrap();
        std::fs::write(
            root.join("client/ng-package.json"),
            r#"{ "lib": { "entryFile": "src/public_api.ts" } }"#,
        )
        .unwrap();
        std::fs::write(root.join("server/ng-package.json"), r"{}").unwrap();

        let source = std::fs::read_to_string(root.join("ng-package.json")).unwrap();
        let plugin = AngularPlugin;
        let result = plugin.resolve_config(&root.join("ng-package.json"), &source, root);

        assert!(has_entry_pattern(&result, "src/public-api.ts"));
        assert!(has_entry_pattern(&result, "client/src/public_api.ts"));
        assert!(has_entry_pattern(&result, "server/src/public_api.ts"));
    }

    #[test]
    fn resolve_config_ng_package_skips_node_modules_nested_configs() {
        let tmp = tempfile::tempdir().expect("temp dir");
        let root = tmp.path();
        std::fs::create_dir_all(root.join("node_modules/some-lib")).unwrap();
        std::fs::write(
            root.join("ng-package.json"),
            r#"{ "lib": { "entryFile": "src/public-api.ts" } }"#,
        )
        .unwrap();
        std::fs::write(
            root.join("node_modules/some-lib/ng-package.json"),
            r#"{ "lib": { "entryFile": "src/leaked.ts" } }"#,
        )
        .unwrap();

        let source = std::fs::read_to_string(root.join("ng-package.json")).unwrap();
        let plugin = AngularPlugin;
        let result = plugin.resolve_config(&root.join("ng-package.json"), &source, root);

        assert!(has_entry_pattern(&result, "src/public-api.ts"));
        assert!(
            !has_entry_pattern(&result, "node_modules/some-lib/src/leaked.ts"),
            "node_modules configs must not be collected: {:?}",
            result.entry_patterns
        );
    }

    #[test]
    fn resolve_config_ng_package_same_dir_sibling_left_to_discovery() {
        let tmp = tempfile::tempdir().expect("temp dir");
        let root = tmp.path();
        std::fs::write(
            root.join("ng-package.json"),
            r#"{ "lib": { "entryFile": "src/public-api.ts" } }"#,
        )
        .unwrap();
        std::fs::write(
            root.join("ng-package.prod.json"),
            r#"{ "lib": { "entryFile": "src/prod-api.ts" } }"#,
        )
        .unwrap();

        let source = std::fs::read_to_string(root.join("ng-package.json")).unwrap();
        let plugin = AngularPlugin;
        let result = plugin.resolve_config(&root.join("ng-package.json"), &source, root);

        assert_eq!(result.entry_patterns.len(), 1);
        assert!(has_entry_pattern(&result, "src/public-api.ts"));
    }

    #[test]
    fn resolve_config_ng_package_does_not_run_angular_json_extractors() {
        let source = r#"{ "lib": { "entryFile": "src/public-api.ts" } }"#;
        let plugin = AngularPlugin;
        let result = plugin.resolve_config(
            Path::new("/project/ng-package.json"),
            source,
            Path::new("/project"),
        );
        assert_eq!(result.entry_patterns.len(), 1);
        assert!(has_entry_pattern(&result, "src/public-api.ts"));
    }

    #[test]
    fn resolve_config_polyfills_skips_packages() {
        let source = r#"{
            "projects": {
                "my-app": {
                    "architect": {
                        "build": {
                            "options": {
                                "polyfills": ["zone.js", "src/polyfills.ts"]
                            }
                        }
                    }
                }
            }
        }"#;
        let plugin = AngularPlugin;
        let result =
            plugin.resolve_config(Path::new("angular.json"), source, Path::new("/project"));
        assert!(!has_entry_pattern(&result, "zone.js"));
        assert!(has_entry_pattern(&result, "src/polyfills.ts"));
    }

    #[test]
    fn ng_packagr_is_enabler_and_tooling_dependency() {
        let plugin = AngularPlugin;
        assert!(plugin.enablers().contains(&"ng-packagr"));
        assert!(plugin.tooling_dependencies().contains(&"ng-packagr"));
    }
}
