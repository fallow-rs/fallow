//! Vitest test runner plugin.
//!
//! Detects Vitest projects and marks test/bench files as entry points.
//! Parses vitest.config to extract test.include, setupFiles, globalSetup,
//! and custom test environments as referenced dependencies.

use std::path::Path;

use super::config_parser;
use super::{Plugin, PluginResult};

pub struct VitestPlugin;

const ENABLERS: &[&str] = &["vitest"];

const ENTRY_PATTERNS: &[&str] = &[
    "**/*.test.{ts,tsx,js,jsx}",
    "**/*.spec.{ts,tsx,js,jsx}",
    "**/__tests__/**/*.{ts,tsx,js,jsx}",
    "**/*.bench.{ts,tsx,js,jsx}",
];

const CONFIG_PATTERNS: &[&str] = &[
    "**/vitest.config.{ts,js,mts,mjs}",
    "vitest.workspace.{ts,js}",
];

const ALWAYS_USED: &[&str] = &[
    "vitest.config.{ts,js,mts,mjs}",
    "vitest.setup.{ts,js}",
    "vitest.workspace.{ts,js}",
    // Common setupFiles conventions used by CRA, Vitest, and community projects.
    // These are often referenced via imported/spread base configs that static
    // analysis can't follow, so we mark them as always-used when Vitest is active.
    "**/src/setupTests.{ts,tsx,js,jsx}",
    "**/src/test-setup.{ts,tsx,js,jsx}",
];

const TOOLING_DEPENDENCIES: &[&str] = &["vitest"];
const CONFIG_EXPORTS: &[&str] = &["default"];

const FIXTURE_PATTERNS: &[&str] = &[
    "**/__fixtures__/**/*.{ts,tsx,js,jsx,json}",
    "**/fixtures/**/*.{ts,tsx,js,jsx,json}",
];

/// Package name suffixes that identify Vitest manual-mock virtual paths.
///
/// Vitest's manual-mock convention places mock factories at `<package>/__mocks__/<module>.ts`
/// and test setups sometimes import from `@<scope>/__mocks__` paths (via package.json `imports`
/// aliases or workspace virtual paths). These specifiers do not exist on npm and must not be
/// flagged as unlisted dependencies.
const VIRTUAL_PACKAGE_SUFFIXES: &[&str] = &["/__mocks__"];

/// Built-in Vitest reporter names that should not be treated as dependencies.
const BUILTIN_REPORTERS: &[&str] = &[
    "default",
    "verbose",
    "dot",
    "json",
    "tap",
    "tap-flat",
    "hanging-process",
    "github-actions",
    "blob",
    "basic",
    "junit",
    "html",
];

/// Vitest config filenames for file-based activation.
/// In monorepos, `vitest` may only be in some workspaces, but shared vite configs
/// embed vitest test configuration. Activate when these files exist.
const VITEST_CONFIG_FILES: &[&str] = &[
    "vitest.config.ts",
    "vitest.config.js",
    "vitest.config.mts",
    "vitest.config.mjs",
    "vite.config.ts",
    "vite.config.js",
    "vite.config.mts",
    "vite.config.mjs",
];

impl Plugin for VitestPlugin {
    fn name(&self) -> &'static str {
        "vitest"
    }

    fn enablers(&self) -> &'static [&'static str] {
        ENABLERS
    }

    /// Activate when `vitest` is in deps OR when a vitest/vite config file exists.
    /// Vitest often embeds its config in `vite.config.{ts,js}` via `defineConfig({ test: {...} })`,
    /// so the presence of a vite config in a workspace implies vitest may be used there.
    fn is_enabled_with_deps(&self, deps: &[String], root: &Path) -> bool {
        let enablers = self.enablers();
        if enablers.iter().any(|e| deps.iter().any(|d| d == e)) {
            return true;
        }
        VITEST_CONFIG_FILES.iter().any(|f| root.join(f).exists())
    }

    fn entry_patterns(&self) -> &'static [&'static str] {
        ENTRY_PATTERNS
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

    fn used_exports(&self) -> Vec<(&'static str, &'static [&'static str])> {
        vec![
            ("vitest.config.{ts,js,mts,mjs}", CONFIG_EXPORTS),
            ("vitest.workspace.{ts,js}", CONFIG_EXPORTS),
        ]
    }

    fn fixture_glob_patterns(&self) -> &'static [&'static str] {
        FIXTURE_PATTERNS
    }

    fn virtual_package_suffixes(&self) -> &'static [&'static str] {
        VIRTUAL_PACKAGE_SUFFIXES
    }

    fn resolve_config(&self, config_path: &Path, source: &str, root: &Path) -> PluginResult {
        let mut result = PluginResult::default();

        // Extract import sources as referenced dependencies
        let imports = config_parser::extract_imports(source, config_path);
        for imp in &imports {
            let dep = crate::resolve::extract_package_name(imp);
            result.referenced_dependencies.push(dep);
        }

        // test.alias → path aliases + mock-file crediting. Vitest merges test.alias
        // with Vite's resolve.alias when running tests, so imports that only resolve
        // through a test alias (virtual modules like `vscode`) and __mocks__ files
        // aliased to mock a real package must be made visible. Collect the top-level
        // map and every test.projects[*].test.alias map, then apply each via
        // process_test_alias (see its docs for the three mechanisms).
        let mut aliases =
            config_parser::extract_config_aliases(source, config_path, &["test", "alias"]);
        aliases.extend(config_parser::extract_config_array_nested_aliases(
            source,
            config_path,
            &["test", "projects"],
            &["test", "alias"],
        ));
        for (find, replacement) in aliases {
            process_test_alias(&mut result, &find, &replacement, config_path, root);
        }

        // test.include → entry patterns that replace defaults
        // Vitest treats root-level test.include as a full override of its default
        // patterns. Project-level includes (test.projects[*].test.include) only ADD
        // to the patterns since projects without test.include inherit root defaults.
        let root_includes =
            config_parser::extract_config_string_array(source, config_path, &["test", "include"]);
        if !root_includes.is_empty() {
            result.replace_entry_patterns = true;
        }
        result.extend_entry_patterns(root_includes);

        // Also check test.projects[*].test.include (Vitest projects/workspaces)
        let project_includes = config_parser::extract_config_array_nested_string_or_array(
            source,
            config_path,
            &["test", "projects"],
            &["test", "include"],
        );
        result.extend_entry_patterns(project_includes);

        // test.setupFiles → setup files (string or array)
        let mut setup_files = config_parser::extract_config_string_or_array(
            source,
            config_path,
            &["test", "setupFiles"],
        );
        // Also check test.projects[*].test.setupFiles (Vitest projects/workspaces)
        setup_files.extend(config_parser::extract_config_array_nested_string_or_array(
            source,
            config_path,
            &["test", "projects"],
            &["test", "setupFiles"],
        ));
        for f in &setup_files {
            result
                .setup_files
                .push(root.join(f.trim_start_matches("./")));
        }

        // test.globalSetup → setup files (string or array)
        let mut global_setup = config_parser::extract_config_string_or_array(
            source,
            config_path,
            &["test", "globalSetup"],
        );
        // Also check test.projects[*].test.globalSetup
        global_setup.extend(config_parser::extract_config_array_nested_string_or_array(
            source,
            config_path,
            &["test", "projects"],
            &["test", "globalSetup"],
        ));
        for f in &global_setup {
            result
                .setup_files
                .push(root.join(f.trim_start_matches("./")));
        }

        // test.environment → if custom, it's a referenced dependency
        // Vitest custom environments use the package name `vitest-environment-<name>`
        if let Some(env) =
            config_parser::extract_config_string(source, config_path, &["test", "environment"])
            && !matches!(env.as_str(), "node" | "jsdom" | "happy-dom")
        {
            result
                .referenced_dependencies
                .push(format!("vitest-environment-{env}"));
            result.referenced_dependencies.push(env);
        }

        // test.reporters → referenced dependencies (shallow to avoid options objects)
        // e.g. reporters: ["default", ["vitest-sonar-reporter", { outputFile: "..." }]]
        let reporters = config_parser::extract_config_nested_shallow_strings(
            source,
            config_path,
            &["test"],
            "reporters",
        );
        for reporter in &reporters {
            if !BUILTIN_REPORTERS.contains(&reporter.as_str()) {
                let dep = crate::resolve::extract_package_name(reporter);
                result.referenced_dependencies.push(dep);
            }
        }

        // test.coverage.provider → if not built-in, it's a referenced dependency
        // e.g. "istanbul" → @vitest/coverage-istanbul, "v8" → @vitest/coverage-v8
        if let Some(provider) = config_parser::extract_config_string(
            source,
            config_path,
            &["test", "coverage", "provider"],
        ) && !matches!(provider.as_str(), "v8" | "istanbul")
        {
            result
                .referenced_dependencies
                .push(format!("@vitest/coverage-{provider}"));
            result.referenced_dependencies.push(provider);
        }

        // test.typecheck.checker → if not built-in, it's a referenced dependency
        // e.g. "vue-tsc" → vue-tsc package
        if let Some(checker) = config_parser::extract_config_string(
            source,
            config_path,
            &["test", "typecheck", "checker"],
        ) && !matches!(checker.as_str(), "tsc")
        {
            result.referenced_dependencies.push(checker);
        }

        // test.browser.provider → if not built-in, it's a referenced dependency
        // "playwright" and "webdriverio" require @vitest/browser peer dependency
        if let Some(provider) = config_parser::extract_config_string(
            source,
            config_path,
            &["test", "browser", "provider"],
        ) && !matches!(provider.as_str(), "preview")
        {
            result
                .referenced_dependencies
                .push("@vitest/browser".to_string());
            result.referenced_dependencies.push(provider);
        }

        result
    }
}

/// Source-file extensions an alias replacement may name. A mock alias always
/// points at a JS/TS file; directory targets (`@/` -> `src`) have no extension
/// and are not seeded as entry points.
const ALIAS_SOURCE_EXTENSIONS: &[&str] = &["ts", "tsx", "js", "jsx", "mjs", "cjs", "mts", "cts"];

/// True when `spec` is a bare npm package specifier (not a relative path, URL,
/// `data:`, or `@/` / `~/` / `#` style path alias key).
fn is_bare_package_specifier(spec: &str) -> bool {
    crate::resolve::is_bare_specifier(spec)
        && crate::resolve::is_valid_package_name(spec)
        && !crate::resolve::is_path_alias(spec)
}

/// True when a normalized alias replacement names a local source file (by
/// extension), as opposed to a directory.
fn alias_target_is_source_file(normalized: &str) -> bool {
    Path::new(normalized)
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ALIAS_SOURCE_EXTENSIONS.contains(&ext))
}

/// Apply one `test.alias` entry to the plugin result.
///
/// Three mechanisms cooperate so both Vitest alias false-positive classes
/// disappear without introducing new ones:
/// - (A) push the alias into `path_aliases` so a virtual-module / alias-only
///   import (`vscode` -> `./mock/vscode.js`) resolves instead of surfacing as
///   `unresolved-import` / `unlisted-dependency`.
/// - (B) when the replacement names a local source FILE, seed it as a support
///   entry point so an aliased `__mocks__` file keeps its exports credited even
///   when the original package resolves through `node_modules` (in which case
///   the production import never reaches the path-alias fallback).
/// - (C) when the alias KEY is a bare package, credit it as a referenced
///   dependency so redirecting its import through the alias (only happens when
///   `node_modules` is absent) does not regress it into a false
///   `unused-dependency`.
///
/// Package-to-package aliases (`'lodash-es' -> 'lodash'`, where BOTH sides are
/// bare npm packages) are special-cased: the replacement is not a filesystem
/// path, so `normalize_config_path` would treat it as a local path and pushing a path alias
/// would turn the source import `Unresolvable` in a no-`node_modules` run.
/// Instead both package names are credited as referenced and no path alias is
/// emitted. A bare directory replacement (`'@/' -> 'src'`) is not affected
/// because the `@/` key is a path-alias key, not a bare package.
fn process_test_alias(
    result: &mut PluginResult,
    find: &str,
    replacement: &str,
    config_path: &Path,
    root: &Path,
) {
    let find_is_pkg = is_bare_package_specifier(find);

    if find_is_pkg && is_bare_package_specifier(replacement) {
        result
            .referenced_dependencies
            .push(crate::resolve::extract_package_name(replacement));
        result
            .referenced_dependencies
            .push(crate::resolve::extract_package_name(find));
        return;
    }

    let Some(normalized) = config_parser::normalize_config_path(replacement, config_path, root)
    else {
        return;
    };

    // (A)
    result
        .path_aliases
        .push((find.to_owned(), normalized.clone()));
    // (B)
    if alias_target_is_source_file(&normalized) {
        result.setup_files.push(root.join(&normalized));
    }
    // (C)
    if find_is_pkg {
        result
            .referenced_dependencies
            .push(crate::resolve::extract_package_name(find));
    }

    tracing::debug!(find, target = %normalized, "vitest test.alias extracted");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn resolve(source: &str) -> PluginResult {
        VitestPlugin.resolve_config(
            std::path::Path::new("vitest.config.ts"),
            source,
            std::path::Path::new("/project"),
        )
    }

    #[test]
    fn mocks_path_suffix_is_present() {
        let suffixes = VitestPlugin.virtual_package_suffixes();
        assert!(
            suffixes.contains(&"/__mocks__"),
            "VitestPlugin should declare /__mocks__ as a virtual package suffix"
        );
    }

    #[test]
    fn scoped_mocks_package_matches_suffix() {
        let suffixes = VitestPlugin.virtual_package_suffixes();
        let candidates = [
            "@aws-sdk/__mocks__",
            "@sentry/__mocks__",
            "@supabase/__mocks__",
            "@mapbox/__mocks__",
            "@ai-sdk/__mocks__",
            "some-pkg/__mocks__",
        ];
        for candidate in &candidates {
            assert!(
                suffixes.iter().any(|s| candidate.ends_with(s)),
                "{candidate} should be matched by a virtual package suffix"
            );
        }
    }

    #[test]
    fn non_mocks_package_does_not_match_suffix() {
        let suffixes = VitestPlugin.virtual_package_suffixes();
        // Includes adversarial cases that share the substring `__mocks__` but
        // don't end with `/__mocks__`, plus a package whose own name contains it.
        let non_mocks = [
            "@aws-sdk/client-s3",
            "vitest",
            "@vitest/coverage-v8",
            "__mocks__-helper",
            "my__mocks__pkg",
            "@scope/__mocks__-utils",
        ];
        for candidate in &non_mocks {
            assert!(
                !suffixes.iter().any(|s| candidate.ends_with(s)),
                "{candidate} should NOT be matched by a virtual package suffix"
            );
        }
    }

    #[test]
    fn reporters_string_array() {
        let source = r#"
            export default {
                test: {
                    reporters: ["default", "vitest-sonar-reporter"]
                }
            };
        "#;
        let result = resolve(source);
        assert!(
            result
                .referenced_dependencies
                .contains(&"vitest-sonar-reporter".to_string())
        );
    }

    #[test]
    fn reporters_tuple_format() {
        let source = r#"
            export default {
                test: {
                    reporters: ["default", ["vitest-sonar-reporter", { outputFile: "report.xml" }]]
                }
            };
        "#;
        let result = resolve(source);
        assert!(
            result
                .referenced_dependencies
                .contains(&"vitest-sonar-reporter".to_string())
        );
    }

    #[test]
    fn reporters_builtin_filtered() {
        let source = r#"
            export default {
                test: {
                    reporters: ["default", "verbose", "json", "junit", "html"]
                }
            };
        "#;
        let result = resolve(source);
        // No non-import deps should be added for built-in reporters
        let non_import_deps: Vec<_> = result
            .referenced_dependencies
            .iter()
            .filter(|d| !d.contains('/') || d.starts_with('@'))
            .collect();
        assert!(
            non_import_deps.is_empty(),
            "Built-in reporters should not be referenced dependencies: {non_import_deps:?}"
        );
    }

    #[test]
    fn reporters_scoped_package() {
        let source = r#"
            export default {
                test: {
                    reporters: ["@vitest/reporter-html"]
                }
            };
        "#;
        let result = resolve(source);
        assert!(
            result
                .referenced_dependencies
                .contains(&"@vitest/reporter-html".to_string())
        );
    }

    #[test]
    fn reporters_missing_does_not_error() {
        let source = r#"
            export default {
                test: {
                    include: ["**/*.test.ts"]
                }
            };
        "#;
        let result = resolve(source);
        // Should not panic or add unexpected deps
        assert!(result.referenced_dependencies.is_empty());
    }

    #[test]
    fn custom_environment() {
        let source = r#"
            export default {
                test: {
                    environment: "edge-runtime"
                }
            };
        "#;
        let result = resolve(source);
        assert!(
            result
                .referenced_dependencies
                .contains(&"vitest-environment-edge-runtime".to_string())
        );
        assert!(
            result
                .referenced_dependencies
                .contains(&"edge-runtime".to_string())
        );
    }

    #[test]
    fn coverage_provider_custom() {
        let source = r#"
            export default {
                test: {
                    coverage: {
                        provider: "custom-provider"
                    }
                }
            };
        "#;
        let result = resolve(source);
        assert!(
            result
                .referenced_dependencies
                .contains(&"@vitest/coverage-custom-provider".to_string())
        );
    }

    #[test]
    fn coverage_provider_builtin_filtered() {
        let source = r#"
            export default {
                test: {
                    coverage: {
                        provider: "v8"
                    }
                }
            };
        "#;
        let result = resolve(source);
        assert!(result.referenced_dependencies.is_empty());
    }

    #[test]
    fn coverage_provider_istanbul_builtin() {
        let source = r#"
            export default {
                test: {
                    coverage: {
                        provider: "istanbul"
                    }
                }
            };
        "#;
        let result = resolve(source);
        assert!(result.referenced_dependencies.is_empty());
    }

    #[test]
    fn typecheck_checker_vue_tsc() {
        let source = r#"
            export default {
                test: {
                    typecheck: {
                        checker: "vue-tsc"
                    }
                }
            };
        "#;
        let result = resolve(source);
        assert!(
            result
                .referenced_dependencies
                .contains(&"vue-tsc".to_string())
        );
    }

    #[test]
    fn typecheck_checker_tsc_builtin() {
        let source = r#"
            export default {
                test: {
                    typecheck: {
                        checker: "tsc"
                    }
                }
            };
        "#;
        let result = resolve(source);
        assert!(result.referenced_dependencies.is_empty());
    }

    #[test]
    fn browser_provider_playwright() {
        let source = r#"
            export default {
                test: {
                    browser: {
                        provider: "playwright"
                    }
                }
            };
        "#;
        let result = resolve(source);
        assert!(
            result
                .referenced_dependencies
                .contains(&"@vitest/browser".to_string())
        );
        assert!(
            result
                .referenced_dependencies
                .contains(&"playwright".to_string())
        );
    }

    #[test]
    fn browser_provider_preview_builtin() {
        let source = r#"
            export default {
                test: {
                    browser: {
                        provider: "preview"
                    }
                }
            };
        "#;
        let result = resolve(source);
        assert!(result.referenced_dependencies.is_empty());
    }

    #[test]
    fn test_include_sets_replace_entry_patterns() {
        let source = r#"
            export default {
                test: {
                    include: ["src/**/*.test.ts"]
                }
            };
        "#;
        let result = resolve(source);
        assert!(
            result.replace_entry_patterns,
            "test.include should trigger replacement of static entry patterns"
        );
        assert_eq!(result.entry_patterns, vec!["src/**/*.test.ts"]);
    }

    #[test]
    fn no_test_include_keeps_defaults() {
        let source = r#"
            export default {
                test: {
                    environment: "jsdom"
                }
            };
        "#;
        let result = resolve(source);
        assert!(
            !result.replace_entry_patterns,
            "without test.include, static patterns should be kept"
        );
        assert!(result.entry_patterns.is_empty());
    }

    #[test]
    fn project_level_include_does_not_replace_defaults() {
        let source = r#"
            export default {
                test: {
                    projects: [
                        {
                            test: {
                                name: "unit-jsdom",
                                include: ["packages/vue/**/*.spec.ts"],
                            }
                        }
                    ]
                }
            };
        "#;
        let result = resolve(source);
        assert!(
            !result.replace_entry_patterns,
            "project-level test.include should not replace static defaults"
        );
        assert_eq!(result.entry_patterns, vec!["packages/vue/**/*.spec.ts"]);
    }

    // test.alias resolution uses normalize_config_path, which strips the project
    // root prefix, so the config path must be ABSOLUTE for these tests (the
    // shared `resolve` helper passes a relative config path that normalizes to a
    // root-relative path that cannot strip an absolute root).
    fn resolve_abs(source: &str) -> PluginResult {
        VitestPlugin.resolve_config(
            std::path::Path::new("/project/vitest.config.ts"),
            source,
            std::path::Path::new("/project"),
        )
    }

    #[test]
    fn test_alias_object_form_virtual_module() {
        // Virtual module aliased to a local mock file: resolves + credits the mock.
        let source = r#"
            export default {
                test: {
                    alias: { vscode: "./test/mock/vscode.js" }
                }
            };
        "#;
        let result = resolve_abs(source);
        assert_eq!(
            result.path_aliases,
            vec![("vscode".to_string(), "test/mock/vscode.js".to_string())]
        );
        assert!(
            result
                .setup_files
                .contains(&std::path::PathBuf::from("/project/test/mock/vscode.js")),
            "local mock file should be seeded as a support entry point: {:?}",
            result.setup_files
        );
        assert!(
            result
                .referenced_dependencies
                .contains(&"vscode".to_string()),
            "bare-package alias key should be credited as referenced"
        );
    }

    #[test]
    fn test_alias_array_form_with_find_replacement() {
        let source = r#"
            export default {
                test: {
                    alias: [{ find: "vscode", replacement: "./test/mock/vscode.js" }]
                }
            };
        "#;
        let result = resolve_abs(source);
        assert_eq!(
            result.path_aliases,
            vec![("vscode".to_string(), "test/mock/vscode.js".to_string())]
        );
        assert!(
            result
                .setup_files
                .contains(&std::path::PathBuf::from("/project/test/mock/vscode.js"))
        );
    }

    #[test]
    fn test_alias_resolve_replacement_for_scoped_mock() {
        // The amplitude/wizard shape: a real scoped package aliased to a mock file
        // via resolve(__dirname, ...).
        let source = r#"
            import { resolve } from "node:path";
            export default {
                test: {
                    alias: {
                        "@scope/pkg": resolve(__dirname, "__mocks__/@scope/pkg.ts")
                    }
                }
            };
        "#;
        let result = resolve_abs(source);
        assert_eq!(
            result.path_aliases,
            vec![(
                "@scope/pkg".to_string(),
                "__mocks__/@scope/pkg.ts".to_string()
            )]
        );
        assert!(
            result.setup_files.contains(&std::path::PathBuf::from(
                "/project/__mocks__/@scope/pkg.ts"
            )),
            "scoped mock file should be seeded: {:?}",
            result.setup_files
        );
        assert!(
            result
                .referenced_dependencies
                .contains(&"@scope/pkg".to_string()),
            "aliased real dependency should stay credited"
        );
    }

    #[test]
    fn test_alias_projects_nested() {
        // test.projects[*].test.alias must be extracted, not only top-level.
        let source = r#"
            export default {
                test: {
                    projects: [
                        {
                            test: {
                                name: "unit",
                                alias: { vscode: "./test/mock/vscode.js" }
                            }
                        }
                    ]
                }
            };
        "#;
        let result = resolve_abs(source);
        assert_eq!(
            result.path_aliases,
            vec![("vscode".to_string(), "test/mock/vscode.js".to_string())]
        );
        assert!(
            result
                .setup_files
                .contains(&std::path::PathBuf::from("/project/test/mock/vscode.js"))
        );
    }

    #[test]
    fn test_alias_projects_nested_new_url_pathname() {
        // Vitest's own workspace fixtures use `new URL(..., import.meta.url).pathname`
        // for project-level test.alias replacements.
        let source = r#"
            export default {
                test: {
                    projects: [
                        {
                            test: {
                                alias: {
                                    "test-alias-from-vitest": new URL("./space/test-alias-to.ts", import.meta.url).pathname
                                }
                            }
                        }
                    ]
                }
            };
        "#;
        let result = resolve_abs(source);
        assert_eq!(
            result.path_aliases,
            vec![(
                "test-alias-from-vitest".to_string(),
                "space/test-alias-to.ts".to_string()
            )]
        );
        assert!(
            result
                .setup_files
                .contains(&std::path::PathBuf::from("/project/space/test-alias-to.ts"))
        );
    }

    #[test]
    fn test_alias_directory_target_not_seeded_as_entry_point() {
        // A directory alias (`@/` -> `src`) is a path alias whose target has no
        // file extension; it must NOT be seeded as a support entry point.
        let source = r#"
            export default {
                test: {
                    alias: { "@/": "./src" }
                }
            };
        "#;
        let result = resolve_abs(source);
        assert_eq!(
            result.path_aliases,
            vec![("@/".to_string(), "src".to_string())]
        );
        assert!(
            result.setup_files.is_empty(),
            "directory alias target should not be seeded: {:?}",
            result.setup_files
        );
    }

    #[test]
    fn test_alias_package_to_package_credits_both_no_path_alias() {
        // `'lodash-es' -> 'lodash'`: both bare packages. Credit both as referenced
        // and emit NO path alias (which would turn the lodash-es import
        // Unresolvable in a no-node_modules run).
        let source = r#"
            export default {
                test: {
                    alias: { "lodash-es": "lodash" }
                }
            };
        "#;
        let result = resolve_abs(source);
        assert!(
            result.path_aliases.is_empty(),
            "package-to-package alias should emit no path alias: {:?}",
            result.path_aliases
        );
        assert!(
            result
                .referenced_dependencies
                .contains(&"lodash".to_string()),
            "alias target package should be credited"
        );
        assert!(
            result
                .referenced_dependencies
                .contains(&"lodash-es".to_string()),
            "alias source package should be credited"
        );
    }

    #[test]
    fn test_alias_regexp_key_skipped_without_panic() {
        // RegExp `find` keys cannot become a starts_with prefix; the shared parser
        // returns None and the entry is silently skipped. Documented non-goal.
        let source = r#"
            export default {
                test: {
                    alias: [{ find: /^msw\/(.*)/, replacement: "./test/mock/msw.js" }]
                }
            };
        "#;
        let result = resolve_abs(source);
        assert!(
            result.path_aliases.is_empty(),
            "RegExp alias key should be skipped: {:?}",
            result.path_aliases
        );
    }
}
