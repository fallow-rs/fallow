//! `require` functions that resolve from the file itself.
//!
//! `createRequire(import.meta.url)` gives the same function as the CommonJS
//! `require`. Code can call it inline, or bind it to any name, so the name
//! `require` alone does not identify such a function. The check is syntactic:
//! a local function named `createRequire` also matches, and the cost is a
//! missed finding.
//!
//! A call with another base, such as `createRequire(process.cwd())`, resolves
//! from another directory. It is not a `require` function of this file.

use std::borrow::Cow;

#[allow(clippy::wildcard_imports, reason = "many AST node types used")]
use oxc_ast::ast::*;

use fallow_types::extract::RequireCallInfo;

use crate::js_text::JsText;

use super::super::ModuleInfoExtractor;

/// The name of the CommonJS `require` function.
pub(super) const REQUIRE: &str = "require";

const CREATE_REQUIRE: &str = "createRequire";
const FILE_URL_TO_PATH: &str = "fileURLToPath";
const NODE_MODULE_SOURCES: [&str; 2] = ["node:module", "module"];

fn is_node_module_source(source: &StringLiteral<'_>) -> bool {
    NODE_MODULE_SOURCES.contains(&source.value.text().as_ref())
}

fn is_import_meta_file(expr: &Expression<'_>) -> bool {
    let Expression::StaticMemberExpression(member) = expr else {
        return false;
    };
    matches!(member.object, Expression::ImportMeta(_))
        && matches!(member.property.name.as_str(), "url" | "filename")
}

fn is_file_url_to_path_of_import_meta(call: &CallExpression<'_>) -> bool {
    let callee_matches = match &call.callee {
        Expression::Identifier(callee) => callee.name == FILE_URL_TO_PATH,
        Expression::StaticMemberExpression(member) => member.property.name == FILE_URL_TO_PATH,
        _ => false,
    };
    callee_matches
        && matches!(
            call.arguments.as_slice(),
            [argument] if argument.as_expression().is_some_and(is_import_meta_file)
        )
}

/// Whether `argument` names the current file.
fn is_this_file_base(argument: &Argument<'_>) -> bool {
    match argument.as_expression() {
        Some(Expression::Identifier(base)) => base.name == "__filename",
        Some(Expression::CallExpression(call)) => is_file_url_to_path_of_import_meta(call),
        Some(expr) => is_import_meta_file(expr),
        None => false,
    }
}

/// Whether `expr` is `createRequire(<this file>)`, also through a member such
/// as `module.createRequire`. `is_alias` answers for another local name of
/// `createRequire`.
pub(super) fn is_this_file_create_require_call(
    expr: &Expression<'_>,
    is_alias: impl Fn(&str) -> bool,
) -> bool {
    let Expression::CallExpression(call) = expr.get_inner_expression() else {
        return false;
    };
    let callee_matches = match call.callee.get_inner_expression() {
        Expression::Identifier(callee) => {
            callee.name == CREATE_REQUIRE || is_alias(callee.name.as_str())
        }
        Expression::StaticMemberExpression(member) => member.property.name == CREATE_REQUIRE,
        _ => false,
    };
    callee_matches && matches!(call.arguments.as_slice(), [base] if is_this_file_base(base))
}

/// The other local name that `declarator` gives to `createRequire`, in
/// `const { createRequire: name } = require('node:module')`.
fn required_create_require_alias<'b>(declarator: &'b VariableDeclarator<'_>) -> Option<&'b str> {
    let BindingPattern::ObjectPattern(pattern) = &declarator.id else {
        return None;
    };
    let Some(Expression::CallExpression(call)) = &declarator.init else {
        return None;
    };
    let Expression::Identifier(callee) = &call.callee else {
        return None;
    };
    let Some(Argument::StringLiteral(source)) = call.arguments.first() else {
        return None;
    };
    if callee.name != REQUIRE || !is_node_module_source(source) {
        return None;
    }
    pattern.properties.iter().find_map(|property| {
        let key = property.key.static_name()?;
        let BindingPattern::BindingIdentifier(local) = &property.value else {
            return None;
        };
        (key.text() == CREATE_REQUIRE).then_some(local.name.as_str())
    })
}

impl ModuleInfoExtractor {
    /// Record the module-level names of `createRequire` and of the `require`
    /// functions of this file. A function body above the declaration reads
    /// them, so a walk in source order is too late.
    pub(super) fn record_program_module_require_names(&mut self, program: &Program<'_>) {
        for statement in &program.body {
            let Statement::ImportDeclaration(decl) = statement else {
                continue;
            };
            if decl.import_kind.is_type() || !is_node_module_source(&decl.source) {
                continue;
            }
            for specifier in decl.specifiers.iter().flatten() {
                if let ImportDeclarationSpecifier::ImportSpecifier(named) = specifier
                    && !named.import_kind.is_type()
                    && named.imported.name() == CREATE_REQUIRE
                {
                    self.create_require_aliases
                        .insert(named.local.name.to_string());
                }
            }
        }
        for statement in &program.body {
            let declaration = match statement {
                Statement::ExportDeclaration(export) => Some(&export.declaration),
                _ => statement.as_declaration(),
            };
            let Some(Declaration::VariableDeclaration(var)) = declaration else {
                continue;
            };
            for declarator in &var.declarations {
                if let Some(alias) = required_create_require_alias(declarator) {
                    self.create_require_aliases.insert(alias.to_string());
                }
                if let Some(name) = self.this_file_require_binding(declarator) {
                    self.module_require_aliases.insert(name.to_string());
                }
            }
        }
    }

    /// The name of the binding when `declarator` binds a `require` function of
    /// this file.
    fn this_file_require_binding<'b>(
        &self,
        declarator: &'b VariableDeclarator<'_>,
    ) -> Option<&'b str> {
        let BindingPattern::BindingIdentifier(id) = &declarator.id else {
            return None;
        };
        let init = declarator.init.as_ref()?;
        is_this_file_create_require_call(init, |name| self.is_create_require_alias(name))
            .then_some(id.name.as_str())
    }

    fn is_create_require_alias(&self, name: &str) -> bool {
        self.create_require_aliases.contains(name) && !self.nested_scope_shadows(name)
    }

    /// Whether the innermost declaration of `name` binds a `require` function
    /// of this file. Each scope records a nested binding of this kind as a
    /// declaration too, so a binding with the same name in an inner scope hides
    /// it, and a binding of this kind still hides an outer import.
    fn is_require_alias(&self, name: &str) -> bool {
        match self
            .nested_declaration_stack
            .iter()
            .rposition(|scope| scope.contains(name))
        {
            Some(index) => self
                .scoped_require_aliases
                .get(index)
                .is_some_and(|scope| scope.contains(name)),
            None => self.module_require_aliases.contains(name),
        }
    }

    /// Whether `expr` is a `require` function of this file: the inline call,
    /// or a binding of that call with any name.
    pub(super) fn is_this_file_require(&self, expr: &Expression<'_>) -> bool {
        match expr.get_inner_expression() {
            Expression::Identifier(id) => self.is_require_alias(id.name.as_str()),
            inner => {
                is_this_file_create_require_call(inner, |name| self.is_create_require_alias(name))
            }
        }
    }

    fn is_require_load_callee(&self, callee: &Expression<'_>) -> bool {
        matches!(callee, Expression::Identifier(ident) if ident.name == REQUIRE)
            || self.is_this_file_require(callee)
    }

    /// The call and the specifier when `init` is a load with a string literal.
    pub(super) fn try_extract_require<'a, 'b>(
        &self,
        init: &'b Expression<'a>,
    ) -> Option<(&'b CallExpression<'a>, Cow<'b, str>)> {
        let Expression::CallExpression(call) = init else {
            return None;
        };
        let Some(Argument::StringLiteral(lit)) = call.arguments.first() else {
            return None;
        };
        self.is_require_load_callee(&call.callee)
            .then(|| (&**call, lit.value.text()))
    }

    pub(super) fn record_bare_require_call(&mut self, expr: &CallExpression<'_>) {
        if let Some(Argument::StringLiteral(lit)) = expr.arguments.first()
            && !self.handled_require_spans.contains(&expr.span)
            && self.is_require_load_callee(&expr.callee)
        {
            self.require_calls.push(RequireCallInfo {
                source: lit.value.text().into_owned(),
                span: expr.span,
                source_span: lit.span,
                destructured_names: Vec::new(),
                local_name: None,
                is_type_only: false,
            });
        }
    }

    /// Record the declarators of `var` as declarations of the nested scope.
    ///
    /// A nested `require` from `createRequire(<this file>)` resolves from the
    /// same file as the module `require`, so it is not a shadow. Another base,
    /// such as `createRequire(join(dir, 'x.json'))`, stays a shadow. The same
    /// binding with another name stays a declaration of its scope, and the
    /// scope also records it as a `require` function.
    pub(super) fn record_nested_scope_declarators(&mut self, var: &VariableDeclaration<'_>) {
        for declarator in &var.declarations {
            let require_binding = self.this_file_require_binding(declarator);
            if require_binding == Some(REQUIRE) {
                continue;
            }
            self.record_nested_declaration_names(declarator.id.get_binding_identifiers());
            if let Some(name) = require_binding
                && self.namespace_depth == 0
                && let Some(scope) = self.scoped_require_aliases.last_mut()
            {
                scope.insert(name.to_string());
            }
        }
    }
}
