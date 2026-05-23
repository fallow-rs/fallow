//! Wrangler / Cloudflare Workers plugin.
//!
//! Detects Cloudflare Workers projects and marks worker entry points
//! and config files.

use std::path::Path;

use super::{Plugin, PluginResult, config_parser};

const ENABLERS: &[&str] = &["wrangler"];

const ENTRY_PATTERNS: &[&str] = &[
    "src/index.{ts,tsx,js,jsx,mts,mjs}",
    "src/worker.{ts,tsx,js,jsx,mts,mjs}",
    "functions/**/*.{ts,tsx,js,jsx,mts,mjs}",
];

const CONFIG_PATTERNS: &[&str] = &["wrangler.{toml,json,jsonc}"];

const ALWAYS_USED: &[&str] = &["wrangler.toml", "wrangler.json", "wrangler.jsonc"];

const TOOLING_DEPENDENCIES: &[&str] = &["wrangler", "@cloudflare/workers-types"];

define_plugin! {
    struct WranglerPlugin => "wrangler",
    enablers: ENABLERS,
    entry_patterns: ENTRY_PATTERNS,
    config_patterns: CONFIG_PATTERNS,
    always_used: ALWAYS_USED,
    tooling_dependencies: TOOLING_DEPENDENCIES,
    resolve_config(config_path, source, root) {
        let mut result = PluginResult::default();
        if !has_higher_precedence_sibling(config_path) {
            result.extend_entry_patterns(extract_main_entries(config_path, source, root));
        }
        result
    },
}

fn has_higher_precedence_sibling(config_path: &Path) -> bool {
    let Some(file_name) = config_path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    let Some(current_index) = WRANGLER_CONFIG_PRECEDENCE
        .iter()
        .position(|candidate| *candidate == file_name)
    else {
        return false;
    };
    let Some(parent) = config_path.parent() else {
        return false;
    };

    WRANGLER_CONFIG_PRECEDENCE[..current_index]
        .iter()
        .any(|candidate| parent.join(candidate).exists())
}

// Matches Wrangler's current findWranglerConfig order.
const WRANGLER_CONFIG_PRECEDENCE: &[&str] = &["wrangler.json", "wrangler.jsonc", "wrangler.toml"];

fn extract_main_entries(config_path: &Path, source: &str, root: &Path) -> Vec<String> {
    let extension = config_path.extension().and_then(|ext| ext.to_str());
    let mut entries = match extension {
        Some("toml") => extract_toml_main_entries(config_path, source, root),
        _ => extract_js_main_entries(config_path, source, root),
    };
    entries.sort();
    entries.dedup();
    entries
}

fn extract_js_main_entries(config_path: &Path, source: &str, root: &Path) -> Vec<String> {
    let top_level = config_parser::extract_config_path_string(source, config_path, &["main"]);
    let env_entries = config_parser::extract_config_object_nested_strings(
        source,
        config_path,
        &["env"],
        &["main"],
    );
    top_level
        .into_iter()
        .chain(env_entries)
        .filter_map(|raw| config_parser::normalize_config_path(&raw, config_path, root))
        .collect()
}

fn extract_toml_main_entries(config_path: &Path, source: &str, root: &Path) -> Vec<String> {
    let Ok(value) = source.parse::<toml::Table>() else {
        return Vec::new();
    };

    let mut entries = Vec::new();
    if let Some(raw) = value.get("main").and_then(toml::Value::as_str)
        && let Some(path) = config_parser::normalize_config_path(raw, config_path, root)
    {
        entries.push(path);
    }

    if let Some(envs) = value.get("env").and_then(toml::Value::as_table) {
        for env in envs.values() {
            if let Some(raw) = env.get("main").and_then(toml::Value::as_str)
                && let Some(path) = config_parser::normalize_config_path(raw, config_path, root)
            {
                entries.push(path);
            }
        }
    }

    entries
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn static_patterns_cover_worker_js_like_extensions() {
        let plugin = WranglerPlugin;

        assert!(
            plugin
                .entry_patterns()
                .contains(&"src/worker.{ts,tsx,js,jsx,mts,mjs}")
        );
    }

    #[test]
    fn jsonc_config_main_entries_are_entry_patterns() {
        let plugin = WranglerPlugin;
        let result = plugin.resolve_config(
            Path::new("/repo/apps/site/wrangler.jsonc"),
            r#"{
                // top-level worker
                "main": "src/worker.tsx",
                "env": {
                    "staging": { "main": "worker/entry.ts" }
                }
            }"#,
            Path::new("/repo"),
        );

        let entries: Vec<&str> = result
            .entry_patterns
            .iter()
            .map(|entry| entry.pattern.as_str())
            .collect();

        assert!(entries.contains(&"apps/site/src/worker.tsx"));
        assert!(entries.contains(&"apps/site/worker/entry.ts"));
    }

    #[test]
    fn toml_config_main_entries_are_entry_patterns() {
        let plugin = WranglerPlugin;
        let result = plugin.resolve_config(
            Path::new("/repo/wrangler.toml"),
            r#"
                name = "demo"
                main = "src/index.mts"

                [env.production]
                main = "src/production-worker.ts"
            "#,
            Path::new("/repo"),
        );

        let entries: Vec<&str> = result
            .entry_patterns
            .iter()
            .map(|entry| entry.pattern.as_str())
            .collect();

        assert!(
            entries.contains(&"src/index.mts"),
            "missing top-level main, entries={entries:?}"
        );
        assert!(
            entries.contains(&"src/production-worker.ts"),
            "missing env main, entries={entries:?}"
        );
    }

    #[test]
    fn wrangler_json_suppresses_jsonc_sibling_main_entries() {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = dir.path();
        std::fs::write(root.join("wrangler.json"), r#"{ "name": "demo" }"#).expect("json config");

        let plugin = WranglerPlugin;
        let result = plugin.resolve_config(
            &root.join("wrangler.jsonc"),
            r#"{ "main": "src/jsonc-worker.ts" }"#,
            root,
        );

        assert!(
            result.entry_patterns.is_empty(),
            "wrangler.json presence should suppress wrangler.jsonc entries"
        );
    }

    #[test]
    fn wrangler_json_suppresses_toml_sibling_main_entries() {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = dir.path();
        std::fs::write(root.join("wrangler.json"), r#"{ "name": "demo" }"#).expect("json config");

        let plugin = WranglerPlugin;
        let result = plugin.resolve_config(
            &root.join("wrangler.toml"),
            r#"main = "src/toml-worker.ts""#,
            root,
        );

        assert!(
            result.entry_patterns.is_empty(),
            "wrangler.json presence should suppress wrangler.toml entries"
        );
    }

    #[test]
    fn wrangler_jsonc_suppresses_toml_sibling_main_entries() {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = dir.path();
        std::fs::write(root.join("wrangler.jsonc"), r#"{ "name": "demo" }"#).expect("jsonc config");

        let plugin = WranglerPlugin;
        let result = plugin.resolve_config(
            &root.join("wrangler.toml"),
            r#"main = "src/toml-worker.ts""#,
            root,
        );

        assert!(
            result.entry_patterns.is_empty(),
            "wrangler.jsonc presence should suppress wrangler.toml entries"
        );
    }

    #[test]
    fn wrangler_higher_precedence_presence_does_not_fallback_to_lower_main() {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = dir.path();
        std::fs::write(root.join("wrangler.json"), "{").expect("invalid json config");

        let plugin = WranglerPlugin;
        let result = plugin.resolve_config(
            &root.join("wrangler.toml"),
            r#"main = "src/toml-worker.ts""#,
            root,
        );

        assert!(
            result.entry_patterns.is_empty(),
            "a present higher-precedence config should suppress lower entries even without a usable main"
        );
    }

    #[test]
    fn wrangler_sibling_precedence_does_not_cross_directories() {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = dir.path();
        let app_a = root.join("apps/a");
        let app_b = root.join("apps/b");
        std::fs::create_dir_all(&app_a).expect("app a");
        std::fs::create_dir_all(&app_b).expect("app b");
        std::fs::write(app_b.join("wrangler.json"), r#"{ "name": "demo" }"#).expect("json config");

        let plugin = WranglerPlugin;
        let result = plugin.resolve_config(
            &app_a.join("wrangler.toml"),
            r#"main = "src/toml-worker.ts""#,
            root,
        );
        let entries: Vec<&str> = result
            .entry_patterns
            .iter()
            .map(|entry| entry.pattern.as_str())
            .collect();

        assert!(
            entries.contains(&"apps/a/src/toml-worker.ts"),
            "configs in different directories should not suppress each other: {entries:?}"
        );
    }
}
