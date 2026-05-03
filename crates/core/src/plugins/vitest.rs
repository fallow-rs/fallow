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
}
