use oxc_ast::ast::{
    Argument, ArrayExpressionElement, ArrowFunctionBody, BinaryExpression, BindingPattern,
    CallExpression, Class, ClassElement, Expression, MethodDefinitionKind, ObjectPropertyKind,
    PropertyDefinition, PropertyKey, Statement, TSAccessibility, TSSignature, TSType,
    TSTypeAnnotation, TSTypeName, TSTypeParameterInstantiation,
};
use oxc_span::{GetSpan, Span};
use rustc_hash::{FxHashMap, FxHashSet};

#[path = "helpers_heritage_names.rs"]
pub(in crate::visitor) mod heritage_names;
#[path = "helpers_heritage_root.rs"]
mod heritage_root;
#[path = "helpers_host_refs.rs"]
mod host_refs;
#[path = "helpers_return_bindings.rs"]
mod return_bindings;
pub(super) use super::visit_impl::visit_value_alias::is_builtin_constructor;
use super::visit_impl::visit_value_alias::value_alias_source;
use crate::js_text::JsText;
use crate::{MemberInfo, MemberKind};
use fallow_types::extract::{AngularInputMember, AngularOutputMember};
use heritage_names::{extract_static_expression_name, extract_type_name};
use heritage_root::heritage_root_name;
pub(super) use heritage_root::is_object_literal_initializer;
use host_refs::extract_host_member_refs;
#[cfg(test)]
use host_refs::{extract_identifiers_from_host_expr, is_valid_member_identifier};
use return_bindings::{
    collect_getter_binding, collect_method_return_binding, is_self_returning_instance_method,
    push_array_element_binding, returns_named_class_type,
};

pub struct AngularComponentMetadata {
    pub template_url: Option<String>,
    pub style_urls: Vec<String>,
    pub inline_template: Option<String>,
    pub inline_template_offset: Option<u32>,
    pub decorator_span: Span,
    pub host_member_refs: Vec<String>,
    pub input_output_members: Vec<String>,
    /// The raw `selector` string value (e.g. `'app-foo, [appBar]'`), present only
    /// for `@Component` (never `@Directive`). Consumed by the Angular
    /// `unrendered-component` detector. `None` when the decorator omits a
    /// `selector`, when the value is non-literal, or when it is a `@Directive`.
    pub selector: Option<String>,
}

const ANGULAR_SIGNAL_APIS: &[&str] = &[
    "input",
    "output",
    "outputFromObservable",
    "model",
    "viewChild",
    "viewChildren",
    "contentChild",
    "contentChildren",
];

pub fn extract_angular_component_metadata(class: &Class<'_>) -> Option<AngularComponentMetadata> {
    for decorator in &class.decorators {
        if let Some(metadata) = extract_decorator_metadata(decorator) {
            return Some(metadata);
        }
    }
    None
}

/// Mutable accumulator for `@Component` / `@Directive` decorator metadata while
/// its object properties are walked.
#[derive(Default)]
struct AngularMetadataAccumulator {
    template_url: Option<String>,
    style_urls: Vec<String>,
    inline_template: Option<String>,
    inline_template_offset: Option<u32>,
    host_member_refs: Vec<String>,
    input_output_members: Vec<String>,
    selector: Option<String>,
}

impl AngularMetadataAccumulator {
    /// True when at least one metadata field was populated.
    fn has_data(&self) -> bool {
        self.template_url.is_some()
            || !self.style_urls.is_empty()
            || self.inline_template.is_some()
            || !self.host_member_refs.is_empty()
            || !self.input_output_members.is_empty()
            || self.selector.is_some()
    }

    /// Build the public metadata struct, anchoring it at the decorator span.
    fn into_metadata(self, decorator_span: Span) -> AngularComponentMetadata {
        AngularComponentMetadata {
            template_url: self.template_url,
            style_urls: self.style_urls,
            inline_template: self.inline_template,
            inline_template_offset: self.inline_template_offset,
            decorator_span,
            host_member_refs: self.host_member_refs,
            input_output_members: self.input_output_members,
            selector: self.selector,
        }
    }
}

/// Extract Angular metadata from a single decorator, or `None` if it is not a
/// data-bearing `@Component` / `@Directive` decorator.
fn extract_decorator_metadata(
    decorator: &oxc_ast::ast::Decorator<'_>,
) -> Option<AngularComponentMetadata> {
    let Expression::CallExpression(call) = &decorator.expression else {
        return None;
    };
    let Expression::Identifier(id) = &call.callee else {
        return None;
    };
    if !matches!(id.name.as_str(), "Component" | "Directive") {
        return None;
    }
    let is_component = id.name.as_str() == "Component";
    let Some(Argument::ObjectExpression(obj)) = call.arguments.first() else {
        return None;
    };

    let mut acc = AngularMetadataAccumulator::default();
    for prop in &obj.properties {
        let ObjectPropertyKind::ObjectProperty(p) = prop else {
            continue;
        };
        let Some(key_name) = p.key.static_name() else {
            continue;
        };
        apply_decorator_property(&mut acc, &key_name.text(), p, is_component);
    }

    acc.has_data().then(|| acc.into_metadata(decorator.span()))
}

/// Apply one `@Component` / `@Directive` object property to the accumulator.
fn apply_decorator_property(
    acc: &mut AngularMetadataAccumulator,
    key_name: &str,
    p: &oxc_ast::ast::ObjectProperty<'_>,
    is_component: bool,
) {
    match key_name {
        "selector" if is_component => {
            if let Expression::StringLiteral(lit) = &p.value {
                acc.selector = Some(lit.value.text().into_owned());
            }
        }
        "templateUrl" => {
            if let Expression::StringLiteral(lit) = &p.value {
                acc.template_url = Some(lit.value.text().into_owned());
            }
        }
        "template" => apply_inline_template(acc, p),
        "styleUrl" => {
            if let Expression::StringLiteral(lit) = &p.value {
                acc.style_urls.push(lit.value.text().into_owned());
            }
        }
        "styleUrls" => {
            if let Expression::ArrayExpression(arr) = &p.value {
                for elem in &arr.elements {
                    if let ArrayExpressionElement::StringLiteral(lit) = elem {
                        acc.style_urls.push(lit.value.text().into_owned());
                    }
                }
            }
        }
        "host" => {
            if let Expression::ObjectExpression(host_obj) = &p.value {
                extract_host_member_refs(host_obj, &mut acc.host_member_refs);
            }
        }
        "inputs" | "outputs" => {
            extract_input_output_members(&p.value, &mut acc.input_output_members);
        }
        "queries" => {
            extract_query_members(&p.value, &mut acc.input_output_members);
        }
        _ => {}
    }
}

/// Capture an inline `template:` string or expressionless template literal.
fn apply_inline_template(
    acc: &mut AngularMetadataAccumulator,
    p: &oxc_ast::ast::ObjectProperty<'_>,
) {
    if let Expression::StringLiteral(lit) = &p.value {
        acc.inline_template = Some(lit.value.text().into_owned());
        acc.inline_template_offset = Some(lit.span.start.saturating_add(1));
    } else if let Expression::TemplateLiteral(tpl) = &p.value
        && tpl.expressions.is_empty()
        && let Some(quasi) = tpl.quasis.first()
    {
        let source = quasi
            .value
            .cooked
            .map_or_else(|| quasi.value.raw.as_str().into(), JsText::text)
            .into_owned();
        acc.inline_template = Some(source);
        acc.inline_template_offset = Some(p.value.span().start.saturating_add(1));
    }
}

fn extract_query_members(value: &Expression<'_>, members: &mut Vec<String>) {
    let Expression::ObjectExpression(obj) = value else {
        return;
    };
    for prop in &obj.properties {
        let ObjectPropertyKind::ObjectProperty(p) = prop else {
            continue;
        };
        if let Some(name) = p.key.static_name() {
            let name = name.text().into_owned();
            if !name.is_empty() {
                members.push(name);
            }
        }
    }
}

fn extract_input_output_members(value: &Expression<'_>, members: &mut Vec<String>) {
    let Expression::ArrayExpression(arr) = value else {
        return;
    };
    for elem in &arr.elements {
        let ArrayExpressionElement::StringLiteral(lit) = elem else {
            continue;
        };
        let text = lit.value.text();
        let member = text.split(':').next().unwrap_or_default().trim();
        if !member.is_empty() {
            members.push(member.to_string());
        }
    }
}

pub fn has_angular_class_decorator(class: &Class<'_>) -> bool {
    class.decorators.iter().any(|d| {
        if let Expression::CallExpression(call) = &d.expression
            && let Expression::Identifier(id) = &call.callee
        {
            matches!(
                id.name.as_str(),
                "Component" | "Directive" | "Injectable" | "Pipe"
            )
        } else {
            false
        }
    })
}

#[derive(Debug, Clone)]
pub(super) enum LitCustomElementDecorator {
    Named { local_name: String },
    Namespace { local_name: String },
}

pub(super) fn lit_custom_element_decorator(class: &Class<'_>) -> Option<LitCustomElementDecorator> {
    class.decorators.iter().find_map(|d| {
        let Expression::CallExpression(call) = &d.expression else {
            return None;
        };
        match &call.callee {
            Expression::Identifier(id) => Some(LitCustomElementDecorator::Named {
                local_name: id.name.to_string(),
            }),
            Expression::StaticMemberExpression(member)
                if member.property.name == "customElement" =>
            {
                let Expression::Identifier(object) = &member.object else {
                    return None;
                };
                Some(LitCustomElementDecorator::Namespace {
                    local_name: object.name.to_string(),
                })
            }
            _ => None,
        }
    })
}

/// Extract the tag-name string literal from a `@customElement('x-foo')`
/// decorator on a class (the Named `customElement(...)` or namespace
/// `ns.customElement(...)` form). Returns `None` when the tag argument is not a
/// static string literal (a computed tag the Lit `unrendered-component` arm
/// cannot key on).
pub(super) fn lit_custom_element_tag(class: &Class<'_>) -> Option<String> {
    class.decorators.iter().find_map(|d| {
        let Expression::CallExpression(call) = &d.expression else {
            return None;
        };
        let is_custom_element = match &call.callee {
            Expression::Identifier(id) => id.name == "customElement",
            Expression::StaticMemberExpression(member) => member.property.name == "customElement",
            _ => false,
        };
        if !is_custom_element {
            return None;
        }
        match call.arguments.first()? {
            Argument::StringLiteral(lit) => Some(lit.value.text().into_owned()),
            _ => None,
        }
    })
}

pub fn extract_custom_elements_define(
    call: &oxc_ast::ast::CallExpression<'_>,
) -> Option<(String, String)> {
    let Expression::StaticMemberExpression(member) = &call.callee else {
        return None;
    };
    // The registry call: bare `customElements.define` or `window.customElements`
    // / `globalThis.customElements.define`. Symmetric with the receiver handling
    // in `extract_custom_element_tag_reference` so a `window.`-qualified define
    // still registers the element (otherwise it would be a silent missed
    // registration and a false `unrendered-component`).
    if member.property.name != "define" || !is_custom_elements_receiver(&member.object) {
        return None;
    }
    let tag = match call.arguments.first()? {
        Argument::StringLiteral(lit) => lit.value.text().into_owned(),
        _ => return None,
    };
    let class_name = match call.arguments.get(1)? {
        Argument::Identifier(id) => id.name.to_string(),
        _ => return None,
    };
    Some((tag, class_name))
}

/// Extract the custom-element tag from an IMPERATIVE render / lookup call:
/// `document.createElement('x-foo')`, `customElements.get('x-foo')`, or
/// `customElements.whenDefined('x-foo')`. These reference an element by tag
/// without an `html` template, so the Lit `unrendered-component` arm must treat
/// the tag as rendered (the string-ref abstain). Only hyphenated custom-element
/// tags are returned; `document.createElement('div')` is a native element.
pub(super) fn extract_custom_element_tag_reference(
    call: &oxc_ast::ast::CallExpression<'_>,
) -> Option<String> {
    let Expression::StaticMemberExpression(member) = &call.callee else {
        return None;
    };
    let recognized = match member.property.name.as_str() {
        // `createElement` is Document-specific; a hyphenated tag argument is a
        // custom-element instantiation regardless of how the document is reached
        // (`document.createElement`, `opts.document.createElement`,
        // `el.ownerDocument.createElement`). Receiver-agnostic on purpose:
        // over-crediting only suppresses a finding, never creates one.
        "createElement" => true,
        // The Custom Elements registry API. Gated on a `customElements` receiver
        // (bare or `window.customElements` / `globalThis.customElements`) so a
        // generic `map.get('a-b')` does not credit an arbitrary hyphenated key.
        "get" | "whenDefined" => is_custom_elements_receiver(&member.object),
        _ => return None,
    };
    if !recognized {
        return None;
    }
    let Argument::StringLiteral(lit) = call.arguments.first()? else {
        return None;
    };
    let tag = &*lit.value.text();
    tag.contains('-').then(|| tag.to_string())
}

/// Whether an expression is the Custom Elements registry: the bare identifier
/// `customElements`, or a `*.customElements` member access (`window.customElements`,
/// `globalThis.customElements`).
fn is_custom_elements_receiver(expr: &Expression<'_>) -> bool {
    match expr {
        Expression::Identifier(id) => id.name == "customElements",
        Expression::StaticMemberExpression(member) => member.property.name == "customElements",
        _ => false,
    }
}

fn is_angular_signal_initializer(value: &Expression<'_>) -> bool {
    angular_signal_initializer_name(value).is_some()
}

/// Returns the Angular signal-API call name for a signal-member initializer:
/// `input` for `input(...)` / `input.required(...)`, `output` for `output(...)`,
/// `model` for `model(...)` / `model.required(...)`, `outputFromObservable` for
/// `outputFromObservable(...)`, and so on. Mirrors the matching logic in
/// `is_angular_signal_initializer` (identifier callee `foo(...)` and the
/// `foo.required(...)` static-member callee form) but surfaces the callee name so
/// the input/output harvester can classify the member's role. Returns `None` for
/// any non-signal-API initializer.
fn angular_signal_initializer_name<'a>(value: &'a Expression<'a>) -> Option<&'a str> {
    let Expression::CallExpression(call) = value else {
        return None;
    };
    match &call.callee {
        Expression::Identifier(id) => {
            let name = id.name.as_str();
            ANGULAR_SIGNAL_APIS.contains(&name).then_some(name)
        }
        Expression::StaticMemberExpression(member) => {
            if let Expression::Identifier(obj) = &member.object {
                let name = obj.name.as_str();
                (ANGULAR_SIGNAL_APIS.contains(&name) && member.property.name == "required")
                    .then_some(name)
            } else {
                None
            }
        }
        _ => None,
    }
}

pub(super) struct AngularSignalQuery {
    pub type_arg: String,
    pub plural: bool,
}

pub(super) fn extract_angular_signal_query(value: &Expression<'_>) -> Option<AngularSignalQuery> {
    let Expression::CallExpression(call) = value else {
        return None;
    };
    let Expression::Identifier(id) = &call.callee else {
        return None;
    };
    let plural = match id.name.as_str() {
        "viewChild" | "contentChild" => false,
        "viewChildren" | "contentChildren" => true,
        _ => return None,
    };
    if let Some(type_args) = call.type_arguments.as_deref()
        && let Some(first) = type_args.params.first()
        && let Some(name) = extract_type_reference_name(first)
        && !is_builtin_constructor(&name)
    {
        return Some(AngularSignalQuery {
            type_arg: name,
            plural,
        });
    }
    if let Some(Argument::Identifier(arg_id)) = call.arguments.first()
        && !is_builtin_constructor(arg_id.name.as_str())
    {
        return Some(AngularSignalQuery {
            type_arg: arg_id.name.to_string(),
            plural,
        });
    }
    None
}

pub(super) fn extract_query_list_element_type(annotation: &TSTypeAnnotation<'_>) -> Option<String> {
    extract_query_list_from_type(&annotation.type_annotation)
}

fn extract_query_list_from_type(ty: &TSType<'_>) -> Option<String> {
    match ty {
        TSType::TSTypeReference(type_ref) => {
            let name = extract_type_name(&type_ref.type_name)?;
            if name != "QueryList" {
                return None;
            }
            let type_args = type_ref.type_arguments.as_deref()?;
            let first = type_args.params.first()?;
            let element = extract_type_reference_name(first)?;
            if is_builtin_constructor(&element) {
                None
            } else {
                Some(element)
            }
        }
        TSType::TSParenthesizedType(paren) => extract_query_list_from_type(&paren.type_annotation),
        TSType::TSUnionType(union) => {
            let mut found: Option<String> = None;
            for branch in &union.types {
                match branch {
                    TSType::TSNullKeyword(_) | TSType::TSUndefinedKeyword(_) => {}
                    other => {
                        if found.is_some() {
                            return None;
                        }
                        found = extract_query_list_from_type(other);
                        found.as_ref()?;
                    }
                }
            }
            found
        }
        _ => None,
    }
}

/// Vue reactivity wrappers whose value (auto-unwrapped in a template) is an
/// array of the wrapped element type. A `v-for` iterates the unwrapped value, so
/// the loop item's class is the array element type of the wrapper's payload.
const VUE_REACTIVITY_WRAPPERS: &[&str] = &[
    "ref",
    "computed",
    "shallowRef",
    "reactive",
    "shallowReactive",
    "readonly",
    "toRef",
    "customRef",
];

/// Extract the element type name of an array-shaped TS type: `T[]`,
/// `readonly T[]`, `Array<T>`, `ReadonlyArray<T>`, a parenthesized form, and a
/// nullable union of any of those. Returns the element name only when it is a
/// non-builtin identifier type reference (a class / interface whose members the
/// analyze layer can credit); `number[]`, `string[]`, `Map[]`, etc. yield `None`.
pub fn array_element_type_from_type(ty: &TSType<'_>) -> Option<String> {
    array_element_target_from_type(ty).map(|(name, _)| name)
}

pub(super) fn array_element_target_from_type(ty: &TSType<'_>) -> Option<(String, Option<Span>)> {
    match ty {
        TSType::TSArrayType(arr) => {
            let name = extract_type_reference_name(&arr.element_type)?;
            (!is_builtin_constructor(&name))
                .then(|| (name, extract_type_reference_origin(&arr.element_type)))
        }
        TSType::TSTypeReference(type_ref) => {
            let name = extract_type_name(&type_ref.type_name)?;
            if name != "Array" && name != "ReadonlyArray" {
                return None;
            }
            let first = type_ref.type_arguments.as_deref()?.params.first()?;
            let element = extract_type_reference_name(first)?;
            (!is_builtin_constructor(&element))
                .then(|| (element, extract_type_reference_origin(first)))
        }
        TSType::TSTypeOperatorType(op)
            if op.operator == oxc_ast::ast::TSTypeOperatorOperator::Readonly =>
        {
            array_element_target_from_type(&op.type_annotation)
        }
        TSType::TSParenthesizedType(paren) => {
            array_element_target_from_type(&paren.type_annotation)
        }
        TSType::TSUnionType(union) => {
            let mut found: Option<(String, Option<Span>)> = None;
            for branch in &union.types {
                match branch {
                    TSType::TSNullKeyword(_) | TSType::TSUndefinedKeyword(_) => {}
                    other => {
                        if found.is_some() {
                            return None;
                        }
                        found = array_element_target_from_type(other);
                        found.as_ref()?;
                    }
                }
            }
            found
        }
        _ => None,
    }
}

/// Extract the value type name of a map-shaped TS type: `Map<K, V>`,
/// `ReadonlyMap<K, V>`, a parenthesized form, and a nullable union of those.
/// Returns the value name only when it is a non-builtin identifier type
/// reference, so `Map<string, number>` yields `None`.
pub(super) fn map_value_type_from_type(ty: &TSType<'_>) -> Option<String> {
    match ty {
        TSType::TSTypeReference(type_ref) => {
            let name = extract_type_name(&type_ref.type_name)?;
            if name != "Map" && name != "ReadonlyMap" {
                return None;
            }
            map_value_type_from_args(type_ref.type_arguments.as_deref())
        }
        TSType::TSParenthesizedType(paren) => map_value_type_from_type(&paren.type_annotation),
        TSType::TSUnionType(union) => {
            let mut found: Option<String> = None;
            for branch in &union.types {
                match branch {
                    TSType::TSNullKeyword(_) | TSType::TSUndefinedKeyword(_) => {}
                    other => {
                        if found.is_some() {
                            return None;
                        }
                        found = map_value_type_from_type(other);
                        found.as_ref()?;
                    }
                }
            }
            found
        }
        _ => None,
    }
}

/// The value type name of a `new Map<K, V>()` initializer, under the same
/// non-builtin rule as `map_value_type_from_type`.
pub(super) fn map_value_type_from_new(expr: &Expression<'_>) -> Option<String> {
    match expr {
        Expression::ParenthesizedExpression(paren) => map_value_type_from_new(&paren.expression),
        Expression::NewExpression(new_expr) => {
            let Expression::Identifier(callee) = &new_expr.callee else {
                return None;
            };
            if callee.name != "Map" {
                return None;
            }
            map_value_type_from_args(new_expr.type_arguments.as_deref())
        }
        _ => None,
    }
}

fn map_value_type_from_args(args: Option<&TSTypeParameterInstantiation<'_>>) -> Option<String> {
    let args = args?;
    if args.params.len() != 2 {
        return None;
    }
    let value = extract_type_reference_name(&args.params[1])?;
    (!is_builtin_constructor(&value)).then_some(value)
}

/// Infer the element class of an array-shaped binding from its optional type
/// annotation and optional initializer. The annotation is authoritative when
/// present; otherwise the initializer is inspected for a Vue reactivity wrapper
/// (`computed<Util[]>(...)` generic arg, or a callback returning a typed array),
/// or a direct array literal of `new Util()` elements. Returns a non-builtin
/// class name only; the analyze layer resolves it to the defining export.
fn infer_array_binding_element_type(
    type_annotation: Option<&TSTypeAnnotation<'_>>,
    init: Option<&Expression<'_>>,
) -> Option<String> {
    infer_array_binding_element_target(type_annotation, init).map(|(name, _)| name)
}

pub(super) fn infer_array_binding_element_target(
    type_annotation: Option<&TSTypeAnnotation<'_>>,
    init: Option<&Expression<'_>>,
) -> Option<(String, Option<Span>)> {
    if let Some(annotation) = type_annotation {
        return array_element_target_from_type(&annotation.type_annotation);
    }
    infer_array_element_from_init(init?)
}

fn infer_array_element_from_init(expr: &Expression<'_>) -> Option<(String, Option<Span>)> {
    match expr {
        Expression::ParenthesizedExpression(paren) => {
            infer_array_element_from_init(&paren.expression)
        }
        // `const xs = await computed(() => [new Foo()])`: an awaited array-shaped
        // initializer types the binding to its element class. See issue #1793.
        Expression::AwaitExpression(await_expr) => {
            infer_array_element_from_init(&await_expr.argument)
        }
        Expression::ArrayExpression(arr) => array_literal_element_type(arr),
        Expression::CallExpression(call) => reactivity_wrapper_element_type(call),
        _ => None,
    }
}

/// The non-builtin class name from a function return type annotation of the
/// shape `Promise<T>` (an async factory) or a direct `T`. Returns `None` for a
/// builtin element and any non-reference shape. Used by the issue #1793
/// `Promise.all(arr.map(cb))` inference pre-pass.
pub(super) fn return_type_element_name(ty: &TSType<'_>) -> Option<String> {
    return_type_element_target(ty).map(|(name, _)| name)
}

pub(super) fn return_type_element_target(ty: &TSType<'_>) -> Option<(String, Option<Span>)> {
    match ty {
        TSType::TSParenthesizedType(paren) => return_type_element_target(&paren.type_annotation),
        TSType::TSTypeReference(type_ref) => {
            let name = extract_type_name(&type_ref.type_name)?;
            if name == "Promise" {
                let first = type_ref.type_arguments.as_deref()?.params.first()?;
                return extract_type_reference_name(first)
                    .filter(|inner| !is_builtin_constructor(inner))
                    .map(|inner| (inner, extract_type_reference_origin(first)));
            }
            (!is_builtin_constructor(&name)).then(|| (name, extract_type_reference_origin(ty)))
        }
        _ => None,
    }
}

fn reactivity_wrapper_element_type(call: &CallExpression<'_>) -> Option<(String, Option<Span>)> {
    let Expression::Identifier(callee) = &call.callee else {
        return None;
    };
    if !VUE_REACTIVITY_WRAPPERS.contains(&callee.name.as_str()) {
        return None;
    }
    if let Some(type_args) = call.type_arguments.as_deref()
        && let Some(first) = type_args.params.first()
        && let Some(element) = array_element_target_from_type(first)
    {
        return Some(element);
    }
    callback_returned_array_element(call.arguments.first()?)
}

pub(super) fn callback_returned_array_element(
    arg: &Argument<'_>,
) -> Option<(String, Option<Span>)> {
    match arg {
        Argument::ArrowFunctionExpression(arrow) => match &arrow.body {
            ArrowFunctionBody::FunctionBody(body) => function_body_returned_array_element(body),
            body => array_literal_element_type_of_expr(body.as_expression()?),
        },
        Argument::FunctionExpression(func) => {
            function_body_returned_array_element(func.body.as_deref()?)
        }
        _ => None,
    }
}

fn function_body_returned_array_element(
    body: &oxc_ast::ast::FunctionBody<'_>,
) -> Option<(String, Option<Span>)> {
    let Statement::ReturnStatement(ret) = body.statements.last()? else {
        return None;
    };
    match unwrap_returned_expr(ret.argument.as_ref()?) {
        Expression::ArrayExpression(arr) => array_literal_element_type(arr),
        // `const utls: Util[] = []; ...; return utls` (the issue #1707 repro):
        // resolve the returned local's declared array element type.
        Expression::Identifier(id) => local_typed_array_element(&body.statements, &id.name),
        _ => None,
    }
}

fn unwrap_returned_expr<'a, 'b>(expr: &'b Expression<'a>) -> &'b Expression<'a> {
    match expr {
        Expression::ParenthesizedExpression(paren) => unwrap_returned_expr(&paren.expression),
        other => other,
    }
}

fn array_literal_element_type_of_expr(expr: &Expression<'_>) -> Option<(String, Option<Span>)> {
    match expr {
        Expression::ParenthesizedExpression(paren) => {
            array_literal_element_type_of_expr(&paren.expression)
        }
        Expression::ArrayExpression(arr) => array_literal_element_type(arr),
        _ => None,
    }
}

/// The element class of an array literal whose elements are ALL `new Class()`
/// with the same non-builtin `Class`. Empty arrays, mixed classes, and any
/// non-`new` element yield `None` (no confident single element type).
fn array_literal_element_type(
    arr: &oxc_ast::ast::ArrayExpression<'_>,
) -> Option<(String, Option<Span>)> {
    let mut element: Option<(String, Option<Span>)> = None;
    for item in &arr.elements {
        let ArrayExpressionElement::NewExpression(new_expr) = item else {
            return None;
        };
        let Expression::Identifier(callee) = &new_expr.callee else {
            return None;
        };
        let name = callee.name.as_str();
        if is_builtin_constructor(name) {
            return None;
        }
        match &element {
            Some((existing, _)) if existing != name => return None,
            Some(_) => {}
            None => element = Some((name.to_string(), Some(callee.span))),
        }
    }
    element
}

/// Scan a function body's top-level statements for `const/let <name>: T[]` and
/// return the array element class of its annotation. Used when a reactivity
/// callback returns a locally-declared, array-annotated binding.
fn local_typed_array_element(
    statements: &[Statement<'_>],
    name: &str,
) -> Option<(String, Option<Span>)> {
    for stmt in statements {
        let Statement::VariableDeclaration(decl) = stmt else {
            continue;
        };
        for declarator in &decl.declarations {
            let BindingPattern::BindingIdentifier(id) = &declarator.id else {
                continue;
            };
            if id.name != name {
                continue;
            }
            if let Some(annotation) = declarator.type_annotation.as_deref() {
                return array_element_target_from_type(&annotation.type_annotation);
            }
        }
    }
    None
}

/// Collect the element class of each non-private component FIELD whose type is
/// an array (or reactive array) of a non-builtin class, keyed by field name.
///
/// Reuses `infer_array_binding_element_type` (the Vue v-for / iteration-binding
/// element-type inference) against each `PropertyDefinition`'s annotation and
/// initializer, so a component field `utils: Util[]` yields `("utils", "Util")`.
/// The Angular template scanner types a `@for` / `*ngFor` loop variable over
/// such a field to its element class. Private fields are skipped because a
/// template cannot iterate them. Over-credit only: a field whose element type
/// cannot be resolved is left out (status quo). See issue #1712.
pub(super) fn collect_component_field_array_types(class: &Class<'_>) -> FxHashMap<String, String> {
    let mut field_types: FxHashMap<String, String> = FxHashMap::default();
    for element in &class.body.body {
        let ClassElement::PropertyDefinition(prop) = element else {
            continue;
        };
        if matches!(prop.accessibility, Some(TSAccessibility::Private)) {
            continue;
        }
        let Some(name) = prop.key.static_name() else {
            continue;
        };
        if let Some(element_type) =
            infer_array_binding_element_type(prop.type_annotation.as_deref(), prop.value.as_ref())
        {
            field_types.insert(name.text().into_owned(), element_type);
        }
    }
    field_types
}

pub(super) fn has_angular_plural_query_decorator(
    decorators: &[oxc_ast::ast::Decorator<'_>],
) -> bool {
    decorators.iter().any(|decorator| {
        let Expression::CallExpression(call) = &decorator.expression else {
            return false;
        };
        let Expression::Identifier(id) = &call.callee else {
            return false;
        };
        matches!(id.name.as_str(), "ViewChildren" | "ContentChildren")
    })
}

/// Extract the public members of `class`.
///
/// `plain_object_names` holds the module-scope `const` bindings whose
/// initializer is an object literal, or `None` outside module scope, where a
/// nested binding can shadow them. A heritage call can take such a binding
/// as data.
pub fn extract_class_members(
    class: &Class<'_>,
    is_angular_class: bool,
    plain_object_names: Option<&FxHashSet<String>>,
) -> Vec<MemberInfo> {
    let class_name = class.id.as_ref().map(|id| id.name.as_str());
    let heritage_root = class
        .heritage
        .as_ref()
        .and_then(|heritage| heritage_root_name(&heritage.expression, plain_object_names));
    let override_base = |is_override: bool| heritage_root.clone().filter(|_| is_override);
    let mut members = Vec::new();
    let mut overload_slots: FxHashSet<(bool, String)> = FxHashSet::default();
    for element in &class.body.body {
        match element {
            ClassElement::MethodDefinition(method) => {
                if !is_member_slot_of_method(class, method, &mut overload_slots) {
                    continue;
                }
                if let Some(mut member) = build_method_member(method, class_name) {
                    member.override_base = override_base(method.r#override);
                    merge_overload_return_flags(class, method, class_name, &mut member);
                    members.push(member);
                }
            }
            ClassElement::PropertyDefinition(prop) => {
                if let Some(mut member) = build_property_member(prop, is_angular_class) {
                    member.override_base = override_base(prop.r#override);
                    members.push(member);
                }
            }
            _ => {}
        }
    }
    members
}

/// Whether `method` is the one declaration that stands for its overload set.
///
/// TypeScript declares an overloaded method as bodyless signatures plus one
/// implementation with a body. They are one runtime member, so one finding
/// must cover them. The implementation stands for the set. A set with no
/// implementation (abstract or ambient signatures) keeps its first signature.
/// Getters and setters are not overloads and keep their own entries.
fn is_member_slot_of_method(
    class: &Class<'_>,
    method: &oxc_ast::ast::MethodDefinition<'_>,
    overload_slots: &mut FxHashSet<(bool, String)>,
) -> bool {
    if method.kind != MethodDefinitionKind::Method {
        return true;
    }
    let Some(name) = method.key.static_name() else {
        return true;
    };
    if method.value.body.is_some() {
        return true;
    }
    let has_implementation = class.body.body.iter().any(|element| {
        matches!(
            element,
            ClassElement::MethodDefinition(other)
                if other.kind == MethodDefinitionKind::Method
                    && other.r#static == method.r#static
                    && other.value.body.is_some()
                    && other.key.static_name().is_some_and(|other_name| other_name == name)
        )
    });
    !has_implementation && overload_slots.insert((method.r#static, name.text().into_owned()))
}

/// Give the kept member of an overload set the return flags of every
/// declaration in the set.
///
/// A signature can declare the class as return type while the implementation
/// returns `any`. The call result is still a class instance, so the members
/// read on it must stay credited.
fn merge_overload_return_flags(
    class: &Class<'_>,
    method: &oxc_ast::ast::MethodDefinition<'_>,
    class_name: Option<&str>,
    member: &mut MemberInfo,
) {
    if method.kind != MethodDefinitionKind::Method {
        return;
    }
    let Some(name) = method.key.static_name() else {
        return;
    };
    for element in &class.body.body {
        let ClassElement::MethodDefinition(other) = element else {
            continue;
        };
        if std::ptr::eq(other.as_ref(), method)
            || other.kind != MethodDefinitionKind::Method
            || other.r#static != method.r#static
            || other
                .key
                .static_name()
                .is_none_or(|other_name| other_name != name)
        {
            continue;
        }
        if method.r#static {
            member.is_instance_returning_static |=
                is_instance_returning_static_method(other, class_name);
        } else {
            member.is_self_returning |= is_self_returning_instance_method(other, class_name);
        }
    }
}

/// Build a `MemberInfo` for a non-constructor, non-private method. A
/// `protected` method gets `is_protected`.
fn build_method_member(
    method: &oxc_ast::ast::MethodDefinition<'_>,
    class_name: Option<&str>,
) -> Option<MemberInfo> {
    let name_str = method.key.static_name()?.text().into_owned();
    if name_str == "constructor"
        || matches!(method.key, PropertyKey::PrivateIdentifier(_))
        || matches!(method.accessibility, Some(TSAccessibility::Private))
    {
        return None;
    }
    let is_instance_returning_static =
        method.r#static && is_instance_returning_static_method(method, class_name);
    let is_self_returning =
        !method.r#static && is_self_returning_instance_method(method, class_name);
    let decorator_names = method
        .decorators
        .iter()
        .map(|d| decorator_path(&d.expression))
        .collect();
    Some(MemberInfo {
        name: name_str,
        kind: MemberKind::ClassMethod,
        span: method.span,
        has_decorator: !method.decorators.is_empty(),
        decorator_names,
        is_instance_returning_static,
        is_self_returning,
        override_base: None,
        is_protected: matches!(method.accessibility, Some(TSAccessibility::Protected)),
        is_static: method.r#static,
    })
}

/// Build a `MemberInfo` for a non-`declare`, non-private property. A
/// `protected` property gets `is_protected`.
fn build_property_member(
    prop: &oxc_ast::ast::PropertyDefinition<'_>,
    is_angular_class: bool,
) -> Option<MemberInfo> {
    let name = prop.key.static_name()?;
    if prop.declare
        || matches!(prop.key, PropertyKey::PrivateIdentifier(_))
        || matches!(prop.accessibility, Some(TSAccessibility::Private))
    {
        return None;
    }
    let has_decorator = !prop.decorators.is_empty()
        || (is_angular_class
            && prop
                .value
                .as_ref()
                .is_some_and(is_angular_signal_initializer));
    let decorator_names = prop
        .decorators
        .iter()
        .map(|d| decorator_path(&d.expression))
        .collect();
    Some(MemberInfo {
        name: name.text().into_owned(),
        kind: MemberKind::ClassProperty,
        span: prop.span,
        has_decorator,
        decorator_names,
        is_instance_returning_static: false,
        is_self_returning: false,
        override_base: None,
        is_protected: matches!(prop.accessibility, Some(TSAccessibility::Protected)),
        is_static: prop.r#static,
    })
}

/// Harvest declared Angular component/directive inputs and outputs from a class.
///
/// Walks the class body's property definitions and classifies each member by:
/// - decorator path: `@Input()` (decorator name `Input`) is an input,
///   `@Output()` (decorator name `Output`) is an output;
/// - signal initializer: `input()` / `model()` are inputs, `output()` /
///   `outputFromObservable()` are outputs.
///
/// A `model()` is recorded as an INPUT ONLY (its implicit `update:` emit is
/// framework-driven, never a dead output). Accessor members (getter / setter
/// `@Input()`) are skipped per-member: a setter body runs on binding, so it is
/// inherently used and never a dead input. The caller gates this on the class
/// carrying an Angular component/directive decorator, so a non-Angular class with
/// a same-named `input` / `Input` helper never contributes.
pub fn extract_angular_inputs_outputs(
    class: &Class<'_>,
) -> (Vec<AngularInputMember>, Vec<AngularOutputMember>) {
    let mut inputs = Vec::new();
    let mut outputs = Vec::new();
    for element in &class.body.body {
        // Accessor inputs (`@Input() set foo(v)` / getter) are inherently used;
        // skip the whole method-definition arm so they never reach the flag path.
        let ClassElement::PropertyDefinition(prop) = element else {
            continue;
        };
        let Some(name) = prop.key.static_name() else {
            continue;
        };
        let span_start = prop.key.span().start;

        if let Some(role) = angular_decorator_member_role(prop) {
            match role {
                AngularMemberRole::Input => inputs.push(AngularInputMember {
                    name: name.text().into_owned(),
                    span_start,
                }),
                // Only an `@Output()` initialized by `new EventEmitter(...)` emits
                // through `this.bar.emit()`, the shape the syntactic scan can see.
                // An output typed as an `Observable` / `Subject` driven by an
                // external stream (e.g. `getLazyEmitter('bounds_changed')`) emits
                // without `.emit()`, so harvesting it would be a false positive.
                AngularMemberRole::Output => {
                    if output_is_event_emitter(prop.value.as_ref()) {
                        outputs.push(AngularOutputMember {
                            name: name.text().into_owned(),
                            span_start,
                        });
                    }
                }
            }
            continue;
        }

        // Signal-based: `input` / `model` -> input, `output` /
        // `outputFromObservable` -> output. `model()` is input-only.
        if let Some(signal_name) = prop
            .value
            .as_ref()
            .and_then(|value| angular_signal_initializer_name(value))
        {
            match signal_name {
                "input" | "model" => inputs.push(AngularInputMember {
                    name: name.text().into_owned(),
                    span_start,
                }),
                "output" | "outputFromObservable" => outputs.push(AngularOutputMember {
                    name: name.text().into_owned(),
                    span_start,
                }),
                _ => {}
            }
        }
    }
    (inputs, outputs)
}

fn angular_decorator_member_role(prop: &PropertyDefinition<'_>) -> Option<AngularMemberRole> {
    prop.decorators.iter().find_map(|decorator| {
        match decorator_path(&decorator.expression).as_str() {
            "Input" => Some(AngularMemberRole::Input),
            "Output" => Some(AngularMemberRole::Output),
            _ => None,
        }
    })
}

enum AngularMemberRole {
    Input,
    Output,
}

/// Whether an `@Output()` property initializer is a `new EventEmitter(...)`
/// construction, the only shape that emits through `this.<name>.emit()` and is
/// therefore visible to the syntactic emit scan. An `@Output()` with no
/// initializer, or one initialized by an `Observable` / `Subject` / lazy-emitter
/// call (driven by an external stream, e.g. `getLazyEmitter('bounds_changed')`),
/// emits without `.emit()`, so it must not be harvested as a dead-output
/// candidate. Conservative: only the literal `new EventEmitter` callee qualifies
/// (bare identifier or a `Foo.EventEmitter` member tail).
fn output_is_event_emitter(value: Option<&Expression<'_>>) -> bool {
    let Some(Expression::NewExpression(new_expr)) = value else {
        return false;
    };
    match &new_expr.callee {
        Expression::Identifier(ident) => ident.name.as_str() == "EventEmitter",
        Expression::StaticMemberExpression(member) => {
            member.property.name.as_str() == "EventEmitter"
        }
        _ => false,
    }
}

fn is_instance_returning_static_method(
    method: &oxc_ast::ast::MethodDefinition<'_>,
    class_name: Option<&str>,
) -> bool {
    if returns_named_class_type(method.value.return_type.as_ref(), class_name) {
        return true;
    }
    // `static async create(): Promise<Cls>` resolves to an instance. The
    // body can build the instance in a local first, so the annotation is the
    // only proof.
    if method.value.r#async
        && let (Some(name), Some(annotation)) = (class_name, method.value.return_type.as_deref())
        && return_type_element_name(&annotation.type_annotation).is_some_and(|ty| ty == name)
    {
        return true;
    }
    let Some(body) = method.value.body.as_ref() else {
        return false;
    };
    body.statements
        .last()
        .is_some_and(|stmt| statement_returns_class_instance(stmt, class_name))
}

fn statement_returns_class_instance(stmt: &Statement<'_>, class_name: Option<&str>) -> bool {
    let Statement::ReturnStatement(ret) = stmt else {
        return false;
    };
    let Some(expr) = ret.argument.as_ref() else {
        return false;
    };
    is_self_construction_expression(expr, class_name)
}

fn is_self_construction_expression(expr: &Expression<'_>, class_name: Option<&str>) -> bool {
    let Expression::NewExpression(new_expr) = expr else {
        return false;
    };
    match &new_expr.callee {
        Expression::ThisExpression(_) => true,
        Expression::Identifier(ident) => class_name.is_some_and(|name| ident.name.as_str() == name),
        _ => false,
    }
}

pub fn extract_super_class_name(class: &Class<'_>) -> Option<String> {
    extract_static_expression_name(&class.heritage.as_ref()?.expression)
}

#[must_use]
pub fn extract_implemented_interface_names(class: &Class<'_>) -> Vec<String> {
    class
        .implements
        .iter()
        .filter_map(|item| extract_type_name(&item.expression))
        .collect()
}

#[must_use]
pub fn extract_type_annotation_name(type_annotation: &TSTypeAnnotation<'_>) -> Option<String> {
    extract_type_reference_name(&type_annotation.type_annotation)
}

pub(super) fn extract_nested_type_bindings(
    type_annotation: &TSTypeAnnotation<'_>,
) -> Vec<(String, String, Option<Span>)> {
    let mut bindings = Vec::new();
    collect_nested_type_bindings(&type_annotation.type_annotation, None, &mut bindings);
    bindings
}

pub(super) fn collect_nested_type_bindings(
    ty: &TSType<'_>,
    prefix: Option<&str>,
    bindings: &mut Vec<(String, String, Option<Span>)>,
) {
    match ty {
        TSType::TSTypeLiteral(type_lit) => {
            for member in &type_lit.members {
                let TSSignature::TSPropertySignature(prop) = member else {
                    continue;
                };
                let Some(property_name) = prop.key.static_name() else {
                    continue;
                };
                let path = if let Some(prefix) = prefix {
                    format!("{prefix}.{}", property_name.text())
                } else {
                    property_name.text().into_owned()
                };
                let Some(type_annotation) = prop.type_annotation.as_deref() else {
                    continue;
                };
                if let Some(type_name) = extract_type_annotation_name(type_annotation) {
                    bindings.push((
                        path,
                        type_name,
                        extract_type_reference_origin(&type_annotation.type_annotation),
                    ));
                } else {
                    collect_nested_type_bindings(
                        &type_annotation.type_annotation,
                        Some(path.as_str()),
                        bindings,
                    );
                }
            }
        }
        TSType::TSParenthesizedType(paren) => {
            collect_nested_type_bindings(&paren.type_annotation, prefix, bindings);
        }
        _ => {}
    }
}

#[must_use]
pub fn extract_class_instance_bindings<F>(
    class: &Class<'_>,
    is_named_import_from: F,
) -> Vec<(String, String)>
where
    F: Fn(&str, &str, &str) -> bool,
{
    extract_class_instance_bindings_with_origins(class, is_named_import_from).0
}

pub(super) fn extract_class_instance_bindings_with_origins<F>(
    class: &Class<'_>,
    is_named_import_from: F,
) -> (Vec<(String, String)>, FxHashMap<String, Span>)
where
    F: Fn(&str, &str, &str) -> bool,
{
    let type_param_constraints = collect_class_type_param_constraints(class);
    let resolve = |raw: String| -> Option<String> {
        if let Some(replacement) = type_param_constraints.get(raw.as_str()) {
            return replacement.clone();
        }
        Some(raw)
    };
    let constraints = collect_class_type_param_constraint_origins(class);
    let resolve_origin = |ty: &TSType<'_>| -> Option<Span> {
        let name = extract_type_reference_name(ty)?;
        match constraints.get(&name) {
            Some(Some(target)) => target.reference_span,
            Some(None) => None,
            None => extract_type_reference_origin(ty),
        }
    };
    let mut origins = FxHashMap::default();
    let mut bindings: Vec<(String, String)> = Vec::new();
    for element in &class.body.body {
        match element {
            ClassElement::MethodDefinition(method) => {
                if matches!(method.kind, MethodDefinitionKind::Constructor) {
                    collect_constructor_param_bindings(
                        method,
                        &resolve,
                        &resolve_origin,
                        &mut bindings,
                        &mut origins,
                    );
                } else if matches!(method.kind, MethodDefinitionKind::Get) {
                    collect_getter_binding(
                        method,
                        &resolve,
                        &resolve_origin,
                        &mut bindings,
                        &mut origins,
                    );
                } else if matches!(method.kind, MethodDefinitionKind::Method) {
                    collect_method_return_binding(class, method, &resolve, &mut bindings);
                }
            }
            ClassElement::PropertyDefinition(prop) => {
                let before = bindings.len();
                collect_property_binding(prop, &resolve, &is_named_import_from, &mut bindings);
                let origin = prop
                    .type_annotation
                    .as_deref()
                    .and_then(|ty| resolve_origin(&ty.type_annotation))
                    .or_else(|| match prop.value.as_ref() {
                        Some(Expression::NewExpression(expr)) => match &expr.callee {
                            Expression::Identifier(id) => Some(id.span),
                            _ => None,
                        },
                        _ => None,
                    });
                if let Some(origin) = origin {
                    for (name, _) in &bindings[before..] {
                        origins.insert(name.clone(), origin);
                    }
                }
            }
            _ => {}
        }
    }
    (bindings, origins)
}

/// Positional super-class type arguments (the `<DerivedClient>` in
/// `extends BaseService<DerivedClient>`). A positional arg that is not a bare
/// identifier type reference (nested generic `Foo<Bar>`, qualified `ns.Foo`,
/// union, literal) yields an empty string so callers keep index alignment with
/// the base's type parameters (issue #1910).
#[must_use]
pub fn extract_super_class_type_args(class: &Class<'_>) -> Vec<String> {
    let Some(type_args) = class
        .heritage
        .as_ref()
        .and_then(|heritage| heritage.type_arguments.as_deref())
    else {
        return Vec::new();
    };
    type_args
        .params
        .iter()
        .map(|ty| plain_type_reference_name(ty).unwrap_or_default())
        .collect()
}

/// Ordered class type-parameter names (`T`, `U` in `class C<T, U>`).
#[must_use]
pub fn extract_class_type_parameter_names(class: &Class<'_>) -> Vec<String> {
    class
        .type_parameters
        .as_deref()
        .map_or_else(Vec::new, |parameters| {
            parameters
                .params
                .iter()
                .map(|parameter| parameter.name.name.to_string())
                .collect()
        })
}

/// Bare identifier type-reference name (`DerivedClient`), or `None` for any
/// other shape (nested generic, qualified name, union, literal).
fn plain_type_reference_name(ty: &TSType<'_>) -> Option<String> {
    let TSType::TSTypeReference(type_ref) = ty else {
        return None;
    };
    if type_ref.type_arguments.is_some() {
        return None;
    }
    match &type_ref.type_name {
        TSTypeName::IdentifierReference(ident) => Some(ident.name.to_string()),
        _ => None,
    }
}

/// Instance-binding fields whose annotation is EXACTLY a class type parameter,
/// as `(field_name, type_param_index)`. Complements the constraint-substituted
/// `extract_class_instance_bindings`: the analyze layer uses these to resolve an
/// inherited generic property to the subclass's concrete type argument rather
/// than the constraint (issue #1910). Non-generic fields are absent here.
#[must_use]
pub fn extract_class_generic_instance_bindings(class: &Class<'_>) -> Vec<(String, usize)> {
    let param_indices = class_type_param_indices(class);
    if param_indices.is_empty() {
        return Vec::new();
    }
    let mut bindings: Vec<(String, usize)> = Vec::new();
    for element in &class.body.body {
        match element {
            ClassElement::MethodDefinition(method) => {
                if matches!(method.kind, MethodDefinitionKind::Constructor) {
                    for param in &method.value.params.items {
                        let Some(accessibility) = param.accessibility else {
                            continue;
                        };
                        if matches!(accessibility, TSAccessibility::Private) {
                            continue;
                        }
                        let BindingPattern::BindingIdentifier(id) = &param.pattern else {
                            continue;
                        };
                        let Some(type_annotation) = param.type_annotation.as_deref() else {
                            continue;
                        };
                        push_generic_binding(
                            id.name.as_str(),
                            type_annotation,
                            &param_indices,
                            &mut bindings,
                        );
                    }
                } else if matches!(method.kind, MethodDefinitionKind::Get) {
                    if matches!(method.accessibility, Some(TSAccessibility::Private)) {
                        continue;
                    }
                    let Some(name) = method.key.static_name() else {
                        continue;
                    };
                    let Some(type_annotation) = method.value.return_type.as_deref() else {
                        continue;
                    };
                    push_generic_binding(
                        &name.text(),
                        type_annotation,
                        &param_indices,
                        &mut bindings,
                    );
                }
            }
            ClassElement::PropertyDefinition(prop) => {
                if matches!(prop.accessibility, Some(TSAccessibility::Private)) {
                    continue;
                }
                let Some(name) = prop.key.static_name() else {
                    continue;
                };
                let Some(type_annotation) = prop.type_annotation.as_deref() else {
                    continue;
                };
                push_generic_binding(&name.text(), type_annotation, &param_indices, &mut bindings);
            }
            _ => {}
        }
    }
    bindings
}

/// Record `(field, type_param_index)` when the field's annotation is exactly a
/// class type parameter.
fn push_generic_binding(
    field: &str,
    type_annotation: &TSTypeAnnotation<'_>,
    param_indices: &FxHashMap<String, usize>,
    bindings: &mut Vec<(String, usize)>,
) {
    let Some(type_name) = extract_type_annotation_name(type_annotation) else {
        return;
    };
    if let Some(index) = param_indices.get(type_name.as_str()) {
        bindings.push((field.to_string(), *index));
    }
}

/// Ordered class type-parameter name -> positional index map.
fn class_type_param_indices(class: &Class<'_>) -> FxHashMap<String, usize> {
    let mut map = FxHashMap::default();
    let Some(type_parameters) = class.type_parameters.as_deref() else {
        return map;
    };
    for (index, param) in type_parameters.params.iter().enumerate() {
        map.insert(param.name.name.to_string(), index);
    }
    map
}

/// Push instance bindings for each non-private typed constructor parameter.
fn collect_constructor_param_bindings(
    method: &oxc_ast::ast::MethodDefinition<'_>,
    resolve: &impl Fn(String) -> Option<String>,
    resolve_origin: &impl Fn(&TSType<'_>) -> Option<Span>,
    bindings: &mut Vec<(String, String)>,
    origins: &mut FxHashMap<String, Span>,
) {
    for param in &method.value.params.items {
        let Some(accessibility) = param.accessibility else {
            continue;
        };
        if matches!(accessibility, TSAccessibility::Private) {
            continue;
        }
        let BindingPattern::BindingIdentifier(id) = &param.pattern else {
            continue;
        };
        let Some(type_annotation) = param.type_annotation.as_deref() else {
            continue;
        };
        push_array_element_binding(id.name.as_str(), type_annotation, resolve, bindings);
        let Some(type_name) = extract_type_annotation_name(type_annotation) else {
            continue;
        };
        let Some(resolved) = resolve(type_name) else {
            continue;
        };
        if let Some(origin) = resolve_origin(&type_annotation.type_annotation) {
            origins.insert(id.name.to_string(), origin);
        }
        bindings.push((id.name.to_string(), resolved));
    }
}

/// Push an instance binding for a non-private property: typed annotation first,
/// then a `new Class()` initializer, then an Angular `inject()` initializer,
/// then a value alias (`levels = Level`).
fn collect_property_binding<F>(
    prop: &oxc_ast::ast::PropertyDefinition<'_>,
    resolve: &impl Fn(String) -> Option<String>,
    is_named_import_from: &F,
    bindings: &mut Vec<(String, String)>,
) where
    F: Fn(&str, &str, &str) -> bool,
{
    if matches!(prop.accessibility, Some(TSAccessibility::Private)) {
        return;
    }
    let Some(name) = prop.key.static_name() else {
        return;
    };
    if let Some(type_annotation) = prop.type_annotation.as_deref() {
        push_array_element_binding(&name.text(), type_annotation, resolve, bindings);
    }
    if let Some(type_annotation) = prop.type_annotation.as_deref()
        && let Some(type_name) = extract_type_annotation_name(type_annotation)
    {
        if let Some(resolved) = resolve(type_name) {
            bindings.push((name.text().into_owned(), resolved));
        }
        return;
    }
    if let Some(Expression::NewExpression(new_expr)) = &prop.value
        && let Expression::Identifier(callee) = &new_expr.callee
        && !is_builtin_constructor(callee.name.as_str())
    {
        bindings.push((name.text().into_owned(), callee.name.to_string()));
        return;
    }
    if let Some(Expression::CallExpression(call)) = &prop.value
        && let Some(type_name) = extract_angular_inject_target(call, is_named_import_from)
    {
        bindings.push((name.text().into_owned(), type_name));
        return;
    }
    if let Some(source) = prop.value.as_ref().and_then(value_alias_source) {
        bindings.push((name.text().into_owned(), source.to_string()));
    }
}

pub fn extract_angular_inject_target<F>(
    call: &CallExpression<'_>,
    is_named_import_from: &F,
) -> Option<String>
where
    F: Fn(&str, &str, &str) -> bool,
{
    let Expression::Identifier(callee) = &call.callee else {
        return None;
    };
    if !is_named_import_from(callee.name.as_str(), "@angular/core", "inject") {
        return None;
    }

    if let Some(type_arguments) = call.type_arguments.as_deref()
        && let Some(TSType::TSTypeReference(type_ref)) = type_arguments.params.first()
        && let Some(type_name) = type_name_root(&type_ref.type_name)
    {
        return Some(type_name);
    }

    let Some(Argument::Identifier(target)) = call.arguments.first() else {
        return None;
    };
    Some(target.name.to_string())
}

fn type_name_root(name: &TSTypeName<'_>) -> Option<String> {
    match name {
        TSTypeName::IdentifierReference(ident) => Some(ident.name.to_string()),
        TSTypeName::QualifiedName(qualified) => type_name_root(&qualified.left),
        TSTypeName::ThisExpression(_) => None,
    }
}

#[must_use]
pub fn collect_class_type_param_constraints(
    class: &Class<'_>,
) -> FxHashMap<String, Option<String>> {
    collect_class_type_param_constraint_origins(class)
        .into_iter()
        .map(|(name, target)| (name, target.map(|target| target.name)))
        .collect()
}

pub(super) fn collect_class_type_param_constraint_origins(
    class: &Class<'_>,
) -> FxHashMap<String, Option<super::TypePropertyTarget>> {
    let mut map = FxHashMap::default();
    let Some(type_parameters) = class.type_parameters.as_deref() else {
        return map;
    };
    for param in &type_parameters.params {
        let constraint_name = param
            .constraint
            .as_ref()
            .and_then(extract_type_reference_name);
        let target = constraint_name.map(|name| super::TypePropertyTarget {
            name,
            reference_span: param
                .constraint
                .as_ref()
                .and_then(extract_type_reference_origin),
            owner_span: None,
        });
        map.insert(param.name.name.to_string(), target);
    }
    map
}

#[must_use]
pub(super) fn extract_type_reference_origin(ty: &TSType<'_>) -> Option<Span> {
    match heritage_names::unwrap_member_preserving_type(ty) {
        TSType::TSTypeReference(reference) => match &reference.type_name {
            TSTypeName::IdentifierReference(reference) => Some(reference.span),
            _ => None,
        },
        TSType::TSParenthesizedType(parenthesized) => {
            extract_type_reference_origin(&parenthesized.type_annotation)
        }
        TSType::TSUnionType(union) if extract_nullable_union_name(union).is_some() => {
            union.types.iter().find_map(extract_type_reference_origin)
        }
        _ => None,
    }
}

pub fn extract_type_reference_name(ty: &TSType<'_>) -> Option<String> {
    match heritage_names::unwrap_member_preserving_type(ty) {
        TSType::TSTypeReference(type_ref) => extract_type_name(&type_ref.type_name),
        TSType::TSParenthesizedType(paren) => extract_type_reference_name(&paren.type_annotation),
        TSType::TSUnionType(union) => extract_nullable_union_name(union),
        _ => None,
    }
}

fn extract_nullable_union_name(union: &oxc_ast::ast::TSUnionType<'_>) -> Option<String> {
    let mut found: Option<String> = None;
    for branch in &union.types {
        match branch {
            TSType::TSNullKeyword(_) | TSType::TSUndefinedKeyword(_) => {}
            other => {
                if found.is_some() {
                    return None;
                }
                found = Some(extract_type_reference_name(other)?);
            }
        }
    }
    found
}

fn decorator_path(expr: &Expression<'_>) -> String {
    match expr {
        Expression::Identifier(id) => id.name.to_string(),
        Expression::StaticMemberExpression(member) => {
            let object = decorator_path(&member.object);
            if object.is_empty() {
                String::new()
            } else {
                format!("{}.{}", object, member.property.name)
            }
        }
        Expression::CallExpression(call) => decorator_path(&call.callee),
        Expression::ParenthesizedExpression(paren) => decorator_path(&paren.expression),
        _ => String::new(),
    }
}

pub(super) fn is_meta_url_arg(arg: &Argument<'_>) -> bool {
    if let Argument::StaticMemberExpression(member) = arg
        && member.property.name == "url"
        && matches!(
            member.object,
            Expression::ImportMeta(_) | Expression::NewTarget(_)
        )
    {
        return true;
    }
    false
}

pub(super) fn ts_import_type_qualifier_root<'a>(
    qualifier: &'a oxc_ast::ast::TSImportTypeQualifier<'a>,
) -> &'a str {
    let mut current = qualifier;
    loop {
        match current {
            oxc_ast::ast::TSImportTypeQualifier::Identifier(id) => return id.name.as_str(),
            oxc_ast::ast::TSImportTypeQualifier::QualifiedName(qn) => current = &qn.left,
        }
    }
}

/// Escape the glob metacharacters of a literal path part.
///
/// The resolver reads a dynamic import pattern as a glob. A literal part, such
/// as the `./[id]/` prefix of a template literal, must match only itself. Each
/// metacharacter becomes a one-character class, for example `[` becomes
/// `[[]`. A class works on every platform, but a backslash escape does not
/// work on Windows. The escape of `(` stops the resolver from reading a text
/// such as `@(a)` as an extglob group.
pub(super) fn escape_glob_literal(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for character in text.chars() {
        if matches!(character, '*' | '?' | '[' | ']' | '{' | '}' | '(') {
            escaped.push('[');
            escaped.push(character);
            escaped.push(']');
        } else {
            escaped.push(character);
        }
    }
    escaped
}

/// The glob prefix and suffix of a `'./dir/' + name + '.js'` concatenation.
/// Both parts are glob-escaped literal text.
pub(super) fn extract_concat_parts(
    expr: &BinaryExpression<'_>,
) -> Option<(String, Option<String>)> {
    let prefix = escape_glob_literal(&extract_leading_string(&expr.left)?);
    let suffix = extract_trailing_string(&expr.right).map(|suffix| escape_glob_literal(&suffix));
    Some((prefix, suffix))
}

fn extract_leading_string(expr: &Expression<'_>) -> Option<String> {
    match expr {
        Expression::StringLiteral(lit) => Some(lit.value.text().into_owned()),
        Expression::BinaryExpression(bin)
            if bin.operator == oxc_ast::ast::BinaryOperator::Addition =>
        {
            extract_leading_string(&bin.left)
        }
        _ => None,
    }
}

fn extract_trailing_string(expr: &Expression<'_>) -> Option<String> {
    match expr {
        Expression::StringLiteral(lit) => {
            let s = lit.value.text().into_owned();
            if s.is_empty() { None } else { Some(s) }
        }
        _ => None,
    }
}

pub(super) fn regex_pattern_to_suffix(pattern: &str) -> Option<String> {
    let p = pattern.strip_prefix('^').unwrap_or(pattern);
    let p = p.strip_prefix(".*").unwrap_or(p);

    let p = p.strip_prefix("\\.")?;

    let p = p.strip_suffix('$')?;

    if let Some(base) = p.strip_suffix('?') {
        if base.chars().all(|c| c.is_ascii_alphanumeric()) && !base.is_empty() {
            let without_last = &base[..base.len() - 1];
            if without_last.is_empty() {
                return None;
            }
            return Some(format!(".{{{without_last},{base}}}"));
        }
        return None;
    }

    if let Some(inner) = p.strip_prefix('(').and_then(|s| s.strip_suffix(')')) {
        let exts: Vec<&str> = inner.split('|').collect();
        if exts
            .iter()
            .all(|e| e.chars().all(|c| c.is_ascii_alphanumeric()) && !e.is_empty())
        {
            return Some(format!(".{{{}}}", exts.join(",")));
        }
        return None;
    }

    if p.chars().all(|c| c.is_ascii_alphanumeric()) && !p.is_empty() {
        return Some(format!(".{p}"));
    }

    None
}

pub(super) fn try_extract_factory_new_class(arguments: &[Argument<'_>]) -> Option<String> {
    factory_new_class_target(arguments).map(|(name, _)| name)
}

pub(super) fn try_extract_factory_new_class_origin(
    arguments: &[Argument<'_>],
) -> Option<(String, Span)> {
    let (name, origin) = factory_new_class_target(arguments)?;
    Some((name, origin?))
}

fn factory_new_class_target(arguments: &[Argument<'_>]) -> Option<(String, Option<Span>)> {
    for arg in arguments {
        let executes = match arg {
            Argument::ArrowFunctionExpression(arrow) => !arrow.r#async,
            Argument::FunctionExpression(function) => !function.r#async && !function.generator,
            _ => false,
        };
        let class_name = match arg {
            Argument::ArrowFunctionExpression(arrow) => match &arrow.body {
                ArrowFunctionBody::FunctionBody(body) => {
                    extract_new_class_from_return_body(&body.statements)
                }
                body => extract_new_class_from_expression(body.as_expression()?),
            },
            Argument::FunctionExpression(func) => {
                extract_new_class_from_return_body(&func.body.as_ref()?.statements)
            }
            _ => None,
        };
        if let Some((name, origin)) = class_name
            && !is_builtin_constructor(&name)
        {
            return Some((name, executes.then_some(origin)));
        }
    }
    None
}

fn extract_new_class_from_expression(expr: &Expression<'_>) -> Option<(String, Span)> {
    if let Expression::NewExpression(new_expr) = expr
        && let Expression::Identifier(callee) = &new_expr.callee
    {
        return Some((callee.name.to_string(), callee.span));
    }
    None
}

fn extract_new_class_from_return_body(stmts: &[Statement<'_>]) -> Option<(String, Span)> {
    for stmt in stmts.iter().rev() {
        if let Statement::ReturnStatement(ret) = stmt
            && let Some(Expression::NewExpression(new_expr)) = &ret.argument
            && let Expression::Identifier(callee) = &new_expr.callee
        {
            return Some((callee.name.to_string(), callee.span));
        }
    }
    None
}

#[cfg(all(test, not(miri)))]
#[path = "helpers_tests.rs"]
mod tests;

/// Keep the start offset of each final resolve-call span.
pub(super) fn finish_package_resolve_sites(sites: Vec<(String, Span)>) -> Box<[(String, u32)]> {
    sites
        .into_iter()
        .map(|(package_name, span)| (package_name, span.start))
        .collect()
}
