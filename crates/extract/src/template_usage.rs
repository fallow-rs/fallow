use fallow_types::extract::{SinkArgKind, SinkLiteralValue, SinkShape, SinkSite};
use oxc_allocator::Allocator;
use oxc_ast::ast::{
    BinaryOperator, CallExpression, Expression, ObjectExpression, ObjectPropertyKind, Statement,
};
use oxc_ast_visit::Visit;
use oxc_parser::Parser;
use oxc_semantic::SemanticBuilder;
use oxc_span::SourceType;
use rustc_hash::{FxHashMap, FxHashSet};

use crate::visitor::ModuleInfoExtractor;
use crate::{MemberAccess, ModuleInfo};

/// The bindings that the script pass of a component declares and that its
/// markup can call. The template-visible imports and bound targets reach the
/// snippet scan as their own parameters; this context adds the bindings that
/// the visitor needs to read a call in markup the same way as in script.
#[derive(Debug, Default, Clone)]
pub struct ScriptBindingContext {
    /// Svelte `createEventDispatcher()` bindings.
    pub(crate) event_dispatch_bindings: FxHashSet<String>,
    /// Vue emit bindings: the `defineEmits()` return binding and `$emit`.
    pub(crate) emit_bindings: FxHashSet<String>,
    /// VueUse model refs (`useVModel` / `useVModels`) with the events that a
    /// write of the ref emits.
    pub(crate) model_ref_events: FxHashMap<String, Vec<String>>,
    /// Script constants with a literal value (`const cmd = "ls"`). A security
    /// sink in markup reads such an argument as a literal, as in script.
    pub(crate) static_sink_literals: FxHashMap<String, StaticSinkValue>,
}

/// The static value of a binding or an expression for the sink checks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StaticSinkValue {
    /// A value with its text.
    Known(SinkLiteralValue),
    /// A static string longer than the limit of a static sink literal.
    /// Extraction does not keep its text, so a check that reads the text
    /// treats it as unknown, and a check for a dynamic value treats it as
    /// static.
    Long,
}

impl StaticSinkValue {
    /// The value, when its text is known.
    pub fn known(self) -> Option<SinkLiteralValue> {
        match self {
            Self::Known(value) => Some(value),
            Self::Long => None,
        }
    }
}

impl ScriptBindingContext {
    /// Whether `snippet` names a dispatch or emit binding of the context.
    fn names_event_binding(&self, snippet: &str) -> (bool, bool) {
        (
            self.event_dispatch_bindings
                .iter()
                .any(|name| snippet.contains(name.as_str())),
            self.emit_bindings
                .iter()
                .any(|name| snippet.contains(name.as_str())),
        )
    }
}

/// The module of a component while its markup snippets merge into it. Each
/// snippet extractor merges through [`ModuleInfoExtractor::merge_into`], the
/// same merge that the script blocks use. A markup snippet has no source
/// position of its own, so each of its spans moves to `anchor`, the start of
/// the markup tag that holds the snippet.
#[derive(Debug, Clone)]
pub struct MarkupFactSink {
    context: std::sync::Arc<ScriptBindingContext>,
    module: Box<ModuleInfo>,
    anchor: u32,
    /// The snippets that merged, by anchor and text. The Svelte scanner reads
    /// the `{...}` of an attribute both as a brace section of the tag and as
    /// the attribute value. The usage fields are sets, so a second read
    /// changes nothing there, but a fact list such as the dispatched events
    /// must get each snippet once.
    absorbed: FxHashSet<(u32, String)>,
}

impl MarkupFactSink {
    /// A snippet that does not parse can hide a dispatch or emit call. When it
    /// names a dispatch or emit binding, the component abstains, as for a call
    /// with no literal event name.
    fn abstain_on_unparsed(&mut self, snippet: &str) {
        let (dispatch, emit) = self.context.names_event_binding(snippet);
        self.module.has_dynamic_dispatch |= dispatch;
        self.module.has_dynamic_emit |= emit;
    }

    /// Whether the snippet `text` at the current anchor merges for the first time.
    fn first_absorb(&mut self, text: &str) -> bool {
        self.absorbed.insert((self.anchor, text.to_string()))
    }

    fn absorb(&mut self, mut extractor: ModuleInfoExtractor) {
        extractor.clear_script_only_facts();
        let anchor = self.anchor;
        extractor.remap_spans_with(|_| oxc_span::Span::new(anchor, anchor));
        let dispatch_base = self.module.svelte_dispatched_events.len();
        extractor.merge_into(&mut self.module);
        for event in &mut self.module.svelte_dispatched_events[dispatch_base..] {
            event.span_start = anchor;
        }
    }
}

#[derive(Debug, Default, Clone)]
pub struct TemplateUsage {
    pub(crate) used_bindings: FxHashSet<String>,
    pub(crate) member_accesses: Vec<MemberAccess>,
    pub(crate) whole_object_uses: Vec<String>,
    /// PascalCase tag names that matched no import or local binding.
    pub(crate) unresolved_tag_names: FxHashSet<String>,
    pub(crate) security_sinks: Vec<SinkSite>,
    /// The component module that the markup snippet facts merge into. `None`
    /// for a scan that keeps only the usage fields above.
    markup: Option<Box<MarkupFactSink>>,
}

impl TemplateUsage {
    /// A usage scan whose markup snippets merge all their facts into `module`.
    pub(crate) fn with_markup_sink(context: ScriptBindingContext, module: ModuleInfo) -> Self {
        Self {
            markup: Some(Box::new(MarkupFactSink {
                context: std::sync::Arc::new(context),
                module: Box::new(module),
                anchor: 0,
                absorbed: FxHashSet::default(),
            })),
            ..Self::default()
        }
    }

    /// Move the markup sink of `other` into this usage. A scanner that starts
    /// a nested usage takes the sink first and gives it back with this call.
    pub(crate) fn take_markup_sink_from(&mut self, other: &mut Self) {
        if self.markup.is_none() {
            self.markup = other.markup.take();
        }
    }

    /// Set the source offset of the markup tag that holds the next snippets.
    pub(crate) fn set_markup_anchor(&mut self, offset: usize) {
        if let Some(sink) = self.markup.as_mut() {
            sink.anchor = u32::try_from(offset).unwrap_or(u32::MAX);
        }
    }

    /// Whether `name` is a VueUse model ref of the component script.
    pub(crate) fn names_model_ref(&self, name: &str) -> bool {
        self.markup
            .as_ref()
            .is_some_and(|sink| sink.context.model_ref_events.contains_key(name))
    }

    /// Give back the component module with the markup snippet facts merged.
    pub(crate) fn take_markup_module(&mut self) -> Option<ModuleInfo> {
        self.markup.take().map(|sink| *sink.module)
    }

    pub(crate) fn merge(&mut self, mut other: Self) {
        self.take_markup_sink_from(&mut other);
        self.used_bindings.extend(other.used_bindings);
        self.unresolved_tag_names.extend(other.unresolved_tag_names);
        self.security_sinks.extend(other.security_sinks);
        for access in other.member_accesses {
            let key = (&access.object, &access.member);
            let already_present = self
                .member_accesses
                .iter()
                .any(|existing| (&existing.object, &existing.member) == key);
            if !already_present {
                self.member_accesses.push(access);
            }
        }
        for whole in other.whole_object_uses {
            if !self
                .whole_object_uses
                .iter()
                .any(|existing| existing == &whole)
            {
                self.whole_object_uses.push(whole);
            }
        }
    }

    /// Whether the template mentions `name` other than as the object of a
    /// member access, for example in a shorthand attribute, a spread or a
    /// call argument.
    pub(crate) fn uses_whole(&self, name: &str) -> bool {
        self.whole_object_uses.iter().any(|whole| whole == name)
    }

    #[cfg(test)]
    pub(crate) fn is_empty(&self) -> bool {
        self.used_bindings.is_empty()
            && self.member_accesses.is_empty()
            && self.whole_object_uses.is_empty()
            && self.unresolved_tag_names.is_empty()
            && self.security_sinks.is_empty()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TemplateSnippetKind {
    Expression,
    Statement,
}

#[cfg(test)]
fn analyze_template_snippet(
    snippet: &str,
    kind: TemplateSnippetKind,
    imported_bindings: &FxHashSet<String>,
    locals: &[String],
    allow_dollar_prefixed_refs: bool,
) -> TemplateUsage {
    analyze_template_snippet_with_bound_targets(
        snippet,
        kind,
        imported_bindings,
        &FxHashMap::default(),
        locals,
        allow_dollar_prefixed_refs,
    )
}

#[cfg(test)]
fn analyze_template_snippet_with_bound_targets(
    snippet: &str,
    kind: TemplateSnippetKind,
    imported_bindings: &FxHashSet<String>,
    bound_targets: &FxHashMap<String, String>,
    locals: &[String],
    allow_dollar_prefixed_refs: bool,
) -> TemplateUsage {
    let mut usage = TemplateUsage::default();
    analyze_template_snippet_into(
        &mut usage,
        &TemplateSnippet {
            source: snippet,
            kind,
            imported_bindings,
            bound_targets,
            locals,
            allow_dollar_prefixed_refs,
        },
    );
    usage
}

/// One markup snippet and the template-visible names it can read.
pub struct TemplateSnippet<'a> {
    pub(crate) source: &'a str,
    pub(crate) kind: TemplateSnippetKind,
    pub(crate) imported_bindings: &'a FxHashSet<String>,
    pub(crate) bound_targets: &'a FxHashMap<String, String>,
    pub(crate) locals: &'a [String],
    pub(crate) allow_dollar_prefixed_refs: bool,
}

/// Analyze one markup snippet into `usage`. With a markup sink, the snippet
/// extractor starts from the script binding context of the component, and
/// every fact that is not script-only merges into the component module.
/// Without a sink, only the usage fields of [`TemplateUsage`] are kept.
pub fn analyze_template_snippet_into(usage: &mut TemplateUsage, input: &TemplateSnippet<'_>) {
    let snippet = input.source.trim();
    let has_sink = usage.markup.is_some();
    if snippet.is_empty()
        || (!has_sink && input.imported_bindings.is_empty() && input.bound_targets.is_empty())
    {
        return;
    }

    let locals = named_locals(snippet, input.locals);
    let wrapped = wrap_snippet(snippet, input.kind, &locals);
    if !crate::nesting::fragment_within_limits(&wrapped) {
        return;
    }
    let allocator = Allocator::default();
    let parser_return = Parser::new(&allocator, &wrapped, SourceType::ts()).parse();

    let semantic_ret = SemanticBuilder::new().build(&parser_return.program);
    let (used_bindings, unresolved_targets) = classify_unresolved_references(
        &semantic_ret,
        input.imported_bindings,
        input.bound_targets,
        input.allow_dollar_prefixed_refs,
    );

    if unresolved_targets.is_empty() && !has_sink {
        return;
    }

    let mut extractor = ModuleInfoExtractor::new();
    if let Some(sink) = usage.markup.as_mut() {
        if parser_return.fatal_error || !parser_return.diagnostics.is_empty() {
            sink.abstain_on_unparsed(snippet);
        }
        extractor.use_script_binding_context(std::sync::Arc::clone(&sink.context), &locals);
    }
    extractor.register_template_object_locals(used_bindings.iter().cloned());
    extractor.visit_program(&parser_return.program);

    extractor.flush_pending_namespace_whole_object_uses();
    let member_accesses = std::mem::take(&mut extractor.member_accesses);
    let whole_object_uses = std::mem::take(&mut extractor.whole_object_uses);
    if let Some(sink) = usage.markup.as_mut()
        && sink.first_absorb(snippet)
    {
        sink.absorb(extractor);
    }
    usage.merge(build_template_usage(
        used_bindings,
        &unresolved_targets,
        input.bound_targets,
        input.allow_dollar_prefixed_refs,
        member_accesses,
        whole_object_uses,
    ));
}

/// Split a parsed snippet's root unresolved references into the imported
/// bindings actually used and the full set of remappable target names
/// (imported bindings + bound targets, with optional `$`-prefix stripping).
fn classify_unresolved_references(
    semantic_ret: &oxc_semantic::SemanticBuilderReturn<'_>,
    imported_bindings: &FxHashSet<String>,
    bound_targets: &FxHashMap<String, String>,
    allow_dollar_prefixed_refs: bool,
) -> (FxHashSet<String>, FxHashSet<String>) {
    let mut used_bindings = FxHashSet::default();
    let mut unresolved_targets = FxHashSet::default();
    for name in semantic_ret
        .semantic
        .scoping()
        .root_unresolved_references()
        .keys()
    {
        let name = name.as_str();
        if imported_bindings.contains(name) {
            used_bindings.insert(name.to_string());
            unresolved_targets.insert(name.to_string());
            continue;
        }
        if bound_targets.contains_key(name) {
            unresolved_targets.insert(name.to_string());
            continue;
        }
        if allow_dollar_prefixed_refs && let Some(stripped) = name.strip_prefix('$') {
            if imported_bindings.contains(stripped) {
                used_bindings.insert(stripped.to_string());
                unresolved_targets.insert(stripped.to_string());
            } else if bound_targets.contains_key(stripped) {
                unresolved_targets.insert(stripped.to_string());
            }
        }
    }
    (used_bindings, unresolved_targets)
}

/// Build the [`TemplateUsage`] from the extracted member accesses / whole-object
/// uses, remapping each object name through the resolved target set.
fn build_template_usage(
    used_bindings: FxHashSet<String>,
    unresolved_targets: &FxHashSet<String>,
    bound_targets: &FxHashMap<String, String>,
    allow_dollar_prefixed_refs: bool,
    member_accesses: Vec<MemberAccess>,
    whole_object_uses: Vec<String>,
) -> TemplateUsage {
    TemplateUsage {
        used_bindings,
        member_accesses: dedup_member_accesses(
            member_accesses
                .into_iter()
                .filter_map(|access| {
                    remap_object_name(
                        &access.object,
                        unresolved_targets,
                        bound_targets,
                        allow_dollar_prefixed_refs,
                    )
                    .map(|object| MemberAccess {
                        object,
                        member: access.member,
                    })
                })
                .collect(),
        ),
        whole_object_uses: dedup_names(
            whole_object_uses
                .into_iter()
                .filter_map(|name| {
                    remap_object_name(
                        &name,
                        unresolved_targets,
                        bound_targets,
                        allow_dollar_prefixed_refs,
                    )
                })
                .collect(),
        ),
        unresolved_tag_names: FxHashSet::default(),
        security_sinks: Vec::new(),
        markup: None,
    }
}

/// Collect both unresolved identifier references AND static member-access chains
/// (`obj.member`) where `obj` is unresolved.
///
/// Unlike `analyze_template_snippet`, this does NOT filter against imported bindings.
/// Returns `(identifiers, member_accesses)`. Used by the Angular template scanner
/// for external templates where unresolved identifiers are potential component
/// class member refs and member-access chains (`dataService.getTotal()`) must
/// be resolved against the component's constructor-injected type bindings to
/// credit the target class's member as used. `locals` are the template locals
/// that the snippet names, as [`named_locals`] gives them.
pub fn collect_unresolved_refs_and_accesses(
    snippet: &str,
    kind: TemplateSnippetKind,
    locals: &[&str],
) -> (FxHashSet<String>, Vec<MemberAccess>) {
    let snippet = snippet.trim();
    if snippet.is_empty() {
        return (FxHashSet::default(), Vec::new());
    }

    let wrapped = wrap_snippet(snippet, kind, locals);
    if !crate::nesting::fragment_within_limits(&wrapped) {
        return (FxHashSet::default(), Vec::new());
    }
    let allocator = Allocator::default();
    let parser_return = Parser::new(&allocator, &wrapped, SourceType::ts()).parse();
    let semantic_ret = SemanticBuilder::new().build(&parser_return.program);

    let unresolved_names: FxHashSet<String> = semantic_ret
        .semantic
        .scoping()
        .root_unresolved_references()
        .keys()
        .map(|name| name.to_string())
        .collect();

    if unresolved_names.is_empty() {
        return (unresolved_names, Vec::new());
    }

    let mut extractor = ModuleInfoExtractor::new();
    extractor.visit_program(&parser_return.program);
    let member_accesses = dedup_member_accesses(
        extractor
            .member_accesses
            .into_iter()
            .filter(|access| unresolved_names.contains(&access.object))
            .collect(),
    );

    (unresolved_names, member_accesses)
}

/// The callee path prefix of a sink in template markup (`template.innerHTML`
/// for Vue `v-html`, Svelte `{@html}` and Angular `[innerHTML]`). The span of
/// such a sink covers its tag or attribute, not the exact sink expression.
pub const TEMPLATE_SINK_CALLEE_PREFIX: &str = "template.";

pub fn template_html_sink(snippet: &str, span_start: usize, span_end: usize) -> Option<SinkSite> {
    template_dom_sink("innerHTML", snippet, span_start, span_end)
}

/// A template binding of an iframe `srcdoc` (`:srcdoc`, `[srcdoc]`,
/// `srcdoc={x}`), captured as the `template.srcdoc` member-assign shape.
pub fn template_srcdoc_sink(snippet: &str, span_start: usize, span_end: usize) -> Option<SinkSite> {
    template_dom_sink("srcdoc", snippet, span_start, span_end)
}

fn template_dom_sink(
    property: &str,
    snippet: &str,
    span_start: usize,
    span_end: usize,
) -> Option<SinkSite> {
    let snippet = snippet.trim();
    if snippet.is_empty() {
        return None;
    }

    let parsed = with_parsed_template_expression(snippet, |expr| {
        if !is_non_literal_template_arg(expr) {
            return None;
        }
        Some((
            classify_template_arg_kind(expr),
            collect_template_arg_idents(expr),
        ))
    });
    let (arg_kind, arg_idents) = match parsed {
        Some(Some(parsed)) => parsed,
        Some(None) => return None,
        None => (SinkArgKind::Other, Vec::new()),
    };

    Some(SinkSite {
        sink_shape: SinkShape::MemberAssign,
        callee_path: format!("{TEMPLATE_SINK_CALLEE_PREFIX}{property}"),
        arg_index: 0,
        arg_is_non_literal: true,
        arg_kind,
        arg_literal: None,
        regex_pattern: None,
        object_properties: Vec::new(),
        object_property_keys: Vec::new(),
        object_property_keys_complete: false,
        arg_idents,
        arg_source_paths: Vec::new(),
        span_start: u32::try_from(span_start).ok()?,
        span_end: u32::try_from(span_end).ok()?,
        url_arg_literal: None,
        url_shape: None,
        call_options: None,
    })
}

fn with_parsed_template_expression<R>(
    snippet: &str,
    body: impl FnOnce(&Expression<'_>) -> R,
) -> Option<R> {
    let snippet = snippet.trim();
    if snippet.is_empty() {
        return None;
    }
    let wrapped = format!("const __f_tpl_sink = ({snippet});");
    if !crate::nesting::fragment_within_limits(&wrapped) {
        return None;
    }
    let allocator = Allocator::default();
    let parser_return = Parser::new(&allocator, &wrapped, SourceType::ts()).parse();
    let Statement::VariableDeclaration(decl) = parser_return.program.body.first()? else {
        return None;
    };
    let expr = decl.declarations.first()?.init.as_ref()?;
    Some(body(expr))
}

fn is_non_literal_template_arg(expr: &Expression<'_>) -> bool {
    match unwrap_template_parens(expr) {
        Expression::StringLiteral(_)
        | Expression::NumericLiteral(_)
        | Expression::BooleanLiteral(_)
        | Expression::NullLiteral(_)
        | Expression::BigIntLiteral(_)
        | Expression::RegExpLiteral(_) => false,
        Expression::TemplateLiteral(tpl) => !tpl.expressions.is_empty(),
        _ => true,
    }
}

fn classify_template_arg_kind(expr: &Expression<'_>) -> SinkArgKind {
    match unwrap_template_parens(expr) {
        Expression::TemplateLiteral(_) => SinkArgKind::TemplateWithSubst,
        Expression::BinaryExpression(bin) if bin.operator == BinaryOperator::Addition => {
            SinkArgKind::Concat
        }
        Expression::ObjectExpression(_) => SinkArgKind::Object,
        Expression::CallExpression(_) => SinkArgKind::Call,
        _ => SinkArgKind::Other,
    }
}

fn collect_template_arg_idents(expr: &Expression<'_>) -> Vec<String> {
    let mut out = Vec::new();
    collect_template_idents_into(expr, &mut out);
    out
}

fn push_template_ident(name: &str, out: &mut Vec<String>) {
    if !out.iter().any(|existing| existing == name) {
        out.push(name.to_string());
    }
}

fn collect_template_idents_into(expr: &Expression<'_>, out: &mut Vec<String>) {
    match expr {
        Expression::Identifier(ident) => push_template_ident(&ident.name, out),
        Expression::ParenthesizedExpression(paren) => {
            collect_template_idents_into(&paren.expression, out);
        }
        Expression::StaticMemberExpression(member) => {
            collect_template_idents_into(&member.object, out);
        }
        Expression::ComputedMemberExpression(member) => {
            collect_template_pair_idents(&member.object, &member.expression, out);
        }
        Expression::BinaryExpression(bin) => {
            collect_template_pair_idents(&bin.left, &bin.right, out);
        }
        Expression::LogicalExpression(logical) => {
            collect_template_pair_idents(&logical.left, &logical.right, out);
        }
        Expression::ConditionalExpression(cond) => {
            collect_template_conditional_idents(&cond.test, &cond.consequent, &cond.alternate, out);
        }
        Expression::SequenceExpression(seq) => {
            for expr in &seq.expressions {
                collect_template_idents_into(expr, out);
            }
        }
        Expression::TemplateLiteral(tpl) => {
            for expr in &tpl.expressions {
                collect_template_idents_into(expr, out);
            }
        }
        Expression::AwaitExpression(await_expr) => {
            collect_template_idents_into(&await_expr.argument, out);
        }
        Expression::UnaryExpression(unary) => {
            collect_template_idents_into(&unary.argument, out);
        }
        Expression::CallExpression(call) => {
            collect_template_call_idents(call, out);
        }
        Expression::ObjectExpression(obj) => {
            collect_template_object_idents(obj, out);
        }
        _ => {}
    }
}

fn collect_template_pair_idents(
    left: &Expression<'_>,
    right: &Expression<'_>,
    out: &mut Vec<String>,
) {
    collect_template_idents_into(left, out);
    collect_template_idents_into(right, out);
}

fn collect_template_conditional_idents(
    test: &Expression<'_>,
    consequent: &Expression<'_>,
    alternate: &Expression<'_>,
    out: &mut Vec<String>,
) {
    collect_template_idents_into(test, out);
    collect_template_idents_into(consequent, out);
    collect_template_idents_into(alternate, out);
}

fn collect_template_call_idents(call: &CallExpression<'_>, out: &mut Vec<String>) {
    collect_template_idents_into(&call.callee, out);
    for arg in &call.arguments {
        if let Some(arg_expr) = arg.as_expression() {
            collect_template_idents_into(arg_expr, out);
        }
    }
}

fn collect_template_object_idents(obj: &ObjectExpression<'_>, out: &mut Vec<String>) {
    for prop in &obj.properties {
        if let ObjectPropertyKind::ObjectProperty(prop) = prop {
            collect_template_idents_into(&prop.value, out);
        }
    }
}

fn unwrap_template_parens<'a>(mut expr: &'a Expression<'a>) -> &'a Expression<'a> {
    while let Expression::ParenthesizedExpression(paren) = expr {
        expr = &paren.expression;
    }
    expr
}

fn remap_object_name(
    name: &str,
    unresolved_names: &FxHashSet<String>,
    bound_targets: &FxHashMap<String, String>,
    allow_dollar_prefixed_refs: bool,
) -> Option<String> {
    if unresolved_names.contains(name) {
        if let Some(target) = bound_targets.get(name) {
            return Some(target.clone());
        }
        return Some(name.to_string());
    }
    if allow_dollar_prefixed_refs
        && let Some(stripped) = name.strip_prefix('$')
        && unresolved_names.contains(stripped)
    {
        if let Some(target) = bound_targets.get(stripped) {
            return Some(target.clone());
        }
        return Some(stripped.to_string());
    }
    // unused-load-data-key Primitive C: the SvelteKit global page store read in a
    // template, `$page.data.KEY` (Svelte 4 `$app/stores`) or `page.data.KEY`
    // (Svelte 5 `$app/state`), arrives here as the dotted member object
    // `$page.data` / `page.data` (the object of the outer `.KEY` access). The
    // root-only logic above remaps `$page`/`page` but drops the `.data` suffix,
    // so the consumed key is lost. Recover the canonical `page.data` object (the
    // `$`-stripped form, matching how the root branch normalizes `$page` -> `page`)
    // so the cross-file detector can see the project-wide `page.data.<key>`
    // consumer channel. Scoped to the `page` store's `data` member only (gated on
    // the Svelte dollar-ref path) to stay byte-identical for every other dotted
    // template member object; `$page.url.pathname` etc. still map root-only.
    if allow_dollar_prefixed_refs
        && (name == "page.data" || name == "$page.data")
        && unresolved_names.contains("page")
    {
        return Some("page.data".to_string());
    }
    None
}

/// The locals in `locals` that `snippet` names, in order.
///
/// Each local becomes a declaration in the wrapped snippet, and a template
/// can hold many snippets inside a scope with many locals. A local that the
/// snippet does not name cannot change how its references resolve, so only
/// the named locals are declared and passed to the markup extractor. A Svelte `$name` store read names `name`.
/// A snippet with a non-ASCII character or a backslash (a Unicode escape in
/// an identifier) keeps every local, because the word scan below reads ASCII
/// identifiers only.
pub fn named_locals<'l>(snippet: &str, locals: &'l [String]) -> Vec<&'l str> {
    if locals.is_empty() {
        return Vec::new();
    }
    if !snippet.is_ascii() || snippet.contains('\\') {
        return locals.iter().map(String::as_str).collect();
    }
    let mut words: FxHashSet<&str> = FxHashSet::default();
    for word in snippet
        .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '$'))
        .filter(|word| !word.is_empty())
    {
        words.insert(word);
        if let Some(store) = word.strip_prefix('$') {
            words.insert(store);
        }
    }
    locals
        .iter()
        .map(String::as_str)
        .filter(|local| words.contains(local))
        .collect()
}

fn wrap_snippet(snippet: &str, kind: TemplateSnippetKind, locals: &[&str]) -> String {
    let mut wrapped = String::new();
    if !locals.is_empty() {
        wrapped.push_str("const __f_tpl_local = undefined;\n");
        for local in locals {
            wrapped.push_str("const ");
            wrapped.push_str(local);
            wrapped.push_str(" = __f_tpl_local;\n");
        }
    }

    match kind {
        TemplateSnippetKind::Expression => {
            wrapped.push_str("void (");
            wrapped.push_str(snippet);
            wrapped.push_str(");\n");
        }
        TemplateSnippetKind::Statement => {
            wrapped.push_str("(() => {\n");
            wrapped.push_str(snippet);
            wrapped.push_str("\n})();\n");
        }
    }

    wrapped
}

fn dedup_member_accesses(accesses: Vec<MemberAccess>) -> Vec<MemberAccess> {
    let mut seen: FxHashSet<(String, String)> = FxHashSet::default();
    let mut deduped = Vec::with_capacity(accesses.len());
    for access in accesses {
        let key = (access.object.clone(), access.member.clone());
        if seen.insert(key) {
            deduped.push(access);
        }
    }
    deduped
}

fn dedup_names(names: Vec<String>) -> Vec<String> {
    let mut seen: FxHashSet<String> = FxHashSet::default();
    let mut deduped = Vec::with_capacity(names.len());
    for name in names {
        if seen.insert(name.clone()) {
            deduped.push(name);
        }
    }
    deduped
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bindings(names: &[&str]) -> FxHashSet<String> {
        names.iter().map(|name| (*name).to_string()).collect()
    }

    #[test]
    fn expression_usage_tracks_named_bindings() {
        let usage = analyze_template_snippet(
            "formatDate(user.createdAt)",
            TemplateSnippetKind::Expression,
            &bindings(&["formatDate"]),
            &[],
            false,
        );

        assert!(usage.used_bindings.contains("formatDate"));
        assert!(usage.member_accesses.is_empty());
        assert!(usage.whole_object_uses.is_empty());
    }

    #[test]
    fn expression_usage_tracks_namespace_members() {
        let usage = analyze_template_snippet(
            "utils.formatDate(user.createdAt)",
            TemplateSnippetKind::Expression,
            &bindings(&["utils"]),
            &[],
            false,
        );

        assert!(usage.used_bindings.contains("utils"));
        assert_eq!(usage.member_accesses.len(), 1);
        assert_eq!(usage.member_accesses[0].object, "utils");
        assert_eq!(usage.member_accesses[0].member, "formatDate");
    }

    #[test]
    fn locals_shadow_imported_names() {
        let usage = analyze_template_snippet(
            "item.name",
            TemplateSnippetKind::Expression,
            &bindings(&["item"]),
            &["item".to_string()],
            false,
        );

        assert!(usage.is_empty());
    }

    #[test]
    fn named_locals_keeps_the_locals_that_the_snippet_names() {
        let locals = ["other", "item", "count", "item2"].map(str::to_string);
        assert_eq!(
            named_locals("$item.name + count(item2)", &locals),
            ["item", "count", "item2"]
        );
        assert!(named_locals("helper(1)", &locals).is_empty());
        assert_eq!(named_locals("caf\u{e9}(item)", &locals), locals);
        assert_eq!(named_locals("\\u0069tem", &locals), locals);
    }

    #[test]
    fn statement_usage_tracks_handler_references() {
        let usage = analyze_template_snippet(
            "count += increment(step);",
            TemplateSnippetKind::Statement,
            &bindings(&["increment"]),
            &[],
            false,
        );

        assert!(usage.used_bindings.contains("increment"));
    }

    #[test]
    fn dollar_prefixed_refs_can_map_to_imported_store_bindings() {
        let usage = analyze_template_snippet(
            "$page.url.pathname",
            TemplateSnippetKind::Expression,
            &bindings(&["page"]),
            &[],
            true,
        );

        assert!(usage.used_bindings.contains("page"));
        assert_eq!(usage.member_accesses.len(), 1);
        assert_eq!(usage.member_accesses[0].object, "page");
        assert_eq!(usage.member_accesses[0].member, "url");
    }

    // unused-load-data-key Primitive C: the SvelteKit global page store read in a
    // template recovers the nested `page.data.<key>` member access.

    #[test]
    fn svelte4_page_store_data_key_recovers_nested_member() {
        let usage = analyze_template_snippet(
            "$page.data.user",
            TemplateSnippetKind::Expression,
            &bindings(&["page"]),
            &[],
            true,
        );

        assert!(
            usage
                .member_accesses
                .iter()
                .any(|a| a.object == "page.data" && a.member == "user"),
            "`$page.data.user` should recover `page.data.user`, got: {:?}",
            usage.member_accesses
        );
    }

    #[test]
    fn svelte5_page_state_data_key_recovers_nested_member() {
        let usage = analyze_template_snippet(
            "page.data.session",
            TemplateSnippetKind::Expression,
            &bindings(&["page"]),
            &[],
            true,
        );

        assert!(
            usage
                .member_accesses
                .iter()
                .any(|a| a.object == "page.data" && a.member == "session"),
            "`page.data.session` should recover `page.data.session`, got: {:?}",
            usage.member_accesses
        );
    }

    #[test]
    fn page_store_non_data_member_stays_root_only() {
        // `$page.url.pathname` must keep mapping to root `page.url` -> `{page, url}`;
        // only the `data` channel recovers the nested key. Regression guard for the
        // existing store-ref behavior.
        let usage = analyze_template_snippet(
            "$page.url.pathname",
            TemplateSnippetKind::Expression,
            &bindings(&["page"]),
            &[],
            true,
        );

        assert!(
            !usage.member_accesses.iter().any(|a| a.object == "page.url"),
            "`$page.url` must not recover a nested member, got: {:?}",
            usage.member_accesses
        );
    }

    #[test]
    fn page_data_nested_recovery_is_svelte_only() {
        // Vue templates pass allow_dollar_prefixed_refs=false; the `page` store is
        // SvelteKit-specific, so `page.data.x` must NOT recover the nested member
        // in a Vue context (root `{page, data}` is still credited).
        let usage = analyze_template_snippet(
            "page.data.x",
            TemplateSnippetKind::Expression,
            &bindings(&["page"]),
            &[],
            false,
        );

        assert!(
            !usage
                .member_accesses
                .iter()
                .any(|a| a.object == "page.data"),
            "Vue context must not recover `page.data`, got: {:?}",
            usage.member_accesses
        );
    }
}
