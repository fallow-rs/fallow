use oxc_ast::ast::{
    Argument, BindingPattern, CallExpression, Declaration, Expression, FormalParameters,
    FunctionBody, Program, Statement, TSType, TSTypeAliasDeclaration, VariableDeclarator,
};
use rustc_hash::FxHashMap;

use super::visit_factory_returns::FactoryReturnFunctionInput;
use super::visit_helpers::StructuralParamMemberCollector;
use crate::visitor::helpers::{extract_type_annotation_name, is_builtin_constructor};
use crate::visitor::{
    LocalStructuralFunction, ModuleInfoExtractor, StructuralCallArgument,
    StructuralClassCallCandidate, StructuralParameterUse,
};

#[derive(Default)]
struct ScopedStructuralUses {
    params: FxHashMap<usize, StructuralParameterUse>,
    typed_property_accesses: Vec<(String, String, String)>,
}

impl ModuleInfoExtractor {
    fn collect_structural_parameter_uses(
        params: &FormalParameters<'_>,
        body: &FunctionBody<'_>,
        inferred_param_types: Option<&[Option<String>]>,
    ) -> ScopedStructuralUses {
        let typed_params: Vec<(usize, String, String)> = params
            .items
            .iter()
            .enumerate()
            .filter_map(|(index, param)| {
                let BindingPattern::BindingIdentifier(id) = &param.pattern else {
                    return None;
                };
                let type_name = param
                    .type_annotation
                    .as_deref()
                    .and_then(extract_type_annotation_name)
                    .or_else(|| inferred_param_types?.get(index).and_then(Clone::clone))?;
                Some((index, id.name.to_string(), type_name))
            })
            .collect();
        if typed_params.is_empty() {
            return ScopedStructuralUses::default();
        }

        let target_params = typed_params
            .iter()
            .map(|(_, param_name, _)| param_name.clone())
            .collect();
        let mut collector = StructuralParamMemberCollector::new(target_params);
        collector.collect_function_body(body);

        let mut uses = ScopedStructuralUses::default();
        for (index, param_name, type_name) in typed_params {
            if let Some(members) = collector.members.remove(param_name.as_str())
                && !members.is_empty()
            {
                uses.params.insert(
                    index,
                    StructuralParameterUse {
                        type_name: type_name.clone(),
                        members,
                    },
                );
            }
            if let Some(property_members) = collector.property_members.remove(param_name.as_str()) {
                uses.typed_property_accesses.extend(
                    property_members
                        .into_iter()
                        .map(|(property_path, member)| (type_name.clone(), property_path, member)),
                );
            }
        }
        uses
    }

    fn record_scoped_parameter_member_accesses(&mut self, uses: &ScopedStructuralUses) {
        self.member_accesses
            .extend(uses.params.values().flat_map(|param| {
                param
                    .members
                    .iter()
                    .map(|member| fallow_types::extract::MemberAccess {
                        object: param.type_name.clone(),
                        member: member.clone(),
                    })
            }));
        for (type_name, property_path, member) in &uses.typed_property_accesses {
            self.record_typed_property_member_fact(
                type_name.clone(),
                property_path.clone(),
                member.clone(),
            );
        }
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
        let uses = Self::collect_structural_parameter_uses(params, body, None);
        self.record_scoped_parameter_member_accesses(&uses);
    }

    pub(super) fn record_local_structural_function(
        &mut self,
        name: &str,
        params: &FormalParameters<'_>,
        body: Option<&FunctionBody<'_>>,
        inferred_param_types: Option<&[Option<String>]>,
    ) {
        let uses = self.record_structural_function_uses(params, body, inferred_param_types);
        if uses.params.is_empty() {
            return;
        }
        self.local_structural_functions.insert(
            name.to_string(),
            LocalStructuralFunction {
                params: uses.params,
            },
        );
    }

    fn record_structural_function_uses(
        &mut self,
        params: &FormalParameters<'_>,
        body: Option<&FunctionBody<'_>>,
        inferred_param_types: Option<&[Option<String>]>,
    ) -> ScopedStructuralUses {
        let Some(body) = body else {
            return ScopedStructuralUses::default();
        };
        let uses = Self::collect_structural_parameter_uses(params, body, inferred_param_types);
        self.scoped_typed_parameter_body_spans.insert(body.span);
        if !uses.params.is_empty() || !uses.typed_property_accesses.is_empty() {
            self.record_scoped_parameter_member_accesses(&uses);
        }
        uses
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
        let is_module_scope = self.is_module_scope();
        let BindingPattern::BindingIdentifier(id) = &declarator.id else {
            return;
        };
        let inferred_param_types = declarator
            .type_annotation
            .as_deref()
            .and_then(extract_type_annotation_name)
            .and_then(|name| self.function_type_alias_params.get(&name))
            .cloned();
        match init {
            Expression::ArrowFunctionExpression(arrow) => {
                if is_module_scope {
                    self.record_local_structural_function(
                        id.name.as_str(),
                        &arrow.params,
                        Some(arrow.body.as_ref()),
                        inferred_param_types.as_deref(),
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
                } else if inferred_param_types.is_some() {
                    self.record_structural_function_uses(
                        &arrow.params,
                        Some(arrow.body.as_ref()),
                        inferred_param_types.as_deref(),
                    );
                }
            }
            Expression::FunctionExpression(function) => {
                if is_module_scope {
                    self.record_local_structural_function(
                        id.name.as_str(),
                        &function.params,
                        function.body.as_deref(),
                        inferred_param_types.as_deref(),
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
                } else if inferred_param_types.is_some() {
                    self.record_structural_function_uses(
                        &function.params,
                        function.body.as_deref(),
                        inferred_param_types.as_deref(),
                    );
                }
            }
            _ => {}
        }
    }

    fn record_function_type_alias(&mut self, alias: &TSTypeAliasDeclaration<'_>) {
        let TSType::TSFunctionType(function) = &alias.type_annotation else {
            return;
        };
        let params = function
            .params
            .items
            .iter()
            .map(|param| {
                param
                    .type_annotation
                    .as_deref()
                    .and_then(extract_type_annotation_name)
            })
            .collect();
        self.function_type_alias_params
            .insert(alias.id.name.to_string(), params);
    }

    pub(super) fn record_program_function_type_aliases(&mut self, program: &Program<'_>) {
        for statement in &program.body {
            let alias = match statement {
                Statement::TSTypeAliasDeclaration(alias) => Some(alias.as_ref()),
                Statement::ExportNamedDeclaration(export) if export.source.is_none() => {
                    match export.declaration.as_ref() {
                        Some(Declaration::TSTypeAliasDeclaration(alias)) => Some(alias.as_ref()),
                        _ => None,
                    }
                }
                _ => None,
            };
            if let Some(alias) = alias {
                self.record_function_type_alias(alias);
            }
        }
    }
}
