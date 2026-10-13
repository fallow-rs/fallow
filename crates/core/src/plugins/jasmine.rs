//! Jasmine test runner plugin.
//!
//! Detects Jasmine projects and marks spec files and helper files as entry
//! points. Parses `spec/support/jasmine.*` to read `spec_dir`, `spec_files`,
//! `helpers`, `requires` and `loader`.

use std::path::Path;

use super::config_parser;
use super::rstest::expand_extglob;
use super::{PathRule, Plugin, PluginResult};

const ENABLERS: &[&str] = &["jasmine"];

/// Binaries that read a `--config` file as a Jasmine config.
const SCRIPT_CONFIG_BINARIES: &[&str] = &["jasmine"];

/// The Jasmine default `spec_files`, relative to the project root.
const DEFAULT_SPEC_FILES: &str = "spec/**/*[sS]pec.{js,mjs}";

const ENTRY_PATTERNS: &[&str] = &[DEFAULT_SPEC_FILES, "spec/helpers/**/*.{js,mjs}"];

const CONFIG_PATTERNS: &[&str] = &["spec/support/jasmine.{json,jsonc,js,cjs,mjs}"];

const ALWAYS_USED: &[&str] = &["spec/support/jasmine.{json,jsonc,js,cjs,mjs}"];

const TOOLING_DEPENDENCIES: &[&str] = &["jasmine"];

const CONFIG_EXPORTS: &[&str] = &["default"];

pub struct JasminePlugin;

impl Plugin for JasminePlugin {
    fn name(&self) -> &'static str {
        "jasmine"
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
        vec![("spec/support/jasmine.{js,cjs,mjs}", CONFIG_EXPORTS)]
    }

    fn script_config_binaries(&self) -> &'static [&'static str] {
        SCRIPT_CONFIG_BINARIES
    }

    fn resolve_config(&self, config_path: &Path, source: &str, root: &Path) -> PluginResult {
        let mut result = PluginResult::default();
        let spec_dir = config_parser::extract_config_string(source, config_path, &["spec_dir"])
            .unwrap_or_default();
        let spec_files =
            config_parser::extract_config_string_array(source, config_path, &["spec_files"]);
        let helpers = config_parser::extract_config_string_array(source, config_path, &["helpers"]);

        let mut includes = Vec::new();
        let mut excludes = Vec::new();
        // The default is relative to the project root, not to `spec_dir`.
        if spec_files.is_empty() {
            includes.extend(expand_extglob(DEFAULT_SPEC_FILES));
        }
        for pattern in spec_files.iter().chain(&helpers) {
            match pattern.strip_prefix('!') {
                Some(excluded) => excludes.extend(spec_dir_patterns(&spec_dir, excluded, root)),
                None => includes.extend(spec_dir_patterns(&spec_dir, pattern, root)),
            }
        }

        // The config lists every spec and helper file, so the static
        // defaults do not apply.
        result.replace_entry_patterns = true;
        result.accumulate_config_entry_patterns = true;
        for include in includes {
            result
                .entry_patterns
                .push(PathRule::new(include).with_excluded_globs(excludes.iter().cloned()));
        }

        // Jasmine resolves `requires` and `loader` from the working directory,
        // not from the config file.
        let mut modules =
            config_parser::extract_config_string_or_array(source, config_path, &["requires"]);
        modules.extend(config_parser::extract_config_string(
            source,
            config_path,
            &["loader"],
        ));
        for module in modules {
            if config_parser::is_package_specifier(&module) {
                result
                    .referenced_dependencies
                    .push(crate::resolve::extract_package_name(&module));
            } else {
                result.setup_files.push(config_parser::lexical_normalize(
                    &root.join(config_parser::path_from_config_string(&module)),
                ));
            }
        }
        result
    }
}

/// The root-relative patterns of one `spec_files` or `helpers` value, which
/// Jasmine reads relative to `spec_dir`.
fn spec_dir_patterns(spec_dir: &str, pattern: &str, root: &Path) -> Vec<String> {
    let joined = if Path::new(pattern).is_absolute() {
        config_parser::path_from_config_string(pattern)
    } else {
        root.join(config_parser::path_from_config_string(spec_dir))
            .join(config_parser::path_from_config_string(pattern))
    };
    let normalized = config_parser::lexical_normalize(&joined);
    let Ok(relative) = normalized.strip_prefix(root) else {
        return Vec::new();
    };
    expand_extglob(&config_parser::path_to_config_string(relative))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn resolve(path: &str, source: &str) -> PluginResult {
        JasminePlugin.resolve_config(Path::new(path), source, Path::new("/project"))
    }

    fn rule<'a>(result: &'a PluginResult, pattern: &str) -> &'a PathRule {
        result
            .entry_patterns
            .iter()
            .find(|rule| rule.pattern == pattern)
            .unwrap_or_else(|| panic!("missing pattern {pattern}: {:?}", result.entry_patterns))
    }

    #[test]
    fn spec_dir_prefixes_spec_files_and_helpers() {
        let result = resolve(
            "/project/spec/support/jasmine.mjs",
            "export default {\n\
               spec_dir: 'tests',\n\
               spec_files: ['unit/**/*.suite.js', '!unit/**/legacy.suite.js'],\n\
               helpers: ['setup/**/*.js'],\n\
               requires: ['source-map-support/register', './bootstrap/setup.js'],\n\
               loader: './config/custom-loader.mjs',\n\
             };",
        );
        assert!(result.replace_entry_patterns);
        let spec = rule(&result, "tests/unit/**/*.suite.js");
        assert_eq!(spec.exclude_globs, ["tests/unit/**/legacy.suite.js"]);
        rule(&result, "tests/setup/**/*.js");
        assert_eq!(result.referenced_dependencies, ["source-map-support"]);
        assert_eq!(
            result.setup_files,
            [
                Path::new("/project/bootstrap/setup.js"),
                Path::new("/project/config/custom-loader.mjs"),
            ]
        );
    }

    #[test]
    fn json_config_expands_the_init_default() {
        let result = resolve(
            "/project/spec/support/jasmine.json",
            r#"{"spec_dir":"spec","spec_files":["**/*[sS]pec.?(m)js"],"helpers":["helpers/**/*.?(m)js"]}"#,
        );
        rule(&result, "spec/**/*[sS]pec.js");
        rule(&result, "spec/**/*[sS]pec.mjs");
        rule(&result, "spec/helpers/**/*.mjs");
    }

    #[test]
    fn missing_spec_files_fall_back_to_the_default() {
        let result = resolve(
            "/project/spec/support/jasmine.json",
            r#"{"loader":"custom-esm-loader"}"#,
        );
        rule(&result, DEFAULT_SPEC_FILES);
        assert_eq!(result.referenced_dependencies, ["custom-esm-loader"]);
    }

    #[test]
    fn missing_spec_files_default_ignores_spec_dir() {
        let result = resolve(
            "/project/spec/support/jasmine.json",
            r#"{"spec_dir":"spec","helpers":["helpers/**/*.js"]}"#,
        );
        rule(&result, DEFAULT_SPEC_FILES);
        rule(&result, "spec/helpers/**/*.js");
    }

    #[test]
    fn plugin_marks_test_entries() {
        assert_eq!(
            JasminePlugin.entry_point_role(),
            fallow_config::EntryPointRole::Test
        );
        assert_eq!(JasminePlugin.script_config_binaries(), ["jasmine"]);
    }
}
