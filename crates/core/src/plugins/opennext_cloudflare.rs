//! OpenNext Cloudflare adapter plugin.
//!
//! The Cloudflare adapter consumes `open-next.config.*` during
//! `opennextjs-cloudflare build`, so the config file and its imports are
//! framework inputs even when the app never imports them directly.

use std::path::Path;

use super::{Plugin, PluginResult, config_parser};

const ENABLERS: &[&str] = &["@opennextjs/cloudflare"];
const SCRIPT_ENABLERS: &[&str] = &["opennextjs-cloudflare"];
const CONFIG_PATTERNS: &[&str] = &["open-next.config.{ts,js,mjs,cjs}"];
const ALWAYS_USED: &[&str] = &[
    "open-next.config.{ts,js,mjs,cjs}",
    "**/open-next.config.{ts,js,mjs,cjs}",
];
const TOOLING_DEPENDENCIES: &[&str] = &["@opennextjs/cloudflare"];

pub struct OpenNextCloudflarePlugin;

impl Plugin for OpenNextCloudflarePlugin {
    fn name(&self) -> &'static str {
        "opennext-cloudflare"
    }

    fn enablers(&self) -> &'static [&'static str] {
        ENABLERS
    }

    fn script_enablers(&self) -> &'static [&'static str] {
        SCRIPT_ENABLERS
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
        let mut result = PluginResult::default();
        let imports = config_parser::extract_imports(source, config_path);
        for imp in &imports {
            result
                .referenced_dependencies
                .push(crate::resolve::extract_package_name(imp));
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustc_hash::FxHashSet;
    use std::path::Path;

    #[test]
    fn activates_from_cloudflare_adapter_dependency() {
        let plugin = OpenNextCloudflarePlugin;
        let deps = vec!["@opennextjs/cloudflare".to_string()];

        assert!(plugin.is_enabled_with_deps(&deps, Path::new("/project")));
    }

    #[test]
    fn activates_from_cloudflare_adapter_script() {
        let plugin = OpenNextCloudflarePlugin;
        let script_packages = FxHashSet::from_iter(["opennextjs-cloudflare".to_string()]);

        assert!(plugin.is_enabled_with_scripts(&script_packages, Path::new("/project")));
    }

    #[test]
    fn does_not_activate_for_plain_next_project() {
        let plugin = OpenNextCloudflarePlugin;
        let deps = vec!["next".to_string()];
        let script_packages = FxHashSet::default();

        assert!(!plugin.is_enabled_with_deps(&deps, Path::new("/project")));
        assert!(!plugin.is_enabled_with_scripts(&script_packages, Path::new("/project")));
    }

    #[test]
    fn exposes_config_patterns_and_nested_always_used_files() {
        let plugin = OpenNextCloudflarePlugin;

        assert_eq!(plugin.config_patterns(), CONFIG_PATTERNS);
        assert!(
            plugin
                .always_used()
                .contains(&"open-next.config.{ts,js,mjs,cjs}")
        );
        assert!(
            plugin
                .always_used()
                .contains(&"**/open-next.config.{ts,js,mjs,cjs}")
        );
        assert_eq!(plugin.script_enablers(), SCRIPT_ENABLERS);
        assert_eq!(plugin.tooling_dependencies(), TOOLING_DEPENDENCIES);
    }
}
