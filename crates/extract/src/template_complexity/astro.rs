//! Astro `<template>` (markup) complexity.
//!
//! Unlike Vue/Svelte/Angular, Astro has no directive-based control flow: its
//! conditionals and loops are ordinary JavaScript expressions inside `{ ... }`
//! markup regions (`{cond && <X/>}`, `{cond ? a : b}`, `{list.map(...)}`). This
//! scanner scores each `{ ... }` expression through the shared engine (logical
//! operators, ternary branches, optional chaining) and counts each iteration
//! call (`.map` / `.flatMap` / `.forEach`) as a control-flow construct, the
//! markup analog of a Vue `v-for` / Svelte `{#each}`. Astro frontmatter,
//! `<script>` / `<style>` blocks, and HTML comments are masked first so only the
//! markup is scored (frontmatter functions are scored separately as real
//! `FunctionComplexity` entries by the Astro parse path).

use std::sync::LazyLock;

use fallow_types::extract::FunctionComplexity;

use super::build_template_complexity;
use super::engine::{ScanError, TemplateComplexity, find_matching_delimiter};

/// Mask Astro frontmatter (`---...---`), `<script>` / `<style>` blocks, and HTML
/// comments. The frontmatter alternative anchors at `\A` so it only matches the
/// leading delimited block; script/style attribute lists are quote-aware so a
/// `>` inside an attribute value does not end the tag early.
static MASK_RE: LazyLock<regex::Regex> = LazyLock::new(|| {
    crate::static_regex(
        r#"(?is)\A\s*---[ \t]*\r?\n.*?\r?\n---|<script\b(?:[^>"']|"[^"]*"|'[^']*')*>.*?</script>|<style\b(?:[^>"']|"[^"]*"|'[^']*')*>.*?</style>|<!--.*?-->"#,
    )
});

/// Iteration method calls that act as Astro's loop construct.
const ITERATION_CALLS: [&str; 3] = [".map(", ".flatMap(", ".forEach("];

/// Compute the synthetic `<template>` complexity for an `.astro` source, or
/// `None` when the markup is trivial (no branches/loops) or malformed.
#[must_use]
pub fn compute_astro_template_complexity(source: &str) -> Option<FunctionComplexity> {
    let masked = super::mask_ranges(source, &MASK_RE);
    let mut complexity = TemplateComplexity::default();
    if scan_markup_expressions(&masked, &mut complexity).is_err() {
        return None;
    }
    build_template_complexity(source, &complexity)
}

/// Walk each top-level `{ ... }` markup expression region of the masked source,
/// scoring its JS-expression complexity and counting iteration calls. Returns
/// `Err` only on a structurally malformed (unbalanced) brace, which drops the
/// whole synthetic entry.
fn scan_markup_expressions(
    masked: &str,
    complexity: &mut TemplateComplexity,
) -> Result<(), ScanError> {
    let bytes = masked.as_bytes();
    let mut offset = 0;
    while offset < bytes.len() {
        if bytes[offset] != b'{' {
            offset += 1;
            continue;
        }
        let close = find_matching_delimiter(masked, offset, b'{', b'}')?;
        let expr = &masked[offset + 1..close];
        // A benign non-boolean markup expression (an object literal `{{...}}`, a
        // plain `{value}`) scores zero and may not tokenize as a boolean
        // expression; ignore a per-expression scan error and continue rather than
        // dropping the whole entry.
        let _ = complexity.add_expression(expr, offset + 1, 0);
        for needle in ITERATION_CALLS {
            let mut from = 0;
            while let Some(pos) = expr[from..].find(needle) {
                complexity.add_control_flow(0);
                from += pos + needle.len();
            }
        }
        offset = close + 1;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::compute_astro_template_complexity;

    #[test]
    fn trivial_template_has_no_entry() {
        let source = "---\nconst x = 1;\n---\n<h1>{x}</h1>\n";
        assert!(compute_astro_template_complexity(source).is_none());
    }

    #[test]
    fn conditional_and_iteration_score() {
        let source = "---\nconst items = [];\nconst show = true;\n---\n\
<ul>{show && items.map((i) => <li>{i.name ?? 'x'}</li>)}</ul>\n";
        let fc = compute_astro_template_complexity(source).expect("non-trivial template");
        // `&&` (logical) + `.map(` (iteration) + `??` (nullish) raise it above the
        // trivial baseline.
        assert!(fc.cyclomatic > 1, "cyclomatic should rise: {fc:?}");
        assert_eq!(fc.name, "<template>");
    }

    #[test]
    fn frontmatter_braces_are_masked() {
        // The frontmatter object literal `{ a: 1 }` must NOT be scored as a markup
        // expression; the markup here is trivial, so there is no entry.
        let source = "---\nconst cfg = { a: 1, b: 2 };\n---\n<div>plain</div>\n";
        assert!(compute_astro_template_complexity(source).is_none());
    }

    #[test]
    fn malformed_brace_drops_entry() {
        let source = "---\n---\n<div>{a && b</div>\n";
        assert!(compute_astro_template_complexity(source).is_none());
    }
}
