//! Synthetic `<template>` complexity for Svelte single-file components.
//!
//! Scores Svelte logic blocks (`{#if}` / `{:else if}` / `{#each}` / `{#await}` /
//! `{:then}` / `{:catch}` / `{#key}`) plus `{ }` text interpolations, bound block
//! expressions, AND attribute-binding expressions inside a tag (`class={cond ? a
//! : b}`, `onclick={x && y}`, `class:active={...}`), which carry the same
//! expression complexity Vue's `:class` and Angular's `[class]` score. All reuse
//! the framework-agnostic JS-expression engine.
//! `<script>` / `<style>` blocks and `<!-- -->` comments are masked out
//! (replaced with equal-length spaces so byte offsets stay accurate) so script
//! control flow is NOT double-counted here (it is scored separately by
//! `translate_script_complexity`). Nesting depth tracks the logic-block stack:
//! an `{#each}` inside an `{#if}` scores deeper than a top-level block, matching
//! Angular's per-block nesting model.

use std::sync::LazyLock;

use fallow_types::extract::FunctionComplexity;

use super::build_template_complexity;
use super::engine::{ScanError, TemplateComplexity, skip_quoted};

static MASK_RE: LazyLock<regex::Regex> = LazyLock::new(|| {
    crate::static_regex(
        r#"(?is)<script\b(?:[^>"']|"[^"]*"|'[^']*')*>[\s\S]*?</script\s*>|<style\b(?:[^>"']|"[^"]*"|'[^']*')*>[\s\S]*?</style\s*>|<!--[\s\S]*?-->"#,
    )
});

/// Compute synthetic `<template>` complexity for a Svelte SFC. Returns `None`
/// for a trivial template (no logic blocks, no non-trivial expression) or any
/// malformed-markup short-circuit.
#[must_use]
pub fn compute_svelte_template_complexity(source: &str) -> Option<FunctionComplexity> {
    let markup = mask_non_template(source);
    let complexity = SvelteScanner::new(&markup).scan().ok()?;
    build_template_complexity(source, &complexity)
}

/// Replace `<script>` / `<style>` blocks and HTML comments with equal-length
/// runs of spaces so the remaining markup byte offsets are unchanged. Mirrors
/// the masking convention in `crate::sfc_template::svelte`.
fn mask_non_template(source: &str) -> String {
    super::mask_ranges(source, &MASK_RE)
}

struct SvelteScanner<'a> {
    source: &'a str,
    complexity: TemplateComplexity,
    nesting: u16,
}

impl<'a> SvelteScanner<'a> {
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
            match self.source.as_bytes()[offset] {
                b'<' => offset = self.scan_element(offset)?,
                b'{' => offset = self.scan_curly(offset)?,
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

    /// Scan an HTML tag's attribute bindings for expression complexity. Markup
    /// elements carry no logic-block nesting (Svelte nesting is logic-block only),
    /// but a `{ ... }` binding inside the tag (`class={cond ? a : b}`,
    /// `onclick={x && y}`, `class:active={loading || !valid}`, a `{shorthand}` or
    /// `{...spread}`) carries the same kind of expression complexity that Vue's
    /// `:class` and Angular's `[class]` bound attributes score, so it must be
    /// counted here for cross-framework parity (it is NOT reached by the
    /// top-level text-interpolation walk, which never sees inside a `<tag ...>`).
    /// Quote-tracking keeps a `>` inside an attribute value from ending the tag
    /// early; a `{ ... }` is scored whether bare (`class={x}`) or embedded in a
    /// quoted value (`class="a {x}"`), and `find_matching_curly` skips any nested
    /// strings / braces inside the expression.
    fn scan_element(&mut self, offset: usize) -> Result<usize, ScanError> {
        let mut index = offset + 1;
        let mut quote: Option<u8> = None;
        while index < self.source.len() {
            let byte = self.source.as_bytes()[index];
            match byte {
                b'{' => {
                    let close = find_matching_curly(self.source, index)?;
                    self.add_expr_slice(self.source[index + 1..close].trim(), index + 1)?;
                    index = close + 1;
                }
                b'\'' | b'"' => {
                    match quote {
                        Some(open) if open == byte => quote = None,
                        None => quote = Some(byte),
                        Some(_) => {}
                    }
                    index += 1;
                }
                b'>' if quote.is_none() => return Ok(index + 1),
                _ => {
                    index += self.source[index..]
                        .chars()
                        .next()
                        .map_or(1, char::len_utf8);
                }
            }
        }
        Err(ScanError)
    }

    fn scan_curly(&mut self, offset: usize) -> Result<usize, ScanError> {
        let end = find_matching_curly(self.source, offset)?;
        let inner = self.source[offset + 1..end].trim();
        let inner_offset = offset + 1;
        self.dispatch_curly(inner, inner_offset)?;
        Ok(end + 1)
    }

    fn dispatch_curly(&mut self, inner: &str, inner_offset: usize) -> Result<(), ScanError> {
        if inner.is_empty() {
            return Ok(());
        }
        if let Some(rest) = inner.strip_prefix('/') {
            // Closing block (`{/if}`, `{/each}`, ...): pop one nesting level.
            let _ = rest;
            self.nesting = self.nesting.saturating_sub(1);
            return Ok(());
        }
        if let Some(rest) = inner.strip_prefix('#') {
            return self.scan_block_open(rest, inner_offset);
        }
        if let Some(rest) = inner.strip_prefix(':') {
            return self.scan_block_continuation(rest, inner_offset);
        }
        if let Some(rest) = inner.strip_prefix('@') {
            return self.scan_at_directive(rest, inner_offset);
        }
        // Plain `{ expr }` text interpolation.
        self.complexity
            .add_expression(inner, inner_offset, self.nesting)
    }

    fn scan_block_open(&mut self, rest: &str, inner_offset: usize) -> Result<(), ScanError> {
        let (keyword, after) = split_keyword(rest);
        match keyword {
            // `{#if cond}` / `{#key expr}` / `{#await promise}`: one branch each,
            // whose whole remainder is the bound expression.
            "if" | "key" | "await" => {
                self.add_control_flow_with_expr(after, inner_offset)?;
                self.nesting = self.nesting.saturating_add(1);
                Ok(())
            }
            "each" => {
                // `{#each <iterable> as <binding> (<key>)}`: score the iterable
                // but not the binding pattern.
                let iterable = each_iterable(after);
                self.complexity.add_control_flow(self.nesting);
                self.add_expr_slice(iterable, inner_offset)?;
                self.nesting = self.nesting.saturating_add(1);
                Ok(())
            }
            // `{#snippet name(params)}` opens a scope but is not control flow.
            "snippet" => {
                self.nesting = self.nesting.saturating_add(1);
                Ok(())
            }
            _ => Ok(()),
        }
    }

    fn scan_block_continuation(
        &mut self,
        rest: &str,
        inner_offset: usize,
    ) -> Result<(), ScanError> {
        let (keyword, after) = split_keyword(rest);
        match keyword {
            "else" => {
                let after_trim = after.trim_start();
                if let Some(condition) = after_trim.strip_prefix("if") {
                    // `{:else if cond}`: a new branch. Match Angular's `@else if`:
                    // cyclomatic +1, cognitive +1 (flat, not nesting-weighted).
                    self.complexity.cyclomatic = self.complexity.cyclomatic.saturating_add(1);
                    self.complexity.cognitive = self.complexity.cognitive.saturating_add(1);
                    self.add_expr_slice(condition.trim(), inner_offset)?;
                } else {
                    // Bare `{:else}`: continuation. Match Angular's bare `@else`:
                    // cognitive +1, no cyclomatic increment.
                    self.complexity.cognitive = self.complexity.cognitive.saturating_add(1);
                }
                Ok(())
            }
            // `{:then ...}` / `{:catch ...}`: each promise-state branch adds one
            // path. Flat cognitive +1 (the await frame already supplied the
            // nesting weight), mirroring the else-if branch treatment.
            "then" | "catch" => {
                self.complexity.cyclomatic = self.complexity.cyclomatic.saturating_add(1);
                self.complexity.cognitive = self.complexity.cognitive.saturating_add(1);
                Ok(())
            }
            _ => Ok(()),
        }
    }

    /// `{@const x = expr}` / `{@html expr}` / `{@render expr}` / `{@debug expr}`
    /// carry a bound expression worth scoring, but are not control flow.
    fn scan_at_directive(&mut self, rest: &str, inner_offset: usize) -> Result<(), ScanError> {
        let (keyword, after) = split_keyword(rest);
        match keyword {
            "const" => {
                if let Some(eq) = after.find('=') {
                    let expr = &after[eq + 1..];
                    let base = inner_offset + 1 + keyword.len() + eq + 1;
                    self.complexity.add_expression(expr, base, self.nesting)?;
                }
                Ok(())
            }
            "html" | "render" | "debug" => self.add_expr_slice(after.trim(), inner_offset),
            _ => Ok(()),
        }
    }

    /// Score a control-flow block whose entire remainder is its bound expression
    /// (`{#if cond}`, `{#await promise}`, `{#key expr}`).
    fn add_control_flow_with_expr(
        &mut self,
        expr: &str,
        inner_offset: usize,
    ) -> Result<(), ScanError> {
        self.complexity.add_control_flow(self.nesting);
        self.add_expr_slice(expr.trim(), inner_offset)
    }

    /// Score `slice` as a bound expression. The `inner_offset` (the block-open
    /// position) is a coarse anchor for the synthetic finding's line/col, which
    /// is all `first_offset` needs: the precise expression column is not
    /// surfaced for the aggregate `<template>` entry.
    fn add_expr_slice(&mut self, slice: &str, inner_offset: usize) -> Result<(), ScanError> {
        if slice.is_empty() {
            return Ok(());
        }
        self.complexity
            .add_expression(slice, inner_offset, self.nesting)
    }
}

/// Find the `}` that closes the `{` at `open`, honoring nested `{ }`, quoted
/// strings, and template literals so a `}` inside a string or nested object
/// does not end the section early. Byte-safe over multibyte text.
fn find_matching_curly(source: &str, open: usize) -> Result<usize, ScanError> {
    let mut offset = open + 1;
    let mut depth = 1_u16;
    while offset < source.len() {
        match source.as_bytes()[offset] {
            b'\'' | b'"' | b'`' => offset = skip_quoted(source, offset)?,
            b'{' => {
                depth = depth.saturating_add(1);
                offset += 1;
            }
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Ok(offset);
                }
                offset += 1;
            }
            _ => offset += source[offset..].chars().next().map_or(1, char::len_utf8),
        }
    }
    Err(ScanError)
}

/// Split a block body into its leading keyword (`if`, `each`, `else`, ...) and
/// the remainder after the first whitespace run.
fn split_keyword(body: &str) -> (&str, &str) {
    match body.find(char::is_whitespace) {
        Some(index) => (&body[..index], &body[index..]),
        None => (body, ""),
    }
}

/// Extract the iterable expression from an `{#each ...}` body remainder. The
/// grammar is `<iterable> as <binding>(...)`; we score only the iterable, the
/// part before the ` as ` keyword (falling back to the whole remainder when no
/// `as` is present, e.g. a malformed or keyless each).
fn each_iterable(after: &str) -> &str {
    let trimmed = after.trim_start();
    let bytes = trimmed.as_bytes();
    let mut index = 0;
    let mut depth = 0_u16;
    while index < bytes.len() {
        match bytes[index] {
            b'(' | b'[' | b'{' => {
                depth = depth.saturating_add(1);
                index += 1;
            }
            b')' | b']' | b'}' => {
                depth = depth.saturating_sub(1);
                index += 1;
            }
            _ if depth == 0
                && trimmed[index..].starts_with("as")
                && before_is_boundary(trimmed, index)
                && after_is_boundary(trimmed, index + 2) =>
            {
                return trimmed[..index].trim();
            }
            _ => index += trimmed[index..].chars().next().map_or(1, char::len_utf8),
        }
    }
    trimmed
}

fn before_is_boundary(source: &str, index: usize) -> bool {
    index == 0 || source.as_bytes()[index - 1].is_ascii_whitespace()
}

fn after_is_boundary(source: &str, index: usize) -> bool {
    index >= source.len() || source.as_bytes()[index].is_ascii_whitespace()
}

#[cfg(all(test, not(miri)))]
mod tests {
    use super::compute_svelte_template_complexity;

    #[test]
    fn each_in_if_with_else_if_counts() {
        let complexity = compute_svelte_template_complexity(
            r"
{#if user?.enabled && ready}
  {#each items as item (item.id)}
    <p>{item.level > 3 ? 'high' : 'low'}</p>
  {/each}
{:else if fallback}
  <p>fallback</p>
{/if}
",
        )
        .expect("template should have complexity");
        assert!(complexity.cyclomatic >= 4, "{complexity:?}");
        assert!(complexity.cognitive >= 3, "{complexity:?}");
        assert_eq!(complexity.name, "<template>");
    }

    #[test]
    fn else_if_cascade_increments_per_branch() {
        let complexity = compute_svelte_template_complexity(
            "{#if a}<p>1</p>{:else if b}<p>2</p>{:else if c}<p>3</p>{:else}<p>4</p>{/if}",
        )
        .expect("template should have complexity");
        // #if + two :else if = 3 branches on top of baseline 1.
        assert_eq!(complexity.cyclomatic, 4, "{complexity:?}");
    }

    #[test]
    fn bare_else_is_continuation_not_a_branch() {
        let complexity = compute_svelte_template_complexity("{#if a}<p>1</p>{:else}<p>2</p>{/if}")
            .expect("template should have complexity");
        assert_eq!(complexity.cyclomatic, 2, "{complexity:?}");
        assert!(complexity.cognitive >= 2, "{complexity:?}");
    }

    #[test]
    fn await_then_catch_each_count() {
        let complexity = compute_svelte_template_complexity(
            "{#await promise}<p>loading</p>{:then value}<p>{value}</p>{:catch error}<p>{error}</p>{/await}",
        )
        .expect("template should have complexity");
        // #await + :then + :catch = 3 branch increments + baseline.
        assert!(complexity.cyclomatic >= 4, "{complexity:?}");
    }

    #[test]
    fn key_block_counts() {
        let complexity = compute_svelte_template_complexity("{#key selectedId}<Child />{/key}")
            .expect("template should have complexity");
        assert!(complexity.cyclomatic >= 2, "{complexity:?}");
    }

    #[test]
    fn interpolation_expressions_contribute() {
        let complexity =
            compute_svelte_template_complexity("<p>{enabled && draft ? 'Draft' : 'New'}</p>")
                .expect("template should have complexity");
        assert!(complexity.cyclomatic >= 3, "{complexity:?}");
    }

    #[test]
    fn markup_only_template_has_no_synthetic_complexity() {
        assert!(
            compute_svelte_template_complexity(r#"<div class="x"><p>Hello world</p></div>"#)
                .is_none()
        );
    }

    #[test]
    fn script_control_flow_is_not_counted() {
        assert!(
            compute_svelte_template_complexity(
                r"<script>
const x = items.filter((i) => i && i.active);
if (a && b) { go(); }
for (const i of items) { use(i); }
</script>
<p>Static</p>"
            )
            .is_none()
        );
    }

    #[test]
    fn malformed_template_does_not_panic_and_yields_no_entry() {
        // Unterminated block expression.
        assert!(compute_svelte_template_complexity("{#if a && ").is_none());
        // Logical with no RHS inside an interpolation.
        assert!(compute_svelte_template_complexity("<p>{a && }</p>").is_none());
        // Unterminated curly.
        assert!(compute_svelte_template_complexity("<p>{ a && b").is_none());
    }

    #[test]
    fn multibyte_text_does_not_panic() {
        let complexity =
            compute_svelte_template_complexity("{#if a && b}\u{4f4f}\u{6240}<p>{c?.d}</p>{/if}")
                .expect("template should have complexity");
        assert!(complexity.cyclomatic >= 2, "{complexity:?}");
    }

    #[test]
    fn comments_are_masked() {
        assert!(
            compute_svelte_template_complexity("<!-- {#if a && b && c} --><p>plain</p>").is_none()
        );
    }

    #[test]
    fn at_const_rhs_contributes() {
        let complexity = compute_svelte_template_complexity(
            "{#each items as item}{@const ok = item?.a && item?.b}<p>{ok}</p>{/each}",
        )
        .expect("template should have complexity");
        // #each control flow + @const optional chains.
        assert!(complexity.cyclomatic >= 3, "{complexity:?}");
    }

    #[test]
    fn attribute_binding_expressions_are_scored() {
        // A `{ ... }` binding inside a tag carries the same expression complexity
        // as Vue's `:class` and Angular's `[class]`, so it must be scored (it is
        // NOT reached by the top-level text-interpolation walk). Parity
        // regression: this whole class was previously dropped because the tag
        // interior was skipped wholesale.
        let class_bind = compute_svelte_template_complexity(
            r#"<div class={a && b ? "x" : (c || d ? "y" : "z")}>t</div>"#,
        )
        .expect("an attribute binding with logic has complexity");
        assert!(
            class_bind.cyclomatic >= 4,
            "class={{ternary+logical}} should score: {class_bind:?}"
        );
        // Event handler and class: directive bindings are also scored.
        let event =
            compute_svelte_template_complexity("<button onclick={() => a && b && go()}>x</button>")
                .expect("event handler with logic has complexity");
        assert!(
            event.cyclomatic >= 2,
            "onclick logic should score: {event:?}"
        );
        // A `>` inside a quoted attribute value must not end the tag early; a
        // plain shorthand carries no complexity and stays dropped.
        assert!(
            compute_svelte_template_complexity(r#"<a title="a > b" href={url}>x</a>"#).is_none(),
            "a quote-enclosed > plus a plain binding has no logic and is dropped"
        );
    }
}
