//! Angular HTML template scanner for member reference extraction.
//!
//! Scans Angular external HTML templates for identifier references in:
//! - `{{ expression }}` interpolation
//! - `[prop]="expression"` / `(event)="statement"` / `[(prop)]="expression"` bindings
//! - `*ngIf="expression"` / `*ngFor="let x of expr"` structural directives
//! - `@if (expr)` / `@for (x of expr; track expr)` / `@switch (expr)` control flow (Angular 17+)
//! - `@defer (when expr)` deferred loading blocks (Angular 17+)
//! - `@let name = expr;` template-local variables (Angular 18+)
//! - `| pipeName` pipe references
//!
//! Referenced identifiers are stored as `MemberAccess` entries with a sentinel object name
//! so the analysis phase can bridge them to the importing component's class members.

use std::sync::LazyLock;

use rustc_hash::FxHashSet;

use fallow_types::extract::SinkSite;

use crate::MemberAccess;
use crate::template_usage::{TemplateSnippetKind, collect_unresolved_refs_and_accesses};

use super::scanners::{scan_curly_section, scan_html_tag};

/// Sentinel value used as the `object` field in `MemberAccess` entries
/// produced by the Angular template scanner. The analysis phase checks imports
/// for entries with this sentinel and merges them into the component's
/// `self_accessed_members` set.
pub const ANGULAR_TPL_SENTINEL: &str = "__angular_tpl__";

/// Result of scanning an Angular template for member references.
#[derive(Debug, Default)]
pub struct AngularTemplateRefs {
    /// Top-level unresolved identifiers referenced in the template
    /// (e.g., `title`, `dataService`, pipe names). Each identifier is a
    /// potential component class member name.
    pub identifiers: FxHashSet<String>,
    /// Static member-access chains (`object.member`) where `object` is one
    /// of the unresolved identifiers above. Used to resolve chains like
    /// `dataService.getTotal()` through the component's typed instance
    /// bindings to credit the correct class's member as used.
    pub member_accesses: Vec<MemberAccess>,
    /// Non-literal template HTML injection sinks captured with source spans.
    pub security_sinks: Vec<SinkSite>,
}

impl AngularTemplateRefs {
    fn add_member_access(&mut self, access: MemberAccess) {
        let key = (&access.object, &access.member);
        let already_present = self
            .member_accesses
            .iter()
            .any(|existing| (&existing.object, &existing.member) == key);
        if !already_present {
            self.member_accesses.push(access);
        }
    }

    /// Whether the given identifier appears in this template's unresolved refs.
    #[cfg(test)]
    #[must_use]
    pub fn contains(&self, name: &str) -> bool {
        self.identifiers.contains(name)
    }

    /// Whether this template produced no refs or member accesses at all.
    #[cfg(test)]
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.identifiers.is_empty() && self.member_accesses.is_empty()
    }
}

/// Regex to strip HTML comments before scanning.
static HTML_COMMENT_RE: LazyLock<regex::Regex> =
    LazyLock::new(|| crate::static_regex(r"(?s)<!--.*?-->"));

/// Regex to extract attribute name-value pairs from an HTML tag.
/// Captures: group 1 = attribute name (including prefix like `[`, `(`, `*`),
///           group 2 = value (inside quotes).
static ATTR_RE: LazyLock<regex::Regex> =
    LazyLock::new(|| crate::static_regex(r#"(?s)([\[()*#a-zA-Z][\w.\-\[\]()]*)\s*=\s*"([^"]*)""#));

/// Regex to parse `*ngFor` microsyntax: `let item of items`.
static NG_FOR_OF_RE: LazyLock<regex::Regex> =
    LazyLock::new(|| crate::static_regex(r"(?s)^\s*let\s+(\w+)\s+of\s+(.+)$"));

/// Regex to match Angular 17+ `@for (item of expr; track expr)` control flow.
static CONTROL_FOR_RE: LazyLock<regex::Regex> =
    LazyLock::new(|| crate::static_regex(r"(?s)^\s*(\w+)\s+of\s+(.+)$"));

/// Scan an Angular HTML template and collect all referenced identifiers and
/// member-access chains.
///
/// Returns a deduplicated set of top-level identifier names, plus static
/// `obj.member` chains where `obj` is an unresolved identifier. Together these
/// represent potential component class member references.
pub fn collect_angular_template_refs(source: &str) -> AngularTemplateRefs {
    let source = strip_html_comments_preserve_offsets(source);
    let bytes = source.as_bytes();
    let mut refs = AngularTemplateRefs::default();
    let mut scopes: Vec<Vec<String>> = vec![Vec::new()];
    let mut index = 0;

    while index < bytes.len() {
        if index + 1 < bytes.len() && bytes[index] == b'{' && bytes[index + 1] == b'{' {
            let Some((expr, next_index)) = scan_curly_section(&source, index, 2, 2) else {
                break;
            };
            collect_expression_refs(expr.trim(), &current_locals(&scopes), &mut refs);
            index = next_index;
            continue;
        }

        if bytes[index] == b'@'
            && let Some(next_index) = handle_control_flow(&source, index, &mut scopes, &mut refs)
        {
            index = next_index;
            continue;
        }

        if bytes[index] == b'}' {
            if scopes.len() > 1 {
                scopes.pop();
            }
            index += 1;
            continue;
        }

        if bytes[index] == b'<' {
            if let Some((tag, next_index)) = scan_html_tag(&source, index) {
                process_tag(tag, index, &mut scopes, &mut refs);
                index = next_index;
                continue;
            }
            index += 1;
            continue;
        }

        index += 1;
    }

    refs
}

fn strip_html_comments_preserve_offsets(source: &str) -> String {
    let mut stripped = String::with_capacity(source.len());
    let mut cursor = 0;
    for range in HTML_COMMENT_RE.find_iter(source) {
        stripped.push_str(&source[cursor..range.start()]);
        stripped.extend(std::iter::repeat_n(' ', range.end() - range.start()));
        cursor = range.end();
    }
    stripped.push_str(&source[cursor..]);
    stripped
}

/// Handle Angular 17+ control flow blocks (`@if`, `@else if`, `@for`, `@switch`, `@case`, `@defer`, `@let`, etc.).
/// Returns the index after the opening `{` of the block (or after the `;` for `@let`),
/// or `None` if not a control flow keyword.
fn handle_control_flow(
    source: &str,
    start: usize,
    scopes: &mut Vec<Vec<String>>,
    refs: &mut AngularTemplateRefs,
) -> Option<usize> {
    let rest = &source[start + 1..]; // skip '@'

    let keyword_end = rest.find(|c: char| !c.is_ascii_alphabetic())?;
    let keyword = &rest[..keyword_end];

    match keyword {
        "if" => {
            let after_keyword = &source[start + 1 + keyword_end..];
            let paren_start = after_keyword.find('(')?;
            let paren_content_start = start + 1 + keyword_end + paren_start;
            let (paren_content, after_paren) = scan_parenthesized(source, paren_content_start)?;
            let (cond_expr, alias_name) = parse_if_condition_and_alias(paren_content);
            let locals = current_locals(scopes);
            collect_expression_refs(cond_expr.trim(), &locals, refs);
            let mut scope_locals = Vec::new();
            if let Some(alias) = alias_name {
                scope_locals.push(alias.to_string());
            }
            scopes.push(scope_locals);
            let brace_pos = source[after_paren..].find('{')?;
            Some(after_paren + brace_pos + 1)
        }
        "switch" | "case" => {
            let after_keyword = &source[start + 1 + keyword_end..];
            let paren_start = after_keyword.find('(')?;
            let paren_content_start = start + 1 + keyword_end + paren_start;
            let (expr, after_paren) = scan_parenthesized(source, paren_content_start)?;
            let locals = current_locals(scopes);
            collect_expression_refs(expr.trim(), &locals, refs);
            scopes.push(Vec::new());
            let brace_pos = source[after_paren..].find('{')?;
            Some(after_paren + brace_pos + 1)
        }
        "for" => {
            let after_keyword = &source[start + 1 + keyword_end..];
            let paren_start = after_keyword.find('(')?;
            let paren_content_start = start + 1 + keyword_end + paren_start;
            let (paren_content, after_paren) = scan_parenthesized(source, paren_content_start)?;

            let mut locals_for_scope = Vec::new();

            let parts: Vec<&str> = paren_content.split(';').collect();
            if let Some(first_part) = parts.first()
                && let Some(caps) = CONTROL_FOR_RE.captures(first_part.trim())
            {
                let binding = caps.get(1).map_or("", |m| m.as_str());
                locals_for_scope.push(binding.to_string());
                for implicit in &["$index", "$first", "$last", "$even", "$odd", "$count"] {
                    locals_for_scope.push((*implicit).to_string());
                }
                let iterable = caps.get(2).map_or("", |m| m.as_str()).trim();
                let current = current_locals(scopes);
                collect_expression_refs(iterable, &current, refs);
            }

            for part in parts.iter().skip(1) {
                let part = part.trim();
                if let Some(track_expr) = part.strip_prefix("track") {
                    let mut all_locals = current_locals(scopes);
                    all_locals.extend(locals_for_scope.clone());
                    collect_expression_refs(track_expr.trim(), &all_locals, refs);
                }
            }

            scopes.push(locals_for_scope);
            let brace_pos = source[after_paren..].find('{')?;
            Some(after_paren + brace_pos + 1)
        }
        "defer" => {
            let after_keyword = &source[start + 1 + keyword_end..];
            let trimmed = after_keyword.trim_start();
            let offset = after_keyword.len() - trimmed.len();
            let abs_after_keyword = start + 1 + keyword_end + offset;

            if trimmed.starts_with('(') {
                let paren_content_start = abs_after_keyword;
                let (paren_content, after_paren) = scan_parenthesized(source, paren_content_start)?;
                let locals = current_locals(scopes);
                for part in paren_content.split(';') {
                    let part = part.trim();
                    if let Some(pos) = part.find("when") {
                        let after_when = &part[pos + 4..];
                        let expr = after_when.trim();
                        if !expr.is_empty() {
                            collect_expression_refs(expr, &locals, refs);
                        }
                    }
                }
                scopes.push(Vec::new());
                let brace_pos = source[after_paren..].find('{')?;
                Some(after_paren + brace_pos + 1)
            } else {
                scopes.push(Vec::new());
                let rest_from = start + 1 + keyword_end;
                let brace_pos = source[rest_from..].find('{')?;
                Some(rest_from + brace_pos + 1)
            }
        }
        "let" => {
            let after_keyword = &source[start + 1 + keyword_end..];
            let trimmed = after_keyword.trim_start();
            let offset = after_keyword.len() - trimmed.len();

            let name_end = trimmed.find(|c: char| !c.is_ascii_alphanumeric() && c != '_')?;
            let var_name = &trimmed[..name_end];

            let rest_after_name = &trimmed[name_end..];
            let eq_pos = rest_after_name.find('=')?;
            let expr_start = eq_pos + 1;
            let expr_rest = &rest_after_name[expr_start..];

            let semi_pos = expr_rest.find(';')?;
            let expr = expr_rest[..semi_pos].trim();

            let locals = current_locals(scopes);
            collect_expression_refs(expr, &locals, refs);

            if let Some(scope) = scopes.last_mut() {
                scope.push(var_name.to_string());
            }

            let abs_semi = start + 1 + keyword_end + offset + name_end + expr_start + semi_pos + 1;
            Some(abs_semi)
        }
        "else" => {
            let rest_from = start + 1 + keyword_end;
            let after_else = source[rest_from..].trim_start();
            let trimmed_offset = source[rest_from..].len() - after_else.len();

            if after_else.starts_with("if")
                && !after_else
                    .as_bytes()
                    .get(2)
                    .is_some_and(|b| b.is_ascii_alphanumeric())
            {
                let if_keyword_end = rest_from + trimmed_offset + 2;
                let after_if = &source[if_keyword_end..];
                let paren_start = after_if.find('(')?;
                let paren_content_start = if_keyword_end + paren_start;
                let (expr, after_paren) = scan_parenthesized(source, paren_content_start)?;
                let locals = current_locals(scopes);
                collect_expression_refs(expr.trim(), &locals, refs);
                scopes.push(Vec::new());
                let brace_pos = source[after_paren..].find('{')?;
                Some(after_paren + brace_pos + 1)
            } else {
                scopes.push(Vec::new());
                let brace_pos = source[rest_from..].find('{')?;
                Some(rest_from + brace_pos + 1)
            }
        }
        "empty" | "default" | "placeholder" | "loading" | "error" => {
            scopes.push(Vec::new());
            let rest_from = start + 1 + keyword_end;
            let brace_pos = source[rest_from..].find('{')?;
            Some(rest_from + brace_pos + 1)
        }
        _ => None,
    }
}

/// Parse the parenthesized content of an `@if (...)` block, splitting off the
/// optional `; as alias` clause. Returns `(condition, alias_name_if_any)`.
///
/// Angular 17+ supports `@if (expression; as alias) { ... }` to bind the truthy
/// result of the condition to a template-local variable usable inside the block.
fn parse_if_condition_and_alias(content: &str) -> (&str, Option<&str>) {
    let Some(semi_pos) = find_top_level_semicolon(content) else {
        return (content, None);
    };
    let cond = &content[..semi_pos];
    let rest = content[semi_pos + 1..].trim_start();
    let Some(after_as) = rest.strip_prefix("as").and_then(|tail| {
        let first = tail.chars().next()?;
        if first.is_whitespace() {
            Some(tail.trim_start())
        } else {
            None
        }
    }) else {
        return (cond, None);
    };
    let alias = after_as
        .split(|c: char| c.is_whitespace() || c == ';')
        .find(|s| !s.is_empty())
        .unwrap_or("");
    if alias.is_empty() {
        return (cond, None);
    }
    (cond, Some(alias))
}

/// Find the byte position of the first top-level `;` in `s`, ignoring
/// semicolons nested inside parens, brackets, braces, or string literals.
fn find_top_level_semicolon(s: &str) -> Option<usize> {
    let bytes = s.as_bytes();
    let mut depth = 0u32;
    let mut in_string: Option<u8> = None;
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if let Some(quote) = in_string {
            if b == b'\\' {
                i += 2;
                continue;
            }
            if b == quote {
                in_string = None;
            }
            i += 1;
            continue;
        }
        match b {
            b'\'' | b'"' | b'`' => in_string = Some(b),
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => depth = depth.saturating_sub(1),
            b';' if depth == 0 => return Some(i),
            _ => {}
        }
        i += 1;
    }
    None
}

/// Scan a parenthesized expression starting at the `(` character.
/// Returns `(content, index_after_closing_paren)`.
fn scan_parenthesized(source: &str, start: usize) -> Option<(&str, usize)> {
    let bytes = source.as_bytes();
    if bytes.get(start) != Some(&b'(') {
        return None;
    }
    let mut depth = 1u32;
    let mut i = start + 1;
    while i < bytes.len() && depth > 0 {
        match bytes[i] {
            b'(' => depth += 1,
            b')' => depth -= 1,
            _ => {}
        }
        if depth > 0 {
            i += 1;
        }
    }
    if depth == 0 {
        Some((&source[start + 1..i], i + 1))
    } else {
        None
    }
}

/// Process an HTML tag, extracting Angular binding attributes.
/// `*ngFor` bindings are added to the current scope so subsequent expressions see them.
fn process_tag(
    tag: &str,
    tag_start: usize,
    scopes: &mut [Vec<String>],
    refs: &mut AngularTemplateRefs,
) {
    let locals = current_locals(scopes);

    for caps in ATTR_RE.captures_iter(tag) {
        let attr_name = caps.get(1).map_or("", |m| m.as_str());
        let attr_value = caps.get(2).map_or("", |m| m.as_str()).trim();

        if attr_value.is_empty() {
            continue;
        }

        if attr_name == "[innerHTML]"
            && let Some(attr) = caps.get(0)
            && let Some(sink) = crate::template_usage::template_html_sink(
                attr_value,
                tag_start + attr.start(),
                tag_start + attr.end(),
            )
        {
            refs.security_sinks.push(sink);
        }

        if attr_name.starts_with('[') && !attr_name.starts_with("[(") {
            collect_expression_refs(attr_value, &locals, refs);
            continue;
        }

        if attr_name.starts_with('(') {
            collect_statement_refs(attr_value, &locals, refs);
            continue;
        }

        if attr_name.starts_with("[(") {
            collect_expression_refs(attr_value, &locals, refs);
            continue;
        }

        if attr_name == "*ngIf" || attr_name == "*ngShow" || attr_name == "*ngSwitch" {
            let expr = attr_value.split(';').next().unwrap_or(attr_value).trim();
            collect_expression_refs(expr, &locals, refs);
            continue;
        }

        if attr_name == "*ngFor" {
            handle_ng_for(attr_value, &locals, scopes, refs);
            continue;
        }

        if attr_name.starts_with('*') {
            collect_expression_refs(attr_value, &locals, refs);
            continue;
        }

        if attr_name.starts_with("bind-") {
            collect_expression_refs(attr_value, &locals, refs);
            continue;
        }

        if attr_name.starts_with("on-") {
            collect_statement_refs(attr_value, &locals, refs);
        }
    }
}

/// Handle `*ngFor` microsyntax. Pushes bindings into the scope so subsequent
/// expressions within the element see them as locals.
fn handle_ng_for(
    value: &str,
    locals: &[String],
    scopes: &mut [Vec<String>],
    refs: &mut AngularTemplateRefs,
) {
    let clauses: Vec<&str> = value.split(';').collect();

    let mut ng_for_locals = locals.to_vec();
    let mut new_scope_locals = Vec::new();

    for clause in &clauses {
        let clause = clause.trim();

        if let Some(caps) = NG_FOR_OF_RE.captures(clause) {
            let binding = caps.get(1).map_or("", |m| m.as_str());
            ng_for_locals.push(binding.to_string());
            new_scope_locals.push(binding.to_string());
            let iterable = caps.get(2).map_or("", |m| m.as_str()).trim();
            collect_expression_refs(iterable, &ng_for_locals, refs);
            continue;
        }

        if let Some(rest) = clause.strip_prefix("let ") {
            if let Some(eq_pos) = rest.find('=') {
                let name = rest[..eq_pos].trim();
                ng_for_locals.push(name.to_string());
                new_scope_locals.push(name.to_string());
            }
            continue;
        }

        if let Some(rest) = clause.strip_prefix("trackBy:") {
            collect_expression_refs(rest.trim(), &ng_for_locals, refs);
        }
    }

    if let Some(scope) = scopes.last_mut() {
        scope.extend(new_scope_locals);
    }
}

/// Collect unresolved identifier references from an expression, handling Angular pipes.
fn collect_expression_refs(expr: &str, locals: &[String], refs: &mut AngularTemplateRefs) {
    if expr.is_empty() {
        return;
    }

    let (main_expr, pipe_names) = split_pipes(expr);
    let (unresolved, member_accesses) =
        collect_unresolved_refs_and_accesses(main_expr, TemplateSnippetKind::Expression, locals);
    refs.identifiers.extend(unresolved);
    for access in member_accesses {
        refs.add_member_access(access);
    }

    for pipe_name in pipe_names {
        if !pipe_name.is_empty() {
            refs.identifiers.insert(pipe_name.to_string());
        }
    }
}

/// Collect unresolved identifier references from a statement (event handler).
fn collect_statement_refs(stmt: &str, locals: &[String], refs: &mut AngularTemplateRefs) {
    if stmt.is_empty() {
        return;
    }
    let (unresolved, member_accesses) =
        collect_unresolved_refs_and_accesses(stmt, TemplateSnippetKind::Statement, locals);
    refs.identifiers.extend(unresolved);
    for access in member_accesses {
        refs.add_member_access(access);
    }
}

/// Split an Angular expression on top-level pipe operators (`|`).
/// Returns the main expression and a list of pipe names.
/// Correctly distinguishes pipes from logical OR (`||`).
fn split_pipes(expr: &str) -> (&str, Vec<&str>) {
    let bytes = expr.as_bytes();
    let mut pipe_positions = Vec::new();
    let mut i = 0;
    let mut depth = 0u32; // parens/brackets/braces nesting
    let mut in_string: Option<u8> = None;

    while i < bytes.len() {
        let b = bytes[i];

        if let Some(quote) = in_string {
            if b == b'\\' {
                i += 2; // skip escape
                continue;
            }
            if b == quote {
                in_string = None;
            }
            i += 1;
            continue;
        }

        match b {
            b'\'' | b'"' | b'`' => in_string = Some(b),
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => depth = depth.saturating_sub(1),
            b'|' if depth == 0 => {
                let prev_is_pipe = i > 0 && bytes[i - 1] == b'|';
                let next_is_pipe = i + 1 < bytes.len() && bytes[i + 1] == b'|';
                if !prev_is_pipe && !next_is_pipe {
                    pipe_positions.push(i);
                }
            }
            _ => {}
        }
        i += 1;
    }

    if pipe_positions.is_empty() {
        return (expr, Vec::new());
    }

    let main_expr = expr[..pipe_positions[0]].trim();
    let mut pipes = Vec::new();
    for (j, &pos) in pipe_positions.iter().enumerate() {
        let end = pipe_positions.get(j + 1).copied().unwrap_or(expr.len());
        let pipe_part = expr[pos + 1..end].trim();
        let name = pipe_part.split(':').next().unwrap_or("").trim();
        if !name.is_empty() {
            pipes.push(name);
        }
    }

    (main_expr, pipes)
}

fn current_locals(scopes: &[Vec<String>]) -> Vec<String> {
    scopes.iter().flat_map(|s| s.iter().cloned()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interpolation_extracts_refs() {
        let refs = collect_angular_template_refs("<p>{{ title() }}</p>");
        assert!(refs.contains("title"));
    }

    #[test]
    fn property_binding_extracts_refs() {
        let refs =
            collect_angular_template_refs(r#"<p [class.highlighted]="isHighlighted">text</p>"#);
        assert!(refs.contains("isHighlighted"));
    }

    #[test]
    fn event_binding_extracts_refs() {
        let refs =
            collect_angular_template_refs(r#"<button (click)="onButtonClick()">Click</button>"#);
        assert!(refs.contains("onButtonClick"));
    }

    #[test]
    fn two_way_binding_extracts_refs() {
        let refs = collect_angular_template_refs(r#"<input [(ngModel)]="userName">"#);
        assert!(refs.contains("userName"));
    }

    #[test]
    fn ng_if_extracts_refs() {
        let refs = collect_angular_template_refs(r#"<div *ngIf="isLoading()">Loading</div>"#);
        assert!(refs.contains("isLoading"));
    }

    #[test]
    fn ng_for_extracts_iterable_not_binding() {
        let refs = collect_angular_template_refs(
            r#"<li *ngFor="let item of items; trackBy: trackByFn">{{ item }}</li>"#,
        );
        assert!(refs.contains("items"), "should contain iterable 'items'");
        assert!(
            refs.contains("trackByFn"),
            "should contain trackBy function"
        );
        assert!(!refs.contains("item"), "binding 'item' should be a local");
    }

    #[test]
    fn control_flow_if_extracts_refs() {
        let refs = collect_angular_template_refs(r"@if (isLoading()) { <div>Loading</div> }");
        assert!(refs.contains("isLoading"));
    }

    #[test]
    fn control_flow_else_if_extracts_refs() {
        let refs = collect_angular_template_refs(
            r"@if (condA) { <p>A</p> } @else if (condB) { <p>B</p> } @else { <p>C</p> }",
        );
        assert!(refs.contains("condA"), "should contain @if condition");
        assert!(refs.contains("condB"), "should contain @else if condition");
    }

    #[test]
    fn control_flow_chained_else_if_extracts_refs() {
        let refs = collect_angular_template_refs(
            r"@if (a) { <p>{{ x }}</p> } @else if (b) { <p>{{ y }}</p> } @else if (c) { <p>{{ z }}</p> }",
        );
        assert!(refs.contains("a"));
        assert!(refs.contains("b"));
        assert!(refs.contains("c"));
        assert!(refs.contains("x"));
        assert!(refs.contains("y"));
        assert!(refs.contains("z"));
    }

    #[test]
    fn control_flow_for_extracts_refs() {
        let refs = collect_angular_template_refs(
            r"@for (item of items; track item.id) { <li>{{ item.name }}</li> }",
        );
        assert!(refs.contains("items"), "should contain iterable");
        assert!(!refs.contains("item"), "binding should be a local");
    }

    #[test]
    fn control_flow_switch_extracts_refs() {
        let refs = collect_angular_template_refs(
            r#"@switch (status) { @case ("active") { <span>Active</span> } }"#,
        );
        assert!(refs.contains("status"));
    }

    #[test]
    fn pipe_extracts_name() {
        let refs = collect_angular_template_refs("<p>{{ birthday | date:'short' }}</p>");
        assert!(refs.contains("birthday"));
        assert!(refs.contains("date"));
    }

    #[test]
    fn logical_or_not_confused_with_pipe() {
        let refs = collect_angular_template_refs("<p>{{ a || b }}</p>");
        assert!(refs.contains("a"));
        assert!(refs.contains("b"));
    }

    #[test]
    fn html_comments_stripped() {
        let refs = collect_angular_template_refs("<!-- {{ hidden }} -->\n<p>{{ visible }}</p>");
        assert!(refs.contains("visible"));
        assert!(!refs.contains("hidden"));
    }

    #[test]
    fn empty_template_returns_empty() {
        let refs = collect_angular_template_refs("");
        assert!(refs.is_empty());
    }

    #[test]
    fn full_angular_template() {
        let refs = collect_angular_template_refs(
            r#"<h1>{{ title() }}</h1>
<p [class.highlighted]="isHighlighted">{{ greeting() }}</p>
@if (isLoading()) { <div>Loading...</div> }
<button (click)="onButtonClick()">Toggle</button>
<button (click)="addItem()">Add</button>
@for (item of items; track item) { <li>{{ item }}</li> }"#,
        );
        assert!(refs.contains("title"));
        assert!(refs.contains("isHighlighted"));
        assert!(refs.contains("greeting"));
        assert!(refs.contains("isLoading"));
        assert!(refs.contains("onButtonClick"));
        assert!(refs.contains("addItem"));
        assert!(refs.contains("items"));
        assert!(!refs.contains("item"));
    }

    #[test]
    fn bare_less_than_in_text_does_not_abort_scanner() {
        let refs = collect_angular_template_refs("count < 10\n<p>{{ title() }}</p>");
        assert!(refs.contains("title"), "refs after bare < should be found");
    }

    #[test]
    fn control_flow_with_object_literal_in_expression() {
        let refs =
            collect_angular_template_refs(r"@if (config.enabled) { <span>{{ label }}</span> }");
        assert!(refs.contains("config"));
        assert!(refs.contains("label"));
    }

    #[test]
    fn defer_when_extracts_refs() {
        let refs = collect_angular_template_refs(
            r"@defer (when isDataReady) { <app-heavy /> } @placeholder { <p>Wait</p> }",
        );
        assert!(refs.contains("isDataReady"));
    }

    #[test]
    fn defer_on_and_when_extracts_refs() {
        let refs =
            collect_angular_template_refs(r"@defer (on viewport; when isReady) { <app-heavy /> }");
        assert!(refs.contains("isReady"));
    }

    #[test]
    fn defer_on_timer_with_nested_parens() {
        let refs = collect_angular_template_refs(
            r"@defer (on timer(1s); when isReady) { <app-heavy /> } @placeholder { <p>{{ label }}</p> }",
        );
        assert!(
            refs.contains("isReady"),
            "when condition through nested parens"
        );
        assert!(refs.contains("label"), "content after defer block");
    }

    #[test]
    fn defer_prefetch_when_extracts_refs() {
        let refs = collect_angular_template_refs(
            r"@defer (prefetch when shouldPrefetch) { <app-heavy /> }",
        );
        assert!(refs.contains("shouldPrefetch"));
    }

    #[test]
    fn defer_without_condition() {
        let refs = collect_angular_template_refs(
            r"@defer { <app-heavy /> } @placeholder { <p>{{ label }}</p> }",
        );
        assert!(refs.contains("label"));
    }

    #[test]
    fn let_extracts_expression_refs() {
        let refs = collect_angular_template_refs(
            r"@let fullName = firstName + ' ' + lastName;
            <p>{{ fullName }}</p>",
        );
        assert!(refs.contains("firstName"));
        assert!(refs.contains("lastName"));
        assert!(!refs.contains("fullName"));
    }

    #[test]
    fn let_simple_alias() {
        let refs = collect_angular_template_refs(
            r"@let name = user.name;
            <p>{{ name }}</p>",
        );
        assert!(refs.contains("user"));
        assert!(!refs.contains("name"));
    }

    #[test]
    fn let_with_pipe() {
        let refs = collect_angular_template_refs(
            r"@let formatted = rawDate | date;
            <span>{{ formatted }}</span>",
        );
        assert!(refs.contains("rawDate"));
        assert!(refs.contains("date"));
        assert!(!refs.contains("formatted"));
    }

    #[test]
    fn split_pipes_no_pipe() {
        let (expr, pipes) = split_pipes("foo.bar");
        assert_eq!(expr, "foo.bar");
        assert!(pipes.is_empty());
    }

    #[test]
    fn split_pipes_single_pipe() {
        let (expr, pipes) = split_pipes("value | date");
        assert_eq!(expr, "value");
        assert_eq!(pipes, vec!["date"]);
    }

    #[test]
    fn split_pipes_with_args() {
        let (expr, pipes) = split_pipes("value | date:'short'");
        assert_eq!(expr, "value");
        assert_eq!(pipes, vec!["date"]);
    }

    #[test]
    fn split_pipes_multiple() {
        let (expr, pipes) = split_pipes("value | date:'short' | uppercase");
        assert_eq!(expr, "value");
        assert_eq!(pipes, vec!["date", "uppercase"]);
    }

    #[test]
    fn split_pipes_preserves_logical_or() {
        let (expr, pipes) = split_pipes("a || b");
        assert_eq!(expr, "a || b");
        assert!(pipes.is_empty());
    }

    #[test]
    fn split_pipes_inside_parens_not_split() {
        let (expr, pipes) = split_pipes("fn(a | b)");
        assert_eq!(expr, "fn(a | b)");
        assert!(pipes.is_empty());
    }

    #[test]
    fn interpolation_extracts_member_access_chain() {
        let refs = collect_angular_template_refs("<p>{{ dataService.getTotal() }}</p>");
        assert!(
            refs.identifiers.contains("dataService"),
            "top-level unresolved identifier must be captured"
        );
        let has_chain = refs
            .member_accesses
            .iter()
            .any(|a| a.object == "dataService" && a.member == "getTotal");
        assert!(
            has_chain,
            "member-access chain dataService.getTotal must be captured, got {:?}",
            refs.member_accesses
        );
    }

    #[test]
    fn control_flow_if_extracts_member_access_chain() {
        let refs =
            collect_angular_template_refs(r"@if (!dataService.isEmpty()) { <div>Items</div> }");
        assert!(refs.identifiers.contains("dataService"));
        let has_chain = refs
            .member_accesses
            .iter()
            .any(|a| a.object == "dataService" && a.member == "isEmpty");
        assert!(
            has_chain,
            "@if condition chain must be captured, got {:?}",
            refs.member_accesses
        );
    }

    #[test]
    fn control_flow_for_iterable_extracts_member_access_chain() {
        let refs = collect_angular_template_refs(
            r"@for (item of dataService.items; track item) { <li>{{ item }}</li> }",
        );
        assert!(refs.identifiers.contains("dataService"));
        let has_chain = refs
            .member_accesses
            .iter()
            .any(|a| a.object == "dataService" && a.member == "items");
        assert!(
            has_chain,
            "@for iterable chain must be captured, got {:?}",
            refs.member_accesses
        );
    }

    #[test]
    fn event_binding_extracts_member_access_chain() {
        let refs =
            collect_angular_template_refs(r#"<button (click)="svc.handleClick()">x</button>"#);
        assert!(refs.identifiers.contains("svc"));
        let has_chain = refs
            .member_accesses
            .iter()
            .any(|a| a.object == "svc" && a.member == "handleClick");
        assert!(
            has_chain,
            "event handler chain must be captured, got {:?}",
            refs.member_accesses
        );
    }

    #[test]
    fn local_binding_does_not_emit_member_chain() {
        let refs =
            collect_angular_template_refs(r#"<li *ngFor="let item of items">{{ item.name }}</li>"#);
        assert!(refs.identifiers.contains("items"));
        let has_local_chain = refs.member_accesses.iter().any(|a| a.object == "item");
        assert!(
            !has_local_chain,
            "chain on local binding must not be emitted, got {:?}",
            refs.member_accesses
        );
    }

    #[test]
    fn parse_if_alias_no_semicolon_returns_full_condition() {
        let (cond, alias) = parse_if_condition_and_alias("withAlias()");
        assert_eq!(cond, "withAlias()");
        assert_eq!(alias, None);
    }

    #[test]
    fn parse_if_alias_extracts_canonical_form() {
        let (cond, alias) = parse_if_condition_and_alias("withAlias(); as aliased");
        assert_eq!(cond, "withAlias()");
        assert_eq!(alias, Some("aliased"));
    }

    #[test]
    fn parse_if_alias_handles_nested_semicolon_in_string() {
        let (cond, alias) = parse_if_condition_and_alias("fn(';'); as result");
        assert_eq!(cond, "fn(';')");
        assert_eq!(alias, Some("result"));
    }

    #[test]
    fn parse_if_alias_handles_nested_semicolon_in_call() {
        let (cond, alias) = parse_if_condition_and_alias("fn(a; b); as result");
        assert_eq!(cond, "fn(a; b)");
        assert_eq!(alias, Some("result"));
    }

    #[test]
    fn parse_if_alias_unknown_tail_still_strips_condition() {
        let (cond, alias) = parse_if_condition_and_alias("cond; let foo = bar");
        assert_eq!(cond, "cond");
        assert_eq!(alias, None);
    }

    #[test]
    fn parse_if_alias_handles_newline_after_as() {
        let (cond, alias) = parse_if_condition_and_alias("svc.compute();\n  as\n  result");
        assert_eq!(cond.trim(), "svc.compute()");
        assert_eq!(alias, Some("result"));
    }

    #[test]
    fn parse_if_alias_handles_tab_between_as_and_name() {
        let (cond, alias) = parse_if_condition_and_alias("cond;\tas\tresult");
        assert_eq!(cond, "cond");
        assert_eq!(alias, Some("result"));
    }

    #[test]
    fn parse_if_alias_does_not_match_as_prefixed_identifier() {
        let (cond, alias) = parse_if_condition_and_alias("cond; aspect");
        assert_eq!(cond, "cond");
        assert_eq!(alias, None);
    }

    #[test]
    fn at_if_with_alias_extracts_condition_refs() {
        let refs = collect_angular_template_refs(
            r"@if (withAlias(); as aliased) { <p>{{ aliased }}</p> }",
        );
        assert!(
            refs.identifiers.contains("withAlias"),
            "@if condition with alias must still credit the call, got {:?}",
            refs.identifiers
        );
        assert!(
            !refs.identifiers.contains("aliased"),
            "alias name must not leak as a class-member ref, got {:?}",
            refs.identifiers
        );
    }

    #[test]
    fn at_if_with_alias_extracts_member_chain() {
        let refs = collect_angular_template_refs(
            r"@if (svc.compute(); as result) { <p>{{ result }}</p> }",
        );
        assert!(refs.identifiers.contains("svc"));
        let has_chain = refs
            .member_accesses
            .iter()
            .any(|a| a.object == "svc" && a.member == "compute");
        assert!(
            has_chain,
            "member-access chain in @if with alias must be captured, got {:?}",
            refs.member_accesses
        );
        assert!(!refs.identifiers.contains("result"));
    }

    #[test]
    fn at_if_alias_does_not_leak_outside_block() {
        let refs = collect_angular_template_refs(
            r"@if (a(); as result) { <p>{{ result }}</p> } <p>{{ result }}</p>",
        );
        assert!(refs.identifiers.contains("a"));
        assert!(
            refs.identifiers.contains("result"),
            "@if alias must not leak past its closing brace, got {:?}",
            refs.identifiers
        );
    }

    #[test]
    fn at_if_alias_with_object_literal_in_condition() {
        let refs = collect_angular_template_refs(
            r"@if (build({ key: value }); as built) { <p>{{ built }}</p> }",
        );
        assert!(refs.identifiers.contains("build"));
        assert!(refs.identifiers.contains("value"));
        assert!(!refs.identifiers.contains("built"));
    }
}
