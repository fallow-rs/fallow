//! Callback arguments and deferred property ownership inside the shared value arena.

#[allow(
    clippy::wildcard_imports,
    reason = "callback acquisition matches AST forms"
)]
use oxc_ast::ast::*;
use oxc_span::{GetSpan, Span};

use super::super::{
    DirectBindingValue, FactoryPropertyOrigin, FactoryPropertyProof, ModuleInfoExtractor,
};
use super::unwrap_static_expr;
use crate::function_body::BodyRef;
use crate::js_text::JsText;

impl ModuleInfoExtractor {
    pub(super) fn prepare_callback_property_observations(
        &mut self,
        object: &ObjectExpression<'_>,
        params: &FormalParameters<'_>,
    ) {
        for property in &object.properties {
            let ObjectPropertyKind::ObjectProperty(property) = property else {
                continue;
            };
            match unwrap_static_expr(&property.value) {
                Expression::ObjectExpression(nested) => {
                    self.prepare_callback_property_observations(nested, params);
                }
                Expression::CallExpression(call) if call.arguments.is_empty() && !call.optional => {
                    let Expression::Identifier(reference) = unwrap_static_expr(&call.callee) else {
                        continue;
                    };
                    let Some((index, declaration)) =
                        params.items.iter().enumerate().find_map(|(index, param)| {
                            let BindingPattern::BindingIdentifier(id) = &param.pattern else {
                                return None;
                            };
                            (id.name == reference.name).then_some((index, id.span))
                        })
                    else {
                        continue;
                    };
                    let requirements =
                        self.combine_requirements(&[], &[(reference.span, declaration)]);
                    if let Some(observation) =
                        self.factory_property_observations.get_mut(&call.span)
                    {
                        observation.2 = Some(FactoryPropertyProof {
                            projection: String::new(),
                            vue_only: false,
                            assigned_class: None,
                            origin: FactoryPropertyOrigin::CallbackParameter {
                                parameter_index: u32::try_from(index).unwrap_or(u32::MAX),
                                declaration,
                                reference: reference.span,
                            },
                            requirements,
                        });
                    }
                }
                _ => {}
            }
        }
    }

    pub(super) fn callback_result_proof(
        &mut self,
        expression: &Expression<'_>,
    ) -> Option<super::super::CallbackResultProof> {
        match unwrap_static_expr(expression) {
            Expression::Identifier(reference) => {
                let (value, entry) = self.direct_binding_value(reference.name.as_str())?;
                let value = self.projection_value_view(value);
                if value.unknown_properties {
                    return None;
                }
                let (mut callback, getter_requirements) =
                    if let Some(callback) = &value.callback_result {
                        (callback.clone(), super::super::BindingRequirements::Empty)
                    } else {
                        let getter = self.projection_value_view(*value.properties.get("get")?);
                        if getter.unknown_properties {
                            return None;
                        }
                        let mut callback = getter.callback_result.clone()?;
                        if callback.getter {
                            return None;
                        }
                        callback.getter = true;
                        (callback, getter.requirements)
                    };
                let requirements = self.combine_requirements(
                    &[
                        callback.class.requirements,
                        entry.requirements,
                        value.requirements,
                        getter_requirements,
                    ],
                    &[(reference.span, entry.declaration_span?)],
                );
                if let Some((name, declaration, _)) = &callback.capture {
                    if self.direct_binding_entry(name)?.declaration_span != Some(*declaration) {
                        return None;
                    }
                    callback.class =
                        self.direct_binding_class_proof(name, callback.lazy_reference)?;
                }
                callback.class.requirements =
                    self.combine_requirements(&[callback.class.requirements, requirements], &[]);
                Some(callback)
            }
            Expression::ObjectExpression(object) => {
                if !self.charge_binding_conditions(object.properties.len()) {
                    return None;
                }
                let mut getter = None;
                let mut keys = rustc_hash::FxHashSet::default();
                for property in &object.properties {
                    let ObjectPropertyKind::ObjectProperty(property) = property else {
                        return None;
                    };
                    let key = property.key.static_name()?;
                    if !keys.insert(key.text().into_owned()) || property.kind != PropertyKind::Init
                    {
                        return None;
                    }
                    if key == "get" {
                        getter = Some(&property.value);
                    }
                }
                let mut proof = self.callback_result_proof(getter?)?;
                if proof.getter {
                    return None;
                }
                proof.getter = true;
                Some(proof)
            }
            Expression::ArrowFunctionExpression(function) if !function.r#async => self
                .callback_body_proof(
                    BodyRef::arrow(&function.body),
                    function.params.span,
                    function.return_type.as_deref(),
                ),
            Expression::FunctionExpression(function)
                if !function.r#async && !function.generator =>
            {
                self.callback_body_proof(
                    BodyRef::Block(function.body.as_deref()?),
                    function.params.span,
                    function.return_type.as_deref(),
                )
            }
            _ => None,
        }
    }

    pub(super) fn callback_body_proof(
        &mut self,
        body: BodyRef<'_, '_>,
        parameters: Span,
        annotation: Option<&TSTypeAnnotation<'_>>,
    ) -> Option<super::super::CallbackResultProof> {
        use super::super::{BindingClassProof, BindingRequirements, CallbackResultProof};
        if !super::visit_factory_returns::function_body_is_terminal(body) {
            return None;
        }
        if let Some(class) =
            super::visit_factory_returns::function_body_returns_new_class_unanimous(body)
        {
            let (target_name, target_reference_span) =
                super::visit_factory_returns::function_body_returns_new_class_target(body)?;
            if class != target_name {
                return None;
            }
            let declaration = self
                .direct_binding_entry(&target_name)
                .and_then(|entry| entry.declaration_span)
                .or_else(|| {
                    self.local_class_exports
                        .get(&target_name)
                        .map(|class| class.binding_span)
                })?;
            let mut returns = Vec::new();
            match body {
                BodyRef::Concise(expression) => returns.push(expression),
                BodyRef::Block(body) => {
                    super::visit_factory_returns::collect_return_args_in_statements(
                        &body.statements,
                        &mut returns,
                    );
                }
            }
            if !self.charge_binding_conditions(returns.len()) {
                return None;
            }
            let references = returns
                .into_iter()
                .map(|expression| {
                    let Expression::NewExpression(new) = unwrap_static_expr(expression) else {
                        return None;
                    };
                    let Expression::Identifier(reference) = &new.callee else {
                        return None;
                    };
                    Some((reference.span, declaration))
                })
                .collect::<Option<Vec<_>>>()?;
            let requirements = self.combine_requirements(&[], &references);
            return Some(CallbackResultProof {
                class: BindingClassProof {
                    target_name,
                    target_reference_span,
                    requirements,
                    builtin_containers: std::sync::Arc::default(),
                },
                lazy_reference: None,
                capture: None,
                getter: false,
            });
        }
        if let Some(annotation) = annotation {
            let target_name = super::extract_type_annotation_name(annotation)?;
            let target_reference_span =
                super::super::helpers::extract_type_reference_origin(&annotation.type_annotation)?;
            return Some(CallbackResultProof {
                class: BindingClassProof {
                    target_name,
                    target_reference_span,
                    requirements: BindingRequirements::Empty,
                    builtin_containers: std::sync::Arc::default(),
                },
                lazy_reference: None,
                capture: None,
                getter: false,
            });
        }
        self.callback_capture_proof(body, parameters)
    }

    fn callback_capture_proof(
        &mut self,
        body: BodyRef<'_, '_>,
        parameters: Span,
    ) -> Option<super::super::CallbackResultProof> {
        use super::super::CallbackResultProof;
        let expression = match body {
            BodyRef::Concise(expression) => expression,
            BodyRef::Block(body) => {
                if super::visit_factory_returns::count_returns_in_statements(&body.statements) != 1
                {
                    return None;
                }
                super::visit_factory_returns::first_return_arg_in_statements(&body.statements)?
            }
        };
        let Expression::Identifier(reference) = unwrap_static_expr(expression) else {
            return None;
        };
        let name = reference.name.to_string();
        let class = self.direct_binding_class_proof(&name, Some(reference.span))?;
        let declaration = self.direct_binding_entry(&name)?.declaration_span?;
        Some(CallbackResultProof {
            class,
            lazy_reference: Some(reference.span),
            capture: Some((
                name,
                declaration,
                Span::new(
                    parameters.start,
                    match body {
                        BodyRef::Concise(expression) => expression.span().end,
                        BodyRef::Block(body) => body.span.end,
                    },
                ),
            )),
            getter: false,
        })
    }

    pub(super) fn retain_callback_result(
        &mut self,
        name: &str,
        declaration: Span,
        callback: super::super::CallbackResultProof,
    ) {
        let mut value = self
            .direct_binding_value(name)
            .map(|(value, _)| self.projection_value_view(value).clone())
            .unwrap_or_default();
        value.callback_result = Some(callback);
        if let Some(value) = self.allocate_projection_value(value) {
            self.attach_direct_binding_value(name, value);
            self.direct_binding_entry_mut(name).declaration_span = Some(declaration);
        }
    }

    fn admitted_callback_result(&self, result: &super::super::CallbackResultProof) -> bool {
        self.admitted_projection_origin(&result.class)
            && result
                .capture
                .as_ref()
                .is_none_or(|(_, declaration, invocation)| {
                    self.subclass_binding_writes
                        .get(declaration)
                        .is_none_or(|writes| {
                            let first =
                                writes.partition_point(|write| write.start < invocation.start);
                            writes
                                .get(first)
                                .is_none_or(|write| write.start >= invocation.end)
                        })
                })
    }

    pub(super) fn emit_local_callback_property_access(
        &mut self,
        callbacks: &[(u32, super::super::CallbackResultProof)],
        parameter_index: u32,
        member: String,
    ) {
        let Some((_, result)) = callbacks
            .iter()
            .find(|(index, result)| *index == parameter_index && !result.getter)
        else {
            return;
        };
        let class = &result.class;
        if self.admitted_callback_result(result) {
            self.emit_local_subclass_member_access(
                &super::super::SubclassAccessProof::Known(class.clone()),
                &member,
            );
            self.member_accesses.push(super::super::MemberAccess {
                object: class.target_name.clone(),
                member,
            });
        }
    }

    pub(super) fn emit_imported_callback_property_access(
        &mut self,
        proof: &FactoryPropertyProof,
        path: &str,
        member: &str,
    ) {
        let FactoryPropertyOrigin::Call {
            name,
            method,
            callbacks,
            ..
        } = &proof.origin
        else {
            return;
        };
        let arguments = callbacks
            .iter()
            .filter(|(_, result)| self.admitted_callback_result(result))
            .map(
                |(index, result)| fallow_types::extract::CallbackFactoryArgument {
                    parameter_index: *index,
                    class_local_name: result.class.target_name.clone(),
                    getter: result.getter,
                    lazy_safe: result.lazy_reference.is_none_or(|reference| {
                        self.subclass_immutable_references.contains(&reference)
                    }),
                },
            )
            .collect::<Vec<_>>();
        let assigned_class = proof
            .assigned_class
            .as_ref()
            .filter(|class| self.admitted_projection_origin(class))
            .map(|class| class.target_name.clone());
        if !arguments.is_empty() || assigned_class.is_some() {
            self.semantic_facts
                .push(super::super::SemanticFact::CallbackFactoryPropertyAccess(
                    Box::new(fallow_types::extract::CallbackFactoryPropertyAccessFact {
                        callee_name: name.clone(),
                        callee_member: method.clone(),
                        property_path: path.to_string(),
                        member: member.to_string(),
                        vue_only: proof.vue_only,
                        assigned_class,
                        arguments,
                    }),
                ));
        }
        if method.is_some() || proof.vue_only || proof.assigned_class.is_some() {
            return;
        }
        self.semantic_facts.push(
            super::super::SemanticFact::ProvenFactoryReturnObjectPropertyAccess(
                fallow_types::extract::FactoryReturnObjectPropertyAccessFact {
                    callee_name: name.clone(),
                    property_path: path.to_string(),
                    member: member.to_string(),
                },
            ),
        );
    }

    pub(super) fn record_callback_value_assignment(
        &mut self,
        path: &str,
        expression: &Expression<'_>,
        plain: bool,
    ) -> bool {
        let Some((parent, "value")) = path.rsplit_once('.') else {
            return false;
        };
        let Some((value, _)) = self.direct_binding_value(parent) else {
            return false;
        };
        let value = self.projection_value_view(value);
        if value.unknown_properties {
            return false;
        }
        let Some(mut proof) = value.factory_property.clone() else {
            return false;
        };
        let FactoryPropertyOrigin::Call {
            name, declaration, ..
        } = &proof.origin
        else {
            return false;
        };
        if !proof.projection.is_empty() || !self.factory_import_binding(name, *declaration) {
            return false;
        }
        let assigned = if plain {
            match unwrap_static_expr(expression) {
                Expression::NewExpression(new) => {
                    if let Expression::Identifier(class) = &new.callee {
                        Some(super::super::BindingClassProof {
                            target_name: class.name.to_string(),
                            target_reference_span: class.span,
                            requirements: super::super::BindingRequirements::Empty,
                            builtin_containers: std::sync::Arc::default(),
                        })
                    } else {
                        None
                    }
                }
                Expression::Identifier(reference) => {
                    self.direct_binding_class_proof(reference.name.as_str(), Some(reference.span))
                }
                _ => None,
            }
        } else {
            None
        };
        proof.projection = "value".to_string();
        proof.vue_only = assigned.is_none();
        proof.assigned_class = assigned;
        self.invalidate_direct_binding_path(path, false);
        if let Some(value) = self.allocate_projection_value(DirectBindingValue {
            factory_property: Some(proof),
            ..DirectBindingValue::default()
        }) {
            self.attach_direct_binding_value(path, value);
        }
        true
    }
}
