//! Framework-agnostic JavaScript-expression complexity engine shared by the
//! Angular, Vue, and Svelte template scanners.
//!
//! The engine scores a bound JS expression (a `v-if` condition, a Svelte
//! `{#if}` condition, an Angular `@if` condition, a `{{ }}` / `{ }`
//! interpolation) by logical-operator count, ternary branches, and optional
//! chaining. It is intentionally identical across frameworks: a Vue
//! `v-if="a?.b && c"`, a Svelte `{#if a?.b && c}`, and an Angular
//! `@if (a?.b && c)` all yield the same metrics. The outer per-framework
//! scanners (sibling modules) own only the control-flow tokenization and feed
//! their bound expressions through [`TemplateComplexity::add_expression`].

/// Internal scanner error. Carries no data: any malformed-template path
/// just falls through and the caller drops the synthetic finding.
#[derive(Debug, Clone, Copy)]
pub(super) struct ScanError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LogicalOperator {
    And,
    Or,
    Nullish,
}

/// Accumulated synthetic `<template>` complexity. `cyclomatic` starts at 1 (the
/// implicit straight-line path); the caller drops the entry when it never rises
/// above the trivial `cyclomatic == 1 && cognitive == 0` baseline.
#[derive(Debug)]
pub(super) struct TemplateComplexity {
    pub(super) cyclomatic: u16,
    pub(super) cognitive: u16,
    pub(super) first_offset: Option<usize>,
}

impl Default for TemplateComplexity {
    fn default() -> Self {
        Self {
            cyclomatic: 1,
            cognitive: 0,
            first_offset: None,
        }
    }
}

impl TemplateComplexity {
    /// Score one bound JS expression and fold its metrics in. `offset` is the
    /// byte offset of `source` within the original template, used to anchor the
    /// synthetic finding at the first non-trivial expression.
    pub(super) fn add_expression(
        &mut self,
        source: &str,
        offset: usize,
        nesting: u16,
    ) -> Result<(), ScanError> {
        let Some(trim_start) = source.find(|c: char| !c.is_whitespace()) else {
            return Ok(());
        };
        self.first_offset.get_or_insert(offset + trim_start);
        let metrics = compute_expression_metrics(&source[trim_start..], nesting, 0)?;
        self.cyclomatic = self.cyclomatic.saturating_add(metrics.cyclomatic);
        self.cognitive = self.cognitive.saturating_add(metrics.cognitive);
        Ok(())
    }

    /// Account for one control-flow construct (an `@if`/`@for`, a `v-if`/`v-for`,
    /// a `{#if}`/`{#each}`): +1 cyclomatic and +1+nesting cognitive (the cognitive
    /// nesting penalty mirrors Sonar's nesting model).
    pub(super) fn add_control_flow(&mut self, nesting: u16) {
        self.cyclomatic = self.cyclomatic.saturating_add(1);
        self.cognitive = self.cognitive.saturating_add(1 + nesting);
    }
}

pub(super) fn find_tag_end(source: &str, tag_start: usize) -> Result<usize, ScanError> {
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

pub(super) fn read_attribute_value(
    source: &str,
    offset: usize,
) -> Result<(usize, usize, usize), ScanError> {
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

#[derive(Clone, Copy, Default)]
struct ExpressionMetrics {
    cyclomatic: u16,
    cognitive: u16,
}

impl ExpressionMetrics {
    fn add(&mut self, other: Self) {
        self.cyclomatic = self.cyclomatic.saturating_add(other.cyclomatic);
        self.cognitive = self.cognitive.saturating_add(other.cognitive);
    }
}

/// Maximum bracket/ternary recursion depth for template-expression metric
/// scoring. Real template expressions nest only 3-5 levels deep, so this cap is
/// generous; past it a pathological input like `((((...))))` is treated as
/// malformed and its synthetic finding is dropped (via [`ScanError`]) rather
/// than recursing until the stack overflows (SIGABRT under release
/// `panic = "abort"`). Mirrors the `MAX_TAINT_BINDING_HOPS` /
/// `MAX_BINDING_PATH_DEPTH` bounded-work style. Issue #1843 follow-up.
const MAX_TEMPLATE_EXPR_DEPTH: u16 = 64;

fn compute_expression_metrics(
    source: &str,
    nesting: u16,
    depth: u16,
) -> Result<ExpressionMetrics, ScanError> {
    if depth > MAX_TEMPLATE_EXPR_DEPTH {
        return Err(ScanError);
    }
    let source = source.trim();
    if source.is_empty() {
        return Ok(ExpressionMetrics::default());
    }
    if let Some((question, colon)) = find_top_level_ternary(source)? {
        let mut metrics = ExpressionMetrics::default();
        metrics.add(compute_expression_metrics(
            &source[..question],
            nesting,
            depth + 1,
        )?);
        metrics.cyclomatic = metrics.cyclomatic.saturating_add(1);
        metrics.cognitive = metrics.cognitive.saturating_add(1 + nesting);
        metrics.add(compute_expression_metrics(
            &source[question + 1..colon],
            nesting.saturating_add(1),
            depth + 1,
        )?);
        metrics.add(compute_expression_metrics(
            &source[colon + 1..],
            nesting.saturating_add(1),
            depth + 1,
        )?);
        return Ok(metrics);
    }
    scan_expression_without_ternary(source, nesting, depth)
}

/// Mutable scanning state shared across the [`scan_expression_without_ternary`]
/// match arms.
struct ScanState {
    metrics: ExpressionMetrics,
    last_logical_operator: Option<LogicalOperator>,
    needs_rhs: bool,
}

fn scan_expression_without_ternary(
    source: &str,
    nesting: u16,
    depth: u16,
) -> Result<ExpressionMetrics, ScanError> {
    let mut state = ScanState {
        metrics: ExpressionMetrics::default(),
        last_logical_operator: None,
        needs_rhs: false,
    };
    let mut offset = 0;

    while offset < source.len() {
        match source.as_bytes()[offset] {
            byte if byte.is_ascii_whitespace() => offset += 1,
            b'\'' | b'"' | b'`' => {
                offset = skip_quoted(source, offset)?;
                state.needs_rhs = false;
            }
            b'(' | b'[' | b'{' => {
                offset = scan_bracket_group(source, offset, nesting, depth, &mut state)?;
            }
            b')' | b']' | b'}' => return Err(ScanError),
            _ if source[offset..].starts_with("?.") => {
                state.metrics.cyclomatic = state.metrics.cyclomatic.saturating_add(1);
                offset += 2;
            }
            _ if source[offset..].starts_with("&&=")
                || source[offset..].starts_with("||=")
                || source[offset..].starts_with("??=") =>
            {
                state.metrics.cyclomatic = state.metrics.cyclomatic.saturating_add(1);
                state.last_logical_operator = None;
                state.needs_rhs = true;
                offset += 3;
            }
            _ if source[offset..].starts_with("&&")
                || source[offset..].starts_with("||")
                || source[offset..].starts_with("??") =>
            {
                offset = scan_logical_operator(source, offset, &mut state)?;
            }
            b',' | b';' => {
                if state.needs_rhs {
                    return Err(ScanError);
                }
                state.last_logical_operator = None;
                offset += 1;
            }
            _ => {
                state.needs_rhs = false;
                offset += source[offset..].chars().next().map_or(1, char::len_utf8);
            }
        }
    }

    if state.needs_rhs {
        Err(ScanError)
    } else {
        Ok(state.metrics)
    }
}

/// Recurse into a bracketed sub-expression `( [ {` at `offset`, folding its
/// metrics into `state` and returning the offset just past the closing bracket.
fn scan_bracket_group(
    source: &str,
    offset: usize,
    nesting: u16,
    depth: u16,
    state: &mut ScanState,
) -> Result<usize, ScanError> {
    let close = matching_close_byte(source.as_bytes()[offset]).ok_or(ScanError)?;
    let end = find_matching_delimiter(source, offset, source.as_bytes()[offset], close)?;
    state.metrics.add(compute_expression_metrics(
        &source[offset + 1..end],
        nesting,
        depth + 1,
    )?);
    state.last_logical_operator = None;
    state.needs_rhs = false;
    Ok(end + 1)
}

/// Score a 2-char logical operator (`&& || ??`) at `offset`, updating cyclomatic
/// / cognitive counts and the logical-operator run state, and return the offset
/// past the operator.
fn scan_logical_operator(
    source: &str,
    offset: usize,
    state: &mut ScanState,
) -> Result<usize, ScanError> {
    if state.needs_rhs {
        return Err(ScanError);
    }
    let operator = if source[offset..].starts_with("&&") {
        LogicalOperator::And
    } else if source[offset..].starts_with("||") {
        LogicalOperator::Or
    } else {
        LogicalOperator::Nullish
    };
    state.metrics.cyclomatic = state.metrics.cyclomatic.saturating_add(1);
    if state.last_logical_operator != Some(operator) {
        state.metrics.cognitive = state.metrics.cognitive.saturating_add(1);
        state.last_logical_operator = Some(operator);
    }
    state.needs_rhs = true;
    Ok(offset + 2)
}

fn find_top_level_ternary(source: &str) -> Result<Option<(usize, usize)>, ScanError> {
    let mut offset = 0;
    let mut depth = 0_u16;
    let mut nested_ternaries = 0_u16;
    let mut question = None;

    while offset < source.len() {
        match source.as_bytes()[offset] {
            b'\'' | b'"' | b'`' => offset = skip_quoted(source, offset)?,
            b'(' | b'[' | b'{' => {
                depth = depth.saturating_add(1);
                offset += 1;
            }
            b')' | b']' | b'}' => {
                if depth == 0 {
                    return Err(ScanError);
                }
                depth -= 1;
                offset += 1;
            }
            b'?' if source[offset..].starts_with("??") || source[offset..].starts_with("?.") => {
                offset += 2;
            }
            b'?' if depth == 0 => {
                if question.is_none() {
                    question = Some(offset);
                } else {
                    nested_ternaries = nested_ternaries.saturating_add(1);
                }
                offset += 1;
            }
            b':' if depth == 0 && question.is_some() => {
                if nested_ternaries == 0 {
                    if let Some(question) = question {
                        return Ok(Some((question, offset)));
                    }
                    return Err(ScanError);
                }
                nested_ternaries -= 1;
                offset += 1;
            }
            _ => offset += source[offset..].chars().next().map_or(1, char::len_utf8),
        }
    }

    if question.is_some() || depth != 0 {
        Err(ScanError)
    } else {
        Ok(None)
    }
}

pub(super) fn find_matching_delimiter(
    source: &str,
    open_offset: usize,
    open: u8,
    close: u8,
) -> Result<usize, ScanError> {
    let mut offset = open_offset + 1;
    let mut depth = 1_u16;
    while offset < source.len() {
        match source.as_bytes()[offset] {
            b'\'' | b'"' | b'`' => offset = skip_quoted(source, offset)?,
            byte if byte == open => {
                depth = depth.saturating_add(1);
                offset += 1;
            }
            byte if byte == close => {
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

fn matching_close_byte(open: u8) -> Option<u8> {
    match open {
        b'(' => Some(b')'),
        b'[' => Some(b']'),
        b'{' => Some(b'}'),
        _ => None,
    }
}

pub(super) fn skip_quoted(source: &str, quote_offset: usize) -> Result<usize, ScanError> {
    let quote = source.as_bytes()[quote_offset];
    let mut offset = quote_offset + 1;
    while offset < source.len() {
        match source.as_bytes()[offset] {
            // Advance past the backslash, then one full char: a fixed +2 byte
            // advance can land mid-character when the escapee is multi-byte.
            b'\\' => {
                offset += 1;
                if offset < source.len() {
                    offset += source[offset..].chars().next().map_or(0, char::len_utf8);
                }
            }
            byte if byte == quote => return Ok(offset + 1),
            _ => offset += source[offset..].chars().next().map_or(1, char::len_utf8),
        }
    }
    Err(ScanError)
}

pub(super) fn skip_whitespace(source: &str, mut offset: usize) -> usize {
    while offset < source.len() && source.as_bytes()[offset].is_ascii_whitespace() {
        offset += 1;
    }
    offset
}

pub(super) fn read_identifier(source: &str, offset: usize) -> Option<(&str, usize)> {
    if offset >= source.len() || !is_identifier_start(source.as_bytes()[offset]) {
        return None;
    }
    let mut end = offset + 1;
    while end < source.len() && is_identifier_continue(source.as_bytes()[end]) {
        end += 1;
    }
    Some((&source[offset..end], end))
}

pub(super) fn is_identifier_before(source: &str, offset: usize) -> bool {
    offset > 0 && is_identifier_continue(source.as_bytes()[offset - 1])
}

pub(super) fn is_identifier_after(source: &str, offset: usize) -> bool {
    offset < source.len() && is_identifier_continue(source.as_bytes()[offset])
}

fn is_identifier_start(byte: u8) -> bool {
    byte == b'_' || byte == b'$' || byte.is_ascii_alphabetic()
}

fn is_identifier_continue(byte: u8) -> bool {
    is_identifier_start(byte) || byte.is_ascii_digit()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shallow_expression_metrics_are_stable() {
        // A normal 2-3 level nested expression scores by logical-operator and
        // ternary count; the depth guard never fires for it.
        let ternary = compute_expression_metrics("(a && b) ? c : (d || e)", 0, 0).unwrap();
        assert_eq!(ternary.cyclomatic, 3);
        assert_eq!(ternary.cognitive, 3);

        let bracketed = compute_expression_metrics("(a && b)", 0, 0).unwrap();
        assert_eq!(bracketed.cyclomatic, 1);
        assert_eq!(bracketed.cognitive, 1);
    }

    #[test]
    fn moderate_nesting_below_cap_scores_identically() {
        // Ten bracket levels is far below MAX_TEMPLATE_EXPR_DEPTH, so wrapping
        // `a && b` in redundant parens yields the same metrics as the bare
        // expression.
        let source = format!("{}a && b{}", "(".repeat(10), ")".repeat(10));
        let metrics = compute_expression_metrics(&source, 0, 0).unwrap();
        assert_eq!(metrics.cyclomatic, 1);
        assert_eq!(metrics.cognitive, 1);
    }

    #[test]
    fn pathologically_deep_nesting_is_dropped_without_crashing() {
        // ~5000 nested parens previously recursed until the stack overflowed
        // (SIGABRT under release panic = "abort"). The depth guard now bails
        // past MAX_TEMPLATE_EXPR_DEPTH and the synthetic finding is dropped.
        let depth = 5000;
        let source = format!("{}a{}", "(".repeat(depth), ")".repeat(depth));
        assert!(compute_expression_metrics(&source, 0, 0).is_err());

        // The public entry point surfaces the same drop as a ScanError.
        let mut complexity = TemplateComplexity::default();
        assert!(complexity.add_expression(&source, 0, 0).is_err());
    }
}
