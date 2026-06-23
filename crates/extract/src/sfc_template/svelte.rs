use std::sync::LazyLock;

use rustc_hash::{FxHashMap, FxHashSet};

use crate::template_usage::TemplateUsage;

use super::scanners::{scan_curly_section, scan_html_tag};
use super::shared::{
    HTML_COMMENT_RE, ParsedAttr, extract_pattern_binding_names, merge_component_tag_usage,
    merge_expression_usage_allow_dollar_refs_with_bound_targets,
    merge_statement_usage_allow_dollar_refs_with_bound_targets, parse_tag_attrs,
};

static STYLE_BLOCK_RE: LazyLock<regex::Regex> = LazyLock::new(|| {
    crate::static_regex(r#"(?is)<style\b(?:[^>"']|"[^"]*"|'[^']*')*>(?P<body>[\s\S]*?)</style>"#)
});

static SCRIPT_BLOCK_RE: LazyLock<regex::Regex> = LazyLock::new(|| {
    crate::static_regex(r#"(?is)<script\b(?:[^>"']|"[^"]*"|'[^']*')*>(?P<body>[\s\S]*?)</script>"#)
});

static SVELTE_EACH_RE: LazyLock<regex::Regex> = LazyLock::new(|| {
    crate::static_regex(
        r"(?is)^#each\s+(?P<iterable>.+?)\s+as\s+(?P<bindings>.+?)(?:\s*\((?P<key>.+)\))?$",
    )
});

static SVELTE_AWAIT_RE: LazyLock<regex::Regex> =
    LazyLock::new(|| crate::static_regex(r"(?is)^#await\s+(?P<expr>.+)$"));

static SVELTE_THEN_RE: LazyLock<regex::Regex> =
    LazyLock::new(|| crate::static_regex(r"(?is)^:then(?:\s+(?P<binding>.+))?$"));

static SVELTE_CATCH_RE: LazyLock<regex::Regex> =
    LazyLock::new(|| crate::static_regex(r"(?is)^:catch(?:\s+(?P<binding>.+))?$"));

static SVELTE_SNIPPET_RE: LazyLock<regex::Regex> = LazyLock::new(|| {
    crate::static_regex(r"(?is)^#snippet\s+[A-Za-z_$][\w$]*\s*\((?P<params>.*)\)\s*$")
});

#[derive(Debug, Clone, PartialEq, Eq)]
enum SvelteBlockKind {
    Root,
    If,
    Each,
    Await,
    Key,
    Snippet,
    Element,
}

const VOID_HTML_TAGS: &[&str] = &[
    "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param", "source",
    "track", "wbr",
];

#[derive(Debug, Clone)]
struct SvelteScopeFrame {
    kind: SvelteBlockKind,
    locals: Vec<String>,
}

#[cfg(test)]
pub(super) fn collect_template_usage(
    source: &str,
    imported_bindings: &FxHashSet<String>,
) -> TemplateUsage {
    collect_template_usage_with_bound_targets(source, imported_bindings, &FxHashMap::default())
}

pub(super) fn collect_template_usage_with_bound_targets(
    source: &str,
    imported_bindings: &FxHashSet<String>,
    bound_targets: &FxHashMap<String, String>,
) -> TemplateUsage {
    let markup = strip_non_template_content(source);
    if markup.is_empty() {
        return TemplateUsage::default();
    }

    SvelteTemplateUsageScanner::new(&markup, imported_bindings, bound_targets).scan()
}

struct SvelteTemplateUsageScanner<'a> {
    markup: &'a str,
    imported_bindings: &'a FxHashSet<String>,
    bound_targets: &'a FxHashMap<String, String>,
    usage: TemplateUsage,
    scopes: Vec<SvelteScopeFrame>,
}

impl<'a> SvelteTemplateUsageScanner<'a> {
    fn new(
        markup: &'a str,
        imported_bindings: &'a FxHashSet<String>,
        bound_targets: &'a FxHashMap<String, String>,
    ) -> Self {
        Self {
            markup,
            imported_bindings,
            bound_targets,
            usage: TemplateUsage::default(),
            scopes: vec![SvelteScopeFrame {
                kind: SvelteBlockKind::Root,
                locals: Vec::new(),
            }],
        }
    }

    fn scan(mut self) -> TemplateUsage {
        let mut index = 0;
        while index < self.markup.len() {
            index = self.scan_next(index);
        }
        self.usage
    }

    fn scan_next(&mut self, index: usize) -> usize {
        match self.markup.as_bytes()[index] {
            b'{' => self.scan_svelte_tag(index),
            b'<' => self.scan_markup_tag(index),
            _ => index + 1,
        }
    }

    fn scan_svelte_tag(&mut self, index: usize) -> usize {
        let Some((tag, next_index)) = scan_curly_section(self.markup, index, 1, 1) else {
            return self.markup.len();
        };
        apply_tag(&mut SvelteTagInput {
            tag: tag.trim(),
            tag_start: index,
            tag_end: next_index,
            imported_bindings: self.imported_bindings,
            bound_targets: self.bound_targets,
            scopes: &mut self.scopes,
            usage: &mut self.usage,
        });
        next_index
    }

    fn scan_markup_tag(&mut self, index: usize) -> usize {
        let Some((tag, next_index)) = scan_html_tag(self.markup, index) else {
            return self.markup.len();
        };
        apply_markup_tag(
            tag,
            self.imported_bindings,
            self.bound_targets,
            &mut self.scopes,
            &mut self.usage,
        );
        next_index
    }
}

/// Collect Svelte custom-event listener names from template `on:<name>`
/// bindings on COMPONENT tags (PascalCase or member-expression tag names).
///
/// `on:<name>` on a lowercase DOM element (`on:click` on a `<button>`) is a DOM
/// event, NOT a custom event, so it is excluded. Event forwarding (`on:save`
/// with no value) still counts as a listen (the parent forwards the child's
/// event upward, so the name IS listened for). Reuses the same markup tag
/// scanning (`scan_html_tag` + `parse_tag_attrs`) as the usage scanner so
/// component-tag detection and attribute parsing stay consistent. The result
/// feeds the `unused-svelte-event` detector's liberal project-wide listened set.
pub(super) fn collect_listened_events(source: &str) -> Vec<String> {
    let markup = strip_non_template_content(source);
    if markup.is_empty() {
        return Vec::new();
    }

    let mut listened: Vec<String> = Vec::new();
    let bytes = markup.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'<' {
            let Some((tag, next_index)) = scan_html_tag(&markup, index) else {
                break;
            };
            collect_tag_listeners(tag, &mut listened);
            index = next_index;
        } else {
            index += 1;
        }
    }

    listened.sort_unstable();
    listened.dedup();
    listened
}

/// Record `on:<name>` listener names on a single tag when the tag names a
/// COMPONENT (PascalCase, or a dotted member-expression like `Icons.Alert`).
fn collect_tag_listeners(tag: &str, listened: &mut Vec<String>) {
    let trimmed = tag.trim();
    if trimmed.starts_with("</") || trimmed.starts_with("<!") || trimmed.starts_with("<?") {
        return;
    }
    let parsed = parse_tag_attrs(trimmed, true);
    if parsed.name.is_empty() {
        return;
    }
    let is_component = parsed.name.contains('.')
        || parsed
            .name
            .chars()
            .next()
            .is_some_and(|ch| ch.is_ascii_uppercase());
    if !is_component {
        return;
    }
    for attr in &parsed.attrs {
        if let Some(event) = attr.name.strip_prefix("on:") {
            // Strip event modifiers (`on:click|preventDefault`); the event name
            // is the segment before the first `|`.
            let name = event.split('|').next().unwrap_or(event).trim();
            if !name.is_empty() {
                listened.push(name.to_string());
            }
        }
    }
}

fn strip_non_template_content(source: &str) -> String {
    let mut hidden_ranges: Vec<(usize, usize)> = Vec::new();
    hidden_ranges.extend(
        HTML_COMMENT_RE
            .find_iter(source)
            .map(|m| (m.start(), m.end())),
    );
    hidden_ranges.extend(
        SCRIPT_BLOCK_RE
            .find_iter(source)
            .map(|m| (m.start(), m.end())),
    );
    hidden_ranges.extend(
        STYLE_BLOCK_RE
            .find_iter(source)
            .map(|m| (m.start(), m.end())),
    );
    hidden_ranges.sort_unstable_by_key(|range| range.0);

    let mut merged: Vec<(usize, usize)> = Vec::with_capacity(hidden_ranges.len());
    for (start, end) in hidden_ranges {
        if let Some((_, last_end)) = merged.last_mut()
            && start <= *last_end
        {
            *last_end = (*last_end).max(end);
            continue;
        }
        merged.push((start, end));
    }

    let mut visible = String::with_capacity(source.len());
    let mut cursor = 0;
    for (start, end) in merged {
        if cursor < start {
            visible.push_str(&source[cursor..start]);
        }
        visible.extend(std::iter::repeat_n(' ', end - start));
        cursor = end;
    }
    if cursor < source.len() {
        visible.push_str(&source[cursor..]);
    }
    visible
}

struct SvelteTagInput<'a> {
    tag: &'a str,
    tag_start: usize,
    tag_end: usize,
    imported_bindings: &'a FxHashSet<String>,
    bound_targets: &'a FxHashMap<String, String>,
    scopes: &'a mut Vec<SvelteScopeFrame>,
    usage: &'a mut TemplateUsage,
}

fn apply_tag(input: &mut SvelteTagInput<'_>) {
    if input.tag.is_empty() {
        return;
    }

    if apply_svelte_block_tag(
        input.tag,
        input.imported_bindings,
        input.bound_targets,
        input.scopes,
        input.usage,
    ) {
        return;
    }

    if apply_svelte_expression_directive(&mut SvelteExpressionDirectiveInput {
        tag: input.tag,
        tag_start: input.tag_start,
        tag_end: input.tag_end,
        imported_bindings: input.imported_bindings,
        bound_targets: input.bound_targets,
        scopes: input.scopes,
        usage: input.usage,
    }) {
        return;
    }

    merge_expression_usage_allow_dollar_refs_with_bound_targets(
        input.usage,
        input.tag,
        input.imported_bindings,
        input.bound_targets,
        &current_locals(input.scopes),
    );
}

fn apply_svelte_block_tag(
    tag: &str,
    imported_bindings: &FxHashSet<String>,
    bound_targets: &FxHashMap<String, String>,
    scopes: &mut Vec<SvelteScopeFrame>,
    usage: &mut TemplateUsage,
) -> bool {
    if let Some(rest) = tag.strip_prefix('/') {
        pop_scope(scopes, rest.trim());
        return true;
    }

    if let Some(expr) = tag.strip_prefix("#if") {
        merge_expr_and_open_block(
            expr,
            SvelteBlockKind::If,
            imported_bindings,
            bound_targets,
            scopes,
            usage,
        );
        return true;
    }

    if let Some(captures) = SVELTE_EACH_RE.captures(tag) {
        apply_each_tag(&captures, imported_bindings, bound_targets, scopes, usage);
        return true;
    }

    if let Some(captures) = SVELTE_AWAIT_RE.captures(tag) {
        apply_await_tag(&captures, imported_bindings, bound_targets, scopes, usage);
        return true;
    }

    // `{:then binding}` / `{:catch binding}` both rebind the await frame's locals.
    if let Some(captures) = SVELTE_THEN_RE
        .captures(tag)
        .or_else(|| SVELTE_CATCH_RE.captures(tag))
    {
        update_await_branch_locals(&captures, scopes);
        return true;
    }

    if let Some(expr) = tag.strip_prefix("#key") {
        merge_expr_and_open_block(
            expr,
            SvelteBlockKind::Key,
            imported_bindings,
            bound_targets,
            scopes,
            usage,
        );
        return true;
    }

    if let Some(captures) = SVELTE_SNIPPET_RE.captures(tag) {
        let params = captures.name("params").map_or("", |m| m.as_str());
        scopes.push(SvelteScopeFrame {
            kind: SvelteBlockKind::Snippet,
            locals: extract_pattern_binding_names(params),
        });
        return true;
    }

    false
}

/// Merge a block opener's condition expression, then push a new scope frame of
/// the given kind (`{#if expr}`, `{#key expr}`).
fn merge_expr_and_open_block(
    expr: &str,
    kind: SvelteBlockKind,
    imported_bindings: &FxHashSet<String>,
    bound_targets: &FxHashMap<String, String>,
    scopes: &mut Vec<SvelteScopeFrame>,
    usage: &mut TemplateUsage,
) {
    merge_expression_usage_allow_dollar_refs_with_bound_targets(
        usage,
        expr.trim(),
        imported_bindings,
        bound_targets,
        &current_locals(scopes),
    );
    scopes.push(SvelteScopeFrame {
        kind,
        locals: Vec::new(),
    });
}

struct SvelteExpressionDirectiveInput<'a> {
    tag: &'a str,
    tag_start: usize,
    tag_end: usize,
    imported_bindings: &'a FxHashSet<String>,
    bound_targets: &'a FxHashMap<String, String>,
    scopes: &'a mut [SvelteScopeFrame],
    usage: &'a mut TemplateUsage,
}

fn apply_svelte_expression_directive(input: &mut SvelteExpressionDirectiveInput<'_>) -> bool {
    if let Some(expr) = input.tag.strip_prefix("@attach") {
        apply_directive_expression(input, expr);
        return true;
    }

    if let Some(expr) = input.tag.strip_prefix("@html") {
        apply_html_directive(input, expr);
        return true;
    }

    if let Some(expr) = input.tag.strip_prefix("@render") {
        apply_directive_expression(input, expr);
        return true;
    }

    if let Some(stmt) = input.tag.strip_prefix("@const") {
        apply_const_tag(
            stmt,
            input.imported_bindings,
            input.bound_targets,
            input.scopes,
            input.usage,
        );
        return true;
    }

    if let Some(expr) = input.tag.strip_prefix("@debug") {
        apply_directive_expression(input, expr);
        return true;
    }

    if let Some(expr) = input.tag.strip_prefix(":else if") {
        apply_directive_expression(input, expr);
        return true;
    }

    if input.tag.starts_with(":else") {
        return true;
    }

    false
}

/// Merge a directive's expression operand into template usage.
fn apply_directive_expression(input: &mut SvelteExpressionDirectiveInput<'_>, expr: &str) {
    apply_expression_tag(
        expr,
        input.imported_bindings,
        input.bound_targets,
        input.scopes,
        input.usage,
    );
}

/// Record a `{@html expr}` security sink, then merge the expression usage.
fn apply_html_directive(input: &mut SvelteExpressionDirectiveInput<'_>, expr: &str) {
    if let Some(sink) =
        crate::template_usage::template_html_sink(expr, input.tag_start, input.tag_end)
    {
        input.usage.security_sinks.push(sink);
    }
    merge_expression_usage_allow_dollar_refs_with_bound_targets(
        input.usage,
        expr.trim(),
        input.imported_bindings,
        input.bound_targets,
        &current_locals(input.scopes),
    );
}

fn apply_each_tag(
    captures: &regex::Captures<'_>,
    imported_bindings: &FxHashSet<String>,
    bound_targets: &FxHashMap<String, String>,
    scopes: &mut Vec<SvelteScopeFrame>,
    usage: &mut TemplateUsage,
) {
    let iterable = captures.name("iterable").map_or("", |m| m.as_str()).trim();
    let bindings = captures.name("bindings").map_or("", |m| m.as_str()).trim();
    let each_locals = extract_pattern_binding_names(bindings);
    let current = current_locals(scopes);
    merge_expression_usage_allow_dollar_refs_with_bound_targets(
        usage,
        iterable,
        imported_bindings,
        bound_targets,
        &current,
    );
    if let Some(key) = captures.name("key").map(|m| m.as_str().trim())
        && !key.is_empty()
    {
        let mut key_locals = current;
        key_locals.extend(each_locals.iter().cloned());
        merge_expression_usage_allow_dollar_refs_with_bound_targets(
            usage,
            key,
            imported_bindings,
            bound_targets,
            &key_locals,
        );
    }
    scopes.push(SvelteScopeFrame {
        kind: SvelteBlockKind::Each,
        locals: each_locals,
    });
}

fn apply_await_tag(
    captures: &regex::Captures<'_>,
    imported_bindings: &FxHashSet<String>,
    bound_targets: &FxHashMap<String, String>,
    scopes: &mut Vec<SvelteScopeFrame>,
    usage: &mut TemplateUsage,
) {
    let expr = captures.name("expr").map_or("", |m| m.as_str()).trim();
    merge_expression_usage_allow_dollar_refs_with_bound_targets(
        usage,
        expr,
        imported_bindings,
        bound_targets,
        &current_locals(scopes),
    );
    scopes.push(SvelteScopeFrame {
        kind: SvelteBlockKind::Await,
        locals: Vec::new(),
    });
}

fn update_await_branch_locals(captures: &regex::Captures<'_>, scopes: &mut [SvelteScopeFrame]) {
    if let Some(frame) = scopes
        .iter_mut()
        .rev()
        .find(|frame| matches!(frame.kind, SvelteBlockKind::Await))
    {
        frame.locals = captures
            .name("binding")
            .map(|m| extract_pattern_binding_names(m.as_str()))
            .unwrap_or_default();
    }
}

fn apply_const_tag(
    stmt: &str,
    imported_bindings: &FxHashSet<String>,
    bound_targets: &FxHashMap<String, String>,
    scopes: &mut [SvelteScopeFrame],
    usage: &mut TemplateUsage,
) {
    let locals = current_locals(scopes);
    merge_statement_usage_allow_dollar_refs_with_bound_targets(
        usage,
        stmt.trim(),
        imported_bindings,
        bound_targets,
        &locals,
    );
    if let Some(lhs) = stmt.split_once('=').map(|(lhs, _)| lhs.trim()) {
        let new_bindings = extract_pattern_binding_names(lhs);
        if let Some(frame) = scopes.last_mut() {
            frame.locals.extend(new_bindings);
        }
    }
}

fn apply_expression_tag(
    expr: &str,
    imported_bindings: &FxHashSet<String>,
    bound_targets: &FxHashMap<String, String>,
    scopes: &[SvelteScopeFrame],
    usage: &mut TemplateUsage,
) {
    merge_expression_usage_allow_dollar_refs_with_bound_targets(
        usage,
        expr.trim(),
        imported_bindings,
        bound_targets,
        &current_locals(scopes),
    );
}

fn apply_markup_tag(
    tag: &str,
    imported_bindings: &FxHashSet<String>,
    bound_targets: &FxHashMap<String, String>,
    scopes: &mut Vec<SvelteScopeFrame>,
    usage: &mut TemplateUsage,
) {
    let trimmed = tag.trim();
    if trimmed.starts_with("</") {
        if let Some(frame) = scopes.last()
            && frame.kind == SvelteBlockKind::Element
        {
            scopes.pop();
        }
        return;
    }

    if trimmed.starts_with("<!") || trimmed.starts_with("<?") {
        return;
    }

    let parsed = parse_tag_attrs(trimmed, true);
    if parsed.name.is_empty() {
        return;
    }

    let current = current_locals(scopes);
    merge_markup_brace_usage(trimmed, usage, imported_bindings, bound_targets, &current);
    if parsed.name.contains('.')
        || parsed
            .name
            .chars()
            .next()
            .is_some_and(|ch| ch.is_ascii_uppercase())
    {
        merge_component_tag_usage(usage, &parsed.name, imported_bindings, &current, false);
    }

    let element_locals = merge_markup_attr_usage(
        &parsed.attrs,
        imported_bindings,
        bound_targets,
        &current,
        usage,
    );

    if !parsed.self_closing && !is_void_html_tag(&parsed.name) {
        scopes.push(SvelteScopeFrame {
            kind: SvelteBlockKind::Element,
            locals: element_locals,
        });
    }
}

/// Merge usage from each markup attribute (directive bindings, shorthand
/// expressions, attribute values), returning the `let:` locals the element scope
/// introduces.
fn merge_markup_attr_usage(
    attrs: &[ParsedAttr],
    imported_bindings: &FxHashSet<String>,
    bound_targets: &FxHashMap<String, String>,
    current: &[String],
    usage: &mut TemplateUsage,
) -> Vec<String> {
    let mut element_locals = Vec::new();
    for attr in attrs {
        if let Some(binding) = directive_binding_name(&attr.name) {
            merge_expression_usage_allow_dollar_refs_with_bound_targets(
                usage,
                binding,
                imported_bindings,
                bound_targets,
                current,
            );
        }
        if let Some(local) = attr.name.strip_prefix("let:")
            && !local.is_empty()
        {
            element_locals.extend(extract_pattern_binding_names(local));
        }
        if let Some(expr) = shorthand_attribute_expression(&attr.name) {
            merge_expression_usage_allow_dollar_refs_with_bound_targets(
                usage,
                expr,
                imported_bindings,
                bound_targets,
                current,
            );
        }
        if let Some(value) = attr.value.as_deref() {
            merge_attribute_value_usage(usage, value, imported_bindings, bound_targets, current);
        }
    }
    element_locals
}

fn merge_markup_brace_usage(
    tag: &str,
    usage: &mut TemplateUsage,
    imported_bindings: &FxHashSet<String>,
    bound_targets: &FxHashMap<String, String>,
    locals: &[String],
) {
    let mut index = 0;
    let bytes = tag.as_bytes();
    while index < bytes.len() {
        if bytes[index] == b'{' {
            let Some((section, next_index)) = scan_curly_section(tag, index, 1, 1) else {
                break;
            };
            let section = section.trim();
            let expr = section
                .strip_prefix("@attach")
                .or_else(|| section.strip_prefix("..."))
                .map_or(section, str::trim);
            if !expr.is_empty() {
                merge_expression_usage_allow_dollar_refs_with_bound_targets(
                    usage,
                    expr,
                    imported_bindings,
                    bound_targets,
                    locals,
                );
            }
            index = next_index;
            continue;
        }
        index += 1;
    }
}

fn directive_binding_name(attr_name: &str) -> Option<&str> {
    for prefix in ["use:", "animate:", "in:", "out:", "transition:"] {
        if let Some(rest) = attr_name.strip_prefix(prefix) {
            let binding = rest
                .split('|')
                .next()
                .map(str::trim)
                .filter(|name| !name.is_empty());
            if binding.is_some() {
                return binding;
            }
        }
    }
    None
}

fn shorthand_attribute_expression(attr_name: &str) -> Option<&str> {
    attr_name
        .strip_prefix('{')
        .and_then(|rest| rest.strip_suffix('}'))
        .map(str::trim)
        .filter(|expr| !expr.is_empty())
}

fn merge_attribute_value_usage(
    usage: &mut TemplateUsage,
    value: &str,
    imported_bindings: &FxHashSet<String>,
    bound_targets: &FxHashMap<String, String>,
    locals: &[String],
) {
    let mut index = 0;
    let mut found_expression = false;
    let bytes = value.as_bytes();

    while index < bytes.len() {
        if bytes[index] == b'{' {
            let Some((expr, next_index)) = scan_curly_section(value, index, 1, 1) else {
                break;
            };
            merge_expression_usage_allow_dollar_refs_with_bound_targets(
                usage,
                expr,
                imported_bindings,
                bound_targets,
                locals,
            );
            found_expression = true;
            index = next_index;
            continue;
        }
        index += 1;
    }

    if !found_expression && value.starts_with('{') && value.ends_with('}') && value.len() >= 2 {
        merge_expression_usage_allow_dollar_refs_with_bound_targets(
            usage,
            &value[1..value.len() - 1],
            imported_bindings,
            bound_targets,
            locals,
        );
    }
}

fn is_void_html_tag(tag_name: &str) -> bool {
    VOID_HTML_TAGS.contains(&tag_name)
}

fn pop_scope(scopes: &mut Vec<SvelteScopeFrame>, closing: &str) {
    let kind = match closing {
        "if" => Some(SvelteBlockKind::If),
        "each" => Some(SvelteBlockKind::Each),
        "await" => Some(SvelteBlockKind::Await),
        "key" => Some(SvelteBlockKind::Key),
        "snippet" => Some(SvelteBlockKind::Snippet),
        _ => None,
    };

    let Some(kind) = kind else {
        return;
    };

    if let Some(index) = scopes.iter().rposition(|frame| frame.kind == kind)
        && index > 0
    {
        scopes.truncate(index);
    }
}

fn current_locals(scopes: &[SvelteScopeFrame]) -> Vec<String> {
    scopes
        .iter()
        .flat_map(|frame| frame.locals.iter().cloned())
        .collect()
}

#[cfg(all(test, not(miri)))]
mod tests {
    use super::{collect_template_usage, collect_template_usage_with_bound_targets};
    use rustc_hash::{FxHashMap, FxHashSet};

    fn imported(names: &[&str]) -> FxHashSet<String> {
        names.iter().map(|name| (*name).to_string()).collect()
    }

    fn bound_targets(pairs: &[(&str, &str)]) -> FxHashMap<String, String> {
        pairs
            .iter()
            .map(|(local, target)| ((*local).to_string(), (*target).to_string()))
            .collect()
    }

    #[test]
    fn plain_expression_marks_binding_used() {
        let usage = collect_template_usage(
            "<script>import { formatDate } from './utils';</script><p>{formatDate(value)}</p>",
            &imported(&["formatDate"]),
        );

        assert!(usage.used_bindings.contains("formatDate"));
    }

    #[test]
    fn each_alias_shadows_import_name() {
        let usage = collect_template_usage(
            "<script>import { item } from './utils';</script>{#each items as item}<p>{item}</p>{/each}",
            &imported(&["item"]),
        );

        assert!(usage.is_empty());
    }

    #[test]
    fn await_then_alias_shadows_import_name() {
        let usage = collect_template_usage(
            "<script>import { value } from './utils';</script>{#await promise}{:then value}<p>{value}</p>{/await}",
            &imported(&["value"]),
        );

        assert!(usage.is_empty());
    }

    #[test]
    fn namespace_member_accesses_are_retained() {
        let usage = collect_template_usage(
            "<script>import * as utils from './utils';</script><p>{utils.formatDate(value)}</p>",
            &imported(&["utils"]),
        );

        assert!(usage.used_bindings.contains("utils"));
        assert_eq!(usage.member_accesses.len(), 1);
        assert_eq!(usage.member_accesses[0].object, "utils");
        assert_eq!(usage.member_accesses[0].member, "formatDate");
    }

    #[test]
    fn styles_are_ignored() {
        let usage = collect_template_usage(
            "<style>.button { color: red; }</style><script>import { button } from './utils';</script>",
            &imported(&["button"]),
        );

        assert!(usage.is_empty());
    }

    #[test]
    fn component_tags_mark_imported_components_used() {
        let usage = collect_template_usage(
            "<script>import FancyButton from './FancyButton.svelte';</script><FancyButton />",
            &imported(&["FancyButton"]),
        );

        assert!(usage.used_bindings.contains("FancyButton"));
    }

    #[test]
    fn namespaced_component_tags_record_member_usage() {
        let usage = collect_template_usage(
            "<script>import * as Icons from './icons';</script><Icons.Alert />",
            &imported(&["Icons"]),
        );

        assert!(usage.used_bindings.contains("Icons"));
        assert_eq!(usage.member_accesses.len(), 1);
        assert_eq!(usage.member_accesses[0].object, "Icons");
        assert_eq!(usage.member_accesses[0].member, "Alert");
    }

    #[test]
    fn directive_names_mark_imported_actions_used() {
        let usage = collect_template_usage(
            "<script>import { tooltip } from './actions';</script><button use:tooltip>Hi</button>",
            &imported(&["tooltip"]),
        );

        assert!(usage.used_bindings.contains("tooltip"));
    }

    #[test]
    fn attribute_value_expressions_mark_imported_bindings_used() {
        let usage = collect_template_usage(
            "<script>import { isActive } from './state';</script><button class:active={isActive}>Hi</button>",
            &imported(&["isActive"]),
        );

        assert!(usage.used_bindings.contains("isActive"));
    }

    #[test]
    fn ternary_expression_marks_branch_calls_used() {
        let usage = collect_template_usage(
            r#"<p>{cond ? inTernary() : ""}</p>"#,
            &imported(&["inTernary"]),
        );

        assert!(
            usage.used_bindings.contains("inTernary"),
            "expected inTernary usage, got: {usage:?}"
        );
    }

    #[test]
    fn method_chain_callback_marks_reference_used() {
        let usage = collect_template_usage(
            r#"<p>{[1, 2].map(inCallback).join(",")}</p>"#,
            &imported(&["inCallback"]),
        );

        assert!(usage.used_bindings.contains("inCallback"));
    }

    #[test]
    fn inline_spread_object_marks_nested_expression_used() {
        let usage = collect_template_usage(
            r#"<button {...{ "data-x": inSpread() }}>x</button>"#,
            &imported(&["inSpread"]),
        );

        assert!(usage.used_bindings.contains("inSpread"));
    }

    #[test]
    fn shorthand_attributes_mark_imported_bindings_used() {
        let usage = collect_template_usage(
            "<script>import { page } from './stores';</script><Component {page} />",
            &imported(&["page"]),
        );

        assert!(usage.used_bindings.contains("page"));
    }

    #[test]
    fn dollar_store_refs_mark_imported_store_used() {
        let usage = collect_template_usage(
            "<script>import { page } from './stores';</script><p>{$page.url.pathname}</p>",
            &imported(&["page"]),
        );

        assert!(usage.used_bindings.contains("page"));
    }

    #[test]
    fn let_directives_shadow_imported_names() {
        let usage = collect_template_usage(
            "<script>import { item } from './utils';</script><Slot let:item><p>{item}</p></Slot>",
            &imported(&["item"]),
        );

        assert!(usage.used_bindings.is_empty());
        assert!(usage.member_accesses.is_empty());
    }

    #[test]
    fn local_let_bindings_shadow_imported_component_tags() {
        let usage = collect_template_usage(
            "<script>import Item from './Item.svelte';</script><Slot let:Item><Item /></Slot>",
            &imported(&["Item"]),
        );

        assert!(usage.used_bindings.is_empty());
        assert!(usage.member_accesses.is_empty());
    }

    #[test]
    fn empty_imported_bindings_returns_empty_usage() {
        let usage = collect_template_usage("<p>{formatDate(value)}</p>", &imported(&[]));

        assert!(usage.is_empty());
    }

    #[test]
    fn only_script_and_style_returns_empty_markup() {
        let usage = collect_template_usage(
            "<script>import { x } from './x';</script><style>p { color: red; }</style>",
            &imported(&["x"]),
        );

        assert!(usage.is_empty());
    }

    #[test]
    fn html_comments_are_stripped() {
        let usage = collect_template_usage(
            "<!-- {hidden(value)} --><p>{visible(value)}</p>",
            &imported(&["hidden", "visible"]),
        );

        assert!(usage.used_bindings.contains("visible"));
        assert!(!usage.used_bindings.contains("hidden"));
    }

    #[test]
    fn overlapping_ranges_are_merged_during_stripping() {
        let usage = collect_template_usage(
            "<script>let x;</script><!-- comment --><style>p{}</style><p>{fmt(v)}</p>",
            &imported(&["fmt"]),
        );

        assert!(usage.used_bindings.contains("fmt"));
    }

    #[test]
    fn key_block_marks_key_expression_used() {
        let usage = collect_template_usage(
            "<p>{#key selectedId}<Child />{/key}</p>",
            &imported(&["selectedId", "Child"]),
        );

        assert!(usage.used_bindings.contains("selectedId"));
        assert!(usage.used_bindings.contains("Child"));
    }

    #[test]
    fn snippet_params_shadow_imported_names() {
        let usage = collect_template_usage(
            "{#snippet row(item)}<p>{item}</p>{/snippet}",
            &imported(&["item"]),
        );

        assert!(usage.is_empty());
    }

    #[test]
    fn snippet_body_uses_outer_imported_bindings() {
        let usage = collect_template_usage(
            "{#snippet row(local)}<p>{format(local)}</p>{/snippet}",
            &imported(&["format"]),
        );

        assert!(usage.used_bindings.contains("format"));
    }

    #[test]
    fn snippet_typed_params_do_not_stack_overflow() {
        let usage = collect_template_usage(
            "{#snippet Link({ href, content }: Props)}<a {href}>{content}</a>{/snippet}",
            &imported(&["href", "content"]),
        );

        assert!(usage.is_empty());
    }

    #[test]
    fn snippet_tuple_typed_param_does_not_stack_overflow() {
        let usage = collect_template_usage(
            "{#snippet foo(x: [number, number])}{/snippet}",
            &imported(&["x"]),
        );

        assert!(usage.is_empty());
    }

    #[test]
    fn at_html_marks_expression_used() {
        let usage = collect_template_usage("{@html sanitize(content)}", &imported(&["sanitize"]));

        assert!(usage.used_bindings.contains("sanitize"));
    }

    #[test]
    fn at_render_marks_expression_used() {
        let usage = collect_template_usage("{@render header()}", &imported(&["header"]));

        assert!(usage.used_bindings.contains("header"));
    }

    #[test]
    fn at_attach_marks_expression_used() {
        let usage = collect_template_usage(
            "<div {@attach myAttach}>Attached</div>",
            &imported(&["myAttach"]),
        );

        assert!(usage.used_bindings.contains("myAttach"));
    }

    #[test]
    fn event_handler_arrow_member_access_maps_script_instance_to_class() {
        let usage = collect_template_usage_with_bound_targets(
            "<button onclick={() => counter.bump()}>{counter.value}</button>",
            &imported(&[]),
            &bound_targets(&[("counter", "Counter")]),
        );

        assert!(
            usage
                .member_accesses
                .iter()
                .any(|access| access.object == "Counter" && access.member == "bump"),
            "counter.bump() should map to Counter.bump, found: {:?}",
            usage.member_accesses
        );
        assert!(
            usage
                .member_accesses
                .iter()
                .any(|access| access.object == "Counter" && access.member == "value"),
            "counter.value should map to Counter.value, found: {:?}",
            usage.member_accesses
        );
    }

    #[test]
    fn template_locals_shadow_script_instance_bindings() {
        let usage = collect_template_usage_with_bound_targets(
            "{#each rows as counter}<button onclick={() => { other.go(); counter.bump(); }} />{/each}",
            &imported(&[]),
            &bound_targets(&[("counter", "Counter"), ("other", "Other")]),
        );

        assert!(
            usage
                .member_accesses
                .iter()
                .any(|access| access.object == "Other" && access.member == "go"),
            "other.go() should still map to Other.go, found: {:?}",
            usage.member_accesses
        );
        assert!(
            !usage
                .member_accesses
                .iter()
                .any(|access| access.object == "Counter" && access.member == "bump"),
            "shadowed counter.bump() must not map to Counter.bump, found: {:?}",
            usage.member_accesses
        );
    }

    #[test]
    fn at_const_marks_rhs_expression_used() {
        let usage = collect_template_usage(
            "{#each items as item}{@const label = format(item)}<p>{label}</p>{/each}",
            &imported(&["format"]),
        );

        assert!(usage.used_bindings.contains("format"));
    }

    #[test]
    fn at_const_shadows_subsequent_usages() {
        let usage = collect_template_usage(
            "{#each items as item}{@const myVal = item.name}<p>{myVal}</p>{/each}",
            &imported(&["myVal"]),
        );

        assert!(usage.used_bindings.contains("myVal"));
    }

    #[test]
    fn at_debug_marks_expression_used() {
        let usage = collect_template_usage("{@debug count}", &imported(&["count"]));

        assert!(usage.used_bindings.contains("count"));
    }

    #[test]
    fn else_if_marks_condition_used() {
        let usage = collect_template_usage(
            "{#if a}<p>a</p>{:else if isReady}<p>b</p>{/if}",
            &imported(&["isReady"]),
        );

        assert!(usage.used_bindings.contains("isReady"));
    }

    #[test]
    fn else_branch_does_not_generate_usage() {
        let usage = collect_template_usage(
            "{#if cond}<p>a</p>{:else}<p>{fallback(x)}</p>{/if}",
            &imported(&["fallback"]),
        );

        assert!(usage.used_bindings.contains("fallback"));
    }

    #[test]
    fn if_block_marks_condition_expression_used() {
        let usage = collect_template_usage(
            "{#if isVisible}<p>Hello</p>{/if}",
            &imported(&["isVisible"]),
        );

        assert!(usage.used_bindings.contains("isVisible"));
    }

    #[test]
    fn closing_unknown_block_kind_is_no_op() {
        let usage = collect_template_usage("{/unknownblock}<p>{fmt(x)}</p>", &imported(&["fmt"]));

        assert!(usage.used_bindings.contains("fmt"));
    }

    #[test]
    fn each_key_expression_marks_binding_used() {
        let usage = collect_template_usage(
            "{#each items as item (getId(item))}<p>{item}</p>{/each}",
            &imported(&["getId"]),
        );

        assert!(usage.used_bindings.contains("getId"));
    }

    #[test]
    fn each_key_expression_has_access_to_each_locals() {
        let usage = collect_template_usage(
            "{#each items as item (item.id)}<p>{item}</p>{/each}",
            &imported(&["item"]),
        );

        assert!(usage.is_empty());
    }

    #[test]
    fn catch_binding_shadows_import_name() {
        let usage = collect_template_usage(
            "{#await promise}{:catch error}<p>{error}</p>{/await}",
            &imported(&["error"]),
        );

        assert!(usage.is_empty());
    }

    #[test]
    fn catch_without_binding_does_not_crash() {
        let usage = collect_template_usage(
            "{#await loadData()}{:catch}<p>Error</p>{/await}",
            &imported(&["loadData"]),
        );

        assert!(usage.used_bindings.contains("loadData"));
    }

    #[test]
    fn then_without_binding_does_not_crash() {
        let usage = collect_template_usage(
            "{#await loadData()}{:then}<p>Done</p>{/await}",
            &imported(&["loadData"]),
        );

        assert!(usage.used_bindings.contains("loadData"));
    }

    #[test]
    fn html_doctype_and_processing_instructions_are_ignored() {
        let usage = collect_template_usage(
            "<!DOCTYPE html><?xml version=\"1.0\"?><p>{fmt(x)}</p>",
            &imported(&["fmt"]),
        );

        assert!(usage.used_bindings.contains("fmt"));
    }

    #[test]
    fn void_html_tags_do_not_push_element_scope() {
        let usage = collect_template_usage(
            "<div><input value={val} /><p>{handler(x)}</p></div>",
            &imported(&["val", "handler"]),
        );

        assert!(usage.used_bindings.contains("val"));
        assert!(usage.used_bindings.contains("handler"));
    }

    #[test]
    fn closing_markup_tag_pops_element_scope() {
        let usage = collect_template_usage(
            "<div let:item><p>{item}</p></div><p>{helper(x)}</p>",
            &imported(&["item", "helper"]),
        );

        assert!(usage.used_bindings.contains("helper"));
    }

    #[test]
    fn animate_directive_marks_binding_used() {
        let usage = collect_template_usage("<div animate:flip>content</div>", &imported(&["flip"]));

        assert!(usage.used_bindings.contains("flip"));
    }

    #[test]
    fn transition_directive_marks_binding_used() {
        let usage =
            collect_template_usage("<div transition:fade>content</div>", &imported(&["fade"]));

        assert!(usage.used_bindings.contains("fade"));
    }

    #[test]
    fn in_directive_marks_binding_used() {
        let usage = collect_template_usage("<div in:fly>content</div>", &imported(&["fly"]));

        assert!(usage.used_bindings.contains("fly"));
    }

    #[test]
    fn out_directive_marks_binding_used() {
        let usage = collect_template_usage("<div out:slide>content</div>", &imported(&["slide"]));

        assert!(usage.used_bindings.contains("slide"));
    }

    #[test]
    fn directive_with_modifier_strips_pipe() {
        let usage = collect_template_usage(
            "<div transition:fade|local>content</div>",
            &imported(&["fade"]),
        );

        assert!(usage.used_bindings.contains("fade"));
    }

    #[test]
    fn unquoted_attribute_value_is_parsed() {
        let usage =
            collect_template_usage("<div data-value=hello>content</div>", &imported(&["hello"]));

        assert!(usage.is_empty());
    }

    #[test]
    fn curly_brace_attribute_value_is_parsed() {
        let usage = collect_template_usage(
            "<div class={getClass()}>content</div>",
            &imported(&["getClass"]),
        );

        assert!(usage.used_bindings.contains("getClass"));
    }

    #[test]
    fn attribute_without_value_is_handled() {
        let usage = collect_template_usage(
            "<button disabled><p>{action(x)}</p></button>",
            &imported(&["action"]),
        );

        assert!(usage.used_bindings.contains("action"));
    }

    #[test]
    fn interpolated_attribute_value_marks_binding_used() {
        let usage = collect_template_usage(
            "<div class=\"prefix-{cls}-suffix\">content</div>",
            &imported(&["cls"]),
        );

        assert!(usage.used_bindings.contains("cls"));
    }

    #[test]
    fn multiple_curly_expressions_all_tracked() {
        let usage = collect_template_usage(
            "<p>{first(x)} and {second(y)}</p>",
            &imported(&["first", "second"]),
        );

        assert!(usage.used_bindings.contains("first"));
        assert!(usage.used_bindings.contains("second"));
    }

    #[test]
    fn empty_curly_braces_produce_no_usage() {
        let usage = collect_template_usage("{ }<p>{fmt(x)}</p>", &imported(&["fmt"]));

        assert!(usage.used_bindings.contains("fmt"));
    }

    #[test]
    fn self_closing_tag_does_not_push_scope() {
        let usage = collect_template_usage("<br /><p>{fmt(x)}</p>", &imported(&["fmt"]));

        assert!(usage.used_bindings.contains("fmt"));
    }

    #[test]
    fn nested_each_and_if_scoping_works_correctly() {
        let usage = collect_template_usage(
            "{#each rows as row}{#if row.visible}<p>{format(row.name)}</p>{/if}{/each}",
            &imported(&["format", "row"]),
        );

        assert!(usage.used_bindings.contains("format"));
        assert!(!usage.used_bindings.contains("row"));
    }

    #[test]
    fn at_const_rhs_references_are_tracked() {
        let usage = collect_template_usage(
            "{#each items as item}{@const x = compute(item)}<p>{x}</p>{/each}",
            &imported(&["compute"]),
        );

        assert!(usage.used_bindings.contains("compute"));
    }

    #[test]
    fn closing_tag_without_element_scope_is_safe() {
        let usage = collect_template_usage("</div><p>{fmt(x)}</p>", &imported(&["fmt"]));

        assert!(usage.used_bindings.contains("fmt"));
    }

    #[test]
    fn snippet_closing_pops_scope_correctly() {
        let usage = collect_template_usage(
            "{#snippet cell(item)}<p>{item}</p>{/snippet}<p>{outer(x)}</p>",
            &imported(&["item", "outer"]),
        );

        assert!(!usage.used_bindings.contains("item"));
        assert!(usage.used_bindings.contains("outer"));
    }

    #[test]
    fn key_block_closing_pops_scope() {
        let usage = collect_template_usage(
            "{#key id}<Child />{/key}<p>{helper(x)}</p>",
            &imported(&["id", "Child", "helper"]),
        );

        assert!(usage.used_bindings.contains("id"));
        assert!(usage.used_bindings.contains("Child"));
        assert!(usage.used_bindings.contains("helper"));
    }

    #[test]
    fn plain_expression_without_prefix_is_tracked() {
        let usage = collect_template_usage("{count + 1}", &imported(&["count"]));

        assert!(usage.used_bindings.contains("count"));
    }

    #[test]
    fn single_quoted_attribute_value_expressions_are_parsed() {
        let usage = collect_template_usage(
            "<div title='{getName()}'>content</div>",
            &imported(&["getName"]),
        );

        assert!(usage.used_bindings.contains("getName"));
    }

    // The `<svelte:component this={X}>` / `<svelte:element this={tag}>` bound
    // target is already credited by the generic attribute-value scan in
    // `apply_markup_tag` (`this` is an ordinary attr whose `{...}` value flows
    // through `merge_attribute_value_usage`), so no special-element dispatch is
    // needed. These tests pin that behavior and guard `<svelte:self>` against a
    // scanner crash.
    #[test]
    fn svelte_component_this_credits_target() {
        let usage = collect_template_usage(
            "<script>import Foo from './Foo.svelte';</script><svelte:component this={Foo} />",
            &imported(&["Foo"]),
        );
        assert!(
            usage.used_bindings.contains("Foo"),
            "Foo should be credited via the existing attr-value scan, got: {usage:?}"
        );
    }

    #[test]
    fn svelte_element_this_credits_tag_binding() {
        let usage = collect_template_usage(
            "<script>let tag = 'div';</script><svelte:element this={tag}>x</svelte:element>",
            &imported(&["tag"]),
        );
        assert!(
            usage.used_bindings.contains("tag"),
            "tag should be credited via the existing attr-value scan, got: {usage:?}"
        );
    }

    #[test]
    fn svelte_element_string_literal_this_credits_nothing() {
        let usage = collect_template_usage(
            r#"<svelte:element this="div">x</svelte:element>"#,
            &imported(&["div"]),
        );
        assert!(
            usage.is_empty(),
            "a string-literal element name is a native DOM tag, got: {usage:?}"
        );
    }

    #[test]
    fn listened_events_credits_component_tag_on_directive() {
        let listened = super::collect_listened_events("<Child on:save on:close />");
        assert!(listened.contains(&"save".to_string()));
        assert!(listened.contains(&"close".to_string()));
    }

    #[test]
    fn listened_events_excludes_dom_element_on_directive() {
        // `on:click` on a lowercase DOM `<button>` is a DOM event, not a custom
        // event, so it must not be credited.
        let listened = super::collect_listened_events("<button on:click>Hi</button>");
        assert!(
            listened.is_empty(),
            "DOM on:click must be excluded: {listened:?}"
        );
    }

    #[test]
    fn listened_events_credits_event_forwarding_without_value() {
        // Event forwarding (`on:save` with no value) on a component still counts.
        let listened = super::collect_listened_events("<Child on:save />");
        assert!(listened.contains(&"save".to_string()));
    }

    #[test]
    fn listened_events_strips_event_modifiers() {
        let listened = super::collect_listened_events("<Child on:save|once={handler} />");
        assert!(listened.contains(&"save".to_string()));
        assert!(!listened.iter().any(|name| name.contains('|')));
    }

    #[test]
    fn listened_events_credits_namespaced_component_tag() {
        let listened = super::collect_listened_events("<Icons.Alert on:dismiss />");
        assert!(listened.contains(&"dismiss".to_string()));
    }

    #[test]
    fn listened_events_ignores_script_block_content() {
        let listened = super::collect_listened_events(
            "<script>const x = 'on:save';</script><Child on:close />",
        );
        assert!(listened.contains(&"close".to_string()));
        assert!(!listened.contains(&"save".to_string()));
    }

    #[test]
    fn svelte_self_does_not_crash() {
        let usage = collect_template_usage(
            "{#if depth}<svelte:self depth={depth} />{/if}",
            &imported(&["depth"]),
        );
        // Inert for component crediting; just must not panic.
        let _ = usage;
    }
}
