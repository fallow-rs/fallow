//! osls plugin.
//!
//! osls is a maintained fork of the Serverless Framework. It reads the same
//! `serverless.yml` config, so this plugin reuses the config reader of the
//! `serverless-framework` plugin. Only the enabler package is different.

use super::serverless_framework::{
    ALWAYS_USED, CONFIG_FILE_EXPORTS, CONFIG_PATTERNS, resolve_serverless_config,
};
use super::{Plugin, PluginResult};

const ENABLERS: &[&str] = &["osls"];

const TOOLING_DEPENDENCIES: &[&str] = &["osls"];

define_plugin! {
    struct OslsPlugin => "osls",
    enablers: ENABLERS,
    config_patterns: CONFIG_PATTERNS,
    config_default_export_opt_out: CONFIG_PATTERNS,
    config_file_exports: CONFIG_FILE_EXPORTS,
    always_used: ALWAYS_USED,
    tooling_dependencies: TOOLING_DEPENDENCIES,
    resolve_config(config_path, source, root) {
        resolve_serverless_config(config_path, source, root)
    },
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn osls_reads_the_serverless_config() {
        let root = PathBuf::from(if cfg!(windows) {
            "C:\\project"
        } else {
            "/project"
        });
        let source = "plugins:\n  - serverless-offline\nfunctions:\n  harvest:\n    handler: src/handlers/harvest.handler\n";
        let result = OslsPlugin.resolve_config(&root.join("serverless.yml"), source, &root);
        let entries: Vec<&str> = result
            .entry_patterns
            .iter()
            .map(|rule| rule.pattern.as_str())
            .collect();
        assert_eq!(
            entries,
            vec!["src/handlers/harvest.{js,mjs,cjs,ts,mts,cts}"]
        );
        assert_eq!(
            result.referenced_dependencies,
            vec!["serverless-offline".to_string()]
        );
    }
}
