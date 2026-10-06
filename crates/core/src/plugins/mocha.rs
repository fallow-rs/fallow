//! Mocha test runner plugin.
//!
//! Detects Mocha projects and marks test files as entry points. Parses the
//! Mocha config to credit the packages that Mocha loads before the tests:
//! the `require` entries and the loader entries of `node-option`.

use std::path::Path;

use super::config_parser;
use super::{Plugin, PluginResult};

const ENABLERS: &[&str] = &["mocha"];

const ENTRY_PATTERNS: &[&str] = &[
    "test/**/*.{ts,tsx,js,jsx}",
    "tests/**/*.{ts,tsx,js,jsx}",
    "spec/**/*.{ts,tsx,js,jsx}",
    "**/*.test.{ts,tsx,js,jsx}",
    "**/*.spec.{ts,tsx,js,jsx}",
];

/// Config files that the plugin parses. A YAML config stays always used but
/// is not parsed.
const CONFIG_PATTERNS: &[&str] = &[".mocharc.{json,jsonc,js,cjs,mjs}"];

const ALWAYS_USED: &[&str] = &[".mocharc.{json,jsonc,yaml,yml,js,cjs,mjs}"];

const TOOLING_DEPENDENCIES: &[&str] = &["mocha", "@types/mocha", "ts-mocha"];

/// Config keys whose values are modules that Mocha requires.
const REQUIRE_KEYS: &[&str] = &["require"];

/// Config keys whose values are Node.js options without the leading dashes.
const NODE_OPTION_KEYS: &[&str] = &["node-option", "nodeOption"];

/// Node.js options whose value is a module that Node.js loads.
const NODE_MODULE_OPTIONS: &[&str] = &["require", "import", "loader", "experimental-loader"];

define_plugin! {
    struct MochaPlugin => "mocha",
    enablers: ENABLERS,
    entry_patterns: ENTRY_PATTERNS,
    config_patterns: CONFIG_PATTERNS,
    always_used: ALWAYS_USED,
    tooling_dependencies: TOOLING_DEPENDENCIES,
    package_json_config_key: "mocha",
    resolve_config(config_path, source, _root) {
        let mut result = PluginResult::default();
        for module in loaded_modules(config_path, source) {
            if config_parser::is_package_specifier(&module) {
                result
                    .referenced_dependencies
                    .push(crate::resolve::extract_package_name(&module));
            }
        }
        result
    },
}

/// The module specifiers that a Mocha config loads before the tests.
fn loaded_modules(config_path: &Path, source: &str) -> Vec<String> {
    let mut modules: Vec<String> = REQUIRE_KEYS
        .iter()
        .flat_map(|key| config_parser::extract_config_string_or_array(source, config_path, &[key]))
        .collect();
    for key in NODE_OPTION_KEYS {
        for option in config_parser::extract_config_string_or_array(source, config_path, &[key]) {
            if let Some(module) = node_option_module(&option) {
                modules.push(module.to_string());
            }
        }
    }
    modules
}

/// The module of a Node.js option such as `import=tsx` or
/// `loader=ts-node/esm`, or `None` for an option that loads no module.
fn node_option_module(option: &str) -> Option<&str> {
    let (name, value) = option.trim_start_matches('-').split_once('=')?;
    NODE_MODULE_OPTIONS
        .contains(&name)
        .then(|| value.trim())
        .filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn referenced(path: &str, source: &str) -> Vec<String> {
        let mut deps = MochaPlugin
            .resolve_config(Path::new(path), source, Path::new("/project"))
            .referenced_dependencies;
        deps.sort_unstable();
        deps
    }

    #[test]
    fn require_string_credits_the_package() {
        assert_eq!(
            referenced("/project/.mocharc.json", r#"{"require":"tsx"}"#),
            ["tsx"]
        );
    }

    #[test]
    fn require_array_credits_packages_and_skips_local_files() {
        assert_eq!(
            referenced(
                "/project/.mocharc.json",
                r#"{"require":["ts-node/register","./test/setup.ts","@scope/hooks/register"]}"#
            ),
            ["@scope/hooks", "ts-node"]
        );
    }

    #[test]
    fn node_option_loaders_credit_the_package() {
        assert_eq!(
            referenced(
                "/project/.mocharc.json",
                r#"{"node-option":["import=tsx","loader=ts-node/esm","enable-source-maps"]}"#
            ),
            ["ts-node", "tsx"]
        );
    }

    #[test]
    fn javascript_config_credits_required_packages() {
        assert_eq!(
            referenced(
                "/project/.mocharc.cjs",
                "module.exports = { require: ['tsx'], spec: 'test/**/*.ts' };"
            ),
            ["tsx"]
        );
    }

    #[test]
    fn package_json_key_is_mocha() {
        assert_eq!(MochaPlugin.package_json_config_key(), Some("mocha"));
    }
}
