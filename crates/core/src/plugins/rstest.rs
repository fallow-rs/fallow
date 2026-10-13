//! Rstest test runner plugin.
//!
//! Detects Rstest projects and marks test files and `__mocks__` files as
//! entry points. Parses `rstest.config.*` to read `include`, `exclude`,
//! `root`, `setupFiles`, `globalSetup`, `testEnvironment`, `browser`,
//! `resolve.alias`, `coverage.provider` and `projects`.

use std::path::{Path, PathBuf};

use oxc_ast::ast::{Expression, ObjectExpression, ObjectPropertyKind};

use super::config_parser;
use super::test_alias;
use super::{PathRule, Plugin, PluginResult};

const ENABLERS: &[&str] = &["@rstest/core"];

/// Binaries that read a `--config` / `-c` file as an Rstest config.
const SCRIPT_CONFIG_BINARIES: &[&str] = &["rstest"];

/// The Rstest default `include`, `**/*.{test,spec}.?(c|m)[jt]s?(x)`.
const DEFAULT_INCLUDE: &str = "**/*.{test,spec}.?(c|m)[jt]s?(x)";

/// The files that `rs.mock` reads by convention.
const MOCK_PATTERN: &str = "**/__mocks__/**/*.?(c|m)[jt]s?(x)";

const ENTRY_PATTERNS: &[&str] = &[
    "**/*.{test,spec}.{js,jsx,ts,tsx,cjs,mjs,cts,mts}",
    "**/__mocks__/**/*.{js,jsx,ts,tsx,cjs,mjs,cts,mts}",
];

const CONFIG_PATTERNS: &[&str] = &["rstest.config.{js,cjs,mjs,ts,cts,mts}"];

const ALWAYS_USED: &[&str] = &["rstest.config.{js,cjs,mjs,ts,cts,mts}"];

const TOOLING_DEPENDENCIES: &[&str] = &["@rstest/core"];

const CONFIG_EXPORTS: &[&str] = &["default"];

/// The extensions that Rstest looks for when a project directory has its own
/// config file.
const CONFIG_EXTENSIONS: &[&str] = &["mts", "mjs", "ts", "js", "cjs", "cts"];

/// The coverage provider that Rstest uses when the config sets none.
const DEFAULT_COVERAGE_PROVIDER: &str = "istanbul";

/// The packages that browser mode loads.
const BROWSER_DEPENDENCIES: &[&str] = &["@rstest/browser", "playwright"];

/// The test environment that needs no package.
const NODE_ENVIRONMENT: &str = "node";

/// The placeholder that Rstest replaces with the project root.
const ROOT_DIR_PLACEHOLDER: &str = "<rootDir>";

/// The number of nested config levels that the plugin follows.
const MAX_PROJECT_DEPTH: usize = 3;

/// Limits for the expansion of a glob entry in `projects`.
const MAX_PROJECT_GLOB_MATCHES: usize = 1_000;

/// The maximum number of patterns that one extglob pattern expands into.
const MAX_EXTGLOB_VARIANTS: usize = 64;

pub struct RstestPlugin;

impl Plugin for RstestPlugin {
    fn name(&self) -> &'static str {
        "rstest"
    }

    fn enablers(&self) -> &'static [&'static str] {
        ENABLERS
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
        vec![(CONFIG_PATTERNS[0], CONFIG_EXPORTS)]
    }

    fn script_config_binaries(&self) -> &'static [&'static str] {
        SCRIPT_CONFIG_BINARIES
    }

    fn resolve_config(&self, config_path: &Path, source: &str, root: &Path) -> PluginResult {
        let mut result = PluginResult::default();
        let Some(config) = read_runner_config(source, config_path) else {
            return result;
        };
        let config_dir = config_path.parent().unwrap_or(root);
        let runner_root = join_path(config_dir, config.project.root.as_deref().unwrap_or("."));
        let mut ctx = ResolveContext {
            result: &mut result,
            config_path,
            root,
            manifest_dir: nearest_manifest_dir(config_dir, root),
            seen: vec![config_path.to_path_buf()],
        };

        // The config decides which files are tests, so the static defaults
        // do not apply. Each config of the project adds its own patterns.
        ctx.result.replace_entry_patterns = true;
        ctx.result.accumulate_config_entry_patterns = true;

        match &config.projects {
            Some(projects) => ctx.add_projects(projects, &runner_root, &runner_root, 0),
            None => ctx.add_project(&config.project, &runner_root, &runner_root),
        }

        // The coverage settings of a project config below the package
        // directory do not apply, because Rstest reads them from the runner
        // config only.
        if is_package_config(config_dir, root) {
            let provider = config
                .coverage_provider
                .as_deref()
                .unwrap_or(DEFAULT_COVERAGE_PROVIDER);
            result
                .referenced_dependencies
                .push(format!("@rstest/coverage-{provider}"));
        }
        result
    }
}

/// The settings of one Rstest project that the plugin reads.
#[derive(Debug, Default, Clone)]
struct ProjectConfig {
    root: Option<String>,
    include: Option<Vec<String>>,
    exclude: Vec<String>,
    test_environment: Option<String>,
    /// The `setupFiles` and `globalSetup` values.
    setup_files: Vec<String>,
    browser_enabled: bool,
    /// The `resolve.alias` entries, each with its targets. A `false` target
    /// is left out.
    aliases: Vec<(String, Vec<String>)>,
}

/// One value of the `projects` array.
#[derive(Debug, Clone)]
enum ProjectEntry {
    /// A path or glob to a project directory or a project config file.
    Path(String),
    Inline(ProjectConfig),
}

/// The settings of an Rstest config file that the plugin reads.
#[derive(Debug, Default)]
struct RunnerConfig {
    project: ProjectConfig,
    projects: Option<Vec<ProjectEntry>>,
    coverage_provider: Option<String>,
}

fn read_runner_config(source: &str, config_path: &Path) -> Option<RunnerConfig> {
    config_parser::extract_from_source(source, config_path, |program| {
        let obj = config_parser::find_config_object(program)?;
        let projects = config_parser::property_expr(obj, "projects")
            .and_then(config_parser::array_expression)
            .map(|array| {
                array
                    .elements
                    .iter()
                    .filter_map(|element| element.as_expression())
                    .filter_map(|expr| {
                        if let Some(project) = config_parser::object_expression(expr) {
                            return Some(ProjectEntry::Inline(read_project(project)));
                        }
                        config_parser::expression_to_string(expr).map(ProjectEntry::Path)
                    })
                    .collect()
            });
        let coverage_provider = config_parser::property_object(obj, "coverage")
            .and_then(|coverage| config_parser::property_string(coverage, "provider"));
        Some(RunnerConfig {
            project: read_project(obj),
            projects,
            coverage_provider,
        })
    })
}

fn read_project(obj: &ObjectExpression<'_>) -> ProjectConfig {
    let strings = |key: &str| {
        config_parser::property_expr(obj, key)
            .map(config_parser::expression_to_string_or_array)
            .unwrap_or_default()
    };
    let exclude = config_parser::property_expr(obj, "exclude")
        .map(|expr| match config_parser::object_expression(expr) {
            Some(exclude) => config_parser::property_expr(exclude, "patterns")
                .map(config_parser::expression_to_string_or_array)
                .unwrap_or_default(),
            None => config_parser::expression_to_string_or_array(expr),
        })
        .unwrap_or_default();
    let test_environment = config_parser::property_expr(obj, "testEnvironment").and_then(|expr| {
        match config_parser::object_expression(expr) {
            Some(env) => config_parser::property_string(env, "name"),
            None => config_parser::expression_to_string(expr),
        }
    });
    let browser_enabled = config_parser::property_object(obj, "browser")
        .and_then(|browser| config_parser::property_expr(browser, "enabled"))
        .is_some_and(|enabled| matches!(enabled, Expression::BooleanLiteral(value) if value.value));
    let mut setup_files = strings("setupFiles");
    setup_files.extend(strings("globalSetup"));
    ProjectConfig {
        root: config_parser::property_string(obj, "root"),
        include: config_parser::property_expr(obj, "include")
            .map(config_parser::expression_to_string_or_array),
        exclude,
        test_environment,
        setup_files,
        browser_enabled,
        aliases: read_aliases(obj),
    }
}

fn read_aliases(obj: &ObjectExpression<'_>) -> Vec<(String, Vec<String>)> {
    let Some(alias) = config_parser::property_object(obj, "resolve")
        .and_then(|resolve| config_parser::property_object(resolve, "alias"))
    else {
        return Vec::new();
    };
    alias
        .properties
        .iter()
        .filter_map(|property| match property {
            ObjectPropertyKind::ObjectProperty(property) => Some(property),
            ObjectPropertyKind::SpreadProperty(_) => None,
        })
        .filter_map(|property| {
            let name = property.key.static_name()?.into_owned();
            let targets = config_parser::expression_to_string_or_array(&property.value);
            (!targets.is_empty()).then_some((name, targets))
        })
        .collect()
}

struct ResolveContext<'a> {
    result: &'a mut PluginResult,
    config_path: &'a Path,
    root: &'a Path,
    /// The directory of the `package.json` that owns the config. A project
    /// config file in another package belongs to the run of that package.
    manifest_dir: Option<PathBuf>,
    seen: Vec<PathBuf>,
}

impl ResolveContext<'_> {
    fn add_project(&mut self, project: &ProjectConfig, project_root: &Path, runner_root: &Path) {
        let mut includes = Vec::new();
        let mut excludes: Vec<String> = project.exclude.clone();
        let default_include = [DEFAULT_INCLUDE.to_owned()];
        let configured = project.include.as_deref().unwrap_or(&default_include);
        for pattern in configured.iter().map(String::as_str).chain([MOCK_PATTERN]) {
            match pattern.strip_prefix('!') {
                Some(excluded) => excludes.push(excluded.to_owned()),
                None => includes.push(pattern),
            }
        }
        let excluded: Vec<String> = excludes
            .iter()
            .flat_map(|pattern| self.project_patterns(pattern, project_root))
            .collect();
        for pattern in includes {
            for include in self.project_patterns(pattern, project_root) {
                self.result
                    .entry_patterns
                    .push(PathRule::new(include).with_excluded_globs(excluded.iter().cloned()));
            }
        }

        if let Some(env) = project.test_environment.as_deref()
            && env != NODE_ENVIRONMENT
        {
            self.result.referenced_dependencies.push(env.to_owned());
            super::credit_environment_optional_peers(env, self.result);
        }

        for specifier in &project.setup_files {
            let specifier = replace_root_dir(specifier, project_root);
            if config_parser::is_package_specifier(&specifier) {
                self.result
                    .referenced_dependencies
                    .push(crate::resolve::extract_package_name(&specifier));
            } else {
                self.result
                    .setup_files
                    .push(join_path(project_root, &specifier));
            }
        }

        if project.browser_enabled {
            self.result
                .referenced_dependencies
                .extend(BROWSER_DEPENDENCIES.iter().map(|dep| (*dep).to_owned()));
        }

        for (name, targets) in &project.aliases {
            let find = name.strip_suffix('$').unwrap_or(name);
            for target in targets {
                let is_path = target.starts_with('.') || Path::new(target).is_absolute();
                let replacement = if target.starts_with('.') {
                    config_parser::path_to_config_string(&join_path(runner_root, target))
                } else {
                    target.clone()
                };
                test_alias::process_test_alias(
                    self.result,
                    find,
                    &replacement,
                    !is_path,
                    self.config_path,
                    self.root,
                );
            }
        }
    }

    /// The root-relative patterns of one project pattern, with the extglob
    /// groups expanded.
    fn project_patterns(&self, pattern: &str, project_root: &Path) -> Vec<String> {
        let pattern = replace_root_dir(pattern, project_root);
        let absolute = join_path(project_root, &pattern);
        config_parser::normalize_config_path(&absolute, self.config_path, self.root)
            .map(|normalized| expand_extglob(&normalized))
            .unwrap_or_default()
    }

    fn add_projects(
        &mut self,
        projects: &[ProjectEntry],
        projects_root: &Path,
        runner_root: &Path,
        depth: usize,
    ) {
        let mut patterns = Vec::new();
        let mut negated = Vec::new();
        for project in projects {
            match project {
                ProjectEntry::Inline(project) => {
                    let root = project.root.as_deref().map_or_else(
                        || projects_root.to_path_buf(),
                        |root| join_path(runner_root, &replace_root_dir(root, projects_root)),
                    );
                    self.add_project(project, &root, runner_root);
                }
                ProjectEntry::Path(pattern) => {
                    let pattern = replace_root_dir(pattern, projects_root);
                    match pattern.strip_prefix('!') {
                        Some(excluded) => negated.push(excluded.to_owned()),
                        None => patterns.push(pattern),
                    }
                }
            }
        }

        let excluded: Vec<glob::Pattern> = negated
            .iter()
            .filter_map(|pattern| glob_in(projects_root, pattern))
            .filter_map(|pattern| glob::Pattern::new(&pattern).ok())
            .collect();
        for pattern in &patterns {
            for path in matching_paths(projects_root, pattern) {
                if excluded.iter().any(|excluded| excluded.matches_path(&path)) {
                    continue;
                }
                if path.is_dir() {
                    match find_config_file(&path) {
                        Some(file) => self.add_project_file(&file, runner_root, depth),
                        None => self.add_project(&ProjectConfig::default(), &path, runner_root),
                    }
                } else if path.is_file() {
                    self.add_project_file(&path, runner_root, depth);
                }
            }
        }
    }

    /// Read a project config file that `projects` names. The file is used.
    fn add_project_file(&mut self, file: &Path, runner_root: &Path, depth: usize) {
        if self.seen.iter().any(|seen| seen == file) || depth >= MAX_PROJECT_DEPTH {
            return;
        }
        self.seen.push(file.to_path_buf());
        let Some(file_dir) = file.parent() else {
            return;
        };
        if nearest_manifest_dir(file_dir, self.root) != self.manifest_dir {
            return;
        }
        self.result.setup_files.push(file.to_path_buf());
        let Ok(source) = std::fs::read_to_string(file) else {
            return;
        };
        let Some(config) = read_runner_config(&source, file) else {
            return;
        };
        match &config.projects {
            Some(projects) => self.add_projects(projects, file_dir, runner_root, depth + 1),
            None => {
                let project_root = config.project.root.as_deref().map_or_else(
                    || file_dir.to_path_buf(),
                    |root| join_path(runner_root, root),
                );
                self.add_project(&config.project, &project_root, runner_root);
            }
        }
    }
}

/// The files and directories that one `projects` path names. A plain path
/// gives itself. A glob gives its matches, without `node_modules`.
fn matching_paths(dir: &Path, pattern: &str) -> Vec<PathBuf> {
    if !pattern.contains(['*', '?', '[', '{']) {
        let path = join_path(dir, pattern);
        return if path.exists() {
            vec![path]
        } else {
            Vec::new()
        };
    }
    let Some(full) = glob_in(dir, pattern) else {
        return Vec::new();
    };
    let Ok(matches) = glob::glob(&full) else {
        return Vec::new();
    };
    matches
        .flatten()
        .take(MAX_PROJECT_GLOB_MATCHES)
        .filter(|path| {
            !path
                .components()
                .any(|component| component.as_os_str() == "node_modules")
        })
        .collect()
}

/// An absolute glob for a pattern relative to `dir`.
fn glob_in(dir: &Path, pattern: &str) -> Option<String> {
    if Path::new(pattern).is_absolute() {
        return Some(pattern.trim_end_matches('/').to_owned());
    }
    let dir = dir.to_str()?;
    let relative = pattern.strip_prefix("./").unwrap_or(pattern);
    Some(format!(
        "{}/{}",
        glob::Pattern::escape(dir),
        relative.trim_end_matches('/')
    ))
}

fn find_config_file(dir: &Path) -> Option<PathBuf> {
    CONFIG_EXTENSIONS
        .iter()
        .map(|ext| dir.join(format!("rstest.config.{ext}")))
        .find(|path| path.is_file())
}

/// Whether `dir` holds the `package.json` of its package. A directory
/// without a `package.json` on disk, as in a unit test, counts as one.
fn is_package_config(dir: &Path, root: &Path) -> bool {
    nearest_manifest_dir(dir, root).is_none_or(|manifest_dir| manifest_dir == dir)
}

/// The nearest directory with a `package.json`, from `dir` up to `root`. A
/// manifest above the project root must not change the result.
fn nearest_manifest_dir(dir: &Path, root: &Path) -> Option<PathBuf> {
    dir.ancestors()
        .take_while(|ancestor| ancestor.starts_with(root))
        .find(|ancestor| ancestor.join("package.json").is_file())
        .map(Path::to_path_buf)
}

fn replace_root_dir(value: &str, root: &Path) -> String {
    if !value.contains(ROOT_DIR_PLACEHOLDER) {
        return value.to_owned();
    }
    value.replacen(
        ROOT_DIR_PLACEHOLDER,
        &config_parser::path_to_config_string(root),
        1,
    )
}

/// Join a config value to a directory, with `.` and `..` resolved. An
/// absolute value stays as it is.
fn join_path(dir: &Path, value: &str) -> PathBuf {
    config_parser::lexical_normalize(&dir.join(config_parser::path_from_config_string(value)))
}

/// Expand the extglob groups `?(a|b)`, `@(a|b)` and `+(a|b)` of a pattern
/// into plain patterns, which the glob matcher supports. A `+(..)` group
/// matches its body once only, which is the common case. Other groups stay
/// as written.
pub(super) fn expand_extglob(pattern: &str) -> Vec<String> {
    let mut variants = vec![String::new()];
    let mut rest = pattern;
    while let Some((start, operator)) = next_extglob_group(rest) {
        let after_open = &rest[start + 2..];
        let Some(close) = after_open.find(')') else {
            break;
        };
        let body = &after_open[..close];
        let mut options: Vec<&str> = body.split('|').collect();
        if operator == '?' {
            options.insert(0, "");
        }
        if variants.len() * options.len() > MAX_EXTGLOB_VARIANTS {
            return vec![pattern.to_owned()];
        }
        let prefix = &rest[..start];
        variants = variants
            .iter()
            .flat_map(|variant| {
                options
                    .iter()
                    .map(move |option| format!("{variant}{prefix}{option}"))
            })
            .collect();
        rest = &after_open[close + 1..];
    }
    variants
        .into_iter()
        .map(|variant| format!("{variant}{rest}"))
        .collect()
}

/// The byte offset and operator of the next `?(`, `@(` or `+(` group whose
/// body has no nested group.
fn next_extglob_group(pattern: &str) -> Option<(usize, char)> {
    pattern.char_indices().find_map(|(index, operator)| {
        if !matches!(operator, '?' | '@' | '+') || !pattern[index + 1..].starts_with('(') {
            return None;
        }
        let body = &pattern[index + 2..];
        let close = body.find(')')?;
        (!body[..close].contains(['(', '{', '}', ','])).then_some((index, operator))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn resolve(path: &str, source: &str) -> PluginResult {
        RstestPlugin.resolve_config(Path::new(path), source, Path::new("/project"))
    }

    fn entry_patterns(result: &PluginResult) -> Vec<String> {
        let mut patterns: Vec<String> = result
            .entry_patterns
            .iter()
            .map(|rule| rule.pattern.clone())
            .collect();
        patterns.sort_unstable();
        patterns
    }

    fn deps(result: &PluginResult) -> Vec<String> {
        let mut deps = result.referenced_dependencies.clone();
        deps.sort_unstable();
        deps.dedup();
        deps
    }

    #[test]
    fn extglob_groups_expand_into_plain_patterns() {
        let mut expanded = expand_extglob("**/*.test.?(c|m)[jt]s?(x)");
        expanded.sort_unstable();
        assert_eq!(
            expanded,
            [
                "**/*.test.[jt]s",
                "**/*.test.[jt]sx",
                "**/*.test.c[jt]s",
                "**/*.test.c[jt]sx",
                "**/*.test.m[jt]s",
                "**/*.test.m[jt]sx",
            ]
        );
        assert_eq!(expand_extglob("src/**/*.ts"), ["src/**/*.ts"]);
        assert_eq!(expand_extglob("a/@(x|y).js"), ["a/x.js", "a/y.js"]);
    }

    #[test]
    fn default_config_keeps_default_include_and_credits_environment() {
        let result = resolve(
            "/project/rstest.config.ts",
            "import { defineConfig } from '@rstest/core';\n\
             export default defineConfig({ testEnvironment: 'happy-dom' });",
        );
        assert!(result.replace_entry_patterns);
        let patterns = entry_patterns(&result);
        assert!(patterns.contains(&"**/*.{test,spec}.[jt]s".to_owned()));
        assert!(patterns.contains(&"**/__mocks__/**/*.m[jt]sx".to_owned()));
        let deps = deps(&result);
        assert!(deps.contains(&"happy-dom".to_owned()));
        assert!(deps.contains(&"@rstest/coverage-istanbul".to_owned()));
    }

    #[test]
    fn include_exclude_and_root_resolve_against_the_project_root() {
        let result = resolve(
            "/project/rstest.config.ts",
            "export default defineConfig(({ envMode }) => ({\n\
               root: 'src',\n\
               include: ['**/*.check.ts', '!**/ignored.check.ts'],\n\
               exclude: { patterns: ['skipped.check.ts'] },\n\
               setupFiles: './setup/per-file.ts',\n\
               globalSetup: ['./setup/global.ts', 'some-pkg/setup'],\n\
               testEnvironment: { name: 'jsdom' },\n\
             }));",
        );
        let check = result
            .entry_patterns
            .iter()
            .find(|rule| rule.pattern == "src/**/*.check.ts")
            .expect("include pattern under root");
        assert!(
            check
                .exclude_globs
                .iter()
                .any(|glob| glob == "src/**/ignored.check.ts")
        );
        assert!(
            check
                .exclude_globs
                .iter()
                .any(|glob| glob == "src/skipped.check.ts")
        );
        assert!(
            result
                .setup_files
                .contains(&PathBuf::from("/project/src/setup/per-file.ts"))
        );
        assert!(
            result
                .setup_files
                .contains(&PathBuf::from("/project/src/setup/global.ts"))
        );
        let deps = deps(&result);
        assert!(deps.contains(&"some-pkg".to_owned()));
        assert!(deps.contains(&"jsdom".to_owned()));
    }

    #[test]
    fn inline_projects_replace_the_root_project() {
        let result = resolve(
            "/project/rstest.config.ts",
            "export default defineConfig({\n\
               coverage: { provider: 'v8' },\n\
               browser: { enabled: true },\n\
               projects: [\n\
                 { root: 'dom', include: ['**/*.dom.ts'], testEnvironment: 'node' },\n\
                 { include: ['app/*.test.ts'], browser: { enabled: true } },\n\
               ],\n\
             });",
        );
        let patterns = entry_patterns(&result);
        assert!(patterns.contains(&"dom/**/*.dom.ts".to_owned()));
        assert!(patterns.contains(&"app/*.test.ts".to_owned()));
        assert!(
            !patterns
                .iter()
                .any(|pattern| pattern.contains("{test,spec}"))
        );
        let deps = deps(&result);
        assert!(deps.contains(&"@rstest/coverage-v8".to_owned()));
        assert!(deps.contains(&"@rstest/browser".to_owned()));
        assert!(deps.contains(&"playwright".to_owned()));
        assert!(!deps.contains(&"node".to_owned()));
    }

    #[test]
    fn resolve_alias_feeds_path_aliases() {
        let result = resolve(
            "/project/rstest.config.ts",
            "export default defineConfig({\n\
               resolve: { alias: { '@app': './src', 'fruit$': './src/fruit.ts', 'gone': false } },\n\
             });",
        );
        assert!(
            result
                .path_aliases
                .contains(&("@app".to_owned(), "src".to_owned()))
        );
        assert!(
            result
                .path_aliases
                .contains(&("fruit".to_owned(), "src/fruit.ts".to_owned()))
        );
        assert!(!result.path_aliases.iter().any(|(find, _)| find == "gone"));
    }

    #[test]
    fn enabler_is_the_core_package() {
        assert_eq!(RstestPlugin.enablers(), ["@rstest/core"]);
        assert_eq!(
            RstestPlugin.entry_point_role(),
            fallow_config::EntryPointRole::Test
        );
    }
}
