use oxc_ast::ast::{
    Argument, BindingPattern, CallExpression, Expression, FormalParameters, FunctionBody,
    VariableDeclarator,
};
use oxc_ast_visit::Visit;

use super::visit_factory_returns::FactoryReturnFunctionInput;
use super::visit_helpers::StructuralParamMemberCollector;
use crate::visitor::helpers::{extract_type_annotation_name, is_builtin_constructor};
use crate::visitor::{
    LocalStructuralFunction, ModuleInfoExtractor, StructuralCallArgument,
    StructuralClassCallCandidate, StructuralParameterUse,
};

impl ModuleInfoExtractor {
    pub(super) fn record_local_structural_function(
        &mut self,
        name: &str,
        params: &FormalParameters<'_>,
        body: Option<&FunctionBody<'_>>,
    ) {
        let Some(body) = body else {
            return;
        };
        let typed_params: Vec<(usize, String, String)> = params
            .items
            .iter()
            .enumerate()
            .filter_map(|(index, param)| {
                let BindingPattern::BindingIdentifier(id) = &param.pattern else {
                    return None;
                };
                let type_annotation = param.type_annotation.as_deref()?;
                let type_name = extract_type_annotation_name(type_annotation)?;
                Some((index, id.name.to_string(), type_name))
            })
            .collect();
        if typed_params.is_empty() {
            return;
        }

        let target_params = typed_params
            .iter()
            .map(|(_, param_name, _)| param_name.clone())
            .collect();
        let mut collector = StructuralParamMemberCollector::new(target_params);
        collector.visit_function_body(body);

        let mut function = LocalStructuralFunction::default();
        for (index, param_name, type_name) in typed_params {
            let Some(members) = collector.members.remove(param_name.as_str()) else {
                continue;
            };
            if members.is_empty() {
                continue;
            }
            function
                .params
                .insert(index, StructuralParameterUse { type_name, members });
        }

        if !function.params.is_empty() {
            self.local_structural_functions
                .insert(name.to_string(), function);
        }
    }

    fn structural_call_argument(arg: &Argument<'_>) -> Option<StructuralCallArgument> {
        let expr = arg.as_expression()?;
        match expr {
            Expression::NewExpression(new_expr) => {
                let Expression::Identifier(callee) = &new_expr.callee else {
                    return None;
                };
                if is_builtin_constructor(callee.name.as_str()) {
                    return None;
                }
                Some(StructuralCallArgument::DirectClass(callee.name.to_string()))
            }
            Expression::Identifier(ident) => {
                Some(StructuralCallArgument::Binding(ident.name.to_string()))
            }
            _ => None,
        }
    }

    pub(super) fn record_structural_class_call_candidate(&mut self, call: &CallExpression<'_>) {
        let Expression::Identifier(callee) = &call.callee else {
            return;
        };

        let arguments: Vec<Option<StructuralCallArgument>> = call
            .arguments
            .iter()
            .map(Self::structural_call_argument)
            .collect();
        if arguments.iter().all(Option::is_none) {
            return;
        }

        self.structural_class_call_candidates
            .push(StructuralClassCallCandidate {
                callee_name: callee.name.to_string(),
                arguments,
            });
    }

    pub(super) fn record_local_structural_function_from_variable_declarator(
        &mut self,
        declarator: &VariableDeclarator<'_>,
        init: &Expression<'_>,
    ) {
        if !self.is_module_scope() {
            return;
        }
        let BindingPattern::BindingIdentifier(id) = &declarator.id else {
            return;
        };
        match init {
            Expression::ArrowFunctionExpression(arrow) => {
                self.record_local_structural_function(
                    id.name.as_str(),
                    &arrow.params,
                    Some(arrow.body.as_ref()),
                );
                self.record_factory_return_function(
                    id.name.as_str(),
                    FactoryReturnFunctionInput {
                        params: &arrow.params,
                        body: Some(arrow.body.as_ref()),
                        is_expression_body: arrow.expression,
                        is_async: arrow.r#async,
                        is_generator: false,
                    },
                );
            }
            Expression::FunctionExpression(function) => {
                self.record_local_structural_function(
                    id.name.as_str(),
                    &function.params,
                    function.body.as_deref(),
                );
                self.record_factory_return_function(
                    id.name.as_str(),
                    FactoryReturnFunctionInput {
                        params: &function.params,
                        body: function.body.as_deref(),
                        is_expression_body: false,
                        is_async: function.r#async,
                        is_generator: function.generator,
                    },
                );
            }
            _ => {}
        }
    }
}
