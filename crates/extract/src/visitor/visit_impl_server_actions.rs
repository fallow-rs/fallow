//! Export-shape check for file-level `"use server"` modules.
//!
//! React Server Components allow only async functions as value exports of a
//! `"use server"` file. When a `"use client"` file imports such a module, the
//! bundler replaces each export with an action reference, so no code of the
//! module (or of its imports) enters the client bundle. The security
//! `client-server-leak` BFS uses the result to stop the client cone at the
//! module. A module with any other value export keeps today's behavior, so
//! the check is conservative: an export that it cannot classify counts as a
//! non-action export.

#[allow(clippy::wildcard_imports, reason = "many module-level AST types used")]
use oxc_ast::ast::*;
use rustc_hash::FxHashMap;

use super::unwrap_parens;

const USE_SERVER: &str = "use server";

/// Kind of a top-level local binding, as far as the export-shape check needs.
#[derive(Clone, Copy, PartialEq, Eq)]
enum LocalKind {
    AsyncFunction,
    TypeOnly,
    Other,
}

/// Return `true` when the program prologue holds `"use server"` and every
/// value export is an async function. Type exports do not count.
pub(super) fn is_server_action_module(program: &Program<'_>) -> bool {
    if !program
        .directives
        .iter()
        .any(|d| d.directive.as_str() == USE_SERVER)
    {
        return false;
    }
    let locals = collect_top_level_locals(program);
    program
        .body
        .iter()
        .all(|stmt| statement_exports_only_actions(stmt, &locals))
}

fn statement_exports_only_actions(
    stmt: &Statement<'_>,
    locals: &FxHashMap<&str, LocalKind>,
) -> bool {
    match stmt {
        Statement::ExportDeclaration(decl) => declaration_is_action_only(&decl.declaration),
        Statement::ExportNamedDeclaration(decl) => named_export_is_action_only(decl, locals),
        // A re-export (`export { x } from "./y"`) cannot be classified here.
        Statement::ExportFromDeclaration(decl) => {
            decl.export_kind.is_type() || decl.specifiers.iter().all(|s| s.export_kind.is_type())
        }
        Statement::ExportDefaultDeclaration(decl) => {
            default_export_is_action(&decl.declaration, locals)
        }
        Statement::ExportAllDeclaration(decl) => decl.export_kind.is_type(),
        Statement::TSExportAssignment(_) => false,
        _ => true,
    }
}

fn named_export_is_action_only(
    decl: &ExportNamedDeclaration<'_>,
    locals: &FxHashMap<&str, LocalKind>,
) -> bool {
    if decl.export_kind.is_type() {
        return true;
    }
    decl.specifiers.iter().all(|spec| {
        if spec.export_kind.is_type() {
            return true;
        }
        let ModuleExportName::IdentifierReference(local) = &spec.local else {
            return false;
        };
        matches!(
            locals.get(local.name.as_str()),
            Some(LocalKind::AsyncFunction | LocalKind::TypeOnly)
        )
    })
}

fn declaration_is_action_only(declaration: &Declaration<'_>) -> bool {
    match declaration {
        Declaration::FunctionDeclaration(func) => {
            func.r#async || func.declare || func.body.is_none()
        }
        Declaration::VariableDeclaration(var) => {
            var.declare
                || var
                    .declarations
                    .iter()
                    .all(|d| d.init.as_ref().is_some_and(is_async_function_expression))
        }
        Declaration::TSTypeAliasDeclaration(_) | Declaration::TSInterfaceDeclaration(_) => true,
        Declaration::TSEnumDeclaration(e) => e.declare,
        Declaration::TSExternalModuleDeclaration(m) => m.declare,
        Declaration::TSNamespaceDeclaration(m) => m.declare,
        Declaration::ClassDeclaration(class) => class.declare,
        Declaration::TSGlobalDeclaration(_) | Declaration::TSImportEqualsDeclaration(_) => false,
    }
}

fn default_export_is_action(
    kind: &ExportDefaultDeclarationKind<'_>,
    locals: &FxHashMap<&str, LocalKind>,
) -> bool {
    match kind {
        ExportDefaultDeclarationKind::FunctionDeclaration(func) => func.r#async,
        ExportDefaultDeclarationKind::TSInterfaceDeclaration(_) => true,
        ExportDefaultDeclarationKind::ClassDeclaration(_) => false,
        _ => kind.as_expression().is_some_and(|expr| {
            if is_async_function_expression(expr) {
                return true;
            }
            let Expression::Identifier(id) = unwrap_parens(expr) else {
                return false;
            };
            locals.get(id.name.as_str()) == Some(&LocalKind::AsyncFunction)
        }),
    }
}

fn is_async_function_expression(expr: &Expression<'_>) -> bool {
    match unwrap_parens(expr) {
        Expression::ArrowFunctionExpression(arrow) => arrow.r#async,
        Expression::FunctionExpression(func) => func.r#async,
        _ => false,
    }
}

/// Classify top-level declarations so `export { x }` and `export default x`
/// can resolve a local name. Names that are declared more than once, or that
/// come from an import, resolve to `Other`.
fn collect_top_level_locals<'a>(program: &'a Program<'a>) -> FxHashMap<&'a str, LocalKind> {
    let mut locals: FxHashMap<&'a str, LocalKind> = FxHashMap::default();
    let mut record = |name: &'a str, kind: LocalKind| {
        locals
            .entry(name)
            .and_modify(|existing| {
                if *existing != kind {
                    *existing = LocalKind::Other;
                }
            })
            .or_insert(kind);
    };
    for stmt in &program.body {
        let declaration = match stmt {
            Statement::ExportDeclaration(decl) => Some(&decl.declaration),
            _ => stmt.as_declaration(),
        };
        if let Some(declaration) = declaration {
            record_declaration(declaration, &mut record);
            continue;
        }
        if let Statement::ImportDeclaration(import) = stmt {
            for spec in import.specifiers.iter().flatten() {
                record(spec.local().name.as_str(), LocalKind::Other);
            }
        }
    }
    locals
}

fn record_declaration<'a>(
    declaration: &'a Declaration<'a>,
    record: &mut impl FnMut(&'a str, LocalKind),
) {
    match declaration {
        Declaration::FunctionDeclaration(func) => {
            // Overload signatures have no body and do not change the kind.
            if func.body.is_none() {
                return;
            }
            if let Some(id) = &func.id {
                let kind = if func.r#async {
                    LocalKind::AsyncFunction
                } else {
                    LocalKind::Other
                };
                record(id.name.as_str(), kind);
            }
        }
        Declaration::VariableDeclaration(var) => {
            for declarator in &var.declarations {
                let BindingPattern::BindingIdentifier(id) = &declarator.id else {
                    continue;
                };
                let is_async_const = var.kind == VariableDeclarationKind::Const
                    && declarator
                        .init
                        .as_ref()
                        .is_some_and(is_async_function_expression);
                let kind = if is_async_const {
                    LocalKind::AsyncFunction
                } else {
                    LocalKind::Other
                };
                record(id.name.as_str(), kind);
            }
        }
        Declaration::TSTypeAliasDeclaration(alias) => {
            record(alias.id.name.as_str(), LocalKind::TypeOnly);
        }
        Declaration::TSInterfaceDeclaration(iface) => {
            record(iface.id.name.as_str(), LocalKind::TypeOnly);
        }
        Declaration::ClassDeclaration(class) => {
            if let Some(id) = &class.id {
                record(id.name.as_str(), LocalKind::Other);
            }
        }
        Declaration::TSEnumDeclaration(e) => record(e.id.name.as_str(), LocalKind::Other),
        _ => {}
    }
}
