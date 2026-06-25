use std::sync::LazyLock;

use rustc_hash::{FxHashMap, FxHashSet};

use crate::template_usage::TemplateUsage;

use super::scanners::{scan_bracket_section, scan_curly_section, scan_html_tag};
use super::shared::{
    HTML_COMMENT_RE, ParsedAttr, ParsedTag, merge_component_tag_usage,
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
    let comment_ranges: Vec<(usize, usize)> = HTML_COMMENT_RE
        .find_iter(source)
        .map(|m| (m.start(), m.end()))
        .collect();

    let mut usage = TemplateUsage::default();
    // Cursor past the last fully-consumed root template body, so nested
    // `<template>` opens (which `TEMPLATE_OPEN_RE` also matches) are skipped
    // rather than rescanned as separate roots.
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
        usage.merge(scan_template_body(
            &source[body_start..body_end],
            body_start,
            imported_bindings,
            bound_targets,
        ));
        scan_from = body_end;
    }

    usage
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

    let v_for_locals =
        collect_v_for_locals(&parsed, imported_bindings, bound_targets, &current, usage);
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
    merge_pattern_binding_usage(usage, binding, imported_bindings, current)
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
}
