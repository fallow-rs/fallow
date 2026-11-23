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

use fallow_types::extract::{ComplexityContributionKind, FunctionComplexity};

use super::build_unit_complexity;
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
    let (complexity, region) = VueScanner::new(&markup).scan().ok()?;
    build_unit_complexity(source, &complexity, "<template>", region)
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
    template_depth: usize,
    template_start: Option<usize>,
    template_end: Option<usize>,
    multiple_roots: bool,
}

impl<'a> VueScanner<'a> {
    fn new(source: &'a str) -> Self {
        Self {
            source,
            complexity: TemplateComplexity::default(),
            nesting: 0,
            tag_stack: Vec::new(),
            template_depth: 0,
            template_start: None,
            template_end: None,
            multiple_roots: false,
        }
    }

    fn scan(mut self) -> Result<(TemplateComplexity, Option<std::ops::Range<usize>>), ScanError> {
        let mut offset = 0;
        while offset < self.source.len() {
            if self.source[offset..].starts_with("{{") {
                let end = self.find_required(offset + 2, "}}")?;
                if self.template_depth > 0 {
                    self.complexity.add_expression(
                        &self.source[offset + 2..end],
                        offset + 2,
                        self.nesting,
                    )?;
                }
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
        let region = self.template_region();
        Ok((self.complexity, region))
    }

    fn template_region(&self) -> Option<std::ops::Range<usize>> {
        if !self.multiple_roots && self.template_depth == 0 {
            if let (Some(start), Some(end)) = (self.template_start, self.template_end) {
                if start < end {
                    return Some(start..end);
                }
            }
        }
        None
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
            let close_name = read_close_tag_name(self.source, offset);
            if close_name.eq_ignore_ascii_case("template") && self.template_depth > 0 {
                self.template_depth -= 1;
                if self.template_depth == 0 && !self.multiple_roots {
                    self.template_end = Some(after);
                }
            }
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
        let is_template = tag_name.eq_ignore_ascii_case("template");
        if is_template {
            if self.template_depth == 0 {
                if self.template_start.is_none() {
                    self.template_start = Some(offset);
                } else {
                    self.multiple_roots = true;
                }
            }
            if self_closing {
                if self.template_depth == 0 && !self.multiple_roots {
                    self.template_end = Some(after);
                }
            } else {
                self.template_depth += 1;
            }
        }

        let in_template = self.template_depth > 0 || is_template;
        let has_control_flow = if in_template {
            self.scan_attributes(offset, tag_end)?
        } else {
            false
        };

        if in_template && !self_closing && !is_void_tag(tag_name) {
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
                has_control_flow |= self.scan_valueless_attr(name, name_start);
                continue;
            }
            offset = skip_whitespace(self.source, offset + 1);
            let (value_start, value_end, next_offset) = read_attribute_value(self.source, offset)?;
            has_control_flow |=
                self.scan_attribute_value(name, name_start, value_start, value_end)?;
            offset = next_offset;
        }
        Ok(has_control_flow)
    }

    /// A directive written without a value: only bare `v-else` matters (a
    /// control-flow continuation). Mirrors Angular's bare `@else`: cognitive
    /// +1, no cyclomatic increment (the new branch path is owned by the paired
    /// `v-if`). Returns `true` for `v-else` so its element opens a nesting level.
    fn scan_valueless_attr(&mut self, name: &str, name_start: usize) -> bool {
        if name == "v-else" {
            self.complexity
                .inc_cognitive_flat(name_start, ComplexityContributionKind::Else);
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
        name_start: usize,
        value_start: usize,
        value_end: usize,
    ) -> Result<bool, ScanError> {
        let value = &self.source[value_start..value_end];
        if is_control_flow_directive(name) {
            self.complexity
                .add_control_flow(name_start, control_flow_kind(name), self.nesting);
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

/// The breakdown kind for a control-flow directive. `v-else-if` maps to
/// `ElseIf` rather than `If` so a cascade reads the way the source does.
/// `v-show` is a conditional render toggle, so it reports as a plain `If`.
fn control_flow_kind(name: &str) -> ComplexityContributionKind {
    match name {
        "v-else-if" => ComplexityContributionKind::ElseIf,
        "v-for" => ComplexityContributionKind::ForOf,
        _ => ComplexityContributionKind::If,
    }
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

fn read_close_tag_name(source: &str, offset: usize) -> &str {
    let rest = &source[offset + 2..];
    let trimmed = rest.trim_start();
    let end = trimmed
        .find(|c: char| c.is_ascii_whitespace() || c == '>' || c == '/')
        .unwrap_or(trimmed.len());
    &trimmed[..end]
}

fn is_void_tag(tag_name: &str) -> bool {
    VOID_HTML_TAGS
        .iter()
        .any(|void| void.eq_ignore_ascii_case(tag_name))
}

#[cfg(all(test, not(miri)))]
mod tests {
    use fallow_types::extract::{ComplexityContributionKind, ComplexityMetric};

    use super::compute_vue_template_complexity;

    /// The reporter's shape in #2150: a `v-if` / `v-else` pair must arrive with a
    /// breakdown, not just a total. Before the shared engine recorded
    /// contributions this returned an empty vec for every template, so the
    /// editor had nothing to render next to the finding.
    #[test]
    fn v_if_else_reports_its_decision_points() {
        let complexity = compute_vue_template_complexity(
            r#"<template>
  <p v-if="a">1</p>
  <p v-else-if="b">2</p>
  <p v-else>3</p>
</template>
"#,
        )
        .expect("template should have complexity");

        let kinds: Vec<ComplexityContributionKind> = complexity
            .contributions
            .iter()
            .map(|contribution| contribution.kind)
            .collect();
        assert!(
            kinds.contains(&ComplexityContributionKind::If)
                && kinds.contains(&ComplexityContributionKind::ElseIf)
                && kinds.contains(&ComplexityContributionKind::Else),
            "{kinds:?}"
        );

        // Each directive is reported on the line it is written on.
        let lines: Vec<u32> = complexity
            .contributions
            .iter()
            .map(|contribution| contribution.line)
            .collect();
        assert_eq!(*lines.iter().min().expect("contributions"), 2);
        assert_eq!(*lines.iter().max().expect("contributions"), 4);
    }

    /// The breakdown must always explain the whole number. `cyclomatic` carries
    /// the implicit straight-line path that is not a contribution; `cognitive`
    /// starts at zero, so it must be fully attributed.
    #[test]
    fn contribution_weights_sum_to_the_reported_metrics() {
        let complexity = compute_vue_template_complexity(
            r#"<template>
  <div v-if="user?.enabled && flags.dashboard">
    <li v-for="item in items" :key="item.id">
      <badge :color="item.level > 3 ? 'red' : 'green'" />
    </li>
  </div>
  <p v-else>none</p>
</template>
"#,
        )
        .expect("template should have complexity");

        let sum = |metric: ComplexityMetric| -> u16 {
            complexity
                .contributions
                .iter()
                .filter(|contribution| contribution.metric == metric)
                .map(|contribution| contribution.weight)
                .sum()
        };
        assert_eq!(sum(ComplexityMetric::Cyclomatic), complexity.cyclomatic - 1);
        assert_eq!(sum(ComplexityMetric::Cognitive), complexity.cognitive);
    }

    /// Offsets are recorded against the masked markup, which preserves byte
    /// offsets, so a `<script>` block above the template must not shift the
    /// reported lines.
    #[test]
    fn contributions_are_anchored_past_a_leading_script_block() {
        let complexity = compute_vue_template_complexity(
            r#"<script setup>
const a = 1;
const b = 2;
</script>

<template>
  <p v-if="a && b">x</p>
</template>
"#,
        )
        .expect("template should have complexity");

        assert!(!complexity.contributions.is_empty());
        for contribution in &complexity.contributions {
            assert_eq!(contribution.line, 7, "{contribution:?}");
        }
    }

    /// A nested control-flow construct carries the nesting penalty in its own
    /// weight, so a consumer can explain a `+2` as "+1 base, +1 nesting".
    #[test]
    fn nested_control_flow_records_its_nesting_penalty() {
        let complexity = compute_vue_template_complexity(
            r#"<template><div v-if="a"><li v-for="i in items">{{ i }}</li></div></template>"#,
        )
        .expect("template should have complexity");

        let nested = complexity
            .contributions
            .iter()
            .find(|contribution| {
                contribution.metric == ComplexityMetric::Cognitive
                    && contribution.kind == ComplexityContributionKind::ForOf
            })
            .expect("nested v-for contribution");
        assert_eq!(nested.nesting, 1, "{nested:?}");
        assert_eq!(nested.weight, 2, "{nested:?}");
    }

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

    /// Issue #2798: the shared engine scans into `${}` interpolations.
    #[test]
    fn template_literal_interpolation_in_binding_counts() {
        let complexity = compute_vue_template_complexity(
            r#"<template><span :title="`a ${ isReady ? label : suffix } b`" :class="`${ on || off }`"></span></template>"#,
        )
        .expect("template should have complexity");
        assert_eq!(complexity.cyclomatic, 3, "{complexity:?}");
        assert_eq!(complexity.cognitive, 2, "{complexity:?}");

        assert!(
            compute_vue_template_complexity(
                r#"<template><span :title="`a && b ? c : d`"></span></template>"#,
            )
            .is_none()
        );
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

    #[test]
    fn short_template_in_long_sfc_reports_template_block_lines() {
        let mut script_lines = Vec::new();
        script_lines.push("<script setup lang=\"ts\">");
        for _ in 0..80 {
            script_lines.push("const x = 1;");
        }
        script_lines.push("</script>");
        let script = script_lines.join("\n");
        let source = format!(
            "{script}\n\n<template>\n  <div v-if=\"ready\">\n    <p v-if=\"active\">ok</p>\n  </div>\n</template>\n"
        );
        let total_lines = source.lines().count();
        assert!(total_lines > 85, "total file must exceed 85 lines");

        let complexity = compute_vue_template_complexity(&source)
            .expect("template with control flow has complexity");
        assert_eq!(
            complexity.line_count, 5,
            "template unit line_count must span only the template block (5 lines)"
        );
        let expected_anchor = u32::try_from(script_lines.len() + 3).unwrap();
        assert_eq!(
            complexity.line, expected_anchor,
            "anchor line must point at the first contributing construct"
        );
    }

    #[test]
    fn large_template_remains_detected_with_accurate_line_count() {
        let mut script_lines = Vec::new();
        script_lines.push("<script setup lang=\"ts\">");
        for _ in 0..30 {
            script_lines.push("const s = 'setup';");
        }
        script_lines.push("</script>");
        let script = script_lines.join("\n");

        let mut template_lines = Vec::new();
        template_lines.push("<template>");
        template_lines.push("  <div v-if=\"ready\">");
        for _ in 0..70 {
            template_lines.push("    <p>row</p>");
        }
        template_lines.push("  </div>");
        template_lines.push("</template>");
        let template = template_lines.join("\n");

        let source = format!("{script}\n\n{template}\n");
        let total_lines = source.lines().count();
        let expected_lines = u32::try_from(template_lines.len()).unwrap();
        assert!(
            total_lines > 100,
            "total file lines ({total_lines}) must exceed template lines ({expected_lines})"
        );

        let complexity = compute_vue_template_complexity(&source)
            .expect("template with control flow has complexity");
        assert_eq!(
            complexity.line_count, expected_lines,
            "large template line_count must accurately reflect the block span (74 lines), not total file ({total_lines})"
        );
        assert!(
            complexity.line_count > 60,
            "template must exceed the default max_unit_size ceiling of 60 LOC"
        );
    }

    #[test]
    fn nested_templates_and_self_closing_preserve_root_range() {
        let source = r#"<script setup lang="ts">
import { ref } from 'vue';
const active = ref(true);
</script>

<template>
  <div class="panel">
    <template v-if="active">
      <span>nested</span>
    </template>
    <template />
  </div>
</template>
"#;
        let total_lines = source.lines().count();
        assert!(
            total_lines > 10,
            "total file lines ({total_lines}) must exceed template lines (8)"
        );
        let complexity = compute_vue_template_complexity(source)
            .expect("template with control flow has complexity");
        assert_eq!(
            complexity.line_count, 8,
            "template unit line_count must span exactly the 8 lines of the root template"
        );
    }

    #[test]
    fn stray_closing_template_tag_does_not_extend_region() {
        let source = r#"<script setup lang="ts">
const ready = true;
</script>

<template>
  <div v-if="ready">
    <p>content</p>
  </div>
</template>

</template>

<div class="footer">
  <p>trailing markup</p>
</div>
"#;
        let total_lines = source.lines().count();
        assert!(total_lines > 15, "fixture must exceed template length");

        let complexity = compute_vue_template_complexity(source)
            .expect("template with control flow has complexity");
        assert_eq!(
            complexity.line_count, 5,
            "stray closing template tag must not extend the template unit region"
        );
    }

    #[test]
    fn multiple_root_templates_falls_back_safely_without_bridging() {
        let source = r#"<template>
  <div v-if="a">first</div>
</template>

<script setup lang="ts">
const mid1 = 1;
const mid2 = 2;
</script>

<template>
  <div v-if="b">second</div>
</template>
"#;
        let total_lines = u32::try_from(source.lines().count()).unwrap();
        let complexity = compute_vue_template_complexity(source)
            .expect("control flow in templates produces complexity");
        assert_eq!(
            complexity.line_count, total_lines,
            "multiple root templates must fall back to total file line count rather than bridging across script"
        );
    }

    #[test]
    fn constructs_outside_template_do_not_anchor_or_contribute_complexity() {
        let source = r#"<custom-info v-if="admin">
  {{ info && extra }}
</custom-info>

<script setup lang="ts">
const ready = true;
</script>

<template>
  <div v-if="ready">
    <p>content</p>
  </div>
</template>
"#;
        let complexity = compute_vue_template_complexity(source)
            .expect("template control flow produces complexity");
        assert_eq!(
            complexity.line_count, 5,
            "template line_count must measure only the <template> block"
        );
        assert_eq!(
            complexity.line, 11,
            "anchor line must point to the v-if inside <template>, not the custom block outside"
        );
        assert_eq!(
            complexity.cyclomatic, 2,
            "cyclomatic complexity must score only the v-if inside template (1 base + 1 branch)"
        );
    }
}
