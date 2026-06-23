//! Synthetic `<template>` complexity for Vue single-file components.
//!
//! Scores Vue template control flow (`v-if` / `v-else-if` / `v-for` / `v-show`,
//! including `<template v-for>`) plus bound-directive expressions and `{{ }}`
//! interpolations, reusing the framework-agnostic JS-expression engine. The
//! SFC `<script>` / `<style>` blocks and `<!-- -->` comments are masked out
//! (replaced with equal-length spaces so byte offsets stay accurate) before
//! scanning, so script control flow is NOT double-counted here (it is scored
//! separately by `translate_script_complexity`). Cognitive nesting tracks
//! CONTROL-FLOW depth, not raw markup depth: only an element bearing a
//! control-flow directive (`v-if` / `v-else-if` / `v-else` / `v-for` / `v-show`)
//! opens a nesting level for its subtree, so a `v-if` buried under plain
//! `<div>` wrappers is not over-weighted. This matches the Svelte and Angular
//! scanners and the cognitive-complexity standard (only nesting control
//! structures increase the nesting penalty).

use std::sync::LazyLock;

use fallow_types::extract::FunctionComplexity;

use super::build_template_complexity;
use super::engine::{
    ScanError, TemplateComplexity, find_tag_end, read_attribute_value, skip_whitespace,
};

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
    /// One entry per open (non-void, non-self-closing) element, recording
    /// whether that element carried a control-flow directive and therefore
    /// opened a cognitive nesting level for its subtree. Popped on the matching
    /// close so only control-flow elements decrement `nesting`. This makes
    /// cognitive nesting track CONTROL-FLOW depth, NOT raw markup depth, so a
    /// `v-if` buried under plain `<div>` wrappers is not over-weighted, matching
    /// the Svelte and Angular scanners and the cognitive-complexity standard.
    tag_stack: Vec<bool>,
}

impl<'a> VueScanner<'a> {
    fn new(source: &'a str) -> Self {
        Self {
            source,
            complexity: TemplateComplexity::default(),
            nesting: 0,
            tag_stack: Vec::new(),
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
            // Closing tag: pop the matching open element. Decrement nesting only
            // when that element was control-flow-bearing, so plain markup never
            // shifts cognitive depth. A stray close (empty stack) is a no-op.
            if self.tag_stack.pop() == Some(true) {
                self.nesting = self.nesting.saturating_sub(1);
            }
            return Ok(after);
        }
        if self.source[offset..].starts_with("<!") || self.source[offset..].starts_with("<?") {
            return Ok(after);
        }

        let self_closing = self.source[..tag_end].trim_end().ends_with('/');
        let tag_name = read_tag_name(self.source, offset);
        // Score the element's directives at the CURRENT nesting (before its own
        // subtree deepens), and learn whether it carries control flow.
        let has_control_flow = self.scan_attributes(offset, tag_end)?;

        if !self_closing && !is_void_tag(tag_name) {
            // Every open element is pushed so the stack stays paired with close
            // tags, but only a control-flow-bearing one opens a nesting level.
            self.tag_stack.push(has_control_flow);
            if has_control_flow {
                self.nesting = self.nesting.saturating_add(1);
            }
        }
        Ok(after)
    }

    /// Scan an element's attributes, scoring directive expressions and control
    /// flow. Returns whether the element carried a control-flow directive
    /// (`v-if` / `v-else-if` / `v-else` / `v-for` / `v-show`), so the caller can
    /// open a cognitive nesting level for its subtree.
    fn scan_attributes(&mut self, tag_start: usize, tag_end: usize) -> Result<bool, ScanError> {
        let mut offset = tag_start + 1;
        let mut has_control_flow = false;
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
                has_control_flow |= self.scan_valueless_attr(name);
                continue;
            }
            offset = skip_whitespace(self.source, offset + 1);
            let (value_start, value_end, next_offset) = read_attribute_value(self.source, offset)?;
            has_control_flow |= self.scan_attribute_value(name, value_start, value_end)?;
            offset = next_offset;
        }
        Ok(has_control_flow)
    }

    /// A directive written without a value: only bare `v-else` matters (a
    /// control-flow continuation). Mirrors Angular's bare `@else`: cognitive
    /// +1, no cyclomatic increment (the new branch path is owned by the paired
    /// `v-if`). Returns `true` for `v-else` so its element opens a nesting level.
    fn scan_valueless_attr(&mut self, name: &str) -> bool {
        if name == "v-else" {
            self.complexity.cognitive = self.complexity.cognitive.saturating_add(1);
            return true;
        }
        false
    }

    /// Score a valued directive. Returns `true` when it is a control-flow
    /// directive (so the element opens a cognitive nesting level for its
    /// subtree). The control-flow construct is scored at the CURRENT nesting,
    /// before the subtree deepens.
    fn scan_attribute_value(
        &mut self,
        name: &str,
        value_start: usize,
        value_end: usize,
    ) -> Result<bool, ScanError> {
        let value = &self.source[value_start..value_end];
        if is_control_flow_directive(name) {
            self.complexity.add_control_flow(self.nesting);
            self.complexity
                .add_expression(value, value_start, self.nesting)?;
            return Ok(true);
        }
        if is_bound_directive(name) {
            self.complexity
                .add_expression(value, value_start, self.nesting)?;
        }
        Ok(false)
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

#[cfg(all(test, not(miri)))]
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

    #[test]
    fn markup_depth_does_not_inflate_cognitive() {
        // The SAME single `v-if` must score identically whether it sits at the
        // top of the template or buried under plain `<div>` wrappers: cognitive
        // nesting tracks CONTROL-FLOW depth, not raw markup depth (issue #1281's
        // failure mode applied to Vue). Regression for the tag-stack fix.
        let shallow =
            compute_vue_template_complexity(r#"<template><div v-if="ok">x</div></template>"#)
                .expect("a v-if has complexity");
        let deep = compute_vue_template_complexity(
            r#"<template><div><div><div><div><div><div><div v-if="ok">x</div></div></div></div></div></div></div></template>"#,
        )
        .expect("a v-if has complexity");
        assert_eq!(
            (shallow.cyclomatic, shallow.cognitive),
            (deep.cyclomatic, deep.cognitive),
            "markup nesting must not change the cognitive weight of a control-flow construct: shallow={shallow:?} deep={deep:?}"
        );
    }

    #[test]
    fn nested_control_flow_still_increments_cognitive() {
        // A v-if nested inside ANOTHER v-if's element subtree IS deeper and must
        // weigh more than two sibling v-ifs, so the fix did not flatten genuine
        // control-flow nesting (only markup nesting was removed).
        let nested = compute_vue_template_complexity(
            r#"<template><div v-if="a"><span v-if="b">x</span></div></template>"#,
        )
        .expect("nested control flow has complexity");
        let siblings = compute_vue_template_complexity(
            r#"<template><div v-if="a">x</div><div v-if="b">y</div></template>"#,
        )
        .expect("sibling control flow has complexity");
        assert_eq!(
            nested.cyclomatic, siblings.cyclomatic,
            "both have two v-if branches: same cyclomatic"
        );
        assert!(
            nested.cognitive > siblings.cognitive,
            "nested control flow must weigh more than sibling control flow: nested={nested:?} siblings={siblings:?}"
        );
    }
}
