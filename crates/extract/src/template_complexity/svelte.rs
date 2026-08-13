//! Synthetic `<template>` and `<snippet:NAME>` complexity for Svelte
//! single-file components.
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

use super::engine::{
    RegexContext, ScanError, TemplateComplexity, read_identifier, skip_block_comment,
    skip_line_comment, skip_number_literal, skip_quoted, skip_regex_literal,
};
use super::{build_template_complexity, build_unit_complexity};

static MASK_RE: LazyLock<regex::Regex> = LazyLock::new(|| {
    crate::static_regex(
        r#"(?is)<script\b(?:[^>"']|"[^"]*"|'[^']*')*>[\s\S]*?</script\s*>|<style\b(?:[^>"']|"[^"]*"|'[^']*')*>[\s\S]*?</style\s*>|<!--[\s\S]*?-->"#,
    )
});

/// Compute synthetic template-family complexity units for a Svelte SFC: the
/// `<template>` unit plus one `<snippet:NAME>` unit per top-level
/// `{#snippet}` block. A qualifying snippet body is scored with nesting
/// rebased to zero and no longer accumulates into the parent template.
/// Returns an empty vector for a trivial template (no logic blocks, no
/// non-trivial expression) or any malformed-markup short-circuit; trivial
/// snippet bodies emit no unit.
#[must_use]
pub fn compute_svelte_template_complexity(source: &str) -> Vec<FunctionComplexity> {
    let markup = mask_non_template(source);
    let Ok(scan) = SvelteScanner::new(&markup).scan() else {
        return Vec::new();
    };
    let mut units = Vec::new();
    units.extend(build_template_complexity(source, &scan.template));
    for snippet in &scan.snippets {
        units.extend(build_unit_complexity(
            source,
            &snippet.complexity,
            &format!("<snippet:{}>", snippet.name),
            Some(snippet.region.clone()),
        ));
    }
    units
}

/// Replace `<script>` / `<style>` blocks and HTML comments with equal-length
/// runs of spaces so the remaining markup byte offsets are unchanged. Mirrors
/// the masking convention in `crate::sfc_template::svelte`.
fn mask_non_template(source: &str) -> String {
    super::mask_ranges(source, &MASK_RE)
}

/// One finished top-level `{#snippet}` unit: its rebased-nesting accumulator
/// plus the byte span of the whole `{#snippet}`..`{/snippet}` block.
struct SnippetUnit {
    name: String,
    complexity: TemplateComplexity,
    region: std::ops::Range<usize>,
}

/// The snippet unit currently being scored. While active, the scanner's
/// `complexity` accumulator holds the snippet body and the parent template's
/// accumulator is parked here; `inner_depth` counts `{#snippet}` opens folded
/// inside the body so the matching `{/snippet}` close is found by name AND
/// depth, never by generic block depth.
struct ActiveSnippet {
    name: String,
    parent: TemplateComplexity,
    parent_nesting: u16,
    /// Byte offset of the `{` of the opening `{#snippet ...}`.
    start: usize,
    inner_depth: u16,
}

struct SvelteScan {
    template: TemplateComplexity,
    snippets: Vec<SnippetUnit>,
}

struct SvelteScanner<'a> {
    source: &'a str,
    complexity: TemplateComplexity,
    nesting: u16,
    active_snippet: Option<ActiveSnippet>,
    snippets: Vec<SnippetUnit>,
}

impl<'a> SvelteScanner<'a> {
    fn new(source: &'a str) -> Self {
        Self {
            source,
            complexity: TemplateComplexity::default(),
            nesting: 0,
            active_snippet: None,
            snippets: Vec::new(),
        }
    }

    fn scan(mut self) -> Result<SvelteScan, ScanError> {
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
        if self.active_snippet.is_some() {
            // An unclosed `{#snippet}` keeps the all-or-nothing malformed
            // drop: half a unit boundary would misattribute everything after
            // the open to the snippet.
            return Err(ScanError);
        }
        Ok(SvelteScan {
            template: self.complexity,
            snippets: self.snippets,
        })
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
        self.dispatch_curly(inner, inner_offset, offset, end)?;
        Ok(end + 1)
    }

    fn dispatch_curly(
        &mut self,
        inner: &str,
        inner_offset: usize,
        open: usize,
        close: usize,
    ) -> Result<(), ScanError> {
        if inner.is_empty() {
            return Ok(());
        }
        if let Some(rest) = inner.strip_prefix('/') {
            self.close_block(rest.trim(), close);
            return Ok(());
        }
        if let Some(rest) = inner.strip_prefix('#') {
            return self.scan_block_open(rest, inner_offset, open);
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

    /// Close a block (`{/if}`, `{/each}`, `{/snippet}`, ...). The
    /// `{/snippet}` close is matched by NAME against the active unit, not by
    /// generic depth, so an unclosed inner logic block cannot silently
    /// swallow the unit boundary; every other close pops one nesting level.
    fn close_block(&mut self, keyword: &str, close: usize) {
        if keyword == "snippet"
            && let Some(active) = self.active_snippet.as_mut()
        {
            if active.inner_depth > 0 {
                active.inner_depth -= 1;
                self.nesting = self.nesting.saturating_sub(1);
            } else {
                self.finish_snippet(close);
            }
            return;
        }
        self.nesting = self.nesting.saturating_sub(1);
    }

    /// Finalize the active snippet unit: restore the parent accumulator and
    /// nesting, and record the unit with its full block byte span
    /// (`{#snippet` open through the `}` of `{/snippet}`).
    fn finish_snippet(&mut self, close: usize) {
        let Some(active) = self.active_snippet.take() else {
            return;
        };
        let complexity = std::mem::replace(&mut self.complexity, active.parent);
        self.nesting = active.parent_nesting;
        self.snippets.push(SnippetUnit {
            name: active.name,
            complexity,
            region: active.start..close + 1,
        });
    }

    fn scan_block_open(
        &mut self,
        rest: &str,
        inner_offset: usize,
        open: usize,
    ) -> Result<(), ScanError> {
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
            // A snippet opened at logic-block nesting 0 outside any other
            // snippet becomes its own `<snippet:NAME>` unit with nesting
            // rebased to zero. Nesting 0 deliberately INCLUDES a snippet
            // declared as a child of a component tag
            // (`<Table>{#snippet row(item)}...{/snippet}</Table>`): markup
            // elements carry no logic-block nesting (`scan_element` consumes
            // the tag and pushes nothing), and that is the dominant Svelte 5
            // idiom. Nested or unnameable snippets keep the folded behavior:
            // one nesting level, body charged to the enclosing unit.
            "snippet" => {
                match snippet_name(after) {
                    Some(name) if self.nesting == 0 && self.active_snippet.is_none() => {
                        self.active_snippet = Some(ActiveSnippet {
                            name: name.to_string(),
                            parent: std::mem::take(&mut self.complexity),
                            parent_nesting: self.nesting,
                            start: open,
                            inner_depth: 0,
                        });
                        self.nesting = 0;
                    }
                    _ => {
                        if let Some(active) = self.active_snippet.as_mut() {
                            active.inner_depth = active.inner_depth.saturating_add(1);
                        }
                        self.nesting = self.nesting.saturating_add(1);
                    }
                }
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

/// Parse the snippet identifier from a `{#snippet name(params)}` body
/// remainder: the text before the first `(`, trimmed. Returns `None` when no
/// plausible identifier is present, in which case the snippet is folded into
/// the enclosing unit rather than emitting a `<snippet:>` unit with an empty
/// or garbage name.
fn snippet_name(after: &str) -> Option<&str> {
    let trimmed = after.trim();
    // '(' is ASCII, so byte-slicing at its index is char-boundary safe.
    let name = trimmed[..trimmed.find('(').unwrap_or(trimmed.len())].trim_end();
    let mut chars = name.chars();
    let first = chars.next()?;
    if first != '_' && first != '$' && !first.is_ascii_alphabetic() {
        return None;
    }
    chars
        .all(|c| c == '_' || c == '$' || c.is_ascii_alphanumeric())
        .then_some(name)
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
    use fallow_types::extract::{ComplexityContributionKind, ComplexityMetric, FunctionComplexity};

    /// Convenience for tests expecting at most the `<template>` unit: scans
    /// and returns it, asserting no snippet units were emitted.
    fn single_unit(source: &str) -> Option<FunctionComplexity> {
        let mut units = compute_svelte_template_complexity(source);
        assert!(
            units.len() <= 1,
            "expected at most the <template> unit: {units:#?}"
        );
        units.pop()
    }

    #[test]
    fn each_in_if_with_else_if_counts() {
        let complexity = single_unit(
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
        let complexity = single_unit(
            "{#if a}<p>1</p>{:else if b}<p>2</p>{:else if c}<p>3</p>{:else}<p>4</p>{/if}",
        )
        .expect("template should have complexity");
        // #if + two :else if = 3 branches on top of baseline 1.
        assert_eq!(complexity.cyclomatic, 4, "{complexity:?}");
    }

    #[test]
    fn bare_else_is_continuation_not_a_branch() {
        let complexity = single_unit("{#if a}<p>1</p>{:else}<p>2</p>{/if}")
            .expect("template should have complexity");
        assert_eq!(complexity.cyclomatic, 2, "{complexity:?}");
        assert!(complexity.cognitive >= 2, "{complexity:?}");
    }

    #[test]
    fn await_then_catch_each_count() {
        let complexity = single_unit(
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
            let complexity =
                single_unit(source).expect("shorthand await block should have complexity");
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
        let complexity = single_unit(
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
            let complexity =
                single_unit(source).expect("regex await expression should have complexity");
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
            let complexity = single_unit(source)
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
        let complexity =
            single_unit(source).expect("valid regex expression should preserve await complexity");

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
        let complexity =
            single_unit(source).expect("valid regex condition should preserve template complexity");

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
        let complexity = single_unit(
            "{#await load()}<p>loading</p>{:then}<p>done</p>{:catch}<p>failed</p>{/await}",
        )
        .expect("bindingless continuations should have complexity");

        assert_eq!(complexity.cyclomatic, 4, "{complexity:?}");
        assert_eq!(complexity.cognitive, 3, "{complexity:?}");
    }

    #[test]
    fn key_block_counts() {
        let complexity = single_unit("{#key selectedId}<Child />{/key}")
            .expect("template should have complexity");
        assert!(complexity.cyclomatic >= 2, "{complexity:?}");
    }

    #[test]
    fn interpolation_expressions_contribute() {
        let complexity = single_unit("<p>{enabled && draft ? 'Draft' : 'New'}</p>")
            .expect("template should have complexity");
        assert!(complexity.cyclomatic >= 3, "{complexity:?}");
    }

    #[test]
    fn markup_only_template_has_no_synthetic_complexity() {
        assert!(single_unit(r#"<div class="x"><p>Hello world</p></div>"#).is_none());
    }

    #[test]
    fn script_control_flow_is_not_counted() {
        assert!(
            single_unit(
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
        assert!(single_unit("{#if a && ").is_none());
        // Logical with no RHS inside an interpolation.
        assert!(single_unit("<p>{a && }</p>").is_none());
        // Shorthand await expression with no logical RHS.
        assert!(single_unit("{#await a && then value}{/await}").is_none());
        // Unterminated curly.
        assert!(single_unit("<p>{ a && b").is_none());
    }

    #[test]
    fn multibyte_text_does_not_panic() {
        let complexity = single_unit("{#if a && b}\u{4f4f}\u{6240}<p>{c?.d}</p>{/if}")
            .expect("template should have complexity");
        assert!(complexity.cyclomatic >= 2, "{complexity:?}");
    }

    #[test]
    fn comments_are_masked() {
        assert!(single_unit("<!-- {#if a && b && c} --><p>plain</p>").is_none());
    }

    #[test]
    fn at_const_rhs_contributes() {
        let complexity =
            single_unit("{#each items as item}{@const ok = item?.a && item?.b}<p>{ok}</p>{/each}")
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
        let class_bind = single_unit(r#"<div class={a && b ? "x" : (c || d ? "y" : "z")}>t</div>"#)
            .expect("an attribute binding with logic has complexity");
        assert!(
            class_bind.cyclomatic >= 4,
            "class={{ternary+logical}} should score: {class_bind:?}"
        );
        // Event handler and class: directive bindings are also scored.
        let event = single_unit("<button onclick={() => a && b && go()}>x</button>")
            .expect("event handler with logic has complexity");
        assert!(
            event.cyclomatic >= 2,
            "onclick logic should score: {event:?}"
        );
        // A `>` inside a quoted attribute value must not end the tag early; a
        // plain shorthand carries no complexity and stays dropped.
        assert!(
            single_unit(r#"<a title="a > b" href={url}>x</a>"#).is_none(),
            "a quote-enclosed > plus a plain binding has no logic and is dropped"
        );
    }

    /// The issue-2227 row body used across the three-variant parity tests.
    const ROW_BODY: &str = "\
  {#if row.big}
    {#each row.cells as cell}
      <span>{cell.length > 3 ? 'wide' : 'thin'}</span>
    {/each}
  {:else}
    <span>-</span>
  {/if}
";

    fn find_unit<'a>(units: &'a [FunctionComplexity], name: &str) -> &'a FunctionComplexity {
        units
            .iter()
            .find(|unit| unit.name == name)
            .unwrap_or_else(|| panic!("missing unit {name}: {units:#?}"))
    }

    /// A top-level `{#snippet}` becomes its own unit whose metrics equal the
    /// file-split equivalent: the parent template scores as if the body lived
    /// in another file, and the snippet body scores with nesting rebased to
    /// zero (no snippet-frame surcharge).
    #[test]
    fn top_level_snippet_matches_the_file_split_arithmetic() {
        let snippet_variant = format!(
            "{{#snippet rowBody(row)}}\n{ROW_BODY}{{/snippet}}\n{{#each rows as row}}\n  {{@render rowBody(row)}}\n{{/each}}\n"
        );
        let split_outer = "{#each rows as row}\n  <Body row={row} />\n{/each}\n";

        let units = compute_svelte_template_complexity(&snippet_variant);
        assert_eq!(units.len(), 2, "{units:#?}");
        let template = find_unit(&units, "<template>");
        let snippet = find_unit(&units, "<snippet:rowBody>");

        let outer = single_unit(split_outer).expect("outer split template has complexity");
        assert_eq!(template.cyclomatic, outer.cyclomatic, "{units:#?}");
        assert_eq!(template.cognitive, outer.cognitive, "{units:#?}");

        let body = single_unit(ROW_BODY).expect("row body has complexity");
        assert_eq!(snippet.cyclomatic, body.cyclomatic, "{units:#?}");
        assert_eq!(
            snippet.cognitive, body.cognitive,
            "the snippet frame must add no nesting surcharge: {units:#?}"
        );

        // The monolithic variant (body inlined under the `{#each}`) pays the
        // nesting surcharge the snippet extraction removes.
        let mono = format!("{{#each rows as row}}\n{ROW_BODY}{{/each}}\n");
        let mono_unit = single_unit(&mono).expect("monolithic template has complexity");
        assert_eq!(
            mono_unit.cyclomatic,
            outer.cyclomatic + body.cyclomatic - 1,
            "cyclomatic is flat: mono equals the two units minus one shared baseline"
        );
        assert!(
            mono_unit.cognitive > outer.cognitive + body.cognitive,
            "inlining must cost nesting weight: {mono_unit:#?}"
        );
    }

    /// A snippet declared as a component-tag child sits at logic-block
    /// nesting 0 (markup elements push no nesting), so it still becomes its
    /// own unit. This is the dominant Svelte 5 idiom.
    #[test]
    fn snippet_as_component_child_is_its_own_unit() {
        let source = "\
<Table rows={rows}>
  {#snippet row(item)}
    {#if item.active}
      <td>{item.value > 3 ? 'high' : 'low'}</td>
    {/if}
  {/snippet}
</Table>
";
        let units = compute_svelte_template_complexity(source);
        let snippet = find_unit(&units, "<snippet:row>");
        assert!(snippet.cyclomatic >= 3, "{units:#?}");
        assert!(
            units.iter().all(|unit| unit.name != "<template>"),
            "the parent has no remaining non-trivial complexity: {units:#?}"
        );
    }

    #[test]
    fn snippet_line_count_covers_the_block_span_and_anchors_in_the_body() {
        let source = "\
<p>{top && bottom}</p>
{#snippet rowBody(row)}
  {#if row.big}
    <b>{row.name}</b>
  {/if}
{/snippet}
";
        let units = compute_svelte_template_complexity(source);
        let snippet = find_unit(&units, "<snippet:rowBody>");
        assert_eq!(
            snippet.line_count, 5,
            "{{#snippet}}..{{/snippet}} spans 5 lines"
        );
        assert_eq!(
            snippet.line, 3,
            "anchored at the first construct inside the body"
        );
        let template = find_unit(&units, "<template>");
        assert_eq!(template.line, 1, "{units:#?}");
        assert!(
            snippet
                .contributions
                .iter()
                .all(|contribution| (3..=5).contains(&contribution.line)),
            "contribution anchors must land inside the body: {snippet:#?}"
        );
    }

    #[test]
    fn nested_snippet_stays_folded_into_the_enclosing_unit() {
        // Inner snippet inside a top-level snippet: folded into the outer
        // snippet unit with the pre-existing nesting surcharge.
        let source = "\
{#snippet outer(row)}
  {#snippet inner(cell)}
    {#if cell.wide}<span>{cell.v}</span>{/if}
  {/snippet}
  {#if row.big}{@render inner(row.cell)}{/if}
{/snippet}
";
        let units = compute_svelte_template_complexity(source);
        assert_eq!(units.len(), 1, "{units:#?}");
        let outer = find_unit(&units, "<snippet:outer>");
        assert!(outer.cyclomatic >= 3, "{units:#?}");

        // Snippet inside a logic block: folded into the template unit.
        let inside_if = "\
{#if ready}
  {#snippet row(item)}
    {#if item.active}<td>{item.v}</td>{/if}
  {/snippet}
  {@render row(current)}
{/if}
";
        let units = compute_svelte_template_complexity(inside_if);
        assert_eq!(units.len(), 1, "{units:#?}");
        assert_eq!(units[0].name, "<template>");
        assert!(units[0].cyclomatic >= 3, "{units:#?}");
    }

    #[test]
    fn trivial_snippet_body_emits_no_unit() {
        let source = "\
{#snippet label()}
  <p>static</p>
{/snippet}
<p>{a && b}</p>
";
        let units = compute_svelte_template_complexity(source);
        assert_eq!(units.len(), 1, "{units:#?}");
        assert_eq!(units[0].name, "<template>");
    }

    #[test]
    fn unbalanced_snippet_drops_the_whole_template() {
        assert!(
            compute_svelte_template_complexity(
                "{#snippet rowBody(row)}\n{#if row.big}<b>x</b>{/if}\n"
            )
            .is_empty(),
            "an unclosed {{#snippet}} must keep the all-or-nothing drop"
        );
    }

    #[test]
    fn snippet_close_is_matched_by_name_across_an_unclosed_inner_block() {
        // The `{#if}` inside the body is never closed; name matching still
        // finds the `{/snippet}` boundary and the unit survives.
        let source = "\
{#snippet rowBody(row)}
  {#if row.big}
    <b>{row.name}</b>
{/snippet}
<p>{a && b}</p>
";
        let units = compute_svelte_template_complexity(source);
        assert!(
            units.iter().any(|unit| unit.name == "<snippet:rowBody>"),
            "{units:#?}"
        );
    }

    #[test]
    fn snippet_params_with_braces_and_defaults_do_not_break_the_boundary() {
        let source = "\
{#snippet cell(props = { pad: 1 }, flag = x && y)}
  {#if props.pad > 0}<td>{flag}</td>{/if}
{/snippet}
";
        let units = compute_svelte_template_complexity(source);
        let snippet = find_unit(&units, "<snippet:cell>");
        assert_eq!(
            snippet.cyclomatic, 2,
            "the body's if is scored; the parameter defaults are signature, not body: {units:#?}"
        );
    }

    #[test]
    fn unnameable_snippet_falls_back_to_folding() {
        for open in ["{#snippet}", "{#snippet 123bad()}"] {
            let source = format!("{open}\n  {{#if a && b}}<p>x</p>{{/if}}\n{{/snippet}}\n");
            let units = compute_svelte_template_complexity(&source);
            assert_eq!(units.len(), 1, "{open}: {units:#?}");
            assert_eq!(units[0].name, "<template>", "{open}: {units:#?}");
        }
    }

    #[test]
    fn duplicate_snippet_names_emit_one_unit_each() {
        let source = "\
{#snippet row(item)}
  {#if item.a}<td>1</td>{/if}
{/snippet}
{#snippet row(item)}
  {#if item.b}<td>2</td>{/if}
{/snippet}
";
        let units = compute_svelte_template_complexity(source);
        assert_eq!(
            units
                .iter()
                .filter(|unit| unit.name == "<snippet:row>")
                .count(),
            2,
            "{units:#?}"
        );
    }

    #[test]
    fn multibyte_snippet_content_does_not_panic() {
        let source = "\
{#snippet row(item)}
  {#if item.a}\u{4f4f}\u{6240}<td>{item.v?.w}</td>{/if}
{/snippet}
";
        let units = compute_svelte_template_complexity(source);
        let snippet = find_unit(&units, "<snippet:row>");
        assert!(snippet.cyclomatic >= 2, "{units:#?}");
    }

    #[test]
    fn render_expression_is_scored_in_the_parent() {
        let source = "\
{#snippet row(item)}
  {#if item.a}<td>x</td>{/if}
{/snippet}
{@render (compact ? row : row)(current)}
";
        let units = compute_svelte_template_complexity(source);
        let template = find_unit(&units, "<template>");
        assert!(
            template.cyclomatic >= 2,
            "the ternary in the render expression belongs to the parent: {units:#?}"
        );
    }

    #[test]
    fn svelte_files_without_snippets_are_unchanged() {
        let source = "{#if a}<p>1</p>{:else if b}<p>2</p>{:else}<p>3</p>{/if}";
        let units = compute_svelte_template_complexity(source);
        assert_eq!(units.len(), 1);
        assert_eq!(units[0].name, "<template>");
        assert_eq!(units[0].cyclomatic, 3, "{units:#?}");
    }
}
