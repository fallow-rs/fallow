//! Angular template cyclomatic and cognitive complexity.

use fallow_types::extract::{FunctionComplexity, byte_offset_to_line_col, compute_line_offsets};

/// Internal scanner error. Carries no data: any malformed-template path
/// just falls through and the caller drops the synthetic finding.
#[derive(Debug, Clone, Copy)]
struct ScanError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LogicalOperator {
    And,
    Or,
    Nullish,
}

#[derive(Debug)]
struct TemplateComplexity {
    cyclomatic: u16,
    cognitive: u16,
    first_offset: Option<usize>,
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
    fn add_expression(
        &mut self,
        source: &str,
        offset: usize,
        nesting: u16,
    ) -> Result<(), ScanError> {
        let Some(trim_start) = source.find(|c: char| !c.is_whitespace()) else {
            return Ok(());
        };
        self.first_offset.get_or_insert(offset + trim_start);
        let metrics = compute_expression_metrics(&source[trim_start..], nesting)?;
        self.cyclomatic = self.cyclomatic.saturating_add(metrics.cyclomatic);
        self.cognitive = self.cognitive.saturating_add(metrics.cognitive);
        Ok(())
    }

    fn add_control_flow(&mut self, nesting: u16) {
        self.cyclomatic = self.cyclomatic.saturating_add(1);
        self.cognitive = self.cognitive.saturating_add(1 + nesting);
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

struct TemplateScanner<'a> {
    source: &'a str,
    complexity: TemplateComplexity,
    block_depth: u16,
}

impl<'a> TemplateScanner<'a> {
    fn new(source: &'a str) -> Self {
        Self {
            source,
            complexity: TemplateComplexity::default(),
            block_depth: 0,
        }
    }

    fn scan(mut self) -> Result<TemplateComplexity, ScanError> {
        let mut offset = 0;
        while offset < self.source.len() {
            if self.source[offset..].starts_with("<!--") {
                offset = self.find_required(offset + 4, "-->")? + 3;
                continue;
            }
            if self.source[offset..].starts_with("{{") {
                let end = self.find_required(offset + 2, "}}")?;
                self.complexity.add_expression(
                    &self.source[offset + 2..end],
                    offset + 2,
                    self.block_depth,
                )?;
                offset = end + 2;
                continue;
            }

            match self.source.as_bytes()[offset] {
                b'\'' | b'"' => offset = skip_quoted(self.source, offset)?,
                b'<' => offset = self.scan_element(offset)?,
                b'@' if !is_identifier_before(self.source, offset) => {
                    if let Some(next) = self.scan_block_keyword(offset)? {
                        offset = next;
                    } else {
                        offset += 1;
                    }
                }
                b'{' => {
                    self.block_depth = self.block_depth.saturating_add(1);
                    offset += 1;
                }
                b'}' => {
                    if self.block_depth == 0 {
                        return Err(ScanError);
                    }
                    self.block_depth -= 1;
                    offset += 1;
                }
                _ => {
                    offset += self.source[offset..]
                        .chars()
                        .next()
                        .map_or(1, char::len_utf8);
                }
            }
        }

        if self.block_depth == 0 {
            Ok(self.complexity)
        } else {
            Err(ScanError)
        }
    }

    fn find_required(&self, offset: usize, needle: &str) -> Result<usize, ScanError> {
        self.source[offset..]
            .find(needle)
            .map(|relative| offset + relative)
            .ok_or(ScanError)
    }

    fn scan_block_keyword(&mut self, offset: usize) -> Result<Option<usize>, ScanError> {
        let Some((keyword, after_keyword)) = read_identifier(self.source, offset + 1) else {
            return Ok(None);
        };

        match keyword {
            "if" | "for" => {
                let (expr_start, expr_end, after_paren) =
                    parse_parenthesized(self.source, after_keyword)?;
                self.complexity.add_control_flow(self.block_depth);
                self.complexity.add_expression(
                    &self.source[expr_start..expr_end],
                    expr_start,
                    self.block_depth,
                )?;
                Ok(Some(after_paren))
            }
            "else" => self.scan_else(after_keyword),
            "switch" => {
                let (expr_start, expr_end, after_paren) =
                    parse_parenthesized(self.source, after_keyword)?;
                self.complexity.cognitive = self
                    .complexity
                    .cognitive
                    .saturating_add(1 + self.block_depth);
                self.complexity.add_expression(
                    &self.source[expr_start..expr_end],
                    expr_start,
                    self.block_depth,
                )?;
                Ok(Some(after_paren))
            }
            "case" => {
                let (expr_start, expr_end, after_paren) =
                    parse_parenthesized(self.source, after_keyword)?;
                self.complexity.cyclomatic = self.complexity.cyclomatic.saturating_add(1);
                self.complexity.add_expression(
                    &self.source[expr_start..expr_end],
                    expr_start,
                    self.block_depth,
                )?;
                Ok(Some(after_paren))
            }
            "default" | "placeholder" | "loading" | "error" | "empty" => Ok(Some(after_keyword)),
            "defer" => self.scan_defer(after_keyword),
            "let" => self.scan_let(after_keyword),
            _ => Ok(None),
        }
    }

    fn scan_else(&mut self, after_else: usize) -> Result<Option<usize>, ScanError> {
        let after_ws = skip_whitespace(self.source, after_else);
        if self.source[after_ws..].starts_with("if")
            && !is_identifier_after(self.source, after_ws + "if".len())
        {
            let after_if = after_ws + "if".len();
            let (expr_start, expr_end, after_paren) = parse_parenthesized(self.source, after_if)?;
            self.complexity.cyclomatic = self.complexity.cyclomatic.saturating_add(1);
            self.complexity.cognitive = self.complexity.cognitive.saturating_add(1);
            self.complexity.add_expression(
                &self.source[expr_start..expr_end],
                expr_start,
                self.block_depth,
            )?;
            Ok(Some(after_paren))
        } else {
            self.complexity.cognitive = self.complexity.cognitive.saturating_add(1);
            Ok(Some(after_else))
        }
    }

    fn scan_defer(&mut self, after_defer: usize) -> Result<Option<usize>, ScanError> {
        let after_ws = skip_whitespace(self.source, after_defer);
        if !self.source[after_ws..].starts_with('(') {
            return Ok(Some(after_defer));
        }
        let (expr_start, expr_end, after_paren) = parse_parenthesized(self.source, after_defer)?;
        let expr = &self.source[expr_start..expr_end];
        if let Some(when_offset) = find_word(expr, "when") {
            let condition_offset = expr_start + when_offset + "when".len();
            self.complexity.add_control_flow(self.block_depth);
            self.complexity.add_expression(
                &self.source[condition_offset..expr_end],
                condition_offset,
                self.block_depth,
            )?;
        }
        Ok(Some(after_paren))
    }

    fn scan_let(&mut self, after_let: usize) -> Result<Option<usize>, ScanError> {
        let Some(relative_end) = self.source[after_let..].find(';') else {
            return Err(ScanError);
        };
        let end = after_let + relative_end;
        if let Some(eq) = self.source[after_let..end].find('=') {
            let expr_start = after_let + eq + 1;
            self.complexity.add_expression(
                &self.source[expr_start..end],
                expr_start,
                self.block_depth,
            )?;
        }
        Ok(Some(end + 1))
    }

    fn scan_element(&mut self, offset: usize) -> Result<usize, ScanError> {
        let end = find_tag_end(self.source, offset)?;
        if !self.source[offset..].starts_with("</") {
            self.scan_attributes(offset, end)?;
        }
        Ok(end + 1)
    }

    fn scan_attributes(&mut self, tag_start: usize, tag_end: usize) -> Result<(), ScanError> {
        let mut offset = tag_start + 1;
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
                continue;
            }
            offset = skip_whitespace(self.source, offset + 1);
            let (value_start, value_end, next_offset) = read_attribute_value(self.source, offset)?;
            self.scan_attribute_value(name, value_start, value_end)?;
            offset = next_offset;
        }
        Ok(())
    }

    fn scan_attribute_value(
        &mut self,
        name: &str,
        value_start: usize,
        value_end: usize,
    ) -> Result<(), ScanError> {
        let value = &self.source[value_start..value_end];
        if matches!(
            name,
            "*ngIf" | "[ngIf]" | "*ngFor" | "*ngForOf" | "[ngFor]" | "[ngForOf]"
        ) {
            self.complexity.add_control_flow(self.block_depth);
            self.complexity
                .add_expression(value, value_start, self.block_depth)?;
        } else if is_bound_template_attribute(name) {
            self.complexity
                .add_expression(value, value_start, self.block_depth)?;
        }
        scan_interpolations(value, value_start, self.block_depth, &mut self.complexity)
    }
}

/// Compute synthetic `<template>` complexity for an Angular HTML template.
pub fn compute_angular_template_complexity(source: &str) -> Option<FunctionComplexity> {
    let complexity = TemplateScanner::new(source).scan().ok()?;
    if complexity.cyclomatic == 1 && complexity.cognitive == 0 {
        return None;
    }

    let line_offsets = compute_line_offsets(source);
    let first_offset = u32::try_from(complexity.first_offset.unwrap_or(0)).unwrap_or(u32::MAX);
    let (line, col) = byte_offset_to_line_col(&line_offsets, first_offset);
    let line_count = u32::try_from(source.lines().count()).unwrap_or(u32::MAX);

    Some(FunctionComplexity {
        name: "<template>".to_string(),
        line,
        col,
        cyclomatic: complexity.cyclomatic,
        cognitive: complexity.cognitive,
        line_count,
        param_count: 0,
        react_hook_count: 0,
        react_jsx_max_depth: 0,
        react_prop_count: 0,
        source_hash: None,
        // The hand-rolled Angular template scanner emits only aggregate metrics;
        // per-construct contributions are out of scope for the first cut.
        contributions: Vec::new(),
    })
}

fn compute_expression_metrics(source: &str, nesting: u16) -> Result<ExpressionMetrics, ScanError> {
    let source = source.trim();
    if source.is_empty() {
        return Ok(ExpressionMetrics::default());
    }
    if let Some((question, colon)) = find_top_level_ternary(source)? {
        let mut metrics = ExpressionMetrics::default();
        metrics.add(compute_expression_metrics(&source[..question], nesting)?);
        metrics.cyclomatic = metrics.cyclomatic.saturating_add(1);
        metrics.cognitive = metrics.cognitive.saturating_add(1 + nesting);
        metrics.add(compute_expression_metrics(
            &source[question + 1..colon],
            nesting.saturating_add(1),
        )?);
        metrics.add(compute_expression_metrics(
            &source[colon + 1..],
            nesting.saturating_add(1),
        )?);
        return Ok(metrics);
    }
    scan_expression_without_ternary(source, nesting)
}

fn scan_expression_without_ternary(
    source: &str,
    nesting: u16,
) -> Result<ExpressionMetrics, ScanError> {
    let mut metrics = ExpressionMetrics::default();
    let mut last_logical_operator: Option<LogicalOperator> = None;
    let mut needs_rhs = false;
    let mut offset = 0;

    while offset < source.len() {
        match source.as_bytes()[offset] {
            byte if byte.is_ascii_whitespace() => offset += 1,
            b'\'' | b'"' | b'`' => {
                offset = skip_quoted(source, offset)?;
                needs_rhs = false;
            }
            b'(' | b'[' | b'{' => {
                let close = matching_close_byte(source.as_bytes()[offset]).ok_or(ScanError)?;
                let end =
                    find_matching_delimiter(source, offset, source.as_bytes()[offset], close)?;
                metrics.add(compute_expression_metrics(
                    &source[offset + 1..end],
                    nesting,
                )?);
                last_logical_operator = None;
                needs_rhs = false;
                offset = end + 1;
            }
            b')' | b']' | b'}' => return Err(ScanError),
            _ if source[offset..].starts_with("?.") => {
                metrics.cyclomatic = metrics.cyclomatic.saturating_add(1);
                offset += 2;
            }
            _ if source[offset..].starts_with("&&=")
                || source[offset..].starts_with("||=")
                || source[offset..].starts_with("??=") =>
            {
                metrics.cyclomatic = metrics.cyclomatic.saturating_add(1);
                last_logical_operator = None;
                needs_rhs = true;
                offset += 3;
            }
            _ if source[offset..].starts_with("&&")
                || source[offset..].starts_with("||")
                || source[offset..].starts_with("??") =>
            {
                if needs_rhs {
                    return Err(ScanError);
                }
                let operator = if source[offset..].starts_with("&&") {
                    LogicalOperator::And
                } else if source[offset..].starts_with("||") {
                    LogicalOperator::Or
                } else {
                    LogicalOperator::Nullish
                };
                metrics.cyclomatic = metrics.cyclomatic.saturating_add(1);
                if last_logical_operator != Some(operator) {
                    metrics.cognitive = metrics.cognitive.saturating_add(1);
                    last_logical_operator = Some(operator);
                }
                needs_rhs = true;
                offset += 2;
            }
            b',' | b';' => {
                if needs_rhs {
                    return Err(ScanError);
                }
                last_logical_operator = None;
                offset += 1;
            }
            _ => {
                needs_rhs = false;
                offset += source[offset..].chars().next().map_or(1, char::len_utf8);
            }
        }
    }

    if needs_rhs {
        Err(ScanError)
    } else {
        Ok(metrics)
    }
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

fn scan_interpolations(
    source: &str,
    base_offset: usize,
    nesting: u16,
    complexity: &mut TemplateComplexity,
) -> Result<(), ScanError> {
    let mut offset = 0;
    while let Some(start) = source[offset..].find("{{") {
        let expr_start = offset + start + 2;
        let Some(relative_end) = source[expr_start..].find("}}") else {
            return Err(ScanError);
        };
        let expr_end = expr_start + relative_end;
        complexity.add_expression(
            &source[expr_start..expr_end],
            base_offset + expr_start,
            nesting,
        )?;
        offset = expr_end + 2;
    }
    Ok(())
}

fn parse_parenthesized(source: &str, offset: usize) -> Result<(usize, usize, usize), ScanError> {
    let open = skip_whitespace(source, offset);
    if !source[open..].starts_with('(') {
        return Err(ScanError);
    }
    let close = find_matching_delimiter(source, open, b'(', b')')?;
    Ok((open + 1, close, close + 1))
}

fn find_matching_delimiter(
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

fn skip_quoted(source: &str, quote_offset: usize) -> Result<usize, ScanError> {
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

fn skip_whitespace(source: &str, mut offset: usize) -> usize {
    while offset < source.len() && source.as_bytes()[offset].is_ascii_whitespace() {
        offset += 1;
    }
    offset
}

fn read_identifier(source: &str, offset: usize) -> Option<(&str, usize)> {
    if offset >= source.len() || !is_identifier_start(source.as_bytes()[offset]) {
        return None;
    }
    let mut end = offset + 1;
    while end < source.len() && is_identifier_continue(source.as_bytes()[end]) {
        end += 1;
    }
    Some((&source[offset..end], end))
}

fn find_word(source: &str, word: &str) -> Option<usize> {
    let mut offset = 0;
    while let Some(relative) = source[offset..].find(word) {
        let start = offset + relative;
        let end = start + word.len();
        if !is_identifier_before(source, start) && !is_identifier_after(source, end) {
            return Some(start);
        }
        offset = end;
    }
    None
}

fn is_bound_template_attribute(name: &str) -> bool {
    name.starts_with('[')
        || name.starts_with('(')
        || name.starts_with("bind-")
        || name.starts_with("on-")
}

fn matching_close_byte(open: u8) -> Option<u8> {
    match open {
        b'(' => Some(b')'),
        b'[' => Some(b']'),
        b'{' => Some(b'}'),
        _ => None,
    }
}

fn is_identifier_before(source: &str, offset: usize) -> bool {
    offset > 0 && is_identifier_continue(source.as_bytes()[offset - 1])
}

fn is_identifier_after(source: &str, offset: usize) -> bool {
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
    use super::compute_angular_template_complexity;

    #[test]
    fn counts_control_flow_and_expressions() {
        let complexity = compute_angular_template_complexity(
            r#"
@if (user?.enabled && featureFlags.dashboard) {
  @for (item of items; track item.id) {
    @switch (item.status) {
      @case ('active') {
        <badge [color]="item.level > 3 ? 'red' : 'green'" />
      }
      @default {
        <placeholder />
      }
    }
  } @empty {
    <empty-state />
  }
} @else {
  @let label = user?.email ?? 'Anonymous';
  <p>{{ label }}</p>
}
"#,
        )
        .expect("template should have complexity");

        assert!(complexity.cyclomatic >= 8, "{complexity:?}");
        assert!(complexity.cognitive >= 5, "{complexity:?}");
    }

    #[test]
    fn resets_logical_sequences_across_ternary_branches() {
        let complexity = compute_angular_template_complexity(
            r#"
@if (enabled) {
  <badge [color]="a && b ? c && d : e && f" />
}
"#,
        )
        .expect("template should have complexity");

        assert!(complexity.cognitive >= 5, "{complexity:?}");
    }

    #[test]
    fn malformed_template_does_not_report_recovered_complexity() {
        assert!(compute_angular_template_complexity("@if (enabled) {").is_none());
        assert!(compute_angular_template_complexity("<p>{{ enabled &&").is_none());
        assert!(compute_angular_template_complexity("@if (enabled &&) { <p /> }").is_none());
        assert!(compute_angular_template_complexity("<p>{{ enabled && }}</p>").is_none());
    }

    #[test]
    fn plain_html_without_angular_syntax_has_no_synthetic_complexity() {
        assert!(compute_angular_template_complexity("<p>Hello world</p>").is_none());
        assert!(
            compute_angular_template_complexity(
                r#"<!DOCTYPE html><html><body><div class="x">Plain</div></body></html>"#
            )
            .is_none()
        );
    }

    #[test]
    fn else_if_cascade_increments_cyclomatic_per_branch() {
        let complexity = compute_angular_template_complexity(
            r"
@if (a) { <p>1</p> }
@else if (b) { <p>2</p> }
@else if (c) { <p>3</p> }
@else { <p>4</p> }
",
        )
        .expect("template should have complexity");
        assert_eq!(complexity.cyclomatic, 4, "{complexity:?}");
    }

    #[test]
    fn for_block_with_track_and_empty_counts_once() {
        let complexity = compute_angular_template_complexity(
            r"
@for (item of items; track item.id) {
  <li>{{ item.name }}</li>
} @empty {
  <li>None</li>
}
",
        )
        .expect("template should have complexity");
        assert_eq!(complexity.cyclomatic, 2, "{complexity:?}");
    }

    #[test]
    fn switch_with_multiple_cases_counts_each() {
        let complexity = compute_angular_template_complexity(
            r"
@switch (status) {
  @case ('a') { <p /> }
  @case ('b') { <p /> }
  @case ('c') { <p /> }
  @default { <p /> }
}
",
        )
        .expect("template should have complexity");
        assert_eq!(complexity.cyclomatic, 4, "{complexity:?}");
    }

    #[test]
    fn defer_when_counts_as_branch_and_other_blocks_pass_through() {
        let complexity = compute_angular_template_complexity(
            r"
@defer (when ready && !blocked) {
  <heavy />
} @placeholder { <p /> }
  @loading { <p /> }
  @error { <p /> }
",
        )
        .expect("template should have complexity");
        assert!(complexity.cyclomatic >= 2, "{complexity:?}");
        assert!(complexity.cognitive >= 1, "{complexity:?}");
    }

    #[test]
    fn defer_without_when_does_not_count() {
        assert!(compute_angular_template_complexity("@defer { <p /> }").is_none());
    }

    #[test]
    fn let_declaration_with_logical_chain_contributes() {
        let complexity = compute_angular_template_complexity(
            "@let label = user?.name && user?.email ?? 'anon';",
        )
        .expect("template should have complexity");
        assert!(complexity.cyclomatic >= 2, "{complexity:?}");
    }

    #[test]
    fn legacy_structural_directives_count_as_control_flow() {
        let complexity = compute_angular_template_complexity(
            r#"
<section *ngIf="user?.isAdmin">
  <div *ngFor="let item of items">{{ item.label }}</div>
</section>
"#,
        )
        .expect("template should have complexity");
        assert!(complexity.cyclomatic >= 3, "{complexity:?}");
    }

    #[test]
    fn bound_attribute_expressions_contribute_complexity() {
        let complexity = compute_angular_template_complexity(
            r#"<button [disabled]="loading || !form.valid" (click)="submit() && refresh()" />"#,
        )
        .expect("template should have complexity");
        assert!(complexity.cyclomatic >= 3, "{complexity:?}");
    }

    #[test]
    fn interpolations_inside_attribute_values_are_scanned() {
        let complexity = compute_angular_template_complexity(
            r#"<input placeholder="{{ enabled && draft ? 'Draft' : 'New' }}" />"#,
        )
        .expect("template should have complexity");
        assert!(complexity.cyclomatic >= 3, "{complexity:?}");
    }

    #[test]
    fn html_comments_are_skipped() {
        assert!(compute_angular_template_complexity("<!-- a && b && c --><p>plain</p>").is_none());
    }

    #[test]
    fn closing_tags_without_attributes_do_not_panic() {
        let complexity =
            compute_angular_template_complexity("<section><div *ngIf=\"a\">x</div></section>")
                .expect("template should have complexity");
        assert!(complexity.cyclomatic >= 2, "{complexity:?}");
    }

    #[test]
    fn quoted_strings_inside_attributes_do_not_break_scanner() {
        let complexity = compute_angular_template_complexity(
            r"<a href='https://example.com?q=1&r=2' [class.x]='a && b' />",
        )
        .expect("template should have complexity");
        assert!(complexity.cyclomatic >= 2, "{complexity:?}");
    }

    #[test]
    fn backslash_before_multibyte_char_in_attribute_does_not_panic() {
        // U+200B is a 3-byte char, so a +2 byte advance past `\` lands mid-char.
        let complexity =
            compute_angular_template_complexity("<a title='x\\\u{200b}y' [class.x]='a && b' />")
                .expect("template should have complexity");
        assert!(complexity.cyclomatic >= 2, "{complexity:?}");
    }
}
