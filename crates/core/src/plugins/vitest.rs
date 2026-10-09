//! Vitest test runner plugin.
//!
//! Detects Vitest projects and marks test/bench files as entry points.
//! Parses vitest.config to extract test.include, setupFiles, globalSetup,
//! and custom test environments as referenced dependencies.

use std::path::{Component, Path, PathBuf};

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
        // Vitest does not load a vite config that a vitest config shadows, so
        // the JSX settings of that vite config apply to no test file.
        if !is_shadowed_vite_config(config_path) {
            // The Vitest version matters only for inline projects, so the
            // disk reads run only for a config that names `projects`.
            let jsx = JsxContext {
                inherit_by_default: !source.contains("projects")
                    || vitest_inherits_by_default(config_path, root),
            };
            add_vitest_jsx_import_sources(&mut result, source, config_path, jsx);
            add_vitest_project_config_files(&mut result, source, config_path, jsx);
        }

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

/// The Vitest default `test.exclude`. A config `exclude` replaces it.
const VITEST_DEFAULT_EXCLUDE: &[&str] = &["**/node_modules/**", "**/.git/**"];

/// The package that Vite imports the JSX runtime from when a config sets no
/// import source.
const DEFAULT_JSX_IMPORT_SOURCE: &str = "react";

/// The first Vitest major in which an inline project without `extends`
/// merges the declaring config.
const VITEST_INHERIT_BY_DEFAULT_MAJOR: u64 = 5;

/// The file names that make Vitest ignore a sibling `vite.config.*`.
const VITEST_CONFIG_NAMES: &[&str] = &[
    "vitest.config.ts",
    "vitest.config.js",
    "vitest.config.mts",
    "vitest.config.mjs",
    "vitest.config.cts",
    "vitest.config.cjs",
];

/// Limits for the expansion of a glob entry in `test.projects`: the number of
/// matches inspected, and the number of project config files read.
const MAX_PROJECT_GLOB_MATCHES: usize = 1_000;
const MAX_PROJECT_GLOB_CONFIGS: usize = 64;

/// The facts outside the config file that change how the plugin reads the
/// JSX settings of test projects.
#[derive(Clone, Copy)]
struct JsxContext {
    /// Whether an inline project without `extends` merges the declaring
    /// config. Vitest 5 does this, Vitest 4 and older do not.
    inherit_by_default: bool,
}

/// Whether `config_path` is a `vite.config.*` that Vitest does not load,
/// because a `vitest.config.*` is next to it.
///
/// A vitest config that imports the vite config, for example to pass it to
/// `mergeConfig`, gets its JSX settings. Then the vite config still applies.
fn is_shadowed_vite_config(config_path: &Path) -> bool {
    let (Some(name), Some(config_dir)) = (
        config_path.file_name().and_then(|name| name.to_str()),
        config_path.parent(),
    ) else {
        return false;
    };
    if !name.starts_with("vite.config.") {
        return false;
    }
    let Some(vitest_config) = VITEST_CONFIG_NAMES
        .iter()
        .map(|name| config_dir.join(name))
        .find(|path| path.is_file())
    else {
        return false;
    };
    let Ok(vitest_source) = std::fs::read_to_string(&vitest_config) else {
        return true;
    };
    !config_parser::extract_imports(&vitest_source, &vitest_config)
        .iter()
        .any(|specifier| imports_vite_config(specifier, config_dir))
}

/// Whether an import specifier of a config in `config_dir` names the
/// `vite.config.*` of that directory.
fn imports_vite_config(specifier: &str, config_dir: &Path) -> bool {
    if !specifier.starts_with('.') {
        return false;
    }
    let target = lexical_join(config_dir, specifier);
    target.parent() == Some(config_dir)
        && target
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name == "vite.config" || name.starts_with("vite.config."))
}

/// Whether the Vitest version of the project that owns `config_path` merges
/// the declaring config into an inline project without `extends`.
///
/// The installed version wins over the declared range. When neither gives a
/// major, the plugin follows Vitest 5.
fn vitest_inherits_by_default(config_path: &Path, root: &Path) -> bool {
    let dirs: Vec<&Path> = config_path
        .ancestors()
        .skip(1)
        .take_while(|dir| dir.starts_with(root))
        .collect();
    let major = dirs
        .iter()
        .find_map(|dir| installed_vitest_major(dir))
        .or_else(|| dirs.iter().find_map(|dir| declared_vitest_major(dir)));
    major.is_none_or(|major| major >= VITEST_INHERIT_BY_DEFAULT_MAJOR)
}

fn installed_vitest_major(dir: &Path) -> Option<u64> {
    let manifest = dir.join("node_modules/vitest/package.json");
    let source = std::fs::read_to_string(manifest).ok()?;
    let value: serde_json::Value = serde_json::from_str(&source).ok()?;
    leading_major(value.get("version")?.as_str()?)
}

fn declared_vitest_major(dir: &Path) -> Option<u64> {
    let pkg = fallow_config::PackageJson::load(&dir.join("package.json")).ok()?;
    let range = [
        &pkg.dependencies,
        &pkg.dev_dependencies,
        &pkg.peer_dependencies,
        &pkg.optional_dependencies,
    ]
    .into_iter()
    .flatten()
    .find_map(|deps| deps.get("vitest"))?;
    range_max_major(range)
}

/// The highest major that a version range admits, or `None` when the range
/// has no upper major, such as `>=4`, `*`, `latest` or `catalog:`.
fn range_max_major(range: &str) -> Option<u64> {
    let range = range.strip_prefix("npm:vitest@").unwrap_or(range);
    let mut max = None;
    for alternative in range.split("||") {
        let alternative = alternative.trim();
        if alternative.starts_with('>') {
            return None;
        }
        let major = leading_major(alternative)?;
        max = max.max(Some(major));
    }
    max
}

/// The major of a version or of a `^`, `~`, `=` or `v` prefixed range.
fn leading_major(version: &str) -> Option<u64> {
    let digits: String = version
        .trim_start_matches(['^', '~', '=', 'v', ' '])
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    digits.parse().ok()
}

/// The JSX transform settings of one config object (`oxc.jsx` or the older
/// `esbuild` form).
#[derive(Clone, Default)]
struct JsxTransform {
    /// `Some(false)` when the transform imports no runtime (classic runtime,
    /// `preserve`, `oxc: false`, or esbuild `transform`). `None` when the
    /// config is silent.
    automatic: Option<bool>,
    import_source: Option<String>,
}

/// The runtime that the JSX transform of one test project imports.
enum JsxRuntime {
    /// The transform adds no runtime import.
    None,
    /// The config sets no import source, so Vite uses the `react` runtime.
    Default,
    /// The config sets this import source.
    Source(String),
}

impl JsxTransform {
    /// Read the settings of one config object. `oxc` wins over `esbuild`,
    /// because Vite maps the older `esbuild` options to `oxc`.
    fn read(config: &ObjectExpression<'_>) -> Self {
        if let Some(oxc) = config_parser::property_expr(config, "oxc") {
            if matches!(oxc, Expression::BooleanLiteral(value) if !value.value) {
                // `oxc: false` turns the transform off, so no runtime import
                // is added.
                return Self {
                    automatic: Some(false),
                    import_source: None,
                };
            }
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
    /// config that it extends.
    fn over(self, base: &Self) -> Self {
        Self {
            automatic: self.automatic.or(base.automatic),
            import_source: self.import_source.or_else(|| base.import_source.clone()),
        }
    }

    fn runtime(self) -> JsxRuntime {
        if self.automatic == Some(false) {
            return JsxRuntime::None;
        }
        match self.import_source {
            None => JsxRuntime::Default,
            Some(source) if source.is_empty() => JsxRuntime::None,
            Some(source) => JsxRuntime::Source(source),
        }
    }
}

/// The settings of one config object that decide which files a test project
/// transforms with which JSX runtime.
#[derive(Clone, Default)]
struct ProjectScope {
    jsx: JsxTransform,
    /// The `test.include` patterns as written. Empty when the config sets
    /// none.
    include: Vec<String>,
    /// The `test.exclude` patterns as written. Empty when the config sets
    /// none.
    exclude: Vec<String>,
    /// `test.root`, else the Vite `root`.
    root: Option<String>,
    /// `test.dir`, relative to the root.
    dir: Option<String>,
}

impl ProjectScope {
    fn read(config: &ObjectExpression<'_>) -> Self {
        let test = config_parser::property_object(config, "test");
        let patterns = |key: &str| {
            test.and_then(|test| config_parser::property_expr(test, key))
                .map(config_parser::expression_to_string_or_array)
                .unwrap_or_default()
        };
        Self {
            jsx: JsxTransform::read(config),
            include: patterns("include"),
            exclude: patterns("exclude"),
            root: test
                .and_then(|test| config_parser::property_string(test, "root"))
                .or_else(|| config_parser::property_string(config, "root")),
            dir: test.and_then(|test| config_parser::property_string(test, "dir")),
        }
    }

    /// Apply `self` over `base`, as Vite `mergeConfig` merges a project config
    /// over the config that it extends: arrays concatenate with the base
    /// values first, and other values of `self` replace the base values.
    fn over(self, base: &Self) -> Self {
        let concat = |base: &[String], own: Vec<String>| {
            let mut merged = base.to_vec();
            merged.extend(own);
            merged
        };
        Self {
            jsx: self.jsx.over(&base.jsx),
            include: concat(&base.include, self.include),
            exclude: concat(&base.exclude, self.exclude),
            root: self.root.or_else(|| base.root.clone()),
            dir: self.dir.or_else(|| base.dir.clone()),
        }
    }

    /// The include and exclude globs, relative to `config_dir`, of the files
    /// that this project transforms. `None` when the project directory is
    /// outside `config_dir`, because no rule glob can reach those files.
    fn globs(&self, config_dir: &Path) -> Option<(Vec<String>, Vec<String>)> {
        let mut base = lexical_join(config_dir, self.root.as_deref().unwrap_or("."));
        if let Some(dir) = &self.dir {
            base = lexical_join(&base, dir);
        }
        // A glob needs forward slashes, also on Windows.
        let prefix = config_parser::path_to_config_string(base.strip_prefix(config_dir).ok()?);
        let scoped = |pattern: &str| scoped_glob(&prefix, pattern, config_dir);

        // Vitest reads a negated include entry as an exclude.
        let (negated, positive): (Vec<&String>, Vec<&String>) = self
            .include
            .iter()
            .partition(|pattern| pattern.starts_with('!'));
        let mut include: Vec<String> = positive
            .iter()
            .filter_map(|pattern| scoped(pattern.as_str()))
            .collect();
        if positive.is_empty() {
            include.extend(scoped(VITEST_DEFAULT_INCLUDE));
        }

        // A config exclude replaces the default exclude. Vitest ignores a
        // negated exclude entry.
        let exclude_patterns: Vec<&str> = if self.exclude.is_empty() {
            VITEST_DEFAULT_EXCLUDE.to_vec()
        } else {
            self.exclude
                .iter()
                .map(String::as_str)
                .filter(|pattern| !pattern.starts_with('!'))
                .collect()
        };
        let mut exclude = Vec::new();
        for pattern in exclude_patterns
            .into_iter()
            .chain(negated.iter().map(|pattern| &pattern[1..]))
        {
            let Some(glob) = scoped(pattern) else {
                continue;
            };
            // An exclude pattern that names a directory also excludes the
            // files in it.
            let trimmed = glob.trim_end_matches('/');
            exclude.push(trimmed.to_string());
            if !trimmed.ends_with("/**") && trimmed != "**" {
                exclude.push(format!("{trimmed}/**"));
            }
        }
        Some((include, exclude))
    }
}

/// A Vitest include or exclude pattern as a glob relative to `config_dir`.
/// `prefix` is the project directory relative to `config_dir`. An absolute
/// pattern outside `config_dir` matches no project file, so it gives `None`.
fn scoped_glob(prefix: &str, pattern: &str, config_dir: &Path) -> Option<String> {
    if pattern.starts_with('/') {
        let relative = Path::new(pattern).strip_prefix(config_dir).ok()?;
        return Some(vitest_include_glob(&config_parser::path_to_config_string(
            relative,
        )));
    }
    let glob = vitest_include_glob(pattern);
    if prefix.is_empty() {
        return Some(glob);
    }
    Some(format!("{prefix}/{glob}"))
}

/// Join a relative path to a directory and remove `.` and `..` segments. An
/// absolute `relative` replaces `dir`.
fn lexical_join(dir: &Path, relative: &str) -> PathBuf {
    let mut joined = dir.to_path_buf();
    for component in Path::new(relative).components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                joined.pop();
            }
            other => joined.push(other.as_os_str()),
        }
    }
    joined
}

/// What the `extends` value of an inline project merges into it.
enum ProjectBase {
    /// The declaring config. Vitest 5 merges it unless `extends` is `false`
    /// or a path. Vitest 4 merges it only with `extends: true`.
    DeclaringConfig,
    /// Nothing: `extends: false`.
    None,
    /// The config file at this path.
    File(PathBuf),
}

fn project_base(
    project: &ObjectExpression<'_>,
    config_path: &Path,
    jsx: JsxContext,
) -> ProjectBase {
    let Some(extends) = config_parser::property_expr(project, "extends") else {
        return if jsx.inherit_by_default {
            ProjectBase::DeclaringConfig
        } else {
            ProjectBase::None
        };
    };
    if let Expression::BooleanLiteral(value) = extends {
        return if value.value {
            ProjectBase::DeclaringConfig
        } else {
            ProjectBase::None
        };
    }
    let (Some(path), Some(config_dir)) = (
        config_parser::expression_to_string(extends),
        config_path.parent(),
    ) else {
        return ProjectBase::DeclaringConfig;
    };
    let path = lexical_join(config_dir, &path);
    // A path to the declaring config is the same as `extends: true`.
    if path == lexical_join(config_dir, &config_path.to_string_lossy()) {
        return ProjectBase::DeclaringConfig;
    }
    ProjectBase::File(path)
}

/// The scope of each test project that a config declares, and the config
/// files that inline projects extend.
///
/// Without `test.projects`, the config itself is the one test project. With
/// it, each inline project merges the declaring config into its own values,
/// as Vitest 5 does, unless `extends` is `false` or a path. On Vitest 4, a
/// project merges it only with `extends: true`. A path names a config file
/// that the project merges instead. A string entry names a project config
/// file, which the plugin reads on its own.
fn collect_project_scopes(
    root: &ObjectExpression<'_>,
    config_path: &Path,
    jsx: JsxContext,
) -> (Vec<ProjectScope>, Vec<PathBuf>) {
    let root_scope = ProjectScope::read(root);
    let projects = config_parser::property_object(root, "test")
        .and_then(|test| config_parser::property_expr(test, "projects"))
        .and_then(config_parser::array_expression);
    let Some(projects) = projects else {
        return (vec![root_scope], Vec::new());
    };
    let mut scopes = Vec::new();
    let mut extended_files = Vec::new();
    for project in projects
        .elements
        .iter()
        .filter_map(|element| element.as_expression())
        .filter_map(config_parser::object_expression)
    {
        let own = ProjectScope::read(project);
        let scope = match project_base(project, config_path, jsx) {
            ProjectBase::DeclaringConfig => own.over(&root_scope),
            ProjectBase::None => own,
            ProjectBase::File(path) => match read_config_scope(&path) {
                Some(base) => {
                    extended_files.push(path);
                    own.over(&base)
                }
                None => own,
            },
        };
        scopes.push(scope);
    }
    (scopes, extended_files)
}

/// Read the scope of the config file that an inline project extends. A Vite
/// config has no top-level `extends`, so one level is enough.
fn read_config_scope(path: &Path) -> Option<ProjectScope> {
    let source = std::fs::read_to_string(path).ok()?;
    config_parser::extract_from_source(&source, path, |program| {
        config_parser::find_config_object(program).map(ProjectScope::read)
    })
}

/// Record the JSX runtime of each test project that the config declares,
/// with the files that the project transforms.
///
/// A relative import source gives a graph edge rule. A package import source
/// credits the package. A config without an import source gives a `react`
/// credit rule, which the analysis applies only when a matching file has
/// JSX.
fn add_vitest_jsx_import_sources(
    result: &mut PluginResult,
    source: &str,
    config_path: &Path,
    jsx: JsxContext,
) {
    let Some(config_dir) = config_path.parent() else {
        return;
    };
    let Some((scopes, extended_files)) =
        config_parser::extract_from_source(source, config_path, |program| {
            config_parser::find_config_object(program)
                .map(|config| collect_project_scopes(config, config_path, jsx))
        })
    else {
        return;
    };
    for path in extended_files {
        if !result.setup_files.contains(&path) {
            result.setup_files.push(path);
        }
    }
    for scope in scopes {
        let Some((include, exclude)) = scope.globs(config_dir) else {
            continue;
        };
        let rule = |source: String| JsxImportSourceRule {
            source,
            config_dir: config_dir.to_path_buf(),
            include: include.clone(),
            exclude: exclude.clone(),
        };
        match scope.jsx.runtime() {
            JsxRuntime::None => {}
            JsxRuntime::Default => result
                .jsx_package_credits
                .push(rule(DEFAULT_JSX_IMPORT_SOURCE.to_string())),
            // A package runtime has no project file to reach, so it only
            // credits the package. A graph edge from test files alone would
            // also make a runtime dependency of the app look test-only.
            JsxRuntime::Source(source) if !is_path_source(&source) => result
                .referenced_dependencies
                .push(crate::resolve::extract_package_name(&source)),
            JsxRuntime::Source(source) => result.jsx_import_sources.push(rule(source)),
        }
    }
}

/// Read the project config files that `test.projects` names by path or by
/// glob.
///
/// Vitest loads such a file as a project config with its own root, and no
/// import shows that use. The config patterns already find a file with a
/// standard name, such as `vitest.config.ts`. A file with another name, such
/// as `vitest.e2e.config.ts`, is read here: the file is credited and its JSX
/// rules apply. A glob entry is expanded on disk. A glob with a brace group
/// is not followed, because the glob matcher has no brace support.
fn add_vitest_project_config_files(
    result: &mut PluginResult,
    source: &str,
    config_path: &Path,
    jsx: JsxContext,
) {
    let Some(config_dir) = config_path.parent() else {
        return;
    };
    let entries =
        config_parser::extract_config_string_or_array(source, config_path, &["test", "projects"]);
    for entry in entries {
        for path in project_config_paths(&entry, config_dir) {
            let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            if !is_vitest_config_name(name)
                || name.starts_with("vitest.config.")
                || name.starts_with("vite.config.")
            {
                continue;
            }
            let Ok(project_source) = std::fs::read_to_string(&path) else {
                continue;
            };
            add_vitest_jsx_import_sources(result, &project_source, &path, jsx);
            if !result.setup_files.contains(&path) {
                result.setup_files.push(path);
            }
        }
    }
}

/// The files that one `test.projects` string entry names. A plain path gives
/// itself. A glob gives the files that match it, without `node_modules`.
fn project_config_paths(entry: &str, config_dir: &Path) -> Vec<PathBuf> {
    // A glob that starts with `**` walks the whole tree, `node_modules`
    // included, on each run. Vitest configs name project directories, so
    // such an entry is not followed.
    let relative = entry.strip_prefix("./").unwrap_or(entry);
    if entry.contains('{') || relative.starts_with("**") {
        return Vec::new();
    }
    if !entry.contains(['*', '?', '[']) {
        return vec![lexical_join(config_dir, entry)];
    }
    let Some(dir) = config_dir.to_str() else {
        return Vec::new();
    };
    let relative = entry.strip_prefix("./").unwrap_or(entry);
    let pattern = format!("{}/{relative}", glob::Pattern::escape(dir));
    let Ok(matches) = glob::glob(&pattern) else {
        return Vec::new();
    };
    matches
        .flatten()
        .take(MAX_PROJECT_GLOB_MATCHES)
        .filter(|path| {
            path.is_file()
                && !path
                    .components()
                    .any(|component| component.as_os_str() == "node_modules")
        })
        .take(MAX_PROJECT_GLOB_CONFIGS)
        .collect()
}

/// Whether a file name matches the Vitest project config pattern
/// `vite(st)(.<name>).config.<ext>`.
fn is_vitest_config_name(name: &str) -> bool {
    let Some(rest) = name
        .strip_prefix("vitest.")
        .or_else(|| name.strip_prefix("vite."))
    else {
        return false;
    };
    if rest.starts_with("config.") {
        return true;
    }
    rest.split_once(".config.").is_some_and(|(middle, _)| {
        !middle.is_empty()
            && middle
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    })
}

/// Whether a JSX import source names a path, not a package.
fn is_path_source(source: &str) -> bool {
    source.starts_with('.') || source.starts_with('/')
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
                    vec![
                        "src/**/*.spec.ts".to_string(),
                        "src/**/*.{spec,test}.{ts,tsx,js}".to_string()
                    ]
                ),
                (
                    "./src/jsx/dom".to_string(),
                    vec!["src/**/*.spec.ts".to_string()]
                ),
            ]
        );
    }

    #[test]
    fn inline_project_inherits_root_jsx_unless_extends_is_false() {
        let source = r"
            export default defineConfig({
                oxc: { jsx: { importSource: './jsx' } },
                test: {
                    projects: [
                        { extends: true, test: { include: ['a/**/*.test.tsx'] } },
                        { test: { include: ['b/**/*.test.tsx'] } },
                        { extends: false, test: { include: ['c/**/*.test.tsx'] } },
                        {
                            extends: false,
                            oxc: { jsx: { importSource: './own' } },
                            test: { include: ['d/**/*.test.tsx'] },
                        },
                        {
                            oxc: { jsx: { runtime: 'automatic' } },
                            test: { include: ['e/**/*.test.tsx'] },
                        },
                    ],
                },
            });
        ";
        assert_eq!(
            jsx_rules(source),
            vec![
                ("./jsx".to_string(), vec!["a/**/*.test.tsx".to_string()]),
                ("./jsx".to_string(), vec!["b/**/*.test.tsx".to_string()]),
                ("./own".to_string(), vec!["d/**/*.test.tsx".to_string()]),
                ("./jsx".to_string(), vec!["e/**/*.test.tsx".to_string()]),
            ]
        );
    }

    #[test]
    fn inherited_include_concatenates_and_extends_false_uses_the_default() {
        let source = r"
            export default defineConfig({
                oxc: { jsx: { importSource: './jsx' } },
                test: {
                    include: ['src/**/*.test.tsx'],
                    projects: [
                        { test: { include: ['pkg/**/*.test.tsx'] } },
                        { extends: true },
                        { extends: false },
                        { extends: false, oxc: { jsx: { importSource: './own' } } },
                    ],
                },
            });
        ";
        assert_eq!(
            jsx_rules(source),
            vec![
                (
                    "./jsx".to_string(),
                    vec![
                        "src/**/*.test.tsx".to_string(),
                        "pkg/**/*.test.tsx".to_string()
                    ]
                ),
                ("./jsx".to_string(), vec!["src/**/*.test.tsx".to_string()]),
                (
                    "./own".to_string(),
                    vec![VITEST_DEFAULT_INCLUDE.to_string()]
                ),
            ]
        );
    }

    #[test]
    fn include_is_relative_to_the_test_dir_or_the_project_root() {
        let source = r"
            export default defineConfig({
                oxc: { jsx: { importSource: './jsx' } },
                test: {
                    projects: [
                        { test: { root: 'pkg', include: ['src/**/*.test.tsx'] } },
                        { root: './app', test: { dir: 'tests' } },
                    ],
                },
            });
        ";
        assert_eq!(
            jsx_rules(source),
            vec![
                (
                    "./jsx".to_string(),
                    vec!["pkg/src/**/*.test.tsx".to_string()]
                ),
                (
                    "./jsx".to_string(),
                    vec![format!("app/tests/{VITEST_DEFAULT_INCLUDE}")]
                ),
            ]
        );
    }

    #[test]
    fn extends_path_reads_the_named_config_and_credits_it() {
        let temp = tempfile::tempdir().expect("tempdir");
        let config_dir = temp.path();
        std::fs::write(
            config_dir.join("base.config.ts"),
            "export default defineConfig({ oxc: { jsx: { importSource: './base-jsx' } }, test: { include: ['base/**/*.test.tsx'] } });",
        )
        .expect("write base config");
        let config_path = config_dir.join("vitest.config.ts");
        let source = r"
            export default defineConfig({
                oxc: { jsx: { importSource: './root-jsx' } },
                test: {
                    projects: [
                        { extends: './base.config.ts', test: { include: ['e/**/*.test.tsx'] } },
                        { extends: './vitest.config.ts', test: { include: ['s/**/*.test.tsx'] } },
                        { extends: './missing.config.ts', test: { include: ['m/**/*.test.tsx'] } },
                    ],
                },
            });
        ";
        let result = VitestPlugin.resolve_config(&config_path, source, config_dir);
        let rules: Vec<(String, Vec<String>)> = result
            .jsx_import_sources
            .iter()
            .map(|rule| (rule.source.clone(), rule.include.clone()))
            .collect();
        assert_eq!(
            rules,
            vec![
                (
                    "./base-jsx".to_string(),
                    vec![
                        "base/**/*.test.tsx".to_string(),
                        "e/**/*.test.tsx".to_string()
                    ]
                ),
                (
                    "./root-jsx".to_string(),
                    vec!["s/**/*.test.tsx".to_string()]
                ),
            ]
        );
        assert!(
            result
                .setup_files
                .contains(&config_dir.join("base.config.ts")),
            "the extended config file must be credited: {:?}",
            result.setup_files
        );
    }

    fn jsx_excludes(source: &str) -> Vec<Vec<String>> {
        VitestPlugin
            .resolve_config(
                std::path::Path::new("/project/vitest.config.ts"),
                source,
                std::path::Path::new("/project"),
            )
            .jsx_import_sources
            .into_iter()
            .map(|rule| rule.exclude)
            .collect()
    }

    fn strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_string()).collect()
    }

    #[test]
    fn exclude_defaults_to_the_vitest_default_and_a_config_exclude_replaces_it() {
        let source = r"
            export default defineConfig({
                oxc: { jsx: { importSource: './jsx' } },
                test: {
                    projects: [
                        { test: { name: 'default' } },
                        { test: { exclude: ['./src/skip', 'src/one.test.tsx', '!keep/**'] } },
                        { test: { exclude: ['/project/abs/**', '/elsewhere/**'] } },
                        { test: { include: ['src/**/*.test.tsx', '!src/gen/**'] } },
                    ],
                },
            });
        ";
        assert_eq!(
            jsx_excludes(source),
            vec![
                strings(&["**/node_modules/**", "**/.git/**"]),
                strings(&[
                    "src/skip",
                    "src/skip/**",
                    "src/one.test.tsx",
                    "src/one.test.tsx/**",
                ]),
                strings(&["abs/**"]),
                strings(&["**/node_modules/**", "**/.git/**", "src/gen/**"]),
            ]
        );
    }

    #[test]
    fn inherited_exclude_concatenates_and_follows_the_project_root() {
        let source = r"
            export default defineConfig({
                oxc: { jsx: { importSource: './jsx' } },
                test: {
                    exclude: ['skip/**'],
                    projects: [
                        { test: { exclude: ['other/**'] } },
                        { test: { root: 'pkg' } },
                        { extends: false, oxc: { jsx: { importSource: './jsx' } } },
                    ],
                },
            });
        ";
        assert_eq!(
            jsx_excludes(source),
            vec![
                strings(&["skip/**", "other/**"]),
                strings(&["pkg/skip/**"]),
                strings(&["**/node_modules/**", "**/.git/**"]),
            ]
        );
    }

    fn jsx_credits(path: &str, source: &str) -> Vec<(String, Vec<String>)> {
        VitestPlugin
            .resolve_config(
                std::path::Path::new(path),
                source,
                std::path::Path::new("/project"),
            )
            .jsx_package_credits
            .into_iter()
            .map(|rule| (rule.source, rule.include))
            .collect()
    }

    #[test]
    fn config_without_import_source_credits_the_default_react_runtime() {
        let source = r"
            export default defineConfig({
                test: {
                    projects: [
                        { test: { include: ['a/**/*.test.tsx'] } },
                        { oxc: false, test: { include: ['b/**/*.test.tsx'] } },
                        { oxc: { jsx: 'preserve' }, test: { include: ['c/**/*.test.tsx'] } },
                        {
                            oxc: { jsx: { runtime: 'classic' } },
                            test: { include: ['d/**/*.test.tsx'] },
                        },
                        {
                            oxc: { jsx: { importSource: './jsx' } },
                            test: { include: ['e/**/*.test.tsx'] },
                        },
                    ],
                },
            });
        ";
        assert_eq!(
            jsx_credits("/project/vitest.config.ts", source),
            vec![("react".to_string(), strings(&["a/**/*.test.tsx"]))]
        );
        assert_eq!(
            jsx_credits(
                "/project/vitest.config.ts",
                "export default defineConfig({ test: {} });"
            ),
            vec![(
                "react".to_string(),
                vec![VITEST_DEFAULT_INCLUDE.to_string()]
            )]
        );
        assert!(
            jsx_credits(
                "/project/vitest.config.ts",
                "export default defineConfig({ oxc: false, test: {} });"
            )
            .is_empty()
        );
    }

    #[test]
    fn project_config_files_named_by_path_are_credited() {
        let temp = tempfile::tempdir().expect("tempdir");
        let config_dir = temp.path();
        std::fs::create_dir_all(config_dir.join("e2e")).expect("create e2e");
        std::fs::write(
            config_dir.join("e2e/vitest.e2e.config.ts"),
            "export default defineConfig({ oxc: { jsx: { importSource: './jsx' } } });",
        )
        .expect("write project config");
        std::fs::write(config_dir.join("e2e/helper.ts"), "export default {};")
            .expect("write helper");
        let source = r"
            export default defineConfig({
                test: {
                    projects: [
                        './e2e/vitest.e2e.config.ts',
                        './e2e/helper.ts',
                        './missing/vitest.config.ts',
                        './packages/*/vitest.unit.config.ts',
                        { test: { name: 'inline' } },
                    ],
                },
            });
        ";
        let result =
            VitestPlugin.resolve_config(&config_dir.join("vitest.config.ts"), source, config_dir);
        assert_eq!(
            result.setup_files,
            vec![config_dir.join("e2e/vitest.e2e.config.ts")]
        );
        let rules: Vec<(&str, &Path)> = result
            .jsx_import_sources
            .iter()
            .map(|rule| (rule.source.as_str(), rule.config_dir.as_path()))
            .collect();
        assert_eq!(rules, vec![("./jsx", config_dir.join("e2e").as_path())]);
    }

    #[test]
    fn project_glob_entries_read_the_matching_config_files() {
        let temp = tempfile::tempdir().expect("tempdir");
        let config_dir = temp.path();
        for dir in ["runtime/r1", "runtime/r2", "node_modules/x/runtime/r3"] {
            std::fs::create_dir_all(config_dir.join(dir)).expect("create dir");
        }
        let project = "export default defineConfig({ oxc: { jsx: { importSource: './jsx' } } });";
        for file in [
            "runtime/r1/vitest.e2e.config.ts",
            "runtime/r2/vitest.e2e.config.ts",
            "node_modules/x/runtime/r3/vitest.e2e.config.ts",
        ] {
            std::fs::write(config_dir.join(file), project).expect("write project config");
        }
        let source = r"
            export default defineConfig({
                test: {
                    projects: [
                        'runtime/*/vitest.e2e.config.ts',
                        './**/r3/vitest.e2e.config.ts',
                        'runtime/{r1,r2}/vitest.e2e.config.ts',
                    ],
                },
            });
        ";
        let result =
            VitestPlugin.resolve_config(&config_dir.join("vitest.config.ts"), source, config_dir);
        assert_eq!(
            result.setup_files,
            vec![
                config_dir.join("runtime/r1/vitest.e2e.config.ts"),
                config_dir.join("runtime/r2/vitest.e2e.config.ts"),
            ]
        );
        let dirs: Vec<PathBuf> = result
            .jsx_import_sources
            .iter()
            .map(|rule| rule.config_dir.clone())
            .collect();
        assert_eq!(
            dirs,
            vec![config_dir.join("runtime/r1"), config_dir.join("runtime/r2")]
        );
    }

    #[test]
    fn vite_config_next_to_a_vitest_config_gives_no_jsx_rule() {
        let temp = tempfile::tempdir().expect("tempdir");
        let dir = temp.path();
        let vite_config = dir.join("vite.config.ts");
        let vite_source =
            "export default defineConfig({ oxc: { jsx: { importSource: './jsx' } } });";
        let rules = |dir: &Path| {
            let result = VitestPlugin.resolve_config(&vite_config, vite_source, dir);
            (
                result.jsx_import_sources.len(),
                result.jsx_package_credits.len(),
            )
        };
        // Without a vitest config, Vitest loads the vite config.
        assert_eq!(rules(dir), (1, 0));

        std::fs::write(
            dir.join("vitest.config.ts"),
            "export default defineConfig({ test: {} });",
        )
        .expect("write vitest config");
        assert_eq!(rules(dir), (0, 0));

        // A vitest config that merges the vite config gets its settings.
        std::fs::write(
            dir.join("vitest.config.ts"),
            "import viteConfig from './vite.config';\nexport default mergeConfig(viteConfig, defineConfig({ test: {} }));",
        )
        .expect("write merging vitest config");
        assert_eq!(rules(dir), (1, 0));

        let plain =
            VitestPlugin.resolve_config(&vite_config, "export default defineConfig({});", dir);
        assert_eq!(plain.jsx_package_credits.len(), 1);
    }

    #[test]
    fn vitest4_inline_project_inherits_only_with_extends_true() {
        let temp = tempfile::tempdir().expect("tempdir");
        let dir = temp.path();
        std::fs::write(
            dir.join("package.json"),
            r#"{ "devDependencies": { "vitest": "^4.1.0" } }"#,
        )
        .expect("write package.json");
        let source = r"
            export default defineConfig({
                oxc: { jsx: { importSource: './jsx' } },
                test: {
                    projects: [
                        { test: { include: ['a/**/*.test.tsx'] } },
                        { extends: true, test: { include: ['b/**/*.test.tsx'] } },
                    ],
                },
            });
        ";
        let rules = |dir: &Path| -> Vec<(String, Vec<String>)> {
            VitestPlugin
                .resolve_config(&dir.join("vitest.config.ts"), source, dir)
                .jsx_import_sources
                .into_iter()
                .map(|rule| (rule.source, rule.include))
                .collect()
        };
        assert_eq!(
            rules(dir),
            vec![("./jsx".to_string(), strings(&["b/**/*.test.tsx"]))]
        );

        // The installed version wins over the declared range.
        std::fs::create_dir_all(dir.join("node_modules/vitest")).expect("create vitest dir");
        std::fs::write(
            dir.join("node_modules/vitest/package.json"),
            r#"{ "name": "vitest", "version": "5.0.1" }"#,
        )
        .expect("write installed vitest");
        assert_eq!(
            rules(dir),
            vec![
                ("./jsx".to_string(), strings(&["a/**/*.test.tsx"])),
                ("./jsx".to_string(), strings(&["b/**/*.test.tsx"])),
            ]
        );
    }

    #[test]
    fn vitest_range_max_major_reads_the_upper_major() {
        assert_eq!(range_max_major("^4.0.0"), Some(4));
        assert_eq!(range_max_major("~4.1.11"), Some(4));
        assert_eq!(range_max_major("4.x"), Some(4));
        assert_eq!(range_max_major("^4.0.0 || ^5.0.0"), Some(5));
        assert_eq!(range_max_major("npm:vitest@^3.2.0"), Some(3));
        assert_eq!(range_max_major(">=4"), None);
        assert_eq!(range_max_major("latest"), None);
        assert_eq!(range_max_major("catalog:"), None);
        assert_eq!(range_max_major("*"), None);
    }

    #[test]
    fn vitest_config_names_follow_the_vitest_pattern() {
        for name in [
            "vitest.config.ts",
            "vite.config.mjs",
            "vitest.e2e.config.ts",
            "vite.browser-node.config.js",
        ] {
            assert!(is_vitest_config_name(name), "{name}");
        }
        for name in [
            "vitest.setup.ts",
            "vitest.a.b.config.ts",
            "jest.e2e.config.ts",
        ] {
            assert!(!is_vitest_config_name(name), "{name}");
        }
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
