//! Detect a build config that leaves every package external.
//!
//! esbuild's `packages: 'external'` option keeps every bare import out of the
//! bundle, workspace siblings included. A workspace whose build sets it does
//! not inline a private sibling's source, so the sibling's packages are not
//! resolved from that workspace's manifest.

use std::path::Path;

use oxc_allocator::Allocator;
use oxc_ast::ast::{Expression, ObjectProperty, PropertyKey};
use oxc_ast_visit::{Visit, walk};
use oxc_parser::Parser;
use oxc_span::SourceType;

/// Return `true` when `source` contains an object property
/// `packages: 'external'` (a string or a template literal without
/// expressions).
pub(super) fn source_sets_packages_external(source: &str, path: &Path) -> bool {
    if !source.contains("packages") || !source.contains("external") {
        return false;
    }
    let source_type = SourceType::from_path(path).unwrap_or_default();
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, source, source_type).parse();
    let mut finder = PackagesExternalFinder { found: false };
    finder.visit_program(&parsed.program);
    finder.found
}

struct PackagesExternalFinder {
    found: bool,
}

impl<'a> Visit<'a> for PackagesExternalFinder {
    fn visit_object_property(&mut self, property: &ObjectProperty<'a>) {
        if is_packages_key(&property.key) && is_external_value(&property.value) {
            self.found = true;
            return;
        }
        walk::walk_object_property(self, property);
    }
}

fn is_packages_key(key: &PropertyKey<'_>) -> bool {
    key.static_name().is_some_and(|name| name == "packages")
}

fn is_external_value(value: &Expression<'_>) -> bool {
    match value {
        Expression::StringLiteral(literal) => literal.value == "external",
        Expression::TemplateLiteral(template) => template
            .single_quasi()
            .is_some_and(|quasi| quasi == "external"),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sets(source: &str) -> bool {
        source_sets_packages_external(source, Path::new("build.mjs"))
    }

    #[test]
    fn finds_packages_external_in_esbuild_build_call() {
        assert!(sets(
            r#"import * as esbuild from "esbuild";
await esbuild.build({ entryPoints: ["src/index.ts"], bundle: true, packages: "external" });"#
        ));
        assert!(sets(
            "const esbuild = require('esbuild');\nconst options = { bundle: true, 'packages': `external` };\nesbuild.build(options);"
        ));
    }

    #[test]
    fn ignores_other_packages_values_and_keys() {
        assert!(!sets(
            r#"import * as esbuild from "esbuild";
await esbuild.build({ bundle: true, packages: "bundle", external: ["react"] });"#
        ));
        assert!(!sets(
            r#"export const meta = { name: "packages", kind: "external" };"#
        ));
    }
}
