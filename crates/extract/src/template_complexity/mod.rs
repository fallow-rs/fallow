//! Synthetic `<template>` cyclomatic and cognitive complexity for framework
//! templates (Angular `.html` + inline decorators, Vue SFCs, Svelte SFCs).
//!
//! The framework-agnostic JS-expression engine (`TemplateComplexity`,
//! `compute_expression_metrics`, and the byte-safe tokenization helpers) lives
//! in [`engine`] and is shared by all three scanners. This module hosts the
//! Angular outer scanner; [`vue`] and [`svelte`] host the SFC scanners.

mod astro;
mod engine;
mod svelte;
mod vue;

use fallow_types::extract::{FunctionComplexity, byte_offset_to_line_col, compute_line_offsets};

use engine::{
    ScanError, TemplateComplexity, find_matching_delimiter, find_tag_end, is_identifier_after,
    is_identifier_before, read_attribute_value, read_identifier, skip_quoted, skip_whitespace,
};

pub use astro::compute_astro_template_complexity;
pub use svelte::compute_svelte_template_complexity;
pub use vue::compute_vue_template_complexity;

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
            offset = self.scan_next(offset)?;
        }

        if self.block_depth == 0 {
            Ok(self.complexity)
        } else {
            Err(ScanError)
        }
    }

    fn scan_next(&mut self, offset: usize) -> Result<usize, ScanError> {
        if self.source[offset..].starts_with("<!--") {
            return self.scan_comment(offset);
        }
        if self.source[offset..].starts_with("{{") {
            return self.scan_interpolation(offset);
        }

        match self.source.as_bytes()[offset] {
            b'\'' | b'"' => skip_quoted(self.source, offset),
            b'<' => self.scan_element(offset),
            b'@' if !is_identifier_before(self.source, offset) => self
                .scan_block_keyword(offset)
                .map(|next| next.unwrap_or(offset + 1)),
            b'{' => Ok(self.push_block_depth(offset)),
            b'}' => self.pop_block_depth(offset),
            _ => Ok(self.advance_char(offset)),
        }
    }

    fn scan_comment(&self, offset: usize) -> Result<usize, ScanError> {
        self.find_required(offset + 4, "-->").map(|end| end + 3)
    }

    fn scan_interpolation(&mut self, offset: usize) -> Result<usize, ScanError> {
        let expr_start = offset + 2;
        let end = self.find_required(expr_start, "}}")?;
        self.complexity.add_expression(
            &self.source[expr_start..end],
            expr_start,
            self.block_depth,
        )?;
        Ok(end + 2)
    }

    fn push_block_depth(&mut self, offset: usize) -> usize {
        self.block_depth = self.block_depth.saturating_add(1);
        offset + 1
    }

    fn pop_block_depth(&mut self, offset: usize) -> Result<usize, ScanError> {
        if self.block_depth == 0 {
            return Err(ScanError);
        }
        self.block_depth -= 1;
        Ok(offset + 1)
    }

    fn advance_char(&self, offset: usize) -> usize {
        offset
            + self.source[offset..]
                .chars()
                .next()
                .map_or(1, char::len_utf8)
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
    build_template_complexity(source, &complexity)
}

/// Replace each `regex` match in `source` with an equal-length run of ASCII
/// spaces, preserving byte offsets for the unmasked regions. Used by the SFC
/// scanners to mask `<script>` / `<style>` / comment regions before scanning,
/// matching the masking convention in `crate::sfc_template`. Building a fresh
/// `String` (rather than mutating in place) keeps the crate `unsafe`-free.
fn mask_ranges(source: &str, regex: &regex::Regex) -> String {
    let mut spans: Vec<(usize, usize)> = regex
        .find_iter(source)
        .map(|m| (m.start(), m.end()))
        .collect();
    spans.sort_unstable_by_key(|range| range.0);

    let mut masked = String::with_capacity(source.len());
    let mut cursor = 0;
    for (start, end) in spans {
        if start < cursor {
            // Overlapping or already-consumed match (the regex alternates, so
            // matches can adjoin); skip to keep the cursor monotonic.
            continue;
        }
        masked.push_str(&source[cursor..start]);
        masked.extend(std::iter::repeat_n(' ', end - start));
        cursor = end;
    }
    masked.push_str(&source[cursor..]);
    masked
}

/// Shared emission shape for every framework template scanner: drop the
/// trivial baseline, anchor the finding at the first non-trivial expression,
/// and produce the synthetic `<template>` [`FunctionComplexity`].
pub(in crate::template_complexity) fn build_template_complexity(
    source: &str,
    complexity: &TemplateComplexity,
) -> Option<FunctionComplexity> {
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
        // The hand-rolled template scanners emit only aggregate metrics;
        // per-construct contributions are out of scope for the first cut.
        contributions: Vec::new(),
    })
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
