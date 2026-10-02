//! Expo framework plugin.
//!
//! Detects Expo projects and marks app entry points and config files. Reads
//! the config plugins that the app config lists.

use std::path::Path;

use super::{Plugin, PluginResult, config_parser};

const ENABLERS: &[&str] = &["expo"];

const ENTRY_PATTERNS: &[&str] = &[
    "App.{ts,tsx,js,jsx}",
    "app/**/*.{ts,tsx,js,jsx}",
    "src/App.{ts,tsx,js,jsx}",
];

const ALWAYS_USED: &[&str] = &[
    "app.json",
    "app.config.{ts,js,mjs,cjs}",
    "metro.config.{ts,js,mjs,cjs}",
    "babel.config.{ts,js,mjs,cjs}",
];

const CONFIG_PATTERNS: &[&str] = &["app.json", "app.config.{ts,js,mjs,cjs}"];

/// The property paths of the config plugin list. `app.json` nests it under
/// `expo`, and `app.config.*` can return it at the top level.
const CONFIG_PLUGIN_PATHS: &[&[&str]] = &[&["plugins"], &["expo", "plugins"]];

const TOOLING_DEPENDENCIES: &[&str] = &["expo", "expo-cli", "@expo/webpack-config"];

pub struct ExpoPlugin;

impl Plugin for ExpoPlugin {
    fn name(&self) -> &'static str {
        "expo"
    }

    fn enablers(&self) -> &'static [&'static str] {
        ENABLERS
    }

    fn is_enabled_with_deps(&self, deps: &[String], _root: &std::path::Path) -> bool {
        deps.iter().any(|dep| dep == "expo") && !deps.iter().any(|dep| dep == "expo-router")
    }

    fn entry_patterns(&self) -> &'static [&'static str] {
        ENTRY_PATTERNS
    }

    fn always_used(&self) -> &'static [&'static str] {
        ALWAYS_USED
    }

    fn config_patterns(&self) -> &'static [&'static str] {
        CONFIG_PATTERNS
    }

    fn tooling_dependencies(&self) -> &'static [&'static str] {
        TOOLING_DEPENDENCIES
    }

    fn resolve_config(&self, config_path: &Path, source: &str, root: &Path) -> PluginResult {
        let mut result = PluginResult::default();
        super::add_import_referenced_dependencies(&mut result, source, config_path);
        add_config_plugins(&mut result, source, config_path, root);
        result
    }
}

/// Credit the config plugins that an Expo app config lists in `plugins`.
///
/// Expo loads each entry by name at prebuild time, so the source never imports
/// it. An entry is a string or a `[name, options]` tuple. A bare name is a
/// package. A relative name is a local plugin file.
pub(super) fn add_config_plugins(
    result: &mut PluginResult,
    source: &str,
    config_path: &Path,
    root: &Path,
) {
    for plugins_path in CONFIG_PLUGIN_PATHS {
        let names = config_parser::extract_config_string_array(source, config_path, plugins_path)
            .into_iter()
            .chain(config_parser::extract_config_array_tuple_heads(
                source,
                config_path,
                plugins_path,
            ));
        for name in names {
            add_config_plugin(result, name.trim(), config_path, root);
        }
    }
}

fn add_config_plugin(result: &mut PluginResult, name: &str, config_path: &Path, root: &Path) {
    if name.is_empty() {
        return;
    }
    if config_parser::is_relative_specifier(name) {
        if let Some(path) = config_parser::normalize_config_path(name, config_path, root) {
            result.push_entry_path(path);
        }
        return;
    }
    result
        .referenced_dependencies
        .push(crate::resolve::extract_package_name(name));
}
