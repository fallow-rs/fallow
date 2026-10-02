//! Test spy calls that read one member of a namespace import.
//!
//! `vi.spyOn(ns, 'helper')` replaces `ns.helper` and reads nothing else of the
//! namespace object. Without this recognizer the namespace is a bare call
//! argument, which credits every export of the module.

#[allow(clippy::wildcard_imports, reason = "many spy call AST types used")]
use oxc_ast::ast::*;

use super::super::ModuleInfoExtractor;

/// The role of a binding that names a test-framework spy API.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::visitor) enum SpyApi {
    /// `vi` or `jest`: the spy call is `<local>.spyOn(object, key)`.
    SpyObject,
    /// `spyOn`: the spy call is `<local>(object, key)`.
    SpyFunction,
    /// The `mock` of `node:test`: the spy call is `<local>.method(object, key)`.
    NodeMock,
}

/// Names that a test runner can inject as globals, with their spy role.
const GLOBAL_SPY_APIS: [(&str, SpyApi); 3] = [
    ("vi", SpyApi::SpyObject),
    ("jest", SpyApi::SpyObject),
    ("spyOn", SpyApi::SpyFunction),
];

/// Modules that export a spy object (`vi`, `jest`) or a `spyOn` function.
const SPY_SOURCES: [&str; 6] = [
    "vitest",
    "@vitest/spy",
    "bun:test",
    "@jest/globals",
    "jest-mock",
    "tinyspy",
];

/// Modules that export the `mock` tracker with a `method` spy.
const NODE_MOCK_SOURCES: [&str; 2] = ["node:test", "test"];

fn imported_spy_api(source: &str, imported: &str) -> Option<SpyApi> {
    if SPY_SOURCES.contains(&source) {
        return match imported {
            "vi" | "jest" => Some(SpyApi::SpyObject),
            "spyOn" => Some(SpyApi::SpyFunction),
            _ => None,
        };
    }
    (NODE_MOCK_SOURCES.contains(&source) && imported == "mock").then_some(SpyApi::NodeMock)
}

/// The member name of a spy call: a string literal or a template literal
/// without substitutions.
fn static_spy_member(argument: &Argument<'_>) -> Option<String> {
    match argument {
        Argument::StringLiteral(value) => Some(value.value.to_string()),
        Argument::TemplateLiteral(value) if value.expressions.is_empty() => value
            .quasis
            .first()
            .and_then(|quasi| quasi.value.cooked.as_ref())
            .map(ToString::to_string),
        _ => None,
    }
}

impl ModuleInfoExtractor {
    /// Register the spy API names of this program before the body walk. A
    /// global name keeps its spy role only when no top-level binding of the
    /// file declares the same name.
    pub(super) fn record_program_spy_api_locals(&mut self, program: &Program<'_>) {
        self.spy_api_locals = GLOBAL_SPY_APIS
            .iter()
            .map(|(name, api)| ((*name).to_string(), *api))
            .collect();
        for statement in &program.body {
            match statement {
                Statement::ImportDeclaration(decl) => {
                    for specifier in decl.specifiers.iter().flatten() {
                        let local = specifier.local().name.as_str();
                        let api = match specifier {
                            ImportDeclarationSpecifier::ImportSpecifier(named)
                                if !decl.import_kind.is_type() && !named.import_kind.is_type() =>
                            {
                                imported_spy_api(
                                    decl.source.value.as_str(),
                                    named.imported.name().as_str(),
                                )
                            }
                            _ => None,
                        };
                        self.set_spy_api_local(local, api);
                    }
                }
                Statement::ExportDeclaration(decl) => {
                    self.clear_spy_api_declaration(&decl.declaration);
                }
                Statement::ExportDefaultDeclaration(decl) => match &decl.declaration {
                    ExportDefaultDeclarationKind::FunctionDeclaration(function) => {
                        if let Some(id) = &function.id {
                            self.set_spy_api_local(id.name.as_str(), None);
                        }
                    }
                    ExportDefaultDeclarationKind::ClassDeclaration(class) => {
                        if let Some(id) = &class.id {
                            self.set_spy_api_local(id.name.as_str(), None);
                        }
                    }
                    _ => {}
                },
                _ => {
                    if let Some(declaration) = statement.as_declaration() {
                        self.clear_spy_api_declaration(declaration);
                    }
                }
            }
        }
    }

    fn clear_spy_api_declaration(&mut self, declaration: &Declaration<'_>) {
        let names: Vec<String> = match declaration {
            Declaration::VariableDeclaration(var) => var
                .declarations
                .iter()
                .flat_map(|declarator| declarator.id.get_binding_identifiers())
                .map(|id| id.name.to_string())
                .collect(),
            Declaration::FunctionDeclaration(function) => {
                function.id.iter().map(|id| id.name.to_string()).collect()
            }
            Declaration::ClassDeclaration(class) => {
                class.id.iter().map(|id| id.name.to_string()).collect()
            }
            Declaration::TSEnumDeclaration(enumd) => vec![enumd.id.name.to_string()],
            Declaration::TSImportEqualsDeclaration(decl) => vec![decl.id.name.to_string()],
            _ => Vec::new(),
        };
        for name in names {
            self.set_spy_api_local(&name, None);
        }
    }

    fn set_spy_api_local(&mut self, local: &str, api: Option<SpyApi>) {
        if let Some(api) = api {
            self.spy_api_locals.insert(local.to_string(), api);
        } else {
            self.spy_api_locals.remove(local);
        }
    }

    /// Whether `name` names the spy API `api` at the current position.
    fn names_spy_api(&self, name: &str, api: SpyApi) -> bool {
        self.spy_api_locals.get(name) == Some(&api) && !self.nested_scope_shadows(name)
    }

    /// Whether the callee is a spy API whose first argument is the spied
    /// object and whose second argument is the spied member name.
    fn is_spy_callee(&self, callee: &Expression<'_>) -> bool {
        match callee {
            Expression::Identifier(function) => {
                self.names_spy_api(function.name.as_str(), SpyApi::SpyFunction)
            }
            Expression::StaticMemberExpression(member) => match &member.object {
                Expression::Identifier(object) if member.property.name == "spyOn" => {
                    self.names_spy_api(object.name.as_str(), SpyApi::SpyObject)
                }
                Expression::Identifier(object) if member.property.name == "method" => {
                    self.names_spy_api(object.name.as_str(), SpyApi::NodeMock)
                }
                // `t.mock.method(object, key)` on a `node:test` test context.
                Expression::StaticMemberExpression(inner) => {
                    member.property.name == "method"
                        && inner.property.name == "mock"
                        && matches!(inner.object, Expression::Identifier(_))
                }
                _ => false,
            },
            _ => false,
        }
    }

    /// Record `spy(ns, 'member')` as the member read `ns.member`, and keep the
    /// namespace argument out of the whole-object rule. A spy call with a
    /// computed member name keeps the whole-object use.
    pub(super) fn record_namespace_spy_call(&mut self, expr: &CallExpression<'_>) {
        let [first, second, ..] = expr.arguments.as_slice() else {
            return;
        };
        let Argument::Identifier(namespace) = first else {
            return;
        };
        let name = namespace.name.as_str();
        if !self.carries_namespace_object(name) || self.namespace_like_binding_is_shadowed(name) {
            return;
        }
        let Some(member) = static_spy_member(second) else {
            return;
        };
        if !self.is_spy_callee(&expr.callee) {
            return;
        }
        self.record_named_member_access(name, &member);
        self.mark_structured_namespace_reference(namespace);
    }
}
