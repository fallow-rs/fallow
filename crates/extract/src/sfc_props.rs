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

/// Harvest Svelte 5 `$props()` declared props and abstain flags from a parsed
/// instance `<script>` program. The Svelte 5 analogue of [`harvest_define_props`]:
/// it reuses the same [`ComponentProp`] IR and the same abstain-flag fields on
/// [`DefinePropsHarvest`] (`has_unharvestable_props`, `has_props_attrs_fallthrough`)
/// so NO new `ModuleInfo` field is needed.
///
/// There is exactly one declaration form to harvest: a variable declarator whose
/// `init` is a `CallExpression` with callee identifier `$props`. The destructure
/// target is handled like `bind_define_props_target`:
/// - object pattern: each property is a declared prop; renames map name -> local;
///   defaults (`{ a = 1 }`, `{ a = $bindable() }`) peel via [`binding_local_name`].
/// - a rest element (`{ a, ...rest }`) sets `has_props_attrs_fallthrough` (abstain).
/// - a bare identifier binding (`let p = $props()`) sets `has_unharvestable_props`
///   (every prop is reached opaquely through `p.x`).
/// - a nested object/array destructure (`{ a: { x } }`) returns `None` from
///   `binding_local_name`, so it sets `has_unharvestable_props` (abstain).
///
/// `used_in_script` is computed via [`resolve_used_locals`], reused verbatim.
/// `used_in_template` is left `false` and set in `sfc.rs::apply_template_usage`.
/// Byte spans are RELATIVE to the script body; the caller remaps them onto the
/// SFC source.
pub fn harvest_svelte_props(program: &Program<'_>) -> DefinePropsHarvest {
    let mut harvest = DefinePropsHarvest::default();

    let mut destructured_locals: FxHashSet<String> = FxHashSet::default();
    // declared prop name -> local binding name (for `{ a: alias }`).
    let mut prop_aliases: FxHashMap<String, String> = FxHashMap::default();
    let mut prop_names: Vec<(String, u32)> = Vec::new();

    for stmt in &program.body {
        let Statement::VariableDeclaration(decl) = stmt else {
            continue;
        };
        for declarator in &decl.declarations {
            let Some(init) = &declarator.init else {
                continue;
            };
            if !is_props_rune_call(init) {
                continue;
            }
            bind_svelte_props_target(
                &declarator.id,
                &mut destructured_locals,
                &mut prop_aliases,
                &mut prop_names,
                &mut harvest,
            );
        }
    }

    if prop_names.is_empty() {
        return harvest;
    }

    let used_locals = resolve_used_locals(program, &destructured_locals);

    for (name, span_start) in prop_names {
        let local = prop_aliases
            .get(&name)
            .cloned()
            .unwrap_or_else(|| name.clone());
        let used_in_script = used_locals.contains(&local);
        harvest.props.push(ComponentProp {
            name,
            local,
            span_start,
            used_in_script,
            used_in_template: false,
            // Svelte: one component per `.svelte` file; the detector (a future
            // consumer) derives the component name from the file stem, so this
            // stays empty, matching the Vue harvest.
            component: String::new(),
            // React-only forward-vs-consume signal; Svelte does not compute it.
            used_outside_forward: false,
        });
    }

    harvest
}

/// Whether an expression is a bare `$props()` rune call (callee is the identifier
/// `$props`). The Svelte compiler treats `$props` as a reserved rune, so a
/// same-named local function is not a real concern, but matching the bare
/// identifier callee keeps the check tight regardless.
fn is_props_rune_call(expr: &Expression<'_>) -> bool {
    let Expression::CallExpression(call) = expr else {
        return false;
    };
    simple_callee_name(&call.callee) == Some("$props")
}

/// Bind the `$props()` destructure target. Mirrors [`bind_define_props_target`]
/// for the destructure form, but a bare identifier binding (`let p = $props()`)
/// is the WHOLE-OBJECT abstain shape for Svelte (every prop reached via `p.x`),
/// so it sets `has_unharvestable_props` rather than tracking member access.
fn bind_svelte_props_target(
    id: &BindingPattern<'_>,
    destructured_locals: &mut FxHashSet<String>,
    prop_aliases: &mut FxHashMap<String, String>,
    prop_names: &mut Vec<(String, u32)>,
    harvest: &mut DefinePropsHarvest,
) {
    match id {
        // `let p = $props()`: every prop reached opaquely through `p.x`. Abstain.
        BindingPattern::BindingIdentifier(_) => {
            harvest.has_unharvestable_props = true;
        }
        BindingPattern::ObjectPattern(pattern) => {
            for prop in &pattern.properties {
                if let Some(local) = binding_local_name(&prop.value) {
                    destructured_locals.insert(local.to_string());
                    if let Some(prop_name) = property_key_name(&prop.key) {
                        prop_names.push((prop_name.clone(), prop.span.start));
                        prop_aliases.insert(prop_name, local.to_string());
                    } else {
                        // A computed key (`{ [k]: v }`) hides the declared name.
                        harvest.has_unharvestable_props = true;
                    }
                } else {
                    // A nested object/array destructure (`{ a: { x } }`):
                    // `binding_local_name` is `None` for non-flat patterns. The
                    // declared prop name is unenumerable in flat form. Abstain.
                    harvest.has_unharvestable_props = true;
                }
            }
            // A rest element (`{ a, ...rest }`) carries arbitrary props opaquely.
            if pattern.rest.is_some() {
                harvest.has_props_attrs_fallthrough = true;
            }
        }
        // Any other binding shape (an array pattern, an assignment pattern at the
        // top level): unenumerable. Abstain.
        _ => harvest.has_unharvestable_props = true,
    }
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

// -- Vue Options API (`export default { props, emits, ... }` and
// `export default defineComponent({ props, emits, ... })`) --------------------
//
// The setup harvest above reads `defineProps` / `defineEmits` macros from a
// `<script setup>` block. The Options API instead declares the same contract as
// keys on the component-options object: `props` / `emits` declare the names,
// `this.<prop>` reads them, and `this.$emit('<name>')` fires events. The harvest
// here finds that options object (a default-export object literal, or the first
// argument of `defineComponent(...)`), reuses the same `ComponentProp` /
// `ComponentEmit` IR and abstain-flag structs the setup versions return, and
// computes per-prop / per-emit usage from a `this.*` walk over the whole script.
//
// Whole-component abstains (set the existing flags so the detector skips the
// file): `mixins: [...]` and an Options-API `extends:` key (a mixin / base may
// read a prop or fire an emit invisibly to the per-component scan), a dynamic
// `this[<computed>]` access, an unharvestable `props` / `emits` value (an
// identifier, a spread, or a `defineComponent<Type>()` type generic with no
// runtime object), and a dynamic `this.$emit(<nonLiteral>)`.

/// Locate the Vue Options API component-options object in a non-setup `<script>`
/// program: the `export default { ... }` object literal, or the first-argument
/// object of `export default defineComponent({ ... })`. A
/// `defineComponent<Type>()` type-generic form (no runtime object argument) is
/// reported via `has_type_generic` so the caller can abstain. Returns `None`
/// when no options object is present (a non-component script).
fn find_options_object<'a, 'b>(
    program: &'b Program<'a>,
    has_type_generic: &mut bool,
) -> Option<&'b ObjectExpression<'a>> {
    for stmt in &program.body {
        let Statement::ExportDefaultDeclaration(export) = stmt else {
            continue;
        };
        let Some(expr) = export.declaration.as_expression() else {
            continue;
        };
        match expr {
            // `export default { ... }`.
            Expression::ObjectExpression(obj) => return Some(obj),
            // `export default defineComponent({ ... })` / `defineComponent<T>()`.
            Expression::CallExpression(call) => {
                if simple_callee_name(&call.callee) != Some("defineComponent") {
                    return None;
                }
                // `defineComponent<Props>()`: the runtime object is absent and the
                // prop names live in a type the per-file scan cannot resolve.
                if call.type_arguments.is_some()
                    && !call
                        .arguments
                        .first()
                        .and_then(|arg| arg.as_expression())
                        .is_some_and(|e| matches!(e, Expression::ObjectExpression(_)))
                {
                    *has_type_generic = true;
                    return None;
                }
                if let Some(Expression::ObjectExpression(obj)) =
                    call.arguments.first().and_then(|arg| arg.as_expression())
                {
                    return Some(obj);
                }
                return None;
            }
            _ => return None,
        }
    }
    None
}

/// The value expression of an options-object property whose static key matches
/// `key`. Returns `None` for a spread, a computed key, or an absent key.
fn options_property_value<'a, 'b>(
    obj: &'b ObjectExpression<'a>,
    key: &str,
) -> Option<&'b Expression<'a>> {
    for prop in &obj.properties {
        if let ObjectPropertyKind::ObjectProperty(p) = prop
            && property_key_name(&p.key).as_deref() == Some(key)
        {
            return Some(&p.value);
        }
    }
    None
}

/// Whether the options object carries a `mixins:` or `extends:` key. Either one
/// can read a prop or fire an emit from another file, invisible to the
/// per-component scan, so the whole component abstains. The `extends:` here is
/// an Options-API component-options KEY, not a JS `class X extends Y` clause.
fn options_has_mixin_or_extends(obj: &ObjectExpression<'_>) -> bool {
    obj.properties.iter().any(|prop| {
        matches!(
            prop,
            ObjectPropertyKind::ObjectProperty(p)
                if matches!(property_key_name(&p.key).as_deref(), Some("mixins" | "extends"))
        )
    })
}

/// Whether the options object declares a `setup(...)` method (or `setup:` value
/// property). A `setup` receives the props object as its first parameter and can
/// read any prop opaquely, so the caller credits a whole-object props use.
fn options_has_setup_method(obj: &ObjectExpression<'_>) -> bool {
    obj.properties.iter().any(|prop| match prop {
        ObjectPropertyKind::ObjectProperty(p) => {
            property_key_name(&p.key).as_deref() == Some("setup")
        }
        ObjectPropertyKind::SpreadProperty(_) => false,
    })
}

/// Harvest Options-API declared props and abstain flags from a non-setup
/// `<script>` program. Reuses [`DefinePropsHarvest`]: `props` carries the
/// declared names with `used_in_script` set from a `this.<prop>` read walk;
/// `has_unharvestable_props` abstains the whole file. Byte spans are RELATIVE to
/// the script body; the caller remaps them onto the SFC source.
pub fn harvest_options_api_props(program: &Program<'_>) -> DefinePropsHarvest {
    let mut harvest = DefinePropsHarvest::default();

    let mut has_type_generic = false;
    let Some(obj) = find_options_object(program, &mut has_type_generic) else {
        if has_type_generic {
            harvest.has_unharvestable_props = true;
        }
        return harvest;
    };

    // A mixin / base component is an opaque additional source of prop reads.
    if options_has_mixin_or_extends(obj) {
        harvest.has_unharvestable_props = true;
    }

    // A `setup(props)` method receives the whole props object as its first
    // parameter and can consume any prop opaquely; credit conservatively as a
    // whole-object props use (the script-side analog of `v-bind="props"`) rather
    // than risk a false positive. Reuses the existing fallthrough abstain.
    if options_has_setup_method(obj) {
        harvest.has_props_attrs_fallthrough = true;
    }

    let mut prop_names: Vec<(String, u32)> = Vec::new();
    if let Some(props_value) = options_property_value(obj, "props") {
        collect_options_prop_names(props_value, &mut prop_names, &mut harvest);
    }

    if prop_names.is_empty() {
        return harvest;
    }

    // `this.foo` reads (and a dynamic `this[<computed>]` whole-component abstain)
    // across the entire script: methods, computed, watch, lifecycle hooks, and a
    // `setup()` body that reads its `props` param is handled separately by the
    // caller (whole-object props use).
    let usage = collect_this_member_usage(program);
    if usage.has_dynamic_this {
        harvest.has_props_attrs_fallthrough = true;
    }

    for (name, span_start) in prop_names {
        let used_in_script = usage.read.contains(&name);
        harvest.props.push(ComponentProp {
            name: name.clone(),
            // Options-API props have no destructure local; the declared name is
            // also the template-credit name, mirroring the setup non-destructure
            // form. Default the local to the prop name.
            local: name,
            span_start,
            used_in_script,
            used_in_template: false,
            // Vue: one component per `.vue` file; the detector derives the name
            // from the file stem, so this stays empty.
            component: String::new(),
            // React-only forward-vs-consume signal; Vue does not compute it.
            used_outside_forward: false,
        });
    }

    harvest
}

/// Collect prop names from the `props:` value of an Options-API component. The
/// array form (`props: ['foo', 'bar']`) credits each string-literal element; the
/// object form (`props: { foo: {...}, bar: Number }`) credits each static object
/// key. An identifier (`props: sharedProps`), a spread, a non-string array
/// element, or any other shape sets `has_unharvestable_props` (abstain).
fn collect_options_prop_names(
    value: &Expression<'_>,
    prop_names: &mut Vec<(String, u32)>,
    harvest: &mut DefinePropsHarvest,
) {
    match value {
        // `props: { foo: { type: String }, bar: Number }`.
        Expression::ObjectExpression(obj) => {
            for prop in &obj.properties {
                match prop {
                    ObjectPropertyKind::ObjectProperty(p) => {
                        if let Some(name) = property_key_name(&p.key) {
                            // Anchor on the property span, matching the setup
                            // object form (`p.span.start`).
                            prop_names.push((name, p.span.start));
                        } else {
                            // A computed key (`[dynamic]: {...}`) hides the name.
                            harvest.has_unharvestable_props = true;
                        }
                    }
                    // `{ ...sharedProps }` hides names: abstain.
                    ObjectPropertyKind::SpreadProperty(_) => {
                        harvest.has_unharvestable_props = true;
                    }
                }
            }
        }
        // `props: ['foo', 'bar']`.
        Expression::ArrayExpression(arr) => {
            for element in &arr.elements {
                if let ArrayExpressionElement::StringLiteral(lit) = element {
                    prop_names.push((lit.value.to_string(), lit.span.start));
                } else if !matches!(element, ArrayExpressionElement::Elision(_)) {
                    harvest.has_unharvestable_props = true;
                }
            }
        }
        // `props: sharedProps` (an identifier) or any other shape: unharvestable.
        _ => harvest.has_unharvestable_props = true,
    }
}

/// Harvest Options-API declared emit events and abstain flags from a non-setup
/// `<script>` program. Reuses [`DefineEmitsHarvest`]: each event's `used` flag is
/// set from a `this.$emit('<name>')` script call; a `this.$emit(<nonLiteral>)`
/// sets `has_dynamic_emit`. Byte spans are RELATIVE to the script body; the
/// caller remaps them onto the SFC source.
pub fn harvest_options_api_emits(program: &Program<'_>) -> DefineEmitsHarvest {
    let mut harvest = DefineEmitsHarvest::default();

    let mut has_type_generic = false;
    let Some(obj) = find_options_object(program, &mut has_type_generic) else {
        if has_type_generic {
            harvest.has_unharvestable_emits = true;
        }
        return harvest;
    };

    // A mixin / base component may fire an emit invisibly to the scan.
    if options_has_mixin_or_extends(obj) {
        harvest.has_unharvestable_emits = true;
    }

    // A `setup(props, { emit })` method can fire bare `emit('name')` calls
    // through the context binding, which the `this.$emit` walk cannot see.
    // Abstain the whole component's emit findings (mirrors the props side,
    // which sets has_props_attrs_fallthrough for the same reason).
    if options_has_setup_method(obj) {
        harvest.has_dynamic_emit = true;
    }

    let mut emit_names: Vec<(String, u32)> = Vec::new();
    if let Some(emits_value) = options_property_value(obj, "emits") {
        collect_options_emit_names(emits_value, &mut emit_names, &mut harvest);
    }

    if emit_names.is_empty() {
        return harvest;
    }

    let usage = collect_this_member_usage(program);
    if usage.has_dynamic_emit {
        harvest.has_dynamic_emit = true;
    }

    for (name, span_start) in emit_names {
        let used = usage.emitted.contains(&name);
        harvest.emits.push(ComponentEmit {
            name,
            span_start,
            used,
        });
    }

    harvest
}

/// Collect emit event names from the `emits:` value of an Options-API component.
/// The array form (`emits: ['save']`) credits each string-literal element; the
/// object form (`emits: { save: payload => true }`) credits each static object
/// key. An identifier, a spread, a non-string array element, or any other shape
/// sets `has_unharvestable_emits` (abstain).
fn collect_options_emit_names(
    value: &Expression<'_>,
    emit_names: &mut Vec<(String, u32)>,
    harvest: &mut DefineEmitsHarvest,
) {
    match value {
        Expression::ObjectExpression(obj) => {
            for prop in &obj.properties {
                match prop {
                    ObjectPropertyKind::ObjectProperty(p) => {
                        if let Some(name) = property_key_name(&p.key) {
                            emit_names.push((name, p.span.start));
                        } else {
                            harvest.has_unharvestable_emits = true;
                        }
                    }
                    ObjectPropertyKind::SpreadProperty(_) => {
                        harvest.has_unharvestable_emits = true;
                    }
                }
            }
        }
        Expression::ArrayExpression(arr) => {
            for element in &arr.elements {
                if let ArrayExpressionElement::StringLiteral(lit) = element {
                    emit_names.push((lit.value.to_string(), lit.span.start));
                } else if !matches!(element, ArrayExpressionElement::Elision(_)) {
                    harvest.has_unharvestable_emits = true;
                }
            }
        }
        _ => harvest.has_unharvestable_emits = true,
    }
}

/// Result of walking a non-setup `<script>` program for `this.*` usage shared by
/// the Options-API prop and emit harvests.
#[derive(Debug, Default)]
struct ThisMemberUsage {
    /// Prop names read via `this.<name>` (any static-member read).
    read: FxHashSet<String>,
    /// Emit event names fired via `this.$emit('<name>')` (string-literal arg).
    emitted: FxHashSet<String>,
    /// A `this[<computed>]` dynamic member access was seen: a prop could be read
    /// opaquely, so the whole component abstains its prop findings.
    has_dynamic_this: bool,
    /// A `this.$emit(<nonLiteral>)` was seen: the event is unknowable, so the
    /// whole component abstains its emit findings.
    has_dynamic_emit: bool,
}

/// Walk every `this.*` access in the program. `this.<name>` (static member)
/// credits a prop read; `this[<computed>]` sets the dynamic-this abstain; a
/// `this.$emit('<name>')` call credits an emit, while `this.$emit(<nonLiteral>)`
/// sets the dynamic-emit abstain.
fn collect_this_member_usage(program: &Program<'_>) -> ThisMemberUsage {
    let mut visitor = ThisMemberVisitor {
        usage: ThisMemberUsage::default(),
    };
    oxc_ast_visit::Visit::visit_program(&mut visitor, program);
    visitor.usage
}

struct ThisMemberVisitor {
    usage: ThisMemberUsage,
}

impl<'a> oxc_ast_visit::Visit<'a> for ThisMemberVisitor {
    fn visit_call_expression(&mut self, call: &CallExpression<'a>) {
        // `this.$emit('event')` / `this.$emit('event', payload)`: a string-literal
        // first arg credits that event; a non-literal first arg is a dynamic emit.
        if let Expression::StaticMemberExpression(member) = &call.callee
            && matches!(member.object, Expression::ThisExpression(_))
            && member.property.name.as_str() == "$emit"
        {
            match call.arguments.first().and_then(|arg| arg.as_expression()) {
                Some(Expression::StringLiteral(lit)) => {
                    self.usage.emitted.insert(lit.value.to_string());
                }
                // `this.$emit(someVar)` / `this.$emit()`: event unknowable.
                _ => self.usage.has_dynamic_emit = true,
            }
            // Walk the arguments (a payload may read a prop via `this.<name>`).
            for arg in &call.arguments {
                if let Some(expr) = arg.as_expression() {
                    self.visit_expression(expr);
                }
            }
            return;
        }
        oxc_ast_visit::walk::walk_call_expression(self, call);
    }

    fn visit_static_member_expression(&mut self, member: &StaticMemberExpression<'a>) {
        // `this.foo`: credit a prop read. `this.$emit` member handled at the call
        // site above; record `$`-prefixed instance API reads too (harmless, no
        // prop is named with a leading `$`).
        if matches!(member.object, Expression::ThisExpression(_)) {
            self.usage.read.insert(member.property.name.to_string());
        }
        oxc_ast_visit::walk::walk_static_member_expression(self, member);
    }

    fn visit_computed_member_expression(&mut self, member: &ComputedMemberExpression<'a>) {
        // `this[<computed>]`: a prop could be read by a name we cannot resolve.
        if matches!(member.object, Expression::ThisExpression(_)) {
            self.usage.has_dynamic_this = true;
        }
        oxc_ast_visit::walk::walk_computed_member_expression(self, member);
    }
}
