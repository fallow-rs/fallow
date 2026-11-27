//! VueUse model refs in a Vue `<script setup>` block.
//!
//! `useVModel(props, key)` from `@vueuse/core` returns a ref whose setter emits
//! `update:<key>` through the component instance, also when the call passes no
//! `emit` function. `useVModels(props)` returns one such ref for each prop. A
//! component that declares `update:<key>` and writes the ref therefore emits
//! the event without an `emit('update:<key>')` call.
//!
//! The harvest credits an event only for a write that runs the setter:
//! - `ref.value = x`, `ref.value += x`, `ref.value++`, or a destructuring
//!   assignment with `ref.value` as a target;
//! - a use of the whole ref (`fn(ref)`, `return ref`, `{ ref }`), because
//!   the receiver can write it;
//! - in the template, `v-model="ref"` or an assignment `ref = x` (the
//!   template unwraps the ref, so the assignment runs the setter).
//!
//! A write to a nested field (`ref.value.field = x`, `v-model="ref.value"` in
//! the template) changes the object in place. The setter does not run, so the
//! event stays unused.

use oxc_ast::ast::*;
use oxc_ast_visit::Visit;
use oxc_semantic::{Scoping, SemanticBuilder, SymbolId};
use rustc_hash::{FxHashMap, FxHashSet};

use crate::js_text::JsText;

const USE_VMODEL: &str = "useVModel";
const USE_VMODELS: &str = "useVModels";
/// The prop that `useVModel(props)` binds when the call gives no key.
const DEFAULT_MODEL_KEY: &str = "modelValue";
const UPDATE_EVENT_PREFIX: &str = "update:";
/// The package scope of VueUse. `@vueuse/core` and `@vueuse/components`
/// export the composables, and `@vueuse/nuxt` imports them automatically.
const VUEUSE_SOURCE_PREFIX: &str = "@vueuse/";

/// The model refs of one script and the events that their writes emit.
#[derive(Debug, Default)]
pub struct ModelRefHarvest {
    /// Events that a write of a model ref in the script emits.
    pub written: FxHashSet<String>,
    /// Each model ref binding with the events that its setter emits. The
    /// template scan credits these events when the template writes the ref.
    pub refs: Vec<(String, Vec<String>)>,
    /// A model ref has an event name that the scan cannot read.
    pub dynamic: bool,
}

/// Harvest the model refs of the top-level declarations of `program`.
/// `declared_events` are the `defineEmits` event names. A whole
/// `useVModels` result can emit each declared `update:*` event.
///
/// The callee must be the VueUse composable: an import from `@vueuse/*`
/// (also under another local name), or a free name that an auto-import
/// supplies. A local function with the same name does not count.
pub fn harvest_model_refs<'n>(
    program: &Program<'_>,
    declared_events: impl Iterator<Item = &'n str>,
) -> ModelRefHarvest {
    let update_events: Vec<String> = declared_events
        .filter(|name| name.starts_with(UPDATE_EVENT_PREFIX))
        .map(str::to_string)
        .collect();
    let mut harvest = ModelRefHarvest::default();
    if update_events.is_empty() {
        return harvest;
    }

    let vueuse_imports = vueuse_imports(program);
    let calls: Vec<(&BindingPattern<'_>, &CallExpression<'_>)> = program
        .body
        .iter()
        .filter_map(|stmt| match stmt {
            Statement::VariableDeclaration(decl) => Some(decl),
            _ => None,
        })
        .flat_map(|decl| decl.declarations.iter())
        .filter_map(|declarator| match &declarator.init {
            Some(Expression::CallExpression(call))
                if callee_name(call).is_some_and(|name| {
                    vueuse_imports.contains_key(name) || is_model_composable(name)
                }) =>
            {
                Some((&declarator.id, &**call))
            }
            _ => None,
        })
        .collect();
    if calls.is_empty() {
        return harvest;
    }

    let semantic = SemanticBuilder::new().build(program).semantic;
    let scoping = semantic.scoping();
    let mut refs = ModelRefs::default();
    for (id, call) in calls {
        let Some(composable) = resolve_composable(call, &vueuse_imports, scoping) else {
            continue;
        };
        match composable {
            USE_VMODEL => collect_use_vmodel(id, call, &mut refs, &mut harvest),
            _ => collect_use_vmodels(id, call, &update_events, &mut refs, &mut harvest),
        }
    }
    if refs.events.is_empty() {
        return harvest;
    }

    let mut writes = ModelRefWriteVisitor {
        refs: &refs,
        scoping,
        written: FxHashSet::default(),
    };
    writes.visit_program(program);
    for symbol in writes.written {
        if let Some(events) = refs.events.get(&symbol) {
            harvest.written.extend(events.iter().cloned());
        }
    }
    harvest.refs = refs
        .events
        .into_iter()
        .filter_map(|(symbol, events)| Some((refs.names.get(&symbol)?.clone(), events)))
        .collect();
    harvest.refs.sort();
    harvest
}

/// The model ref symbols with their names and the events that their setters
/// emit.
#[derive(Default)]
struct ModelRefs {
    events: FxHashMap<SymbolId, Vec<String>>,
    names: FxHashMap<SymbolId, String>,
}

impl ModelRefs {
    fn insert(&mut self, ident: &BindingIdentifier<'_>, events: Vec<String>) {
        let Some(symbol) = ident.symbol_id.get() else {
            return;
        };
        self.events.insert(symbol, events);
        self.names.insert(symbol, ident.name.to_string());
    }
}

fn is_model_composable(name: &str) -> bool {
    name == USE_VMODEL || name == USE_VMODELS
}

fn callee_name<'c>(call: &'c CallExpression<'_>) -> Option<&'c str> {
    match &call.callee {
        Expression::Identifier(ident) => Some(ident.name.as_str()),
        _ => None,
    }
}

/// The local names of the model composables that the program imports from
/// VueUse, mapped to the composable name.
fn vueuse_imports<'p>(program: &'p Program<'_>) -> FxHashMap<&'p str, &'static str> {
    let mut imports = FxHashMap::default();
    for stmt in &program.body {
        let Statement::ImportDeclaration(import) = stmt else {
            continue;
        };
        if !import.source.value.starts_with(VUEUSE_SOURCE_PREFIX) {
            continue;
        }
        for specifier in import.specifiers.iter().flatten() {
            let ImportDeclarationSpecifier::ImportSpecifier(specifier) = specifier else {
                continue;
            };
            let composable = match specifier.imported.name().as_str() {
                USE_VMODEL => USE_VMODEL,
                USE_VMODELS => USE_VMODELS,
                _ => continue,
            };
            imports.insert(specifier.local.name.as_str(), composable);
        }
    }
    imports
}

/// The VueUse composable that `call` calls: an import from VueUse, or a model
/// composable name with no binding in the program (an auto-import).
fn resolve_composable(
    call: &CallExpression<'_>,
    vueuse_imports: &FxHashMap<&str, &'static str>,
    scoping: &Scoping,
) -> Option<&'static str> {
    let Expression::Identifier(callee) = &call.callee else {
        return None;
    };
    if let Some(composable) = vueuse_imports.get(callee.name.as_str()) {
        return Some(composable);
    }
    let unresolved = callee
        .reference_id
        .get()
        .is_none_or(|reference| scoping.get_reference(reference).symbol_id().is_none());
    match callee.name.as_str() {
        USE_VMODEL if unresolved => Some(USE_VMODEL),
        USE_VMODELS if unresolved => Some(USE_VMODELS),
        _ => None,
    }
}

/// The event that a model ref emits, or `None` when the scan cannot read it.
enum ModelEvent {
    Known(String),
    Dynamic,
}

/// `const ref = useVModel(props, key, emit, options)`.
fn collect_use_vmodel(
    id: &BindingPattern<'_>,
    call: &CallExpression<'_>,
    refs: &mut ModelRefs,
    harvest: &mut ModelRefHarvest,
) {
    let options = model_options(call.arguments.get(3));
    let event = match options.event_name {
        Some(event) => event,
        None => match call.arguments.get(1) {
            None => ModelEvent::Known(format!("{UPDATE_EVENT_PREFIX}{DEFAULT_MODEL_KEY}")),
            Some(arg) => match argument_string(arg) {
                Some(key) => ModelEvent::Known(format!("{UPDATE_EVENT_PREFIX}{key}")),
                None => ModelEvent::Dynamic,
            },
        },
    };
    let ModelEvent::Known(event) = event else {
        harvest.dynamic = true;
        return;
    };
    match id {
        // A `deep` ref emits on a nested change too, so any use can emit.
        BindingPattern::BindingIdentifier(ident) if !options.deep => {
            refs.insert(ident, vec![event]);
        }
        _ => {
            harvest.written.insert(event);
        }
    }
}

/// `const { open, step: page } = useVModels(props, emit, options)`.
fn collect_use_vmodels(
    id: &BindingPattern<'_>,
    call: &CallExpression<'_>,
    update_events: &[String],
    refs: &mut ModelRefs,
    harvest: &mut ModelRefHarvest,
) {
    let options = model_options(call.arguments.get(2));
    if options.event_name.is_some() {
        // One event name for every prop: the scan does not map it per ref.
        harvest.dynamic = true;
        return;
    }
    let BindingPattern::ObjectPattern(pattern) = id else {
        harvest.written.extend(update_events.iter().cloned());
        return;
    };
    if options.deep || pattern.rest.is_some() {
        harvest.written.extend(update_events.iter().cloned());
        return;
    }
    for property in &pattern.properties {
        let key = property
            .key
            .static_name()
            .map(|name| name.text().into_owned());
        let local = match &property.value {
            BindingPattern::BindingIdentifier(ident) => Some(ident),
            BindingPattern::AssignmentPattern(assign) => match &assign.left {
                BindingPattern::BindingIdentifier(ident) => Some(ident),
                _ => None,
            },
            _ => None,
        };
        match (key, local) {
            (Some(key), Some(local)) => {
                refs.insert(local, vec![format!("{UPDATE_EVENT_PREFIX}{key}")]);
            }
            _ => harvest.written.extend(update_events.iter().cloned()),
        }
    }
}

/// The options of a `useVModel` call that change the emitted event.
struct ModelOptions {
    /// `eventName` replaces `update:<key>`.
    event_name: Option<ModelEvent>,
    /// `deep` emits on a nested change of the ref value.
    deep: bool,
}

fn model_options(arg: Option<&Argument<'_>>) -> ModelOptions {
    let mut options = ModelOptions {
        event_name: None,
        deep: false,
    };
    let Some(arg) = arg else {
        return options;
    };
    let Argument::ObjectExpression(object) = arg else {
        options.event_name = Some(ModelEvent::Dynamic);
        return options;
    };
    for property in &object.properties {
        let ObjectPropertyKind::ObjectProperty(property) = property else {
            options.event_name = Some(ModelEvent::Dynamic);
            continue;
        };
        let key = property
            .key
            .static_name()
            .map(|name| name.text().into_owned());
        match key.as_deref() {
            Some("eventName") => {
                options.event_name = Some(match expression_string(&property.value) {
                    Some(name) => ModelEvent::Known(name),
                    None => ModelEvent::Dynamic,
                });
            }
            Some("deep") => options.deep = true,
            Some(_) => {}
            None => options.event_name = Some(ModelEvent::Dynamic),
        }
    }
    options
}

fn argument_string(arg: &Argument<'_>) -> Option<String> {
    arg.as_expression().and_then(expression_string)
}

fn expression_string(expr: &Expression<'_>) -> Option<String> {
    match expr {
        Expression::StringLiteral(lit) => Some(lit.value.text().into_owned()),
        Expression::TemplateLiteral(tpl) if tpl.expressions.is_empty() => tpl
            .quasis
            .first()
            .and_then(|quasi| quasi.value.cooked)
            .map(|cooked| cooked.text().into_owned()),
        _ => None,
    }
}

/// Collect the model ref bindings that the script writes or uses whole.
/// A name resolves through the semantic scopes, so a parameter or a local
/// that shadows a model ref does not count as the ref.
struct ModelRefWriteVisitor<'r> {
    refs: &'r ModelRefs,
    scoping: &'r Scoping,
    written: FxHashSet<SymbolId>,
}

impl ModelRefWriteVisitor<'_> {
    /// The model ref symbol that `ident` resolves to.
    fn model_ref(&self, ident: &IdentifierReference<'_>) -> Option<SymbolId> {
        let symbol = self
            .scoping
            .get_reference(ident.reference_id.get()?)
            .symbol_id()?;
        self.refs.events.contains_key(&symbol).then_some(symbol)
    }

    /// The model ref `X` of a `X.value` member expression.
    fn value_member_ref(&self, member: &StaticMemberExpression<'_>) -> Option<SymbolId> {
        let Expression::Identifier(object) = &member.object else {
            return None;
        };
        if member.property.name != "value" {
            return None;
        }
        self.model_ref(object)
    }

    fn credit_simple_target(&mut self, target: &SimpleAssignmentTarget<'_>) {
        if let SimpleAssignmentTarget::StaticMemberExpression(member) = target
            && let Some(binding) = self.value_member_ref(member)
        {
            self.written.insert(binding);
        }
    }

    fn credit_target(&mut self, target: &AssignmentTarget<'_>) {
        match target {
            AssignmentTarget::ArrayAssignmentTarget(array) => {
                for element in array.elements.iter().flatten() {
                    self.credit_target_maybe_default(element);
                }
                if let Some(rest) = &array.rest {
                    self.credit_target(&rest.target);
                }
            }
            AssignmentTarget::ObjectAssignmentTarget(object) => {
                for property in &object.properties {
                    if let AssignmentTargetProperty::AssignmentTargetPropertyProperty(property) =
                        property
                    {
                        self.credit_target_maybe_default(&property.binding);
                    }
                }
                if let Some(rest) = &object.rest {
                    self.credit_target(&rest.target);
                }
            }
            _ => {
                if let Some(simple) = target.as_simple_assignment_target() {
                    self.credit_simple_target(simple);
                }
            }
        }
    }

    fn credit_target_maybe_default(&mut self, target: &AssignmentTargetMaybeDefault<'_>) {
        match target {
            AssignmentTargetMaybeDefault::AssignmentTargetWithDefault(with_default) => {
                self.credit_target(&with_default.binding);
            }
            _ => {
                if let Some(target) = target.as_assignment_target() {
                    self.credit_target(target);
                }
            }
        }
    }
}

impl<'a> Visit<'a> for ModelRefWriteVisitor<'_> {
    fn visit_assignment_expression(&mut self, expr: &AssignmentExpression<'a>) {
        self.credit_target(&expr.left);
        oxc_ast_visit::walk::walk_assignment_expression(self, expr);
    }

    fn visit_update_expression(&mut self, expr: &UpdateExpression<'a>) {
        self.credit_simple_target(&expr.argument);
        oxc_ast_visit::walk::walk_update_expression(self, expr);
    }

    fn visit_for_of_statement(&mut self, stmt: &ForOfStatement<'a>) {
        if let Some(target) = stmt.left.as_assignment_target() {
            self.credit_target(target);
        }
        oxc_ast_visit::walk::walk_for_of_statement(self, stmt);
    }

    fn visit_for_in_statement(&mut self, stmt: &ForInStatement<'a>) {
        if let Some(target) = stmt.left.as_assignment_target() {
            self.credit_target(target);
        }
        oxc_ast_visit::walk::walk_for_in_statement(self, stmt);
    }

    fn visit_static_member_expression(&mut self, expr: &StaticMemberExpression<'a>) {
        // A `X.value` read is not a write. An assignment target credits it above.
        if self.value_member_ref(expr).is_some() {
            return;
        }
        oxc_ast_visit::walk::walk_static_member_expression(self, expr);
    }

    fn visit_identifier_reference(&mut self, ident: &IdentifierReference<'a>) {
        // Any other use of the whole ref can reach code that writes it.
        if let Some(symbol) = self.model_ref(ident) {
            self.written.insert(symbol);
        }
    }
}

#[cfg(test)]
mod tests {
    use oxc_allocator::Allocator;
    use oxc_parser::Parser;
    use oxc_span::SourceType;

    use super::harvest_model_refs;

    fn written(source: &str, declared: &[&str]) -> (Vec<String>, bool) {
        let allocator = Allocator::default();
        let program = Parser::new(&allocator, source, SourceType::ts())
            .parse()
            .program;
        let harvest = harvest_model_refs(&program, declared.iter().copied());
        let mut events: Vec<String> = harvest.written.into_iter().collect();
        events.sort();
        (events, harvest.dynamic)
    }

    #[test]
    fn value_assignment_credits_the_event_of_the_key() {
        let (events, dynamic) = written(
            "const a = useVModel(props, 'title'); a.value = '';",
            &["update:title", "update:other"],
        );
        assert_eq!(events, ["update:title"]);
        assert!(!dynamic);
    }

    #[test]
    fn call_without_key_binds_model_value() {
        let (events, _) = written(
            "const a = useVModel(props); a.value++;",
            &["update:modelValue"],
        );
        assert_eq!(events, ["update:modelValue"]);
    }

    #[test]
    fn reads_and_nested_writes_credit_nothing() {
        let (events, _) = written(
            "const a = useVModel(props, 'modelValue'); a.value.id = 1; if (a.value) {} const [x] = [a.value];",
            &["update:modelValue"],
        );
        assert!(events.is_empty(), "{events:?}");
    }

    #[test]
    fn whole_ref_use_and_destructuring_target_credit_the_event() {
        let (events, _) = written(
            "const a = useVModel(props, 'a'); const b = useVModel(props, 'b'); track(a); [b.value] = [1];",
            &["update:a", "update:b"],
        );
        assert_eq!(events, ["update:a", "update:b"]);
    }

    #[test]
    fn event_name_option_and_deep_option() {
        let (events, _) = written(
            "const a = useVModel(props, 'a', undefined, { eventName: 'change:a' }); a.value = 1; \
             const b = useVModel(props, 'b', undefined, { deep: true });",
            &["update:a", "update:b", "change:a"],
        );
        assert_eq!(events, ["change:a", "update:b"]);
    }

    #[test]
    fn a_shadowing_parameter_is_not_the_ref() {
        let (events, _) = written(
            "const a = useVModel(props, 'modelValue'); const f = (a: any) => { a.value = 1; track(a); };",
            &["update:modelValue"],
        );
        assert!(events.is_empty(), "{events:?}");
    }

    #[test]
    fn callee_must_be_the_vueuse_composable() {
        let (local, _) = written(
            "const useVModel = (p: any, k: string) => ({ value: p[k] }); \
             const a = useVModel(props, 'modelValue'); a.value = 'x';",
            &["update:modelValue"],
        );
        assert!(local.is_empty(), "{local:?}");
        let (aliased, _) = written(
            "import { useVModel as useModel } from '@vueuse/core'; \
             const a = useModel(props, 'modelValue'); a.value = 'x';",
            &["update:modelValue"],
        );
        assert_eq!(aliased, ["update:modelValue"]);
    }

    #[test]
    fn for_of_head_target_credits_the_event() {
        let (events, _) = written(
            "const a = useVModel(props, 'modelValue'); for (a.value of items) {}",
            &["update:modelValue"],
        );
        assert_eq!(events, ["update:modelValue"]);
    }

    #[test]
    fn non_literal_key_is_dynamic() {
        let (_, dynamic) = written("const a = useVModel(props, key);", &["update:a"]);
        assert!(dynamic);
    }

    #[test]
    fn use_vmodels_maps_each_destructured_ref_to_its_prop() {
        let (events, _) = written(
            "const { open, step: page } = useVModels(props); open.value = false; console.log(page.value);",
            &["update:open", "update:step"],
        );
        assert_eq!(events, ["update:open"]);
    }

    #[test]
    fn use_vmodels_without_destructuring_credits_every_update_event() {
        let (events, _) = written(
            "const models = useVModels(props);",
            &["update:open", "update:step", "close"],
        );
        assert_eq!(events, ["update:open", "update:step"]);
    }
}
