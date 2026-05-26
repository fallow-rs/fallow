//! Varlock plugin.
//!
//! Varlock consumes `.env.schema` files directly. Provider plugins can be
//! declared through schema `@plugin(...)` decorators, and Vite projects consume
//! `@varlock/vite-integration` through Vite's plugin pipeline.

use std::path::{Path, PathBuf};

use super::{Plugin, PluginResult};

const ENABLERS: &[&str] = &["varlock", "@varlock/"];
const CONFIG_PATTERNS: &[&str] = &[".env.schema", "**/.env.schema"];
const ALWAYS_USED: &[&str] = &[".env.schema", "**/.env.schema"];
const TOOLING_DEPENDENCIES: &[&str] = &["varlock", "@varlock/vite-integration"];

pub struct VarlockPlugin;

impl Plugin for VarlockPlugin {
    fn name(&self) -> &'static str {
        "varlock"
    }

    fn enablers(&self) -> &'static [&'static str] {
        ENABLERS
    }

    fn is_enabled_with_deps(&self, deps: &[String], root: &Path) -> bool {
        Self::has_varlock_dependency(deps) || root.join(".env.schema").is_file()
    }

    fn is_enabled_with_files(
        &self,
        deps: &[String],
        root: &Path,
        discovered_files: &[PathBuf],
    ) -> bool {
        self.is_enabled_with_deps(deps, root)
            || discovered_files.iter().any(|path| is_env_schema_path(path))
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

    fn resolve_config(&self, config_path: &Path, source: &str, _root: &Path) -> PluginResult {
        if !is_env_schema_path(config_path) {
            return PluginResult::default();
        }

        PluginResult {
            referenced_dependencies: extract_schema_plugin_dependencies(source),
            ..PluginResult::default()
        }
    }
}

impl VarlockPlugin {
    fn has_varlock_dependency(deps: &[String]) -> bool {
        deps.iter().any(|dep| {
            dep == "varlock" || dep == "@varlock/vite-integration" || dep.starts_with("@varlock/")
        })
    }
}

fn is_env_schema_path(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name == ".env.schema")
}

fn extract_schema_plugin_dependencies(source: &str) -> Vec<String> {
    let mut deps = Vec::new();
    for line in source.lines() {
        let mut rest = line;
        while let Some(start) = rest.find("@plugin(") {
            let after_start = &rest[start + "@plugin(".len()..];
            let Some(end) = after_start.find(')') else {
                break;
            };
            if let Some(dep) = plugin_dependency_from_argument(&after_start[..end]) {
                deps.push(dep);
            }
            rest = &after_start[end + 1..];
        }
    }

    deps.sort();
    deps.dedup();
    deps
}

fn plugin_dependency_from_argument(argument: &str) -> Option<String> {
    let specifier = argument
        .trim()
        .trim_matches(|ch| matches!(ch, '"' | '\'' | '`'));
    if !is_package_specifier(specifier) {
        return None;
    }

    Some(crate::resolve::extract_package_name(
        &strip_npm_version_selector(specifier),
    ))
}

fn strip_npm_version_selector(specifier: &str) -> String {
    if let Some(rest) = specifier.strip_prefix('@') {
        let Some((scope, package_and_rest)) = rest.split_once('/') else {
            return specifier.to_string();
        };
        let package = package_and_rest
            .split('@')
            .next()
            .unwrap_or(package_and_rest);
        return format!("@{scope}/{package}");
    }

    specifier.split('@').next().unwrap_or(specifier).to_string()
}

fn is_package_specifier(specifier: &str) -> bool {
    !specifier.is_empty()
        && specifier != "."
        && specifier != ".."
        && !specifier.starts_with("./")
        && !specifier.starts_with("../")
        && !specifier.starts_with('/')
        && !specifier.contains(':')
        && !specifier.contains('\\')
        && !specifier.chars().any(char::is_whitespace)
}

#[cfg(test)]
mod tests {
    use super::*;
    use fallow_config::EntryPointRole;

    #[test]
    fn activates_from_varlock_dependency_prefix_or_schema_file() {
        let plugin = VarlockPlugin;
        let tmp = tempfile::tempdir().expect("temp dir");

        assert!(plugin.is_enabled_with_deps(&["varlock".to_string()], tmp.path()));
        assert!(plugin.is_enabled_with_deps(
            &["@varlock/google-secret-manager-plugin".to_string()],
            tmp.path()
        ));
        assert!(!plugin.is_enabled_with_deps(&["varlockish".to_string()], tmp.path()));

        std::fs::write(tmp.path().join(".env.schema"), "APP_ENV=dev\n").expect("schema");
        assert!(plugin.is_enabled_with_deps(&[], tmp.path()));
    }

    #[test]
    fn activates_from_discovered_schema_files() {
        let plugin = VarlockPlugin;
        let files = vec![PathBuf::from("/repo/apps/web/.env.schema")];

        assert!(plugin.is_enabled_with_files(&[], Path::new("/repo"), &files));
    }

    #[test]
    fn exposes_varlock_conventions() {
        let plugin = VarlockPlugin;

        assert_eq!(plugin.config_patterns(), CONFIG_PATTERNS);
        assert_eq!(plugin.always_used(), ALWAYS_USED);
        assert_eq!(plugin.tooling_dependencies(), TOOLING_DEPENDENCIES);
        assert_eq!(plugin.entry_point_role(), EntryPointRole::Support);
    }

    #[test]
    fn resolve_config_credits_schema_plugin_packages() {
        let source = r#"
            # @plugin(@varlock/google-secret-manager-plugin)
            # @plugin("@varlock/bitwarden-plugin@1.2.3")
            # @plugin('varlock-custom-provider/subpath')
            # @plugin(`@scope/varlock-provider@latest`)
        "#;
        let plugin = VarlockPlugin;
        let result = plugin.resolve_config(Path::new(".env.schema"), source, Path::new("/repo"));

        assert_eq!(
            result.referenced_dependencies,
            vec![
                "@scope/varlock-provider".to_string(),
                "@varlock/bitwarden-plugin".to_string(),
                "@varlock/google-secret-manager-plugin".to_string(),
                "varlock-custom-provider".to_string(),
            ]
        );
    }

    #[test]
    fn resolve_config_dedups_and_ignores_non_package_specifiers() {
        let source = r"
            # @plugin(@varlock/google-secret-manager-plugin)
            # @plugin(@varlock/google-secret-manager-plugin)
            # @plugin(./local-plugin.js)
            # @plugin(https://example.com/plugin.js)
            # @plugin(file:./plugin.js)
            # @plugin(bad\path)
            # @plugin()
            # @plugin( )
        ";
        let plugin = VarlockPlugin;
        let result = plugin.resolve_config(Path::new(".env.schema"), source, Path::new("/repo"));

        assert_eq!(
            result.referenced_dependencies,
            vec!["@varlock/google-secret-manager-plugin".to_string()]
        );
    }

    #[test]
    fn resolve_config_ignores_non_schema_files() {
        let plugin = VarlockPlugin;
        let result = plugin.resolve_config(
            Path::new("vite.config.ts"),
            "# @plugin(@varlock/google-secret-manager-plugin)",
            Path::new("/repo"),
        );

        assert!(result.referenced_dependencies.is_empty());
    }
}
