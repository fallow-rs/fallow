//! Velite plugin.
//!
//! Detects Velite projects, keeps `velite.config.*` and generated `.velite`
//! collection output reachable, and models content roots declared via
//! `defineConfig` / `defineCollection` so Velite-managed markdown / MDX content
//! is not reported as unused.

use std::path::Path;

use fallow_graph::resolve::extract_package_name;
use oxc_allocator::Allocator;
use oxc_ast::ast::{Argument, CallExpression, Expression, ObjectExpression};
use oxc_ast_visit::{Visit, walk};
use oxc_parser::Parser;
use oxc_span::SourceType;

use super::{Plugin, PluginResult, config_parser};

const ENABLERS: &[&str] = &["velite"];
const CONFIG_PATTERNS: &[&str] = &["velite.config.{ts,mts,cts,js,mjs,cjs}"];
const ALWAYS_USED: &[&str] = &["velite.config.{ts,mts,cts,js,mjs,cjs}", ".velite/**"];
const DISCOVERY_HIDDEN_DIRS: &[&str] = &[".velite"];
const TOOLING_DEPENDENCIES: &[&str] = &["velite"];
const CONFIG_EXTENSIONS: &[&str] = &["ts", "mts", "cts", "js", "mjs", "cjs"];
const CONTENT_EXTENSIONS: &str = "{md,mdx,yml,yaml,json}";
/// Velite's default content root when `root` is omitted from the config.
const DEFAULT_ROOT: &str = "content";
/// Velite's default generated-output directory when `output.data` is omitted.
const DEFAULT_OUTPUT_DATA: &str = ".velite";

/// Built-in plugin for Velite content-pipeline projects.
pub struct VelitePlugin;

impl Plugin for VelitePlugin {
    fn name(&self) -> &'static str {
        "velite"
    }

    fn enablers(&self) -> &'static [&'static str] {
        ENABLERS
    }

    fn is_enabled_with_deps(&self, deps: &[String], root: &Path) -> bool {
        deps.iter()
            .any(|dep| ENABLERS.iter().any(|enabler| dep == enabler))
            || CONFIG_EXTENSIONS
                .iter()
                .any(|ext| root.join(format!("velite.config.{ext}")).is_file())
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

    fn tooling_dependencies(&self) -> &'static [&'static str] {
        TOOLING_DEPENDENCIES
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

        let collected = collect_config(source, config_path);

        // Content root: top-level `root` (default `content`), normalized
        // config-relative. Collection `pattern` globs are relative to it.
        let root_dir = collected
            .root_dir
            .as_deref()
            .and_then(|raw| config_parser::normalize_config_path(raw, config_path, root))
            .or_else(|| config_parser::normalize_config_path(DEFAULT_ROOT, config_path, root));

        if let Some(root_dir) = root_dir {
            // Keep only positive globs. Fast-glob negations (`!posts/private/**`)
            // exclude files; they are not content entry roots.
            let positive: Vec<&str> = collected
                .patterns
                .iter()
                .filter(|pattern| !pattern.starts_with('!'))
                .map(|pattern| pattern.trim_start_matches("./"))
                .filter(|pattern| !pattern.is_empty())
                .collect();

            // Fall back to the whole content root when no positive collection
            // pattern survives (no `defineCollection`, or negation-only globs).
            if positive.is_empty() {
                result.push_entry_pattern(format!("{root_dir}/**/*.{CONTENT_EXTENSIONS}"));
            } else {
                for pattern in positive {
                    result.push_entry_pattern(format!("{root_dir}/{pattern}"));
                }
            }
        }

        // Generated output: the default `.velite` is covered by the static
        // `always_used` glob (matched anywhere, including workspace packages).
        // Only a non-default `output.data` needs an explicit, config-relative
        // always-used entry. Compare the raw value so a monorepo config that
        // spells out the default does not add a redundant entry.
        if let Some(output_dir) = collected
            .output_data
            .as_deref()
            .filter(|raw| raw.trim_start_matches("./") != DEFAULT_OUTPUT_DATA)
            .and_then(|raw| config_parser::normalize_config_path(raw, config_path, root))
        {
            result.always_used_files.push(format!("{output_dir}/**"));
        }

        result
    }
}

#[derive(Default)]
struct CollectedConfig {
    /// Raw `root` value from `defineConfig`, config-relative.
    root_dir: Option<String>,
    /// Raw `output.data` value from `defineConfig`, config-relative.
    output_data: Option<String>,
    /// Collection `pattern` globs, relative to `root_dir`.
    patterns: Vec<String>,
}

fn collect_config(source: &str, config_path: &Path) -> CollectedConfig {
    let source_type = SourceType::from_path(config_path).unwrap_or_default();
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, source, source_type).parse();

    let mut collector = ConfigCollector::default();

    // Top-level `root` / `output.data` from the default-export config object
    // (handles `defineConfig({...})`, bare object, `satisfies`/`as`, const ref).
    if let Some(config) = config_parser::find_config_object_pub(&parsed.program) {
        collector.root_dir = config_parser::find_property(config, "root")
            .and_then(|prop| config_parser::expression_to_path_string(&prop.value));
        collector.output_data = config_parser::find_property(config, "output")
            .and_then(|prop| config_parser::object_expression(&prop.value))
            .and_then(|output| config_parser::find_property(output, "data"))
            .and_then(|prop| config_parser::expression_to_path_string(&prop.value));
    }

    // Collection patterns from every `defineCollection(...)` call, wherever it
    // appears (inline in `collections`, or extracted to a `const`).
    collector.visit_program(&parsed.program);
    collector.patterns.sort();
    collector.patterns.dedup();

    CollectedConfig {
        root_dir: collector.root_dir,
        output_data: collector.output_data,
        patterns: collector.patterns,
    }
}

#[derive(Default)]
struct ConfigCollector {
    root_dir: Option<String>,
    output_data: Option<String>,
    patterns: Vec<String>,
}

impl<'a> Visit<'a> for ConfigCollector {
    fn visit_call_expression(&mut self, call: &CallExpression<'a>) {
        if call_name(call) == Some("defineCollection")
            && let Some(Expression::ObjectExpression(options)) =
                call.arguments.first().and_then(Argument::as_expression)
        {
            self.collect_pattern(options);
        }

        walk::walk_call_expression(self, call);
    }
}

impl ConfigCollector {
    fn collect_pattern(&mut self, options: &ObjectExpression<'_>) {
        let Some(prop) = config_parser::find_property(options, "pattern") else {
            return;
        };
        push_string_or_array(&prop.value, &mut self.patterns);
    }
}

/// Collect string-literal values from a `string | string[]` expression.
fn push_string_or_array(expr: &Expression<'_>, out: &mut Vec<String>) {
    match expr {
        Expression::ArrayExpression(array) => {
            for element in array.elements.iter().filter_map(|el| el.as_expression()) {
                if let Some(value) = config_parser::expression_to_string(element) {
                    out.push(value);
                }
            }
        }
        _ => {
            if let Some(value) = config_parser::expression_to_string(expr) {
                out.push(value);
            }
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
    fn activates_from_packages_or_config_file() {
        let plugin = VelitePlugin;
        let tmp = tempfile::tempdir().expect("temp dir");

        assert!(plugin.is_enabled_with_deps(&["velite".to_string()], tmp.path()));
        assert!(!plugin.is_enabled_with_deps(&["next".to_string()], tmp.path()));

        for ext in CONFIG_EXTENSIONS {
            let cfg = tmp.path().join(format!("velite.config.{ext}"));
            std::fs::write(&cfg, "export default {};\n").expect("config");
            assert!(
                plugin.is_enabled_with_deps(&[], tmp.path()),
                "velite.config.{ext} should activate the plugin"
            );
            std::fs::remove_file(&cfg).expect("remove config");
        }
    }

    #[test]
    fn exposes_static_velite_conventions() {
        let plugin = VelitePlugin;

        assert_eq!(plugin.config_patterns(), CONFIG_PATTERNS);
        assert!(
            plugin
                .always_used()
                .contains(&"velite.config.{ts,mts,cts,js,mjs,cjs}")
        );
        assert!(plugin.always_used().contains(&".velite/**"));
        assert_eq!(plugin.discovery_hidden_dirs(), DISCOVERY_HIDDEN_DIRS);
        assert!(plugin.tooling_dependencies().contains(&"velite"));
    }

    fn patterns_of(result: &PluginResult) -> Vec<String> {
        result
            .entry_patterns
            .iter()
            .map(|rule| rule.pattern.clone())
            .collect()
    }

    #[test]
    fn extracts_content_roots_and_imported_config_packages() {
        let plugin = VelitePlugin;
        let root = Path::new("/repo");
        let config_path = root.join("velite.config.ts");
        let source = r"
            import { defineConfig, defineCollection, s } from 'velite';
            import rehypeShiki from '@shikijs/rehype';

            const posts = defineCollection({
                name: 'Post',
                pattern: 'blog/**/*.mdx',
                schema: s.object({}),
            });

            export default defineConfig({
                root: 'content',
                output: { data: '.velite', assets: 'public/static' },
                collections: { posts },
            });
        ";

        let result = plugin.resolve_config(&config_path, source, root);
        let patterns = patterns_of(&result);

        assert!(patterns.contains(&"content/blog/**/*.mdx".to_string()));
        assert!(
            result
                .referenced_dependencies
                .contains(&"velite".to_string())
        );
        assert!(
            result
                .referenced_dependencies
                .contains(&"@shikijs/rehype".to_string())
        );
        // Default output dir is covered by static always_used; no extra entry.
        assert!(result.always_used_files.is_empty());
    }

    #[test]
    fn defaults_root_to_content_when_omitted() {
        let plugin = VelitePlugin;
        let root = Path::new("/repo");
        let config_path = root.join("velite.config.ts");
        let source = r"
            import { defineConfig, defineCollection } from 'velite';
            export default defineConfig({
                collections: {
                    docs: defineCollection({ pattern: 'docs/**/*.md', schema: {} }),
                },
            });
        ";

        let patterns = patterns_of(&plugin.resolve_config(&config_path, source, root));
        assert!(patterns.contains(&"content/docs/**/*.md".to_string()));
    }

    #[test]
    fn honors_explicit_root_and_array_patterns() {
        let plugin = VelitePlugin;
        let root = Path::new("/repo");
        let config_path = root.join("velite.config.ts");
        let source = r"
            import { defineConfig, defineCollection } from 'velite';
            export default defineConfig({
                root: './src/content',
                collections: {
                    mixed: defineCollection({ pattern: ['posts/*.md', 'pages/*.mdx'] }),
                },
            });
        ";

        let patterns = patterns_of(&plugin.resolve_config(&config_path, source, root));
        assert!(patterns.contains(&"src/content/posts/*.md".to_string()));
        assert!(patterns.contains(&"src/content/pages/*.mdx".to_string()));
    }

    #[test]
    fn custom_output_data_is_credited_as_always_used() {
        let plugin = VelitePlugin;
        let root = Path::new("/repo");
        let config_path = root.join("velite.config.ts");
        let source = r"
            import { defineConfig, defineCollection } from 'velite';
            export default defineConfig({
                output: { data: 'generated/velite' },
                collections: {
                    docs: defineCollection({ pattern: 'docs/**/*.md' }),
                },
            });
        ";

        let result = plugin.resolve_config(&config_path, source, root);
        assert!(
            result
                .always_used_files
                .contains(&"generated/velite/**".to_string())
        );
    }

    #[test]
    fn negation_only_pattern_falls_back_to_root_glob() {
        let plugin = VelitePlugin;
        let root = Path::new("/repo");
        let config_path = root.join("velite.config.ts");
        let source = r"
            import { defineConfig, defineCollection } from 'velite';
            export default defineConfig({
                collections: {
                    docs: defineCollection({ pattern: ['!private/**'] }),
                },
            });
        ";

        let patterns = patterns_of(&plugin.resolve_config(&config_path, source, root));
        assert!(patterns.contains(&format!("content/**/*.{CONTENT_EXTENSIONS}")));
        assert!(!patterns.iter().any(|p| p.contains('!')));
    }

    #[test]
    fn default_output_data_adds_no_redundant_always_used_entry() {
        let plugin = VelitePlugin;
        let root = Path::new("/repo");
        let config_path = root.join("apps/web/velite.config.ts");
        // Explicitly spells out the default; static `.velite/**` already covers it.
        let source = r"
            import { defineConfig, defineCollection } from 'velite';
            export default defineConfig({
                output: { data: '.velite' },
                collections: { docs: defineCollection({ pattern: 'docs/**/*.md' }) },
            });
        ";

        let result = plugin.resolve_config(&config_path, source, root);
        assert!(
            result.always_used_files.is_empty(),
            "default output.data must not add a redundant entry: {:?}",
            result.always_used_files
        );
    }

    #[test]
    fn custom_output_data_in_workspace_is_scoped_to_package() {
        let plugin = VelitePlugin;
        let root = Path::new("/repo");
        let config_path = root.join("apps/web/velite.config.ts");
        let source = r"
            import { defineConfig, defineCollection } from 'velite';
            export default defineConfig({
                output: { data: 'generated/velite' },
                collections: { docs: defineCollection({ pattern: 'docs/**/*.md' }) },
            });
        ";

        let result = plugin.resolve_config(&config_path, source, root);
        assert!(
            result
                .always_used_files
                .contains(&"apps/web/generated/velite/**".to_string()),
            "custom output.data must be credited config-relative: {:?}",
            result.always_used_files
        );
    }

    #[test]
    fn nested_workspace_config_scopes_patterns_to_package() {
        let plugin = VelitePlugin;
        let root = Path::new("/repo");
        let config_path = root.join("apps/web/velite.config.ts");
        let source = r"
            import { defineConfig, defineCollection } from 'velite';
            export default defineConfig({
                root: 'content',
                collections: { posts: defineCollection({ pattern: 'posts/**/*.md' }) },
            });
        ";

        let patterns = patterns_of(&plugin.resolve_config(&config_path, source, root));
        assert!(patterns.contains(&"apps/web/content/posts/**/*.md".to_string()));
        assert!(
            !patterns.iter().any(|p| p.starts_with("content/")),
            "patterns must be scoped to the config's package: {patterns:?}"
        );
    }

    #[test]
    fn malformed_config_falls_back_to_default_root_glob() {
        let plugin = VelitePlugin;
        let root = Path::new("/repo");
        let config_path = root.join("velite.config.ts");
        // No collections / patterns recoverable.
        let source = "export default someFactory();\n";

        let patterns = patterns_of(&plugin.resolve_config(&config_path, source, root));
        assert!(patterns.contains(&format!("content/**/*.{CONTENT_EXTENSIONS}")));
    }
}
