//! Factory returns, injection tokens, the literal allowlist of mutating
//! calls, and the element and value types of bindings that the visitor
//! records during the walk.

use super::*;

impl ModuleInfoExtractor {
    /// Record a same-file function whose body returns `new Class()` so a later
    /// `const x = <name>()` binding can resolve to that class. Module scope only;
    /// imported / re-exported factory wrappers are out of scope (issue #1441).
    pub(super) fn record_factory_return_function(
        &mut self,
        name: &str,
        input: FactoryReturnFunctionInput<'_, '_>,
    ) {
        if !self.is_module_scope() {
            return;
        }
        let Some(body) = input.body else {
            return;
        };
        // A cross-module factory must hand back the class instance synchronously:
        // an `async` fn returns `Promise<T>` and a generator returns an iterator,
        // so `const x = make(); x.member` would be on the wrong type. Such
        // factories are excluded from the STRICT (cross-module) map; the same-file
        // (loose) maps below are unaffected. See #1441 (Part A).
        let strict_eligible = !input.is_async && !input.is_generator;
        // An `async` factory hands back a promise of the class. It is exported
        // through its own strict map and binds only an awaited call result.
        let returns_promise = input.is_async && !input.is_generator;
        if returns_promise {
            self.promise_factory_functions.insert(name.to_string());
        } else if input.is_generator {
            self.generator_factory_functions.insert(name.to_string());
        }
        if let Some(class_name) = function_body_returns_new_class(body) {
            // An all-paths-unanimous, non-falling-through proof additionally
            // qualifies this factory for cross-module export (see
            // `strict_factory_return_functions`); the same-file map below keeps
            // the looser last-return semantics.
            if strict_eligible
                && let Some(unanimous_class) = function_body_returns_new_class_unanimous(body)
            {
                self.strict_factory_return_functions
                    .insert(name.to_string(), unanimous_class);
            } else if returns_promise
                && let Some(unanimous_class) = function_body_returns_new_class_unanimous(body)
            {
                self.strict_async_factory_return_functions
                    .insert(name.to_string(), unanimous_class);
            }
            self.factory_return_functions
                .insert(name.to_string(), class_name);
        } else if let Some(returned_id) = function_body_returns_identifier(body, input.params) {
            // The alias is eligible for STRICT promotion only when it returns
            // synchronously and the body cannot fall through to `undefined`. The
            // class is value-proven later in `resolve_factory_return_aliases`.
            if strict_eligible && function_body_is_terminal(body) {
                self.strict_alias_eligible.insert(name.to_string());
                // Collect assignments to the returned id from THIS function's own
                // body (not nested functions), tying the value-proof to the alias
                // function, an assignment in a sibling/unrelated function must not
                // prove it. See #1441 (Part A).
                let mut assignments = Vec::new();
                collect_self_scope_assignments(body.statements(), &returned_id, &mut assignments);
                if !assignments.is_empty() {
                    self.alias_in_body_assignments
                        .insert(name.to_string(), assignments);
                }
            }
            self.factory_return_alias_functions
                .insert(name.to_string(), returned_id);
        } else if strict_eligible
            && let Some(return_type) = input.return_type
            && let Some(class_name) = extract_type_annotation_name(return_type)
        {
            // #1744: no body value-proof (`return registry.get() as Ctrl`), but
            // the function's explicit return-TYPE annotation names a class. Trust
            // the declared contract as a TYPE claim: TypeScript enforces that every
            // return conforms to the annotation, so the returned value IS an
            // instance of that class. This deliberately widens the #1441
            // value-vs-type doctrine (which rejects a returned-IDENTIFIER's
            // variable annotation, `let api: RESTApi`, because an assignment can
            // contradict it) because a FUNCTION return-type annotation is the
            // author's own compiler-checked contract, not a contradictable local.
            // It stays over-credit-safe: the analyze layer credits only when the
            // name resolves to a real class-with-members export, so a wrong
            // annotation (an interface, a primitive, a different class) is a
            // harmless no-op, a false negative at worst, never a false positive.
            self.strict_factory_return_functions
                .insert(name.to_string(), class_name.clone());
            self.factory_return_functions
                .insert(name.to_string(), class_name);
        } else if returns_promise
            && let Some(return_type) = input.return_type
            && let Some(class_name) = return_type_element_name(&return_type.type_annotation)
        {
            // `async function load(): Promise<Gauge>`: the same compiler-checked
            // contract as the sync arm above, for the awaited value.
            self.strict_async_factory_return_functions
                .insert(name.to_string(), class_name.clone());
            self.factory_return_functions
                .insert(name.to_string(), class_name);
        }

        // Object-literal return (`return { orders: factory.ordersPage }`): capture
        // the shape for the object-property member-crediting join. Orthogonal to
        // the single-class arms above (a function returns one or the other), so it
        // is checked unconditionally. Strict (cross-module) eligibility additionally
        // requires a terminal body, so `const ui = createUi()` cannot bind a value
        // the factory only returns on some paths. See issue #1858.
        if self.capture_factory_properties && strict_eligible && function_body_is_terminal(body) {
            self.prepare_factory_property_observations(input.binding_span, input.params, body);
            self.prepare_factory_result_observation(name, input.binding_span, body);
        }
        let properties = function_body_returns_object_shape(body);
        if let Some(properties) = &properties {
            let is_strict_eligible = strict_eligible && function_body_is_terminal(body);
            self.factory_return_object_candidates.push(
                crate::visitor::FactoryReturnObjectCandidate {
                    fn_name: name.to_string(),
                    is_strict_eligible,
                    properties: properties.clone(),
                },
            );
        }
        self.record_factory_binding_producer(name, input, body, properties, returns_promise);
        if strict_eligible
            && (self.strict_factory_return_functions.contains_key(name)
                || self.strict_alias_eligible.contains(name))
            && let Some(callback) =
                self.callback_body_proof(body, input.params.span, input.return_type)
        {
            self.retain_callback_result(name, input.binding_span, callback);
        }
    }

    fn record_factory_binding_producer(
        &mut self,
        name: &str,
        input: FactoryReturnFunctionInput<'_, '_>,
        body: BodyRef<'_, '_>,
        properties: Option<Vec<(String, crate::visitor::ObjectPropValueRef)>>,
        returns_promise: bool,
    ) {
        if self.capture_subclass_provenance || self.capture_factory_properties {
            self.record_binding_declaration(name, input.binding_span);
            let result = visit_factory_returns::function_body_returns_new_class_target(body)
                .map(
                    |(target_name, target_reference_span)| crate::visitor::BindingClassProof {
                        builtin_containers: std::sync::Arc::default(),
                        target_name,
                        target_reference_span,
                        requirements: crate::visitor::BindingRequirements::Empty,
                    },
                )
                .or_else(|| {
                    input.return_type.and_then(|annotation| {
                        let (target_name, target_reference_span) = if returns_promise {
                            let (name, origin) =
                                crate::visitor::helpers::return_type_element_target(
                                    &annotation.type_annotation,
                                )?;
                            (name, origin?)
                        } else {
                            (
                                extract_type_annotation_name(annotation)?,
                                crate::visitor::helpers::extract_type_reference_origin(
                                    &annotation.type_annotation,
                                )?,
                            )
                        };
                        Some(crate::visitor::BindingClassProof {
                            builtin_containers: std::sync::Arc::default(),
                            target_name,
                            target_reference_span,
                            requirements: crate::visitor::BindingRequirements::Empty,
                        })
                    })
                });
            let shape = properties
                .into_iter()
                .flatten()
                .filter_map(|(path, value)| {
                    let crate::visitor::ObjectPropValueRef::Class(
                        target_name,
                        Some(target_reference_span),
                    ) = value
                    else {
                        return None;
                    };
                    Some((
                        path,
                        crate::visitor::BindingClassProof {
                            builtin_containers: std::sync::Arc::default(),
                            target_name,
                            target_reference_span,
                            requirements: crate::visitor::BindingRequirements::Empty,
                        },
                    ))
                })
                .collect();
            self.register_binding_producer(
                name,
                crate::visitor::BindingProducer {
                    name: name.to_string(),
                    declaration_span: input.binding_span,
                    result,
                    shape,
                    is_async: input.is_async,
                    is_generator: input.is_generator,
                    is_static_method: false,
                },
            );
        }
    }

    /// Capture `const local = callee(...)` or `const local = await callee(...)`
    /// (bare-identifier callee) as a factory return candidate. `resolve_factory_return_candidates` keeps only those
    /// whose callee is a known same-file `new Class()` factory or an imported
    /// callee (cross-module). See issue #1441.
    ///
    /// Not scope-gated, mirroring the `const n = new Class()` instance binding:
    /// the consumer is commonly inside a setup/composable function, and
    /// `binding_target_names` is module-flat by design.
    pub(super) fn record_factory_return_candidate(
        &mut self,
        declarator: &VariableDeclarator<'_>,
        init: &Expression<'_>,
    ) {
        let (call, awaited) = match init {
            Expression::AwaitExpression(await_expr) => (&await_expr.argument, true),
            _ => (init, false),
        };
        let Some(callee_name) = Self::bare_call_callee_name(self.class_carrying_init(call)) else {
            return;
        };

        match &declarator.id {
            // Per-module breadth cap (issue #1843 follow-up): the guard stops
            // recording once at capacity so a pathological bundle cannot grow the
            // candidate set (and its finalize scan) without bound. At capacity the
            // arm falls through to the no-op `_ =>` arm, identical to skipping.
            BindingPattern::BindingIdentifier(id)
                if self.factory_return_candidates.len() < MAX_FACTORY_RETURN_CANDIDATES =>
            {
                if self.capture_subclass_provenance {
                    let call = match unwrap_static_expr(init) {
                        Expression::AwaitExpression(expression) => {
                            unwrap_static_expr(&expression.argument)
                        }
                        expression => expression,
                    };
                    if let Expression::CallExpression(call) = call
                        && let Expression::Identifier(reference) = &call.callee
                        && let Some(value) = self.capture_binding_producer_call(
                            reference.name.as_str(),
                            reference.span,
                            awaited,
                        )
                    {
                        self.attach_captured_binding_value(id.name.as_str(), value);
                    }
                }
                self.factory_return_candidates
                    .push(crate::visitor::FactoryReturnCandidate {
                        local_name: id.name.to_string(),
                        callee_name,
                        awaited,
                    });
            }
            // `const { a, b } = useApi()`. The instance is never named, so queue one
            // direct factory-result access per statically named key. Dropping this
            // shape is what reported every member of a destructured factory result
            // as unused.
            BindingPattern::ObjectPattern(pattern) => {
                let Some(keys) = crate::visitor::destructured_factory_keys(pattern) else {
                    // A rest element or computed key can read any property.
                    if !awaited {
                        self.factory_whole_object_candidates.push(callee_name);
                    }
                    return;
                };
                let call = match unwrap_static_expr(init) {
                    Expression::AwaitExpression(expression) => {
                        unwrap_static_expr(&expression.argument)
                    }
                    expression => expression,
                };
                let captured = if let Expression::CallExpression(call) = call
                    && let Expression::Identifier(reference) = &call.callee
                {
                    self.capture_binding_producer_call(
                        reference.name.as_str(),
                        reference.span,
                        awaited,
                    )
                } else {
                    None
                };
                for key in keys {
                    if let Some(id) = captured {
                        self.pending_subclass_member_accesses.push(
                            crate::visitor::PendingSubclassMemberAccess {
                                proof: crate::visitor::SubclassAccessProof::Value {
                                    id,
                                    requirements: crate::visitor::BindingRequirements::Empty,
                                },
                                member: key.clone(),
                            },
                        );
                    }
                    self.factory_unnamed_result_accesses
                        .push((callee_name.clone(), key, awaited));
                }
            }
            _ => {}
        }
    }

    /// The callee of a bare `identifier(...)` call. A member-expression callee
    /// (`obj.f()`) and a call-of-a-call (`f()()`) are deliberately not matched:
    /// neither resolves to a proven factory export, so crediting through them would
    /// be a guess.
    pub(super) fn bare_call_callee_name(expression: &Expression<'_>) -> Option<String> {
        let Expression::CallExpression(call) = expression else {
            return None;
        };
        let Expression::Identifier(callee) = &call.callee else {
            return None;
        };
        Some(callee.name.to_string())
    }

    /// Record `const TOKEN = new InjectionToken<Interface>(...)` declarations
    /// so the analyze layer can follow the token's interface type argument to
    /// the classes that `implement` it. Gated on `InjectionToken` being a named
    /// import from `@angular/core` (a same-named local class is ignored). A
    /// token with no type argument carries no interface for the template-chain
    /// bridge, but a tree-shakable token (a `{ factory }` / `{ providedIn }`
    /// second argument) still records a self-provide DI site so the
    /// `unprovided-inject` detector treats it as provided. See issues #920 and
    /// the Angular `unprovided-inject` arm.
    fn record_injection_token(&mut self, name: &str, init: &Expression<'_>) {
        if !self.is_module_scope() {
            return;
        }
        let Expression::NewExpression(new_expr) = init else {
            return;
        };
        let Expression::Identifier(callee) = &new_expr.callee else {
            return;
        };
        if !self.is_named_import_from(callee.name.as_str(), "@angular/core", "InjectionToken") {
            return;
        }

        // A tree-shakable token (`new InjectionToken('x', { factory: () => ... })`
        // or `{ providedIn: 'root' }`) provides itself, so record it as a Provide
        // of its own const name. This makes a self-providing token count as
        // provided even without a `{ provide: TOKEN }` object anywhere.
        if let Some(Argument::ObjectExpression(options)) = new_expr.arguments.get(1)
            && object_has_any_key(options, &["factory", "providedIn"])
        {
            self.di_key_sites.push(DiKeySite {
                key_local: name.to_string(),
                role: DiRole::Provide,
                framework: DiFramework::Angular,
                span_start: new_expr.span.start,
            });
        }

        // Record EVERY `new InjectionToken(...)` declaration so the
        // `unprovided-inject` FP gate recognizes it as a token regardless of its
        // type argument. The interface name (consumed by the #920 template-member
        // bridge in `unused_members.rs`) comes from a type-REFERENCE type argument
        // (`new InjectionToken<Greeter>(...)`); a primitive type argument
        // (`<string>`) or no type argument yields an empty interface, which the
        // bridge harmlessly skips (it credits implementers of a named interface,
        // and no interface is named "").
        let interface_name = new_expr
            .type_arguments
            .as_deref()
            .and_then(|type_arguments| type_arguments.params.first())
            .and_then(|param| match param {
                TSType::TSTypeReference(type_ref) => {
                    type_name_root(&type_ref.type_name).map(|(interface_name, _)| interface_name)
                }
                _ => None,
            })
            .unwrap_or_default();
        self.injection_tokens
            .push((name.to_string(), interface_name));
    }

    pub(super) fn clear_literal_allowlist_on_mutating_member_call(
        &mut self,
        call: &CallExpression<'_>,
    ) {
        if let Expression::StaticMemberExpression(member) = &call.callee
            && let Expression::Identifier(object) = &member.object
            && !matches!(member.property.name.as_str(), "has" | "includes")
            && self.literal_allowlist_binding(&object.name)
        {
            self.record_literal_allowlist_binding(object.name.as_str(), false);
        }
    }

    pub(super) fn svelte_derived_new_class(init: &Expression<'_>) -> Option<String> {
        let Expression::CallExpression(call) = init else {
            return None;
        };
        if !Self::is_svelte_derived_call(call) {
            return None;
        }

        if let Some(expr) = call.arguments.first().and_then(Argument::as_expression)
            && let Expression::NewExpression(new_expr) = expr
            && let Expression::Identifier(callee) = &new_expr.callee
            && !crate::visitor::helpers::is_builtin_constructor(callee.name.as_str())
        {
            return Some(callee.name.to_string());
        }

        crate::visitor::helpers::try_extract_factory_new_class(&call.arguments)
    }

    fn is_svelte_derived_call(call: &CallExpression<'_>) -> bool {
        match &call.callee {
            Expression::Identifier(id) => id.name == "$derived",
            Expression::StaticMemberExpression(member) => {
                member.property.name == "by"
                    && matches!(&member.object, Expression::Identifier(id) if id.name == "$derived")
            }
            _ => false,
        }
    }

    /// Substitute a class type-parameter with its constraint when available.
    pub(super) fn resolve_class_type_param(&self, type_name: &str) -> Option<String> {
        let Some(frame) = self.class_type_param_constraints.last() else {
            return Some(type_name.to_string());
        };
        match frame.get(type_name) {
            Some(Some(constraint)) => Some(constraint.name.clone()),
            Some(None) => None,
            None => Some(type_name.to_string()),
        }
    }

    pub(super) fn resolve_class_type_param_origin(
        &self,
        type_name: &str,
        origin: Option<Span>,
    ) -> Option<Span> {
        match self
            .class_type_param_constraints
            .last()
            .and_then(|frame| frame.get(type_name))
        {
            Some(Some(constraint)) => constraint.reference_span,
            Some(None) => None,
            None => origin,
        }
    }

    /// Recognize `.forEach(...)` on iterables and bind the callback element.
    pub(super) fn bind_iterable_callback_parameter(&mut self, expr: &CallExpression<'_>) {
        let (receiver_expr, method_name) = match &expr.callee {
            Expression::StaticMemberExpression(member) => (&member.object, &member.property.name),
            Expression::ChainExpression(chain) => match &chain.expression {
                ChainElement::StaticMemberExpression(member) => {
                    (&member.object, &member.property.name)
                }
                _ => return,
            },
            _ => return,
        };
        // Array iteration methods whose callback's FIRST parameter is an element.
        // `reduce` / `reduceRight` are excluded because their first callback
        // parameter is the accumulator, not an element. See issue #1707 follow-up.
        if !ITERABLE_ELEMENT_CALLBACK_METHODS.contains(&method_name.as_str()) {
            return;
        }
        let Some(receiver_name) = static_member_object_name(receiver_expr) else {
            return;
        };
        // A `this.<field>()` / `this.<field>` iteration receiver is class-scoped
        // so it matches the same class's qualified query-list key (issue #1821).
        let receiver_name = self.qualify_this_scope(&receiver_name);
        let map_value_type = (method_name == "forEach")
            .then(|| self.map_value_type_for(&receiver_name))
            .flatten();
        let Some(element_type) = map_value_type
            .or_else(|| self.iterable_element_type_for(&receiver_name))
            // `all().forEach((t) => ...)` with `function all(): Table[]` (issue #4313).
            .or_else(|| self.local_call_array_element_type(receiver_expr))
        else {
            return;
        };
        let Some(first_arg) = expr.arguments.first() else {
            return;
        };
        let param_name = match first_arg {
            Argument::ArrowFunctionExpression(arrow) => {
                arrow.params.items.first().and_then(|p| match &p.pattern {
                    BindingPattern::BindingIdentifier(id) => Some((id.name.to_string(), id.span)),
                    _ => None,
                })
            }
            Argument::FunctionExpression(func) => {
                func.params.items.first().and_then(|p| match &p.pattern {
                    BindingPattern::BindingIdentifier(id) => Some((id.name.to_string(), id.span)),
                    _ => None,
                })
            }
            _ => None,
        };
        if let Some((name, declaration_span)) = param_name {
            {
                let proof = self.iteration_binding_class_proof(
                    &receiver_name,
                    iteration_source_reference(receiver_expr),
                );
                self.map_callback_value_bindings.insert(
                    first_arg.span().start,
                    (name, declaration_span, element_type, proof),
                );
            }
        }
    }

    pub(super) fn bind_map_callback_value(&mut self, start: u32) {
        if let Some((binding, declaration_span, class_name, proof)) =
            self.map_callback_value_bindings.remove(&start)
        {
            self.record_binding_declaration(&binding, declaration_span);
            if let Some(proof) = proof {
                self.record_captured_binding_class_proof(&binding, proof);
            }
            self.record_scoped_class_binding(&binding, class_name);
        }
    }

    /// The element class of an iterable receiver, consulting the Angular
    /// query-list map first (`this.items()`) then the general array / reactive
    /// array binding map (`const utils: Util[]`). Reused by the `for...of` and
    /// array-callback iteration-binding paths (issue #1707 follow-up).
    pub(super) fn iterable_element_type_for(&self, receiver_name: &str) -> Option<String> {
        self.iterable_element_types
            .get(receiver_name)
            .or_else(|| self.array_binding_element_types.get(receiver_name))
            .or_else(|| {
                self.scoped_array_binding_element_types
                    .iter()
                    .rev()
                    .find_map(|scope| scope.get(receiver_name))
            })
            .cloned()
            .or_else(|| self.binding_iteration_target(receiver_name, false))
            .or_else(|| {
                let map_name = receiver_name.strip_suffix(".values()")?;
                self.map_value_type_for(map_name)
            })
    }

    fn array_type_origin(ty: &TSType<'_>) -> Option<Span> {
        match ty {
            TSType::TSArrayType(array) => {
                crate::visitor::helpers::extract_type_reference_origin(&array.element_type)
            }
            TSType::TSTypeReference(reference) => reference
                .type_arguments
                .as_deref()?
                .params
                .first()
                .and_then(crate::visitor::helpers::extract_type_reference_origin),
            TSType::TSParenthesizedType(paren) => Self::array_type_origin(&paren.type_annotation),
            TSType::TSTypeOperatorType(operator) => {
                Self::array_type_origin(&operator.type_annotation)
            }
            TSType::TSUnionType(union) => union.types.iter().find_map(Self::array_type_origin),
            _ => None,
        }
    }

    pub(super) fn record_array_binding_element_type(&mut self, binding: String, element: String) {
        if self.is_module_scope() {
            self.array_binding_element_types.insert(binding, element);
        } else if let Some(scope) = self.scoped_array_binding_element_types.last_mut() {
            scope.insert(binding, element);
        }
    }

    fn record_map_binding_value_type(&mut self, binding: String, value: String) {
        let root = binding
            .split_once('.')
            .map_or(binding.as_str(), |(root, _)| root);
        if let Some(index) = self
            .scoped_direct_object_binding_roots
            .iter()
            .rposition(|roots| roots.contains(root))
        {
            self.scoped_map_binding_value_types[index].insert(binding, value);
        } else {
            self.map_binding_value_types.insert(binding, value);
        }
    }

    /// The value class of a map-typed receiver name (already `this`-qualified).
    pub(super) fn map_value_type_for(&self, receiver_name: &str) -> Option<String> {
        let root = receiver_name
            .split_once('.')
            .map_or(receiver_name, |(root, _)| root);
        for (scope, roots) in self
            .scoped_map_binding_value_types
            .iter()
            .zip(&self.scoped_direct_object_binding_roots)
            .rev()
        {
            if let Some(value) = scope.get(receiver_name) {
                return Some(value.clone());
            }
            if roots.contains(root) {
                return None;
            }
        }
        self.map_binding_value_types
            .get(receiver_name)
            .cloned()
            .or_else(|| self.binding_iteration_target(receiver_name, true))
    }

    /// The value class `V` of a `map.get(key)` expression when `map` is a
    /// binding typed `Map<K, V>`. Looks through `( )`, `!` and `?.` so
    /// `map.get(key)?.m()` and `map.get(key)!.m()` both resolve.
    pub(super) fn map_get_value_type(&self, expr: &Expression<'_>) -> Option<String> {
        let call = match expr {
            Expression::ParenthesizedExpression(paren) => {
                return self.map_get_value_type(&paren.expression);
            }
            Expression::TSNonNullExpression(non_null) => {
                return self.map_get_value_type(&non_null.expression);
            }
            Expression::CallExpression(call) => call.as_ref(),
            Expression::ChainExpression(chain) => match &chain.expression {
                ChainElement::CallExpression(call) => call.as_ref(),
                _ => return None,
            },
            _ => return None,
        };
        let Expression::StaticMemberExpression(callee) = &call.callee else {
            return None;
        };
        if callee.property.name != "get" || call.arguments.len() != 1 {
            return None;
        }
        let receiver_name = static_member_object_name(&callee.object)?;
        self.map_value_type_for(&self.qualify_this_scope(&receiver_name))
    }

    /// Record the value class of a binding typed `Map<K, V>` or initialized
    /// with `new Map<K, V>()`. The annotation wins over the initializer.
    pub(super) fn record_map_binding_from_declaration(
        &mut self,
        binding: String,
        type_annotation: Option<&TSTypeAnnotation<'_>>,
        init: Option<&Expression<'_>>,
    ) {
        let value = match type_annotation {
            Some(annotation) => map_value_type_from_type(&annotation.type_annotation),
            None => init.and_then(map_value_type_from_new),
        };
        if let Some(value) = value.and_then(|value| self.resolve_class_type_param(&value)) {
            if self.capture_subclass_provenance {
                let origin = type_annotation
                    .and_then(|annotation| Self::map_type_origin(&annotation.type_annotation))
                    .or_else(|| match init.map(unwrap_static_expr) {
                        Some(Expression::NewExpression(expression)) => expression
                            .type_arguments
                            .as_deref()
                            .and_then(|arguments| arguments.params.get(1))
                            .and_then(crate::visitor::helpers::extract_type_reference_origin),
                        _ => None,
                    });
                let raw = type_annotation
                    .and_then(|annotation| map_value_type_from_type(&annotation.type_annotation))
                    .or_else(|| init.and_then(map_value_type_from_new));
                let origin = raw
                    .as_deref()
                    .and_then(|raw| self.resolve_class_type_param_origin(raw, origin));
                self.record_binding_iteration_proof(&binding, &value, origin, true);
            }
            self.record_map_binding_value_type(binding, value);
        }
    }

    fn map_type_origin(ty: &TSType<'_>) -> Option<Span> {
        match ty {
            TSType::TSTypeReference(reference) => reference
                .type_arguments
                .as_deref()?
                .params
                .get(1)
                .and_then(crate::visitor::helpers::extract_type_reference_origin),
            TSType::TSParenthesizedType(parenthesized) => {
                Self::map_type_origin(&parenthesized.type_annotation)
            }
            TSType::TSUnionType(union) => union.types.iter().find_map(Self::map_type_origin),
            _ => None,
        }
    }

    pub(super) fn is_vue_array_wrapper_call(call: &CallExpression<'_>) -> bool {
        matches!(&call.callee, Expression::Identifier(callee) if matches!(callee.name.as_str(), "ref" | "shallowRef" | "computed"))
    }

    pub(super) fn record_vue_ref_value_array_binding_type(
        &mut self,
        binding: &str,
        init: &Expression<'_>,
        element: &str,
    ) {
        let Expression::CallExpression(call) = init else {
            return;
        };
        let Expression::Identifier(callee) = &call.callee else {
            return;
        };
        if matches!(callee.name.as_str(), "ref" | "shallowRef" | "computed") {
            self.record_array_binding_element_type(format!("{binding}.value"), element.to_string());
        }
    }

    pub(super) fn record_object_array_member_binding_types(
        &mut self,
        binding: &str,
        type_annotation: Option<&TSTypeAnnotation<'_>>,
    ) {
        let Some(type_annotation) = type_annotation else {
            return;
        };
        let TSType::TSTypeLiteral(literal) = &type_annotation.type_annotation else {
            return;
        };
        for member in &literal.members {
            let TSSignature::TSPropertySignature(prop) = member else {
                continue;
            };
            let Some(property_name) = prop.key.static_name().map(static_name_text) else {
                continue;
            };
            let Some(property_type) = prop.type_annotation.as_deref() else {
                continue;
            };
            let Some(element) = array_element_type_from_type(&property_type.type_annotation) else {
                continue;
            };
            self.record_binding_iteration_proof(
                &format!("{binding}.{property_name}"),
                &element,
                Self::array_type_origin(&property_type.type_annotation),
                false,
            );
            self.record_array_binding_element_type(format!("{binding}.{property_name}"), element);
        }
    }

    pub(in crate::visitor) fn is_named_import_from(
        &self,
        local_name: &str,
        source: &str,
        imported_name: &str,
    ) -> bool {
        self.imports.iter().any(|import| {
            import.source == source
                && import.local_name == local_name
                && matches!(&import.imported_name, ImportedName::Named(name) if name == imported_name)
        })
    }

    pub(super) fn record_source_returning_helper_from_variable_declarator(
        &mut self,
        decl: &VariableDeclaration<'_>,
        declarator: &VariableDeclarator<'_>,
        init: &Expression<'_>,
    ) {
        if !self.is_module_scope() {
            return;
        }
        let BindingPattern::BindingIdentifier(id) = &declarator.id else {
            return;
        };
        if decl.kind != VariableDeclarationKind::Const {
            self.source_returning_helpers.remove(id.name.as_str());
            return;
        }
        let helper = match init {
            Expression::ArrowFunctionExpression(arrow) => extract_arrow_return_expr(arrow)
                .and_then(|expr| source_returning_helper(&arrow.params, expr)),
            Expression::FunctionExpression(function) => function
                .body
                .as_deref()
                .and_then(extract_function_body_final_return_expr)
                .and_then(|expr| source_returning_helper(&function.params, expr)),
            _ => None,
        };
        if let Some(helper) = helper {
            self.source_returning_helpers
                .insert(id.name.to_string(), helper);
        } else {
            self.source_returning_helpers.remove(id.name.as_str());
        }
    }

    pub(super) fn record_initialized_declarator_bindings(
        &mut self,
        decl: &VariableDeclaration<'_>,
        declarator: &VariableDeclarator<'_>,
        init: &Expression<'_>,
    ) {
        if let BindingPattern::BindingIdentifier(id) = &declarator.id {
            if decl.kind == VariableDeclarationKind::Const && self.is_module_scope() {
                self.record_module_const_values(id.name.as_str(), init);
            }

            self.record_static_sink_literal_binding(decl, declarator, init);
            self.record_sink_alias_bindings(id.name.as_str(), init);
            let sources = self.node_module_register_sources_from_expression(init);
            if !sources.is_empty() {
                self.record_node_module_register_url_binding(id.name.to_string(), sources);
            }
            self.record_current_module_file_path_binding(id.name.as_str(), init);
            self.record_injection_token(id.name.as_str(), init);
            self.record_di_string_key_const(id.name.as_str(), decl, init);
            self.record_event_dispatch_binding(id.name.as_str(), init);
            self.record_child_process_fork_target_binding(id.name.as_str(), init);
            self.record_tainted_source_binding(id.name.as_str(), init);
            self.record_tainted_helper_call_binding(id.name.as_str(), init);
            // #1146 chain step AFTER the direct captures: a same-declarator
            // direct source read seeds hop 1 first, so the dedup min-merge in
            // `push_tainted_binding` keeps the direct depth.
            self.record_chained_tainted_binding(id.name.as_str(), init);
            let sanitizer_scope = self.sanitizer_scope_for_expr(init);
            self.record_sanitizer_binding(id.name.as_str(), sanitizer_scope);
            let allowlist = decl.kind == VariableDeclarationKind::Const
                && is_literal_string_allowlist_expr(init);
            self.record_literal_allowlist_binding(id.name.as_str(), allowlist);
            self.record_path_sink_binding(id.name.as_str(), self.path_sink_binding_for_expr(init));
            self.record_path_relative_binding(
                id.name.as_str(),
                self.path_relative_target_for_expr(init),
            );
        } else {
            for id in declarator.id.get_binding_identifiers() {
                self.record_sanitizer_binding(id.name.as_str(), None);
                self.record_literal_allowlist_binding(id.name.as_str(), false);
                self.record_path_sink_binding(id.name.as_str(), None);
                self.record_path_relative_binding(id.name.as_str(), None);
            }
        }
    }
}
