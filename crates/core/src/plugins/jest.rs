//! Jest test runner plugin.
//!
//! Detects Jest projects and marks test files as entry points.
//! Parses jest.config to extract setupFiles, testMatch, transform,
//! reporters, testEnvironment, preset, globalSetup/Teardown, watchPlugins,
//! resolver, snapshotSerializers, testRunner, and runner as referenced dependencies.
//!
//! Monorepo Jest configs that delegate test discovery to per-project
//! configs via the `projects` field are followed in two shapes:
//!
//! 1. String paths / globs (`<rootDir>/packages/*` or
//!    `<rootDir>/packages/*/jest.config.js`). Each match is resolved to a
//!    concrete config file (`jest.config.{ts,js,mjs,cjs,json}` inside a
//!    matched directory, or the matched file itself) or a per-package
//!    `package.json` carrying a top-level `"jest"` key. The resolved
//!    config is parsed recursively.
//! 2. Inline `ProjectConfig` objects
//!    (`projects: [{ preset: "ts-jest", runner: "jest-runner-eslint" }]`).
//!    Each object's package-typed fields are credited as referenced
//!    dependencies and its setup files are credited as setup entry points.

use std::path::{Path, PathBuf};

use rustc_hash::FxHashSet;

use super::config_parser;
use super::{Plugin, PluginResult};

/// Built-in Jest reporter names that should not be treated as dependencies.
const BUILTIN_REPORTERS: &[&str] = &["default", "verbose", "summary"];

/// Maximum depth for following `projects` chains across nested Jest configs.
/// Real monorepos rarely chain more than 2 levels (root, then per-package);
/// 4 is a generous ceiling that also bounds pathological inputs.
const MAX_PROJECTS_DEPTH: usize = 4;

/// Maximum number of child configs accepted from a single `projects` entry.
/// Real monorepos contain dozens of packages, not thousands; this ceiling
/// keeps pathological globs (`projects: ["**/*"]`) bounded.
/// Scope is per-entry: a config with N entries can still resolve up to
/// N * MAX_EXPANDED_PROJECTS children. That is fine in practice because
/// real configs use a small handful of focused globs, not many broad ones.
const MAX_EXPANDED_PROJECTS: usize = 64;

/// Hard cap on raw glob iterations (matches inspected, accepted or not).
/// `MAX_EXPANDED_PROJECTS` only bounds accepted child configs, so a `**/*`
/// across a tree of non-config files would otherwise still iterate every
/// match. This cap bounds the iteration itself.
const MAX_GLOB_ITERATIONS: usize = 1024;

/// Filenames a Jest project directory may contain, in resolution order
/// (matches Jest's own `--config` lookup precedence). `package.json` is
/// probed last because Jest uses it only when no dedicated config is found.
const JEST_CONFIG_FILENAMES: &[&str] = &[
    "jest.config.ts",
    "jest.config.js",
    "jest.config.mjs",
    "jest.config.cjs",
    "jest.config.json",
];

/// Filename whose top-level `"jest"` key holds an embedded Jest config.
const PACKAGE_JSON_FILENAME: &str = "package.json";

define_plugin!(
    struct JestPlugin => "jest",
    enablers: &["jest"],
    entry_patterns: &[
        "**/*.test.{ts,tsx,js,jsx}",
        "**/*.spec.{ts,tsx,js,jsx}",
        "**/__tests__/**/*.{ts,tsx,js,jsx}",
        "**/__mocks__/**/*.{ts,tsx,js,jsx,mjs,cjs}",
    ],
    config_patterns: &["jest.config.{ts,js,mjs,cjs}", "jest.config.json"],
    always_used: &["jest.config.{ts,js,mjs,cjs}", "jest.setup.{ts,js,tsx,jsx}"],
    tooling_dependencies: &["jest", "jest-environment-jsdom", "ts-jest", "babel-jest"],
    fixture_glob_patterns: &[
        "**/__fixtures__/**/*.{ts,tsx,js,jsx,json}",
        "**/fixtures/**/*.{ts,tsx,js,jsx,json}",
    ],
    package_json_config_key: "jest",
    resolve_config(config_path, source, root) {
        let mut result = PluginResult::default();
        let mut visited = FxHashSet::default();
        extract_jest_config(config_path, source, root, &mut result, &mut visited, 0);
        result
    },
);

/// Parse a Jest config and recurse into any `projects` entries.
///
/// The visited set is keyed by canonicalized config path so cycles
/// (`a` referencing `b` referencing `a`) terminate after one full pass
/// across each file.
fn extract_jest_config(
    config_path: &Path,
    source: &str,
    root: &Path,
    result: &mut PluginResult,
    visited: &mut FxHashSet<PathBuf>,
    depth: usize,
) {
    let key = std::fs::canonicalize(config_path).unwrap_or_else(|_| config_path.to_path_buf());
    if !visited.insert(key) {
        return;
    }

    let filename = config_path.file_name().and_then(|n| n.to_str());
    let (parse_source, parse_path_buf) = if filename == Some(PACKAGE_JSON_FILENAME) {
        let Some(jest_str) = extract_package_json_jest_value(source) else {
            return;
        };
        (format!("({jest_str})"), config_path.with_extension("js"))
    } else if config_path.extension().is_some_and(|ext| ext == "json") {
        (format!("({source})"), config_path.with_extension("js"))
    } else {
        (source.to_string(), config_path.to_path_buf())
    };
    let parse_path: &Path = &parse_path_buf;

    let imports = config_parser::extract_imports(&parse_source, parse_path);
    for imp in &imports {
        let dep = crate::resolve::extract_package_name(imp);
        result.referenced_dependencies.push(dep);
    }

    extract_jest_setup_files(&parse_source, parse_path, root, result);
    extract_jest_dependencies(&parse_source, parse_path, result);

    if depth >= MAX_PROJECTS_DEPTH {
        return;
    }
    extract_jest_inline_projects(&parse_source, parse_path, root, result);
    extract_jest_string_projects(JestStringProjectsInput {
        parse_source: &parse_source,
        parse_path,
        config_path,
        root,
        result,
        visited,
        depth,
    });
}

struct JestStringProjectsInput<'a> {
    parse_source: &'a str,
    parse_path: &'a Path,
    config_path: &'a Path,
    root: &'a Path,
    result: &'a mut PluginResult,
    visited: &'a mut FxHashSet<PathBuf>,
    depth: usize,
}

/// Resolve string-shaped `projects:` entries to concrete child configs, parse
/// each recursively, and merge only their deps and setup files into `result`.
///
/// Child `replace_entry_patterns` / `testMatch` flags are intentionally not
/// merged: Jest scopes those per-project. See the leak-guard test.
fn extract_jest_string_projects(input: JestStringProjectsInput<'_>) {
    let JestStringProjectsInput {
        parse_source,
        parse_path,
        config_path,
        root,
        result,
        visited,
        depth,
    } = input;
    let project_entries =
        config_parser::extract_config_string_array(parse_source, parse_path, &["projects"]);
    if project_entries.is_empty() {
        return;
    }
    let root_dir = jest_root_dir(parse_source, parse_path, root);
    for entry in &project_entries {
        for child_config in expand_project_entry(entry, &root_dir, config_path, root) {
            let Ok(child_source) = std::fs::read_to_string(&child_config) else {
                continue;
            };
            let child_root = child_config
                .parent()
                .map_or_else(|| root.to_path_buf(), Path::to_path_buf);

            let mut child_result = PluginResult::default();
            extract_jest_config(
                &child_config,
                &child_source,
                &child_root,
                &mut child_result,
                visited,
                depth + 1,
            );
            result
                .referenced_dependencies
                .extend(child_result.referenced_dependencies);
            result.setup_files.extend(child_result.setup_files);
        }
    }
}

/// Parse `package.json` source and return its top-level `"jest"` value
/// re-serialised as a JSON string suitable for wrapping in parentheses
/// and feeding to the existing AST-based config helpers.
fn extract_package_json_jest_value(source: &str) -> Option<String> {
    let parsed: serde_json::Value = serde_json::from_str(source).ok()?;
    let jest_val = parsed.get("jest")?;
    if !jest_val.is_object() {
        return None;
    }
    serde_json::to_string(jest_val).ok()
}

/// Credit referenced dependencies and setup files for inline
/// `ProjectConfig` objects in a `projects: [...]` array, e.g.
/// `projects: [{ preset: "ts-jest", runner: "jest-runner-eslint" }]`.
///
/// Each field follows the same rules as the top-level extraction
/// (`extract_jest_setup_files` / `extract_jest_dependencies`): built-in
/// runners / environments are filtered, relative-path resolvers are
/// dropped, and the existing `BUILTIN_REPORTERS` list applies. `transform`
/// (an object whose values are package names) is intentionally not
/// followed when it appears inline, because the existing
/// `extract_config_shallow_strings` helper only walks the top level.
///
/// All field reads route through
/// `extract_config_array_nested_string_or_array`, which inspects
/// only object elements of the array (string elements like
/// `"<rootDir>/packages/*"` are skipped here and handled separately by
/// the string-path expansion path).
fn extract_jest_inline_projects(
    parse_source: &str,
    parse_path: &Path,
    root: &Path,
    result: &mut PluginResult,
) {
    let read = |key: &str| -> Vec<String> {
        config_parser::extract_config_array_nested_string_or_array(
            parse_source,
            parse_path,
            &["projects"],
            &[key],
        )
    };

    credit_inline_project_runner_fields(&read, result);
    credit_inline_project_setup_files(parse_source, parse_path, root, result);
    credit_inline_project_plugin_fields(&read, result);
}

/// Credit preset / resolver / testRunner / runner / testEnvironment fields of
/// an inline `ProjectConfig`, applying the same built-in filtering as the
/// top-level extraction.
fn credit_inline_project_runner_fields(
    read: &impl Fn(&str) -> Vec<String>,
    result: &mut PluginResult,
) {
    for value in read("preset") {
        result
            .referenced_dependencies
            .push(crate::resolve::extract_package_name(&value));
    }

    for value in read("resolver") {
        if value.starts_with('.') || value.starts_with('/') {
            continue;
        }
        result
            .referenced_dependencies
            .push(crate::resolve::extract_package_name(&value));
    }

    for value in read("testRunner") {
        if !matches!(
            value.as_str(),
            "jest-jasmine2" | "jest-circus" | "jest-circus/runner"
        ) {
            result
                .referenced_dependencies
                .push(crate::resolve::extract_package_name(&value));
        }
    }

    for value in read("runner") {
        if value != "jest-runner" {
            result
                .referenced_dependencies
                .push(crate::resolve::extract_package_name(&value));
        }
    }

    for value in read("testEnvironment") {
        super::credit_environment_optional_peers(&value, result);
        // The bare built-in names resolve inside jest and name no package; every
        // other value, including the fully qualified `jest-environment-jsdom`,
        // is a package the project has to declare.
        if !matches!(value.as_str(), "node" | "jsdom") {
            result
                .referenced_dependencies
                .push(crate::resolve::extract_package_name(&value));
        }
    }
}

/// Keys of an inline `ProjectConfig` that name setup entry points.
const INLINE_PROJECT_SETUP_KEYS: [&str; 4] = [
    "setupFiles",
    "setupFilesAfterEnv",
    "globalSetup",
    "globalTeardown",
];

/// Credit setup / global setup-teardown files of an inline `ProjectConfig` as
/// setup entry points.
///
/// A `<rootDir>` path resolves against the root directory of that project
/// ([`inline_project_root_dir`]). Other paths keep the project-root behavior
/// of the top-level extraction.
fn credit_inline_project_setup_files(
    parse_source: &str,
    parse_path: &Path,
    root: &Path,
    result: &mut PluginResult,
) {
    let mut keys = vec!["rootDir"];
    keys.extend(INLINE_PROJECT_SETUP_KEYS);
    let config_dir = jest_config_dir(parse_path, root);
    for fields in config_parser::extract_config_array_object_fields(
        parse_source,
        parse_path,
        &["projects"],
        &keys,
    ) {
        let Some((root_dir_values, setup_values)) = fields.split_first() else {
            continue;
        };
        let root_dir = inline_project_root_dir(root_dir_values.first(), config_dir);
        for value in setup_values.iter().flatten() {
            result
                .setup_files
                .push(resolve_setup_path(value, &root_dir, root));
        }
    }
}

/// The directory that Jest substitutes for `<rootDir>` in an inline project.
///
/// Jest gives an inline project the directory of the parent config as its
/// default `rootDir`. It does not use the `rootDir` option of the parent. A
/// `rootDir` that starts with `<rootDir>` resolves against that directory.
/// Jest resolves a relative `rootDir` without the token against the working
/// directory of the process. Fallow does not know that directory, so it uses
/// the config directory, where Jest usually runs.
fn inline_project_root_dir(raw: Option<&String>, config_dir: &Path) -> PathBuf {
    raw.map_or_else(
        || config_dir.to_path_buf(),
        |raw| {
            let relative = raw
                .strip_prefix("<rootDir>")
                .map_or(raw.as_str(), |rest| rest.trim_start_matches(['/', '\\']));
            config_parser::lexical_normalize(&config_dir.join(relative))
        },
    )
}

/// Credit snapshotSerializers / watchPlugins / reporters package fields of an
/// inline `ProjectConfig`, filtering built-in reporters.
fn credit_inline_project_plugin_fields(
    read: &impl Fn(&str) -> Vec<String>,
    result: &mut PluginResult,
) {
    for value in read("snapshotSerializers") {
        result
            .referenced_dependencies
            .push(crate::resolve::extract_package_name(&value));
    }
    for value in read("watchPlugins") {
        result
            .referenced_dependencies
            .push(crate::resolve::extract_package_name(&value));
    }
    for value in read("reporters") {
        if !BUILTIN_REPORTERS.contains(&value.as_str()) {
            result
                .referenced_dependencies
                .push(crate::resolve::extract_package_name(&value));
        }
    }
}

/// Resolve a `projects:` entry to one or more concrete Jest config files.
///
/// Accepts directories (probed for any `jest.config.{ts,js,mjs,cjs,json}`,
/// then `package.json` with a `"jest"` key), direct config file paths,
/// and glob patterns of either form.
///
/// Two caps bound the work: `MAX_GLOB_ITERATIONS` limits the raw number
/// of matches inspected (so `**/*` on a deep tree of non-config files
/// still terminates promptly), and `MAX_EXPANDED_PROJECTS` limits the
/// number of accepted child configs.
fn expand_project_entry(
    entry: &str,
    root_dir: &Path,
    config_path: &Path,
    root: &Path,
) -> Vec<PathBuf> {
    let resolved = resolve_project_pattern(entry, root_dir, config_path, root);
    let pattern_str = resolved.to_string_lossy();
    let mut configs = Vec::new();

    let Ok(matches) = glob::glob(&pattern_str) else {
        return configs;
    };
    let mut iterations = 0_usize;
    for matched in matches.flatten() {
        iterations += 1;
        if iterations > MAX_GLOB_ITERATIONS || configs.len() >= MAX_EXPANDED_PROJECTS {
            break;
        }
        if matched.is_dir() {
            if let Some(found) = probe_directory_for_jest_config(&matched) {
                configs.push(found);
            }
        } else if matched.is_file() && is_recognised_config_path(&matched) {
            configs.push(matched);
        }
    }
    configs
}

/// Probe a matched directory for a recognised Jest config in resolution
/// order: dedicated `jest.config.{ts,js,mjs,cjs,json}` first, then
/// `package.json` carrying a top-level `"jest"` key.
fn probe_directory_for_jest_config(dir: &Path) -> Option<PathBuf> {
    for filename in JEST_CONFIG_FILENAMES {
        let candidate = dir.join(filename);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    let pkg = dir.join(PACKAGE_JSON_FILENAME);
    if pkg.is_file() && package_json_has_jest_key(&pkg) {
        return Some(pkg);
    }
    None
}

/// True when `package.json` exists and has a top-level `"jest"` object.
/// Reading + parsing the file is cheap relative to the cost of recursing
/// into a synthesised config that turns out to be empty.
fn package_json_has_jest_key(path: &Path) -> bool {
    let Ok(raw) = std::fs::read_to_string(path) else {
        return false;
    };
    let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&raw) else {
        return false;
    };
    parsed.get("jest").is_some_and(serde_json::Value::is_object)
}

/// True when `path`'s filename is a recognised Jest config (dedicated
/// config file or `package.json` whose `"jest"` key holds an object).
fn is_recognised_config_path(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    if JEST_CONFIG_FILENAMES.contains(&name) {
        return true;
    }
    name == PACKAGE_JSON_FILENAME && package_json_has_jest_key(path)
}

/// Substitute `<rootDir>` and resolve relative paths against the config file.
///
/// Jest replaces `<rootDir>` in a string `projects` entry with the `rootDir`
/// option of the config ([`jest_root_dir`]).
fn resolve_project_pattern(
    entry: &str,
    root_dir: &Path,
    config_path: &Path,
    root: &Path,
) -> PathBuf {
    if let Some(rest) = entry.strip_prefix("<rootDir>") {
        let trimmed = rest.trim_start_matches(['/', '\\']);
        return root_dir.join(trimmed);
    }
    let path = Path::new(entry);
    if path.is_absolute() {
        return path.to_path_buf();
    }
    config_path.parent().unwrap_or(root).join(entry)
}

/// The directory that Jest substitutes for `<rootDir>`. Jest resolves the
/// `rootDir` option against the config directory, and uses the config
/// directory when the option is absent. A config path without a directory
/// falls back to `root`.
fn jest_root_dir(parse_source: &str, parse_path: &Path, root: &Path) -> PathBuf {
    let config_dir = jest_config_dir(parse_path, root);
    config_parser::extract_config_string(parse_source, parse_path, &["rootDir"]).map_or_else(
        || config_dir.to_path_buf(),
        |dir| config_parser::lexical_normalize(&config_dir.join(dir)),
    )
}

/// The directory of the Jest config. A config path without a directory falls
/// back to `root`.
fn jest_config_dir<'a>(parse_path: &'a Path, root: &'a Path) -> &'a Path {
    parse_path
        .parent()
        .filter(|dir| dir.is_absolute())
        .unwrap_or(root)
}

/// Resolve one setup file path. A `<rootDir>` path resolves against the Jest
/// root directory. Other paths keep the project-root behavior.
fn resolve_setup_path(entry: &str, root_dir: &Path, root: &Path) -> PathBuf {
    if let Some(rest) = entry.strip_prefix("<rootDir>") {
        return root_dir.join(rest.trim_start_matches(['/', '\\']));
    }
    root.join(entry.trim_start_matches("./"))
}

/// Extract setup files from Jest config (setupFiles, setupFilesAfterEnv, globalSetup, globalTeardown).
fn extract_jest_setup_files(
    parse_source: &str,
    parse_path: &Path,
    root: &Path,
    result: &mut PluginResult,
) {
    if let Some(preset) =
        config_parser::extract_config_string(parse_source, parse_path, &["preset"])
    {
        result
            .referenced_dependencies
            .push(crate::resolve::extract_package_name(&preset));
    }

    let root_dir = jest_root_dir(parse_source, parse_path, root);
    for key in &["setupFiles", "setupFilesAfterEnv"] {
        let files = config_parser::extract_config_string_array(parse_source, parse_path, &[key]);
        for f in &files {
            result
                .setup_files
                .push(resolve_setup_path(f, &root_dir, root));
        }
    }

    for key in &["globalSetup", "globalTeardown"] {
        if let Some(path) = config_parser::extract_config_string(parse_source, parse_path, &[key]) {
            result
                .setup_files
                .push(resolve_setup_path(&path, &root_dir, root));
        }
    }

    let test_match =
        config_parser::extract_config_string_array(parse_source, parse_path, &["testMatch"]);
    if !test_match.is_empty() {
        result.replace_entry_patterns = true;
    }
    result.extend_entry_patterns(test_match);

    if result.entry_patterns.is_empty()
        && let Some(regex) =
            config_parser::extract_config_string(parse_source, parse_path, &["testRegex"])
        && let Some(glob) = test_regex_to_glob(&regex)
    {
        result.replace_entry_patterns = true;
        result.push_entry_pattern(glob);
    }
}

/// Best-effort conversion of a Jest `testRegex` to a glob pattern.
///
/// Handles common patterns like:
/// - `"src/.*\\.test\\.ts$"` → `"src/**/*.test.ts"`
/// - `".*\\.(test|spec)\\.tsx?$"` → stays as defaults (no fixed prefix)
fn test_regex_to_glob(regex: &str) -> Option<String> {
    let meta_chars = ['.', '*', '+', '?', '(', '[', '|', '^', '$', '{', '\\'];
    let prefix_end = regex
        .find(|c: char| meta_chars.contains(&c))
        .unwrap_or(regex.len());
    let prefix = &regex[..prefix_end];

    if prefix.is_empty() || !prefix.contains('/') {
        return None;
    }

    let ext = if regex.contains("tsx?") {
        "{ts,tsx}"
    } else if regex.contains("jsx?") {
        "{js,jsx}"
    } else if regex.contains("\\.ts") {
        "ts"
    } else if regex.contains("\\.js") {
        "js"
    } else {
        "{ts,tsx,js,jsx}"
    };

    let name_pattern = if regex.contains("(test|spec)") || regex.contains("(spec|test)") {
        "*.{test,spec}"
    } else if regex.contains("\\.spec\\.") {
        "*.spec"
    } else {
        "*.test"
    };

    Some(format!("{prefix}**/{name_pattern}.{ext}"))
}

/// Extract referenced dependencies from Jest config (transform, reporters, environment, etc.).
fn extract_jest_dependencies(parse_source: &str, parse_path: &Path, result: &mut PluginResult) {
    extract_jest_array_dependencies(parse_source, parse_path, result);
    extract_jest_scalar_dependencies(parse_source, parse_path, result);
}

/// Credit dependencies from Jest's array/object config fields (transform,
/// reporters, watchPlugins, snapshotSerializers).
fn extract_jest_array_dependencies(
    parse_source: &str,
    parse_path: &Path,
    result: &mut PluginResult,
) {
    let transform_values =
        config_parser::extract_config_shallow_strings(parse_source, parse_path, "transform");
    for val in &transform_values {
        result
            .referenced_dependencies
            .push(crate::resolve::extract_package_name(val));
    }

    let reporters =
        config_parser::extract_config_shallow_strings(parse_source, parse_path, "reporters");
    for reporter in &reporters {
        if !BUILTIN_REPORTERS.contains(&reporter.as_str()) {
            result
                .referenced_dependencies
                .push(crate::resolve::extract_package_name(reporter));
        }
    }

    let watch_plugins =
        config_parser::extract_config_shallow_strings(parse_source, parse_path, "watchPlugins");
    for plugin in &watch_plugins {
        result
            .referenced_dependencies
            .push(crate::resolve::extract_package_name(plugin));
    }

    let serializers = config_parser::extract_config_string_array(
        parse_source,
        parse_path,
        &["snapshotSerializers"],
    );
    for s in &serializers {
        result
            .referenced_dependencies
            .push(crate::resolve::extract_package_name(s));
    }
}

/// Credit dependencies from Jest's scalar config fields (testEnvironment,
/// resolver, testRunner, runner), applying built-in filtering.
fn extract_jest_scalar_dependencies(
    parse_source: &str,
    parse_path: &Path,
    result: &mut PluginResult,
) {
    if let Some(env) =
        config_parser::extract_config_string(parse_source, parse_path, &["testEnvironment"])
    {
        super::credit_environment_optional_peers(&env, result);
        // The bare built-in names resolve inside jest and name no package. Every
        // other value is a package the project declares, and the unprefixed form
        // additionally resolves through jest's `jest-environment-` shorthand.
        if !matches!(env.as_str(), "node" | "jsdom") {
            result.referenced_dependencies.push(format!(
                "jest-environment-{}",
                super::canonical_test_environment(&env)
            ));
            result.referenced_dependencies.push(env);
        }
    }

    if let Some(resolver) =
        config_parser::extract_config_string(parse_source, parse_path, &["resolver"])
        && !resolver.starts_with('.')
        && !resolver.starts_with('/')
    {
        result
            .referenced_dependencies
            .push(crate::resolve::extract_package_name(&resolver));
    }

    if let Some(runner) =
        config_parser::extract_config_string(parse_source, parse_path, &["testRunner"])
        && !matches!(
            runner.as_str(),
            "jest-jasmine2" | "jest-circus" | "jest-circus/runner"
        )
    {
        result
            .referenced_dependencies
            .push(crate::resolve::extract_package_name(&runner));
    }

    if let Some(runner) =
        config_parser::extract_config_string(parse_source, parse_path, &["runner"])
        && runner != "jest-runner"
    {
        result
            .referenced_dependencies
            .push(crate::resolve::extract_package_name(&runner));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_config_preset() {
        let source = r#"module.exports = { preset: "ts-jest" };"#;
        let plugin = JestPlugin;
        let result = plugin.resolve_config(
            std::path::Path::new("jest.config.js"),
            source,
            std::path::Path::new("/project"),
        );
        assert!(
            result
                .referenced_dependencies
                .contains(&"ts-jest".to_string())
        );
    }

    #[test]
    fn resolve_config_root_dir_token_uses_the_root_dir_option() {
        // Kibana layout: a nested config sets `rootDir` to the repository
        // root and names its setup file from there.
        let source = r#"
            module.exports = {
                rootDir: "../../..",
                setupFilesAfterEnv: ["<rootDir>/src/packages/pkg/setup_test.ts"],
                globalSetup: "<rootDir>/src/packages/pkg/global_setup.ts"
            };
        "#;
        let result = JestPlugin.resolve_config(
            Path::new("/project/src/packages/pkg/jest.config.js"),
            source,
            Path::new("/project"),
        );
        assert!(
            result
                .setup_files
                .contains(&PathBuf::from("/project/src/packages/pkg/setup_test.ts")),
            "setup files: {:?}",
            result.setup_files
        );
        assert!(
            result
                .setup_files
                .contains(&PathBuf::from("/project/src/packages/pkg/global_setup.ts"))
        );
    }

    #[test]
    fn resolve_config_inline_project_root_dir_token_uses_the_project_root_dir() {
        // Jest gives an inline project the directory of the parent config as
        // its default `rootDir`, not the `rootDir` option of the parent. A
        // `<rootDir>` value in the project resolves against that directory.
        // Fallow also resolves a plain relative value there.
        let source = r#"
            module.exports = {
                rootDir: "../..",
                projects: [
                    {
                        rootDir: "<rootDir>/pkg-a",
                        setupFiles: ["<rootDir>/setup.ts"]
                    },
                    {
                        rootDir: "pkg-b",
                        globalTeardown: "<rootDir>/teardown.ts"
                    },
                    {
                        setupFilesAfterEnv: ["<rootDir>/after.ts", "./plain.ts"],
                        globalSetup: "<rootDir>/global.ts"
                    }
                ]
            };
        "#;
        let result = JestPlugin.resolve_config(
            Path::new("/project/apps/web/jest.config.js"),
            source,
            Path::new("/project"),
        );
        let mut setup_files = result.setup_files;
        setup_files.sort();
        assert_eq!(
            setup_files,
            [
                "/project/apps/web/after.ts",
                "/project/apps/web/global.ts",
                "/project/apps/web/pkg-a/setup.ts",
                "/project/apps/web/pkg-b/teardown.ts",
                "/project/plain.ts",
            ]
            .map(PathBuf::from)
        );
    }

    fn write_jest_child(dir: &Path, preset: &str) {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(
            dir.join("jest.config.js"),
            format!(r#"module.exports = {{ preset: "{preset}" }};"#),
        )
        .unwrap();
    }

    #[test]
    fn resolve_config_string_projects_root_dir_token_uses_the_root_dir_option() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write_jest_child(&root.join("apps/web/packages/a"), "preset-config-dir");
        write_jest_child(&root.join("packages/b"), "preset-root-dir-option");

        let default_source = r#"module.exports = { projects: ["<rootDir>/packages/*"] };"#;
        let default_path = root.join("apps/web/jest.config.js");
        let result = JestPlugin.resolve_config(&default_path, default_source, root);
        assert_eq!(result.referenced_dependencies, ["preset-config-dir"]);

        let option_source =
            r#"module.exports = { rootDir: "../..", projects: ["<rootDir>/packages/*"] };"#;
        let option_path = root.join("apps/other/jest.config.js");
        let result = JestPlugin.resolve_config(&option_path, option_source, root);
        assert_eq!(result.referenced_dependencies, ["preset-root-dir-option"]);
    }

    #[test]
    fn resolve_config_root_dir_token_defaults_to_the_config_directory() {
        let source = r#"
            module.exports = { setupFiles: ["<rootDir>/setup.ts"] };
        "#;
        let result = JestPlugin.resolve_config(
            Path::new("/project/packages/a/jest.config.js"),
            source,
            Path::new("/project"),
        );
        assert_eq!(
            result.setup_files,
            vec![PathBuf::from("/project/packages/a/setup.ts")]
        );
    }

    #[test]
    fn resolve_config_global_setup_teardown() {
        let source = r#"
            module.exports = {
                globalSetup: "./test/global-setup.ts",
                globalTeardown: "./test/global-teardown.ts"
            };
        "#;
        let plugin = JestPlugin;
        let result = plugin.resolve_config(
            std::path::Path::new("jest.config.js"),
            source,
            std::path::Path::new("/project"),
        );
        assert!(
            result
                .setup_files
                .contains(&std::path::PathBuf::from("/project/test/global-setup.ts"))
        );
        assert!(result.setup_files.contains(&std::path::PathBuf::from(
            "/project/test/global-teardown.ts"
        )));
    }

    #[test]
    fn resolve_config_watch_plugins() {
        let source = r#"
            module.exports = {
                watchPlugins: [
                    "jest-watch-typeahead/filename",
                    "jest-watch-typeahead/testname"
                ]
            };
        "#;
        let plugin = JestPlugin;
        let result = plugin.resolve_config(
            std::path::Path::new("jest.config.js"),
            source,
            std::path::Path::new("/project"),
        );
        let deps = &result.referenced_dependencies;
        assert!(deps.contains(&"jest-watch-typeahead".to_string()));
    }

    #[test]
    fn resolve_config_resolver() {
        let source = r#"module.exports = { resolver: "jest-resolver-enhanced" };"#;
        let plugin = JestPlugin;
        let result = plugin.resolve_config(
            std::path::Path::new("jest.config.js"),
            source,
            std::path::Path::new("/project"),
        );
        assert!(
            result
                .referenced_dependencies
                .contains(&"jest-resolver-enhanced".to_string())
        );
    }

    #[test]
    fn resolve_config_resolver_relative_not_added() {
        let source = r#"module.exports = { resolver: "./custom-resolver.js" };"#;
        let plugin = JestPlugin;
        let result = plugin.resolve_config(
            std::path::Path::new("jest.config.js"),
            source,
            std::path::Path::new("/project"),
        );
        assert!(
            !result
                .referenced_dependencies
                .iter()
                .any(|d| d.contains("custom-resolver"))
        );
    }

    #[test]
    fn resolve_config_snapshot_serializers() {
        let source = r#"
            module.exports = {
                snapshotSerializers: ["enzyme-to-json/serializer"]
            };
        "#;
        let plugin = JestPlugin;
        let result = plugin.resolve_config(
            std::path::Path::new("jest.config.js"),
            source,
            std::path::Path::new("/project"),
        );
        assert!(
            result
                .referenced_dependencies
                .contains(&"enzyme-to-json".to_string())
        );
    }

    #[test]
    fn resolve_config_test_runner_builtin() {
        let source = r#"module.exports = { testRunner: "jest-circus/runner" };"#;
        let plugin = JestPlugin;
        let result = plugin.resolve_config(
            std::path::Path::new("jest.config.js"),
            source,
            std::path::Path::new("/project"),
        );
        assert!(
            !result
                .referenced_dependencies
                .iter()
                .any(|d| d.contains("jest-circus"))
        );
    }

    #[test]
    fn resolve_config_custom_runner() {
        let source = r#"module.exports = { runner: "jest-runner-eslint" };"#;
        let plugin = JestPlugin;
        let result = plugin.resolve_config(
            std::path::Path::new("jest.config.js"),
            source,
            std::path::Path::new("/project"),
        );
        assert!(
            result
                .referenced_dependencies
                .contains(&"jest-runner-eslint".to_string())
        );
    }

    #[test]
    fn resolve_config_json() {
        let source = r#"{"preset": "ts-jest", "testEnvironment": "jsdom"}"#;
        let plugin = JestPlugin;
        let result = plugin.resolve_config(
            std::path::Path::new("jest.config.json"),
            source,
            std::path::Path::new("/project"),
        );
        assert!(
            result
                .referenced_dependencies
                .contains(&"ts-jest".to_string())
        );
    }

    #[test]
    fn test_regex_with_directory_prefix() {
        assert_eq!(
            test_regex_to_glob(r"src/.*\.test\.ts$"),
            Some("src/**/*.test.ts".to_string())
        );
    }

    #[test]
    fn test_regex_without_directory_prefix() {
        assert_eq!(
            test_regex_to_glob(r".*\.test\.ts$"),
            None,
            "regex without directory prefix should return None (same as defaults)"
        );
    }

    #[test]
    fn test_regex_tsx_extension() {
        assert_eq!(
            test_regex_to_glob(r"src/.*\.test\.tsx?$"),
            Some("src/**/*.test.{ts,tsx}".to_string())
        );
    }

    #[test]
    fn test_regex_spec_pattern() {
        assert_eq!(
            test_regex_to_glob(r"src/.*\.spec\.ts$"),
            Some("src/**/*.spec.ts".to_string())
        );
    }

    #[test]
    fn test_regex_test_or_spec() {
        assert_eq!(
            test_regex_to_glob(r"src/.*(test|spec)\.ts$"),
            Some("src/**/*.{test,spec}.ts".to_string())
        );
    }

    #[test]
    fn resolve_config_test_regex_replaces_defaults() {
        let source =
            r#"{"testRegex": "src/.*\\.test\\.ts$", "transform": {"^.+\\.tsx?$": "ts-jest"}}"#;
        let plugin = JestPlugin;
        let result = plugin.resolve_config(
            std::path::Path::new("jest.config.json"),
            source,
            std::path::Path::new("/project"),
        );
        assert!(
            result.replace_entry_patterns,
            "testRegex with directory prefix should trigger replacement"
        );
        assert_eq!(result.entry_patterns, vec!["src/**/*.test.ts"]);
    }

    #[test]
    fn resolve_config_json_transform_object_values() {
        let source = r#"{"transform": {"^.+\\.tsx?$": "ts-jest"}}"#;
        let plugin = JestPlugin;
        let result = plugin.resolve_config(
            std::path::Path::new("jest.config.json"),
            source,
            std::path::Path::new("/project"),
        );
        assert!(
            result
                .referenced_dependencies
                .contains(&"ts-jest".to_string()),
            "should extract transform values from object"
        );
    }

    /// `<rootDir>/packages/*` should expand to every package directory and
    /// pick up each child's `jest.config.*` so the children's deps are credited.
    #[test]
    fn resolve_config_projects_directory_glob() {
        use std::fs;
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        let pkg_a = root.join("packages/a");
        let pkg_b = root.join("packages/b");
        fs::create_dir_all(&pkg_a).unwrap();
        fs::create_dir_all(&pkg_b).unwrap();
        fs::write(
            pkg_a.join("jest.config.js"),
            r#"module.exports = { preset: "ts-jest" };"#,
        )
        .unwrap();
        fs::write(
            pkg_b.join("jest.config.js"),
            r#"module.exports = { preset: "babel-jest" };"#,
        )
        .unwrap();

        let parent_source = r#"module.exports = { projects: ["<rootDir>/packages/*"] };"#;
        let parent_path = root.join("jest.config.js");
        fs::write(&parent_path, parent_source).unwrap();

        let plugin = JestPlugin;
        let result = plugin.resolve_config(&parent_path, parent_source, root);

        assert!(
            result
                .referenced_dependencies
                .contains(&"ts-jest".to_string()),
            "child a's preset must be credited, got: {:?}",
            result.referenced_dependencies,
        );
        assert!(
            result
                .referenced_dependencies
                .contains(&"babel-jest".to_string()),
            "child b's preset must be credited, got: {:?}",
            result.referenced_dependencies,
        );
    }

    /// `<rootDir>/packages/*/jest.config.ts` (file-shaped glob) should still
    /// expand and recurse into every match.
    #[test]
    fn resolve_config_projects_file_glob() {
        use std::fs;
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let pkg_a = root.join("packages/a");
        fs::create_dir_all(&pkg_a).unwrap();
        fs::write(
            pkg_a.join("jest.config.ts"),
            r#"export default { reporters: ["jest-junit"] };"#,
        )
        .unwrap();

        let parent_source =
            r#"module.exports = { projects: ["<rootDir>/packages/*/jest.config.ts"] };"#;
        let parent_path = root.join("jest.config.js");
        fs::write(&parent_path, parent_source).unwrap();

        let plugin = JestPlugin;
        let result = plugin.resolve_config(&parent_path, parent_source, root);

        assert!(
            result
                .referenced_dependencies
                .contains(&"jest-junit".to_string()),
            "child reporter must be credited, got: {:?}",
            result.referenced_dependencies,
        );
    }

    /// A directory glob that matches a package without a jest config must
    /// not crash and must not affect parent-config extraction.
    #[test]
    fn resolve_config_projects_missing_child_config_ignored() {
        use std::fs;
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let pkg = root.join("packages/empty");
        fs::create_dir_all(&pkg).unwrap();

        let parent_source =
            r#"module.exports = { projects: ["<rootDir>/packages/*"], preset: "ts-jest" };"#;
        let parent_path = root.join("jest.config.js");
        fs::write(&parent_path, parent_source).unwrap();

        let plugin = JestPlugin;
        let result = plugin.resolve_config(&parent_path, parent_source, root);

        assert!(
            result
                .referenced_dependencies
                .contains(&"ts-jest".to_string()),
            "parent's own preset must still be credited",
        );
    }

    /// Two configs that reference each other must terminate after both are
    /// parsed once. Both presets get credited; the cycle does not loop.
    #[test]
    fn resolve_config_projects_cycle_guard() {
        use std::fs;
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let a_dir = root.join("a");
        let b_dir = root.join("b");
        fs::create_dir_all(&a_dir).unwrap();
        fs::create_dir_all(&b_dir).unwrap();
        let a_path = a_dir.join("jest.config.js");
        let b_path = b_dir.join("jest.config.js");

        fs::write(
            &a_path,
            format!(
                r#"module.exports = {{ projects: ["{}"], preset: "ts-jest" }};"#,
                b_path.to_string_lossy().replace('\\', "/"),
            ),
        )
        .unwrap();
        fs::write(
            &b_path,
            format!(
                r#"module.exports = {{ projects: ["{}"], preset: "babel-jest" }};"#,
                a_path.to_string_lossy().replace('\\', "/"),
            ),
        )
        .unwrap();

        let plugin = JestPlugin;
        let source_a = std::fs::read_to_string(&a_path).unwrap();
        let result = plugin.resolve_config(&a_path, &source_a, root);

        assert!(
            result
                .referenced_dependencies
                .contains(&"ts-jest".to_string()),
        );
        assert!(
            result
                .referenced_dependencies
                .contains(&"babel-jest".to_string()),
        );
    }

    /// A child config's `testMatch` / `replace_entry_patterns` must NOT bleed
    /// into the parent's result. Jest scopes those flags per-project; if they
    /// propagated, a narrow `testMatch` in one package would silently drop
    /// every other package's tests from the parent's entry-point list.
    #[test]
    fn resolve_config_projects_child_entry_patterns_do_not_leak_to_parent() {
        use std::fs;
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let pkg = root.join("packages/a");
        fs::create_dir_all(&pkg).unwrap();
        fs::write(
            pkg.join("jest.config.js"),
            r#"module.exports = { testMatch: ["src/**/only-here.test.ts"], preset: "ts-jest" };"#,
        )
        .unwrap();

        let parent_source = r#"module.exports = { projects: ["<rootDir>/packages/*"] };"#;
        let parent_path = root.join("jest.config.js");
        fs::write(&parent_path, parent_source).unwrap();

        let plugin = JestPlugin;
        let result = plugin.resolve_config(&parent_path, parent_source, root);

        assert!(
            !result.replace_entry_patterns,
            "child's testMatch must NOT toggle replace_entry_patterns on the parent",
        );
        assert!(
            !result
                .entry_patterns
                .iter()
                .any(|p| p.pattern == "src/**/only-here.test.ts"),
            "child's testMatch entry pattern must NOT appear in the parent's entry list, got: {:?}",
            result.entry_patterns,
        );
        assert!(
            result
                .referenced_dependencies
                .contains(&"ts-jest".to_string()),
            "child's preset must still be credited via the merge",
        );
    }

    /// Metadata-only inline `ProjectConfig` fields (`displayName`,
    /// `rootDir`, etc.) must not pollute referenced deps. String entries
    /// pointing at a non-existent root resolve to no child configs and
    /// must not crash.
    #[test]
    fn resolve_config_projects_metadata_only_inline_object_no_deps() {
        let source =
            r#"module.exports = { projects: [{ displayName: "foo" }, "<rootDir>/packages/a"] };"#;
        let plugin = JestPlugin;
        let result = plugin.resolve_config(
            std::path::Path::new("jest.config.js"),
            source,
            std::path::Path::new("/tmp/fallow-jest-projects-nonexistent-zzz"),
        );
        assert!(
            result.referenced_dependencies.is_empty(),
            "metadata-only field must not be credited as a dep, got: {:?}",
            result.referenced_dependencies,
        );
    }

    /// Inline `ProjectConfig` objects must credit their package-typed
    /// fields (preset, runner, testRunner, testEnvironment, etc.) and
    /// setup files just like a standalone child config would.
    #[test]
    fn resolve_config_projects_inline_object_credits_dependencies() {
        let source = r#"
            module.exports = {
                projects: [
                    { preset: "ts-jest", runner: "jest-runner-eslint" },
                    {
                        displayName: "browser",
                        testEnvironment: "jest-environment-puppeteer",
                        snapshotSerializers: ["enzyme-to-json/serializer"],
                        reporters: ["default", "jest-junit"]
                    },
                    {
                        setupFiles: ["./setup-browser.ts"],
                        globalSetup: "./global-setup.ts"
                    }
                ]
            };
        "#;
        let plugin = JestPlugin;
        let result = plugin.resolve_config(
            std::path::Path::new("jest.config.js"),
            source,
            std::path::Path::new("/project"),
        );

        let deps = &result.referenced_dependencies;
        assert!(
            deps.contains(&"ts-jest".to_string()),
            "preset must be credited, got: {deps:?}",
        );
        assert!(
            deps.contains(&"jest-runner-eslint".to_string()),
            "custom runner must be credited, got: {deps:?}",
        );
        assert!(
            deps.contains(&"jest-environment-puppeteer".to_string()),
            "testEnvironment must surface as a dep, got: {deps:?}",
        );
        assert!(
            deps.contains(&"enzyme-to-json".to_string()),
            "snapshotSerializer package must be credited, got: {deps:?}",
        );
        assert!(
            deps.contains(&"jest-junit".to_string()),
            "non-builtin reporter must be credited, got: {deps:?}",
        );
        assert!(
            !deps.iter().any(|d| d == "default"),
            "builtin reporter `default` must not be credited as a dep, got: {deps:?}",
        );
        assert!(
            result
                .setup_files
                .contains(&std::path::PathBuf::from("/project/setup-browser.ts")),
            "inline setupFiles must be added to result.setup_files, got: {:?}",
            result.setup_files,
        );
        assert!(
            result
                .setup_files
                .contains(&std::path::PathBuf::from("/project/global-setup.ts")),
            "inline globalSetup must be added to result.setup_files, got: {:?}",
            result.setup_files,
        );
    }

    /// Built-in test runners and environments must NOT be credited as
    /// referenced deps even when they appear inside an inline ProjectConfig.
    #[test]
    fn resolve_config_projects_inline_object_filters_builtins() {
        let source = r#"
            module.exports = {
                projects: [
                    { testRunner: "jest-circus", testEnvironment: "node", runner: "jest-runner" }
                ]
            };
        "#;
        let plugin = JestPlugin;
        let result = plugin.resolve_config(
            std::path::Path::new("jest.config.js"),
            source,
            std::path::Path::new("/project"),
        );
        assert!(
            !result
                .referenced_dependencies
                .iter()
                .any(|d| d.contains("jest-circus") || d.contains("jest-runner") || d == "node"),
            "built-in testRunner / runner / testEnvironment must not be credited, got: {:?}",
            result.referenced_dependencies,
        );
    }

    /// A monorepo where each package keeps its Jest config under
    /// `package.json#jest` (instead of a dedicated `jest.config.*` file)
    /// must still have its child deps credited when the parent uses a
    /// directory-shaped `projects` glob.
    #[test]
    fn resolve_config_projects_directory_glob_picks_up_package_json_jest_key() {
        use std::fs;
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        let pkg = root.join("packages/a");
        fs::create_dir_all(&pkg).unwrap();
        fs::write(
            pkg.join("package.json"),
            r#"{ "name": "a", "jest": { "preset": "ts-jest" } }"#,
        )
        .unwrap();

        let parent_source = r#"module.exports = { projects: ["<rootDir>/packages/*"] };"#;
        let parent_path = root.join("jest.config.js");
        fs::write(&parent_path, parent_source).unwrap();

        let plugin = JestPlugin;
        let result = plugin.resolve_config(&parent_path, parent_source, root);

        assert!(
            result
                .referenced_dependencies
                .contains(&"ts-jest".to_string()),
            "preset from package.json#jest must be credited, got: {:?}",
            result.referenced_dependencies,
        );
    }

    /// A directory entry whose `package.json` has no `jest` key must be
    /// treated as having no child config (silently skipped, not an error).
    #[test]
    fn resolve_config_projects_package_json_without_jest_key_skipped() {
        use std::fs;
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let pkg = root.join("packages/a");
        fs::create_dir_all(&pkg).unwrap();
        fs::write(pkg.join("package.json"), r#"{ "name": "a" }"#).unwrap();

        let parent_source =
            r#"module.exports = { projects: ["<rootDir>/packages/*"], preset: "ts-jest" };"#;
        let parent_path = root.join("jest.config.js");
        fs::write(&parent_path, parent_source).unwrap();

        let plugin = JestPlugin;
        let result = plugin.resolve_config(&parent_path, parent_source, root);

        assert!(
            result
                .referenced_dependencies
                .contains(&"ts-jest".to_string()),
        );
        assert_eq!(
            result.referenced_dependencies.len(),
            1,
            "no extra deps should be credited from a package.json with no jest key, got: {:?}",
            result.referenced_dependencies,
        );
    }
}
