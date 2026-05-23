//! Fumadocs plugin.
//!
//! Detects Fumadocs projects, keeps `source.config.*` and generated `.source`
//! modules reachable, and models content roots declared in MDX collections.

use std::path::Path;

use fallow_graph::resolve::extract_package_name;
use oxc_allocator::Allocator;
use oxc_ast::ast::{Argument, CallExpression, Expression, ObjectExpression, ObjectPropertyKind};
use oxc_ast_visit::{Visit, walk};
use oxc_parser::Parser;
use oxc_span::SourceType;

use super::{Plugin, PluginResult, config_parser};

const ENABLERS: &[&str] = &["fumadocs-mdx", "fumadocs-core", "fumadocs-ui"];
const CONFIG_PATTERNS: &[&str] = &["source.config.{ts,tsx,js,jsx,mts,mjs,cts,cjs}"];
const ALWAYS_USED: &[&str] = &[
    "source.config.{ts,tsx,js,jsx,mts,mjs,cts,cjs}",
    ".source/**/*.{ts,tsx,js,jsx,mts,mjs,cts,cjs}",
];
const DISCOVERY_HIDDEN_DIRS: &[&str] = &[".source"];
const VIRTUAL_MODULE_PREFIXES: &[&str] = &["fumadocs-mdx:"];
const CONFIG_EXTENSIONS: &[&str] = &["ts", "tsx", "js", "jsx", "mts", "mjs", "cts", "cjs"];
const CONTENT_EXTENSIONS: &str = "{md,mdx,json,yml,yaml}";

/// Built-in plugin for Fumadocs MDX projects.
pub struct FumadocsPlugin;

impl Plugin for FumadocsPlugin {
    fn name(&self) -> &'static str {
        "fumadocs"
    }

    fn enablers(&self) -> &'static [&'static str] {
        ENABLERS
    }

    fn is_enabled_with_deps(&self, deps: &[String], root: &Path) -> bool {
        deps.iter()
            .any(|dep| ENABLERS.iter().any(|enabler| dep == enabler))
            || CONFIG_EXTENSIONS
                .iter()
                .any(|ext| root.join(format!("source.config.{ext}")).is_file())
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

    fn virtual_module_prefixes(&self) -> &'static [&'static str] {
        VIRTUAL_MODULE_PREFIXES
    }

    fn resolve_config(&self, config_path: &Path, source: &str, root: &Path) -> PluginResult {
        let mut result = PluginResult::default();

        for specifier in config_parser::extract_imports(source, config_path) {
            let package_name = extract_package_name(&specifier);
            if !package_name.is_empty()
                && !package_name.starts_with('.')
                && !package_name.starts_with('/')
            {
                result.referenced_dependencies.push(package_name);
            }
        }
        result.referenced_dependencies.sort();
        result.referenced_dependencies.dedup();

        result.extend_entry_patterns(
            collect_content_dirs(source, config_path, root)
                .into_iter()
                .map(|dir| format!("{dir}/**/*.{CONTENT_EXTENSIONS}")),
        );

        result
    }
}

fn collect_content_dirs(source: &str, config_path: &Path, root: &Path) -> Vec<String> {
    let source_type = SourceType::from_path(config_path).unwrap_or_default();
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, source, source_type).parse();
    let mut collector = ContentDirCollector {
        dirs: Vec::new(),
        config_path,
        root,
    };
    collector.visit_program(&parsed.program);
    collector.dirs.sort();
    collector.dirs.dedup();
    collector.dirs
}

struct ContentDirCollector<'a> {
    dirs: Vec<String>,
    config_path: &'a Path,
    root: &'a Path,
}

impl<'a> Visit<'a> for ContentDirCollector<'a> {
    fn visit_call_expression(&mut self, call: &CallExpression<'a>) {
        match call_name(call) {
            Some("defineCollections" | "defineDocs") => {
                if let Some(expr) = call.arguments.first().and_then(Argument::as_expression) {
                    self.collect_collection_definition(expr);
                }
            }
            Some("defineConfig") => {
                if let Some(expr) = call.arguments.first().and_then(Argument::as_expression) {
                    self.collect_define_config_expression(expr);
                }
            }
            _ => {}
        }

        walk::walk_call_expression(self, call);
    }
}

impl ContentDirCollector<'_> {
    fn collect_define_config_expression(&mut self, expr: &Expression<'_>) {
        match expr {
            Expression::ObjectExpression(options) => self.collect_define_config(options),
            Expression::CallExpression(call) => {
                if let Some(inner) = call.arguments.first().and_then(Argument::as_expression) {
                    self.collect_define_config_expression(inner);
                }
            }
            _ => {}
        }
    }

    fn collect_define_config(&mut self, options: &ObjectExpression<'_>) {
        if let Some(collections) = config_parser::find_property(options, "collections") {
            self.collect_collection_container(&collections.value);
        }
    }

    fn collect_collection_definition(&mut self, expr: &Expression<'_>) {
        match expr {
            Expression::ObjectExpression(object) => self.collect_dir_property(object),
            Expression::ArrayExpression(array) => {
                for element in array.elements.iter().filter_map(|el| el.as_expression()) {
                    self.collect_collection_definition(element);
                }
            }
            _ => {}
        }
    }

    fn collect_collection_container(&mut self, expr: &Expression<'_>) {
        match expr {
            Expression::ObjectExpression(object) => {
                for prop in &object.properties {
                    let ObjectPropertyKind::ObjectProperty(prop) = prop else {
                        continue;
                    };
                    self.collect_collection_definition(&prop.value);
                }
            }
            Expression::ArrayExpression(array) => {
                for element in array.elements.iter().filter_map(|el| el.as_expression()) {
                    self.collect_collection_definition(element);
                }
            }
            _ => {}
        }
    }

    fn collect_dir_property(&mut self, object: &ObjectExpression<'_>) {
        let Some(prop) = config_parser::find_property(object, "dir") else {
            return;
        };
        let Some(raw_dir) = config_parser::expression_to_path_string(&prop.value) else {
            return;
        };
        if let Some(dir) =
            config_parser::normalize_config_path(&raw_dir, self.config_path, self.root)
        {
            self.dirs.push(dir);
        }
    }
}

fn call_name<'a>(call: &'a CallExpression<'a>) -> Option<&'a str> {
    match &call.callee {
        Expression::Identifier(identifier) => Some(identifier.name.as_str()),
        Expression::StaticMemberExpression(member) => Some(member.property.name.as_str()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn activates_from_packages_or_source_config_file() {
        let plugin = FumadocsPlugin;
        let tmp = tempfile::tempdir().expect("temp dir");

        assert!(plugin.is_enabled_with_deps(&["fumadocs-mdx".to_string()], tmp.path()));
        assert!(!plugin.is_enabled_with_deps(&["next".to_string()], tmp.path()));

        std::fs::write(tmp.path().join("source.config.ts"), "export default {};\n")
            .expect("source config");
        assert!(plugin.is_enabled_with_deps(&[], tmp.path()));
    }

    #[test]
    fn exposes_static_fumadocs_conventions() {
        let plugin = FumadocsPlugin;

        assert_eq!(plugin.config_patterns(), CONFIG_PATTERNS);
        assert!(
            plugin
                .always_used()
                .contains(&"source.config.{ts,tsx,js,jsx,mts,mjs,cts,cjs}")
        );
        assert!(
            plugin
                .always_used()
                .contains(&".source/**/*.{ts,tsx,js,jsx,mts,mjs,cts,cjs}")
        );
        assert_eq!(plugin.discovery_hidden_dirs(), DISCOVERY_HIDDEN_DIRS);
        assert_eq!(plugin.virtual_module_prefixes(), VIRTUAL_MODULE_PREFIXES);
    }

    #[test]
    fn extracts_content_dirs_and_imported_config_packages() {
        let plugin = FumadocsPlugin;
        let root = Path::new("/repo/apps/docs");
        let config_path = root.join("source.config.ts");
        let source = r"
            import { defineCollections, defineConfig } from 'fumadocs-mdx/config';
            import { transformer } from '@acme/fumadocs-preset';

            const docs = defineCollections({
                type: 'doc',
                dir: './content/docs',
                transform: transformer,
            });

            export default defineConfig({
                collections: [
                    docs,
                    { type: 'doc', dir: 'content/blog' },
                ],
            });
        ";

        let result = plugin.resolve_config(&config_path, source, root);
        let patterns: Vec<String> = result
            .entry_patterns
            .iter()
            .map(|rule| rule.pattern.clone())
            .collect();

        assert!(patterns.contains(&"content/blog/**/*.{md,mdx,json,yml,yaml}".to_string()));
        assert!(patterns.contains(&"content/docs/**/*.{md,mdx,json,yml,yaml}".to_string()));
        assert!(
            result
                .referenced_dependencies
                .contains(&"fumadocs-mdx".to_string())
        );
        assert!(
            result
                .referenced_dependencies
                .contains(&"@acme/fumadocs-preset".to_string())
        );
    }

    #[test]
    fn direct_collections_do_not_harvest_nested_option_dirs() {
        let plugin = FumadocsPlugin;
        let root = Path::new("/repo/apps/docs");
        let config_path = root.join("source.config.ts");
        let source = r"
            import { defineConfig } from 'fumadocs-mdx/config';

            export default defineConfig({
                collections: [
                    {
                        type: 'doc',
                        dir: 'content/docs',
                        meta: { dir: 'src/internal' },
                    },
                ],
            });
        ";

        let result = plugin.resolve_config(&config_path, source, root);
        let patterns: Vec<String> = result
            .entry_patterns
            .iter()
            .map(|rule| rule.pattern.clone())
            .collect();

        assert!(patterns.contains(&"content/docs/**/*.{md,mdx,json,yml,yaml}".to_string()));
        assert!(
            !patterns.contains(&"src/internal/**/*.{md,mdx,json,yml,yaml}".to_string()),
            "nested option dirs should not become Fumadocs content roots: {patterns:?}"
        );
    }
}
