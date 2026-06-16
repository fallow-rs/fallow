//! Vue `<script setup>` `defineProps` harvesting for the `unused-component-prop`
//! detector.
//!
//! Harvests declared prop names from a parsed `<script setup>` program, in both
//! the runtime object form (`defineProps({ foo: {...} })`) and the inline TS
//! literal form (`defineProps<{ foo: T }>()`), unwrapping `withDefaults(...)`.
//! Also computes each prop's `used_in_script` flag (a destructured local binding
//! with a resolved reference, or a `props.<name>` member access where `props` is
//! the `defineProps` return binding) and the whole-file abstain flags. Template
//! usage (`used_in_template`) is applied separately in `sfc.rs::apply_template_usage`.
//!
//! Zero-FP doctrine: every shape that cannot be statically harvested (a
//! type-reference type argument such as `defineProps<Props>()`, a rest-destructure
//! of the props return, `defineExpose` / `defineModel`) sets an abstain flag so
//! the detector skips the whole file rather than risk a false positive.

use oxc_ast::ast::*;
use oxc_semantic::SemanticBuilder;
use rustc_hash::{FxHashMap, FxHashSet};

use fallow_types::extract::{ComponentEmit, ComponentProp};

/// Result of harvesting `defineProps` from a `<script setup>` program.
#[derive(Debug, Default)]
pub struct DefinePropsHarvest {
    /// Declared props with their span and `used_in_script` flag. The
    /// `used_in_template` flag is left `false` here and set in `apply_template_usage`.
    pub props: Vec<ComponentProp>,
    /// `defineProps` had a type-reference type argument (names unharvestable).
    pub has_unharvestable_props: bool,
    /// The `defineProps` return is rest-destructured (`const { ...rest } = ...`).
    pub has_props_attrs_fallthrough: bool,
    /// `defineExpose(...)` was called.
    pub has_define_expose: bool,
    /// `defineModel(...)` was called.
    pub has_define_model: bool,
    /// The `defineProps` return binding name (`const props = defineProps(...)`),
    /// used by the template scanner to credit `props.<name>` member accesses in
    /// the template. `None` for the destructure form.
    pub props_return_binding: Option<String>,
}

/// Harvest `defineProps` declared props and abstain flags from a `<script setup>`
/// program. The byte spans returned are RELATIVE to the script body; the caller
/// remaps them onto the SFC source.
pub fn harvest_define_props(program: &Program<'_>) -> DefinePropsHarvest {
    let mut harvest = DefinePropsHarvest::default();

    // A pass over top-level statements: find the defineProps call, its return
    // binding (for member-access credit), the destructured prop locals (for
    // resolved-reference credit), and defineExpose / defineModel presence.
    let mut props_return_binding: Option<String> = None;
    let mut destructured_locals: FxHashSet<String> = FxHashSet::default();
    // prop name -> local binding name (for `const { name: alias } = defineProps()`).
    let mut prop_aliases: FxHashMap<String, String> = FxHashMap::default();
    let mut prop_names: Vec<(String, u32)> = Vec::new();

    for stmt in &program.body {
        match stmt {
            Statement::VariableDeclaration(decl) => {
                for declarator in &decl.declarations {
                    let Some(init) = &declarator.init else {
                        continue;
                    };
                    // `const m = defineModel(...)` / `const e = defineExpose(...)`:
                    // detect the macro on the assigned-call form too.
                    if let Expression::CallExpression(call) = init {
                        inspect_macro_call(call, &mut harvest);
                    }
                    let Some(call) = unwrap_define_props_call(init) else {
                        continue;
                    };
                    if prop_names.is_empty() && !harvest.has_unharvestable_props {
                        collect_define_props_names(call, &mut prop_names, &mut harvest);
                    }
                    bind_define_props_target(
                        &declarator.id,
                        &mut props_return_binding,
                        &mut destructured_locals,
                        &mut prop_aliases,
                        &mut harvest,
                    );
                }
            }
            Statement::ExpressionStatement(expr_stmt) => {
                // Bare `defineProps(...)` / `defineExpose(...)` / `defineModel(...)`.
                if let Expression::CallExpression(call) = &expr_stmt.expression {
                    inspect_macro_call(call, &mut harvest);
                    if prop_names.is_empty()
                        && !harvest.has_unharvestable_props
                        && let Some(inner) = unwrap_define_props_call(&expr_stmt.expression)
                    {
                        collect_define_props_names(inner, &mut prop_names, &mut harvest);
                    }
                }
            }
            _ => {}
        }
    }

    if prop_names.is_empty() {
        return harvest;
    }

    // Script usage: resolved references for destructured locals, plus member
    // accesses `props.<name>` against the return binding.
    let used_locals = resolve_used_locals(program, &destructured_locals);
    let (member_used, props_used_whole) = props_return_binding.as_deref().map_or_else(
        || (FxHashSet::default(), false),
        |binding| collect_prop_binding_usage(program, binding),
    );

    // Whole-object use of the props binding (`toRefs(props)`, `{ ...props }`,
    // `someFn(props)`, `return props`) consumes every prop opaquely, the
    // script-side analog of `v-bind="props"`. Abstain on the whole file.
    if props_used_whole {
        harvest.has_props_attrs_fallthrough = true;
    }

    for (name, span_start) in prop_names {
        // A renamed prop (`const { name: alias } = defineProps()`) is read through
        // its local alias; default the local to the prop name (shorthand
        // destructure, or the non-destructure `props.name` / template `name` form).
        let local = prop_aliases
            .get(&name)
            .cloned()
            .unwrap_or_else(|| name.clone());
        let used_in_script = used_locals.contains(&local) || member_used.contains(&name);
        harvest.props.push(ComponentProp {
            name,
            local,
            span_start,
            used_in_script,
            used_in_template: false,
            // Vue: one component per `.vue` file; the detector derives the
            // component name from the file stem, so this stays empty.
            component: String::new(),
            // React-only forward-vs-consume signal; Vue does not compute it.
            used_outside_forward: false,
        });
    }

    harvest.props_return_binding = props_return_binding;
    harvest
}

/// Unwrap an expression to the inner `defineProps(...)` call, peeling
/// `withDefaults(defineProps(...), {...})`. Returns `None` for anything else.
fn unwrap_define_props_call<'a, 'b>(expr: &'b Expression<'a>) -> Option<&'b CallExpression<'a>> {
    let Expression::CallExpression(call) = expr else {
        return None;
    };
    let callee_name = simple_callee_name(&call.callee)?;
    if callee_name == "defineProps" {
        return Some(call);
    }
    if callee_name == "withDefaults" {
        let first = call.arguments.first()?.as_expression()?;
        return unwrap_define_props_call(first);
    }
    None
}

/// The bare identifier name of a call's callee, or `None` for member / computed callees.
fn simple_callee_name<'a>(callee: &Expression<'a>) -> Option<&'a str> {
    match callee {
        Expression::Identifier(ident) => Some(ident.name.as_str()),
        _ => None,
    }
}

/// Record `defineExpose` / `defineModel` presence from any call expression.
fn inspect_macro_call(call: &CallExpression<'_>, harvest: &mut DefinePropsHarvest) {
    if let Some(name) = simple_callee_name(&call.callee) {
        match name {
            "defineExpose" => harvest.has_define_expose = true,
            "defineModel" => harvest.has_define_model = true,
            _ => {}
        }
    }
}

/// Collect prop names from a `defineProps(...)` call: the runtime object-literal
/// keys, or the inline TS type-literal member names. A type-reference type
/// argument sets `has_unharvestable_props` and harvests nothing.
fn collect_define_props_names(
    call: &CallExpression<'_>,
    prop_names: &mut Vec<(String, u32)>,
    harvest: &mut DefinePropsHarvest,
) {
    // Inline TS form: `defineProps<{ foo: T }>()`.
    if let Some(type_args) = &call.type_arguments {
        if let Some(first) = type_args.params.first() {
            match first {
                TSType::TSTypeLiteral(lit) => {
                    for member in &lit.members {
                        if let TSSignature::TSPropertySignature(sig) = member
                            && let Some(name) = property_key_name(&sig.key)
                        {
                            prop_names.push((name, sig.span.start));
                        }
                    }
                }
                // A type reference (`defineProps<Props>()`) or any non-literal
                // type argument: names require cross-file resolution. Abstain.
                _ => harvest.has_unharvestable_props = true,
            }
        }
        return;
    }

    // Runtime object form: `defineProps({ foo: {...}, bar: {...} })`.
    if let Some(first) = call.arguments.first().and_then(|arg| arg.as_expression()) {
        match first {
            Expression::ObjectExpression(obj) => {
                for prop in &obj.properties {
                    match prop {
                        ObjectPropertyKind::ObjectProperty(p) => {
                            if let Some(name) = property_key_name(&p.key) {
                                prop_names.push((name, p.span.start));
                            }
                        }
                        // Spread inside the props object (`{ ...base }`) hides
                        // names: abstain on the whole file.
                        ObjectPropertyKind::SpreadProperty(_) => {
                            harvest.has_unharvestable_props = true;
                        }
                    }
                }
            }
            // Array form `defineProps(['foo', 'bar'])`.
            Expression::ArrayExpression(arr) => {
                for element in &arr.elements {
                    if let ArrayExpressionElement::StringLiteral(lit) = element {
                        prop_names.push((lit.value.to_string(), lit.span.start));
                    } else if !matches!(element, ArrayExpressionElement::Elision(_)) {
                        // A non-literal array element (spread / computed): abstain.
                        harvest.has_unharvestable_props = true;
                    }
                }
            }
            // A non-object, non-array argument (an identifier / call): abstain.
            _ => harvest.has_unharvestable_props = true,
        }
    }
}

/// The static name of an object-property or type-property key.
fn property_key_name(key: &PropertyKey<'_>) -> Option<String> {
    key.static_name().map(|name| name.to_string())
}

/// The local binding name of a destructured prop value, peeling an
/// `AssignmentPattern` (a default value, `{ foo = 2 }`). Returns `None` for a
/// nested object/array destructure (out of scope: a prop is a flat value).
fn binding_local_name<'a>(pattern: &'a BindingPattern<'a>) -> Option<&'a str> {
    match pattern {
        BindingPattern::BindingIdentifier(ident) => Some(ident.name.as_str()),
        BindingPattern::AssignmentPattern(assign) => binding_local_name(&assign.left),
        _ => None,
    }
}

/// Bind the `defineProps` return target: a simple identifier
/// (`const props = ...`) sets the member-access binding; an object pattern
/// (`const { foo } = ...`) collects destructured locals; a rest element
/// (`const { ...rest } = ...`) sets the fallthrough abstain.
fn bind_define_props_target(
    id: &BindingPattern<'_>,
    props_return_binding: &mut Option<String>,
    destructured_locals: &mut FxHashSet<String>,
    prop_aliases: &mut FxHashMap<String, String>,
    harvest: &mut DefinePropsHarvest,
) {
    match id {
        BindingPattern::BindingIdentifier(ident) => {
            *props_return_binding = Some(ident.name.to_string());
        }
        BindingPattern::ObjectPattern(pattern) => {
            for prop in &pattern.properties {
                // A destructured prop may carry a default (`{ foo = 2 }`), which
                // oxc represents as an `AssignmentPattern`; resolve to the local
                // identifier either way.
                if let Some(local) = binding_local_name(&prop.value) {
                    destructured_locals.insert(local.to_string());
                    // Map the declared prop name to its local for `{ name: alias }`;
                    // shorthand `{ name }` maps name -> name.
                    if let Some(prop_name) = property_key_name(&prop.key) {
                        prop_aliases.insert(prop_name, local.to_string());
                    }
                }
            }
            // A rest element (`const { ...rest } = defineProps()`) can carry any
            // prop indirectly: set the fallthrough abstain.
            if pattern.rest.is_some() {
                harvest.has_props_attrs_fallthrough = true;
            }
        }
        _ => {}
    }
}

/// Resolve which of the destructured prop locals have at least one resolved
/// reference in the program (via `oxc_semantic`), mirroring the import-binding
/// usage check in `parse.rs::compute_semantic_usage`.
fn resolve_used_locals(
    program: &Program<'_>,
    destructured_locals: &FxHashSet<String>,
) -> FxHashSet<String> {
    let mut used: FxHashSet<String> = FxHashSet::default();
    if destructured_locals.is_empty() {
        return used;
    }
    let semantic_ret = SemanticBuilder::new().build(program);
    let scoping = semantic_ret.semantic.scoping();
    let root_scope = scoping.root_scope_id();
    for local in destructured_locals {
        let name = oxc_str::Ident::from(local.as_str());
        if let Some(symbol_id) = scoping.get_binding(root_scope, name)
            && scoping.get_resolved_references(symbol_id).next().is_some()
        {
            used.insert(local.clone());
        }
    }
    used
}

/// Inspect every use of the `defineProps` return binding: collect prop names
/// accessed as `<binding>.<name>` (member access), and report whether the binding
/// is ever used as a WHOLE object (`toRefs(props)`, `{ ...props }`,
/// `someFn(props)`, `return props`). A whole-object use consumes every prop
/// opaquely, so the detector must abstain on the whole file.
fn collect_prop_binding_usage(program: &Program<'_>, binding: &str) -> (FxHashSet<String>, bool) {
    let mut visitor = PropBindingVisitor {
        binding,
        accessed: FxHashSet::default(),
        used_whole: false,
    };
    oxc_ast_visit::Visit::visit_program(&mut visitor, program);
    (visitor.accessed, visitor.used_whole)
}

struct PropBindingVisitor<'a> {
    binding: &'a str,
    accessed: FxHashSet<String>,
    used_whole: bool,
}

impl<'a> oxc_ast_visit::Visit<'a> for PropBindingVisitor<'a> {
    fn visit_static_member_expression(&mut self, expr: &StaticMemberExpression<'a>) {
        // `props.foo`: record the member and do NOT descend into the object, so a
        // member access is not also counted as a bare whole-object reference.
        if let Expression::Identifier(ident) = &expr.object
            && ident.name.as_str() == self.binding
        {
            self.accessed.insert(expr.property.name.to_string());
            return;
        }
        oxc_ast_visit::walk::walk_static_member_expression(self, expr);
    }

    fn visit_identifier_reference(&mut self, ident: &IdentifierReference<'a>) {
        // Any bare reference to the props binding that is NOT a `props.<member>`
        // object (those are short-circuited above) is a whole-object use.
        if ident.name.as_str() == self.binding {
            self.used_whole = true;
        }
    }
}

/// Result of harvesting `defineEmits` from a `<script setup>` program for the
/// `unused-component-emit` detector. Mirrors [`DefinePropsHarvest`].
#[derive(Debug, Default)]
pub struct DefineEmitsHarvest {
    /// Declared emit events with their span and `used` flag. An event is `used`
    /// when the bound emit name is called as `emit('<name>')` somewhere in the
    /// program.
    pub emits: Vec<ComponentEmit>,
    /// `defineEmits` had a type-reference type argument (`defineEmits<MyEmits>()`)
    /// or another non-literal form, so the event names are unharvestable.
    pub has_unharvestable_emits: bool,
    /// An `emit(<nonLiteral>)` call was seen: the emitted event cannot be known,
    /// so the detector abstains on the whole file.
    pub has_dynamic_emit: bool,
    /// The emit binding was used as a WHOLE value (passed to a function,
    /// returned, or spread), which can emit any event opaquely. Abstain.
    pub has_emit_whole_object_use: bool,
    /// The `defineEmits` return binding name (`const emit = defineEmits(...)`),
    /// used by the template scanner to credit `<emit>('<name>')` calls in the
    /// template. `None` when no harvestable bound emit exists.
    pub emit_binding: Option<String>,
}

/// Harvest `defineEmits` declared event names, abstain flags, and per-event
/// `used` status from a `<script setup>` program. The byte spans returned are
/// RELATIVE to the script body; the caller remaps them onto the SFC source.
///
/// Three declaration forms are harvested:
/// 1. Type tuple-call: `defineEmits<{ (e: 'foo'): void; (e: 'bar', n: number): void }>()`.
/// 2. Type object (Vue 3.3+): `defineEmits<{ foo: [x: string]; bar: [] }>()`.
/// 3. Runtime array: `defineEmits(['foo', 'bar'])`.
///
/// A type-reference type argument or any non-literal form sets
/// `has_unharvestable_emits` (abstain). The `defineEmits` return MUST be bound to
/// a name (`const emit = defineEmits(...)`) for usage to be trackable; a bare
/// unbound `defineEmits([...])` sets `has_unharvestable_emits` (the component
/// cannot emit, usage is untrackable, so abstain).
pub fn harvest_define_emits(program: &Program<'_>) -> DefineEmitsHarvest {
    let mut harvest = DefineEmitsHarvest::default();

    let mut emit_return_binding: Option<String> = None;
    let mut emit_names: Vec<(String, u32)> = Vec::new();

    for stmt in &program.body {
        match stmt {
            Statement::VariableDeclaration(decl) => {
                for declarator in &decl.declarations {
                    let Some(init) = &declarator.init else {
                        continue;
                    };
                    let Some(call) = unwrap_define_emits_call(init) else {
                        continue;
                    };
                    if emit_names.is_empty() && !harvest.has_unharvestable_emits {
                        collect_define_emits_names(call, &mut emit_names, &mut harvest);
                    }
                    // The return must bind to a plain identifier to be trackable.
                    if let BindingPattern::BindingIdentifier(ident) = &declarator.id {
                        emit_return_binding = Some(ident.name.to_string());
                    } else {
                        // A destructured / non-identifier binding hides the emit
                        // function name: usage untrackable, abstain.
                        harvest.has_unharvestable_emits = true;
                    }
                }
            }
            Statement::ExpressionStatement(expr_stmt) => {
                // Bare `defineEmits(...)` with no binding: the component cannot
                // emit through a name we can track. Abstain.
                if let Some(call) = unwrap_define_emits_call(&expr_stmt.expression) {
                    if emit_names.is_empty() && !harvest.has_unharvestable_emits {
                        collect_define_emits_names(call, &mut emit_names, &mut harvest);
                    }
                    harvest.has_unharvestable_emits = true;
                }
            }
            _ => {}
        }
    }

    if emit_names.is_empty() {
        return harvest;
    }

    // Without a bound emit name, every declared event is untrackable. Abstain.
    let Some(binding) = emit_return_binding else {
        harvest.has_unharvestable_emits = true;
        return harvest;
    };

    let mut visitor = EmitBindingVisitor {
        binding: &binding,
        emitted: FxHashSet::default(),
        has_dynamic_emit: false,
        used_whole: false,
    };
    oxc_ast_visit::Visit::visit_program(&mut visitor, program);
    if visitor.has_dynamic_emit {
        harvest.has_dynamic_emit = true;
    }
    if visitor.used_whole {
        harvest.has_emit_whole_object_use = true;
    }

    for (name, span_start) in emit_names {
        let used = visitor.emitted.contains(&name);
        harvest.emits.push(ComponentEmit {
            name,
            span_start,
            used,
        });
    }

    harvest.emit_binding = Some(binding);
    harvest
}

/// Unwrap an expression to the inner `defineEmits(...)` call. Returns `None` for
/// anything else.
fn unwrap_define_emits_call<'a, 'b>(expr: &'b Expression<'a>) -> Option<&'b CallExpression<'a>> {
    let Expression::CallExpression(call) = expr else {
        return None;
    };
    let callee_name = simple_callee_name(&call.callee)?;
    if callee_name == "defineEmits" {
        return Some(call);
    }
    None
}

/// Collect emit event names from a `defineEmits(...)` call: the type tuple-call
/// signatures, the type object-literal property names, or the runtime
/// string-literal array elements. A type reference or non-literal form sets
/// `has_unharvestable_emits` and harvests nothing.
fn collect_define_emits_names(
    call: &CallExpression<'_>,
    emit_names: &mut Vec<(String, u32)>,
    harvest: &mut DefineEmitsHarvest,
) {
    // Inline TS form: `defineEmits<{ ... }>()`.
    if let Some(type_args) = &call.type_arguments {
        if let Some(first) = type_args.params.first() {
            match first {
                TSType::TSTypeLiteral(lit) => {
                    for member in &lit.members {
                        match member {
                            // Tuple-call form: `(e: 'foo'): void`. The first
                            // parameter's string-literal type is the event name.
                            TSSignature::TSCallSignatureDeclaration(sig) => {
                                if let Some((name, span_start)) = call_signature_event_name(sig) {
                                    emit_names.push((name, span_start));
                                } else {
                                    harvest.has_unharvestable_emits = true;
                                }
                            }
                            // Object form (Vue 3.3+): `foo: [x: string]`. The
                            // property name is the event name.
                            TSSignature::TSPropertySignature(sig) => {
                                if let Some(name) = property_key_name(&sig.key) {
                                    emit_names.push((name, sig.span.start));
                                }
                            }
                            _ => harvest.has_unharvestable_emits = true,
                        }
                    }
                }
                // A type reference (`defineEmits<MyEmits>()`) or any non-literal
                // type argument: names require cross-file resolution. Abstain.
                _ => harvest.has_unharvestable_emits = true,
            }
        }
        return;
    }

    // Runtime array form: `defineEmits(['foo', 'bar'])`.
    if let Some(first) = call.arguments.first().and_then(|arg| arg.as_expression()) {
        match first {
            Expression::ArrayExpression(arr) => {
                for element in &arr.elements {
                    if let ArrayExpressionElement::StringLiteral(lit) = element {
                        emit_names.push((lit.value.to_string(), lit.span.start));
                    } else if !matches!(element, ArrayExpressionElement::Elision(_)) {
                        harvest.has_unharvestable_emits = true;
                    }
                }
            }
            // A non-array runtime argument (an object validator, an identifier,
            // a call): unharvestable in v1. Abstain.
            _ => harvest.has_unharvestable_emits = true,
        }
    }
}

/// The first parameter's string-literal type from a `TSCallSignatureDeclaration`
/// member (`(e: 'foo'): void`), with the signature's start as the span anchor.
fn call_signature_event_name(sig: &TSCallSignatureDeclaration<'_>) -> Option<(String, u32)> {
    let first = sig.params.items.first()?;
    let type_annotation = first.type_annotation.as_deref()?;
    if let TSType::TSLiteralType(lit) = &type_annotation.type_annotation
        && let TSLiteral::StringLiteral(str_lit) = &lit.literal
    {
        return Some((str_lit.value.to_string(), sig.span.start));
    }
    None
}

/// Inspect every use of the `defineEmits` return binding: collect the event names
/// emitted via `<binding>('<name>')`, report a dynamic `<binding>(<nonLiteral>)`
/// emit (event unknowable), and report whether the binding is ever used as a
/// WHOLE value (passed / returned / spread), all of which force a whole-file
/// abstain.
struct EmitBindingVisitor<'a> {
    binding: &'a str,
    emitted: FxHashSet<String>,
    has_dynamic_emit: bool,
    used_whole: bool,
}

impl<'a> oxc_ast_visit::Visit<'a> for EmitBindingVisitor<'a> {
    fn visit_call_expression(&mut self, call: &CallExpression<'a>) {
        // `emit('event')` / `emit('event', payload)`: the bound emit name called
        // with a string-literal first argument credits that event as used.
        if let Expression::Identifier(ident) = &call.callee
            && ident.name.as_str() == self.binding
        {
            match call.arguments.first().and_then(|arg| arg.as_expression()) {
                Some(Expression::StringLiteral(lit)) => {
                    self.emitted.insert(lit.value.to_string());
                }
                // `emit(someVar)` / `emit(\`x\`)` / `emit()`: the event cannot be
                // known statically. Abstain on the whole file.
                _ => self.has_dynamic_emit = true,
            }
            // Walk the ARGUMENTS (a payload may use the binding elsewhere) but
            // not re-classify the callee identifier as a whole-object use.
            for arg in &call.arguments {
                if let Some(expr) = arg.as_expression() {
                    self.visit_expression(expr);
                }
            }
            return;
        }
        oxc_ast_visit::walk::walk_call_expression(self, call);
    }

    fn visit_identifier_reference(&mut self, ident: &IdentifierReference<'a>) {
        // Any bare reference to the emit binding that is NOT the callee of an
        // `emit(...)` call (short-circuited above) is a whole-value use: the
        // emit function flowed somewhere opaque. Abstain.
        if ident.name.as_str() == self.binding {
            self.used_whole = true;
        }
    }
}
