//! RedwoodSDK plugin.
//!
//! RedwoodSDK apps built with `rwsdk/vite` use `src/worker.*` as the
//! Cloudflare worker entrypoint by convention.

use std::path::Path;

use oxc_allocator::Allocator;
use oxc_ast::ast::{CallExpression, Expression, ImportDeclaration};
use oxc_ast_visit::{Visit, walk};
use oxc_parser::Parser;
use oxc_span::SourceType;

use super::{Plugin, PluginResult, config_parser};

const ENABLERS: &[&str] = &["rwsdk"];

const ENTRY_PATTERNS: &[&str] = &["src/worker.{ts,tsx,js,jsx,mts,mjs}"];

const CONFIG_PATTERNS: &[&str] = &["vite.config.{ts,js,mts,mjs}"];

const ALWAYS_USED: &[&str] = &["vite.config.{ts,js,mts,mjs}"];

const VITE_CONFIG_EXPORTS: &[&str] = &["default"];

define_plugin! {
    struct RedwoodSdkPlugin => "redwoodsdk",
    enablers: ENABLERS,
    entry_patterns: ENTRY_PATTERNS,
    config_patterns: CONFIG_PATTERNS,
    always_used: ALWAYS_USED,
    used_exports: [("vite.config.{ts,js,mts,mjs}", VITE_CONFIG_EXPORTS)],
    resolve_config(config_path, source, _root) {
        resolve_redwoodsdk_config(config_path, source)
    },
}

fn resolve_redwoodsdk_config(config_path: &Path, source: &str) -> PluginResult {
    let mut result = PluginResult::default();
    for specifier in config_parser::extract_imports(source, config_path) {
        push_unique(
            &mut result.referenced_dependencies,
            crate::resolve::extract_package_name(&specifier),
        );
    }

    if has_redwood_vite_call(config_path, source) {
        result.always_used_files.push(
            config_path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("vite.config.*")
                .to_string(),
        );
    }

    result
}

fn has_redwood_vite_call(config_path: &Path, source: &str) -> bool {
    let source_type = SourceType::from_path(config_path).unwrap_or_default();
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, source, source_type).parse();
    let mut collector = RedwoodViteCallCollector::default();
    collector.visit_program(&parsed.program);
    collector.found_call
}

#[derive(Default)]
struct RedwoodViteCallCollector {
    local_names: Vec<String>,
    namespaces: Vec<String>,
    found_call: bool,
}

impl<'a> Visit<'a> for RedwoodViteCallCollector {
    fn visit_import_declaration(&mut self, decl: &ImportDeclaration<'a>) {
        if decl.source.value != "rwsdk/vite" {
            return;
        }

        if let Some(specifiers) = &decl.specifiers {
            for specifier in specifiers {
                match specifier {
                    oxc_ast::ast::ImportDeclarationSpecifier::ImportSpecifier(specifier)
                        if specifier.imported.name() == "redwood" =>
                    {
                        push_unique(&mut self.local_names, specifier.local.name.to_string());
                    }
                    oxc_ast::ast::ImportDeclarationSpecifier::ImportNamespaceSpecifier(
                        specifier,
                    ) => {
                        push_unique(&mut self.namespaces, specifier.local.name.to_string());
                    }
                    _ => {}
                }
            }
        }
    }

    fn visit_call_expression(&mut self, call: &CallExpression<'a>) {
        if self.is_redwood_call(call) {
            self.found_call = true;
            return;
        }

        walk::walk_call_expression(self, call);
    }
}

impl RedwoodViteCallCollector {
    fn is_redwood_call(&self, call: &CallExpression<'_>) -> bool {
        match &call.callee {
            Expression::Identifier(identifier) => self
                .local_names
                .iter()
                .any(|name| name == identifier.name.as_str()),
            Expression::StaticMemberExpression(member) if matches!(&member.object, Expression::Identifier(object) if self.namespaces.iter().any(|name| name == object.name.as_str())) => {
                member.property.name == "redwood"
            }
            _ => false,
        }
    }
}

fn push_unique(values: &mut Vec<String>, value: String) {
    if !values.contains(&value) {
        values.push(value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn static_patterns_cover_worker_js_like_extensions() {
        let plugin = RedwoodSdkPlugin;

        assert!(
            plugin
                .entry_patterns()
                .contains(&"src/worker.{ts,tsx,js,jsx,mts,mjs}")
        );
    }

    #[test]
    fn credits_vite_config_import_dependencies() {
        let result = resolve_redwoodsdk_config(
            Path::new("/repo/apps/website/vite.config.mts"),
            r#"
                import { cloudflare } from "@cloudflare/vite-plugin";
                import { defineConfig } from "vite";
                import { redwood } from "rwsdk/vite";

                export default defineConfig({
                    plugins: [cloudflare(), redwood()],
                });
            "#,
        );

        assert!(
            result
                .referenced_dependencies
                .contains(&"@cloudflare/vite-plugin".to_string())
        );
        assert!(result.referenced_dependencies.contains(&"vite".to_string()));
        assert!(
            result
                .referenced_dependencies
                .contains(&"rwsdk".to_string())
        );
    }

    #[test]
    fn detects_named_redwood_vite_call() {
        assert!(has_redwood_vite_call(
            Path::new("/repo/vite.config.ts"),
            r#"
                import { redwood } from "rwsdk/vite";
                export default { plugins: [redwood()] };
            "#
        ));
    }

    #[test]
    fn detects_aliased_redwood_vite_call() {
        assert!(has_redwood_vite_call(
            Path::new("/repo/vite.config.ts"),
            r#"
                import { redwood as rw } from "rwsdk/vite";
                export default { plugins: [rw()] };
            "#
        ));
    }

    #[test]
    fn detects_namespace_redwood_vite_call() {
        assert!(has_redwood_vite_call(
            Path::new("/repo/vite.config.ts"),
            r#"
                import * as rwsdkVite from "rwsdk/vite";
                export default { plugins: [rwsdkVite.redwood()] };
            "#
        ));
    }

    #[test]
    fn ignores_unrelated_local_redwood_call() {
        assert!(!has_redwood_vite_call(
            Path::new("/repo/vite.config.ts"),
            r"
                function redwood() {}
                export default { plugins: [redwood()] };
            "
        ));
    }

    #[test]
    fn ignores_unrelated_redwood_import_source() {
        assert!(!has_redwood_vite_call(
            Path::new("/repo/vite.config.ts"),
            r#"
                import { redwood } from "not-rwsdk";
                export default { plugins: [redwood()] };
            "#
        ));
    }
}
