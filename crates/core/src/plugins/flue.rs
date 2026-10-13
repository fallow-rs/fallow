//! Flue plugin.
//!
//! Flue loads the app, database and Cloudflare modules and each agent module
//! by file name. No code imports these files, so the plugin makes them entry
//! points. The modules live in `.flue/`, in `src/` or at the package root.
//!
//! A `flue.config.*` file can name the app, database and Cloudflare modules
//! and the agent pattern. When a config file exists, the plugin reads these
//! values and they replace the default entry patterns.

use std::path::Path;

use super::{Plugin, PluginResult, config_parser};

const ENABLERS: &[&str] = &["@flue/runtime"];

const CONFIG_PATTERNS: &[&str] = &["flue.config.{ts,mts,mjs,js,cjs,cts}"];

const ALWAYS_USED: &[&str] = &["flue.config.{ts,mts,mjs,js,cjs,cts}"];

const ENTRY_PATTERNS: &[&str] = &[
    "{app,db,cloudflare}.{ts,mts,js,mjs}",
    ".flue/{app,db,cloudflare}.{ts,mts,js,mjs}",
    "src/{app,db,cloudflare}.{ts,mts,js,mjs}",
    "{agent,agents/*,agents/*/agent}.{ts,mts,js,mjs}",
    ".flue/{agent,agents/*,agents/*/agent}.{ts,mts,js,mjs}",
    "src/{agent,agents/*,agents/*/agent}.{ts,mts,js,mjs}",
];

const DISCOVERY_HIDDEN_DIRS: &[&str] = &[".flue"];

/// The modules that Flue loads by name, each also a config key.
const MODULE_NAMES: &[&str] = &["app", "db", "cloudflare"];

/// The extensions of a module that Flue loads by name.
const MODULE_EXTENSIONS: &str = "{ts,mts,js,mjs}";

/// The default agent pattern, relative to the source root.
const AGENTS_PATTERN: &str = "{agent,agents/*,agents/*/agent}.{ts,mts,js,mjs}";

/// The directories that Flue reads as the source root, in order.
const SOURCE_ROOTS: &[&str] = &[".flue", "src"];

pub struct FluePlugin;

impl Plugin for FluePlugin {
    fn name(&self) -> &'static str {
        "flue"
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

    fn discovery_hidden_dirs(&self) -> &'static [&'static str] {
        DISCOVERY_HIDDEN_DIRS
    }

    fn resolve_config(&self, config_path: &Path, source: &str, root: &Path) -> PluginResult {
        let mut result = PluginResult::default();
        super::add_import_referenced_dependencies(&mut result, source, config_path);

        let config_dir = config_path.parent().unwrap_or(root);
        let source_root = source_root(config_dir);
        let values = config_entry_values(source, config_path, source_root);

        result.extend_config_dir_entry_patterns(values, config_path, root);
        result.replace_entry_patterns = true;
        result
    }
}

/// The first source root directory that exists next to the config file, or
/// the config directory itself.
fn source_root(config_dir: &Path) -> &'static str {
    SOURCE_ROOTS
        .iter()
        .find(|name| config_dir.join(name).is_dir())
        .copied()
        .unwrap_or("")
}

/// The entry values of one config, relative to the config directory.
///
/// A module key with a string value names its module file relative to the
/// config directory. A missing key uses the default file name in the source
/// root. The `agents` value is a pattern relative to the source root.
fn config_entry_values(source: &str, config_path: &Path, source_root: &str) -> Vec<String> {
    let configured = |key: &str| config_parser::extract_config_string(source, config_path, &[key]);

    let mut values: Vec<String> = MODULE_NAMES
        .iter()
        .map(|name| {
            configured(name).unwrap_or_else(|| {
                join_source_root(source_root, &format!("{name}.{MODULE_EXTENSIONS}"))
            })
        })
        .collect();

    let agents = configured("agents").unwrap_or_else(|| AGENTS_PATTERN.to_string());
    values.push(join_source_root(source_root, &agents));
    values
}

fn join_source_root(source_root: &str, value: &str) -> String {
    let value = value.trim_start_matches("./");
    if source_root.is_empty() {
        value.to_string()
    } else {
        format!("{source_root}/{value}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_use_source_root_for_missing_keys() {
        let source = r#"
            import { defineConfig } from "@flue/runtime/config";
            export default defineConfig({ target: "node" });
        "#;
        let values = config_entry_values(source, Path::new("/p/flue.config.ts"), "src");
        assert_eq!(
            values,
            vec![
                "src/app.{ts,mts,js,mjs}".to_string(),
                "src/db.{ts,mts,js,mjs}".to_string(),
                "src/cloudflare.{ts,mts,js,mjs}".to_string(),
                format!("src/{AGENTS_PATTERN}"),
            ]
        );
    }

    #[test]
    fn config_keys_override_defaults() {
        let source = r#"
            export default defineConfig({
                target: "cloudflare",
                app: "./server/routes.ts",
                cloudflare: "./server/workers.ts",
                agents: "assistants/*.ts",
            });
        "#;
        let values = config_entry_values(source, Path::new("/p/flue.config.ts"), ".flue");
        assert_eq!(
            values,
            vec![
                "./server/routes.ts".to_string(),
                ".flue/db.{ts,mts,js,mjs}".to_string(),
                "./server/workers.ts".to_string(),
                ".flue/assistants/*.ts".to_string(),
            ]
        );
    }

    #[test]
    fn root_source_root_has_no_prefix() {
        let values = config_entry_values("export default {}", Path::new("/p/flue.config.js"), "");
        assert_eq!(values[0], "app.{ts,mts,js,mjs}");
        assert_eq!(values[3], AGENTS_PATTERN);
    }

    #[test]
    fn resolve_config_replaces_static_entries() {
        let result = FluePlugin.resolve_config(
            Path::new("/project/flue.config.ts"),
            r#"export default { app: "./server/routes.ts" };"#,
            Path::new("/project"),
        );
        assert!(result.replace_entry_patterns);
        assert!(
            result
                .entry_patterns
                .iter()
                .any(|rule| rule.pattern == "server/routes.ts")
        );
    }
}
