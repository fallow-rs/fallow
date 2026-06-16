//! Synthetic `<template>` complexity for Vue single-file components.
//!
//! Scores Vue template control flow (`v-if` / `v-else-if` / `v-for` / `v-show`,
//! including `<template v-for>`) plus bound-directive expressions and `{{ }}`
//! interpolations, reusing the framework-agnostic JS-expression engine. The
//! SFC `<script>` / `<style>` blocks and `<!-- -->` comments are masked out
//! (replaced with equal-length spaces so byte offsets stay accurate) before
//! scanning, so script control flow is NOT double-counted here (it is scored
//! separately by `translate_script_complexity`). Nesting depth tracks the HTML
//! tag stack: a control-flow directive on a child element scores deeper than
//! one on its ancestor, matching Angular's per-block nesting model.

use std::sync::LazyLock;

use fallow_types::extract::FunctionComplexity;

use super::build_template_complexity;
use super::engine::{ScanError, TemplateComplexity, skip_quoted, skip_whitespace};

/// HTML elements that never have a closing tag, so they must not push onto the
/// tag stack even when written without a self-closing slash.
const VOID_HTML_TAGS: &[&str] = &[
    "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param", "source",
    "track", "wbr",
];

static MASK_RE: LazyLock<regex::Regex> = LazyLock::new(|| {
    crate::static_regex(
        r#"(?is)<script\b(?:[^>"']|"[^"]*"|'[^']*')*>[\s\S]*?</script\s*>|<style\b(?:[^>"']|"[^"]*"|'[^']*')*>[\s\S]*?</style\s*>|<!--[\s\S]*?-->"#,
    )
});

/// Compute synthetic `<template>` complexity for a Vue SFC. Returns `None` for a
/// trivial template (no control flow, no non-trivial expression) or any
/// malformed-markup short-circuit.
#[must_use]
pub fn compute_vue_template_complexity(source: &str) -> Option<FunctionComplexity> {
    let markup = mask_non_template(source);
    let complexity = VueScanner::new(&markup).scan().ok()?;
    build_template_complexity(source, &complexity)
}

/// Replace `<script>` / `<style>` blocks and HTML comments with equal-length
/// runs of spaces so the remaining template byte offsets are unchanged. Mirrors
/// the masking convention in `crate::sfc_template::svelte`.
fn mask_non_template(source: &str) -> String {
    super::mask_ranges(source, &MASK_RE)
}

struct VueScanner<'a> {
    source: &'a str,
    complexity: TemplateComplexity,
    nesting: u16,
}

impl<'a> VueScanner<'a> {
    fn new(source: &'a str) -> Self {
        Self {
            source,
            complexity: TemplateComplexity::default(),
            nesting: 0,
        }
    }

    fn scan(mut self) -> Result<TemplateComplexity, ScanError> {
        let mut offset = 0;
        while offset < self.source.len() {
            if self.source[offset..].starts_with("{{") {
                let end = self.find_required(offset + 2, "}}")?;
                self.complexity.add_expression(
                    &self.source[offset + 2..end],
                    offset + 2,
                    self.nesting,
                )?;
                offset = end + 2;
                continue;
            }
            match self.source.as_bytes()[offset] {
                b'<' => offset = self.scan_element(offset)?,
                _ => {
                    offset += self.source[offset..]
                        .chars()
                        .next()
                        .map_or(1, char::len_utf8);
                }
            }
        }
        Ok(self.complexity)
    }

    fn find_required(&self, offset: usize, needle: &str) -> Result<usize, ScanError> {
        self.source[offset..]
            .find(needle)
            .map(|relative| offset + relative)
            .ok_or(ScanError)
    }

    fn scan_element(&mut self, offset: usize) -> Result<usize, ScanError> {
        let tag_end = find_tag_end(self.source, offset)?;
        let after = tag_end + 1;
        if self.source[offset..].starts_with("</") {
            // Closing tag: pop one level of nesting if any is open.
            self.nesting = self.nesting.saturating_sub(1);
            return Ok(after);
        }
        if self.source[offset..].starts_with("<!") || self.source[offset..].starts_with("<?") {
            return Ok(after);
        }

        let self_closing = self.source[..tag_end].trim_end().ends_with('/');
        let tag_name = read_tag_name(self.source, offset);
        self.scan_attributes(offset, tag_end)?;

        if !self_closing && !is_void_tag(tag_name) {
            self.nesting = self.nesting.saturating_add(1);
        }
        Ok(after)
    }

    fn scan_attributes(&mut self, tag_start: usize, tag_end: usize) -> Result<(), ScanError> {
        let mut offset = tag_start + 1;
        // Skip the tag name.
        while offset < tag_end {
            let byte = self.source.as_bytes()[offset];
            if byte.is_ascii_whitespace() || matches!(byte, b'/' | b'>') {
                break;
            }
            offset += 1;
        }

        while offset < tag_end {
            offset = skip_whitespace(self.source, offset);
            if offset >= tag_end || matches!(self.source.as_bytes()[offset], b'/' | b'>') {
                break;
            }

            let name_start = offset;
            while offset < tag_end {
                let byte = self.source.as_bytes()[offset];
                if byte.is_ascii_whitespace() || matches!(byte, b'=' | b'/' | b'>') {
                    break;
                }
                offset += 1;
            }
            let name = &self.source[name_start..offset];
            offset = skip_whitespace(self.source, offset);
            if offset >= tag_end || self.source.as_bytes()[offset] != b'=' {
                // Valueless attribute (`disabled`, bare `v-else`).
                self.scan_valueless_attr(name);
                continue;
            }
            offset = skip_whitespace(self.source, offset + 1);
            let (value_start, value_end, next_offset) = read_attribute_value(self.source, offset)?;
            self.scan_attribute_value(name, value_start, value_end)?;
            offset = next_offset;
        }
        Ok(())
    }

    /// A directive written without a value: only bare `v-else` matters (a
    /// control-flow continuation). Mirrors Angular's bare `@else`: cognitive
    /// +1, no cyclomatic increment (the new branch path is owned by the paired
    /// `v-if`).
    fn scan_valueless_attr(&mut self, name: &str) {
        if name == "v-else" {
            self.complexity.cognitive = self.complexity.cognitive.saturating_add(1);
        }
    }

    fn scan_attribute_value(
        &mut self,
        name: &str,
        value_start: usize,
        value_end: usize,
    ) -> Result<(), ScanError> {
        let value = &self.source[value_start..value_end];
        if is_control_flow_directive(name) {
            self.complexity.add_control_flow(self.nesting);
            self.complexity
                .add_expression(value, value_start, self.nesting)?;
        } else if is_bound_directive(name) {
            self.complexity
                .add_expression(value, value_start, self.nesting)?;
        }
        Ok(())
    }
}

/// `v-if` / `v-else-if` / `v-for` / `v-show` each introduce a branch / loop.
fn is_control_flow_directive(name: &str) -> bool {
    matches!(name, "v-if" | "v-else-if" | "v-for" | "v-show")
}

/// Any directive whose value is a bound JS expression worth scoring for
/// expression complexity (logical operators, ternaries, optional chaining).
/// Includes `:`-shorthand and `v-bind:` bindings, `@`/`v-on:` handlers, and the
/// remaining built-in expression directives.
fn is_bound_directive(name: &str) -> bool {
    name.starts_with(':')
        || name.starts_with('@')
        || name.starts_with("v-bind")
        || name.starts_with("v-on")
        || name.starts_with("v-model")
        || matches!(name, "v-html" | "v-text" | "v-memo")
}

fn read_tag_name(source: &str, tag_start: usize) -> &str {
    let bytes = source.as_bytes();
    let mut end = tag_start + 1;
    while end < source.len() {
        let byte = bytes[end];
        if byte.is_ascii_whitespace() || matches!(byte, b'/' | b'>') {
            break;
        }
        end += 1;
    }
    &source[tag_start + 1..end]
}

fn is_void_tag(tag_name: &str) -> bool {
    VOID_HTML_TAGS
        .iter()
        .any(|void| void.eq_ignore_ascii_case(tag_name))
}

fn find_tag_end(source: &str, tag_start: usize) -> Result<usize, ScanError> {
    let mut offset = tag_start + 1;
    while offset < source.len() {
        match source.as_bytes()[offset] {
            b'\'' | b'"' => offset = skip_quoted(source, offset)?,
            b'>' => return Ok(offset),
            _ => offset += source[offset..].chars().next().map_or(1, char::len_utf8),
        }
    }
    Err(ScanError)
}

fn read_attribute_value(source: &str, offset: usize) -> Result<(usize, usize, usize), ScanError> {
    if offset >= source.len() {
        return Err(ScanError);
    }
    let byte = source.as_bytes()[offset];
    if matches!(byte, b'\'' | b'"') {
        let after = skip_quoted(source, offset)?;
        Ok((offset + 1, after - 1, after))
    } else {
        let mut end = offset;
        while end < source.len() {
            let byte = source.as_bytes()[end];
            if byte.is_ascii_whitespace() || matches!(byte, b'/' | b'>') {
                break;
            }
            end += 1;
        }
        Ok((offset, end, end))
    }
}

#[cfg(test)]
mod tests {
    use super::compute_vue_template_complexity;

    #[test]
    fn nested_v_for_in_v_if_with_ternary_binding_counts() {
        let complexity = compute_vue_template_complexity(
            r#"
<template>
  <div v-if="user?.enabled && featureFlags.dashboard">
    <li v-for="item in items" :key="item.id">
      <badge :color="item.level > 3 ? 'red' : 'green'" />
    </li>
  </div>
</template>
"#,
        )
        .expect("template should have complexity");
        assert!(complexity.cyclomatic >= 4, "{complexity:?}");
        assert!(complexity.cognitive >= 3, "{complexity:?}");
        assert_eq!(complexity.name, "<template>");
    }

    #[test]
    fn template_v_for_counts_as_control_flow() {
        let complexity = compute_vue_template_complexity(
            r#"<template><template v-for="row in rows"><p>{{ row.name }}</p></template></template>"#,
        )
        .expect("template should have complexity");
        assert!(complexity.cyclomatic >= 2, "{complexity:?}");
    }

    #[test]
    fn v_else_is_continuation_not_a_new_branch() {
        // The paired `v-if` owns the cyclomatic increment; bare `v-else` only
        // adds cognitive weight, exactly like Angular's bare `@else`.
        let complexity = compute_vue_template_complexity(
            r#"<template><p v-if="a">x</p><p v-else>y</p></template>"#,
        )
        .expect("template should have complexity");
        assert_eq!(complexity.cyclomatic, 2, "{complexity:?}");
        assert!(complexity.cognitive >= 2, "{complexity:?}");
    }

    #[test]
    fn else_if_cascade_increments_per_branch() {
        let complexity = compute_vue_template_complexity(
            r#"<template><p v-if="a">1</p><p v-else-if="b">2</p><p v-else-if="c">3</p><p v-else>4</p></template>"#,
        )
        .expect("template should have complexity");
        // v-if + two v-else-if = 3 branches on top of the baseline 1.
        assert_eq!(complexity.cyclomatic, 4, "{complexity:?}");
    }

    #[test]
    fn interpolation_expressions_contribute() {
        let complexity = compute_vue_template_complexity(
            r"<template><p>{{ enabled && draft ? 'Draft' : 'New' }}</p></template>",
        )
        .expect("template should have complexity");
        assert!(complexity.cyclomatic >= 3, "{complexity:?}");
    }

    #[test]
    fn bound_attribute_expressions_contribute() {
        let complexity = compute_vue_template_complexity(
            r#"<template><button :disabled="loading || !form.valid" @click="submit() && refresh()" /></template>"#,
        )
        .expect("template should have complexity");
        assert!(complexity.cyclomatic >= 3, "{complexity:?}");
    }

    #[test]
    fn markup_only_template_has_no_synthetic_complexity() {
        assert!(
            compute_vue_template_complexity(
                r#"<template><div class="x"><p>Hello world</p></div></template>"#
            )
            .is_none()
        );
    }

    #[test]
    fn script_control_flow_is_not_counted() {
        // The `<script>` has an if/for, but the template is trivial: no entry.
        assert!(
            compute_vue_template_complexity(
                r"<script setup>
const x = items.filter((i) => i && i.active);
if (a && b) { go(); }
for (const i of items) { use(i); }
</script>
<template><p>Static</p></template>"
            )
            .is_none()
        );
    }

    #[test]
    fn malformed_template_does_not_panic_and_yields_no_entry() {
        // Unterminated interpolation short-circuits via ScanError.
        assert!(compute_vue_template_complexity(r"<template><p>{{ a && </template>").is_none());
        // Unterminated tag.
        assert!(compute_vue_template_complexity(r#"<template><p v-if="a"#).is_none());
        // Logical with no RHS inside an interpolation.
        assert!(compute_vue_template_complexity(r"<template>{{ a && }}</template>").is_none());
    }

    #[test]
    fn multibyte_text_does_not_panic() {
        let complexity = compute_vue_template_complexity(
            "<template><p v-if=\"a && b\">\u{4f4f}\u{6240}{{ c?.d }}</p></template>",
        )
        .expect("template should have complexity");
        assert!(complexity.cyclomatic >= 2, "{complexity:?}");
    }

    #[test]
    fn comments_are_masked() {
        assert!(
            compute_vue_template_complexity(
                r#"<template><!-- v-if="a && b && c" --><p>plain</p></template>"#
            )
            .is_none()
        );
    }
}
