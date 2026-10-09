//! Vitest test runner plugin.
//!
//! Detects Vitest projects and marks test/bench files as entry points.
//! Parses vitest.config to extract test.include, setupFiles, globalSetup,
//! and custom test environments as referenced dependencies.

use std::path::Path;

use fallow_config::JsxImportSourceRule;
use oxc_ast::ast::{Expression, ObjectExpression};

use super::config_parser;
use super::test_alias;
use super::{Plugin, PluginResult};

pub struct VitestPlugin;

const ENABLERS: &[&str] = &["vitest"];

/// Binaries that read a `--config` / `-c` file as a vitest config.
const SCRIPT_CONFIG_BINARIES: &[&str] = &["vitest"];

const ENTRY_PATTERNS: &[&str] = &[
    "**/*.test.{ts,tsx,js,jsx}",
    "**/*.spec.{ts,tsx,js,jsx}",
    "**/__tests__/**/*.{ts,tsx,js,jsx}",
    "**/*.bench.{ts,tsx,js,jsx}",
];

const CONFIG_PATTERNS: &[&str] = &[
    "**/vitest.config.{ts,js,mts,mjs,cts,cjs}",
    "**/vitest.workspace.{ts,js}",
    // A vite config carrying a `test` block IS the vitest config for that
    // project; vitest reads it directly. Every extraction below is keyed under
    // `test`, so a vite config without one contributes nothing.
    "**/vite.config.{ts,js,mts,mjs,cts,cjs}",
];

const ALWAYS_USED: &[&str] = &[
    "vitest.config.{ts,js,mts,mjs,cts,cjs}",
    "vitest.setup.{ts,js}",
    "vitest.workspace.{ts,js}",
    "**/src/setupTests.{ts,tsx,js,jsx}",
    "**/src/test-setup.{ts,tsx,js,jsx}",
];

const TOOLING_DEPENDENCIES: &[&str] = &["vitest"];
const CONFIG_EXPORTS: &[&str] = &["default"];

const FIXTURE_PATTERNS: &[&str] = &[
    "**/__fixtures__/**/*.{ts,tsx,js,jsx,json}",
    "**/fixtures/**/*.{ts,tsx,js,jsx,json}",
];

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
    "vitest.config.cts",
    "vitest.config.cjs",
    "vite.config.ts",
    "vite.config.js",
    "vite.config.mts",
    "vite.config.mjs",
    "vite.config.cts",
    "vite.config.cjs",
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
            ("vitest.config.{ts,js,mts,mjs,cts,cjs}", CONFIG_EXPORTS),
            ("vitest.workspace.{ts,js}", CONFIG_EXPORTS),
        ]
    }

    fn fixture_glob_patterns(&self) -> &'static [&'static str] {
        FIXTURE_PATTERNS
    }

    fn script_config_binaries(&self) -> &'static [&'static str] {
        SCRIPT_CONFIG_BINARIES
    }

    fn resolve_config(&self, config_path: &Path, source: &str, root: &Path) -> PluginResult {
        let mut result = PluginResult::default();

        let imports = config_parser::extract_imports(source, config_path);
        add_import_dependencies(&mut result, &imports);
        result.referenced_dependencies.extend(
            config_parser::extract_vite_plugin_option_dependencies(source, config_path),
        );

        apply_vitest_aliases(&mut result, source, config_path, root);
        apply_vitest_includes(&mut result, source, config_path);
        add_vitest_setup_files(&mut result, source, config_path, root);
        add_vitest_jsx_import_sources(&mut result, source, config_path);

        add_vitest_environment_dependency(&mut result, source, config_path);
        add_vitest_reporter_dependencies(&mut result, source, config_path);
        add_vitest_coverage_dependency(&mut result, source, config_path);
        add_vitest_typecheck_dependency(&mut result, source, config_path);
        add_vitest_browser_dependency(&mut result, source, config_path);

        result
    }
}

fn add_import_dependencies(result: &mut PluginResult, imports: &[String]) {
    for imp in imports {
        let dep = crate::resolve::extract_package_name(imp);
        result.referenced_dependencies.push(dep);
    }
}

fn apply_vitest_aliases(result: &mut PluginResult, source: &str, config_path: &Path, root: &Path) {
    test_alias::apply_test_block_aliases(result, source, config_path, root);
    for (find, replacement, is_bare) in
        config_parser::extract_config_aliases_kinded(source, config_path, &["resolve", "alias"])
    {
        test_alias::process_test_alias(result, &find, &replacement, is_bare, config_path, root);
    }
    test_alias::apply_workspace_array_aliases(result, source, config_path, root);
    test_alias::debug_unreachable_config(source, config_path);
}

fn apply_vitest_includes(result: &mut PluginResult, source: &str, config_path: &Path) {
    let root_includes =
        config_parser::extract_config_string_array(source, config_path, &["test", "include"]);
    if !root_includes.is_empty() {
        result.replace_entry_patterns = true;
    }
    result.extend_entry_patterns(root_includes);

    let project_includes = config_parser::extract_config_array_nested_string_or_array(
        source,
        config_path,
        &["test", "projects"],
        &["test", "include"],
    );
    result.extend_entry_patterns(project_includes);
}

/// The Vitest default `test.include`, `**/*.{test,spec}.?(c|m)[jt]s?(x)`, as
/// brace groups that the glob matcher supports.
const VITEST_DEFAULT_INCLUDE: &str = "**/*.{test,spec}.{js,jsx,ts,tsx,mjs,cjs,mts,cts}";

/// The JSX transform settings of one config object (`oxc.jsx` or the older
/// `esbuild` form).
#[derive(Clone, Default)]
struct JsxTransform {
    /// `Some(false)` when the transform imports no runtime (classic runtime,
    /// `preserve`, or esbuild `transform`). `None` when the config is silent.
    automatic: Option<bool>,
    import_source: Option<String>,
}

impl JsxTransform {
    /// Read the settings of one config object. `oxc` wins over `esbuild`,
    /// because Vite maps the older `esbuild` options to `oxc`.
    fn read(config: &ObjectExpression<'_>) -> Self {
        if let Some(oxc) = config_parser::property_expr(config, "oxc") {
            return config_parser::object_expression(oxc)
                .and_then(|oxc| config_parser::property_expr(oxc, "jsx"))
                .map_or_else(Self::default, Self::read_oxc_jsx);
        }
        config_parser::property_object(config, "esbuild").map_or_else(Self::default, |esbuild| {
            Self {
                automatic: config_parser::property_string(esbuild, "jsx")
                    .map(|mode| mode == "automatic"),
                import_source: config_parser::property_string(esbuild, "jsxImportSource"),
            }
        })
    }

    fn read_oxc_jsx(jsx: &Expression<'_>) -> Self {
        let Some(jsx) = config_parser::object_expression(jsx) else {
            // `jsx: 'preserve'` keeps the JSX, so no runtime import is added.
            return Self {
                automatic: config_parser::expression_to_string(jsx).map(|_| false),
                import_source: None,
            };
        };
        Self {
            automatic: config_parser::property_string(jsx, "runtime")
                .map(|runtime| runtime != "classic"),
            import_source: config_parser::property_string(jsx, "importSource"),
        }
    }

    /// Apply `self` over `base`, as Vite merges a project config over the
    /// root config.
    fn over(self, base: &Self) -> Self {
        Self {
            automatic: self.automatic.or(base.automatic),
            import_source: self.import_source.or_else(|| base.import_source.clone()),
        }
    }

    /// The import source when the transform adds a runtime import.
    fn runtime_source(self) -> Option<String> {
        if self.automatic == Some(false) {
            return None;
        }
        self.import_source.filter(|source| !source.is_empty())
    }
}

/// Record the JSX import source of the root config and of each inline
/// `test.projects` config, with the files that each config transforms.
///
/// A project with `extends: true` inherits the root settings. A project
/// without its own `test.include` uses the root include, and the root falls
/// back to the Vitest default include.
fn add_vitest_jsx_import_sources(result: &mut PluginResult, source: &str, config_path: &Path) {
    let Some(config_dir) = config_path.parent() else {
        return;
    };
    let rules = config_parser::extract_from_source(source, config_path, |program| {
        config_parser::find_config_object(program).map(collect_jsx_import_sources)
    })
    .unwrap_or_default();
    for (source, include) in rules {
        // A package runtime has no project file to reach, so it only credits
        // the package. A graph edge from test files alone would also make a
        // runtime dependency of the app look test-only.
        if !is_path_source(&source) {
            result
                .referenced_dependencies
                .push(crate::resolve::extract_package_name(&source));
            continue;
        }
        result.jsx_import_sources.push(JsxImportSourceRule {
            source,
            config_dir: config_dir.to_path_buf(),
            include,
        });
    }
}

fn collect_jsx_import_sources(root: &ObjectExpression<'_>) -> Vec<(String, Vec<String>)> {
    let root_jsx = JsxTransform::read(root);
    let root_test = config_parser::property_object(root, "test");
    let root_include = root_test
        .and_then(test_include)
        .unwrap_or_else(|| vec![VITEST_DEFAULT_INCLUDE.to_string()]);
    let mut rules = Vec::new();
    let projects = root_test
        .and_then(|test| config_parser::property_expr(test, "projects"))
        .and_then(config_parser::array_expression);
    // With `test.projects`, the root config is not a test project, and a
    // project inherits the root transform only with `extends: true`.
    if projects.is_none()
        && let Some(source) = root_jsx.clone().runtime_source()
    {
        rules.push((source, root_include.clone()));
    }
    for project in projects
        .iter()
        .flat_map(|projects| projects.elements.iter())
        .filter_map(|element| element.as_expression())
        .filter_map(config_parser::object_expression)
    {
        let own = JsxTransform::read(project);
        let jsx = if extends_root(project) {
            own.over(&root_jsx)
        } else {
            own
        };
        let Some(source) = jsx.runtime_source() else {
            continue;
        };
        let include = config_parser::property_object(project, "test")
            .and_then(test_include)
            .unwrap_or_else(|| root_include.clone());
        rules.push((source, include));
    }
    rules
}

/// Whether a project config has `extends: true`, which merges the root config
/// into it. A string value names another config file, which is not read.
/// Whether a JSX import source names a path, not a package.
fn is_path_source(source: &str) -> bool {
    source.starts_with('.') || source.starts_with('/')
}

fn extends_root(project: &ObjectExpression<'_>) -> bool {
    matches!(
        config_parser::property_expr(project, "extends"),
        Some(Expression::BooleanLiteral(value)) if value.value
    )
}

/// The `include` globs of a `test` block, or `None` when it sets none.
fn test_include(test: &ObjectExpression<'_>) -> Option<Vec<String>> {
    let include: Vec<String> = config_parser::property_expr(test, "include")
        .map(config_parser::expression_to_string_or_array)?
        .iter()
        .map(|pattern| vitest_include_glob(pattern))
        .collect();
    (!include.is_empty()).then_some(include)
}

/// Rewrite the extglob groups of a Vitest include pattern as brace groups,
/// which the glob matcher supports.
///
/// Vitest matches `include` with picomatch. `@(a|b)`, `+(a|b)` and a plain
/// `(a|b)` group become `{a,b}`, so `(*.)+(spec|test)` becomes
/// `*.{spec,test}`. A `+(..)` group matches its body once only, which is the
/// common case. Other forms (`!(..)`, `?(..)`, `*(..)`) and groups with
/// nested syntax stay as written, so they match nothing and add no edge.
fn vitest_include_glob(pattern: &str) -> String {
    let pattern = pattern.strip_prefix("./").unwrap_or(pattern);
    let mut out = String::with_capacity(pattern.len());
    let mut rest = pattern;
    while let Some(open) = rest.find('(') {
        let (before, after_open) = (&rest[..open], &rest[open + 1..]);
        let Some(close) = after_open.find(')') else {
            break;
        };
        let body = &after_open[..close];
        let operator = before.chars().next_back();
        if matches!(operator, Some('!' | '*'))
            || body.is_empty()
            || body.contains(['(', '{', '}', ','])
        {
            return format!("{out}{rest}");
        }
        let has_operator = matches!(operator, Some('@' | '+' | '?'));
        out.push_str(if has_operator {
            &before[..before.len() - 1]
        } else {
            before
        });
        let alternatives = body.replace('|', ",");
        if operator == Some('?') {
            // `?(a|b)` matches zero or one of the alternatives.
            out.push_str("{,");
            out.push_str(&alternatives);
            out.push('}');
        } else if body.contains('|') {
            out.push('{');
            out.push_str(&alternatives);
            out.push('}');
        } else {
            out.push_str(body);
        }
        rest = &after_open[close + 1..];
    }
    out.push_str(rest);
    out
}

fn add_vitest_setup_files(
    result: &mut PluginResult,
    source: &str,
    config_path: &Path,
    root: &Path,
) {
    let mut setup_files =
        config_parser::extract_config_string_or_array(source, config_path, &["test", "setupFiles"]);
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

    let mut global_setup = config_parser::extract_config_string_or_array(
        source,
        config_path,
        &["test", "globalSetup"],
    );
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
}

fn add_vitest_environment_dependency(result: &mut PluginResult, source: &str, config_path: &Path) {
    let Some(env) =
        config_parser::extract_config_string(source, config_path, &["test", "environment"])
    else {
        return;
    };

    match env.as_str() {
        "node" => {}
        "jsdom" | "happy-dom" => {
            result.referenced_dependencies.push(env.clone());
            super::credit_environment_optional_peers(&env, result);
        }
        _ => {
            // A built-in vitest environment names no installable package: there
            // is no `vitest-environment-<value>` package, and the bare name may
            // belong to an unrelated one, so crediting either exempts the wrong
            // dependency while leaving the peer vitest actually loads
            // unreported. The catalogue rows carry that peer instead.
            let credited = super::credit_config_value(
                super::config_value_credits::CreditSurface::VitestBuiltinEnvironment,
                &env,
                result,
            );
            if !credited {
                result
                    .referenced_dependencies
                    .push(format!("vitest-environment-{env}"));
                result.referenced_dependencies.push(env);
            }
        }
    }
}

fn add_vitest_reporter_dependencies(result: &mut PluginResult, source: &str, config_path: &Path) {
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
}

fn add_vitest_coverage_dependency(result: &mut PluginResult, source: &str, config_path: &Path) {
    if let Some(provider) =
        config_parser::extract_config_string(source, config_path, &["test", "coverage", "provider"])
        && !matches!(provider.as_str(), "v8" | "istanbul")
    {
        result
            .referenced_dependencies
            .push(format!("@vitest/coverage-{provider}"));
        result.referenced_dependencies.push(provider);
    }
}

fn add_vitest_typecheck_dependency(result: &mut PluginResult, source: &str, config_path: &Path) {
    if let Some(checker) =
        config_parser::extract_config_string(source, config_path, &["test", "typecheck", "checker"])
        && !matches!(checker.as_str(), "tsc")
    {
        result.referenced_dependencies.push(checker);
    }
}

fn add_vitest_browser_dependency(result: &mut PluginResult, source: &str, config_path: &Path) {
    if let Some(provider) =
        config_parser::extract_config_string(source, config_path, &["test", "browser", "provider"])
        && !matches!(provider.as_str(), "preview")
    {
        result
            .referenced_dependencies
            .push("@vitest/browser".to_string());
        result.referenced_dependencies.push(provider);
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
    fn package_jsx_import_source_credits_the_package_without_a_rule() {
        let result = VitestPlugin.resolve_config(
            std::path::Path::new("/project/vitest.config.ts"),
            "export default defineConfig({ oxc: { jsx: { importSource: '@emotion/react' } } });",
            std::path::Path::new("/project"),
        );
        assert!(result.jsx_import_sources.is_empty());
        assert!(
            result
                .referenced_dependencies
                .contains(&"@emotion/react".to_string())
        );
    }

    #[test]
    fn optional_extglob_groups_become_optional_brace_groups() {
        assert_eq!(
            vitest_include_glob("**/*.{test,spec}.?(c|m)[jt]s?(x)"),
            "**/*.{test,spec}.{,c,m}[jt]s{,x}"
        );
    }

    fn jsx_rules(source: &str) -> Vec<(String, Vec<String>)> {
        let result = VitestPlugin.resolve_config(
            std::path::Path::new("/project/vitest.config.ts"),
            source,
            std::path::Path::new("/project"),
        );
        result
            .jsx_import_sources
            .iter()
            .map(|rule| {
                assert_eq!(rule.config_dir, std::path::Path::new("/project"));
                (rule.source.clone(), rule.include.clone())
            })
            .collect()
    }

    #[test]
    fn jsx_import_source_at_root_uses_root_include() {
        let source = r"
            export default defineConfig({
                oxc: { jsx: { runtime: 'automatic', importSource: './src/jsx' } },
                test: { include: ['./src/**/*.test.tsx'] },
            });
        ";
        assert_eq!(
            jsx_rules(source),
            vec![(
                "./src/jsx".to_string(),
                vec!["src/**/*.test.tsx".to_string()]
            )]
        );
    }

    #[test]
    fn jsx_import_source_at_root_falls_back_to_default_include() {
        let source = r"
            export default defineConfig({
                oxc: { jsx: { importSource: './jsx' } },
            });
        ";
        assert_eq!(
            jsx_rules(source),
            vec![(
                "./jsx".to_string(),
                vec![VITEST_DEFAULT_INCLUDE.to_string()]
            )]
        );
    }

    #[test]
    fn jsx_import_source_per_project() {
        let source = r"
            export default defineConfig({
                test: {
                    include: ['src/**/*.spec.ts'],
                    projects: [
                        './packages/*/vitest.config.ts',
                        {
                            oxc: { jsx: { runtime: 'automatic', importSource: './src/jsx' } },
                            extends: true,
                            test: {
                                include: ['src/**/(*.)+(spec|test).+(ts|tsx|js)'],
                                name: 'main',
                            },
                        },
                        {
                            oxc: { jsx: { importSource: './src/jsx/dom' } },
                            test: { name: 'dom' },
                        },
                    ],
                },
            });
        ";
        assert_eq!(
            jsx_rules(source),
            vec![
                (
                    "./src/jsx".to_string(),
                    vec!["src/**/*.{spec,test}.{ts,tsx,js}".to_string()]
                ),
                (
                    "./src/jsx/dom".to_string(),
                    vec!["src/**/*.spec.ts".to_string()]
                ),
            ]
        );
    }

    #[test]
    fn jsx_import_source_is_inherited_only_with_extends_true() {
        let source = r"
            export default defineConfig({
                oxc: { jsx: { importSource: './jsx' } },
                test: {
                    projects: [
                        { extends: true, test: { include: ['a/**/*.test.tsx'] } },
                        { test: { include: ['b/**/*.test.tsx'] } },
                        {
                            extends: true,
                            oxc: { jsx: { runtime: 'automatic' } },
                            test: { include: ['c/**/*.test.tsx'] },
                        },
                    ],
                },
            });
        ";
        assert_eq!(
            jsx_rules(source),
            vec![
                ("./jsx".to_string(), vec!["a/**/*.test.tsx".to_string()]),
                ("./jsx".to_string(), vec!["c/**/*.test.tsx".to_string()]),
            ]
        );
    }

    #[test]
    fn classic_jsx_runtime_adds_no_rule() {
        let source = r"
            export default defineConfig({
                oxc: { jsx: { runtime: 'automatic', importSource: './jsx' } },
                test: {
                    projects: [
                        {
                            extends: true,
                            oxc: { jsx: { runtime: 'classic' } },
                            test: { include: ['a/**/*.test.tsx'] },
                        },
                        {
                            esbuild: { jsx: 'transform', jsxImportSource: 'solid-js' },
                            test: { include: ['b/**/*.test.tsx'] },
                        },
                        {
                            oxc: { jsx: 'preserve' },
                            test: { include: ['c/**/*.test.tsx'] },
                        },
                    ],
                },
            });
        ";
        assert!(jsx_rules(source).is_empty());
    }

    #[test]
    fn esbuild_jsx_import_source_is_read() {
        let source = r"
            export default defineConfig({
                esbuild: { jsx: 'automatic', jsxImportSource: './src/jsx' },
                test: { include: ['test/**/*.test.tsx'] },
            });
        ";
        assert_eq!(
            jsx_rules(source),
            vec![(
                "./src/jsx".to_string(),
                vec!["test/**/*.test.tsx".to_string()]
            )]
        );
    }

    #[test]
    fn config_without_jsx_import_source_adds_no_rule() {
        let source = r"
            export default defineConfig({
                oxc: { jsx: { runtime: 'automatic' } },
                test: { include: ['src/**/*.test.tsx'] },
            });
        ";
        assert!(jsx_rules(source).is_empty());
    }

    #[test]
    fn include_extglob_groups_become_brace_groups() {
        assert_eq!(
            vitest_include_glob("./src/**/(*.)+(spec|test).+(ts|tsx|js)"),
            "src/**/*.{spec,test}.{ts,tsx,js}"
        );
        assert_eq!(
            vitest_include_glob("src/**/*.@(test|spec).tsx"),
            "src/**/*.{test,spec}.tsx"
        );
        assert_eq!(
            vitest_include_glob("src/**/*.test.tsx"),
            "src/**/*.test.tsx"
        );
        // Forms without a brace equivalent stay as written.
        assert_eq!(vitest_include_glob("src/!(skip)/*.ts"), "src/!(skip)/*.ts");
        assert_eq!(vitest_include_glob("src/(a|(b))/*.ts"), "src/(a|(b))/*.ts");
    }

    /// Issue #2226: neither runner gives literal `X/__mocks__` imports special
    /// meaning, so Vitest declares no virtual package suffixes and a literal
    /// import of such a package is reported as unlisted, matching Jest.
    #[test]
    fn no_virtual_package_suffixes_declared() {
        assert!(
            VitestPlugin.virtual_package_suffixes().is_empty(),
            "VitestPlugin must not suppress /__mocks__ package names"
        );
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
        assert!(result.referenced_dependencies.is_empty());
    }

    #[test]
    fn credits_react_babel_plugin_dependencies() {
        let source = r#"
            import { defineConfig } from "vitest/config";
            import react from "@vitejs/plugin-react";

            export default defineConfig({
                plugins: [
                    react({
                        babel: {
                            plugins: [["module:@preact/signals-react-transform", {}]],
                            presets: ["@babel/preset-react"],
                        },
                    }),
                ],
            });
        "#;
        let result = resolve(source);
        assert!(
            result
                .referenced_dependencies
                .contains(&"@preact/signals-react-transform".to_string()),
            "React Babel plugin dependency should be credited: {:?}",
            result.referenced_dependencies
        );
        assert!(
            result
                .referenced_dependencies
                .contains(&"@babel/preset-react".to_string()),
            "React Babel preset dependency should be credited: {:?}",
            result.referenced_dependencies
        );
    }

    #[test]
    fn custom_environment() {
        let source = r#"
            export default {
                test: {
                    environment: "custom-env"
                }
            };
        "#;
        let result = resolve(source);
        assert!(
            result
                .referenced_dependencies
                .contains(&"vitest-environment-custom-env".to_string())
        );
        assert!(
            result
                .referenced_dependencies
                .contains(&"custom-env".to_string())
        );
    }

    /// `edge-runtime` is a built-in vitest environment backed by the optional
    /// peer `@edge-runtime/vm`. Crediting the shorthand names instead exempted an
    /// unrelated CLI package and left the real dependency reported as unused.
    #[test]
    fn builtin_edge_runtime_environment_credits_vm_package() {
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
                .contains(&"@edge-runtime/vm".to_string()),
            "expected @edge-runtime/vm, got {:?}",
            result.referenced_dependencies
        );
        assert!(
            !result
                .referenced_dependencies
                .contains(&"vitest-environment-edge-runtime".to_string()),
            "no such package exists"
        );
        assert!(
            !result
                .referenced_dependencies
                .contains(&"edge-runtime".to_string()),
            "the bare name is an unrelated package and must not be exempted"
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

    fn resolve_abs(source: &str) -> PluginResult {
        VitestPlugin.resolve_config(
            std::path::Path::new("/project/vitest.config.ts"),
            source,
            std::path::Path::new("/project"),
        )
    }

    #[test]
    fn test_alias_object_form_virtual_module() {
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

    #[test]
    fn top_level_resolve_alias_extracted_from_vitest_config() {
        let source = r#"
            import { resolve } from "node:path";
            export default {
                resolve: {
                    alias: { "vite/module-runner": resolve(__dirname, "src/module-runner/index.ts") }
                },
                test: { include: ["**/*.spec.ts"] }
            };
        "#;
        let result = resolve_abs(source);
        assert!(
            result.path_aliases.contains(&(
                "vite/module-runner".to_string(),
                "src/module-runner/index.ts".to_string()
            )),
            "top-level resolve.alias must be extracted: {:?}",
            result.path_aliases
        );
    }

    #[test]
    fn project_level_resolve_alias_extracted() {
        let source = r#"
            export default {
                test: {
                    projects: [
                        { test: { name: "browser" }, resolve: { alias: { "test-alias-from-vite": "./mock/to.ts" } } }
                    ]
                }
            };
        "#;
        let result = resolve_abs(source);
        assert!(
            result
                .path_aliases
                .contains(&("test-alias-from-vite".to_string(), "mock/to.ts".to_string())),
            "project-level resolve.alias must be extracted: {:?}",
            result.path_aliases
        );
    }

    #[test]
    fn function_form_define_config_test_alias_extracted() {
        let source = r#"
            import { defineConfig } from "vitest/config";
            export default defineConfig(() => ({
                test: { alias: { vscode: "./test/mock/vscode.ts" } }
            }));
        "#;
        let result = resolve_abs(source);
        assert!(
            result
                .path_aliases
                .contains(&("vscode".to_string(), "test/mock/vscode.ts".to_string())),
            "function-form defineConfig test.alias must be extracted: {:?}",
            result.path_aliases
        );
    }
}
