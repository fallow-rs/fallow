//! Playwright test runner plugin.
//!
//! Detects Playwright projects and marks test files and config as entry points.

use std::path::{Path, PathBuf};

use rustc_hash::FxHashMap;

use oxc_ast::ast::{Expression, ObjectExpression, ObjectPropertyKind};

use super::config_parser;
use super::{Plugin, PluginResult};
use crate::scripts;

/// Test entry patterns for a project without a config, or with a config whose
/// test locations are not statically known. The directory patterns also keep
/// the helpers in the conventional test directories.
const DEFAULT_TEST_ENTRY_PATTERNS: &[&str] = &[
    "**/*.spec.{ts,tsx,js,jsx}",
    "**/*.test.{ts,tsx,js,jsx}",
    "tests/**/*.{ts,tsx,js,jsx}",
    "e2e/**/*.{ts,tsx,js,jsx}",
];

/// The file-name patterns of [`DEFAULT_TEST_ENTRY_PATTERNS`]. A config with a
/// static `testDir` keeps them, because other test runners (for example
/// `node --test`) select files with the same names and do not always have
/// their own plugin.
const TEST_FILE_NAME_PATTERNS: &[&str] =
    &["**/*.spec.{ts,tsx,js,jsx}", "**/*.test.{ts,tsx,js,jsx}"];

/// The file extensions of the Playwright default `testMatch`,
/// `**/*.@(spec|test).?(c|m)[jt]s?(x)`.
const TEST_FILE_EXTENSIONS: &str = "{ts,tsx,js,jsx,mts,cts,mjs,cjs}";

define_plugin!(
    struct PlaywrightPlugin => "playwright",
    enablers: &["@playwright/test"],
    entry_patterns: DEFAULT_TEST_ENTRY_PATTERNS,
    config_patterns: &["playwright.config.{ts,js}"],
    always_used: &["playwright.config.{ts,js}"],
    tooling_dependencies: &["@playwright/test", "playwright"],
    fixture_glob_patterns: &[
        "**/fixtures/**/*.{ts,tsx,js,jsx,json}",
        "e2e/fixtures/**/*.{ts,tsx,js,jsx,json}",
    ],
    resolve_config(config_path, source, root) {
        let mut result = PluginResult::default();

        let config_dir = config_path
            .parent()
            .filter(|parent| parent.is_absolute())
            .unwrap_or(root);

        let imports = config_parser::extract_imports(source, config_path);
        for imp in &imports {
            let dep = crate::resolve::extract_package_name(imp);
            result.referenced_dependencies.push(dep);
        }

        if let Some(setup) =
            config_parser::extract_config_string(source, config_path, &["globalSetup"])
        {
            result
                .setup_files
                .push(config_dir.join(setup.trim_start_matches("./")));
        }
        if let Some(teardown) =
            config_parser::extract_config_string(source, config_path, &["globalTeardown"])
        {
            result
                .setup_files
                .push(config_dir.join(teardown.trim_start_matches("./")));
        }

        let (web_deps, web_setup) = collect_web_server(source, config_path, root, config_dir);
        result.referenced_dependencies.extend(web_deps);
        result.setup_files.extend(web_setup);

        // Every config states its full set of test entries, so that two
        // configs in one project keep the entries of both. A config without
        // static test locations restates the defaults.
        match test_entry_patterns(source, config_path, root, config_dir) {
            Some(test_dir_patterns) => {
                result.extend_entry_patterns(TEST_FILE_NAME_PATTERNS.iter().copied());
                result.extend_entry_patterns(test_dir_patterns);
            }
            None => result.extend_entry_patterns(DEFAULT_TEST_ENTRY_PATTERNS.iter().copied()),
        }
        result.replace_entry_patterns = true;

        result
    },
);

/// A `testDir` or `testMatch` value of one config scope.
#[derive(Clone)]
enum ScopeValue<T> {
    Absent,
    Static(T),
    Dynamic,
}

impl<T: Clone> ScopeValue<T> {
    /// The value of a project, which inherits the top-level value when it
    /// does not set its own.
    fn or_inherit(&self, top: &Self) -> Self {
        match self {
            Self::Absent => top.clone(),
            other => other.clone(),
        }
    }
}

/// The test locations of the top-level config or of one project.
#[derive(Clone)]
struct TestScope {
    test_dir: ScopeValue<String>,
    test_match: ScopeValue<Vec<String>>,
}

/// Entry patterns for the test files that the config selects, as project-root
/// relative globs.
///
/// Playwright collects test files only below `testDir`. Each scope (the
/// top-level config, or each `projects[]` element) gives one directory. A
/// project inherits `testDir` and `testMatch` from the top level. A glob
/// `testMatch` applies below `testDir`, with a `**/` prefix as Playwright adds
/// one. A regular expression or other non-static `testMatch` keeps every
/// script file below `testDir`. Unknown scopes add the conventional test
/// directories while preserving entry patterns from known scopes.
fn test_entry_patterns(
    source: &str,
    config_path: &Path,
    root: &Path,
    config_dir: &Path,
) -> Option<Vec<String>> {
    let scopes = config_parser::extract_from_source(source, config_path, |program| {
        let config = config_parser::find_config_object(program)?;
        Some(config_test_scopes(config))
    })?;

    let mut patterns = Vec::new();
    for scope in scopes {
        let ScopeValue::Static(test_dir) = scope.test_dir else {
            patterns.extend(
                DEFAULT_TEST_ENTRY_PATTERNS
                    .iter()
                    .skip(TEST_FILE_NAME_PATTERNS.len())
                    .map(|pattern| (*pattern).to_string()),
            );
            continue;
        };
        let Some(prefix) = test_dir_prefix(&test_dir, root, config_dir) else {
            patterns.extend(
                DEFAULT_TEST_ENTRY_PATTERNS
                    .iter()
                    .skip(TEST_FILE_NAME_PATTERNS.len())
                    .map(|pattern| (*pattern).to_string()),
            );
            continue;
        };
        match scope.test_match {
            ScopeValue::Absent => {
                let glob =
                    case_insensitive_glob(&format!("*.{{spec,test}}.{TEST_FILE_EXTENSIONS}"));
                patterns.push(format!("{prefix}**/{glob}"));
            }
            ScopeValue::Static(globs) => {
                for glob in globs {
                    let glob = glob.strip_prefix("**/").unwrap_or(&glob);
                    let glob = case_insensitive_glob(glob);
                    patterns.push(format!("{prefix}**/{glob}"));
                }
            }
            ScopeValue::Dynamic => {
                patterns.push(format!("{prefix}**/*.{TEST_FILE_EXTENSIONS}"));
            }
        }
    }
    Some(patterns)
}

/// Known projects keep their entries even when another project is dynamic.
/// Unknown projects also keep the top-level scope and conventional defaults.
fn config_test_scopes(config: &ObjectExpression<'_>) -> Vec<TestScope> {
    let top = scope_of(config);
    let inherited = TestScope {
        test_dir: top.test_dir.clone(),
        test_match: ScopeValue::Dynamic,
    };
    let unknown = TestScope {
        test_dir: ScopeValue::Dynamic,
        test_match: ScopeValue::Dynamic,
    };
    let projects = match scope_property(config, "projects") {
        ScopeValue::Absent => return vec![top],
        ScopeValue::Static(expr) => expr,
        ScopeValue::Dynamic => return vec![inherited, unknown],
    };
    let Some(projects) = config_parser::array_expression(projects) else {
        return vec![inherited, unknown];
    };
    if projects.elements.is_empty() {
        return vec![top];
    }
    let mut scopes = Vec::new();
    for element in &projects.elements {
        let Some(project) = element
            .as_expression()
            .and_then(config_parser::object_expression)
        else {
            scopes.extend([inherited.clone(), unknown.clone()]);
            continue;
        };
        let own = scope_of(project);
        if matches!(own.test_dir, ScopeValue::Dynamic) {
            scopes.push(TestScope {
                test_dir: top.test_dir.clone(),
                test_match: own.test_match.or_inherit(&top.test_match),
            });
        }
        scopes.push(TestScope {
            test_dir: own.test_dir.or_inherit(&top.test_dir),
            test_match: own.test_match.or_inherit(&top.test_match),
        });
    }
    scopes
}

fn scope_of(obj: &ObjectExpression<'_>) -> TestScope {
    let test_dir = match scope_property(obj, "testDir") {
        ScopeValue::Absent => ScopeValue::Absent,
        ScopeValue::Dynamic => ScopeValue::Dynamic,
        ScopeValue::Static(expr) => config_parser::expression_to_path_string(expr)
            .map_or(ScopeValue::Dynamic, ScopeValue::Static),
    };
    let test_match = match scope_property(obj, "testMatch") {
        ScopeValue::Absent => ScopeValue::Absent,
        ScopeValue::Dynamic => ScopeValue::Dynamic,
        ScopeValue::Static(expr) => {
            static_globs(expr).map_or(ScopeValue::Dynamic, ScopeValue::Static)
        }
    };
    TestScope {
        test_dir,
        test_match,
    }
}

/// The last assignment wins. A later unresolved spread or computed key can
/// override this field, so it cannot safely narrow the test entries.
fn scope_property<'a>(obj: &'a ObjectExpression<'a>, key: &str) -> ScopeValue<&'a Expression<'a>> {
    for property in obj.properties.iter().rev() {
        match property {
            ObjectPropertyKind::SpreadProperty(_) => return ScopeValue::Dynamic,
            ObjectPropertyKind::ObjectProperty(property) => {
                if property.computed {
                    return ScopeValue::Dynamic;
                }
                if property.key.static_name().is_some_and(|name| name == key) {
                    return ScopeValue::Static(&property.value);
                }
            }
        }
    }
    ScopeValue::Absent
}

/// Extglob group openers. Playwright matches `testMatch` with extglob
/// support, for example `**/*.@(spec|test).ts`, but the entry glob matcher
/// reads these characters literally, so such a glob matches no file.
const EXTGLOB_OPENERS: [&str; 5] = ["@(", "?(", "+(", "*(", "!("];

/// The globs of a string or string-array `testMatch`. Returns `None` for a
/// regular expression, a non-literal value, a glob that names a path outside
/// `testDir`, a glob with an extglob group, or a glob with a directory part.
///
/// Playwright adds a `**/` prefix and matches the glob against the absolute
/// file path. Thus the directory part of a glob such as `tests/**/*.e2e.ts`
/// can match `testDir` itself or a parent directory of `testDir`. A glob that
/// is relative to `testDir` cannot show that match, so such a `testMatch`
/// keeps every script file below `testDir`.
fn static_globs(expr: &Expression<'_>) -> Option<Vec<String>> {
    let globs = match config_parser::array_expression(expr) {
        Some(array) => array
            .elements
            .iter()
            .map(|element| config_parser::expression_to_path_string(element.as_expression()?))
            .collect::<Option<Vec<_>>>()?,
        None => vec![config_parser::expression_to_path_string(expr)?],
    };
    let in_test_dir = |glob: &String| {
        !glob.is_empty() && !glob.starts_with('/') && !glob.split('/').any(|part| part == "..")
    };
    let file_name_only = |glob: &String| !glob.strip_prefix("**/").unwrap_or(glob).contains('/');
    let usable = |glob: &String| {
        in_test_dir(glob)
            && file_name_only(glob)
            && glob.is_ascii()
            && !glob.contains(['[', '\\'])
            && !EXTGLOB_OPENERS.iter().any(|opener| glob.contains(opener))
    };
    (!globs.is_empty() && globs.iter().all(usable)).then_some(globs)
}

/// Playwright matches filenames without case sensitivity. Globset has no
/// per-pattern flag in entry rules, so expand ASCII letters into character
/// classes. Patterns with existing classes or escapes use the broad fallback.
fn case_insensitive_glob(glob: &str) -> String {
    let mut pattern = String::new();
    for ch in glob.chars() {
        if ch.is_ascii_alphabetic() {
            pattern.push('[');
            pattern.push(ch.to_ascii_lowercase());
            pattern.push(ch.to_ascii_uppercase());
            pattern.push(']');
        } else {
            pattern.push(ch);
        }
    }
    pattern
}

/// `testDir` as a project-root-relative directory prefix with a trailing `/`,
/// or an empty prefix for the root itself. `testDir` is relative to the
/// config file directory. Returns `None` for a directory outside the root.
fn test_dir_prefix(test_dir: &str, root: &Path, config_dir: &Path) -> Option<String> {
    let dir = config_parser::lexical_normalize(
        &config_dir.join(config_parser::path_from_config_string(test_dir)),
    );
    let relative = dir
        .strip_prefix(config_parser::lexical_normalize(root))
        .ok()?;
    let relative = globset::escape(&config_parser::path_to_config_string(relative));
    Some(if relative.is_empty() {
        String::new()
    } else {
        format!("{relative}/")
    })
}

/// Parse Playwright `webServer.command` entries (object and array forms) into
/// referenced dependencies and reachable setup files.
///
/// Each command is run through the shared script parser ([`scripts::analyze_command`]),
/// so invoked npm binaries are credited as dependencies and local file arguments are
/// seeded as support entry files exactly as they would be in a package.json script.
/// `config_dir` is the directory of the config file: file arguments resolve there by
/// default, matching Playwright's `webServer.cwd` default. A `webServer.cwd` (per
/// object, or per array element) overrides that base, resolved relative to `config_dir`
/// (an absolute cwd replaces it). `root` is the project root, used only for
/// binary-to-package resolution (it owns `node_modules`). Commands that delegate to a
/// package manager (`npm run start`, `yarn dev`) credit nothing, since the underlying
/// script's own dependencies are analyzed separately.
fn collect_web_server(
    source: &str,
    config_path: &Path,
    root: &Path,
    config_dir: &Path,
) -> (Vec<String>, Vec<PathBuf>) {
    let mut commands: Vec<(String, Option<String>)> = Vec::new();

    if let Some(command) =
        config_parser::extract_config_command(source, config_path, &["webServer", "command"])
    {
        let cwd = config_parser::extract_config_string(source, config_path, &["webServer", "cwd"]);
        commands.push((command, cwd));
    }

    commands.extend(config_parser::extract_config_array_object_command_pairs(
        source,
        config_path,
        &["webServer"],
        "command",
        "cwd",
    ));

    let mut referenced_dependencies = Vec::new();
    let mut setup_files = Vec::new();

    for (command, cwd) in commands {
        let analysis = scripts::analyze_command(&command, root, &FxHashMap::default());
        referenced_dependencies.extend(analysis.used_packages);

        let base = cwd.map_or_else(
            || config_dir.to_path_buf(),
            |dir| config_dir.join(dir.trim_start_matches("./")),
        );
        for file in analysis
            .config_files
            .into_iter()
            .chain(analysis.entry_files)
        {
            setup_files.push(base.join(file.trim_start_matches("./")));
        }
    }

    (referenced_dependencies, setup_files)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_config_global_setup() {
        let source = r#"
            export default {
                globalSetup: "./global-setup.ts"
            };
        "#;
        let plugin = PlaywrightPlugin;
        let result = plugin.resolve_config(
            Path::new("playwright.config.ts"),
            source,
            Path::new("/project"),
        );
        assert_eq!(
            result.setup_files,
            vec![Path::new("/project/global-setup.ts")]
        );
    }

    #[test]
    fn resolve_config_global_teardown() {
        let source = r#"
            export default {
                globalTeardown: "./global-teardown.ts"
            };
        "#;
        let plugin = PlaywrightPlugin;
        let result = plugin.resolve_config(
            Path::new("playwright.config.ts"),
            source,
            Path::new("/project"),
        );
        assert_eq!(
            result.setup_files,
            vec![Path::new("/project/global-teardown.ts")]
        );
    }

    #[test]
    fn resolve_config_both_setup_and_teardown() {
        let source = r#"
            export default {
                globalSetup: "./setup.ts",
                globalTeardown: "./teardown.ts"
            };
        "#;
        let plugin = PlaywrightPlugin;
        let result = plugin.resolve_config(
            Path::new("playwright.config.ts"),
            source,
            Path::new("/project"),
        );
        assert_eq!(
            result.setup_files,
            vec![
                Path::new("/project/setup.ts"),
                Path::new("/project/teardown.ts"),
            ]
        );
    }

    #[test]
    fn resolve_config_imports() {
        let source = r#"
            import { defineConfig, devices } from '@playwright/test';
            export default defineConfig({
                globalSetup: "./setup.ts"
            });
        "#;
        let plugin = PlaywrightPlugin;
        let result = plugin.resolve_config(
            Path::new("playwright.config.ts"),
            source,
            Path::new("/project"),
        );
        assert!(
            result
                .referenced_dependencies
                .contains(&"@playwright/test".to_string())
        );
        assert_eq!(result.setup_files, vec![Path::new("/project/setup.ts")]);
    }

    #[test]
    fn resolve_config_empty() {
        let source = r"export default {};";
        let plugin = PlaywrightPlugin;
        let result = plugin.resolve_config(
            Path::new("playwright.config.ts"),
            source,
            Path::new("/project"),
        );
        assert!(result.setup_files.is_empty());
        assert!(result.referenced_dependencies.is_empty());
    }

    #[test]
    fn resolve_config_setup_strips_dot_slash() {
        let source = r#"
            export default {
                globalSetup: "./tests/global-setup.ts"
            };
        "#;
        let plugin = PlaywrightPlugin;
        let result = plugin.resolve_config(
            Path::new("playwright.config.ts"),
            source,
            Path::new("/project"),
        );
        assert_eq!(
            result.setup_files,
            vec![Path::new("/project/tests/global-setup.ts")]
        );
    }

    #[test]
    fn resolve_config_setup_without_dot_slash() {
        let source = r#"
            export default {
                globalSetup: "tests/global-setup.ts"
            };
        "#;
        let plugin = PlaywrightPlugin;
        let result = plugin.resolve_config(
            Path::new("playwright.config.ts"),
            source,
            Path::new("/project"),
        );
        assert_eq!(
            result.setup_files,
            vec![Path::new("/project/tests/global-setup.ts")]
        );
    }

    #[test]
    fn fixture_patterns_are_set() {
        let plugin = PlaywrightPlugin;
        assert!(!plugin.fixture_glob_patterns().is_empty());
    }

    fn resolve(source: &str) -> PluginResult {
        PlaywrightPlugin.resolve_config(
            Path::new("playwright.config.ts"),
            source,
            Path::new("/project"),
        )
    }

    #[test]
    fn web_server_object_command_credits_cli_dependency() {
        let source = r#"
            export default {
                webServer: { command: "srvx --port 3000", url: "http://localhost:3000" }
            };
        "#;
        let result = resolve(source);
        assert!(
            result.referenced_dependencies.contains(&"srvx".to_string()),
            "srvx CLI binary should be credited, got {:?}",
            result.referenced_dependencies
        );
        assert!(
            result.setup_files.is_empty(),
            "a flag-only command seeds no files, got {:?}",
            result.setup_files
        );
    }

    #[test]
    fn web_server_template_command_credits_pnpm_exec_cli_dependency() {
        let source = r"
            const PORT = 3000;
            export default {
                webServer: {
                    command: `pnpm build && pnpm exec srvx --prod --port ${PORT} --hostname 127.0.0.1`
                }
            };
        ";
        let result = resolve(source);
        assert!(
            result.referenced_dependencies.contains(&"srvx".to_string()),
            "srvx CLI binary should be credited from pnpm exec template command, got {:?}",
            result.referenced_dependencies
        );
    }

    #[test]
    fn web_server_array_template_command_credits_pnpm_exec_cli_dependency() {
        let source = r"
            const PORT = 3000;
            export default {
                webServer: [
                    {
                        command: `pnpm exec srvx --prod --port ${PORT}`,
                    },
                ],
            };
        ";
        let result = resolve(source);
        assert!(
            result.referenced_dependencies.contains(&"srvx".to_string()),
            "srvx CLI binary should be credited from array template command, got {:?}",
            result.referenced_dependencies
        );
    }

    #[test]
    fn web_server_array_node_runner_seeds_file_and_credits_runner() {
        let source = r#"
            export default {
                webServer: [{ command: "tsx scripts/e2e-server.ts" }]
            };
        "#;
        let result = resolve(source);
        assert!(
            result.referenced_dependencies.contains(&"tsx".to_string()),
            "tsx node runner should be credited, got {:?}",
            result.referenced_dependencies
        );
        assert_eq!(
            result.setup_files,
            vec![Path::new("/project/scripts/e2e-server.ts")]
        );
    }

    #[test]
    fn web_server_object_command_honors_cwd() {
        let source = r#"
            export default {
                webServer: { command: "node server.js", cwd: "packages/api" }
            };
        "#;
        let result = resolve(source);
        assert!(
            !result.referenced_dependencies.contains(&"node".to_string()),
            "node must not be credited as a dependency, got {:?}",
            result.referenced_dependencies
        );
        assert_eq!(
            result.setup_files,
            vec![Path::new("/project/packages/api/server.js")],
            "server.js must resolve under webServer.cwd"
        );
    }

    #[test]
    fn web_server_array_per_element_cwd() {
        let source = r#"
            export default {
                webServer: [
                    { command: "tsx scripts/api.ts", cwd: "packages/api" },
                    { command: "tsx scripts/web.ts" }
                ]
            };
        "#;
        let result = resolve(source);
        assert!(
            result
                .setup_files
                .contains(&PathBuf::from("/project/packages/api/scripts/api.ts"))
        );
        assert!(
            result
                .setup_files
                .contains(&PathBuf::from("/project/scripts/web.ts"))
        );
    }

    #[test]
    fn web_server_package_manager_delegation_is_noop() {
        let source = r#"
            export default {
                webServer: { command: "npm run start" }
            };
        "#;
        let result = resolve(source);
        assert!(
            result.referenced_dependencies.is_empty(),
            "npm run delegation must not credit a phantom dependency, got {:?}",
            result.referenced_dependencies
        );
        assert!(result.setup_files.is_empty());
    }

    #[test]
    fn web_server_and_global_setup_coexist() {
        let source = r#"
            export default {
                globalSetup: "./setup.ts",
                webServer: { command: "tsx scripts/e2e-server.ts" }
            };
        "#;
        let result = resolve(source);
        assert!(
            result
                .setup_files
                .contains(&PathBuf::from("/project/setup.ts"))
        );
        assert!(
            result
                .setup_files
                .contains(&PathBuf::from("/project/scripts/e2e-server.ts"))
        );
    }

    #[test]
    fn web_server_strips_leading_dot_slash_in_file_args() {
        let source = r#"
            export default {
                webServer: { command: "tsx ./scripts/e2e-server.ts" }
            };
        "#;
        let result = resolve(source);
        assert_eq!(
            result.setup_files,
            vec![Path::new("/project/scripts/e2e-server.ts")]
        );
    }

    #[test]
    fn no_web_server_seeds_nothing() {
        let source = r#"
            export default {
                globalSetup: "./setup.ts"
            };
        "#;
        let result = resolve(source);
        assert_eq!(result.setup_files, vec![Path::new("/project/setup.ts")]);
    }

    /// Build a platform-absolute path from a `/project/...`-style logical path.
    /// On Windows a leading-slash path lacks a drive and is NOT absolute, so the
    /// `config_path.parent().is_absolute()` gate in `resolve_config` would fall
    /// back to the root and drop the nested config directory. The registry
    /// always passes a genuinely-absolute config path at runtime (drive-rooted
    /// on Windows), so these tests must do the same. On Unix this is the identity.
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

    /// Resolve with an absolute, nested config path (as the registry passes at
    /// runtime), to exercise the config-file-directory base.
    fn resolve_at(config_path: &str, source: &str) -> PluginResult {
        PlaywrightPlugin.resolve_config(&abs(config_path), source, &abs("/project"))
    }

    #[test]
    fn web_server_file_args_resolve_from_nested_config_dir_not_root() {
        let source = r#"
            export default {
                webServer: { command: "tsx scripts/e2e-server.ts" }
            };
        "#;
        let result = resolve_at("/project/apps/web/playwright.config.ts", source);
        assert_eq!(
            result.setup_files,
            vec![abs("/project/apps/web/scripts/e2e-server.ts")],
            "nested-config file args must resolve under the config directory, not the project root"
        );
    }

    #[test]
    fn web_server_nested_config_cwd_resolves_relative_to_config_dir() {
        let source = r#"
            export default {
                webServer: { command: "tsx scripts/server.ts", cwd: "api" }
            };
        "#;
        let result = resolve_at("/project/apps/web/playwright.config.ts", source);
        assert_eq!(
            result.setup_files,
            vec![abs("/project/apps/web/api/scripts/server.ts")],
            "cwd must resolve relative to the config directory"
        );
    }

    fn entry_patterns(result: &PluginResult) -> Vec<&str> {
        result
            .entry_patterns
            .iter()
            .map(|rule| rule.pattern.as_str())
            .collect()
    }

    fn default_patterns() -> Vec<&'static str> {
        DEFAULT_TEST_ENTRY_PATTERNS.to_vec()
    }

    /// The patterns that the config adds after the kept file-name patterns.
    fn test_dir_patterns(result: &PluginResult) -> Vec<&str> {
        let patterns = entry_patterns(result);
        let (file_names, test_dir) = patterns.split_at(TEST_FILE_NAME_PATTERNS.len());
        assert_eq!(file_names, TEST_FILE_NAME_PATTERNS);
        test_dir.to_vec()
    }

    #[test]
    fn test_dir_replaces_default_test_directories() {
        let result = resolve(r"export default defineConfig({ testDir: './ui' });");
        assert!(result.replace_entry_patterns);
        let patterns = entry_patterns(&result);
        assert!(
            !patterns.contains(&"tests/**/*.{ts,tsx,js,jsx}")
                && !patterns.contains(&"e2e/**/*.{ts,tsx,js,jsx}"),
            "a static testDir drops the default test directories, got {patterns:?}"
        );
        assert_eq!(
            test_dir_patterns(&result),
            vec![
                "ui/**/*.{[sS][pP][eE][cC],[tT][eE][sS][tT]}.{[tT][sS],[tT][sS][xX],[jJ][sS],[jJ][sS][xX],[mM][tT][sS],[cC][tT][sS],[mM][jJ][sS],[cC][jJ][sS]}"
            ]
        );
    }

    #[test]
    fn test_dir_resolves_from_nested_config_dir() {
        let result = resolve_at(
            "/project/e2e/playwright.config.ts",
            r"export default defineConfig({ testDir: './ui' });",
        );
        assert_eq!(
            test_dir_patterns(&result),
            vec![
                "e2e/ui/**/*.{[sS][pP][eE][cC],[tT][eE][sS][tT]}.{[tT][sS],[tT][sS][xX],[jJ][sS],[jJ][sS][xX],[mM][tT][sS],[cC][tT][sS],[mM][jJ][sS],[cC][jJ][sS]}"
            ]
        );
    }

    #[test]
    fn test_dir_dot_selects_the_config_dir() {
        let result = resolve_at(
            "/project/e2e/playwright.config.ts",
            r"export default { testDir: '.' };",
        );
        assert_eq!(
            test_dir_patterns(&result),
            vec![
                "e2e/**/*.{[sS][pP][eE][cC],[tT][eE][sS][tT]}.{[tT][sS],[tT][sS][xX],[jJ][sS],[jJ][sS][xX],[mM][tT][sS],[cC][tT][sS],[mM][jJ][sS],[cC][jJ][sS]}"
            ]
        );
    }

    #[test]
    fn test_dir_with_dirname_join() {
        let result = resolve(
            r"
            import path from 'node:path';
            export default { testDir: path.join(__dirname, 'specs') };
        ",
        );
        assert_eq!(
            test_dir_patterns(&result),
            vec![
                "specs/**/*.{[sS][pP][eE][cC],[tT][eE][sS][tT]}.{[tT][sS],[tT][sS][xX],[jJ][sS],[jJ][sS][xX],[mM][tT][sS],[cC][tT][sS],[mM][jJ][sS],[cC][jJ][sS]}"
            ]
        );
    }

    #[test]
    fn project_test_dirs_each_give_entries() {
        let result = resolve(
            r"export default defineConfig({
                projects: [{ testDir: './a' }, { testDir: './b' }],
            });",
        );
        assert_eq!(
            test_dir_patterns(&result),
            vec![
                "a/**/*.{[sS][pP][eE][cC],[tT][eE][sS][tT]}.{[tT][sS],[tT][sS][xX],[jJ][sS],[jJ][sS][xX],[mM][tT][sS],[cC][tT][sS],[mM][jJ][sS],[cC][jJ][sS]}",
                "b/**/*.{[sS][pP][eE][cC],[tT][eE][sS][tT]}.{[tT][sS],[tT][sS][xX],[jJ][sS],[jJ][sS][xX],[mM][tT][sS],[cC][tT][sS],[mM][jJ][sS],[cC][jJ][sS]}",
            ]
        );
    }

    #[test]
    fn project_inherits_top_level_test_dir_and_test_match() {
        let result = resolve(
            r"export default defineConfig({
                testDir: './tests',
                testMatch: '*.e2e.ts',
                projects: [{ name: 'chromium' }, { testDir: './smoke' }],
            });",
        );
        assert_eq!(
            test_dir_patterns(&result),
            vec![
                "tests/**/*.[eE]2[eE].[tT][sS]",
                "smoke/**/*.[eE]2[eE].[tT][sS]"
            ]
        );
    }

    #[test]
    fn glob_test_match_applies_below_test_dir() {
        let result = resolve(
            r"export default {
                testDir: './ui',
                testMatch: ['**/*.flow.ts', '*.check.ts'],
            };",
        );
        assert_eq!(
            test_dir_patterns(&result),
            vec![
                "ui/**/*.[fF][lL][oO][wW].[tT][sS]",
                "ui/**/*.[cC][hH][eE][cC][kK].[tT][sS]"
            ]
        );
    }

    #[test]
    fn test_match_with_test_dir_name_keeps_files_in_test_dir() {
        let result = resolve(
            r"export default {
                testDir: './tests',
                testMatch: 'tests/**/*.e2e.ts',
            };",
        );
        let patterns = test_dir_patterns(&result);
        assert_eq!(patterns, vec!["tests/**/*.{ts,tsx,js,jsx,mts,cts,mjs,cjs}"]);
        let matcher = globset::Glob::new(patterns[0]).unwrap().compile_matcher();
        assert!(matcher.is_match("tests/login.e2e.ts"));
    }

    #[test]
    fn test_match_with_directory_part_keeps_every_script_below_test_dir() {
        let result = resolve(
            r"export default {
                testDir: './e2e',
                testMatch: ['**/*.flow.ts', 'e2e/*.pw.ts'],
            };",
        );
        assert_eq!(
            test_dir_patterns(&result),
            vec!["e2e/**/*.{ts,tsx,js,jsx,mts,cts,mjs,cjs}"]
        );
    }

    #[test]
    fn extglob_test_match_keeps_every_script_below_test_dir() {
        let result = resolve(
            r"export default {
                testDir: './e2e',
                testMatch: '**/*.@(e2e|smoke).ts',
            };",
        );
        assert_eq!(
            test_dir_patterns(&result),
            vec!["e2e/**/*.{ts,tsx,js,jsx,mts,cts,mjs,cjs}"]
        );
    }

    #[test]
    fn extglob_in_one_test_match_glob_keeps_every_script_below_test_dir() {
        let result = resolve(
            r"export default {
                testDir: './e2e',
                testMatch: ['**/*.flow.ts', '**/*.@(spec|test).?(c|m)[jt]s?(x)'],
            };",
        );
        assert_eq!(
            test_dir_patterns(&result),
            vec!["e2e/**/*.{ts,tsx,js,jsx,mts,cts,mjs,cjs}"]
        );
    }

    #[test]
    fn regex_test_match_keeps_every_script_below_test_dir() {
        let result = resolve(
            r"export default {
                testDir: './ui',
                projects: [
                    { name: 'setup', testMatch: /global\.setup\.ts/ },
                    { name: 'chromium' },
                ],
            };",
        );
        assert_eq!(
            test_dir_patterns(&result),
            vec![
                "ui/**/*.{ts,tsx,js,jsx,mts,cts,mjs,cjs}",
                "ui/**/*.{[sS][pP][eE][cC],[tT][eE][sS][tT]}.{[tT][sS],[tT][sS][xX],[jJ][sS],[jJ][sS][xX],[mM][tT][sS],[cC][tT][sS],[mM][jJ][sS],[cC][jJ][sS]}",
            ]
        );
    }

    #[test]
    fn missing_test_dir_restates_default_entries() {
        let result = resolve(r"export default defineConfig({ testMatch: '*.e2e.ts' });");
        assert!(result.replace_entry_patterns);
        assert_eq!(entry_patterns(&result), default_patterns());
    }

    #[test]
    fn project_without_test_dir_restates_default_entries() {
        let result = resolve(
            r"export default defineConfig({
                projects: [{ testDir: './a' }, { name: 'chromium' }],
            });",
        );
        assert_eq!(
            test_dir_patterns(&result),
            [
                vec!["a/**/*.{[sS][pP][eE][cC],[tT][eE][sS][tT]}.{[tT][sS],[tT][sS][xX],[jJ][sS],[jJ][sS][xX],[mM][tT][sS],[cC][tT][sS],[mM][jJ][sS],[cC][jJ][sS]}"],
                DEFAULT_TEST_ENTRY_PATTERNS[TEST_FILE_NAME_PATTERNS.len()..].to_vec(),
            ]
            .concat()
        );
    }

    #[test]
    fn dynamic_test_dir_restates_default_entries() {
        let result = resolve(r"export default { testDir: process.env.TEST_DIR };");
        assert_eq!(entry_patterns(&result), default_patterns());
    }

    #[test]
    fn test_dir_outside_root_restates_default_entries() {
        let result = resolve(r"export default { testDir: '../outside' };");
        assert_eq!(entry_patterns(&result), default_patterns());
    }

    #[test]
    fn config_overrides_keep_the_selected_files() {
        for (source, selected, rejected) in [
            (
                "export default { testDir: './wrong', testDir: './ui', testMatch: '*.pw.ts' };",
                "ui/live.pw.ts",
                "wrong/live.pw.ts",
            ),
            (
                "export default { testDir: './ui', testMatch: '*.wrong.ts', testMatch: '*.pw.ts' };",
                "ui/live.pw.ts",
                "ui/live.wrong.ts",
            ),
            (
                "export default { projects: [{testDir:'./wrong'}], projects: [{testDir:'./ui', testMatch:'*.pw.ts'}] };",
                "ui/live.pw.ts",
                "wrong/live.pw.ts",
            ),
            (
                "export default { projects: [{...shared, testDir:'./ui'}] };",
                "ui/live.pw.ts",
                "outside/live.pw.ts",
            ),
            (
                "export default {...shared, testDir:'./ui', testMatch:'*.pw.ts'};",
                "ui/live.pw.ts",
                "outside/live.pw.ts",
            ),
            (
                "export default {testDir:'./ui', testMatch:'*.pw.ts', projects:[...sharedProjects]};",
                "ui/live.pw.ts",
                "outside/live.pw.ts",
            ),
            (
                "export default {testDir:'./ui',testMatch:'*.wrong.ts',projects:sharedProjects};",
                "ui/live.pw.ts",
                "outside/live.pw.ts",
            ),
            (
                "export default {projects:[{testDir:'./ui',testMatch:'*.pw.ts'},...sharedProjects]};",
                "ui/live.pw.ts",
                "outside/live.pw.ts",
            ),
            (
                "export default {testDir:'./ui',testMatch:'*.pw.ts',projects:[{name:'chromium',...devices['Desktop Chrome']}]};",
                "ui/live.pw.ts",
                "outside/live.pw.ts",
            ),
            (
                "export default {testDir:'./ui',testMatch:'*.wrong.ts',projects:[{...shared}]};",
                "ui/live.pw.ts",
                "outside/live.pw.ts",
            ),
            (
                "export default { testDir:'./ui', [key]:'./tests', testMatch:'*.pw.ts', projects:[{name:'chromium'}] };",
                "tests/live.pw.ts",
                "outside/live.pw.ts",
            ),
        ] {
            let result = resolve(source);
            let matchers: Vec<_> = result
                .entry_patterns
                .iter()
                .map(|rule| {
                    globset::GlobBuilder::new(&rule.pattern)
                        .literal_separator(true)
                        .build()
                        .unwrap()
                        .compile_matcher()
                })
                .collect();
            assert!(
                matchers.iter().any(|matcher| matcher.is_match(selected)),
                "the config selects {selected}: {source}"
            );
            assert!(
                !matchers.iter().any(|matcher| matcher.is_match(rejected)),
                "the config does not select {rejected}: {source}"
            );
        }
    }

    #[test]
    fn global_setup_resolves_from_nested_config_dir() {
        let source = r#"
            export default {
                globalSetup: "./setup.ts"
            };
        "#;
        let result = resolve_at("/project/apps/web/playwright.config.ts", source);
        assert_eq!(
            result.setup_files,
            vec![abs("/project/apps/web/setup.ts")],
            "globalSetup must resolve under the config directory"
        );
    }
}
