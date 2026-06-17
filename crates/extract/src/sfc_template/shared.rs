use std::sync::LazyLock;

use rustc_hash::{FxHashMap, FxHashSet};

use crate::template_usage::{
    TemplateSnippetKind, TemplateUsage, analyze_template_snippet,
    analyze_template_snippet_with_bound_targets,
};

use super::scanners::scan_curly_section;

/// Regex for stripping HTML comments (`<!-- ... -->`), shared by Vue and Svelte.
pub(super) static HTML_COMMENT_RE: LazyLock<regex::Regex> =
    LazyLock::new(|| crate::static_regex(r"(?s)<!--.*?-->"));

pub(super) fn merge_expression_usage(
    usage: &mut TemplateUsage,
    snippet: &str,
    imported_bindings: &FxHashSet<String>,
    locals: &[String],
) {
    merge_snippet_usage(
        usage,
        snippet,
        TemplateSnippetKind::Expression,
        imported_bindings,
        locals,
        false,
    );
}

#[cfg(test)]
pub(super) fn merge_statement_usage(
    usage: &mut TemplateUsage,
    snippet: &str,
    imported_bindings: &FxHashSet<String>,
    locals: &[String],
) {
    merge_snippet_usage(
        usage,
        snippet,
        TemplateSnippetKind::Statement,
        imported_bindings,
        locals,
        false,
    );
}

#[cfg(test)]
pub(super) fn merge_expression_usage_allow_dollar_refs(
    usage: &mut TemplateUsage,
    snippet: &str,
    imported_bindings: &FxHashSet<String>,
    locals: &[String],
) {
    merge_snippet_usage(
        usage,
        snippet,
        TemplateSnippetKind::Expression,
        imported_bindings,
        locals,
        true,
    );
}

#[cfg(test)]
pub(super) fn merge_statement_usage_allow_dollar_refs(
    usage: &mut TemplateUsage,
    snippet: &str,
    imported_bindings: &FxHashSet<String>,
    locals: &[String],
) {
    merge_snippet_usage(
        usage,
        snippet,
        TemplateSnippetKind::Statement,
        imported_bindings,
        locals,
        true,
    );
}

pub(super) fn merge_expression_usage_with_bound_targets(
    usage: &mut TemplateUsage,
    snippet: &str,
    imported_bindings: &FxHashSet<String>,
    bound_targets: &FxHashMap<String, String>,
    locals: &[String],
) {
    merge_snippet_usage_with_bound_targets(&mut BoundSnippetUsageInput {
        usage,
        snippet,
        kind: TemplateSnippetKind::Expression,
        imported_bindings,
        bound_targets,
        locals,
        allow_dollar_prefixed_refs: false,
    });
}

pub(super) fn merge_statement_usage_with_bound_targets(
    usage: &mut TemplateUsage,
    snippet: &str,
    imported_bindings: &FxHashSet<String>,
    bound_targets: &FxHashMap<String, String>,
    locals: &[String],
) {
    merge_snippet_usage_with_bound_targets(&mut BoundSnippetUsageInput {
        usage,
        snippet,
        kind: TemplateSnippetKind::Statement,
        imported_bindings,
        bound_targets,
        locals,
        allow_dollar_prefixed_refs: false,
    });
}

pub(super) fn merge_expression_usage_allow_dollar_refs_with_bound_targets(
    usage: &mut TemplateUsage,
    snippet: &str,
    imported_bindings: &FxHashSet<String>,
    bound_targets: &FxHashMap<String, String>,
    locals: &[String],
) {
    merge_snippet_usage_with_bound_targets(&mut BoundSnippetUsageInput {
        usage,
        snippet,
        kind: TemplateSnippetKind::Expression,
        imported_bindings,
        bound_targets,
        locals,
        allow_dollar_prefixed_refs: true,
    });
}

pub(super) fn merge_statement_usage_allow_dollar_refs_with_bound_targets(
    usage: &mut TemplateUsage,
    snippet: &str,
    imported_bindings: &FxHashSet<String>,
    bound_targets: &FxHashMap<String, String>,
    locals: &[String],
) {
    merge_snippet_usage_with_bound_targets(&mut BoundSnippetUsageInput {
        usage,
        snippet,
        kind: TemplateSnippetKind::Statement,
        imported_bindings,
        bound_targets,
        locals,
        allow_dollar_prefixed_refs: true,
    });
}

fn merge_snippet_usage(
    usage: &mut TemplateUsage,
    snippet: &str,
    kind: TemplateSnippetKind,
    imported_bindings: &FxHashSet<String>,
    locals: &[String],
    allow_dollar_prefixed_refs: bool,
) {
    usage.merge(analyze_template_snippet(
        snippet,
        kind,
        imported_bindings,
        locals,
        allow_dollar_prefixed_refs,
    ));
}

struct BoundSnippetUsageInput<'a> {
    usage: &'a mut TemplateUsage,
    snippet: &'a str,
    kind: TemplateSnippetKind,
    imported_bindings: &'a FxHashSet<String>,
    bound_targets: &'a FxHashMap<String, String>,
    locals: &'a [String],
    allow_dollar_prefixed_refs: bool,
}

fn merge_snippet_usage_with_bound_targets(input: &mut BoundSnippetUsageInput<'_>) {
    input
        .usage
        .merge(analyze_template_snippet_with_bound_targets(
            input.snippet,
            input.kind,
            input.imported_bindings,
            input.bound_targets,
            input.locals,
            input.allow_dollar_prefixed_refs,
        ));
}

pub(super) fn merge_component_tag_usage(
    usage: &mut TemplateUsage,
    tag_name: &str,
    imported_bindings: &FxHashSet<String>,
    locals: &[String],
    allow_kebab_case: bool,
) {
    let tag_name = tag_name.trim();
    if tag_name.is_empty() {
        return;
    }

    if tag_name.contains('.') {
        merge_expression_usage(usage, tag_name, imported_bindings, locals);
        return;
    }

    mark_binding_used(usage, tag_name, imported_bindings, locals);

    if allow_kebab_case && tag_name.contains('-') {
        let camel = kebab_to_camel_case(tag_name);
        if !camel.is_empty() {
            mark_binding_used(usage, &camel, imported_bindings, locals);
            let pascal = uppercase_first(&camel);
            mark_binding_used(usage, &pascal, imported_bindings, locals);
        }
    }
}

fn mark_binding_used(
    usage: &mut TemplateUsage,
    binding: &str,
    imported_bindings: &FxHashSet<String>,
    locals: &[String],
) {
    if binding.is_empty() || locals.iter().any(|local| local == binding) {
        return;
    }

    if imported_bindings.contains(binding) {
        usage.used_bindings.insert(binding.to_string());
        return;
    }

    if binding
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_uppercase())
    {
        usage.unresolved_tag_names.insert(binding.to_string());
    }
}

fn kebab_to_camel_case(source: &str) -> String {
    let mut camel = String::new();
    let mut uppercase_next = false;

    for ch in source.chars() {
        if ch == '-' {
            uppercase_next = true;
            continue;
        }

        if uppercase_next {
            camel.extend(ch.to_uppercase());
            uppercase_next = false;
        } else {
            camel.push(ch);
        }
    }

    camel
}

fn uppercase_first(source: &str) -> String {
    let mut chars = source.chars();
    let Some(first) = chars.next() else {
        return String::new();
    };

    let mut output = String::new();
    output.extend(first.to_uppercase());
    output.push_str(chars.as_str());
    output
}

pub(super) fn merge_pattern_binding_usage(
    usage: &mut TemplateUsage,
    pattern: &str,
    imported_bindings: &FxHashSet<String>,
    locals: &[String],
) -> Vec<String> {
    let mut bindings = Vec::new();
    collect_pattern_usage(usage, pattern, imported_bindings, locals, &mut bindings);
    bindings
}

fn collect_pattern_usage(
    usage: &mut TemplateUsage,
    pattern: &str,
    imported_bindings: &FxHashSet<String>,
    locals: &[String],
    bindings: &mut Vec<String>,
) {
    let pattern = trim_outer_parens(pattern.trim());
    let pattern = pattern.strip_prefix("...").unwrap_or(pattern).trim();
    if pattern.is_empty() {
        return;
    }

    if pattern.contains(',') {
        let parts = split_top_level(pattern, ',');
        if parts.len() > 1 {
            for part in parts {
                collect_pattern_usage(usage, part.trim(), imported_bindings, locals, bindings);
            }
            return;
        }
    }

    let pattern = strip_trailing_type_annotation(pattern);

    if let Some(inner) = strip_wrapping(pattern, '{', '}') {
        for part in split_top_level(inner, ',') {
            let part = part.trim();
            if part.is_empty() || part == "..." {
                continue;
            }
            if let Some((_, rhs)) = split_top_level_once(part, ':') {
                collect_pattern_usage(usage, rhs, imported_bindings, locals, bindings);
                continue;
            }
            if let Some((lhs, rhs)) = split_top_level_once(part, '=') {
                merge_expression_usage(usage, rhs, imported_bindings, locals);
                collect_pattern_usage(usage, lhs, imported_bindings, locals, bindings);
                continue;
            }
            collect_pattern_usage(usage, part, imported_bindings, locals, bindings);
        }
        return;
    }

    if let Some(inner) = strip_wrapping(pattern, '[', ']') {
        for part in split_top_level(inner, ',') {
            collect_pattern_usage(usage, part.trim(), imported_bindings, locals, bindings);
        }
        return;
    }

    if let Some((lhs, rhs)) = split_top_level_once(pattern, '=') {
        merge_expression_usage(usage, rhs, imported_bindings, locals);
        collect_pattern_usage(usage, lhs, imported_bindings, locals, bindings);
        return;
    }

    if let Some(ident) = valid_identifier(pattern) {
        bindings.push(ident.to_string());
    }
}

pub(super) fn extract_pattern_binding_names(pattern: &str) -> Vec<String> {
    let pattern = trim_outer_parens(pattern.trim());
    let pattern = pattern.strip_prefix("...").unwrap_or(pattern).trim();
    if pattern.is_empty() {
        return Vec::new();
    }

    if pattern.contains(',') {
        let parts = split_top_level(pattern, ',');
        if parts.len() > 1 {
            return parts
                .into_iter()
                .flat_map(|part| extract_pattern_binding_names(part.trim()))
                .collect();
        }
    }

    let pattern = strip_trailing_type_annotation(pattern);

    if let Some(inner) = strip_wrapping(pattern, '{', '}') {
        return split_top_level(inner, ',')
            .into_iter()
            .flat_map(|part| {
                let part = part.trim();
                if part.is_empty() || part == "..." {
                    return Vec::new();
                }
                if let Some((_, rhs)) = split_top_level_once(part, ':') {
                    return extract_pattern_binding_names(rhs);
                }
                if let Some((lhs, _)) = split_top_level_once(part, '=') {
                    return extract_pattern_binding_names(lhs);
                }
                extract_pattern_binding_names(part)
            })
            .collect();
    }

    if let Some(inner) = strip_wrapping(pattern, '[', ']') {
        return split_top_level(inner, ',')
            .into_iter()
            .flat_map(|part| extract_pattern_binding_names(part.trim()))
            .collect();
    }

    if let Some((lhs, _)) = split_top_level_once(pattern, '=') {
        return extract_pattern_binding_names(lhs);
    }

    valid_identifier(pattern)
        .map(|ident| vec![ident.to_string()])
        .unwrap_or_default()
}

fn split_top_level(source: &str, delimiter: char) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut start = 0;
    let mut depth = 0_i32;
    let mut in_single = false;
    let mut in_double = false;
    let mut in_backtick = false;
    let mut escape = false;

    for (idx, ch) in source.char_indices() {
        if escape {
            escape = false;
            continue;
        }
        match ch {
            '\\' if in_single || in_double || in_backtick => {
                escape = true;
            }
            '\'' if !in_double && !in_backtick => in_single = !in_single,
            '"' if !in_single && !in_backtick => in_double = !in_double,
            '`' if !in_single && !in_double => in_backtick = !in_backtick,
            _ if in_single || in_double || in_backtick => {}
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth -= 1,
            _ if ch == delimiter && depth == 0 => {
                parts.push(&source[start..idx]);
                start = idx + ch.len_utf8();
            }
            _ => {}
        }
    }

    parts.push(&source[start..]);
    parts
}

fn split_top_level_once(source: &str, delimiter: char) -> Option<(&str, &str)> {
    let mut depth = 0_i32;
    let mut in_single = false;
    let mut in_double = false;
    let mut in_backtick = false;
    let mut escape = false;

    for (idx, ch) in source.char_indices() {
        if escape {
            escape = false;
            continue;
        }
        match ch {
            '\\' if in_single || in_double || in_backtick => {
                escape = true;
            }
            '\'' if !in_double && !in_backtick => in_single = !in_single,
            '"' if !in_single && !in_backtick => in_double = !in_double,
            '`' if !in_single && !in_double => in_backtick = !in_backtick,
            _ if in_single || in_double || in_backtick => {}
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth -= 1,
            _ if ch == delimiter && depth == 0 => {
                let rhs = &source[idx + ch.len_utf8()..];
                return Some((&source[..idx], rhs));
            }
            _ => {}
        }
    }
    None
}

fn strip_wrapping(source: &str, open: char, close: char) -> Option<&str> {
    source
        .strip_prefix(open)
        .and_then(|inner| inner.strip_suffix(close))
}

/// Strip a trailing TypeScript type annotation from a single binding pattern.
///
/// Handles `{ a, b }: Props` → `{ a, b }`, `[a, b]: number[]` → `[a, b]`,
/// and plain `name: Type` → `name`. Returns the substring before the first
/// top-level `:` (outside brackets and quoted strings), or the input unchanged
/// when no such colon exists.
///
/// The caller must split multi-binding patterns on top-level commas first;
/// otherwise a tuple type like `x: [number, number]` followed by `, y` would
/// be misinterpreted, and even without a second binding the colon-before-comma
/// rule could not be enforced.
fn strip_trailing_type_annotation(pattern: &str) -> &str {
    let mut depth = 0_i32;
    let mut in_single = false;
    let mut in_double = false;
    let mut in_backtick = false;
    let mut escape = false;

    for (idx, ch) in pattern.char_indices() {
        if escape {
            escape = false;
            continue;
        }
        match ch {
            '\\' if in_single || in_double || in_backtick => {
                escape = true;
            }
            '\'' if !in_double && !in_backtick => in_single = !in_single,
            '"' if !in_single && !in_backtick => in_double = !in_double,
            '`' if !in_single && !in_double => in_backtick = !in_backtick,
            _ if in_single || in_double || in_backtick => {}
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth -= 1,
            ':' if depth == 0 => return &pattern[..idx],
            _ => {}
        }
    }
    pattern
}

fn trim_outer_parens(source: &str) -> &str {
    source
        .strip_prefix('(')
        .and_then(|inner| inner.strip_suffix(')'))
        .unwrap_or(source)
}

fn valid_identifier(source: &str) -> Option<&str> {
    let mut chars = source.chars();
    let first = chars.next()?;
    if !matches!(first, 'A'..='Z' | 'a'..='z' | '_' | '$') {
        return None;
    }
    chars
        .all(|ch| matches!(ch, 'A'..='Z' | 'a'..='z' | '0'..='9' | '_' | '$'))
        .then_some(source)
}

/// A parsed HTML/SFC tag with its name, attributes, and self-closing status.
#[derive(Debug)]
pub(super) struct ParsedTag {
    pub name: String,
    pub attrs: Vec<ParsedAttr>,
    pub self_closing: bool,
}

/// A single attribute on an HTML/SFC tag.
#[derive(Debug)]
pub(super) struct ParsedAttr {
    pub name: String,
    pub value: Option<String>,
}

/// Parse an HTML tag string into its name, attributes, and self-closing status.
///
/// When `braced_values` is true, attribute values like `={expr}` are handled
/// (Svelte syntax). When false, only quoted and unquoted values are recognized (Vue).
pub(super) fn parse_tag_attrs(tag: &str, braced_values: bool) -> ParsedTag {
    let inner = tag.trim_start_matches('<').trim_end_matches('>').trim();
    let self_closing = inner.ends_with('/');
    let inner = inner.trim_end_matches('/').trim_end();

    let name_end = inner
        .char_indices()
        .find_map(|(idx, ch)| ch.is_whitespace().then_some(idx))
        .unwrap_or(inner.len());
    let name = inner[..name_end].trim().to_string();

    let mut attrs = Vec::new();
    let mut index = name_end;

    while index < inner.len() {
        let remaining = &inner[index..];
        let trimmed = remaining.trim_start();
        index += remaining.len() - trimmed.len();
        if index >= inner.len() {
            break;
        }

        let name_end = inner[index..]
            .char_indices()
            .find_map(|(offset, ch)| (ch.is_whitespace() || ch == '=').then_some(index + offset))
            .unwrap_or(inner.len());
        let attr_name = inner[index..name_end].trim();
        index = name_end;

        let remaining = &inner[index..];
        let trimmed = remaining.trim_start();
        index += remaining.len() - trimmed.len();

        let mut value = None;
        if inner.as_bytes().get(index) == Some(&b'=') {
            index += 1;
            let remaining = &inner[index..];
            let trimmed = remaining.trim_start();
            index += remaining.len() - trimmed.len();
            if let Some(quote) = inner.as_bytes().get(index).copied() {
                if quote == b'\'' || quote == b'"' {
                    let quote = quote as char;
                    index += 1;
                    let value_start = index;
                    while index < inner.len() && inner.as_bytes()[index] as char != quote {
                        index += 1;
                    }
                    value = Some(inner[value_start..index].to_string());
                    if index < inner.len() {
                        index += 1;
                    }
                } else if braced_values && quote == b'{' {
                    let Some((expr, next_index)) = scan_curly_section(inner, index, 1, 1) else {
                        break;
                    };
                    value = Some(format!("{{{expr}}}"));
                    index = next_index;
                } else {
                    let value_end = inner[index..]
                        .char_indices()
                        .find_map(|(offset, ch)| ch.is_whitespace().then_some(index + offset))
                        .unwrap_or(inner.len());
                    value = Some(inner[index..value_end].to_string());
                    index = value_end;
                }
            }
        }

        if !attr_name.is_empty() {
            attrs.push(ParsedAttr {
                name: attr_name.to_string(),
                value,
            });
        }
    }

    ParsedTag {
        name,
        attrs,
        self_closing,
    }
}

#[cfg(test)]
mod tests {
    use rustc_hash::FxHashSet;

    use super::{
        extract_pattern_binding_names, kebab_to_camel_case, merge_component_tag_usage,
        merge_expression_usage, merge_expression_usage_allow_dollar_refs,
        merge_pattern_binding_usage, merge_statement_usage,
        merge_statement_usage_allow_dollar_refs, split_top_level, split_top_level_once,
        strip_trailing_type_annotation, strip_wrapping, trim_outer_parens, uppercase_first,
        valid_identifier,
    };
    use crate::template_usage::TemplateUsage;

    #[test]
    fn extracts_nested_object_pattern_bindings() {
        assert_eq!(
            extract_pattern_binding_names("{ item: { id, label }, count = 0 }"),
            vec!["id", "label", "count"],
        );
    }

    #[test]
    fn extracts_array_pattern_bindings() {
        assert_eq!(
            extract_pattern_binding_names("[first, , { value: second }, ...rest]"),
            vec!["first", "second", "rest"],
        );
    }

    #[test]
    fn extracts_comma_separated_parameters() {
        assert_eq!(
            extract_pattern_binding_names("item, index = 0"),
            vec!["item", "index"],
        );
    }

    #[test]
    fn extract_pattern_empty_string_returns_empty() {
        assert!(extract_pattern_binding_names("").is_empty());
    }

    #[test]
    fn extract_pattern_only_spread_returns_empty() {
        assert!(extract_pattern_binding_names("...").is_empty());
    }

    #[test]
    fn extract_pattern_spread_prefix_extracts_binding() {
        assert_eq!(extract_pattern_binding_names("...rest"), vec!["rest"]);
    }

    #[test]
    fn extract_pattern_with_outer_parens() {
        assert_eq!(
            extract_pattern_binding_names("(item, idx)"),
            vec!["item", "idx"],
        );
    }

    #[test]
    fn extract_pattern_invalid_identifier_returns_empty() {
        assert!(extract_pattern_binding_names("123invalid").is_empty());
    }

    #[test]
    fn extract_pattern_object_with_empty_parts() {
        assert_eq!(extract_pattern_binding_names("{ a, , b }"), vec!["a", "b"],);
    }

    #[test]
    fn extract_pattern_object_with_rest_spread() {
        assert_eq!(extract_pattern_binding_names("{ a, ... }"), vec!["a"],);
    }

    #[test]
    fn extract_pattern_top_level_default_value() {
        assert_eq!(extract_pattern_binding_names("x = 42"), vec!["x"],);
    }

    #[test]
    fn pattern_usage_tracks_default_initializer_references() {
        let mut usage = TemplateUsage::default();
        let imported_bindings = FxHashSet::from_iter(["fallbackItem".to_string()]);

        let locals = merge_pattern_binding_usage(
            &mut usage,
            "{ item = fallbackItem }",
            &imported_bindings,
            &[],
        );

        assert_eq!(locals, vec!["item"]);
        assert!(usage.used_bindings.contains("fallbackItem"));
    }

    #[test]
    fn pattern_usage_empty_pattern_returns_no_bindings() {
        let mut usage = TemplateUsage::default();
        let imported_bindings = FxHashSet::from_iter(["foo".to_string()]);

        let locals = merge_pattern_binding_usage(&mut usage, "", &imported_bindings, &[]);

        assert!(locals.is_empty());
        assert!(usage.used_bindings.is_empty());
    }

    #[test]
    fn pattern_usage_top_level_default_tracks_reference() {
        let mut usage = TemplateUsage::default();
        let imported_bindings = FxHashSet::from_iter(["defaultVal".to_string()]);

        let locals =
            merge_pattern_binding_usage(&mut usage, "x = defaultVal", &imported_bindings, &[]);

        assert_eq!(locals, vec!["x"]);
        assert!(usage.used_bindings.contains("defaultVal"));
    }

    #[test]
    fn pattern_usage_array_with_nested_defaults() {
        let mut usage = TemplateUsage::default();
        let imported_bindings = FxHashSet::from_iter(["fallback".to_string()]);

        let locals =
            merge_pattern_binding_usage(&mut usage, "[a, b = fallback]", &imported_bindings, &[]);

        assert_eq!(locals, vec!["a", "b"]);
        assert!(usage.used_bindings.contains("fallback"));
    }

    #[test]
    fn pattern_usage_typed_destructure_does_not_infinite_recurse() {
        let mut usage = TemplateUsage::default();
        let imported_bindings = FxHashSet::from_iter(["id".to_string(), "name".to_string()]);

        let locals =
            merge_pattern_binding_usage(&mut usage, "{ id, name }: Item", &imported_bindings, &[]);

        assert_eq!(locals.len(), 2);
        assert!(locals.contains(&"id".to_string()));
        assert!(locals.contains(&"name".to_string()));
    }

    #[test]
    fn pattern_usage_typed_array_destructure() {
        let mut usage = TemplateUsage::default();
        let imported_bindings = FxHashSet::from_iter(["a".to_string()]);

        let locals =
            merge_pattern_binding_usage(&mut usage, "[a, b]: number[]", &imported_bindings, &[]);

        assert_eq!(locals.len(), 2);
        assert!(locals.contains(&"a".to_string()));
        assert!(locals.contains(&"b".to_string()));
    }

    #[test]
    fn component_tag_usage_marks_exact_binding_used() {
        let mut usage = TemplateUsage::default();
        let imported_bindings =
            FxHashSet::from_iter(["GreetingCard".to_string(), "AlertBox".to_string()]);

        merge_component_tag_usage(&mut usage, "GreetingCard", &imported_bindings, &[], false);

        assert!(usage.used_bindings.contains("GreetingCard"));
        assert!(!usage.used_bindings.contains("AlertBox"));
    }

    #[test]
    fn component_tag_usage_converts_kebab_case_for_vue() {
        let mut usage = TemplateUsage::default();
        let imported_bindings = FxHashSet::from_iter(["MyButton".to_string()]);

        merge_component_tag_usage(&mut usage, "my-button", &imported_bindings, &[], true);

        assert!(usage.used_bindings.contains("MyButton"));
    }

    #[test]
    fn component_tag_usage_respects_shadowing_locals() {
        let mut usage = TemplateUsage::default();
        let imported_bindings = FxHashSet::from_iter(["Item".to_string()]);

        merge_component_tag_usage(
            &mut usage,
            "Item",
            &imported_bindings,
            &["Item".to_string()],
            false,
        );

        assert!(usage.used_bindings.is_empty());
    }

    #[test]
    fn component_tag_usage_tracks_namespaced_members() {
        let mut usage = TemplateUsage::default();
        let imported_bindings = FxHashSet::from_iter(["icons".to_string()]);

        merge_component_tag_usage(&mut usage, "icons.Alert", &imported_bindings, &[], false);

        assert!(usage.used_bindings.contains("icons"));
        assert_eq!(usage.member_accesses.len(), 1);
        assert_eq!(usage.member_accesses[0].object, "icons");
        assert_eq!(usage.member_accesses[0].member, "Alert");
    }

    #[test]
    fn component_tag_usage_empty_tag_is_noop() {
        let mut usage = TemplateUsage::default();
        let imported_bindings = FxHashSet::from_iter(["Foo".to_string()]);

        merge_component_tag_usage(&mut usage, "", &imported_bindings, &[], false);

        assert!(usage.used_bindings.is_empty());
    }

    #[test]
    fn component_tag_usage_whitespace_only_tag_is_noop() {
        let mut usage = TemplateUsage::default();
        let imported_bindings = FxHashSet::from_iter(["Foo".to_string()]);

        merge_component_tag_usage(&mut usage, "  \t  ", &imported_bindings, &[], false);

        assert!(usage.used_bindings.is_empty());
    }

    #[test]
    fn component_tag_usage_empty_bindings_is_noop() {
        let mut usage = TemplateUsage::default();
        let imported_bindings = FxHashSet::default();

        merge_component_tag_usage(&mut usage, "MyComponent", &imported_bindings, &[], true);

        assert!(usage.used_bindings.is_empty());
    }

    #[test]
    fn component_tag_kebab_without_allow_flag_skips_conversion() {
        let mut usage = TemplateUsage::default();
        let imported_bindings = FxHashSet::from_iter(["MyButton".to_string()]);

        merge_component_tag_usage(&mut usage, "my-button", &imported_bindings, &[], false);

        assert!(!usage.used_bindings.contains("MyButton"));
    }

    #[test]
    fn component_tag_kebab_also_tries_camel_case() {
        let mut usage = TemplateUsage::default();
        let imported_bindings = FxHashSet::from_iter(["myButton".to_string()]);

        merge_component_tag_usage(&mut usage, "my-button", &imported_bindings, &[], true);

        assert!(usage.used_bindings.contains("myButton"));
    }

    #[test]
    fn component_tag_binding_not_imported_is_noop() {
        let mut usage = TemplateUsage::default();
        let imported_bindings = FxHashSet::from_iter(["OtherComponent".to_string()]);

        merge_component_tag_usage(&mut usage, "Missing", &imported_bindings, &[], false);

        assert!(usage.used_bindings.is_empty());
    }

    #[test]
    fn expression_usage_marks_imported_binding() {
        let mut usage = TemplateUsage::default();
        let imported_bindings = FxHashSet::from_iter(["formatDate".to_string()]);

        merge_expression_usage(&mut usage, "formatDate(x)", &imported_bindings, &[]);

        assert!(usage.used_bindings.contains("formatDate"));
    }

    #[test]
    fn statement_usage_marks_imported_binding() {
        let mut usage = TemplateUsage::default();
        let imported_bindings = FxHashSet::from_iter(["doSomething".to_string()]);

        merge_statement_usage(&mut usage, "doSomething();", &imported_bindings, &[]);

        assert!(usage.used_bindings.contains("doSomething"));
    }

    #[test]
    fn expression_usage_dollar_refs_resolves_store_binding() {
        let mut usage = TemplateUsage::default();
        let imported_bindings = FxHashSet::from_iter(["count".to_string()]);

        merge_expression_usage_allow_dollar_refs(&mut usage, "$count + 1", &imported_bindings, &[]);

        assert!(usage.used_bindings.contains("count"));
    }

    #[test]
    fn statement_usage_dollar_refs_resolves_store_binding() {
        let mut usage = TemplateUsage::default();
        let imported_bindings = FxHashSet::from_iter(["store".to_string()]);

        merge_statement_usage_allow_dollar_refs(
            &mut usage,
            "$store.update();",
            &imported_bindings,
            &[],
        );

        assert!(usage.used_bindings.contains("store"));
    }

    #[test]
    fn kebab_to_camel_basic() {
        assert_eq!(kebab_to_camel_case("my-button"), "myButton");
    }

    #[test]
    fn kebab_to_camel_multiple_segments() {
        assert_eq!(
            kebab_to_camel_case("my-long-component-name"),
            "myLongComponentName",
        );
    }

    #[test]
    fn kebab_to_camel_no_dashes() {
        assert_eq!(kebab_to_camel_case("button"), "button");
    }

    #[test]
    fn kebab_to_camel_leading_dash() {
        assert_eq!(kebab_to_camel_case("-button"), "Button");
    }

    #[test]
    fn kebab_to_camel_trailing_dash() {
        assert_eq!(kebab_to_camel_case("button-"), "button");
    }

    #[test]
    fn kebab_to_camel_only_dashes_returns_empty() {
        assert_eq!(kebab_to_camel_case("---"), "");
    }

    #[test]
    fn kebab_to_camel_empty_string() {
        assert_eq!(kebab_to_camel_case(""), "");
    }

    #[test]
    fn uppercase_first_basic() {
        assert_eq!(uppercase_first("hello"), "Hello");
    }

    #[test]
    fn uppercase_first_already_uppercase() {
        assert_eq!(uppercase_first("Hello"), "Hello");
    }

    #[test]
    fn uppercase_first_empty_string() {
        assert_eq!(uppercase_first(""), "");
    }

    #[test]
    fn uppercase_first_single_char() {
        assert_eq!(uppercase_first("a"), "A");
    }

    #[test]
    fn valid_identifier_simple() {
        assert_eq!(valid_identifier("foo"), Some("foo"));
    }

    #[test]
    fn valid_identifier_with_underscore() {
        assert_eq!(valid_identifier("_private"), Some("_private"));
    }

    #[test]
    fn valid_identifier_with_dollar() {
        assert_eq!(valid_identifier("$store"), Some("$store"));
    }

    #[test]
    fn valid_identifier_with_digits() {
        assert_eq!(valid_identifier("item2"), Some("item2"));
    }

    #[test]
    fn valid_identifier_starts_with_digit() {
        assert_eq!(valid_identifier("2item"), None);
    }

    #[test]
    fn valid_identifier_empty() {
        assert_eq!(valid_identifier(""), None);
    }

    #[test]
    fn valid_identifier_contains_dash() {
        assert_eq!(valid_identifier("my-var"), None);
    }

    #[test]
    fn valid_identifier_contains_space() {
        assert_eq!(valid_identifier("my var"), None);
    }

    #[test]
    fn trim_parens_removes_outer() {
        assert_eq!(trim_outer_parens("(foo)"), "foo");
    }

    #[test]
    fn trim_parens_no_parens() {
        assert_eq!(trim_outer_parens("foo"), "foo");
    }

    #[test]
    fn trim_parens_only_opening() {
        assert_eq!(trim_outer_parens("(foo"), "(foo");
    }

    #[test]
    fn trim_parens_only_closing() {
        assert_eq!(trim_outer_parens("foo)"), "foo)");
    }

    #[test]
    fn trim_parens_empty() {
        assert_eq!(trim_outer_parens(""), "");
    }

    #[test]
    fn strip_wrapping_curly() {
        assert_eq!(strip_wrapping("{ a, b }", '{', '}'), Some(" a, b "));
    }

    #[test]
    fn strip_wrapping_square() {
        assert_eq!(strip_wrapping("[x, y]", '[', ']'), Some("x, y"));
    }

    #[test]
    fn strip_wrapping_no_match() {
        assert_eq!(strip_wrapping("a, b", '{', '}'), None);
    }

    #[test]
    fn strip_wrapping_mismatched() {
        assert_eq!(strip_wrapping("{a, b", '{', '}'), None);
    }

    #[test]
    fn strip_type_from_object_destructure() {
        assert_eq!(
            strip_trailing_type_annotation("{ href, content }: Props"),
            "{ href, content }"
        );
    }

    #[test]
    fn strip_type_from_array_destructure() {
        assert_eq!(strip_trailing_type_annotation("[a, b]: number[]"), "[a, b]");
    }

    #[test]
    fn strip_type_preserves_plain_destructure() {
        assert_eq!(
            strip_trailing_type_annotation("{ href, content }"),
            "{ href, content }"
        );
    }

    #[test]
    fn strip_type_preserves_identifier() {
        assert_eq!(strip_trailing_type_annotation("item"), "item");
    }

    #[test]
    fn strip_type_nested_braces() {
        assert_eq!(
            strip_trailing_type_annotation("{ a: { b, c } }: Type"),
            "{ a: { b, c } }"
        );
    }

    #[test]
    fn strip_type_from_simple_identifier() {
        assert_eq!(strip_trailing_type_annotation("x: number"), "x");
    }

    #[test]
    fn strip_type_from_identifier_with_tuple_type() {
        assert_eq!(strip_trailing_type_annotation("x: [number, number]"), "x");
    }

    #[test]
    fn extract_pattern_typed_tuple_param() {
        assert_eq!(
            extract_pattern_binding_names("x: [number, number]"),
            vec!["x"]
        );
    }

    #[test]
    fn extract_pattern_multiple_typed_params() {
        assert_eq!(
            extract_pattern_binding_names("a: number, b: string"),
            vec!["a", "b"]
        );
    }

    #[test]
    fn split_top_level_simple() {
        assert_eq!(split_top_level("a, b, c", ','), vec!["a", " b", " c"]);
    }

    #[test]
    fn split_top_level_respects_nested_braces() {
        assert_eq!(split_top_level("{ a, b }, c", ','), vec!["{ a, b }", " c"],);
    }

    #[test]
    fn split_top_level_respects_nested_brackets() {
        assert_eq!(split_top_level("[a, b], c", ','), vec!["[a, b]", " c"],);
    }

    #[test]
    fn split_top_level_respects_nested_parens() {
        assert_eq!(split_top_level("fn(a, b), c", ','), vec!["fn(a, b)", " c"],);
    }

    #[test]
    fn split_top_level_respects_single_quotes() {
        assert_eq!(split_top_level("'a,b', c", ','), vec!["'a,b'", " c"],);
    }

    #[test]
    fn split_top_level_respects_double_quotes() {
        assert_eq!(split_top_level(r#""a,b", c"#, ','), vec![r#""a,b""#, " c"],);
    }

    #[test]
    fn split_top_level_respects_backticks() {
        assert_eq!(split_top_level("`a,b`, c", ','), vec!["`a,b`", " c"],);
    }

    #[test]
    fn split_top_level_respects_escape_in_string() {
        assert_eq!(split_top_level(r"'a\',b', c", ','), vec![r"'a\',b'", " c"],);
    }

    #[test]
    fn split_top_level_no_delimiter() {
        assert_eq!(split_top_level("abc", ','), vec!["abc"]);
    }

    #[test]
    fn split_top_level_once_simple() {
        assert_eq!(
            split_top_level_once("key: value", ':'),
            Some(("key", " value")),
        );
    }

    #[test]
    fn split_top_level_once_nested() {
        assert_eq!(
            split_top_level_once("{ a: b }: c", ':'),
            Some(("{ a: b }", " c")),
        );
    }

    #[test]
    fn split_top_level_once_no_delimiter() {
        assert_eq!(split_top_level_once("abc", ':'), None);
    }

    #[test]
    fn split_top_level_once_delimiter_in_single_quotes() {
        assert_eq!(split_top_level_once("'a:b': c", ':'), Some(("'a:b'", " c")),);
    }

    #[test]
    fn split_top_level_once_delimiter_in_double_quotes() {
        assert_eq!(
            split_top_level_once(r#""a:b": c"#, ':'),
            Some((r#""a:b""#, " c")),
        );
    }

    #[test]
    fn split_top_level_once_delimiter_in_backticks() {
        assert_eq!(split_top_level_once("`a:b`: c", ':'), Some(("`a:b`", " c")),);
    }

    #[test]
    fn split_top_level_once_escape_in_string() {
        assert_eq!(
            split_top_level_once(r"'a\':b': c", ':'),
            Some((r"'a\':b'", " c")),
        );
    }

    #[test]
    fn component_tag_kebab_all_dashes_does_not_mark_empty() {
        let mut usage = TemplateUsage::default();
        let imported_bindings = FxHashSet::from_iter([String::new()]);

        merge_component_tag_usage(&mut usage, "---", &imported_bindings, &[], true);

        assert!(usage.used_bindings.is_empty());
    }
}
