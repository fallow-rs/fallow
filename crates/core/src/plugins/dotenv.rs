//! dotenv CLI plugin.
//!
//! The dotenv wrappers are parsed by the shared script analyzer. This plugin
//! registers the integration for plugin discovery and Knip migration, and
//! activates only when a dotenv CLI package is declared or invoked.

use super::Plugin;

const ENABLERS: &[&str] = &["dotenv-cli", "@dotenvx/dotenvx"];

pub struct DotenvPlugin;

impl Plugin for DotenvPlugin {
    fn name(&self) -> &'static str {
        "dotenv"
    }

    fn enablers(&self) -> &'static [&'static str] {
        ENABLERS
    }

    fn script_enablers(&self) -> &'static [&'static str] {
        ENABLERS
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use rustc_hash::FxHashSet;

    use super::DotenvPlugin;
    use crate::plugins::Plugin;

    #[test]
    fn activates_only_for_dotenv_cli_packages_or_scripts() {
        let plugin = DotenvPlugin;
        let root = Path::new(".");

        for package in ["dotenv-cli", "@dotenvx/dotenvx"] {
            assert!(plugin.is_enabled_with_deps(&[package.to_owned()], root));
            assert!(
                plugin.is_enabled_with_scripts(&FxHashSet::from_iter([package.to_owned()]), root)
            );
        }
        assert!(!plugin.is_enabled_with_deps(&["dotenv".to_owned()], root));
    }
}
