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
use super::{PathRule, Plugin, PluginResult};

pub struct VitestPlugin;

const ENABLERS: &[&str] = &["vitest"];

/// Binaries that read a `--config` / `-c` file as a vitest config.
const SCRIPT_CONFIG_BINARIES: &[&str] = &["vitest"];

const ENTRY_PATTERNS: &[&str] = &[
    "**/*.test.{ts,tsx,js,jsx}",
    "**/*.spec.{ts,tsx,js,jsx}",
    "**/__tests__/**/*.{ts,tsx,js,jsx}",
    VITEST_DEFAULT_BENCH_INCLUDE,
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
        self.resolve_config_in_project(config_path, source, root, root)
    }

    fn resolve_config_in_project(
        &self,
        config_path: &Path,
        source: &str,
        root: &Path,
        project_root: &Path,
    ) -> PluginResult {
        let run_dir = vitest_run_dir(config_path, root);
        resolve_vitest_config(config_path, source, root, project_root, &run_dir)
    }

    fn resolve_script_config(
        &self,
        config_path: &Path,
        source: &str,
        root: &Path,
        project_root: &Path,
    ) -> PluginResult {
        resolve_vitest_config(config_path, source, root, project_root, root)
    }
}

/// Read a Vitest config. `run_dir` is the directory that Vitest runs in,
/// which is the base of `test.include` and of a reporter path.
fn resolve_vitest_config(
    config_path: &Path,
    source: &str,
    root: &Path,
    project_root: &Path,
    run_dir: &Path,
) -> PluginResult {
    let mut result = PluginResult::default();

    let imports = config_parser::extract_imports(source, config_path);
    add_import_dependencies(&mut result, &imports);
    result
        .referenced_dependencies
        .extend(config_parser::extract_vite_plugin_option_dependencies(
            source,
            config_path,
        ));

    apply_vitest_aliases(&mut result, source, config_path, root);
    // Vitest does not load a vite config that a vitest config shadows.
    if !is_shadowed_vite_config(config_path) {
        apply_vitest_includes(&mut result, source, config_path, root, run_dir);
    }
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
        add_vitest_jsx_import_sources(&mut result, source, config_path, project_root, jsx);
        add_vitest_project_config_files(&mut result, source, config_path, project_root, jsx);
    }

    add_vitest_environment_dependency(&mut result, source, config_path);
    add_vitest_reporter_dependencies(&mut result, source, config_path, root, run_dir);
    add_vitest_coverage_dependency(&mut result, source, config_path);
    add_vitest_typecheck_dependency(&mut result, source, config_path);
    add_vitest_browser_dependency(&mut result, source, config_path);

    result
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

/// Register the `test.include`, `test.projects[].test.include` and
/// `test.benchmark.include` globs of a config as entry patterns.
///
/// Vitest resolves the globs against the directory that it runs in. A config
/// below `root` (a fixture or a package that is not a workspace) therefore
/// gives patterns below its own directory, and only a config that runs in
/// `root` replaces the default patterns. A replacing config keeps the bench
/// files: `test.benchmark.include` is a separate option with its own default.
fn apply_vitest_includes(
    result: &mut PluginResult,
    source: &str,
    config_path: &Path,
    root: &Path,
    run_dir: &Path,
) {
    // Every result accumulates, so a replacing config keeps the patterns of
    // the configs below `root` that the registry merged before it.
    result.accumulate_config_entry_patterns = true;
    let prefix = run_dir
        .strip_prefix(root)
        .map(config_parser::path_to_config_string)
        .unwrap_or_default();
    let root_includes =
        config_parser::extract_config_string_array(source, config_path, &["test", "include"]);
    let bench_includes = config_parser::extract_config_string_array(
        source,
        config_path,
        &["test", "benchmark", "include"],
    );
    push_vitest_include_entries(result, &root_includes, &prefix, root);
    push_vitest_include_entries(result, &bench_includes, &prefix, root);
    if !root_includes.is_empty() && prefix.is_empty() {
        result.replace_entry_patterns = true;
        if bench_includes.is_empty() {
            push_vitest_include_entries(result, &[VITEST_DEFAULT_BENCH_INCLUDE], &prefix, root);
        }
    }

    let project_includes = config_parser::extract_config_array_nested_string_or_array(
        source,
        config_path,
        &["test", "projects"],
        &["test", "include"],
    );
    push_vitest_include_entries(result, &project_includes, &prefix, root);
}

/// The directory that Vitest runs in for `config_path`. Vitest finds a
/// `vitest.config.<ext>` or `vite.config.<ext>` file in the directory that
/// it runs in, so that directory is the directory of the config. A config
/// with another name, such as `vitest.config.e2e.ts`, loads only through
/// `--config`, from a package script that runs in `root`. A config outside
/// `root` also gives `root`.
fn vitest_run_dir(config_path: &Path, root: &Path) -> PathBuf {
    let discovered = config_path
        .file_name()
        .and_then(|name| name.to_str())
        .and_then(|name| {
            name.strip_prefix("vitest.config.")
                .or_else(|| name.strip_prefix("vite.config."))
        })
        .is_some_and(|extension| VITEST_CONFIG_EXTENSIONS.contains(&extension));
    match config_path.parent() {
        Some(dir) if discovered && dir.starts_with(root) => dir.to_path_buf(),
        _ => root.to_path_buf(),
    }
}

/// Register each Vitest include glob as an entry pattern below `prefix`. The
/// extglob groups of the glob go through the shared rewrite, so a group such
/// as `@(test|spec)` matches the files that Vitest runs. A negated entry is
/// an exclude for Vitest and stays as written.
fn push_vitest_include_entries<S: AsRef<str>>(
    result: &mut PluginResult,
    includes: &[S],
    prefix: &str,
    root: &Path,
) {
    result
        .entry_patterns
        .extend(includes.iter().filter_map(|include| {
            let include = include.as_ref();
            let glob = if include.starts_with('!') {
                vitest_extglob(include)
            } else {
                scoped_glob(prefix, include, root)?
            };
            Some(PathRule::from_extglob(&glob))
        }));
}

/// The Vitest default `test.include`, `**/*.{test,spec}.?(c|m)[jt]s?(x)`, as
/// brace groups that the glob matcher supports.
const VITEST_DEFAULT_INCLUDE: &str = "**/*.{test,spec}.{js,jsx,ts,tsx,mjs,cjs,mts,cts}";

/// The Vitest default `test.benchmark.include`,
/// `**/*.{bench,benchmark}.?(c|m)[jt]s?(x)`, as brace groups.
const VITEST_DEFAULT_BENCH_INCLUDE: &str = "**/*.{bench,benchmark}.{js,jsx,ts,tsx,mjs,cjs,mts,cts}";

/// The Vitest default `test.exclude`. A config `exclude` replaces it.
const VITEST_DEFAULT_EXCLUDE: &[&str] = &["**/node_modules/**", "**/.git/**"];

/// The package that Vite imports the JSX runtime from when a config sets no
/// import source.
const DEFAULT_JSX_IMPORT_SOURCE: &str = "react";

/// The first Vitest major in which an inline project without `extends`
/// merges the declaring config.
const VITEST_INHERIT_BY_DEFAULT_MAJOR: u64 = 5;

/// The file names that make Vitest ignore a sibling `vite.config.*`.
/// The extensions of the config files that Vitest finds without `--config`.
const VITEST_CONFIG_EXTENSIONS: &[&str] = &["ts", "js", "mts", "mjs", "cts", "cjs"];

const VITEST_CONFIG_NAMES: &[&str] = &[
    "vitest.config.ts",
    "vitest.config.js",
    "vitest.config.mts",
    "vitest.config.mjs",
    "vitest.config.cts",
    "vitest.config.cjs",
];

/// The maximum number of project config files that one glob entry in
/// `test.projects` reads.
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
/// A vitest config that names the vite config still loads it: an import for
/// `mergeConfig`, a `projects` entry or an `extends` path. Then the vite
/// config applies. Any mention of `vite.config` counts, so a dynamic path
/// keeps the vite config too.
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
    !vitest_source.contains("vite.config")
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
    /// that this project transforms, in one or more groups. `None` when the
    /// project directory is outside `config_dir`, because no rule glob can
    /// reach those files.
    ///
    /// The excludes of a negated extglob group apply only to the include
    /// glob of that group, so such an include gets a group of its own.
    fn glob_groups(&self, config_dir: &Path) -> Option<Vec<(Vec<String>, Vec<String>)>> {
        let (includes, exclude) = self.globs(config_dir)?;
        let mut plain = Vec::new();
        let mut groups = Vec::new();
        for include in includes {
            let rewritten = fallow_config::extglob::rewrite_include(&include);
            if rewritten.excludes.is_empty() {
                plain.push(rewritten.include);
            } else {
                let mut own_exclude = exclude.clone();
                own_exclude.extend(rewritten.excludes);
                groups.push((vec![rewritten.include], own_exclude));
            }
        }
        if !plain.is_empty() {
            groups.insert(0, (plain, exclude));
        }
        Some(groups)
    }

    /// The include globs, with their extglob groups, and the rewritten
    /// exclude globs, relative to `config_dir`. An exclude glob never matches
    /// more files than Vitest excludes, so an exclude with a negated group
    /// is dropped.
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
            let Some(glob) =
                scoped(pattern).and_then(|glob| fallow_config::extglob::rewrite_exclude(&glob))
            else {
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

/// A Vitest include or exclude pattern as a glob relative to `config_dir`, with
/// its extglob groups.
/// `prefix` is the project directory relative to `config_dir`. An absolute
/// pattern outside `config_dir` matches no project file, so it gives `None`.
fn scoped_glob(prefix: &str, pattern: &str, config_dir: &Path) -> Option<String> {
    if pattern.starts_with('/') {
        let relative = Path::new(pattern).strip_prefix(config_dir).ok()?;
        return Some(vitest_extglob(&config_parser::path_to_config_string(
            relative,
        )));
    }
    let glob = vitest_extglob(pattern);
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
/// file, which the plugin reads on its own. An extended file outside
/// `project_root` is not read, so the project keeps only its own values.
fn collect_project_scopes(
    root: &ObjectExpression<'_>,
    config_path: &Path,
    project_root: &Path,
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
            ProjectBase::File(path) if !config_parser::is_inside_root(&path, project_root) => own,
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
    project_root: &Path,
    jsx: JsxContext,
) {
    let Some(config_dir) = config_path.parent() else {
        return;
    };
    let Some((scopes, extended_files)) =
        config_parser::extract_from_source(source, config_path, |program| {
            config_parser::find_config_object(program)
                .map(|config| collect_project_scopes(config, config_path, project_root, jsx))
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
        let Some(groups) = scope.glob_groups(config_dir) else {
            continue;
        };
        let rules = |source: &str| -> Vec<JsxImportSourceRule> {
            groups
                .iter()
                .map(|(include, exclude)| JsxImportSourceRule {
                    source: source.to_string(),
                    config_dir: config_dir.to_path_buf(),
                    include: include.clone(),
                    exclude: exclude.clone(),
                })
                .collect()
        };
        match scope.jsx.runtime() {
            JsxRuntime::None => {}
            JsxRuntime::Default => result
                .jsx_package_credits
                .extend(rules(DEFAULT_JSX_IMPORT_SOURCE)),
            // A package runtime has no project file to reach, so it only
            // credits the package. A graph edge from test files alone would
            // also make a runtime dependency of the app look test-only.
            JsxRuntime::Source(source) if !is_path_source(&source) => result
                .referenced_dependencies
                .push(crate::resolve::extract_package_name(&source)),
            JsxRuntime::Source(source) => result.jsx_import_sources.extend(rules(&source)),
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
/// is not followed, because the glob matcher has no brace support. A file
/// outside the project root is not read.
fn add_vitest_project_config_files(
    result: &mut PluginResult,
    source: &str,
    config_path: &Path,
    project_root: &Path,
    jsx: JsxContext,
) {
    let Some(config_dir) = config_path.parent() else {
        return;
    };
    let entries =
        config_parser::extract_config_string_or_array(source, config_path, &["test", "projects"]);
    for entry in entries {
        for path in project_config_paths(&entry, config_dir, project_root) {
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
            add_vitest_jsx_import_sources(result, &project_source, &path, project_root, jsx);
            if !result.setup_files.contains(&path) {
                result.setup_files.push(path);
            }
        }
    }
}

/// The files that one `test.projects` string entry names. A plain path gives
/// itself. A glob gives the files that match it, without `node_modules`. The
/// glob walk does not enter `node_modules`, `.git`, or a gitignored path. A
/// path outside `project_root` gives nothing, because the config comes from
/// the analyzed repository.
fn project_config_paths(entry: &str, config_dir: &Path, project_root: &Path) -> Vec<PathBuf> {
    // A glob that starts with `**` walks the whole source tree on each run.
    // Vitest configs name project directories, so such an entry is not
    // followed.
    let relative = entry.strip_prefix("./").unwrap_or(entry);
    if entry.contains('{') || relative.starts_with("**") {
        return Vec::new();
    }
    if !entry.contains(['*', '?', '[']) {
        let path = lexical_join(config_dir, entry);
        return if config_parser::is_inside_root(&path, project_root) {
            vec![path]
        } else {
            Vec::new()
        };
    }
    if !config_parser::glob_base_is_inside_root(config_dir, relative, project_root) {
        return Vec::new();
    }
    super::project_glob::expand(config_dir, relative, project_root)
        .paths
        .into_iter()
        .filter(|path| {
            path.is_file()
                && config_parser::is_inside_root(path, project_root)
                && !super::project_glob::is_in_node_modules(path, project_root)
        })
        .map(|path| config_parser::lexical_normalize(&path))
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

/// Whether a JSX import source or a reporter names a path, not a package.
fn is_path_source(source: &str) -> bool {
    source.starts_with('.') || source.starts_with('/')
}

/// A Vitest include or exclude pattern in the extglob syntax that
/// [`fallow_config::extglob`] rewrites.
///
/// Vitest matches globs with picomatch, which also reads a plain `(a|b)`
/// group as one of its alternatives. This function marks each plain group
/// as the equivalent `@(a|b)` group, so `(*.)+(spec|test)` becomes
/// `@(*.)+(spec|test)`. It also removes a leading `./`.
fn vitest_extglob(pattern: &str) -> String {
    let pattern = pattern.strip_prefix("./").unwrap_or(pattern);
    let mut out = String::with_capacity(pattern.len() + 2);
    let mut previous = None;
    let mut escaped = false;
    for character in pattern.chars() {
        if character == '(' && !escaped && !matches!(previous, Some('@' | '?' | '+' | '*' | '!')) {
            out.push('@');
        }
        escaped = character == '\\' && !escaped;
        previous = Some(character);
        out.push(character);
    }
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

fn add_vitest_reporter_dependencies(
    result: &mut PluginResult,
    source: &str,
    config_path: &Path,
    root: &Path,
    run_dir: &Path,
) {
    let reporters = config_parser::extract_config_nested_shallow_strings(
        source,
        config_path,
        &["test"],
        "reporters",
    );
    for reporter in &reporters {
        if BUILTIN_REPORTERS.contains(&reporter.as_str()) {
            continue;
        }
        // Vitest imports a reporter path as a module, so the file is used.
        if is_path_source(reporter) {
            let path = lexical_join(run_dir, reporter);
            if path.starts_with(root) && !result.setup_files.contains(&path) {
                result.setup_files.push(path);
            }
            continue;
        }
        let dep = crate::resolve::extract_package_name(reporter);
        result.referenced_dependencies.push(dep);
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
mod tests;
