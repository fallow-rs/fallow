//! Caller facts from the shared quote-aware markup scanner.

use super::scanners::scan_html_tag;
use super::shared::{ParsedAttr, kebab_to_camel_case, parse_tag_attrs};
use fallow_types::extract::{
    ComponentContractFacts, ComponentFramework, ComponentInvocation, ComponentReference, ImportInfo,
};

pub fn collect(
    source: &str,
    imports: &[ImportInfo],
    framework: ComponentFramework,
    offset: u32,
    bindings: &[fallow_types::extract::ComponentSpreadBinding],
    aliases: &[fallow_types::extract::ComponentAliasBinding],
) -> ComponentContractFacts {
    let mut facts = ComponentContractFacts::default();
    let mut cursor = 0;
    while cursor < source.len() {
        let Some(relative) = source[cursor..].find('<') else {
            break;
        };
        let start = cursor + relative;
        if source[start..].starts_with("<!--") {
            cursor = source[start + 4..]
                .find("-->")
                .map_or(source.len(), |end| start + 4 + end + 3);
            continue;
        }
        let Some((tag, end)) = scan_html_tag(source, start) else {
            facts.incomplete_frameworks.push(framework);
            break;
        };
        cursor = end;
        if tag.starts_with("</") || tag.starts_with("<!") {
            continue;
        }
        let parsed = parse_tag_attrs(tag, true);
        let name = parsed.name.as_str();
        if matches!(name, "script" | "style") {
            let closing = format!("</{name}");
            cursor = source[cursor..]
                .find(&closing)
                .map_or(source.len(), |end| cursor + end + closing.len());
            continue;
        }
        collect_attribute_escapes(&parsed.attrs, imports, aliases, framework, &mut facts);
        let dynamic = matches!(name, "component" | "svelte:component" | "svelte:element")
            || parsed.attrs.iter().any(|attr| {
                matches!(
                    attr.name.as_str(),
                    "[ngComponentOutlet]" | "*ngComponentOutlet" | "is" | ":is" | "v-bind:is"
                )
            });
        if dynamic {
            facts.incomplete_frameworks.push(framework);
        }
        let imported = imports.iter().find(|import| {
            !import.local_name.is_empty()
                && (import.local_name == name
                    || (framework == ComponentFramework::Vue
                        && kebab_to_camel_case(name).eq_ignore_ascii_case(&import.local_name)))
        });
        let Some(target) = target_for_tag(name, imported, framework, start as u32 + offset) else {
            continue;
        };
        let mut supplied = Vec::new();
        let mut supplied_properties = Vec::new();
        let mut unknown_props = dynamic;
        for attr in &parsed.attrs {
            if framework == ComponentFramework::Lit && attr.name.starts_with('.') {
                supplied_properties.push(attr.name[1..].to_string());
                continue;
            }
            unknown_props |= attribute_supply(attr, framework, &mut supplied, bindings, source);
        }
        // Template scope and dynamic aliases require the framework's compiler.
        // Retaining caller uncertainty prevents a shadowed tag from proving absence.
        if imported.is_some()
            && template_binding_may_shadow(source, imported.map_or("", |import| &import.local_name))
        {
            unknown_props = true;
        }
        if framework == ComponentFramework::Svelte && !parsed.self_closing {
            let closing = format!("</{}>", parsed.name);
            if let Some(end) = source[cursor..].find(&closing) {
                if !source[cursor..cursor + end].trim().is_empty() {
                    supplied.push("children".to_string());
                }
            } else {
                unknown_props = true;
            }
        }
        supplied.sort_unstable();
        supplied.dedup();
        facts.invocations.push(ComponentInvocation {
            target,
            span_start: start as u32 + offset,
            supplied,
            supplied_properties,
            unknown_props,
            framework,
        });
    }
    facts
}

fn collect_attribute_escapes(
    attrs: &[ParsedAttr],
    imports: &[ImportInfo],
    aliases: &[fallow_types::extract::ComponentAliasBinding],
    framework: ComponentFramework,
    facts: &mut ComponentContractFacts,
) {
    for attr in attrs {
        let value = attr.value.as_deref().unwrap_or(&attr.name);
        let expression = value.contains('{')
            || (framework == ComponentFramework::Angular && attr.name.starts_with(['[', '(', '*']))
            || (framework == ComponentFramework::Vue
                && (attr.name.starts_with([':', '@']) || attr.name.starts_with("v-")));
        if !expression {
            continue;
        }
        let words: Vec<_> = value
            .split(|character: char| {
                !character.is_ascii_alphanumeric() && character != '_' && character != '$'
            })
            .collect();
        for alias in aliases {
            if words.contains(&alias.local.as_str()) {
                facts.escapes.push(alias.target.clone());
            }
        }
        for import in imports {
            if !import.local_name.is_empty() && words.contains(&import.local_name.as_str()) {
                facts.escapes.push(ComponentReference::Import {
                    local: import.local_name.clone(),
                    span_start: import.span.start,
                });
            }
        }
    }
}

fn template_binding_may_shadow(source: &str, local: &str) -> bool {
    if local.is_empty() {
        return false;
    }
    source.contains("=>")
        || source.contains("function")
        || source.contains(" as ")
        || source.contains('|')
        || source.contains("{#snippet")
        || source.contains("v-for=")
        || source.contains("v-slot")
        || source.contains("#each")
        || source.contains("#await")
        || source.contains("let:")
}

fn literal_keys(source: &str) -> Option<Vec<String>> {
    use oxc_ast::ast::{Expression, Statement};
    let source = format!("({source});");
    let allocator = oxc_allocator::Allocator::default();
    let parsed = oxc_parser::Parser::new(&allocator, &source, oxc_span::SourceType::ts()).parse();
    if parsed.fatal_error || !parsed.diagnostics.is_empty() {
        return None;
    }
    let Statement::ExpressionStatement(statement) = parsed.program.body.first()? else {
        return None;
    };
    let Expression::ObjectExpression(object) = statement.expression.without_parentheses() else {
        return None;
    };
    literal_object_keys(object)
}

fn literal_object_keys(object: &oxc_ast::ast::ObjectExpression<'_>) -> Option<Vec<String>> {
    use oxc_ast::ast::{Expression, ObjectPropertyKind, PropertyKey, PropertyKind};
    let mut keys = Vec::new();
    for property in &object.properties {
        match property {
            ObjectPropertyKind::SpreadProperty(spread) => {
                let Expression::ObjectExpression(object) = spread.argument.without_parentheses()
                else {
                    return None;
                };
                keys.extend(literal_object_keys(object)?);
            }
            ObjectPropertyKind::ObjectProperty(property) => {
                if property.computed || property.method || property.kind != PropertyKind::Init {
                    return None;
                }
                match &property.key {
                    PropertyKey::StaticIdentifier(name) => keys.push(name.name.to_string()),
                    PropertyKey::StringLiteral(name) => keys.push(name.value.to_string()),
                    _ => return None,
                }
            }
        }
    }
    Some(keys)
}

fn spread_keys(
    expression: &str,
    bindings: &[fallow_types::extract::ComponentSpreadBinding],
    source: &str,
) -> Option<Vec<String>> {
    if let Some(keys) = literal_keys(expression) {
        return Some(keys);
    }
    let local = expression.trim();
    let binding = bindings.iter().find(|binding| binding.local == local)?;
    if source.contains("v-for=")
        || source.contains("v-slot")
        || source.contains("#each")
        || source.contains("#await")
        || source.contains("{#snippet")
    {
        return None;
    }
    // Template expressions can mutate an otherwise unescaped script object.
    // Only bare spread uses are inspected; any other textual reference abstains.
    for (start, _) in source.match_indices(local) {
        let prefix = &source[..start];
        let suffix = &source[start + local.len()..];
        let bare_vue = prefix.ends_with("v-bind=\"") && suffix.starts_with('"')
            || prefix.ends_with("v-bind='") && suffix.starts_with('\'');
        let bare_brace = prefix.ends_with("{...") && suffix.starts_with('}');
        if !bare_vue && !bare_brace {
            return None;
        }
    }
    Some(binding.keys.clone())
}

fn attribute_supply(
    attr: &ParsedAttr,
    framework: ComponentFramework,
    supplied: &mut Vec<String>,
    bindings: &[fallow_types::extract::ComponentSpreadBinding],
    source: &str,
) -> bool {
    let mut unknown_props = false;

    let name = attr.name.as_str();
    if name.starts_with('{') {
        let inner = name
            .strip_prefix('{')
            .and_then(|name| name.strip_suffix('}'))
            .unwrap_or(name);
        if let Some(spread) = inner.strip_prefix("...") {
            if let Some(keys) = spread_keys(spread, bindings, source) {
                supplied.extend(keys);
            } else {
                unknown_props = true;
            }
        } else if !inner.is_empty()
            && inner
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'$'))
        {
            supplied.push(inner.to_string());
        } else {
            unknown_props = true;
        }
        return unknown_props;
    }
    if name.contains('[') && framework != ComponentFramework::Angular {
        unknown_props = true;
        return unknown_props;
    }
    if framework == ComponentFramework::Vue && name == "v-bind" {
        if let Some(keys) = attr
            .value
            .as_deref()
            .and_then(|value| spread_keys(value, bindings, source))
        {
            supplied.extend(keys);
            return false;
        }
        return true;
    }
    named_attribute_supply(attr, framework, supplied)
}

fn named_attribute_supply(
    attr: &ParsedAttr,
    framework: ComponentFramework,
    supplied: &mut Vec<String>,
) -> bool {
    let name = attr.name.as_str();
    let mut unknown_props = false;
    match framework {
        ComponentFramework::Vue => {
            if name == "v-model" || name.starts_with("v-model.") {
                supplied.push("modelValue".to_string());
            } else if let Some(name) = name.strip_prefix("v-model:") {
                supplied.push(kebab_to_camel_case(name.split('.').next().unwrap_or(name)));
            } else if let Some(name) = name
                .strip_prefix(':')
                .or_else(|| name.strip_prefix("v-bind:"))
            {
                supplied.push(kebab_to_camel_case(name.split('.').next().unwrap_or(name)));
            } else if !name.starts_with(['@', '#']) && !name.starts_with("v-") {
                supplied.push(kebab_to_camel_case(name));
            }
        }
        ComponentFramework::Svelte => {
            if let Some(name) = name.strip_prefix("bind:") {
                supplied.push(name.to_string());
            } else if !name.contains(':') {
                supplied.push(name.to_string());
            }
        }
        ComponentFramework::Astro => {
            if !name.contains(':') {
                supplied.push(name.to_string());
            }
        }
        ComponentFramework::Angular => {
            if name.starts_with("[(") && name.ends_with(")]") {
                supplied.push(name[2..name.len() - 2].to_string());
            } else if name.starts_with('[') && name.ends_with(']') {
                supplied.push(name[1..name.len() - 1].to_string());
            } else if !name.starts_with(['(', '*', '#']) {
                supplied.push(name.to_string());
            }
        }
        ComponentFramework::Lit => {
            supplied.push(name.trim_start_matches('?').to_ascii_lowercase());
        }
        ComponentFramework::Ember => {
            if let Some(name) = name.strip_prefix('@') {
                supplied.push(name.to_string());
            } else if name == "...attributes" {
                unknown_props = true;
            }
        }
        _ => {
            supplied.push(name.to_string());
        }
    }
    unknown_props
}

fn target_for_tag(
    name: &str,
    imported: Option<&ImportInfo>,
    framework: ComponentFramework,
    anchor: u32,
) -> Option<ComponentReference> {
    Some(if let Some(import) = imported {
        ComponentReference::Import {
            local: import.local_name.clone(),
            span_start: import.span.start,
        }
    } else if framework == ComponentFramework::Angular
        || (framework == ComponentFramework::Lit && name.contains('-'))
    {
        ComponentReference::Selector(if framework == ComponentFramework::Lit {
            name.to_ascii_lowercase()
        } else {
            name.to_string()
        })
    } else if framework == ComponentFramework::Vue
        && (name.contains('-') || name.as_bytes().first().is_some_and(u8::is_ascii_uppercase))
    {
        // Nuxt's synthetic import lane joins this name in the graph.
        ComponentReference::Import {
            local: name.to_string(),
            span_start: anchor,
        }
    } else if framework == ComponentFramework::Ember
        && name.as_bytes().first().is_some_and(u8::is_ascii_uppercase)
    {
        ComponentReference::Import {
            local: name.to_string(),
            span_start: anchor,
        }
    } else if name.contains('-') {
        ComponentReference::Selector(name.to_ascii_lowercase())
    } else {
        return None;
    })
}
