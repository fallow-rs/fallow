//! Calls that a build tool evaluates at compile time.
//!
//! The tool reads the arguments of such a call with a static parse of the
//! instrumented copy, so each argument must stay as written. A mutation
//! switch in an argument makes the tool reject the file, and the baseline
//! run fails:
//!
//! - `feature("FLAG")` from `bun:bundle`: Bun requires a string literal
//!   (issue #5285).
//! - A call of a Bun macro, an import `with { type: "macro" }`: Bun runs the
//!   macro at compile time and needs static arguments.
//! - `import.meta.glob(...)`: Vite requires literal patterns and options.
//!
//! The walk skips the arguments of these calls. The mutants of the code
//! around the call stay, for example the condition mutants of
//! `if (feature("FLAG"))`.

use oxc_ast::ast::{
    CallExpression, Expression, ImportAttributeKey, ImportDeclaration, ImportDeclarationSpecifier,
    ModuleExportName, Program, Statement,
};
use std::collections::BTreeSet;

use crate::logging_only::callee_without_wrappers;

const BUN_BUNDLE: &str = "bun:bundle";
const BUN_FEATURE: &str = "feature";
const TYPE_KEY: &str = "type";
const MACRO_TYPE: &str = "macro";
const VITE_GLOB: &str = "glob";

/// The local names whose calls run at compile time.
#[derive(Default)]
pub(super) struct CompileTimeCalls {
    /// Local function names: `feature` from `bun:bundle` and the named or
    /// default imports of a macro module.
    functions: BTreeSet<String>,
    /// Local namespace names of a macro module.
    namespaces: BTreeSet<String>,
}

impl CompileTimeCalls {
    pub(super) fn scan(program: &Program<'_>) -> Self {
        let mut calls = Self::default();
        for statement in &program.body {
            if let Statement::ImportDeclaration(import) = statement {
                calls.add_import(import);
            }
        }
        calls
    }

    fn add_import(&mut self, import: &ImportDeclaration<'_>) {
        let Some(specifiers) = &import.specifiers else {
            return;
        };
        let is_macro = is_macro_import(import);
        let is_bun_bundle = import.source.value == BUN_BUNDLE;
        for specifier in specifiers {
            match specifier {
                ImportDeclarationSpecifier::ImportSpecifier(named) => {
                    let bun_feature = is_bun_bundle
                        && matches!(
                            &named.imported,
                            ModuleExportName::IdentifierName(name) if name.name == BUN_FEATURE
                        );
                    if is_macro || bun_feature {
                        self.functions.insert(named.local.name.to_string());
                    }
                }
                ImportDeclarationSpecifier::ImportDefaultSpecifier(default) if is_macro => {
                    self.functions.insert(default.local.name.to_string());
                }
                ImportDeclarationSpecifier::ImportNamespaceSpecifier(namespace) if is_macro => {
                    self.namespaces.insert(namespace.local.name.to_string());
                }
                _ => {}
            }
        }
    }

    /// True when the build tool evaluates `call` at compile time. The check
    /// sees through parentheses and TypeScript wrappers on the callee.
    pub(super) fn contains(&self, call: &CallExpression<'_>) -> bool {
        let callee = callee_without_wrappers(&call.callee);
        if let Expression::Identifier(identifier) = callee {
            return self.functions.contains(identifier.name.as_str());
        }
        let Expression::StaticMemberExpression(member) = callee else {
            return false;
        };
        match callee_without_wrappers(&member.object) {
            Expression::ImportMeta(_) => member.property.name == VITE_GLOB,
            Expression::Identifier(object) => self.namespaces.contains(object.name.as_str()),
            _ => false,
        }
    }
}

fn is_macro_import(import: &ImportDeclaration<'_>) -> bool {
    import.with_clause.as_ref().is_some_and(|clause| {
        clause.with_entries.iter().any(|entry| {
            let is_type = match &entry.key {
                ImportAttributeKey::Identifier(name) => name.name == TYPE_KEY,
                ImportAttributeKey::StringLiteral(literal) => literal.value == TYPE_KEY,
            };
            is_type && entry.value.value == MACRO_TYPE
        })
    })
}
