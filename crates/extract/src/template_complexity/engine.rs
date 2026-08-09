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

use fallow_types::extract::{ComplexityContributionKind, ComplexityMetric};

/// Internal scanner error. Carries no data: any malformed-template path
/// just falls through and the caller drops the synthetic finding.
#[derive(Debug, Clone, Copy)]
pub(super) struct ScanError;

/// Minimal lexical state needed to distinguish a JavaScript regex literal
/// from division. Reserved words only open an operand position when they are
/// actual keywords, not property names such as `value.of`.
#[derive(Debug, Clone, Copy)]
pub(super) struct RegexContext {
    can_start: bool,
    after_property_access: bool,
}

impl RegexContext {
    pub(super) const fn expression_start() -> Self {
        Self {
            can_start: true,
            after_property_access: false,
        }
    }

    pub(super) const fn can_start(self) -> bool {
        self.can_start
    }

    pub(super) fn after_value(&mut self) {
        self.can_start = false;
        self.after_property_access = false;
    }

    pub(super) fn after_operator(&mut self) {
        self.can_start = true;
        self.after_property_access = false;
    }

    pub(super) fn after_property_access(&mut self) {
        self.can_start = false;
        self.after_property_access = true;
    }

    pub(super) fn after_identifier(&mut self, identifier: &str) {
        self.can_start = !self.after_property_access && is_regex_prefix_keyword(identifier);
        self.after_property_access = false;
    }

    pub(super) fn after_character(&mut self, character: char) {
        if character.is_alphanumeric() {
            self.after_value();
        } else {
            self.after_operator();
        }
    }

    fn skip_markup(&mut self, slash: usize) -> usize {
        self.after_operator();
        slash + 1
    }
}

fn is_regex_prefix_keyword(identifier: &str) -> bool {
    matches!(
        identifier,
        "await"
            | "case"
            | "delete"
            | "do"
            | "else"
            | "extends"
            | "in"
            | "instanceof"
            | "new"
            | "return"
            | "throw"
            | "typeof"
            | "void"
            | "yield"
    )
}

/// Accumulator state captured before a fallible expression scan, so a scan that
/// fails partway can be undone.
#[derive(Debug, Clone, Copy)]
struct Checkpoint {
    cyclomatic: u16,
    cognitive: u16,
    first_offset: Option<usize>,
    contributions: usize,
}

/// One recorded increment, still anchored at a byte offset into the template
/// source. [`super::build_template_complexity`] resolves the offset to
/// line/column once, against the original (unmasked) file.
#[derive(Debug, Clone, Copy)]
pub(super) struct RawContribution {
    pub(super) offset: usize,
    pub(super) metric: ComplexityMetric,
    pub(super) kind: ComplexityContributionKind,
    pub(super) weight: u16,
    pub(super) nesting: u16,
}

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
    pub(super) contributions: Vec<RawContribution>,
}

impl Default for TemplateComplexity {
    fn default() -> Self {
        Self {
            cyclomatic: 1,
            cognitive: 0,
            first_offset: None,
            contributions: Vec::new(),
        }
    }
}

impl TemplateComplexity {
    /// Record a cyclomatic `+1` at `offset`. Every cyclomatic increment in this
    /// module goes through here so the breakdown can never drift from the
    /// aggregate, mirroring the discipline in `crate::complexity`.
    pub(super) fn inc_cyclomatic(&mut self, offset: usize, kind: ComplexityContributionKind) {
        self.contributions.push(RawContribution {
            offset,
            metric: ComplexityMetric::Cyclomatic,
            kind,
            weight: 1,
            nesting: 0,
        });
        self.cyclomatic = self.cyclomatic.saturating_add(1);
    }

    /// Record a cognitive increment of `1 + nesting` at `offset`.
    pub(super) fn inc_cognitive(
        &mut self,
        offset: usize,
        kind: ComplexityContributionKind,
        nesting: u16,
    ) {
        let weight = 1_u16.saturating_add(nesting);
        self.contributions.push(RawContribution {
            offset,
            metric: ComplexityMetric::Cognitive,
            kind,
            weight,
            nesting,
        });
        self.cognitive = self.cognitive.saturating_add(weight);
    }

    /// Record a cognitive `+1` at `offset` with no nesting penalty (an `else` /
    /// `v-else` continuation, an `else if` cascade).
    pub(super) fn inc_cognitive_flat(&mut self, offset: usize, kind: ComplexityContributionKind) {
        self.inc_cognitive(offset, kind, 0);
    }

    /// Score one bound JS expression and fold its metrics in. `offset` is the
    /// byte offset of `source` within the original template, used to anchor the
    /// synthetic finding at the first non-trivial expression and every
    /// contribution recorded inside the expression.
    ///
    /// All-or-nothing: the scan records increments as it walks, so a malformed
    /// expression is rolled back before returning. Most callers propagate the
    /// error and drop the whole template, but the Astro scanner deliberately
    /// swallows it (a benign non-boolean markup expression need not tokenize as
    /// one), and half-scored metrics there would invent a synthetic finding for
    /// a template that has none.
    pub(super) fn add_expression(
        &mut self,
        source: &str,
        offset: usize,
        nesting: u16,
    ) -> Result<(), ScanError> {
        let Some(trim_start) = source.find(|c: char| !c.is_whitespace()) else {
            return Ok(());
        };
        let checkpoint = Checkpoint {
            cyclomatic: self.cyclomatic,
            cognitive: self.cognitive,
            first_offset: self.first_offset,
            contributions: self.contributions.len(),
        };
        self.first_offset.get_or_insert(offset + trim_start);
        let result = self.scan_expression(&source[trim_start..], offset + trim_start, nesting, 0);
        if result.is_err() {
            self.cyclomatic = checkpoint.cyclomatic;
            self.cognitive = checkpoint.cognitive;
            self.first_offset = checkpoint.first_offset;
            self.contributions.truncate(checkpoint.contributions);
        }
        result
    }

    /// Account for one control-flow construct (an `@if`/`@for`, a `v-if`/`v-for`,
    /// a `{#if}`/`{#each}`): +1 cyclomatic and +1+nesting cognitive (the cognitive
    /// nesting penalty mirrors Sonar's nesting model). `offset` anchors both
    /// increments at the directive or block keyword that introduced them.
    pub(super) fn add_control_flow(
        &mut self,
        offset: usize,
        kind: ComplexityContributionKind,
        nesting: u16,
    ) {
        self.inc_cyclomatic(offset, kind);
        self.inc_cognitive(offset, kind, nesting);
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

/// Maximum bracket/ternary recursion depth for template-expression metric
/// scoring. Real template expressions nest only 3-5 levels deep, so this cap is
/// generous; past it a pathological input like `((((...))))` is treated as
/// malformed and its synthetic finding is dropped (via [`ScanError`]) rather
/// than recursing until the stack overflows (SIGABRT under release
/// `panic = "abort"`). Mirrors the `MAX_TAINT_BINDING_HOPS` /
/// `MAX_BINDING_PATH_DEPTH` bounded-work style. Issue #1843 follow-up.
const MAX_TEMPLATE_EXPR_DEPTH: u16 = 64;

impl TemplateComplexity {
    /// Score a JS expression, recording each increment at its absolute offset.
    /// `base` is the byte offset of `source` within the original template, so a
    /// recursive call into a sub-slice must advance it by the slice's start.
    fn scan_expression(
        &mut self,
        source: &str,
        base: usize,
        nesting: u16,
        depth: u16,
    ) -> Result<(), ScanError> {
        if depth > MAX_TEMPLATE_EXPR_DEPTH {
            return Err(ScanError);
        }
        let leading = source.len() - source.trim_start().len();
        let base = base + leading;
        let source = source.trim();
        if source.is_empty() {
            return Ok(());
        }
        if let Some((question, colon)) = find_top_level_ternary(source)? {
            self.scan_expression(&source[..question], base, nesting, depth + 1)?;
            self.inc_cyclomatic(base + question, ComplexityContributionKind::Ternary);
            self.inc_cognitive(
                base + question,
                ComplexityContributionKind::Ternary,
                nesting,
            );
            self.scan_expression(
                &source[question + 1..colon],
                base + question + 1,
                nesting.saturating_add(1),
                depth + 1,
            )?;
            self.scan_expression(
                &source[colon + 1..],
                base + colon + 1,
                nesting.saturating_add(1),
                depth + 1,
            )?;
            return Ok(());
        }
        self.scan_expression_without_ternary(ExprScope {
            source,
            base,
            nesting,
            depth,
        })
    }
}

/// Mutable scanning state shared across the `scan_expression_without_ternary`
/// match arms. The metric counters live on [`TemplateComplexity`]; what remains
/// here is the operator-run bookkeeping that decides whether the NEXT logical
/// operator earns a cognitive increment.
struct ScanState {
    last_logical_operator: Option<LogicalOperator>,
    needs_rhs: bool,
    regex: RegexContext,
}

impl ScanState {
    fn new() -> Self {
        Self {
            last_logical_operator: None,
            needs_rhs: false,
            regex: RegexContext::expression_start(),
        }
    }
}

/// The expression slice currently being scanned, with everything needed to
/// place a contribution: `base` is the slice's byte offset in the template,
/// `nesting` the cognitive nesting penalty in force, `depth` the recursion
/// guard.
#[derive(Clone, Copy)]
struct ExprScope<'a> {
    source: &'a str,
    base: usize,
    nesting: u16,
    depth: u16,
}

impl TemplateComplexity {
    fn scan_expression_without_ternary(&mut self, scope: ExprScope<'_>) -> Result<(), ScanError> {
        let ExprScope { source, base, .. } = scope;
        let mut state = ScanState::new();
        let mut offset = 0;

        while offset < source.len() {
            match source.as_bytes()[offset] {
                byte if byte.is_ascii_whitespace() => offset += 1,
                b'\'' | b'"' | b'`' => {
                    offset = skip_quoted(source, offset)?;
                    state.needs_rhs = false;
                    state.regex.after_value();
                }
                b'/' if is_tag_close(source, offset) => offset = state.regex.skip_markup(offset),
                b'/' if source.as_bytes().get(offset + 1) == Some(&b'/') => {
                    offset = skip_line_comment(source, offset);
                }
                b'/' if source.as_bytes().get(offset + 1) == Some(&b'*') => {
                    offset = skip_block_comment(source, offset)?;
                }
                b'/' if state.regex.can_start() => {
                    offset = skip_regex_literal(source, offset)?;
                    state.needs_rhs = false;
                    state.regex.after_value();
                }
                b'/' => {
                    offset += usize::from(source.as_bytes().get(offset + 1) == Some(&b'=')) + 1;
                    state.regex.after_operator();
                }
                b'(' | b'[' | b'{' => {
                    offset = self.scan_bracket_group(scope, offset, &mut state)?;
                }
                b')' | b']' | b'}' => return Err(ScanError),
                _ if source[offset..].starts_with("?.") => {
                    self.inc_cyclomatic(base + offset, ComplexityContributionKind::OptionalChain);
                    offset += 2;
                    state.regex.after_property_access();
                }
                _ if source[offset..].starts_with("&&=")
                    || source[offset..].starts_with("||=")
                    || source[offset..].starts_with("??=") =>
                {
                    self.inc_cyclomatic(
                        base + offset,
                        ComplexityContributionKind::LogicalAssignment,
                    );
                    state.last_logical_operator = None;
                    state.needs_rhs = true;
                    state.regex.after_operator();
                    offset += 3;
                }
                _ if source[offset..].starts_with("&&")
                    || source[offset..].starts_with("||")
                    || source[offset..].starts_with("??") =>
                {
                    offset = self.scan_logical_operator(source, base, offset, &mut state)?;
                }
                b',' | b';' => {
                    if state.needs_rhs {
                        return Err(ScanError);
                    }
                    state.last_logical_operator = None;
                    state.regex.after_operator();
                    offset += 1;
                }
                byte if is_identifier_start(byte) => {
                    let (identifier, end) = read_identifier(source, offset).ok_or(ScanError)?;
                    state.needs_rhs = false;
                    state.regex.after_identifier(identifier);
                    offset = end;
                }
                byte if byte.is_ascii_digit() => {
                    offset = skip_number_literal(source, offset);
                    state.needs_rhs = false;
                    state.regex.after_value();
                }
                b'.' if source[offset..].starts_with("...") => {
                    offset += 3;
                    state.regex.after_operator();
                }
                b'.' => {
                    offset += 1;
                    state.regex.after_property_access();
                }
                b'+' | b'-'
                    if source.as_bytes().get(offset + 1) == Some(&source.as_bytes()[offset]) =>
                {
                    offset += 2;
                }
                _ => {
                    state.needs_rhs = false;
                    let character = source[offset..].chars().next().ok_or(ScanError)?;
                    offset += character.len_utf8();
                    state.regex.after_character(character);
                }
            }
        }

        if state.needs_rhs {
            Err(ScanError)
        } else {
            Ok(())
        }
    }

    /// Recurse into a bracketed sub-expression `( [ {` at `offset`, recording its
    /// contributions and returning the offset just past the closing bracket.
    fn scan_bracket_group(
        &mut self,
        scope: ExprScope<'_>,
        offset: usize,
        state: &mut ScanState,
    ) -> Result<usize, ScanError> {
        let ExprScope {
            source,
            base,
            nesting,
            depth,
        } = scope;
        let close = matching_close_byte(source.as_bytes()[offset]).ok_or(ScanError)?;
        let end = find_matching_delimiter(source, offset, source.as_bytes()[offset], close)?;
        self.scan_expression(
            &source[offset + 1..end],
            base + offset + 1,
            nesting,
            depth + 1,
        )?;
        state.last_logical_operator = None;
        state.needs_rhs = false;
        state.regex.after_value();
        Ok(end + 1)
    }

    /// Score a 2-char logical operator (`&& || ??`) at `offset`, updating the
    /// counters and the logical-operator run state, and return the offset past
    /// the operator. A run of the SAME operator earns one cognitive increment
    /// for the run, so only the operator that opens the run records a cognitive
    /// contribution; every operator records a cyclomatic one.
    fn scan_logical_operator(
        &mut self,
        source: &str,
        base: usize,
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
        let kind = match operator {
            LogicalOperator::And => ComplexityContributionKind::LogicalAnd,
            LogicalOperator::Or => ComplexityContributionKind::LogicalOr,
            LogicalOperator::Nullish => ComplexityContributionKind::NullishCoalescing,
        };
        self.inc_cyclomatic(base + offset, kind);
        if state.last_logical_operator != Some(operator) {
            self.inc_cognitive_flat(base + offset, kind);
            state.last_logical_operator = Some(operator);
        }
        state.needs_rhs = true;
        state.regex.after_operator();
        Ok(offset + 2)
    }
}

fn find_top_level_ternary(source: &str) -> Result<Option<(usize, usize)>, ScanError> {
    let mut offset = 0;
    let mut depth = 0_u16;
    let mut nested_ternaries = 0_u16;
    let mut question = None;
    let mut regex = RegexContext::expression_start();

    while offset < source.len() {
        match source.as_bytes()[offset] {
            byte if byte.is_ascii_whitespace() => offset += 1,
            b'\'' | b'"' | b'`' => {
                offset = skip_quoted(source, offset)?;
                regex.after_value();
            }
            b'/' if is_tag_close(source, offset) => offset = regex.skip_markup(offset),
            b'/' if source.as_bytes().get(offset + 1) == Some(&b'/') => {
                offset = skip_line_comment(source, offset);
            }
            b'/' if source.as_bytes().get(offset + 1) == Some(&b'*') => {
                offset = skip_block_comment(source, offset)?;
            }
            b'/' if regex.can_start() => {
                offset = skip_regex_literal(source, offset)?;
                regex.after_value();
            }
            b'/' => {
                offset += usize::from(source.as_bytes().get(offset + 1) == Some(&b'=')) + 1;
                regex.after_operator();
            }
            b'(' | b'[' | b'{' => {
                depth = depth.saturating_add(1);
                offset += 1;
                regex.after_operator();
            }
            b')' | b']' | b'}' => {
                if depth == 0 {
                    return Err(ScanError);
                }
                depth -= 1;
                offset += 1;
                regex.after_value();
            }
            b'?' if source[offset..].starts_with("?.") => {
                offset += 2;
                regex.after_property_access();
            }
            b'?' if source[offset..].starts_with("??") => {
                offset += 2;
                regex.after_operator();
            }
            b'?' if depth == 0 => {
                if question.is_none() {
                    question = Some(offset);
                } else {
                    nested_ternaries = nested_ternaries.saturating_add(1);
                }
                offset += 1;
                regex.after_operator();
            }
            b':' if depth == 0 && question.is_some() => {
                if nested_ternaries == 0 {
                    let question = question.ok_or(ScanError)?;
                    return Ok(Some((question, offset)));
                }
                nested_ternaries -= 1;
                offset += 1;
                regex.after_operator();
            }
            byte if is_identifier_start(byte) => {
                let (identifier, end) = read_identifier(source, offset).ok_or(ScanError)?;
                regex.after_identifier(identifier);
                offset = end;
            }
            byte if byte.is_ascii_digit() => {
                offset = skip_number_literal(source, offset);
                regex.after_value();
            }
            b'.' if source[offset..].starts_with("...") => {
                offset += 3;
                regex.after_operator();
            }
            b'.' => {
                offset += 1;
                regex.after_property_access();
            }
            b'+' | b'-'
                if source.as_bytes().get(offset + 1) == Some(&source.as_bytes()[offset]) =>
            {
                offset += 2;
            }
            _ => {
                let character = source[offset..].chars().next().ok_or(ScanError)?;
                offset += character.len_utf8();
                regex.after_character(character);
            }
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
    let mut regex = RegexContext::expression_start();
    while offset < source.len() {
        match source.as_bytes()[offset] {
            byte if byte.is_ascii_whitespace() => offset += 1,
            b'\'' | b'"' | b'`' => {
                offset = skip_quoted(source, offset)?;
                regex.after_value();
            }
            b'/' if is_tag_close(source, offset) => offset = regex.skip_markup(offset),
            b'/' if source.as_bytes().get(offset + 1) == Some(&b'/') => {
                offset = skip_line_comment(source, offset);
            }
            b'/' if source.as_bytes().get(offset + 1) == Some(&b'*') => {
                offset = skip_block_comment(source, offset)?;
            }
            b'/' if regex.can_start() => {
                offset = skip_regex_literal(source, offset)?;
                regex.after_value();
            }
            b'/' => {
                offset += usize::from(source.as_bytes().get(offset + 1) == Some(&b'=')) + 1;
                regex.after_operator();
            }
            byte if byte == open => {
                depth = depth.saturating_add(1);
                offset += 1;
                regex.after_operator();
            }
            byte if byte == close => {
                depth -= 1;
                if depth == 0 {
                    return Ok(offset);
                }
                offset += 1;
                regex.after_value();
            }
            b'(' | b'[' | b'{' => {
                offset += 1;
                regex.after_operator();
            }
            b')' | b']' | b'}' => {
                offset += 1;
                regex.after_value();
            }
            byte if is_identifier_start(byte) => {
                let (identifier, end) = read_identifier(source, offset).ok_or(ScanError)?;
                regex.after_identifier(identifier);
                offset = end;
            }
            byte if byte.is_ascii_digit() => {
                offset = skip_number_literal(source, offset);
                regex.after_value();
            }
            _ if source[offset..].starts_with("?.") => {
                offset += 2;
                regex.after_property_access();
            }
            b'.' if source[offset..].starts_with("...") => {
                offset += 3;
                regex.after_operator();
            }
            b'.' => {
                offset += 1;
                regex.after_property_access();
            }
            b'+' | b'-'
                if source.as_bytes().get(offset + 1) == Some(&source.as_bytes()[offset]) =>
            {
                offset += 2;
            }
            _ => {
                let character = source[offset..].chars().next().ok_or(ScanError)?;
                offset += character.len_utf8();
                regex.after_character(character);
            }
        }
    }
    Err(ScanError)
}

/// The shared delimiter matcher also walks JSX-like Astro markup nested inside
/// `{ ... }`. Recognize only a complete closing tag here, while leaving an
/// ambiguous sequence such as `value</li>/.test(input)` on the regex path.
fn is_tag_close(source: &str, slash: usize) -> bool {
    if slash == 0 || source.as_bytes()[slash - 1] != b'<' {
        return false;
    }

    let bytes = source.as_bytes();
    let mut offset = slash + 1;
    if bytes.get(offset) == Some(&b'>') {
        return bytes.get(offset + 1) != Some(&b'/');
    }
    if !bytes
        .get(offset)
        .is_some_and(|byte| byte.is_ascii_alphabetic() || matches!(byte, b'_' | b'$'))
    {
        return false;
    }

    offset += 1;
    while bytes.get(offset).is_some_and(|byte| {
        byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'$' | b'-' | b'.' | b':')
    }) {
        offset += 1;
    }
    while bytes.get(offset).is_some_and(u8::is_ascii_whitespace) {
        offset += 1;
    }

    bytes.get(offset) == Some(&b'>') && bytes.get(offset + 1) != Some(&b'/')
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

pub(super) fn skip_line_comment(source: &str, slash: usize) -> usize {
    source[slash + 2..]
        .find('\n')
        .map_or(source.len(), |newline| slash + 2 + newline + 1)
}

pub(super) fn skip_block_comment(source: &str, slash: usize) -> Result<usize, ScanError> {
    let close = source[slash + 2..].find("*/").ok_or(ScanError)?;
    Ok(slash + close + 4)
}

/// Skip a JavaScript regex literal, including escaped characters, character
/// classes, and trailing flags. The caller only enters here when the preceding
/// token requires an operand, which keeps division on the ordinary scan path.
pub(super) fn skip_regex_literal(source: &str, slash: usize) -> Result<usize, ScanError> {
    let mut offset = slash + 1;
    let mut in_character_class = false;
    while offset < source.len() {
        match source.as_bytes()[offset] {
            b'\\' => {
                offset += 1;
                if offset < source.len() {
                    offset += source[offset..].chars().next().map_or(0, char::len_utf8);
                }
            }
            b'[' if !in_character_class => {
                in_character_class = true;
                offset += 1;
            }
            b']' if in_character_class => {
                in_character_class = false;
                offset += 1;
            }
            b'/' if !in_character_class => {
                offset += 1;
                while offset < source.len() && source.as_bytes()[offset].is_ascii_alphabetic() {
                    offset += 1;
                }
                return Ok(offset);
            }
            b'\n' | b'\r' => return Err(ScanError),
            _ => offset += source[offset..].chars().next().map_or(1, char::len_utf8),
        }
    }
    Err(ScanError)
}

pub(super) fn skip_number_literal(source: &str, mut offset: usize) -> usize {
    while offset < source.len()
        && (source.as_bytes()[offset].is_ascii_alphanumeric()
            || matches!(source.as_bytes()[offset], b'.' | b'_' | b'\''))
    {
        offset += 1;
    }
    offset
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

    /// Score one bare expression and return its own `(cyclomatic, cognitive)`
    /// contribution, with the accumulator's implicit straight-line path removed.
    fn expression_metrics(source: &str) -> Option<(u16, u16)> {
        let mut complexity = TemplateComplexity::default();
        complexity.add_expression(source, 0, 0).ok()?;
        Some((complexity.cyclomatic - 1, complexity.cognitive))
    }

    #[test]
    fn shallow_expression_metrics_are_stable() {
        // A normal 2-3 level nested expression scores by logical-operator and
        // ternary count; the depth guard never fires for it.
        assert_eq!(expression_metrics("(a && b) ? c : (d || e)"), Some((3, 3)));
        assert_eq!(expression_metrics("(a && b)"), Some((1, 1)));
    }

    #[test]
    fn moderate_nesting_below_cap_scores_identically() {
        // Ten bracket levels is far below MAX_TEMPLATE_EXPR_DEPTH, so wrapping
        // `a && b` in redundant parens yields the same metrics as the bare
        // expression.
        let source = format!("{}a && b{}", "(".repeat(10), ")".repeat(10));
        assert_eq!(expression_metrics(&source), Some((1, 1)));
    }

    #[test]
    fn pathologically_deep_nesting_is_dropped_without_crashing() {
        // ~5000 nested parens previously recursed until the stack overflowed
        // (SIGABRT under release panic = "abort"). The depth guard now bails
        // past MAX_TEMPLATE_EXPR_DEPTH and the synthetic finding is dropped.
        let depth = 5000;
        let source = format!("{}a{}", "(".repeat(depth), ")".repeat(depth));

        let mut complexity = TemplateComplexity::default();
        assert!(complexity.add_expression(&source, 0, 0).is_err());
    }

    #[test]
    fn expression_contributions_are_anchored_at_their_operator() {
        let mut complexity = TemplateComplexity::default();
        complexity.add_expression("a && b || c", 100, 0).unwrap();

        let offsets: Vec<usize> = complexity
            .contributions
            .iter()
            .filter(|contribution| contribution.metric == ComplexityMetric::Cyclomatic)
            .map(|contribution| contribution.offset)
            .collect();
        assert_eq!(offsets, vec![102, 107]);
    }

    #[test]
    fn regex_contents_are_ignored_and_following_operators_are_scored() {
        assert_eq!(
            expression_metrics(r"/[?():{}&|]+/.test(input) && ready"),
            Some((1, 1))
        );
        assert_eq!(
            expression_metrics(r"(value</li>/.test(input)) && ready"),
            Some((1, 1))
        );
    }

    #[test]
    fn markup_closing_tags_do_not_look_like_regex_literals() {
        let source = "{show && items.map((item) => <li>{item}</li>)}";
        let close = find_matching_delimiter(source, 0, b'{', b'}')
            .expect("markup expression should have a matching brace");

        assert_eq!(close, source.len() - 1);
        assert_eq!(expression_metrics(&source[1..close]), Some((1, 1)));
    }
}
