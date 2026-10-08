//! GraphQL Codegen plugin.
//!
//! Detects GraphQL Codegen projects and marks config files as always used.
//! Parses codegen config to extract referenced dependencies and the
//! `documents` globs. The codegen run reads the files that those globs match,
//! so no import reaches them.

use super::{Plugin, PluginResult, config_parser};

const ENABLERS: &[&str] = &["@graphql-codegen/cli"];

const CONFIG_PATTERNS: &[&str] = &["codegen*.{ts,js,cjs,mjs}", "graphql.config.{ts,js,cjs,mjs}"];

const ALWAYS_USED: &[&str] = &[
    "codegen*.{ts,js,cjs,mjs,yml,yaml}",
    "graphql.config.{ts,js,cjs,mjs,yml,yaml}",
];

const TOOLING_DEPENDENCIES: &[&str] = &[
    "@graphql-codegen/cli",
    "@graphql-codegen/typescript",
    "@graphql-codegen/typescript-operations",
    "@graphql-codegen/typescript-react-query",
];

define_plugin! {
    struct GraphqlCodegenPlugin => "graphql-codegen",
    enablers: ENABLERS,
    config_patterns: CONFIG_PATTERNS,
    always_used: ALWAYS_USED,
    tooling_dependencies: TOOLING_DEPENDENCIES,
    resolve_config(config_path, source, _root) {
        let mut result = PluginResult::default();
        crate::plugins::add_import_referenced_dependencies(&mut result, source, config_path);
        let mut documents =
            config_parser::extract_config_string_array(source, config_path, &["documents"]);
        documents.extend(config_parser::extract_config_string(
            source,
            config_path,
            &["documents"],
        ));
        documents.retain(|pattern| !pattern.starts_with('!'));
        result.extend_entry_patterns(documents);
        result
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn matches(patterns: &[&str], name: &str) -> bool {
        patterns.iter().any(|pattern| {
            globset::Glob::new(pattern).is_ok_and(|glob| glob.compile_matcher().is_match(name))
        })
    }

    #[test]
    fn named_and_commonjs_codegen_configs_are_config_files() {
        assert!(matches(CONFIG_PATTERNS, "codegen.cjs"));
        assert!(matches(CONFIG_PATTERNS, "codegen-metadata.cjs"));
        assert!(matches(CONFIG_PATTERNS, "codegen.ts"));
        assert!(matches(ALWAYS_USED, "codegen-admin.cjs"));
        assert!(!matches(CONFIG_PATTERNS, "codegen-utils.md"));
    }

    #[test]
    fn documents_globs_are_entry_patterns() {
        let source = r"
            module.exports = {
              schema: 'http://localhost:3000/graphql',
              documents: [
                './src/modules/**/graphql/**/*.ts',
                '!./src/generated/**',
              ],
              generates: {},
            };
        ";
        let result = GraphqlCodegenPlugin.resolve_config(
            std::path::Path::new("codegen.cjs"),
            source,
            std::path::Path::new("/project"),
        );
        let patterns: Vec<&str> = result
            .entry_patterns
            .iter()
            .map(|rule| rule.pattern.as_str())
            .collect();
        assert_eq!(patterns, vec!["src/modules/**/graphql/**/*.ts"]);
        assert!(!result.replace_entry_patterns);
    }

    #[test]
    fn single_documents_string_is_an_entry_pattern() {
        let source = "module.exports = { documents: 'src/**/*.graphql' };";
        let result = GraphqlCodegenPlugin.resolve_config(
            std::path::Path::new("codegen.cjs"),
            source,
            std::path::Path::new("/project"),
        );
        assert_eq!(result.entry_patterns.len(), 1);
    }
}
