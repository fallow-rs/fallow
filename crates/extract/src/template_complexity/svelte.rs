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

use fallow_types::extract::{ComplexityContributionKind, FunctionComplexity};

use super::build_template_complexity;
use super::engine::{
    RegexContext, ScanError, TemplateComplexity, read_identifier, skip_block_comment,
    skip_line_comment, skip_number_literal, skip_quoted, skip_regex_literal,
};

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
                    self.add_expr_slice(self.source[index + 1..close].trim())?;
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
        // Plain `{ expr }` text interpolation. `inner` is already trimmed, so
        // it anchors its own contributions; `inner_offset` still points at the
        // pre-trim `{`.
        self.add_expr_slice(inner)
    }

    fn scan_block_open(&mut self, rest: &str, inner_offset: usize) -> Result<(), ScanError> {
        let (keyword, after) = split_keyword(rest);
        match keyword {
            // `{#if cond}` / `{#key expr}`: one branch each, whose whole
            // remainder is the bound expression. `{#key}` uses the closest
            // shared control-flow vocabulary.
            "if" | "key" => {
                self.add_control_flow_with_expr(
                    after,
                    inner_offset,
                    ComplexityContributionKind::If,
                )?;
                self.nesting = self.nesting.saturating_add(1);
                Ok(())
            }
            // Await blocks can omit the pending branch with
            // `{#await expression then binding}` or
            // `{#await expression catch binding}`. Score the promise expression
            // separately from the binding and record the selected state as the
            // same flat continuation used by `{:then}` / `{:catch}`.
            "await" => {
                let shorthand = split_await_shorthand(after)?;
                self.complexity.add_control_flow(
                    inner_offset,
                    ComplexityContributionKind::Await,
                    self.nesting,
                );
                self.add_expr_slice(shorthand.expression)?;
                if let Some(state) = shorthand.state {
                    let state_offset = self.offset_of(state.keyword);
                    self.complexity.inc_cyclomatic(state_offset, state.kind);
                    self.complexity.inc_cognitive_flat(state_offset, state.kind);
                }
                self.nesting = self.nesting.saturating_add(1);
                Ok(())
            }
            "each" => {
                // `{#each <iterable> as <binding> (<key>)}`: score the iterable
                // but not the binding pattern.
                let iterable = each_iterable(after);
                self.complexity.add_control_flow(
                    inner_offset,
                    ComplexityContributionKind::ForOf,
                    self.nesting,
                );
                self.add_expr_slice(iterable)?;
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
                    self.complexity
                        .inc_cyclomatic(inner_offset, ComplexityContributionKind::ElseIf);
                    self.complexity
                        .inc_cognitive_flat(inner_offset, ComplexityContributionKind::ElseIf);
                    self.add_expr_slice(condition.trim())?;
                } else {
                    // Bare `{:else}`: continuation. Match Angular's bare `@else`:
                    // cognitive +1, no cyclomatic increment.
                    self.complexity
                        .inc_cognitive_flat(inner_offset, ComplexityContributionKind::Else);
                }
                Ok(())
            }
            // `{:then ...}` / `{:catch ...}`: each promise-state branch adds one
            // path. Flat cognitive +1 (the await frame already supplied the
            // nesting weight), mirroring the else-if branch treatment.
            "then" | "catch" => {
                let kind = if keyword == "catch" {
                    ComplexityContributionKind::Catch
                } else {
                    ComplexityContributionKind::Then
                };
                self.complexity.inc_cyclomatic(inner_offset, kind);
                self.complexity.inc_cognitive_flat(inner_offset, kind);
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
            "html" | "render" | "debug" => self.add_expr_slice(after.trim()),
            _ => Ok(()),
        }
    }

    /// Score a control-flow block whose entire remainder is its bound expression
    /// (`{#if cond}`, `{#await promise}`, `{#key expr}`).
    fn add_control_flow_with_expr(
        &mut self,
        expr: &str,
        inner_offset: usize,
        kind: ComplexityContributionKind,
    ) -> Result<(), ScanError> {
        self.complexity
            .add_control_flow(inner_offset, kind, self.nesting);
        self.add_expr_slice(expr.trim())
    }

    /// Score `slice` as a bound expression, anchored at its true position.
    fn add_expr_slice(&mut self, slice: &str) -> Result<(), ScanError> {
        if slice.is_empty() {
            return Ok(());
        }
        let offset = self.offset_of(slice);
        self.complexity.add_expression(slice, offset, self.nesting)
    }

    /// Byte offset of `slice` within the scanned markup.
    ///
    /// Every slice that reaches this scanner is a subslice of `self.source`
    /// (produced by splitting and trimming it), so the pointer delta is its
    /// exact offset, and masking preserves offsets against the original file.
    /// Recovering the offset this way keeps the block keyword parsing free of a
    /// base offset threaded through every `split_keyword` and `trim` step, where
    /// one missed adjustment would silently misplace a breakdown entry.
    ///
    /// Passing a slice from anywhere else (an owned `String`, a literal) would
    /// saturate to `0` and anchor the whole breakdown at line 1, so the
    /// subslice precondition is asserted in debug builds rather than left to
    /// produce quietly wrong output.
    fn offset_of(&self, slice: &str) -> usize {
        let base = self.source.as_ptr().addr();
        let start = slice.as_ptr().addr();
        debug_assert!(
            start >= base && start + slice.len() <= base + self.source.len(),
            "offset_of expects a subslice of the scanned markup"
        );
        start.saturating_sub(base)
    }
}

/// Find the `}` that closes the `{` at `open`, honoring nested `{ }`, quoted
/// strings, template literals, comments, and regex literals. Byte-safe over
/// multibyte text.
fn find_matching_curly(source: &str, open: usize) -> Result<usize, ScanError> {
    let mut offset = open + 1;
    let mut depth = 1_u16;
    let mut regex = RegexContext::expression_start();
    let mut at_start = true;
    let mut directive_keyword_pending = false;
    while offset < source.len() {
        match source.as_bytes()[offset] {
            byte if byte.is_ascii_whitespace() => offset += 1,
            b'#' | b':' | b'@' if at_start => {
                at_start = false;
                directive_keyword_pending = true;
                regex.after_operator();
                offset += 1;
            }
            b'/' if offset == open + 1 && starts_block_close(source, offset) => {
                at_start = false;
                regex.after_operator();
                offset += 1;
            }
            b'\'' | b'"' | b'`' => {
                at_start = false;
                offset = skip_quoted(source, offset)?;
                regex.after_value();
            }
            b'/' if source.as_bytes().get(offset + 1) == Some(&b'/') => {
                offset = skip_line_comment(source, offset);
            }
            b'/' if source.as_bytes().get(offset + 1) == Some(&b'*') => {
                offset = skip_block_comment(source, offset)?;
            }
            b'/' if regex.can_start() => {
                at_start = false;
                offset = skip_regex_literal(source, offset)?;
                regex.after_value();
            }
            b'/' => {
                at_start = false;
                offset += usize::from(source.as_bytes().get(offset + 1) == Some(&b'=')) + 1;
                regex.after_operator();
            }
            b'{' => {
                at_start = false;
                depth = depth.saturating_add(1);
                offset += 1;
                regex.after_operator();
            }
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Ok(offset);
                }
                offset += 1;
                regex.after_value();
            }
            byte if byte == b'_' || byte == b'$' || byte.is_ascii_alphabetic() => {
                at_start = false;
                let (identifier, end) = read_identifier(source, offset).ok_or(ScanError)?;
                if directive_keyword_pending {
                    directive_keyword_pending = identifier == "else";
                    regex.after_operator();
                } else {
                    regex.after_identifier(identifier);
                }
                offset = end;
            }
            byte if byte.is_ascii_digit() => {
                at_start = false;
                offset = skip_number_literal(source, offset);
                regex.after_value();
            }
            _ if source[offset..].starts_with("?.") => {
                at_start = false;
                offset += 2;
                regex.after_property_access();
            }
            b'.' if source[offset..].starts_with("...") => {
                at_start = false;
                offset += 3;
                regex.after_operator();
            }
            b'.' => {
                at_start = false;
                offset += 1;
                regex.after_property_access();
            }
            b'+' | b'-'
                if source.as_bytes().get(offset + 1) == Some(&source.as_bytes()[offset]) =>
            {
                at_start = false;
                offset += 2;
            }
            _ => {
                at_start = false;
                let character = source[offset..].chars().next().ok_or(ScanError)?;
                offset += character.len_utf8();
                regex.after_character(character);
            }
        }
    }
    Err(ScanError)
}

fn starts_block_close(source: &str, slash: usize) -> bool {
    read_identifier(source, slash + 1).is_some_and(|(keyword, end)| {
        matches!(keyword, "if" | "each" | "await" | "key" | "snippet")
            && source.as_bytes().get(end) == Some(&b'}')
    })
}

/// Split a block body into its leading keyword (`if`, `each`, `else`, ...) and
/// the remainder after the first whitespace run.
fn split_keyword(body: &str) -> (&str, &str) {
    match body.find(char::is_whitespace) {
        Some(index) => (&body[..index], &body[index..]),
        None => (body, ""),
    }
}

/// Split an await body into its promise expression and optional shorthand
/// state keyword. Svelte permits both `{#await expression then binding}` and
/// `{#await expression catch binding}`. Only a top-level keyword begins the
/// shorthand state, so nested calls, object literals, strings, and comments
/// remain part of the promise expression.
struct AwaitShorthand<'a> {
    expression: &'a str,
    state: Option<AwaitShorthandState<'a>>,
}

#[derive(Clone, Copy)]
struct AwaitShorthandState<'a> {
    keyword: &'a str,
    kind: ComplexityContributionKind,
}

fn split_await_shorthand(after: &str) -> Result<AwaitShorthand<'_>, ScanError> {
    let trimmed = after.trim_start();
    let bytes = trimmed.as_bytes();
    let mut index = 0;
    let mut depth = 0_u16;
    let mut regex = RegexContext::expression_start();

    while index < bytes.len() {
        match bytes[index] {
            b'\'' | b'"' | b'`' => {
                index = skip_quoted(trimmed, index)?;
                regex.after_value();
            }
            b'/' if bytes.get(index + 1) == Some(&b'/') => {
                index = skip_line_comment(trimmed, index);
            }
            b'/' if bytes.get(index + 1) == Some(&b'*') => {
                index = skip_block_comment(trimmed, index)?;
            }
            b'/' if regex.can_start() => {
                index = skip_regex_literal(trimmed, index)?;
                regex.after_value();
            }
            b'/' => {
                index += usize::from(bytes.get(index + 1) == Some(&b'=')) + 1;
                regex.after_operator();
            }
            b'(' | b'[' | b'{' => {
                depth = depth.saturating_add(1);
                index += 1;
                regex.after_operator();
            }
            b')' | b']' | b'}' => {
                depth = depth.saturating_sub(1);
                index += 1;
                regex.after_value();
            }
            byte if byte == b'_' || byte == b'$' || byte.is_ascii_alphabetic() => {
                let Some((identifier, identifier_end)) = read_identifier(trimmed, index) else {
                    return Err(ScanError);
                };
                let kind = match identifier {
                    "then" => Some(ComplexityContributionKind::Then),
                    "catch" => Some(ComplexityContributionKind::Catch),
                    _ => None,
                };
                if depth == 0
                    && let Some(kind) = kind
                {
                    let binding = trimmed[identifier_end..].trim_start();
                    if before_is_boundary(trimmed, index)
                        && after_is_boundary(trimmed, identifier_end)
                        && starts_binding(binding)
                    {
                        return Ok(AwaitShorthand {
                            expression: trimmed[..index].trim_end(),
                            state: Some(AwaitShorthandState {
                                keyword: identifier,
                                kind,
                            }),
                        });
                    }
                }
                regex.after_identifier(identifier);
                index = identifier_end;
            }
            byte if byte.is_ascii_digit() => {
                index = skip_number_literal(trimmed, index);
                regex.after_value();
            }
            b'.' if trimmed[index..].starts_with("...") => {
                index += 3;
                regex.after_operator();
            }
            b'.' => {
                index += 1;
                regex.after_property_access();
            }
            b'+' | b'-' if bytes.get(index + 1) == Some(&bytes[index]) => {
                index += 2;
            }
            byte if byte.is_ascii_whitespace() => index += 1,
            _ => {
                let character = trimmed[index..].chars().next().ok_or(ScanError)?;
                index += character.len_utf8();
                regex.after_character(character);
            }
        }
    }

    Ok(AwaitShorthand {
        expression: trimmed,
        state: None,
    })
}

fn starts_binding(binding: &str) -> bool {
    binding
        .chars()
        .next()
        .is_some_and(|first| matches!(first, '{' | '[' | '_' | '$') || first.is_alphabetic())
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
    use fallow_types::extract::{ComplexityContributionKind, ComplexityMetric};

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
            "{#await promise}\n<p>loading</p>\n{:then value}\n<p>{value}</p>\n{:catch error}\n<p>{error}</p>\n{/await}",
        )
        .expect("template should have complexity");
        // #await + :then + :catch = 3 branch increments + baseline.
        assert_eq!(complexity.cyclomatic, 4, "{complexity:?}");
        assert_eq!(complexity.cognitive, 3, "{complexity:?}");

        for (line, kind) in [
            (1, ComplexityContributionKind::Await),
            (3, ComplexityContributionKind::Then),
            (5, ComplexityContributionKind::Catch),
        ] {
            let contributions: Vec<_> = complexity
                .contributions
                .iter()
                .filter(|contribution| contribution.line == line)
                .collect();
            assert_eq!(contributions.len(), 2, "line {line}: {complexity:?}");
            assert!(
                contributions
                    .iter()
                    .all(|contribution| contribution.kind == kind && contribution.weight == 1),
                "line {line}: {complexity:?}"
            );
            assert!(
                contributions
                    .iter()
                    .any(|contribution| contribution.metric == ComplexityMetric::Cyclomatic)
            );
            assert!(
                contributions
                    .iter()
                    .any(|contribution| contribution.metric == ComplexityMetric::Cognitive)
            );
        }
    }

    #[test]
    fn await_shorthand_counts_the_selected_state() {
        for (source, state_kind) in [
            (
                "{#await import('./Component.svelte') then { default: Component }}<Component />{/await}",
                ComplexityContributionKind::Then,
            ),
            (
                "{#await load() catch error}<p>{error}</p>{/await}",
                ComplexityContributionKind::Catch,
            ),
            (
                "{#await load() then { value = choose('catch error') }}<p>{value}</p>{/await}",
                ComplexityContributionKind::Then,
            ),
        ] {
            let complexity = compute_svelte_template_complexity(source)
                .expect("shorthand await block should have complexity");
            assert_eq!(complexity.cyclomatic, 3, "{source}: {complexity:?}");
            assert_eq!(complexity.cognitive, 2, "{source}: {complexity:?}");

            for kind in [ComplexityContributionKind::Await, state_kind] {
                let contributions: Vec<_> = complexity
                    .contributions
                    .iter()
                    .filter(|contribution| contribution.kind == kind)
                    .collect();
                assert_eq!(contributions.len(), 2, "{source}: {complexity:?}");
                assert!(
                    contributions
                        .iter()
                        .all(|contribution| contribution.weight == 1),
                    "{source}: {complexity:?}"
                );
            }
        }
    }

    #[test]
    fn await_shorthand_splits_only_the_top_level_state_keyword() {
        let complexity = compute_svelte_template_complexity(
            r#"{#await resolve({ then: "catch" }).then(load) && ready then value}<p>{value}</p>{/await}"#,
        )
        .expect("shorthand await block should have complexity");

        assert_eq!(complexity.cyclomatic, 4, "{complexity:?}");
        assert_eq!(complexity.cognitive, 3, "{complexity:?}");
        assert_eq!(
            complexity
                .contributions
                .iter()
                .filter(|contribution| contribution.kind == ComplexityContributionKind::Then)
                .count(),
            2,
            "{complexity:?}"
        );
    }

    #[test]
    fn await_regex_contents_do_not_start_shorthand() {
        for (source, state_kind, state_line) in [
            (
                "{#await / then value /.test(input)}\n<p>loading</p>\n{:then result}\n<p>{result}</p>\n{/await}",
                ComplexityContributionKind::Then,
                3,
            ),
            (
                "{#await / catch error /.test(input)}\n<p>loading</p>\n{:catch error}\n<p>{error}</p>\n{/await}",
                ComplexityContributionKind::Catch,
                3,
            ),
        ] {
            let complexity = compute_svelte_template_complexity(source)
                .expect("regex await expression should have complexity");
            assert_eq!(complexity.cyclomatic, 3, "{source}: {complexity:?}");
            assert_eq!(complexity.cognitive, 2, "{source}: {complexity:?}");
            assert!(
                complexity
                    .contributions
                    .iter()
                    .filter(|contribution| contribution.kind == state_kind)
                    .all(|contribution| contribution.line == state_line),
                "{source}: {complexity:?}"
            );
        }
    }

    #[test]
    fn await_regex_and_division_expressions_keep_real_shorthand() {
        for (source, state_kind) in [
            (
                "{#await / then value /.test(input) then result}<p>{result}</p>{/await}",
                ComplexityContributionKind::Then,
            ),
            (
                "{#await / catch error /.test(input) catch error}<p>{error}</p>{/await}",
                ComplexityContributionKind::Catch,
            ),
            (
                "{#await total / divisor then result}<p>{result}</p>{/await}",
                ComplexityContributionKind::Then,
            ),
            (
                "{#await of / divisor then result}<p>{result}</p>{/await}",
                ComplexityContributionKind::Then,
            ),
            (
                "{#await values.of / divisor then result}<p>{result}</p>{/await}",
                ComplexityContributionKind::Then,
            ),
        ] {
            let complexity = compute_svelte_template_complexity(source)
                .unwrap_or_else(|| panic!("await shorthand should have complexity: {source}"));
            assert_eq!(complexity.cyclomatic, 3, "{source}: {complexity:?}");
            assert_eq!(complexity.cognitive, 2, "{source}: {complexity:?}");
            assert_eq!(
                complexity
                    .contributions
                    .iter()
                    .filter(|contribution| contribution.kind == state_kind)
                    .count(),
                2,
                "{source}: {complexity:?}"
            );
        }
    }

    #[test]
    fn await_regex_after_return_scores_expression_and_real_shorthand() {
        let source = "{#await (() => { return /[))] then fake/; })() && ready\nthen result}<p>{result}</p>{/await}";
        let complexity = compute_svelte_template_complexity(source)
            .expect("valid regex expression should preserve await complexity");

        assert_eq!(complexity.cyclomatic, 4, "{complexity:?}");
        assert_eq!(complexity.cognitive, 3, "{complexity:?}");
        assert!(
            complexity
                .contributions
                .iter()
                .filter(|contribution| { contribution.kind == ComplexityContributionKind::Then })
                .all(|contribution| contribution.line == 2),
            "{complexity:?}"
        );
        assert_eq!(
            complexity
                .contributions
                .iter()
                .filter(|contribution| {
                    contribution.kind == ComplexityContributionKind::LogicalAnd
                })
                .count(),
            2,
            "{complexity:?}"
        );
    }

    #[test]
    fn if_regex_contents_are_ignored_and_following_operators_are_scored() {
        let source = "{#if /[?():{}&|]+/.test(input) && ready}<p>ready</p>{/if}";
        let complexity = compute_svelte_template_complexity(source)
            .expect("valid regex condition should preserve template complexity");

        assert_eq!(complexity.cyclomatic, 3, "{complexity:?}");
        assert_eq!(complexity.cognitive, 2, "{complexity:?}");
        assert_eq!(
            complexity
                .contributions
                .iter()
                .filter(|contribution| {
                    contribution.kind == ComplexityContributionKind::LogicalAnd
                })
                .count(),
            2,
            "{complexity:?}"
        );
    }

    #[test]
    fn await_bindingless_continuations_count() {
        let complexity = compute_svelte_template_complexity(
            "{#await load()}<p>loading</p>{:then}<p>done</p>{:catch}<p>failed</p>{/await}",
        )
        .expect("bindingless continuations should have complexity");

        assert_eq!(complexity.cyclomatic, 4, "{complexity:?}");
        assert_eq!(complexity.cognitive, 3, "{complexity:?}");
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
        // Shorthand await expression with no logical RHS.
        assert!(compute_svelte_template_complexity("{#await a && then value}{/await}").is_none());
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
