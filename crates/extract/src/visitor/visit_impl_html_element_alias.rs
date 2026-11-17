//! Module-scope aliases of the native `HTMLElement` constructor. A `const`
//! that holds `HTMLElement` when it is available and a fallback class when it
//! is not is an alias of `HTMLElement`, so a class that extends the alias
//! extends `HTMLElement`.

use oxc_ast::ast::{
    BinaryOperator, BindingPattern, Expression, TSType, TSTypeQueryExprName, UnaryOperator,
    VariableDeclaration, VariableDeclarationKind, VariableDeclarator,
};

use super::super::ModuleInfoExtractor;
use super::visit_value_alias::unwrap_static_expr;

impl ModuleInfoExtractor {
    pub(in crate::visitor) fn record_native_html_element_alias_candidate(
        &mut self,
        decl: &VariableDeclaration<'_>,
        declarator: &VariableDeclarator<'_>,
        init: &Expression<'_>,
    ) {
        if !self.is_module_scope() || decl.kind != VariableDeclarationKind::Const {
            return;
        }
        let BindingPattern::BindingIdentifier(id) = &declarator.id else {
            return;
        };
        let Expression::TSAsExpression(assertion) = init else {
            return;
        };
        let TSType::TSTypeQuery(query) = &assertion.type_annotation else {
            return;
        };
        let TSTypeQueryExprName::IdentifierReference(type_name) = &query.expr_name else {
            return;
        };
        if type_name.name != "HTMLElement" {
            return;
        }
        let Expression::ConditionalExpression(conditional) =
            unwrap_static_expr(&assertion.expression)
        else {
            return;
        };
        if html_element_availability_test(&conditional.test)
            && is_html_element_identifier(&conditional.consequent)
            && matches!(
                unwrap_static_expr(&conditional.alternate),
                Expression::ClassExpression(_)
            )
        {
            self.native_html_element_alias_candidates
                .insert(id.name.to_string());
        }
    }

    pub(in crate::visitor) fn resolve_native_html_element_aliases(&mut self) {
        if self.native_html_element_alias_candidates.is_empty()
            || self.local_declaration_names.contains("HTMLElement")
            || self
                .imports
                .iter()
                .any(|import| import.local_name == "HTMLElement")
        {
            return;
        }
        let aliases = &self.native_html_element_alias_candidates;
        for export in &mut self.exports {
            if export
                .super_class
                .as_deref()
                .is_some_and(|name| aliases.contains(name))
            {
                export.super_class = Some("HTMLElement".to_string());
            }
        }
        for heritage in &mut self.class_heritage {
            if heritage
                .super_class
                .as_deref()
                .is_some_and(|name| aliases.contains(name))
            {
                heritage.super_class = Some("HTMLElement".to_string());
            }
        }
        for class in self.local_class_exports.values_mut() {
            if class
                .super_class
                .as_deref()
                .is_some_and(|name| aliases.contains(name))
            {
                class.super_class = Some("HTMLElement".to_string());
            }
        }
    }
}

fn html_element_availability_test(expr: &Expression<'_>) -> bool {
    let Expression::BinaryExpression(binary) = unwrap_static_expr(expr) else {
        return false;
    };
    if !matches!(
        binary.operator,
        BinaryOperator::Inequality | BinaryOperator::StrictInequality
    ) {
        return false;
    }
    let Expression::UnaryExpression(typeof_expr) = unwrap_static_expr(&binary.left) else {
        return false;
    };
    typeof_expr.operator == UnaryOperator::Typeof
        && is_html_element_identifier(&typeof_expr.argument)
        && matches!(
            unwrap_static_expr(&binary.right),
            Expression::StringLiteral(literal) if literal.value == "undefined"
        )
}

fn is_html_element_identifier(expr: &Expression<'_>) -> bool {
    matches!(
        unwrap_static_expr(expr),
        Expression::Identifier(identifier) if identifier.name == "HTMLElement"
    )
}
