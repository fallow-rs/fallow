//! Oxfmt plugin.
//!
//! Detects Oxfmt projects and marks config files as always used.
//! Oxfmt configs carry formatting options only (no plugin or extends
//! mechanism), so resolution credits static imports from TypeScript configs
//! (typically `defineConfig` from `oxfmt` itself) and nothing else.

use super::config_parser;
use super::{Plugin, PluginResult};

const ENABLERS: &[&str] = &["oxfmt"];

const CONFIG_PATTERNS: &[&str] = &[
    ".oxfmtrc.{json,jsonc}",
    "oxfmt.config.{ts,mts,cts,js,mjs,cjs}",
];

const ALWAYS_USED: &[&str] = &[
    ".oxfmtrc.{json,jsonc}",
    "oxfmt.config.{ts,mts,cts,js,mjs,cjs}",
];

const TOOLING_DEPENDENCIES: &[&str] = &["oxfmt"];

define_plugin! {
    struct OxfmtPlugin => "oxfmt",
    enablers: ENABLERS,
    config_patterns: CONFIG_PATTERNS,
    always_used: ALWAYS_USED,
    tooling_dependencies: TOOLING_DEPENDENCIES,
    resolve_config(config_path, source, _root) {
        let mut result = PluginResult::default();

        let imports = config_parser::extract_imports(source, config_path);
        for imp in &imports {
            let dep = crate::resolve::extract_package_name(imp);
            result.referenced_dependencies.push(dep);
        }

        result
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    #[test]
    fn resolve_config_json_options_have_no_deps() {
        let source = r#"{"printWidth": 100, "singleQuote": true}"#;
        let plugin = OxfmtPlugin;
        let result =
            plugin.resolve_config(Path::new(".oxfmtrc.json"), source, Path::new("/project"));

        assert!(result.referenced_dependencies.is_empty());
    }

    #[test]
    fn resolve_config_jsonc_options_have_no_deps() {
        let source = "// formatter config\n{\"printWidth\": 100}\n";
        let plugin = OxfmtPlugin;
        let result =
            plugin.resolve_config(Path::new(".oxfmtrc.jsonc"), source, Path::new("/project"));

        assert!(result.referenced_dependencies.is_empty());
    }

    #[test]
    fn resolve_config_ts_define_config_import() {
        let source = r#"
            import { defineConfig } from "oxfmt";
            export default defineConfig({ printWidth: 120 });
        "#;
        let plugin = OxfmtPlugin;
        let result =
            plugin.resolve_config(Path::new("oxfmt.config.ts"), source, Path::new("/project"));

        assert!(
            result
                .referenced_dependencies
                .contains(&"oxfmt".to_string())
        );
    }

    #[test]
    fn resolve_config_ts_shared_import_is_credited() {
        let source = r#"
            import base from "@scope/oxfmt-shared";
            export default { ...base, printWidth: 120 };
        "#;
        let plugin = OxfmtPlugin;
        let result =
            plugin.resolve_config(Path::new("oxfmt.config.ts"), source, Path::new("/project"));

        assert!(
            result
                .referenced_dependencies
                .contains(&"@scope/oxfmt-shared".to_string())
        );
    }

    #[test]
    fn resolve_config_malformed_does_not_panic() {
        let plugin = OxfmtPlugin;
        let json = plugin.resolve_config(
            Path::new(".oxfmtrc.json"),
            "{{not json",
            Path::new("/project"),
        );
        let ts = plugin.resolve_config(
            Path::new("oxfmt.config.ts"),
            "export default {{{",
            Path::new("/project"),
        );

        assert!(json.referenced_dependencies.is_empty());
        assert!(ts.referenced_dependencies.is_empty());
    }
}
