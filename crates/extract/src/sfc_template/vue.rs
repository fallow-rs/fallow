use std::sync::LazyLock;

use rustc_hash::{FxHashMap, FxHashSet};

use crate::template_usage::TemplateUsage;

use super::scanners::{scan_bracket_section, scan_curly_section, scan_html_tag};
use super::shared::{
    HTML_COMMENT_RE, merge_component_tag_usage, merge_expression_usage_with_bound_targets,
    merge_pattern_binding_usage, merge_statement_usage_with_bound_targets, parse_tag_attrs,
};

static TEMPLATE_BLOCK_RE: LazyLock<regex::Regex> = LazyLock::new(|| {
    crate::static_regex(
        r#"(?is)<template\b(?:[^>"']|"[^"]*"|'[^']*')*>(?P<body>[\s\S]*?)</template>"#,
    )
});

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
    for cap in TEMPLATE_BLOCK_RE.captures_iter(source) {
        let Some(template_match) = cap.get(0) else {
            continue;
        };
        if comment_ranges
            .iter()
            .any(|&(start, end)| template_match.start() >= start && template_match.start() < end)
        {
            continue;
        }
        let Some(body_match) = cap.name("body") else {
            continue;
        };
        usage.merge(scan_template_body(
            body_match.as_str(),
            body_match.start(),
            imported_bindings,
            bound_targets,
        ));
    }

    usage
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
            apply_tag(
                tag,
                body_offset + index,
                body_offset + next_index,
                imported_bindings,
                bound_targets,
                &mut scopes,
                &mut usage,
            );
            index = next_index;
            continue;
        }

        index += 1;
    }

    usage
}

fn apply_tag(
    tag: &str,
    tag_start: usize,
    tag_end: usize,
    imported_bindings: &FxHashSet<String>,
    bound_targets: &FxHashMap<String, String>,
    scopes: &mut Vec<Vec<String>>,
    usage: &mut TemplateUsage,
) {
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
    if let Some(value) = parsed
        .attrs
        .iter()
        .find(|attr| attr.name == "v-html")
        .and_then(|attr| attr.value.as_deref())
        && let Some(sink) = crate::template_usage::template_html_sink(value, tag_start, tag_end)
    {
        usage.security_sinks.push(sink);
    }

    let mut v_for_locals = Vec::new();
    let mut slot_locals = Vec::new();
    if let Some(value) = parsed
        .attrs
        .iter()
        .find(|attr| attr.name == "v-for")
        .and_then(|attr| attr.value.as_deref())
        && let Some(captures) = VUE_FOR_RE.captures(value)
    {
        let binding = captures.name("binding").map_or("", |m| m.as_str()).trim();
        let source_expr = captures.name("source").map_or("", |m| m.as_str()).trim();
        merge_expression_usage_with_bound_targets(
            usage,
            source_expr,
            imported_bindings,
            bound_targets,
            &current,
        );
        v_for_locals.extend(merge_pattern_binding_usage(
            usage,
            binding,
            imported_bindings,
            &current,
        ));
    }

    if let Some(value) = parsed
        .attrs
        .iter()
        .find(|attr| {
            attr.name == "slot-scope"
                || attr.name.starts_with("v-slot")
                || attr.name.starts_with('#')
        })
        .and_then(|attr| attr.value.as_deref())
    {
        slot_locals.extend(merge_pattern_binding_usage(
            usage,
            value,
            imported_bindings,
            &current,
        ));
    }

    let mut element_locals = v_for_locals.clone();
    element_locals.extend(slot_locals);

    let mut attr_locals = current.clone();
    attr_locals.extend(element_locals.iter().cloned());
    let mut arg_locals = current;
    arg_locals.extend(v_for_locals);

    for attr in &parsed.attrs {
        mark_custom_directive_usage(&attr.name, imported_bindings, usage);
        if let Some(expr) = dynamic_argument_expression(&attr.name) {
            merge_expression_usage_with_bound_targets(
                usage,
                expr,
                imported_bindings,
                bound_targets,
                &arg_locals,
            );
        }
        if let Some(expr) = attr.value.as_deref() {
            if attr.name == "v-for"
                || attr.name == "slot-scope"
                || attr.name.starts_with("v-slot")
                || attr.name.starts_with('#')
            {
                continue;
            }

            if is_statement_attr(&attr.name) {
                merge_statement_usage_with_bound_targets(
                    usage,
                    expr,
                    imported_bindings,
                    bound_targets,
                    &attr_locals,
                );
            } else if is_expression_attr(&attr.name) || is_custom_directive_attr(&attr.name) {
                merge_expression_usage_with_bound_targets(
                    usage,
                    expr,
                    imported_bindings,
                    bound_targets,
                    &attr_locals,
                );
            }
        }
    }

    if !parsed.self_closing {
        scopes.push(element_locals);
    }
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
