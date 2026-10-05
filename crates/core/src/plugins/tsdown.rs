//! tsdown TypeScript library bundler plugin.
//!
//! Detects tsdown projects and marks config files as always used.
//! Parses tsdown config to extract referenced dependencies.

use std::path::Path;

use oxc_ast::ast::Expression;

use super::config_parser;
use super::{Plugin, PluginResult};

const ENABLERS: &[&str] = &["tsdown"];

const CONFIG_PATTERNS: &[&str] = &["tsdown.config.{ts,mts,cts,js,cjs,mjs,json}"];

const ALWAYS_USED: &[&str] = &["tsdown.config.{ts,mts,cts,js,cjs,mjs,json}"];

const TOOLING_DEPENDENCIES: &[&str] = &["tsdown"];

/// The config file names that tsdown loads in each workspace package.
const WORKSPACE_CONFIG_FILE: &str = "tsdown.config.{ts,mts,cts,js,cjs,mjs,json}";

/// The `workspace.include` value that tells tsdown to read the package
/// manager workspaces. It is not a glob.
const WORKSPACE_INCLUDE_AUTO: &str = "auto";

define_plugin! {
    struct TsdownPlugin => "tsdown",
    enablers: ENABLERS,
    config_patterns: CONFIG_PATTERNS,
    always_used: ALWAYS_USED,
    tooling_dependencies: TOOLING_DEPENDENCIES,
    package_json_config_key: "tsdown",
    resolve_config(config_path, source, root) {
        let mut result = PluginResult::default();
        super::add_import_referenced_dependencies(&mut result, source, config_path);

        let entries = config_parser::extract_config_array_or_object_string_or_array(source, config_path, &["entry"]);
        result.extend_config_dir_entry_patterns(entries, config_path, root);

        result
            .always_used_files
            .extend(workspace_config_patterns(source, config_path, root));

        result
    }
}

/// The config files of the packages that the `workspace` option matches.
///
/// tsdown builds each package that a `workspace` glob matches and loads the
/// config file in that package. The option is a glob, a glob array, or an
/// object with an `include` glob or glob array. Each glob is relative to the
/// config file directory.
fn workspace_config_patterns(source: &str, config_path: &Path, root: &Path) -> Vec<String> {
    let globs = config_parser::extract_from_source(source, config_path, |program| {
        let config = config_parser::find_config_object(program)?;
        let workspace = config_parser::find_property(config, "workspace")?;
        let mut globs = Vec::new();
        collect_workspace_globs(&workspace.value, &mut globs);
        Some(globs)
    })
    .unwrap_or_default();

    let mut patterns: Vec<String> = globs
        .iter()
        .filter(|glob| glob.as_str() != WORKSPACE_INCLUDE_AUTO && !glob.starts_with('!'))
        .filter_map(|glob| {
            config_parser::normalize_config_path(glob.trim_end_matches('/'), config_path, root)
        })
        .map(|directory| format!("{directory}/{WORKSPACE_CONFIG_FILE}"))
        .collect();
    patterns.sort_unstable();
    patterns.dedup();
    patterns
}

/// Collect the glob strings of a `workspace` value. A conditional value
/// contributes the globs of both branches, because either branch can run.
fn collect_workspace_globs(value: &Expression<'_>, globs: &mut Vec<String>) {
    match value {
        Expression::ConditionalExpression(conditional) => {
            collect_workspace_globs(&conditional.consequent, globs);
            collect_workspace_globs(&conditional.alternate, globs);
        }
        Expression::ObjectExpression(object) => {
            if let Some(include) = config_parser::find_property(object, "include") {
                collect_workspace_globs(&include.value, globs);
            }
        }
        Expression::ArrayExpression(array) => globs.extend(
            array
                .elements
                .iter()
                .filter_map(|element| element.as_expression())
                .filter_map(config_parser::expression_to_string),
        ),
        _ => globs.extend(config_parser::expression_to_string(value)),
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    #[test]
    fn resolve_config_entry_array() {
        let source = r#"
            export default {
                entry: ["src/index.ts", "src/cli.ts"]
            };
        "#;
        let plugin = TsdownPlugin;
        let result = plugin.resolve_config(
            Path::new("/project/tsdown.config.ts"),
            source,
            Path::new("/project"),
        );
        assert_eq!(result.entry_patterns, vec!["src/index.ts", "src/cli.ts"]);
    }

    #[test]
    fn resolve_config_entry_single() {
        let source = r#"
            export default {
                entry: ["src/index.ts"]
            };
        "#;
        let plugin = TsdownPlugin;
        let result = plugin.resolve_config(
            Path::new("/project/tsdown.config.ts"),
            source,
            Path::new("/project"),
        );
        assert_eq!(result.entry_patterns, vec!["src/index.ts"]);
    }

    #[test]
    fn resolve_config_imports() {
        let source = r#"
            import { defineConfig } from 'tsdown';
            import react from '@vitejs/plugin-react';
            export default defineConfig({
                entry: ["src/index.ts"]
            });
        "#;
        let plugin = TsdownPlugin;
        let result = plugin.resolve_config(
            Path::new("/project/tsdown.config.ts"),
            source,
            Path::new("/project"),
        );
        assert!(
            result
                .referenced_dependencies
                .contains(&"tsdown".to_string())
        );
        assert!(
            result
                .referenced_dependencies
                .contains(&"@vitejs/plugin-react".to_string())
        );
        assert_eq!(result.entry_patterns, vec!["src/index.ts"]);
    }

    #[test]
    fn resolve_config_empty() {
        let source = r"export default {};";
        let plugin = TsdownPlugin;
        let result = plugin.resolve_config(
            Path::new("/project/tsdown.config.ts"),
            source,
            Path::new("/project"),
        );
        assert!(result.entry_patterns.is_empty());
        assert!(result.referenced_dependencies.is_empty());
    }

    #[test]
    fn resolve_config_no_entry() {
        let source = r#"
            export default {
                format: ["cjs", "esm"]
            };
        "#;
        let plugin = TsdownPlugin;
        let result = plugin.resolve_config(
            Path::new("/project/tsdown.config.ts"),
            source,
            Path::new("/project"),
        );
        assert!(result.entry_patterns.is_empty());
    }

    #[test]
    fn resolve_config_define_config() {
        let source = r#"
            import { defineConfig } from 'tsdown';
            export default defineConfig({
                entry: ["src/main.ts", "src/worker.ts"]
            });
        "#;
        let plugin = TsdownPlugin;
        let result = plugin.resolve_config(
            Path::new("/project/tsdown.config.ts"),
            source,
            Path::new("/project"),
        );
        assert_eq!(result.entry_patterns, vec!["src/main.ts", "src/worker.ts"]);
        assert!(
            result
                .referenced_dependencies
                .contains(&"tsdown".to_string())
        );
    }

    #[test]
    fn resolve_config_entry_string() {
        let source = r#"
            import { defineConfig } from 'tsdown';
            export default defineConfig({
                entry: "src/index.ts"
            });
        "#;
        let plugin = TsdownPlugin;
        let result = plugin.resolve_config(
            Path::new("/project/tsdown.config.ts"),
            source,
            Path::new("/project"),
        );
        assert_eq!(result.entry_patterns, vec!["src/index.ts"]);
    }

    #[test]
    fn resolve_config_entry_object_map() {
        let source = r#"
            import { defineConfig } from 'tsdown';
            export default defineConfig({
                entry: { main: "src/main.ts", worker: "src/worker.ts" }
            });
        "#;
        let plugin = TsdownPlugin;
        let result = plugin.resolve_config(
            Path::new("/project/tsdown.config.ts"),
            source,
            Path::new("/project"),
        );
        assert_eq!(result.entry_patterns, vec!["src/main.ts", "src/worker.ts"]);
    }

    #[test]
    fn resolve_config_nested_entry_resolves_from_config_directory() {
        let source = r#"
            export default {
                entry: ["src/cli.ts", "./src/index.ts", "src/bin/*.ts", "!src/bin/skip.ts"]
            };
        "#;
        let plugin = TsdownPlugin;
        let result = plugin.resolve_config(
            Path::new("/project/packages/lib/tsdown.config.ts"),
            source,
            Path::new("/project"),
        );
        assert_eq!(
            result.entry_patterns,
            vec![
                "packages/lib/src/cli.ts",
                "packages/lib/src/index.ts",
                "packages/lib/src/bin/*.ts",
            ]
        );
        assert!(
            result
                .entry_patterns
                .iter()
                .all(|rule| rule.exclude_globs == ["packages/lib/src/bin/skip.ts"])
        );
    }

    #[test]
    fn resolve_config_entry_resolves_from_workspace_root() {
        let source = r#"
            export default {
                entry: ["src/cli.ts"]
            };
        "#;
        let plugin = TsdownPlugin;
        let result = plugin.resolve_config(
            Path::new("/project/packages/lib/tsdown.config.ts"),
            source,
            Path::new("/project/packages/lib"),
        );
        assert_eq!(result.entry_patterns, vec!["src/cli.ts"]);
    }

    #[test]
    fn resolve_config_entry_from_each_config_in_array() {
        let source = r#"
            import { defineConfig } from 'tsdown';
            const shared = { format: ["esm"] };
            export default defineConfig([
                { entry: ["src/main.ts"] },
                { ...shared, entry: { worker: "src/worker.ts" } },
            ]);
        "#;
        let plugin = TsdownPlugin;
        let result = plugin.resolve_config(
            Path::new("/project/tsdown.config.ts"),
            source,
            Path::new("/project"),
        );
        assert_eq!(result.entry_patterns, vec!["src/main.ts", "src/worker.ts"]);
    }

    #[test]
    fn resolve_config_entry_from_plain_exported_array() {
        let source = r#"
            export default [
                { entry: "src/main.ts" },
                { entry: ["src/worker.ts"] },
            ];
        "#;
        let plugin = TsdownPlugin;
        let result = plugin.resolve_config(
            Path::new("/project/tsdown.config.ts"),
            source,
            Path::new("/project"),
        );
        assert_eq!(result.entry_patterns, vec!["src/main.ts", "src/worker.ts"]);
    }

    #[test]
    fn resolve_config_json_entry_object_map() {
        let source = r#"{ "entry": { "index": "src/index.ts", "cli": "src/cli.ts" } }"#;
        let result = TsdownPlugin.resolve_config(
            Path::new("/project/tsdown.config.json"),
            source,
            Path::new("/project"),
        );
        assert_eq!(result.entry_patterns, vec!["src/index.ts", "src/cli.ts"]);
    }

    #[test]
    fn reads_package_json_config_key() {
        assert_eq!(TsdownPlugin.package_json_config_key(), Some("tsdown"));
    }

    fn workspace_patterns(config_path: &str, source: &str) -> Vec<String> {
        TsdownPlugin
            .resolve_config(Path::new(config_path), source, Path::new("/project"))
            .always_used_files
    }

    #[test]
    fn resolve_config_workspace_array_marks_package_configs() {
        let source = r#"export default { workspace: ["packages/*", "./apps/cli/"] };"#;
        assert_eq!(
            workspace_patterns("/project/tsdown.config.ts", source),
            vec![
                "apps/cli/tsdown.config.{ts,mts,cts,js,cjs,mjs,json}",
                "packages/*/tsdown.config.{ts,mts,cts,js,cjs,mjs,json}",
            ]
        );
    }

    #[test]
    fn resolve_config_workspace_include_object_marks_package_configs() {
        let source = r#"
            import { defineConfig } from 'tsdown';
            export default defineConfig({
                workspace: { include: ["packages/*"], exclude: ["packages/skip"] }
            });
        "#;
        assert_eq!(
            workspace_patterns("/project/tsdown.config.ts", source),
            vec!["packages/*/tsdown.config.{ts,mts,cts,js,cjs,mjs,json}"]
        );
    }

    #[test]
    fn resolve_config_workspace_string_resolves_from_config_directory() {
        let source = r#"export default { workspace: "libs/*" };"#;
        assert_eq!(
            workspace_patterns("/project/tools/tsdown.config.ts", source),
            vec!["tools/libs/*/tsdown.config.{ts,mts,cts,js,cjs,mjs,json}"]
        );
    }

    #[test]
    fn resolve_config_workspace_conditional_marks_both_branches() {
        let source = r"
            import { defineConfig } from 'tsdown';
            export default defineConfig(({ env }) => {
                const client = env?.FACE === 'client';
                return {
                    workspace: client ? ['vendor/*', 'apps/cli'] : ['vendor/*', 'apps/host'],
                };
            });
        ";
        assert_eq!(
            workspace_patterns("/project/tsdown.config.ts", source),
            vec![
                "apps/cli/tsdown.config.{ts,mts,cts,js,cjs,mjs,json}",
                "apps/host/tsdown.config.{ts,mts,cts,js,cjs,mjs,json}",
                "vendor/*/tsdown.config.{ts,mts,cts,js,cjs,mjs,json}",
            ]
        );
    }

    #[test]
    fn resolve_config_workspace_without_globs_marks_nothing() {
        for source in [
            "export default { workspace: true };",
            r#"export default { workspace: { include: "auto" } };"#,
            r#"export default { entry: ["src/index.ts"] };"#,
        ] {
            assert!(
                workspace_patterns("/project/tsdown.config.ts", source).is_empty(),
                "source: {source}"
            );
        }
    }
}
