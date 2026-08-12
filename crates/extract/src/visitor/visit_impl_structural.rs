use oxc_ast::ast::{
    Argument, BindingPattern, CallExpression, Expression, FormalParameters, FunctionBody,
    VariableDeclarator,
};
use oxc_ast_visit::Visit;
use rustc_hash::FxHashMap;

use super::visit_factory_returns::FactoryReturnFunctionInput;
use super::visit_helpers::StructuralParamMemberCollector;
use crate::visitor::helpers::{extract_type_annotation_name, is_builtin_constructor};
use crate::visitor::{
    LocalStructuralFunction, ModuleInfoExtractor, StructuralCallArgument,
    StructuralClassCallCandidate, StructuralParameterUse,
};

impl ModuleInfoExtractor {
    fn collect_structural_parameter_uses(
        params: &FormalParameters<'_>,
        body: &FunctionBody<'_>,
    ) -> FxHashMap<usize, StructuralParameterUse> {
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
            return FxHashMap::default();
        }

        let target_params = typed_params
            .iter()
            .map(|(_, param_name, _)| param_name.clone())
            .collect();
        let mut collector = StructuralParamMemberCollector::new(target_params);
        collector.visit_function_body(body);

        typed_params
            .into_iter()
            .filter_map(|(index, param_name, type_name)| {
                let members = collector.members.remove(param_name.as_str())?;
                (!members.is_empty())
                    .then_some((index, StructuralParameterUse { type_name, members }))
            })
            .collect()
    }

    fn record_scoped_parameter_member_accesses(
        &mut self,
        params: &FxHashMap<usize, StructuralParameterUse>,
    ) {
        self.member_accesses
            .extend(params.values().flat_map(|param| {
                param
                    .members
                    .iter()
                    .map(|member| fallow_types::extract::MemberAccess {
                        object: param.type_name.clone(),
                        member: member.clone(),
                    })
            }));
    }

    pub(super) fn record_scoped_typed_parameter_accesses(
        &mut self,
        params: &FormalParameters<'_>,
        body: Option<&FunctionBody<'_>>,
    ) {
        let Some(body) = body else {
            return;
        };
        if self.scoped_typed_parameter_body_spans.contains(&body.span) {
            return;
        }
        let uses = Self::collect_structural_parameter_uses(params, body);
        self.record_scoped_parameter_member_accesses(&uses);
    }

    pub(super) fn record_local_structural_function(
        &mut self,
        name: &str,
        params: &FormalParameters<'_>,
        body: Option<&FunctionBody<'_>>,
    ) {
        let Some(body) = body else {
            return;
        };
        let function = LocalStructuralFunction {
            params: Self::collect_structural_parameter_uses(params, body),
        };
        self.scoped_typed_parameter_body_spans.insert(body.span);
        if function.params.is_empty() {
            return;
        }

        self.record_scoped_parameter_member_accesses(&function.params);
        self.local_structural_functions
            .insert(name.to_string(), function);
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
                        return_type: arrow.return_type.as_deref(),
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
                        return_type: function.return_type.as_deref(),
                    },
                );
            }
            _ => {}
        }
    }
}
