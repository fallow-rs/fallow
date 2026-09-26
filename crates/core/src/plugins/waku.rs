//! Waku framework plugin.
//!
//! Waku's managed mode globs every module under `<srcDir>/pages` into its file
//! router, skipping any path with a `_components`, `_hooks` or `_actions`
//! segment. Route modules export `default` and `getConfig`; modules under
//! `_api/` export HTTP method handlers. `<srcDir>/middleware/*` modules and the
//! optional `<srcDir>/waku.{server,client}` entries replace or extend the
//! managed server. The generated `<srcDir>/pages.gen.ts` route types are kept
//! alive. `srcDir` defaults to `src` and is read from `waku.config.*`.

use std::path::Path;

use super::{PathRule, Plugin, PluginResult, UsedExportRule, config_parser};

const ENABLERS: &[&str] = &["waku"];

const CONFIG_PATTERNS: &[&str] = &["waku.config.{ts,js,mts,mjs,cts,cjs}"];

const DEFAULT_SRC_DIR: &str = "src";

/// Waku's `EXTENSIONS` constant.
const EXTENSIONS: &str = "{ts,tsx,js,jsx,mjs,cjs}";

/// Folder names the file router ignores anywhere under `pages/`.
const IGNORED_SEGMENT_REGEX: &str = "^_(components|hooks|actions)$";

const ROUTE_EXPORTS: &[&str] = &["default", "getConfig"];

/// `default` handles every method; the rest are Waku's `METHODS`.
const API_EXPORTS: &[&str] = &[
    "default",
    "getConfig",
    "GET",
    "HEAD",
    "POST",
    "PUT",
    "DELETE",
    "CONNECT",
    "OPTIONS",
    "TRACE",
    "PATCH",
    "QUERY",
];

const DEFAULT_EXPORTS: &[&str] = &["default"];

const ENTRY_PATTERNS: &[&str] = &[
    "src/pages/**/*.{ts,tsx,js,jsx,mjs,cjs}",
    "src/middleware/*.{ts,tsx,js,jsx,mjs,cjs}",
    "src/waku.{server,client}.{ts,tsx,js,jsx,mjs,cjs}",
    "src/pages.gen.ts",
];

pub struct WakuPlugin;

impl Plugin for WakuPlugin {
    fn name(&self) -> &'static str {
        "waku"
    }

    fn enablers(&self) -> &'static [&'static str] {
        ENABLERS
    }

    fn entry_patterns(&self) -> &'static [&'static str] {
        ENTRY_PATTERNS
    }

    fn entry_pattern_rules(&self) -> Vec<PathRule> {
        entry_rules(DEFAULT_SRC_DIR)
    }

    fn config_patterns(&self) -> &'static [&'static str] {
        CONFIG_PATTERNS
    }

    fn always_used(&self) -> &'static [&'static str] {
        CONFIG_PATTERNS
    }

    fn used_export_rules(&self) -> Vec<UsedExportRule> {
        used_export_rules(DEFAULT_SRC_DIR)
    }

    fn resolve_config(&self, config_path: &Path, source: &str, root: &Path) -> PluginResult {
        let mut result = PluginResult::default();

        for import in config_parser::extract_imports(source, config_path) {
            result
                .referenced_dependencies
                .push(crate::resolve::extract_package_name(&import));
        }

        let Some(src_dir) = config_parser::extract_config_string(source, config_path, &["srcDir"])
            .and_then(|raw| config_parser::normalize_config_path(raw.trim(), config_path, root))
        else {
            return result;
        };
        // A literal directory name, not a glob.
        let src_dir = globset::escape(&src_dir);

        result.replace_entry_patterns = true;
        result.replace_used_export_rules = true;
        result.entry_patterns = entry_rules(&src_dir);
        result.used_exports = used_export_rules(&src_dir);
        result
    }
}

fn entry_rules(src_dir: &str) -> Vec<PathRule> {
    vec![
        pages_rule(src_dir),
        PathRule::new(format!("{src_dir}/middleware/*.{EXTENSIONS}"))
            .with_excluded_globs([format!("{src_dir}/middleware/*.{{test,spec}}.{EXTENSIONS}")]),
        PathRule::new(server_client_pattern(src_dir)),
        // Generated route types. An entry rule rather than always-used, so a
        // custom srcDir replaces it with the rest of the defaults.
        PathRule::new(format!("{src_dir}/pages.gen.ts")),
    ]
}

fn used_export_rules(src_dir: &str) -> Vec<UsedExportRule> {
    let api_pattern = format!("{src_dir}/pages/_api/**/*.{EXTENSIONS}");
    vec![
        UsedExportRule::new(pages_pattern(src_dir), ROUTE_EXPORTS.iter().copied())
            .with_excluded_globs([api_pattern.clone()])
            .with_excluded_segment_regexes([IGNORED_SEGMENT_REGEX]),
        UsedExportRule::new(api_pattern, API_EXPORTS.iter().copied())
            .with_excluded_segment_regexes([IGNORED_SEGMENT_REGEX]),
        UsedExportRule::new(
            format!("{src_dir}/middleware/*.{EXTENSIONS}"),
            DEFAULT_EXPORTS.iter().copied(),
        ),
        UsedExportRule::new(
            server_client_pattern(src_dir),
            DEFAULT_EXPORTS.iter().copied(),
        ),
        UsedExportRule::new(CONFIG_PATTERNS[0], DEFAULT_EXPORTS.iter().copied()),
    ]
}

fn pages_rule(src_dir: &str) -> PathRule {
    PathRule::new(pages_pattern(src_dir)).with_excluded_segment_regexes([IGNORED_SEGMENT_REGEX])
}

fn pages_pattern(src_dir: &str) -> String {
    format!("{src_dir}/pages/**/*.{EXTENSIONS}")
}

fn server_client_pattern(src_dir: &str) -> String {
    format!("{src_dir}/waku.{{server,client}}.{EXTENSIONS}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry_patterns(rules: &[PathRule]) -> Vec<&str> {
        rules.iter().map(|rule| rule.pattern.as_str()).collect()
    }

    fn exports_for<'a>(rules: &'a [UsedExportRule], pattern: &str) -> Option<&'a [String]> {
        rules
            .iter()
            .find(|rule| rule.path.pattern == pattern)
            .map(|rule| rule.exports.as_slice())
    }

    #[test]
    fn default_rules_use_src_dir() {
        let plugin = WakuPlugin;
        let entries = plugin.entry_pattern_rules();

        assert_eq!(
            entry_patterns(&entries),
            vec![
                "src/pages/**/*.{ts,tsx,js,jsx,mjs,cjs}",
                "src/middleware/*.{ts,tsx,js,jsx,mjs,cjs}",
                "src/waku.{server,client}.{ts,tsx,js,jsx,mjs,cjs}",
                "src/pages.gen.ts",
            ]
        );
        assert_eq!(
            entries[0].exclude_segment_regexes,
            vec![IGNORED_SEGMENT_REGEX.to_string()],
            "the router skips _components, _hooks and _actions folders"
        );
        assert!(
            !plugin.always_used().contains(&"src/pages.gen.ts"),
            "a custom srcDir must be able to replace the generated-file rule"
        );
    }

    #[test]
    fn route_and_api_exports_are_split() {
        let rules = WakuPlugin.used_export_rules();

        let route = rules
            .iter()
            .find(|rule| rule.path.pattern == "src/pages/**/*.{ts,tsx,js,jsx,mjs,cjs}")
            .expect("route rule");
        assert_eq!(route.exports, vec!["default", "getConfig"]);
        assert_eq!(
            route.path.exclude_globs,
            vec!["src/pages/_api/**/*.{ts,tsx,js,jsx,mjs,cjs}".to_string()]
        );

        let api =
            exports_for(&rules, "src/pages/_api/**/*.{ts,tsx,js,jsx,mjs,cjs}").expect("api rule");
        assert!(api.iter().any(|name| name == "GET"));
        assert!(api.iter().any(|name| name == "QUERY"));
        assert!(api.iter().any(|name| name == "default"));
    }

    #[test]
    fn resolve_config_reads_custom_src_dir() {
        let source = r#"
            import { defineConfig } from "waku/config";
            export default defineConfig({ srcDir: "app" });
        "#;
        let root = Path::new("/project");
        let result = WakuPlugin.resolve_config(&root.join("waku.config.ts"), source, root);

        assert!(result.replace_entry_patterns);
        assert!(result.replace_used_export_rules);
        assert_eq!(
            entry_patterns(&result.entry_patterns)[0],
            "app/pages/**/*.{ts,tsx,js,jsx,mjs,cjs}"
        );
        assert!(
            exports_for(
                &result.used_exports,
                "app/pages/_api/**/*.{ts,tsx,js,jsx,mjs,cjs}"
            )
            .is_some()
        );
        let entries = entry_patterns(&result.entry_patterns);
        assert!(entries.contains(&"app/pages.gen.ts"));
        assert!(!entries.iter().any(|pattern| pattern.starts_with("src/")));
        assert!(result.referenced_dependencies.contains(&"waku".to_string()));
    }

    #[test]
    fn resolve_config_escapes_glob_characters_in_src_dir() {
        let source = r#"export default { srcDir: "app[v2]" };"#;
        let root = Path::new("/project");
        let result = WakuPlugin.resolve_config(&root.join("waku.config.ts"), source, root);

        assert_eq!(
            entry_patterns(&result.entry_patterns)[0],
            "app[[]v2[]]/pages/**/*.{ts,tsx,js,jsx,mjs,cjs}"
        );
    }

    #[test]
    fn resolve_config_without_src_dir_keeps_defaults() {
        let source = r#"
            import { defineConfig } from "waku/config";
            export default defineConfig({ vite: {} });
        "#;
        let root = Path::new("/project");
        let result = WakuPlugin.resolve_config(&root.join("waku.config.ts"), source, root);

        assert!(!result.replace_entry_patterns);
        assert!(result.entry_patterns.is_empty());
    }
}
