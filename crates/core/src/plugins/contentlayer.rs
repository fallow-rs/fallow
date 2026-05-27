//! Contentlayer plugin.
//!
//! Detects Contentlayer projects, keeps `contentlayer.config.*`, generated
//! `.contentlayer` modules, and statically declared content roots reachable.

use std::path::Path;

use fallow_graph::resolve::extract_package_name;
use oxc_allocator::Allocator;
use oxc_ast::ast::{
    Argument, CallExpression, Expression, FunctionBody, ObjectExpression, Statement,
};
use oxc_ast_visit::{Visit, walk};
use oxc_parser::Parser;
use oxc_span::SourceType;

use super::{Plugin, PluginResult, config_parser};

const ENABLERS: &[&str] = &[
    "contentlayer",
    "contentlayer2",
    "next-contentlayer",
    "next-contentlayer2",
];
const CONFIG_PATTERNS: &[&str] = &["contentlayer.config.{ts,js,mts,mjs}"];
const ALWAYS_USED: &[&str] = &[
    "contentlayer.config.{ts,js,mts,mjs}",
    ".contentlayer/**/*.{ts,tsx,js,jsx,mts,mjs,cts,cjs}",
];
const DISCOVERY_HIDDEN_DIRS: &[&str] = &[".contentlayer"];
const CONFIG_EXTENSIONS: &[&str] = &["ts", "js", "mts", "mjs"];
const CONTENT_EXTENSIONS: &str = "{md,mdx,json,yml,yaml}";

/// Built-in plugin for Contentlayer projects.
pub struct ContentlayerPlugin;

impl Plugin for ContentlayerPlugin {
    fn name(&self) -> &'static str {
        "contentlayer"
    }

    fn enablers(&self) -> &'static [&'static str] {
        ENABLERS
    }

    fn tooling_dependencies(&self) -> &'static [&'static str] {
        ENABLERS
    }

    fn is_enabled_with_deps(&self, deps: &[String], root: &Path) -> bool {
        deps.iter()
            .any(|dep| ENABLERS.iter().any(|enabler| dep == enabler))
            || CONFIG_EXTENSIONS
                .iter()
                .any(|ext| root.join(format!("contentlayer.config.{ext}")).is_file())
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

        result.extend_entry_patterns(collect_content_patterns(source, config_path, root));

        result
    }
}

fn collect_content_patterns(source: &str, config_path: &Path, root: &Path) -> Vec<String> {
    let source_type = SourceType::from_path(config_path).unwrap_or_default();
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, source, source_type).parse();
    let mut collector = ContentlayerCollector {
        content_dirs: Vec::new(),
        document_patterns: Vec::new(),
        config_path,
        root,
    };
    collector.visit_program(&parsed.program);
    collector.into_patterns()
}

struct ContentlayerCollector<'a> {
    content_dirs: Vec<String>,
    document_patterns: Vec<String>,
    config_path: &'a Path,
    root: &'a Path,
}

impl<'a> Visit<'a> for ContentlayerCollector<'a> {
    fn visit_call_expression(&mut self, call: &CallExpression<'a>) {
        match call_name(call) {
            Some("makeSource" | "makeSourceConfig") => {
                if let Some(options) = call.arguments.first().and_then(object_from_argument) {
                    self.collect_content_dir(options);
                }
            }
            Some("defineDocumentType") => {
                if let Some(options) = call.arguments.first().and_then(object_from_argument) {
                    self.collect_document_pattern(options);
                }
            }
            _ => {}
        }

        walk::walk_call_expression(self, call);
    }
}

impl ContentlayerCollector<'_> {
    fn collect_content_dir(&mut self, options: &ObjectExpression<'_>) {
        let Some(prop) = config_parser::find_property(options, "contentDirPath") else {
            return;
        };
        let Some(raw_dir) = config_parser::expression_to_path_string(&prop.value) else {
            return;
        };
        if let Some(dir) =
            config_parser::normalize_config_path(&raw_dir, self.config_path, self.root)
        {
            self.content_dirs.push(dir);
        }
    }

    fn collect_document_pattern(&mut self, options: &ObjectExpression<'_>) {
        let Some(prop) = config_parser::find_property(options, "filePathPattern") else {
            return;
        };
        let Some(pattern) = config_parser::expression_to_path_string(&prop.value) else {
            return;
        };
        let trimmed = pattern.trim_start_matches("./");
        if !trimmed.is_empty() && !trimmed.starts_with("../") && !trimmed.starts_with('/') {
            self.document_patterns.push(trimmed.to_string());
        }
    }

    fn into_patterns(mut self) -> Vec<String> {
        let mut patterns = Vec::new();

        self.content_dirs.sort();
        self.content_dirs.dedup();
        self.document_patterns.sort();
        self.document_patterns.dedup();

        for dir in &self.content_dirs {
            patterns.push(format!("{dir}/**/*.{CONTENT_EXTENSIONS}"));

            for document_pattern in &self.document_patterns {
                patterns.push(format!(
                    "{}/{}",
                    dir.trim_end_matches('/'),
                    document_pattern.trim_start_matches('/')
                ));
            }
        }

        patterns.sort();
        patterns.dedup();
        patterns
    }
}

fn object_from_argument<'a>(argument: &'a Argument<'a>) -> Option<&'a ObjectExpression<'a>> {
    match argument {
        Argument::ObjectExpression(object) => Some(object),
        Argument::ArrowFunctionExpression(arrow) => object_from_function_body(&arrow.body),
        Argument::FunctionExpression(function) => function
            .body
            .as_ref()
            .and_then(|body| object_from_function_body(body)),
        _ => argument.as_expression().and_then(object_from_expression),
    }
}

fn object_from_expression<'a>(expr: &'a Expression<'a>) -> Option<&'a ObjectExpression<'a>> {
    match expr {
        Expression::ObjectExpression(object) => Some(object),
        Expression::ParenthesizedExpression(paren) => object_from_expression(&paren.expression),
        Expression::TSSatisfiesExpression(ts_sat) => object_from_expression(&ts_sat.expression),
        Expression::TSAsExpression(ts_as) => object_from_expression(&ts_as.expression),
        Expression::ArrowFunctionExpression(arrow) => object_from_function_body(&arrow.body),
        Expression::FunctionExpression(function) => function
            .body
            .as_ref()
            .and_then(|body| object_from_function_body(body)),
        _ => None,
    }
}

fn object_from_function_body<'a>(body: &'a FunctionBody<'a>) -> Option<&'a ObjectExpression<'a>> {
    for statement in &body.statements {
        match statement {
            Statement::ExpressionStatement(expr_stmt) => {
                if let Some(object) = object_from_expression(&expr_stmt.expression) {
                    return Some(object);
                }
            }
            Statement::ReturnStatement(return_statement) => {
                let Some(argument) = &return_statement.argument else {
                    continue;
                };
                if let Some(object) = object_from_expression(argument) {
                    return Some(object);
                }
            }
            _ => {}
        }
    }
    None
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
    fn activates_from_packages_or_contentlayer_config_file() {
        let plugin = ContentlayerPlugin;
        let tmp = tempfile::tempdir().expect("temp dir");

        for package in ENABLERS {
            assert!(plugin.is_enabled_with_deps(&[(*package).to_string()], tmp.path()));
        }
        assert!(!plugin.is_enabled_with_deps(&["next".to_string()], tmp.path()));

        std::fs::write(
            tmp.path().join("contentlayer.config.ts"),
            "export default {};\n",
        )
        .expect("contentlayer config");
        assert!(plugin.is_enabled_with_deps(&[], tmp.path()));
    }

    #[test]
    fn exposes_static_contentlayer_conventions() {
        let plugin = ContentlayerPlugin;

        assert_eq!(plugin.config_patterns(), CONFIG_PATTERNS);
        assert_eq!(plugin.tooling_dependencies(), ENABLERS);
        assert!(
            plugin
                .always_used()
                .contains(&"contentlayer.config.{ts,js,mts,mjs}")
        );
        assert!(
            plugin
                .always_used()
                .contains(&".contentlayer/**/*.{ts,tsx,js,jsx,mts,mjs,cts,cjs}")
        );
        assert_eq!(plugin.discovery_hidden_dirs(), DISCOVERY_HIDDEN_DIRS);
    }

    #[test]
    fn extracts_content_roots_and_imported_config_packages() {
        let plugin = ContentlayerPlugin;
        let root = Path::new("/repo/apps/web");
        let config_path = root.join("contentlayer.config.ts");
        let source = r"
            import { defineDocumentType, makeSource } from 'contentlayer2/source-files';
            import remarkGfm from 'remark-gfm';
            import rehypeSlug from 'rehype-slug';

            export const Blog = defineDocumentType(() => ({
                name: 'Blog',
                filePathPattern: 'blog/**/*.mdx',
                contentType: 'mdx',
            }));

            export const Authors = defineDocumentType(function () {
                return {
                    name: 'Authors',
                    filePathPattern: './authors/**/*.mdx',
                    contentType: 'mdx',
                };
            });

            export default makeSource({
                contentDirPath: 'data',
                documentTypes: [Blog, Authors],
                mdx: {
                    remarkPlugins: [remarkGfm],
                    rehypePlugins: [rehypeSlug],
                },
            });
        ";

        let result = plugin.resolve_config(&config_path, source, root);
        let patterns: Vec<String> = result
            .entry_patterns
            .iter()
            .map(|rule| rule.pattern.clone())
            .collect();

        assert!(patterns.contains(&"data/**/*.{md,mdx,json,yml,yaml}".to_string()));
        assert!(patterns.contains(&"data/authors/**/*.mdx".to_string()));
        assert!(patterns.contains(&"data/blog/**/*.mdx".to_string()));
        assert!(
            result
                .referenced_dependencies
                .contains(&"contentlayer2".to_string())
        );
        assert!(
            result
                .referenced_dependencies
                .contains(&"remark-gfm".to_string())
        );
        assert!(
            result
                .referenced_dependencies
                .contains(&"rehype-slug".to_string())
        );
    }

    #[test]
    fn skips_document_patterns_that_escape_content_dir() {
        let plugin = ContentlayerPlugin;
        let root = Path::new("/repo/apps/web");
        let config_path = root.join("contentlayer.config.ts");
        let source = r"
            import { defineDocumentType, makeSource } from 'contentlayer/source-files';

            const Unsafe = defineDocumentType(() => ({
                name: 'Unsafe',
                filePathPattern: '../outside/**/*.mdx',
            }));

            export default makeSource({
                contentDirPath: 'data',
                documentTypes: [Unsafe],
            });
        ";

        let result = plugin.resolve_config(&config_path, source, root);
        let patterns: Vec<String> = result
            .entry_patterns
            .iter()
            .map(|rule| rule.pattern.clone())
            .collect();

        assert_eq!(patterns, vec!["data/**/*.{md,mdx,json,yml,yaml}"]);
    }
}
