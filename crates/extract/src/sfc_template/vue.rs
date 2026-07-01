use std::sync::LazyLock;

use rustc_hash::{FxHashMap, FxHashSet};

use crate::template_usage::TemplateUsage;

use super::scanners::{
    scan_bracket_section, scan_curly_section, scan_html_tag, scan_paren_section,
};
use super::shared::{
    HTML_COMMENT_RE, ParsedAttr, ParsedTag, kebab_to_camel_case, merge_component_tag_usage,
    merge_expression_usage_with_bound_targets, merge_pattern_binding_usage,
    merge_statement_usage_with_bound_targets, parse_tag_attrs,
};

/// Matches a `<template ...>` OPENING tag only (quote-aware over the attribute
/// list). The matching `</template>` is located separately by
/// [`find_template_body_end`] with nesting depth tracking, because a Vue SFC
/// root `<template>` commonly contains nested `<template #slot>` elements and a
/// non-greedy `</template>` body capture would truncate the body at the FIRST
/// nested close, dropping every component rendered after it (issue: false
/// `unused-export` on `<template #slot>` + later `<Component>`).
static TEMPLATE_OPEN_RE: LazyLock<regex::Regex> =
    LazyLock::new(|| crate::static_regex(r#"(?is)<template\b(?:[^>"']|"[^"]*"|'[^']*')*>"#));

static VUE_FOR_RE: LazyLock<regex::Regex> =
    LazyLock::new(|| crate::static_regex(r"(?is)^(?P<binding>.+?)\s+(?:in|of)\s+(?P<source>.+)$"));

/// Matches a `v-for="..."` / `v-for='...'` attribute and captures its value.
/// Used only by the pre-scan that types loop variables (issue #1707); the
/// streaming scan re-parses each directive via `parse_tag_attrs` / `VUE_FOR_RE`.
static VUE_FOR_ATTR_RE: LazyLock<regex::Regex> =
    LazyLock::new(|| crate::static_regex(r#"(?is)\bv-for\s*=\s*(?:"([^"]*)"|'([^']*)')"#));

/// Matches a `<style ...>...</style>` block, capturing the body. Used to scan
/// for Vue SFC CSS `v-bind(expr)` references, which bind a script/prop binding
/// into CSS and otherwise look like usage nowhere in the `<template>`.
static VUE_STYLE_RE: LazyLock<regex::Regex> = LazyLock::new(|| {
    crate::static_regex(r#"(?is)<style\b(?:[^>"']|"[^"]*"|'[^']*')*>(?P<body>.*?)</style>"#)
});

#[cfg(test)]
pub(super) fn collect_template_usage(
    source: &str,
    imported_bindings: &FxHashSet<String>,
) -> TemplateUsage {
    collect_template_usage_with_bound_targets(
        source,
        imported_bindings,
        &FxHashMap::default(),
        &FxHashMap::default(),
    )
}

pub(super) fn collect_template_usage_with_bound_targets(
    source: &str,
    imported_bindings: &FxHashSet<String>,
    bound_targets: &FxHashMap<String, String>,
    iterable_types: &FxHashMap<String, String>,
) -> TemplateUsage {
    let comment_ranges: Vec<(usize, usize)> = HTML_COMMENT_RE
        .find_iter(source)
        .map(|m| (m.start(), m.end()))
        .collect();
    let body_ranges = template_body_ranges(source, &comment_ranges);

    // Type each `v-for` loop item to its source iterable's element class up front
    // (issue #1707), so a member access on the item (`{{ util.getter }}`) anywhere
    // in the streamed scan remaps onto the class via the effective bound targets.
    // Only clone when there is something to add. Scoped to the same template body
    // regions (comments excluded) the streaming scan covers, so a `v-for="..."`
    // string inside `<script>` or an HTML comment is never picked up.
    let augmented = augment_bound_targets_with_v_for_types(
        source,
        &body_ranges,
        &comment_ranges,
        iterable_types,
        bound_targets,
    );
    let effective_bound_targets = augmented.as_ref().unwrap_or(bound_targets);

    let mut usage = TemplateUsage::default();
    for &(body_start, body_end) in &body_ranges {
        usage.merge(scan_template_body(
            &source[body_start..body_end],
            body_start,
            imported_bindings,
            effective_bound_targets,
            iterable_types,
        ));
    }

    scan_style_vbind_usage(
        source,
        imported_bindings,
        effective_bound_targets,
        &mut usage,
    );

    usage
}

/// Compute the `(start, end)` byte ranges of each root `<template>` body, mirroring
/// the streaming scan's skip logic: nested `<template>` opens are subsumed by the
/// enclosing root body, comment-embedded opens and self-closing `<template/>` are
/// skipped.
fn template_body_ranges(source: &str, comment_ranges: &[(usize, usize)]) -> Vec<(usize, usize)> {
    let mut ranges = Vec::new();
    // Cursor past the last fully-consumed root template body, so nested
    // `<template>` opens (which `TEMPLATE_OPEN_RE` also matches) are skipped rather
    // than rescanned as separate roots.
    let mut scan_from = 0usize;
    for open in TEMPLATE_OPEN_RE.find_iter(source) {
        if open.start() < scan_from {
            continue;
        }
        if comment_ranges
            .iter()
            .any(|&(start, end)| open.start() >= start && open.start() < end)
        {
            continue;
        }
        // `<template/>` self-closing: no body.
        if open.as_str().trim_end().ends_with("/>") {
            continue;
        }
        let body_start = open.end();
        let Some(body_end) = find_template_body_end(source, body_start) else {
            continue;
        };
        ranges.push((body_start, body_end));
        scan_from = body_end;
    }
    ranges
}

/// Pre-scan the template body regions for `v-for` directives and, for each whose
/// source iterable resolves to a known element class in `iterable_types`, bind the
/// loop item variable to that class. Returns an augmented copy of `bound_targets`
/// only when at least one typed loop variable was found (so the common no-array
/// case pays no clone). First-write-wins on a repeated item name across v-fors:
/// the over-credit direction is preferred (never introduces a false positive).
/// Matches are constrained to `body_ranges` and excluded from `comment_ranges`, so
/// a `v-for="..."` occurring in `<script>` or inside an HTML comment is ignored.
fn augment_bound_targets_with_v_for_types(
    source: &str,
    body_ranges: &[(usize, usize)],
    comment_ranges: &[(usize, usize)],
    iterable_types: &FxHashMap<String, String>,
    bound_targets: &FxHashMap<String, String>,
) -> Option<FxHashMap<String, String>> {
    if iterable_types.is_empty() {
        return None;
    }
    let mut augmented: Option<FxHashMap<String, String>> = None;
    for caps in VUE_FOR_ATTR_RE.captures_iter(source) {
        let Some(value) = caps.get(1).or_else(|| caps.get(2)) else {
            continue;
        };
        let pos = value.start();
        let in_template = body_ranges
            .iter()
            .any(|&(start, end)| pos >= start && pos < end);
        if !in_template {
            continue;
        }
        let in_comment = comment_ranges
            .iter()
            .any(|&(start, end)| pos >= start && pos < end);
        if in_comment {
            continue;
        }
        let Some((item, class)) = v_for_element_binding(value.as_str(), iterable_types) else {
            continue;
        };
        augmented
            .get_or_insert_with(|| bound_targets.clone())
            .entry(item)
            .or_insert(class);
    }
    augmented
}

/// Resolve a `v-for` directive value (`"(util, index) of utils"`) to
/// `(item_name, element_class)` when the source iterable has a known element
/// class and the loop item is a bare identifier. Destructured items
/// (`{ id } of items`) and unknown sources yield `None`.
fn v_for_element_binding(
    directive_value: &str,
    iterable_types: &FxHashMap<String, String>,
) -> Option<(String, String)> {
    let caps = VUE_FOR_RE.captures(directive_value)?;
    let binding = caps.name("binding").map_or("", |m| m.as_str());
    let source_expr = caps.name("source").map_or("", |m| m.as_str()).trim();
    let class = iterable_types.get(source_expr)?;
    let item = v_for_item_identifier(binding)?;
    Some((item, class.clone()))
}

/// Extract the loop ITEM identifier from a `v-for` binding pattern. The item is
/// the first (value) element; a destructured or non-identifier item yields
/// `None`. A `(util, index)` alias tuple and an optional TS annotation
/// (`(util: Util, index)`) are handled.
fn v_for_item_identifier(binding: &str) -> Option<String> {
    let trimmed = binding.trim();
    let inner = trimmed
        .strip_prefix('(')
        .and_then(|rest| rest.strip_suffix(')'))
        .unwrap_or(trimmed)
        .trim();
    // The first comma-separated element is the item. A destructured item begins
    // with `{`/`[`, so a naive split leaves an invalid identifier that fails the
    // identifier check below (correctly declining to type it).
    let first = inner.split(',').next()?.trim();
    // Drop an optional TS type annotation (`util: Util`).
    let name = first.split(':').next().unwrap_or(first).trim();
    is_vfor_identifier(name).then(|| name.to_string())
}

fn is_vfor_identifier(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    matches!(first, 'A'..='Z' | 'a'..='z' | '_' | '$')
        && chars.all(|ch| matches!(ch, 'A'..='Z' | 'a'..='z' | '0'..='9' | '_' | '$'))
}

/// Scan each `<style>` block for Vue SFC CSS `v-bind(expr)` references and credit
/// the referenced bindings, so a prop / import used only via `v-bind(accent)` is
/// not reported as unused. Handles the identifier form `v-bind(accent)`, the
/// member form `v-bind(props.accent)`, and the string form `v-bind('a.b')` (the
/// quoted content is itself a JS expression). Restricted to `<style>` bodies so a
/// template attribute `v-bind="..."` is never matched here.
fn scan_style_vbind_usage(
    source: &str,
    imported_bindings: &FxHashSet<String>,
    bound_targets: &FxHashMap<String, String>,
    usage: &mut TemplateUsage,
) {
    for caps in VUE_STYLE_RE.captures_iter(source) {
        let Some(body) = caps.name("body") else {
            continue;
        };
        let body = body.as_str();
        let bytes = body.as_bytes();
        let mut index = 0;
        while let Some(rel) = body[index..].find("v-bind") {
            let start = index + rel;
            // Require a token boundary before `v-bind` so a longer identifier
            // (e.g. a CSS custom property containing the substring) is not matched.
            let preceded_by_word = start
                .checked_sub(1)
                .is_some_and(|prev| bytes[prev].is_ascii_alphanumeric() || bytes[prev] == b'_');
            let mut paren = start + "v-bind".len();
            while bytes.get(paren).is_some_and(u8::is_ascii_whitespace) {
                paren += 1;
            }
            if !preceded_by_word
                && bytes.get(paren) == Some(&b'(')
                && let Some((expr, next)) = scan_paren_section(body, paren)
            {
                credit_style_vbind_expr(expr.trim(), imported_bindings, bound_targets, usage);
                index = next;
                continue;
            }
            index = start + "v-bind".len();
        }
    }
}

/// Credit the bindings referenced by a single `v-bind(...)` style expression.
/// A wholly-quoted argument (`v-bind('foo.bar')`) is unwrapped because Vue treats
/// the string content as the JS expression.
fn credit_style_vbind_expr(
    expr: &str,
    imported_bindings: &FxHashSet<String>,
    bound_targets: &FxHashMap<String, String>,
    usage: &mut TemplateUsage,
) {
    let inner = strip_wrapping_quotes(expr).unwrap_or(expr);
    if inner.is_empty() {
        return;
    }
    merge_expression_usage_with_bound_targets(usage, inner, imported_bindings, bound_targets, &[]);
}

/// Strip a single matching wrapping quote pair (`'x'` / `"x"`), but only when the
/// quote does not reappear inside, so a real expression such as `'a' + b` is left
/// intact for the analyzer.
fn strip_wrapping_quotes(expr: &str) -> Option<&str> {
    let bytes = expr.as_bytes();
    let first = *bytes.first()?;
    if (first == b'\'' || first == b'"') && bytes.len() >= 2 && *bytes.last()? == first {
        let inner = &expr[1..expr.len() - 1];
        if !inner.as_bytes().contains(&first) {
            return Some(inner);
        }
    }
    None
}

/// Locate the `</template>` that closes the root template whose body starts at
/// `body_start`, counting nested `<template>` opens so a slot template's close
/// does not terminate the root body early. Returns the byte index of the `<` of
/// the matching `</template>`, or `None` if the markup is unbalanced.
fn find_template_body_end(source: &str, body_start: usize) -> Option<usize> {
    let bytes = source.as_bytes();
    let mut index = body_start;
    let mut depth: usize = 1;
    while index < bytes.len() {
        // Byte-slice comparisons throughout: the fallback `index += 1` can land
        // inside a multi-byte UTF-8 char (e.g. CJK text in a template), so string
        // slicing here would panic on a non-char-boundary index.
        if bytes[index..].starts_with(b"<!--") {
            if let Some(rel) = source[index + 4..].find("-->") {
                index += 4 + rel + 3;
                continue;
            }
            // Unclosed comment (malformed markup): treat the `<` as ordinary
            // text and keep scanning rather than bailing, so a valid root
            // `</template>` later in the body is still found and the file's
            // component renders stay credited.
            index += 1;
            continue;
        }
        if bytes[index] == b'<'
            && let Some((tag, next_index)) = scan_html_tag(source, index)
        {
            let trimmed = tag.trim();
            if is_template_close_tag(trimmed) {
                depth -= 1;
                if depth == 0 {
                    return Some(index);
                }
            } else if is_template_open_tag(trimmed) && !trimmed.trim_end().ends_with("/>") {
                depth += 1;
            }
            index = next_index;
            continue;
        }
        index += 1;
    }
    None
}

/// Whether `tag` (a full `<...>` tag string) is a `<template ...>` opening tag
/// (case-insensitive), guarding against `<templatefoo>` via a boundary check.
fn is_template_open_tag(tag: &str) -> bool {
    let Some(rest) = tag.strip_prefix('<') else {
        return false;
    };
    let Some(after) = rest.get(..8) else {
        return false;
    };
    after.eq_ignore_ascii_case("template")
        && rest[8..]
            .chars()
            .next()
            .is_none_or(|c| c.is_whitespace() || c == '>' || c == '/')
}

/// Whether `tag` (a full `<...>` tag string) is a `</template>` closing tag
/// (case-insensitive), tolerating interior whitespace before `>` such as the
/// `</template >` Prettier emits for slot blocks. Guards against `</templatefoo>`
/// via the post-name remainder check.
fn is_template_close_tag(tag: &str) -> bool {
    let Some(rest) = tag.strip_prefix("</") else {
        return false;
    };
    let Some(after) = rest.get(..8) else {
        return false;
    };
    after.eq_ignore_ascii_case("template") && rest[8..].trim() == ">"
}

fn scan_template_body(
    body: &str,
    body_offset: usize,
    imported_bindings: &FxHashSet<String>,
    bound_targets: &FxHashMap<String, String>,
    iterable_types: &FxHashMap<String, String>,
) -> TemplateUsage {
    let mut usage = TemplateUsage::default();
    let mut scopes: Vec<Vec<String>> = vec![Vec::new()];
    let bytes = body.as_bytes();
    let mut index = 0;

    while index < bytes.len() {
        if bytes[index..].starts_with(b"<!--") {
            if let Some(end) = body[index + 4..].find("-->") {
                index += 4 + end + 3;
            } else {
                break;
            }
            continue;
        }

        if bytes[index..].starts_with(b"{{") {
            let Some((expr, next_index)) = scan_curly_section(body, index, 2, 2) else {
                break;
            };
            merge_expression_usage_with_bound_targets(
                &mut usage,
                expr.trim(),
                imported_bindings,
                bound_targets,
                &current_locals(&scopes),
            );
            index = next_index;
            continue;
        }

        if bytes[index] == b'<' {
            let Some((tag, next_index)) = scan_html_tag(body, index) else {
                break;
            };
            apply_tag(VueTagInput {
                tag,
                tag_start: body_offset + index,
                tag_end: body_offset + next_index,
                imported_bindings,
                bound_targets,
                iterable_types,
                scopes: &mut scopes,
                usage: &mut usage,
            });
            index = next_index;
            continue;
        }

        index += 1;
    }

    usage
}

struct VueTagInput<'a> {
    tag: &'a str,
    tag_start: usize,
    tag_end: usize,
    imported_bindings: &'a FxHashSet<String>,
    bound_targets: &'a FxHashMap<String, String>,
    iterable_types: &'a FxHashMap<String, String>,
    scopes: &'a mut Vec<Vec<String>>,
    usage: &'a mut TemplateUsage,
}

fn apply_tag(input: VueTagInput<'_>) {
    let VueTagInput {
        tag,
        tag_start,
        tag_end,
        imported_bindings,
        bound_targets,
        iterable_types,
        scopes,
        usage,
    } = input;

    let trimmed = tag.trim();
    if trimmed.starts_with("</") {
        if scopes.len() > 1 {
            scopes.pop();
        }
        return;
    }

    if trimmed.starts_with("<!") || trimmed.starts_with("<?") {
        return;
    }

    let current = current_locals(scopes);
    let parsed = parse_tag_attrs(trimmed, false);
    mark_tag_usage(&parsed.name, imported_bindings, &current, usage);
    collect_v_html_sink(&parsed, tag_start, tag_end, usage);

    let v_for_locals = collect_v_for_locals(
        &parsed,
        imported_bindings,
        bound_targets,
        iterable_types,
        &current,
        usage,
    );
    let slot_locals = collect_slot_locals(&parsed, imported_bindings, &current, usage);

    let mut element_locals = v_for_locals.clone();
    element_locals.extend(slot_locals);

    let mut attr_locals = current.clone();
    attr_locals.extend(element_locals.iter().cloned());
    let mut arg_locals = current;
    arg_locals.extend(v_for_locals);

    scan_vue_attrs(
        &parsed,
        imported_bindings,
        bound_targets,
        &arg_locals,
        &attr_locals,
        usage,
    );

    if !parsed.self_closing {
        scopes.push(element_locals);
    }
}

fn collect_v_html_sink(
    parsed: &ParsedTag,
    tag_start: usize,
    tag_end: usize,
    usage: &mut TemplateUsage,
) {
    if let Some(value) = attr_value(parsed, "v-html")
        && let Some(sink) = crate::template_usage::template_html_sink(value, tag_start, tag_end)
    {
        usage.security_sinks.push(sink);
    }
}

fn collect_v_for_locals(
    parsed: &ParsedTag,
    imported_bindings: &FxHashSet<String>,
    bound_targets: &FxHashMap<String, String>,
    iterable_types: &FxHashMap<String, String>,
    current: &[String],
    usage: &mut TemplateUsage,
) -> Vec<String> {
    let Some(value) = attr_value(parsed, "v-for") else {
        return Vec::new();
    };
    let Some(captures) = VUE_FOR_RE.captures(value) else {
        return Vec::new();
    };
    let binding = captures.name("binding").map_or("", |m| m.as_str()).trim();
    let source_expr = captures.name("source").map_or("", |m| m.as_str()).trim();
    merge_expression_usage_with_bound_targets(
        usage,
        source_expr,
        imported_bindings,
        bound_targets,
        current,
    );
    let mut locals = merge_pattern_binding_usage(usage, binding, imported_bindings, current);
    // When the loop item is typed to an element class (issue #1707), drop it from
    // the locals so it is NOT declared as a resolved binding in the wrapped
    // snippet; it stays an unresolved reference that remaps onto the class via the
    // pre-augmented `bound_targets`, crediting `item.member`. The `index` alias
    // and destructured items are unaffected (they stay locals).
    if let Some((item, _)) = v_for_element_binding(value, iterable_types) {
        locals.retain(|name| name != &item);
    }
    locals
}

fn collect_slot_locals(
    parsed: &ParsedTag,
    imported_bindings: &FxHashSet<String>,
    current: &[String],
    usage: &mut TemplateUsage,
) -> Vec<String> {
    parsed
        .attrs
        .iter()
        .find(|attr| {
            attr.name == "slot-scope"
                || attr.name.starts_with("v-slot")
                || attr.name.starts_with('#')
        })
        .and_then(|attr| attr.value.as_deref())
        .map_or_else(Vec::new, |value| {
            merge_pattern_binding_usage(usage, value, imported_bindings, current)
        })
}

fn scan_vue_attrs(
    parsed: &ParsedTag,
    imported_bindings: &FxHashSet<String>,
    bound_targets: &FxHashMap<String, String>,
    arg_locals: &[String],
    attr_locals: &[String],
    usage: &mut TemplateUsage,
) {
    for attr in &parsed.attrs {
        mark_custom_directive_usage(&attr.name, imported_bindings, usage);
        if let Some(expr) = dynamic_argument_expression(&attr.name) {
            merge_expression_usage_with_bound_targets(
                usage,
                expr,
                imported_bindings,
                bound_targets,
                arg_locals,
            );
        }
        if attr.value.is_none()
            && let Some(binding) = vbind_shorthand_binding(&attr.name)
        {
            merge_expression_usage_with_bound_targets(
                usage,
                &binding,
                imported_bindings,
                bound_targets,
                attr_locals,
            );
        }
        scan_vue_attr_value(attr, imported_bindings, bound_targets, attr_locals, usage);
    }
}

fn scan_vue_attr_value(
    attr: &ParsedAttr,
    imported_bindings: &FxHashSet<String>,
    bound_targets: &FxHashMap<String, String>,
    attr_locals: &[String],
    usage: &mut TemplateUsage,
) {
    let Some(expr) = attr.value.as_deref() else {
        return;
    };
    if attr.name == "v-for"
        || attr.name == "slot-scope"
        || attr.name.starts_with("v-slot")
        || attr.name.starts_with('#')
    {
        return;
    }

    if is_statement_attr(&attr.name) {
        merge_statement_usage_with_bound_targets(
            usage,
            expr,
            imported_bindings,
            bound_targets,
            attr_locals,
        );
    } else if is_expression_attr(&attr.name) || is_custom_directive_attr(&attr.name) {
        merge_expression_usage_with_bound_targets(
            usage,
            expr,
            imported_bindings,
            bound_targets,
            attr_locals,
        );
    }
}

fn attr_value<'a>(parsed: &'a ParsedTag, name: &str) -> Option<&'a str> {
    parsed
        .attrs
        .iter()
        .find(|attr| attr.name == name)
        .and_then(|attr| attr.value.as_deref())
}

fn dynamic_argument_expression(attr_name: &str) -> Option<&str> {
    let start = attr_name.find('[')?;
    let (expr, _) = scan_bracket_section(attr_name, start)?;
    let expr = expr.trim();
    (!expr.is_empty()).then_some(expr)
}

fn current_locals(scopes: &[Vec<String>]) -> Vec<String> {
    scopes
        .iter()
        .flat_map(|locals| locals.iter().cloned())
        .collect()
}

fn mark_tag_usage(
    tag_name: &str,
    imported_bindings: &FxHashSet<String>,
    locals: &[String],
    usage: &mut TemplateUsage,
) {
    if tag_name.is_empty() || is_builtin_component(tag_name) {
        return;
    }

    merge_component_tag_usage(usage, tag_name, imported_bindings, locals, true);
}

fn mark_custom_directive_usage(
    attr_name: &str,
    imported_bindings: &FxHashSet<String>,
    usage: &mut TemplateUsage,
) {
    let Some(directive_name) = directive_name(attr_name) else {
        return;
    };

    if is_builtin_directive(directive_name) {
        return;
    }

    let mut binding = String::from("v");
    binding.push_str(&to_pascal_case(directive_name));
    if imported_bindings.contains(binding.as_str()) {
        usage.used_bindings.insert(binding);
    }
}

fn directive_name(attr_name: &str) -> Option<&str> {
    attr_name
        .strip_prefix("v-")?
        .split([':', '.'])
        .next()
        .map(str::trim)
        .filter(|name| !name.is_empty())
}

/// Vue 3.4+ same-name `v-bind` shorthand: a value-less `:arg` (or `v-bind:arg`)
/// is shorthand for `:arg="arg"`, so the argument name references a local
/// binding (`:open` = `:open="open"`, `:some-prop` = `:some-prop="someProp"`).
/// Only fires for a value-less attribute; with an explicit value the value
/// expression names the reference and the bare argument is the binding target.
/// A dynamic argument (`:[expr]`) has no static same-name form and is credited
/// separately via `dynamic_argument_expression`. Modifiers (`:foo.prop`) do not
/// change the referenced variable, so the argument before the first `.` wins.
/// The argument is camelized via `kebab_to_camel_case` to match Vue's own
/// `camelize` (hyphen-only, so `:some_prop` stays `some_prop`).
fn vbind_shorthand_binding(attr_name: &str) -> Option<String> {
    let arg = attr_name
        .strip_prefix(':')
        .or_else(|| attr_name.strip_prefix("v-bind:"))?;
    if arg.starts_with('[') {
        return None;
    }
    let name = arg
        .split('.')
        .next()
        .map(str::trim)
        .filter(|n| !n.is_empty())?;
    Some(kebab_to_camel_case(name))
}

fn is_custom_directive_attr(name: &str) -> bool {
    directive_name(name).is_some_and(|directive| !is_builtin_directive(directive))
}

fn to_pascal_case(name: &str) -> String {
    let mut result = String::new();
    let mut uppercase_next = true;
    for ch in name.chars() {
        if matches!(ch, '-' | '_' | ':') {
            uppercase_next = true;
            continue;
        }
        if uppercase_next {
            result.extend(ch.to_uppercase());
            uppercase_next = false;
        } else {
            result.push(ch);
        }
    }
    result
}

fn is_builtin_component(tag_name: &str) -> bool {
    matches!(
        tag_name,
        "component"
            | "Component"
            | "slot"
            | "Slot"
            | "template"
            | "Template"
            | "transition"
            | "Transition"
            | "transition-group"
            | "TransitionGroup"
            | "keep-alive"
            | "KeepAlive"
            | "teleport"
            | "Teleport"
            | "suspense"
            | "Suspense"
    )
}

fn is_builtin_directive(name: &str) -> bool {
    matches!(
        name,
        "bind"
            | "cloak"
            | "else"
            | "else-if"
            | "for"
            | "html"
            | "if"
            | "memo"
            | "model"
            | "once"
            | "on"
            | "pre"
            | "show"
            | "slot"
            | "text"
    )
}

fn is_statement_attr(name: &str) -> bool {
    name.starts_with('@') || name.starts_with("v-on:")
}

fn is_expression_attr(name: &str) -> bool {
    name.starts_with(':')
        || name.starts_with("v-bind:")
        || matches!(
            name,
            "v-if"
                | "v-else-if"
                | "v-show"
                | "v-html"
                | "v-text"
                | "v-memo"
                | "v-model"
                | "v-on"
                | "v-bind"
        )
        || name.starts_with("v-model:")
}

#[cfg(test)]
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
    fn mustache_marks_binding_used() {
        let usage = collect_template_usage(
            "<script setup>import { formatDate } from './utils';</script><template><p>{{ formatDate(value) }}</p></template>",
            &imported(&["formatDate"]),
        );

        assert!(usage.used_bindings.contains("formatDate"));
    }

    #[test]
    fn nested_slot_template_does_not_truncate_root_body() {
        // A `<template #slot>` inside a component must NOT terminate the root
        // template body at its `</template>`; components rendered AFTER it stay
        // credited. Regression for the non-greedy `</template>` body capture.
        let usage = collect_template_usage(
            "<template><Header><template #logo>x</template></Header><Content /></template>",
            &imported(&["Header", "Content"]),
        );

        assert!(usage.used_bindings.contains("Header"));
        assert!(
            usage.used_bindings.contains("Content"),
            "component after a nested slot template must still be credited"
        );
    }

    #[test]
    fn multibyte_text_before_nested_template_does_not_panic() {
        // Depth-aware body scanning must use byte-safe slicing: CJK text between
        // tags must not cause a non-char-boundary panic, and the trailing
        // component must still be credited.
        let usage = collect_template_usage(
            "<template><Header>住所<template #logo>都市</template></Header><Content />住所</template>",
            &imported(&["Header", "Content"]),
        );
        assert!(usage.used_bindings.contains("Header"));
        assert!(usage.used_bindings.contains("Content"));
    }

    #[test]
    fn deeply_nested_slot_templates_credit_trailing_components() {
        let usage = collect_template_usage(
            "<template><A><template #a><B><template #b>y</template></B></template></A><C /><D /></template>",
            &imported(&["A", "B", "C", "D"]),
        );
        for name in ["A", "B", "C", "D"] {
            assert!(
                usage.used_bindings.contains(name),
                "{name} should be credited across nested slot templates"
            );
        }
    }

    #[test]
    fn closing_template_tag_with_interior_whitespace_is_matched() {
        // Prettier emits `</template >` (space before `>`) for slot blocks. The
        // root close must still be recognized so `find_template_body_end` does not
        // return `None` and drop the whole body, falsely reporting every
        // template-only binding unused. Regression for issue #1439.
        for source in [
            "<template><Content /></template >",
            "<template><Content /></template\n>",
        ] {
            let usage = collect_template_usage(source, &imported(&["Content"]));
            assert!(
                usage.used_bindings.contains("Content"),
                "component must stay credited when the root close tag has interior whitespace: {source:?}"
            );
        }
    }

    #[test]
    fn nested_slot_close_with_whitespace_does_not_truncate_root_body() {
        // A `</template >` closing a nested slot must decrement depth like the
        // exact form, so the trailing component after it stays credited.
        let usage = collect_template_usage(
            "<template><Header><template #logo>x</template ></Header><Content /></template >",
            &imported(&["Header", "Content"]),
        );
        assert!(usage.used_bindings.contains("Header"));
        assert!(usage.used_bindings.contains("Content"));
    }

    #[test]
    fn whitespace_close_inside_comment_or_attribute_does_not_truncate() {
        // A `</template >` appearing in an HTML comment or a quoted attribute must
        // not be treated as the root close: comments are skipped and `scan_html_tag`
        // is quote-aware, so the real close still ends the body and trailing
        // bindings stay credited.
        for source in [
            "<template><!-- </template > --><Content /></template >",
            "<template><div title=\"</template >\"></div><Content /></template >",
        ] {
            let usage = collect_template_usage(source, &imported(&["Content"]));
            assert!(
                usage.used_bindings.contains("Content"),
                "spurious whitespace close must not truncate the body: {source:?}"
            );
        }
    }

    #[test]
    fn v_for_alias_shadows_import_name() {
        let usage = collect_template_usage(
            "<script setup>import { item } from './utils';</script><template><li v-for=\"item in items\">{{ item }}</li></template>",
            &imported(&["item"]),
        );

        assert!(usage.is_empty());
    }

    #[test]
    fn slot_scope_alias_shadows_import_name() {
        let usage = collect_template_usage(
            "<script setup>import { item } from './utils';</script><template><List v-slot=\"{ item }\">{{ item }}</List></template>",
            &imported(&["item"]),
        );

        assert!(usage.used_bindings.is_empty());
        assert!(usage.member_accesses.is_empty());
    }

    #[test]
    fn namespace_member_accesses_are_retained() {
        let usage = collect_template_usage(
            "<script setup>import * as utils from './utils';</script><template><p>{{ utils.formatDate(value) }}</p></template>",
            &imported(&["utils"]),
        );

        assert!(usage.used_bindings.contains("utils"));
        assert_eq!(usage.member_accesses.len(), 1);
        assert_eq!(usage.member_accesses[0].object, "utils");
        assert_eq!(usage.member_accesses[0].member, "formatDate");
    }

    #[test]
    fn event_handlers_are_treated_as_statements() {
        let usage = collect_template_usage(
            "<script setup>import { increment } from './utils';</script><template><button @click=\"count += increment(step)\">Add</button></template>",
            &imported(&["increment"]),
        );

        assert!(usage.used_bindings.contains("increment"));
    }

    #[test]
    fn v_bind_object_syntax_marks_binding_used() {
        let usage = collect_template_usage(
            "<script setup>import { attrs } from './utils';</script><template><button v-bind=\"attrs\">Add</button></template>",
            &imported(&["attrs"]),
        );

        assert!(usage.used_bindings.contains("attrs"));
    }

    #[test]
    fn v_on_object_syntax_marks_binding_used() {
        let usage = collect_template_usage(
            "<script setup>import { handlers } from './utils';</script><template><button v-on=\"handlers\">Add</button></template>",
            &imported(&["handlers"]),
        );

        assert!(usage.used_bindings.contains("handlers"));
    }

    #[test]
    fn component_tags_mark_imported_components_used() {
        let usage = collect_template_usage(
            "<script setup>import FancyCard from './FancyCard.vue';</script><template><FancyCard /><fancy-card /></template>",
            &imported(&["FancyCard"]),
        );

        assert!(usage.used_bindings.contains("FancyCard"));
    }

    #[test]
    fn namespaced_component_tags_record_member_usage() {
        let usage = collect_template_usage(
            "<script setup>import * as Form from './form';</script><template><Form.Input /></template>",
            &imported(&["Form"]),
        );

        assert!(usage.used_bindings.contains("Form"));
        assert_eq!(usage.member_accesses.len(), 1);
        assert_eq!(usage.member_accesses[0].object, "Form");
        assert_eq!(usage.member_accesses[0].member, "Input");
    }

    #[test]
    fn local_slot_bindings_shadow_imported_component_tags() {
        let usage = collect_template_usage(
            "<script setup>import { Item } from './components';</script><template><List v-slot=\"{ Item }\"><Item /></List></template>",
            &imported(&["Item"]),
        );

        assert!(usage.used_bindings.is_empty());
        assert!(usage.member_accesses.is_empty());
    }

    #[test]
    fn custom_directives_mark_imported_bindings_used() {
        let usage = collect_template_usage(
            "<script setup>import { vFocusTrap } from './directives';</script><template><input v-focus-trap /></template>",
            &imported(&["vFocusTrap"]),
        );

        assert!(usage.used_bindings.contains("vFocusTrap"));
    }

    #[test]
    fn custom_directive_values_mark_imported_bindings_used() {
        let usage = collect_template_usage(
            "<script setup>import { tooltipText } from './utils';</script><template><div v-tooltip=\"tooltipText\" /></template>",
            &imported(&["tooltipText"]),
        );

        assert!(usage.used_bindings.contains("tooltipText"));
    }

    #[test]
    fn dynamic_v_bind_argument_marks_binding_used() {
        let usage = collect_template_usage(
            "<script setup>import { dynamicAttr } from './utils';</script><template><div v-bind:[dynamicAttr]=\"value\" /></template>",
            &imported(&["dynamicAttr"]),
        );

        assert!(usage.used_bindings.contains("dynamicAttr"));
    }

    #[test]
    fn nested_dynamic_v_bind_argument_marks_all_bindings_used() {
        let usage = collect_template_usage(
            "<script setup>import { activeField, fieldMap } from './utils';</script><template><div v-bind:[fieldMap[activeField]]=\"value\" /></template>",
            &imported(&["activeField", "fieldMap"]),
        );

        assert!(usage.used_bindings.contains("activeField"));
        assert!(usage.used_bindings.contains("fieldMap"));
    }

    #[test]
    fn dynamic_v_on_argument_marks_binding_used() {
        let usage = collect_template_usage(
            "<script setup>import { dynamicEvent } from './utils';</script><template><button v-on:[dynamicEvent]=\"handleClick\" /></template>",
            &imported(&["dynamicEvent"]),
        );

        assert!(usage.used_bindings.contains("dynamicEvent"));
    }

    #[test]
    fn dynamic_v_slot_argument_ignores_slot_scope_shadowing() {
        let usage = collect_template_usage(
            "<script setup>import { slotName } from './utils';</script><template><List v-slot:[slotName]=\"{ slotName }\">{{ slotName }}</List></template>",
            &imported(&["slotName"]),
        );

        assert!(usage.used_bindings.contains("slotName"));
    }

    #[test]
    fn slot_default_initializers_mark_imported_bindings_used() {
        let usage = collect_template_usage(
            "<script setup>import { fallbackItem } from './utils';</script><template><List v-slot=\"{ item = fallbackItem }\">{{ item }}</List></template>",
            &imported(&["fallbackItem"]),
        );

        assert!(usage.used_bindings.contains("fallbackItem"));
    }

    #[test]
    fn v_for_typed_destructure_does_not_infinite_recurse() {
        let usage = collect_template_usage(
            "<script setup>import { id } from './utils';</script><template><li v-for=\"({ id, name }: Item) in items\">{{ id }}</li></template>",
            &imported(&["id"]),
        );

        assert!(usage.is_empty());
    }

    #[test]
    fn v_slot_typed_destructure_does_not_infinite_recurse() {
        let usage = collect_template_usage(
            "<script setup>import { data } from './utils';</script><template><List v-slot=\"{ data, loading }: QueryResult\">{{ data }}</List></template>",
            &imported(&["data"]),
        );

        assert!(usage.used_bindings.is_empty());
        assert!(usage.member_accesses.is_empty());
    }

    #[test]
    fn v_for_typed_destructure_still_tracks_iterable() {
        let usage = collect_template_usage(
            "<script setup>import { items } from './data';</script><template><li v-for=\"({ id }: Item) in items\">{{ id }}</li></template>",
            &imported(&["items"]),
        );

        assert!(usage.used_bindings.contains("items"));
    }

    #[test]
    fn event_handler_member_call_maps_script_instance_to_class() {
        let usage = collect_template_usage_with_bound_targets(
            "<template><button @click=\"counter.bump()\">{{ counter.value }}</button></template>",
            &imported(&[]),
            &bound_targets(&[("counter", "Counter")]),
            &FxHashMap::default(),
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
    fn v_for_local_shadows_script_instance_binding() {
        let usage = collect_template_usage_with_bound_targets(
            "<template><button v-for=\"counter in counters\" @click=\"other.go(); counter.bump()\" /></template>",
            &imported(&[]),
            &bound_targets(&[("counter", "Counter"), ("other", "Other")]),
            &FxHashMap::default(),
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

    fn iterable_types(pairs: &[(&str, &str)]) -> FxHashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect()
    }

    #[test]
    fn v_for_item_typed_to_element_class_credits_member_access() {
        // `v-for="(util, index) of utils"` where `utils` is `Util[]`: the item
        // `util`'s member accesses credit the `Util` class (issue #1707).
        let usage = collect_template_usage_with_bound_targets(
            "<template><template v-for=\"(util, index) of utils\" :key=\"index\">{{ util.getter }} {{ util.property }} {{ util.hello() }}</template></template>",
            &imported(&[]),
            &FxHashMap::default(),
            &iterable_types(&[("utils", "Util")]),
        );

        for member in ["getter", "property", "hello"] {
            assert!(
                usage
                    .member_accesses
                    .iter()
                    .any(|access| access.object == "Util" && access.member == member),
                "util.{member} should map to Util.{member}, found: {:?}",
                usage.member_accesses
            );
        }
    }

    #[test]
    fn v_for_item_typed_without_alias_tuple_credits_member_access() {
        // Bare item form `v-for="util of utils"` (no `(item, index)` tuple).
        let usage = collect_template_usage_with_bound_targets(
            "<template><li v-for=\"util of utils\">{{ util.getter }}</li></template>",
            &imported(&[]),
            &FxHashMap::default(),
            &iterable_types(&[("utils", "Util")]),
        );

        assert!(
            usage
                .member_accesses
                .iter()
                .any(|access| access.object == "Util" && access.member == "getter"),
            "util.getter should map to Util.getter, found: {:?}",
            usage.member_accesses
        );
    }

    #[test]
    fn v_for_index_alias_is_not_typed_as_element_class() {
        // The second alias (`index`) is a number, never the element class: a
        // member access on it must not remap onto the class.
        let usage = collect_template_usage_with_bound_targets(
            "<template><li v-for=\"(util, index) of utils\">{{ index.toFixed() }}</li></template>",
            &imported(&[]),
            &FxHashMap::default(),
            &iterable_types(&[("utils", "Util")]),
        );

        assert!(
            !usage
                .member_accesses
                .iter()
                .any(|access| access.object == "Util"),
            "index member access must not map to the element class, found: {:?}",
            usage.member_accesses
        );
    }

    #[test]
    fn v_for_untyped_source_leaves_item_unmapped() {
        // Neuter check: with no known element type for the source, the item stays
        // a local and its member accesses are NOT credited (pre-fix behavior).
        let usage = collect_template_usage_with_bound_targets(
            "<template><li v-for=\"(util, index) of utils\">{{ util.getter }}</li></template>",
            &imported(&[]),
            &FxHashMap::default(),
            &FxHashMap::default(),
        );

        assert!(
            !usage
                .member_accesses
                .iter()
                .any(|access| access.object == "Util"),
            "untyped v-for item must not credit any class, found: {:?}",
            usage.member_accesses
        );
    }

    #[test]
    fn v_for_destructured_item_is_not_typed() {
        // A destructured item (`{ id }`) has no bare identifier to type; the scan
        // declines to bind it (no spurious class member credit).
        let usage = collect_template_usage_with_bound_targets(
            "<template><li v-for=\"({ id }, index) of utils\">{{ id }}</li></template>",
            &imported(&[]),
            &FxHashMap::default(),
            &iterable_types(&[("utils", "Util")]),
        );

        assert!(
            !usage
                .member_accesses
                .iter()
                .any(|access| access.object == "Util"),
            "destructured item must not credit the element class, found: {:?}",
            usage.member_accesses
        );
    }

    #[test]
    fn v_for_inside_comment_does_not_type_template_reference() {
        // The pre-scan is scoped to template bodies with comments excluded (same
        // as the streaming scan), so a `v-for=` inside an HTML comment must not
        // type a later bare `util.x` reference elsewhere in the template. Here the
        // `<span>{{ util.getter }}` is outside any real v-for, so `util` is unbound
        // and must credit nothing (issue #1707 hardening).
        let usage = collect_template_usage_with_bound_targets(
            "<template><!-- <li v-for=\"util of utils\">{{ util.getter }}</li> --><span>{{ util.getter }}</span></template>",
            &imported(&[]),
            &FxHashMap::default(),
            &iterable_types(&[("utils", "Util")]),
        );

        assert!(
            !usage
                .member_accesses
                .iter()
                .any(|access| access.object == "Util"),
            "a v-for in a comment must not type a template reference, found: {:?}",
            usage.member_accesses
        );
    }
}
