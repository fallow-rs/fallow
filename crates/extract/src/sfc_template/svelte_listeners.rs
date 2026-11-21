//! Svelte custom-event listener names in markup, for the
//! `unused-svelte-event` detector. The event name is an attribute name, not
//! an expression, so the markup snippet visitor cannot see it, and this tag
//! scan reads it.

use super::scanners::scan_html_tag;
use super::shared::parse_tag_attrs;
use super::svelte::strip_non_template_content;

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

#[cfg(test)]
mod tests {
    use super::collect_listened_events;

    #[test]
    fn listened_events_credits_component_tag_on_directive() {
        let listened = collect_listened_events("<Child on:save on:close />");
        assert!(listened.contains(&"save".to_string()));
        assert!(listened.contains(&"close".to_string()));
    }

    #[test]
    fn listened_events_excludes_dom_element_on_directive() {
        // `on:click` on a lowercase DOM `<button>` is a DOM event, not a custom
        // event, so it must not be credited.
        let listened = collect_listened_events("<button on:click>Hi</button>");
        assert!(
            listened.is_empty(),
            "DOM on:click must be excluded: {listened:?}"
        );
    }

    #[test]
    fn listened_events_credits_event_forwarding_without_value() {
        // Event forwarding (`on:save` with no value) on a component still counts.
        let listened = collect_listened_events("<Child on:save />");
        assert!(listened.contains(&"save".to_string()));
    }

    #[test]
    fn listened_events_strips_event_modifiers() {
        let listened = collect_listened_events("<Child on:save|once={handler} />");
        assert!(listened.contains(&"save".to_string()));
        assert!(!listened.iter().any(|name| name.contains('|')));
    }

    #[test]
    fn listened_events_credits_namespaced_component_tag() {
        let listened = collect_listened_events("<Icons.Alert on:dismiss />");
        assert!(listened.contains(&"dismiss".to_string()));
    }

    #[test]
    fn listened_events_ignores_script_block_content() {
        let listened =
            collect_listened_events("<script>const x = 'on:save';</script><Child on:close />");
        assert!(listened.contains(&"close".to_string()));
        assert!(!listened.contains(&"save".to_string()));
    }
}
