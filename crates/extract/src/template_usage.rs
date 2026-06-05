use fallow_types::extract::{SinkArgKind, SinkShape, SinkSite};
use oxc_allocator::Allocator;
use oxc_ast::ast::{BinaryOperator, Expression, ObjectPropertyKind, Statement};
use oxc_ast_visit::Visit;
use oxc_parser::Parser;
use oxc_semantic::SemanticBuilder;
use oxc_span::SourceType;
use rustc_hash::{FxHashMap, FxHashSet};

use crate::MemberAccess;
use crate::visitor::ModuleInfoExtractor;

#[derive(Debug, Default, Clone)]
pub struct TemplateUsage {
    pub(crate) used_bindings: FxHashSet<String>,
    pub(crate) member_accesses: Vec<MemberAccess>,
    pub(crate) whole_object_uses: Vec<String>,
    /// PascalCase tag names that matched no import or local binding.
    pub(crate) unresolved_tag_names: FxHashSet<String>,
    pub(crate) security_sinks: Vec<SinkSite>,
}

impl TemplateUsage {
    pub(crate) fn merge(&mut self, other: Self) {
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

pub fn analyze_template_snippet(
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

pub fn analyze_template_snippet_with_bound_targets(
    snippet: &str,
    kind: TemplateSnippetKind,
    imported_bindings: &FxHashSet<String>,
    bound_targets: &FxHashMap<String, String>,
    locals: &[String],
    allow_dollar_prefixed_refs: bool,
) -> TemplateUsage {
    let snippet = snippet.trim();
    if snippet.is_empty() || (imported_bindings.is_empty() && bound_targets.is_empty()) {
        return TemplateUsage::default();
    }

    let wrapped = wrap_snippet(snippet, kind, locals);
    let allocator = Allocator::default();
    let parser_return = Parser::new(&allocator, &wrapped, SourceType::ts()).parse();

    let semantic_ret = SemanticBuilder::new().build(&parser_return.program);
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

    if unresolved_targets.is_empty() {
        return TemplateUsage::default();
    }

    let mut extractor = ModuleInfoExtractor::new();
    extractor.visit_program(&parser_return.program);

    TemplateUsage {
        used_bindings,
        member_accesses: dedup_member_accesses(
            extractor
                .member_accesses
                .into_iter()
                .filter_map(|access| {
                    remap_object_name(
                        &access.object,
                        &unresolved_targets,
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
            extractor
                .whole_object_uses
                .into_iter()
                .filter_map(|name| {
                    remap_object_name(
                        &name,
                        &unresolved_targets,
                        bound_targets,
                        allow_dollar_prefixed_refs,
                    )
                })
                .collect(),
        ),
        unresolved_tag_names: FxHashSet::default(),
        security_sinks: Vec::new(),
    }
}

/// Collect both unresolved identifier references AND static member-access chains
/// (`obj.member`) where `obj` is unresolved.
///
/// Unlike [`analyze_template_snippet`], this does NOT filter against imported bindings.
/// Returns `(identifiers, member_accesses)`. Used by the Angular template scanner
/// for external templates where unresolved identifiers are potential component
/// class member refs and member-access chains (`dataService.getTotal()`) must
/// be resolved against the component's constructor-injected type bindings to
/// credit the target class's member as used.
pub fn collect_unresolved_refs_and_accesses(
    snippet: &str,
    kind: TemplateSnippetKind,
    locals: &[String],
) -> (FxHashSet<String>, Vec<MemberAccess>) {
    let snippet = snippet.trim();
    if snippet.is_empty() {
        return (FxHashSet::default(), Vec::new());
    }

    let wrapped = wrap_snippet(snippet, kind, locals);
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

pub fn template_html_sink(snippet: &str, span_start: usize, span_end: usize) -> Option<SinkSite> {
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
        callee_path: "template.innerHTML".to_string(),
        arg_index: 0,
        arg_is_non_literal: true,
        arg_kind,
        arg_idents,
        span_start: u32::try_from(span_start).ok()?,
        span_end: u32::try_from(span_end).ok()?,
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
    let wrapped = format!("const __fallow_template_sink = ({snippet});");
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
            collect_template_idents_into(&member.object, out);
            collect_template_idents_into(&member.expression, out);
        }
        Expression::BinaryExpression(bin) => {
            collect_template_idents_into(&bin.left, out);
            collect_template_idents_into(&bin.right, out);
        }
        Expression::LogicalExpression(logical) => {
            collect_template_idents_into(&logical.left, out);
            collect_template_idents_into(&logical.right, out);
        }
        Expression::ConditionalExpression(cond) => {
            collect_template_idents_into(&cond.test, out);
            collect_template_idents_into(&cond.consequent, out);
            collect_template_idents_into(&cond.alternate, out);
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
            collect_template_idents_into(&call.callee, out);
            for arg in &call.arguments {
                if let Some(arg_expr) = arg.as_expression() {
                    collect_template_idents_into(arg_expr, out);
                }
            }
        }
        Expression::ObjectExpression(obj) => {
            for prop in &obj.properties {
                if let ObjectPropertyKind::ObjectProperty(prop) = prop {
                    collect_template_idents_into(&prop.value, out);
                }
            }
        }
        _ => {}
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
    None
}

fn wrap_snippet(snippet: &str, kind: TemplateSnippetKind, locals: &[String]) -> String {
    let mut wrapped = String::new();
    if !locals.is_empty() {
        wrapped.push_str("const __fallow_local = undefined;\n");
        for local in locals {
            wrapped.push_str("const ");
            wrapped.push_str(local);
            wrapped.push_str(" = __fallow_local;\n");
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
}
