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
//! Referenced bare identifiers are persisted as typed semantic facts, while
//! member-access chains keep their real object/member shape for typed-instance
//! propagation.

use std::sync::LazyLock;

use rustc_hash::{FxHashMap, FxHashSet};

use fallow_types::extract::SinkSite;

use crate::MemberAccess;
use crate::template_usage::{TemplateSnippetKind, collect_unresolved_refs_and_accesses};

use super::angular_text::{icu_message_head, mask_string_contents};
use super::recovery::RecoveryScanner;
use super::scanners::scan_curly_section;

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
    fn contains(&self, name: &str) -> bool {
        self.identifiers.contains(name)
    }

    /// Whether this template produced no refs or member accesses at all.
    #[cfg(test)]
    #[must_use]
    fn is_empty(&self) -> bool {
        self.identifiers.is_empty() && self.member_accesses.is_empty()
    }
}

/// Regex to strip HTML comments before scanning.
static HTML_COMMENT_RE: LazyLock<regex::Regex> =
    LazyLock::new(|| crate::static_regex(r"(?s)<!--.*?-->"));

/// Regex matching an opening HTML/Angular element tag name. Captures group 1 =
/// the tag name. Closing tags (`</foo>`) are excluded by the negative lookahead
/// is unavailable in `regex`, so the leading `[a-zA-Z]` after `<` (not `/`)
/// rejects them; only an opening tag's first byte after `<` is an ASCII letter.
static ELEMENT_TAG_RE: LazyLock<regex::Regex> =
    LazyLock::new(|| crate::static_regex(r"<([a-zA-Z][a-zA-Z0-9\-]*)"));

/// Collect custom element selector tag names referenced in an Angular template.
///
/// Returns the deduplicated set of opening-tag names that contain a hyphen
/// (`<app-foo>` -> `app-foo`). The hyphen requirement is the precise,
/// FP-resistant discriminator between an Angular component element selector
/// (which conventionally and, for custom elements, definitionally contains a
/// hyphen) and a native HTML element (no native tag name contains a hyphen). Tag
/// names are lowercased for case-insensitive matching against the declared
/// selector set. HTML comments are masked first so a `<app-foo>` inside a comment
/// is not credited.
///
/// First-cut scope is ELEMENT selectors only; attribute selectors
/// (`<div appFoo>`) are intentionally NOT harvested here, and the detector
/// abstains on any component with a non-element selector.
#[must_use]
pub fn collect_angular_used_selectors(source: &str) -> Vec<String> {
    let masked = strip_html_comments_preserve_offsets(source);
    let mut seen = FxHashSet::default();
    let mut out = Vec::new();
    for caps in ELEMENT_TAG_RE.captures_iter(&masked) {
        let Some(name) = caps.get(1) else {
            continue;
        };
        let tag = name.as_str();
        if !tag.contains('-') {
            continue;
        }
        let lowered = tag.to_ascii_lowercase();
        if seen.insert(lowered.clone()) {
            out.push(lowered);
        }
    }
    out
}

/// Regex to extract attribute name-value pairs from an HTML tag.
/// Captures: group 1 = attribute name (including prefix like `[`, `(`, `*`),
///           group 2 = value (inside quotes).
static ATTR_RE: LazyLock<regex::Regex> = LazyLock::new(|| {
    crate::static_regex(r#"(?s)([\[()*#a-zA-Z][\w.\-\[\]()%@]*)\s*=\s*"([^"]*)""#)
});

/// Regex for an explicit `this.member` read. Angular templates resolve `this`
/// to the component instance, so the member is a component reference.
static THIS_MEMBER_RE: LazyLock<regex::Regex> = LazyLock::new(|| {
    crate::static_regex(r"(?:^|[^\w$.])this\s*(?:\?\.|!?\s*\.)\s*([A-Za-z_$][\w$]*)")
});

/// Regex for a trailing `as alias` export in a structural directive clause
/// (`*ngIf="user$ | async as user"`). Captures group 1 = the expression.
static MICROSYNTAX_AS_RE: LazyLock<regex::Regex> =
    LazyLock::new(|| crate::static_regex(r"(?s)^(.*?)\s+as\s+[A-Za-z_$][\w$]*\s*$"));

/// Regex for a keyed structural directive clause (`context: ctx`,
/// `trackBy: fn`, `else tpl`). Captures group 1 = the expression.
static MICROSYNTAX_KEY_RE: LazyLock<regex::Regex> =
    LazyLock::new(|| crate::static_regex(r"(?s)^[A-Za-z_$][\w$]*\s*(?::\s*|\s+)(\S.*)$"));

/// Regex for a context export clause after the first clause (`index as i`).
/// The key names a template context variable, not a component member.
static MICROSYNTAX_EXPORT_RE: LazyLock<regex::Regex> =
    LazyLock::new(|| crate::static_regex(r"^[A-Za-z_$][\w$]*\s+as\s+[A-Za-z_$][\w$]*$"));

/// Marks a scope entry as a loop item that the scanner keeps out of the parser
/// locals, so that member chains on the item stay visible for typed remapping.
/// A bare read of the item inside its loop is still not a component reference.
const LOOP_ITEM_MARKER: char = '\u{1}';

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
    collect_angular_template_refs_inner(source, &FxHashMap::default(), true)
}

/// As [`collect_angular_template_refs`], additionally typing each `@for` /
/// `*ngFor` loop variable whose source iterable is a component field in
/// `component_field_array_types` (`utils: Util[]`) to that element class, so
/// member accesses on the loop item (`{{ util.getName() }}`) are re-emitted as
/// `Util.getName` member-access chains instead of dropped. Over-credit only.
/// See issue #1712.
pub fn collect_angular_template_refs_with_field_types(
    source: &str,
    component_field_array_types: &FxHashMap<String, String>,
) -> AngularTemplateRefs {
    collect_angular_template_refs_inner(source, component_field_array_types, false)
}

fn collect_angular_template_refs_inner(
    source: &str,
    component_field_array_types: &FxHashMap<String, String>,
    preserve_untyped_iteration_sources: bool,
) -> AngularTemplateRefs {
    let source = strip_html_comments_preserve_offsets(source);
    AngularTemplateScanner::new(
        &source,
        component_field_array_types,
        preserve_untyped_iteration_sources,
    )
    .scan()
}

struct AngularTemplateScanner<'a> {
    source: &'a str,
    recovery: RecoveryScanner<'a>,
    refs: AngularTemplateRefs,
    scopes: Vec<Vec<String>>,
    /// Component field name -> array element class, so a `@for` / `*ngFor` over a
    /// bare-identifier component field types the loop variable to the element
    /// class (issue #1712).
    component_field_array_types: &'a FxHashMap<String, String>,
    /// Loop variable name -> element class, populated as `@for` / `*ngFor`
    /// blocks are entered. A member access on such a loop variable is remapped
    /// onto the element class in the finalize pass.
    iteration_element_types: FxHashMap<String, String>,
    /// Loop variable name -> iterable field name for external templates that do
    /// not know component field types during scanning. A member access on the
    /// item is remapped onto the iterable field (`util.getName` -> `utils.getName`)
    /// so core can resolve `utils -> Util` from the owning component.
    iteration_iterable_sources: FxHashMap<String, String>,
    preserve_untyped_iteration_sources: bool,
    /// Scope depth of each open ICU message. Inside an ICU message, outside
    /// its case bodies, every `{` opens a case, also when `{{` follows it.
    icu_message_depths: Vec<usize>,
    /// Set after the first `{{` without a closing `}}`. A scan for the end of
    /// each later `{{` would read to the end of the template again, which is
    /// quadratic. Angular rejects such a template, so its later
    /// interpolations are not read.
    interpolation_unterminated: bool,
}

impl<'a> AngularTemplateScanner<'a> {
    fn new(
        source: &'a str,
        component_field_array_types: &'a FxHashMap<String, String>,
        preserve_untyped_iteration_sources: bool,
    ) -> Self {
        Self {
            source,
            recovery: RecoveryScanner::new(source),
            refs: AngularTemplateRefs::default(),
            scopes: vec![Vec::new()],
            component_field_array_types,
            iteration_element_types: FxHashMap::default(),
            iteration_iterable_sources: FxHashMap::default(),
            preserve_untyped_iteration_sources,
            icu_message_depths: Vec::new(),
            interpolation_unterminated: false,
        }
    }

    fn scan(mut self) -> AngularTemplateRefs {
        let mut index = 0;
        while index < self.source.len() {
            index = self.scan_next(index);
        }
        remap_iteration_member_accesses(
            &mut self.refs,
            &self.iteration_element_types,
            &self.iteration_iterable_sources,
        );
        self.refs
    }

    fn scan_next(&mut self, index: usize) -> usize {
        let bytes = self.source.as_bytes();
        // Angular reads `other {{{n}} items}` as a case brace followed by an
        // interpolation, but `{{{a: 1}.a}}` outside an ICU message as an
        // interpolation that starts with an object literal.
        if starts_interpolation(bytes, index) && !self.at_icu_case_level() {
            return self.scan_interpolation(index);
        }

        match bytes[index] {
            b'@' => self.scan_control_flow(index),
            b'{' => self.open_brace(index),
            b'}' => self.close_scope(index),
            b'<' => self.scan_tag(index),
            _ => index + 1,
        }
    }

    fn scan_interpolation(&mut self, index: usize) -> usize {
        // An unterminated `{{` must not hide the tags after it.
        if self.interpolation_unterminated {
            return index + 2;
        }
        let Some((expr, next_index)) = scan_curly_section(self.source, index, 2, 2) else {
            self.interpolation_unterminated = true;
            return index + 2;
        };
        collect_expression_refs(expr.trim(), &current_locals(&self.scopes), &mut self.refs);
        next_index
    }

    fn scan_control_flow(&mut self, index: usize) -> usize {
        let mut iter_ctx = IterationTypingCtx {
            component_field_array_types: self.component_field_array_types,
            iteration_element_types: &mut self.iteration_element_types,
            iteration_iterable_sources: &mut self.iteration_iterable_sources,
            preserve_untyped_iteration_sources: self.preserve_untyped_iteration_sources,
        };
        handle_control_flow(
            self.source,
            index,
            &mut self.scopes,
            &mut self.refs,
            &mut iter_ctx,
        )
        .unwrap_or(index + 1)
    }

    fn at_icu_case_level(&self) -> bool {
        self.icu_message_depths.last() == Some(&self.scopes.len())
    }

    /// A bare `{` in Angular text opens an ICU message or an ICU case
    /// (`{n, plural, =1 {one}}`). Its own scope keeps the matching `}` from
    /// closing an enclosing control-flow block. Angular reads the head of an
    /// ICU message as a component expression.
    fn open_brace(&mut self, index: usize) -> usize {
        let icu_head = if self.at_icu_case_level() {
            None
        } else {
            icu_message_head(self.source, index)
        };
        let Some((head, next_index)) = icu_head else {
            self.scopes.push(Vec::new());
            return index + 1;
        };
        collect_expression_refs(head.trim(), &current_locals(&self.scopes), &mut self.refs);
        self.scopes.push(Vec::new());
        self.icu_message_depths.push(self.scopes.len());
        next_index
    }

    fn close_scope(&mut self, index: usize) -> usize {
        // `>=` also drops the entry of an ICU message that a template error
        // left open when an enclosing block closes.
        while self
            .icu_message_depths
            .last()
            .is_some_and(|&depth| depth >= self.scopes.len())
        {
            self.icu_message_depths.pop();
        }
        if self.scopes.len() > 1 {
            self.scopes.pop();
        }
        index + 1
    }

    fn scan_tag(&mut self, index: usize) -> usize {
        let Some((tag, next_index)) = self.recovery.html_tag(index) else {
            return index + 1;
        };
        let mut iter_ctx = IterationTypingCtx {
            component_field_array_types: self.component_field_array_types,
            iteration_element_types: &mut self.iteration_element_types,
            iteration_iterable_sources: &mut self.iteration_iterable_sources,
            preserve_untyped_iteration_sources: self.preserve_untyped_iteration_sources,
        };
        process_tag(tag, index, &mut self.scopes, &mut self.refs, &mut iter_ctx);
        next_index
    }
}

/// Carries the component-field element-type table (`utils` -> `Util`) plus the
/// mutable loop-variable typing accumulator (`util` -> `Util`) through the
/// `@for` / `*ngFor` code paths so a bare-identifier loop over a typed array
/// field types its item to the element class (issue #1712).
struct IterationTypingCtx<'a> {
    component_field_array_types: &'a FxHashMap<String, String>,
    iteration_element_types: &'a mut FxHashMap<String, String>,
    iteration_iterable_sources: &'a mut FxHashMap<String, String>,
    preserve_untyped_iteration_sources: bool,
}

impl IterationTypingCtx<'_> {
    fn preserves_untyped_iteration_sources(&self) -> bool {
        self.preserve_untyped_iteration_sources
    }

    /// If `iterable` is a bare-identifier component field with a known element
    /// class, record `binding -> class` and return `true` so the caller can keep
    /// the loop variable OUT of the block locals (its member accesses then
    /// survive the scan and remap onto the element class). A destructured or
    /// non-identifier binding, or an unknown / member-expression iterable, is
    /// left untyped (`false`). First-write-wins on a repeated loop-variable name.
    fn try_type_loop_variable(&mut self, binding: &str, iterable: &str) -> bool {
        let binding = binding.trim();
        let iterable = iterable.trim();
        if binding.is_empty() || !is_bare_identifier(binding) {
            return false;
        }
        let Some(class) = self.component_field_array_types.get(iterable) else {
            if self.preserve_untyped_iteration_sources {
                self.iteration_iterable_sources
                    .entry(binding.to_string())
                    .or_insert_with(|| iterable.to_string());
            }
            return false;
        };
        self.iteration_element_types
            .entry(binding.to_string())
            .or_insert_with(|| class.clone());
        true
    }
}

/// Whether `name` is a single bare JS identifier (no member access, no
/// destructure). The `@for` / `*ngFor` binding must be a plain name to type it
/// to the element class.
fn is_bare_identifier(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    matches!(first, 'A'..='Z' | 'a'..='z' | '_' | '$')
        && chars.all(|ch| matches!(ch, 'A'..='Z' | 'a'..='z' | '0'..='9' | '_' | '$'))
}

/// Remap each collected member access whose object is a typed iteration variable
/// onto its element class. A `util.getName` access becomes `Util.getName`.
/// Deduplicates so a remapped access already present as `Util.getName` is not
/// doubled.
///
/// Bare reads of the loop item are filtered while the loop scope is open (see
/// [`LOOP_ITEM_MARKER`]). A component member with the same name, read outside
/// the loop, stays a reference.
fn remap_iteration_member_accesses(
    refs: &mut AngularTemplateRefs,
    iteration_element_types: &FxHashMap<String, String>,
    iteration_iterable_sources: &FxHashMap<String, String>,
) {
    if iteration_element_types.is_empty() && iteration_iterable_sources.is_empty() {
        return;
    }
    for access in &mut refs.member_accesses {
        if let Some(class) = iteration_element_types.get(&access.object) {
            access.object = class.clone();
        } else if let Some(iterable) = iteration_iterable_sources.get(&access.object) {
            access.object = iterable.clone();
        }
    }
    dedup_member_accesses(&mut refs.member_accesses);
}

fn loop_item_local(binding: &str) -> String {
    let mut marked = String::with_capacity(binding.len() + 1);
    marked.push(LOOP_ITEM_MARKER);
    marked.push_str(binding.trim());
    marked
}

/// In-place dedup of `(object, member)` member-access pairs, preserving order.
fn dedup_member_accesses(accesses: &mut Vec<MemberAccess>) {
    let mut seen: FxHashSet<(String, String)> = FxHashSet::default();
    accesses.retain(|access| seen.insert((access.object.clone(), access.member.clone())));
}

fn starts_interpolation(bytes: &[u8], index: usize) -> bool {
    index + 1 < bytes.len() && bytes[index] == b'{' && bytes[index + 1] == b'{'
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
    iter_ctx: &mut IterationTypingCtx<'_>,
) -> Option<usize> {
    let rest = &source[start + 1..]; // skip '@'

    let keyword_end = rest.find(|c: char| !c.is_ascii_alphabetic())?;
    let keyword = &rest[..keyword_end];

    match keyword {
        "if" => handle_if_control_flow(source, start, keyword_end, scopes, refs),
        "switch" | "case" => {
            handle_parenthesized_block_control_flow(source, start, keyword_end, scopes, refs)
        }
        "for" => handle_for_control_flow(source, start, keyword_end, scopes, refs, iter_ctx),
        "defer" => handle_defer_control_flow(source, start, keyword_end, scopes, refs),
        "let" => handle_let_control_flow(source, start, keyword_end, scopes, refs),
        "else" => handle_else_control_flow(source, start, keyword_end, scopes, refs),
        "empty" | "default" | "placeholder" | "loading" | "error" => {
            push_empty_control_flow_scope(source, start, keyword_end, scopes)
        }
        _ => None,
    }
}

fn handle_parenthesized_block_control_flow(
    source: &str,
    start: usize,
    keyword_end: usize,
    scopes: &mut Vec<Vec<String>>,
    refs: &mut AngularTemplateRefs,
) -> Option<usize> {
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

fn handle_defer_control_flow(
    source: &str,
    start: usize,
    keyword_end: usize,
    scopes: &mut Vec<Vec<String>>,
    refs: &mut AngularTemplateRefs,
) -> Option<usize> {
    let after_keyword = &source[start + 1 + keyword_end..];
    let trimmed = after_keyword.trim_start();
    let offset = after_keyword.len() - trimmed.len();
    let abs_after_keyword = start + 1 + keyword_end + offset;

    if !trimmed.starts_with('(') {
        return push_empty_control_flow_scope(source, start, keyword_end, scopes);
    }

    let (paren_content, after_paren) = scan_parenthesized(source, abs_after_keyword)?;
    let locals = current_locals(scopes);
    collect_defer_when_refs(paren_content, &locals, refs);
    scopes.push(Vec::new());
    let brace_pos = source[after_paren..].find('{')?;
    Some(after_paren + brace_pos + 1)
}

fn collect_defer_when_refs(paren_content: &str, locals: &[String], refs: &mut AngularTemplateRefs) {
    for part in paren_content.split(';') {
        let part = part.trim();
        if let Some(pos) = part.find("when") {
            let expr = part[pos + 4..].trim();
            if !expr.is_empty() {
                collect_expression_refs(expr, locals, refs);
            }
        }
    }
}

fn handle_let_control_flow(
    source: &str,
    start: usize,
    keyword_end: usize,
    scopes: &mut [Vec<String>],
    refs: &mut AngularTemplateRefs,
) -> Option<usize> {
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

fn handle_else_control_flow(
    source: &str,
    start: usize,
    keyword_end: usize,
    scopes: &mut Vec<Vec<String>>,
    refs: &mut AngularTemplateRefs,
) -> Option<usize> {
    let rest_from = start + 1 + keyword_end;
    let after_else = source[rest_from..].trim_start();
    let trimmed_offset = source[rest_from..].len() - after_else.len();

    if is_else_if(after_else) {
        let if_keyword_end = rest_from + trimmed_offset + 2;
        return handle_else_if_control_flow(source, if_keyword_end, scopes, refs);
    }

    scopes.push(Vec::new());
    let brace_pos = source[rest_from..].find('{')?;
    Some(rest_from + brace_pos + 1)
}

fn is_else_if(after_else: &str) -> bool {
    after_else.starts_with("if")
        && !after_else
            .as_bytes()
            .get(2)
            .is_some_and(|b| b.is_ascii_alphanumeric())
}

fn handle_else_if_control_flow(
    source: &str,
    if_keyword_end: usize,
    scopes: &mut Vec<Vec<String>>,
    refs: &mut AngularTemplateRefs,
) -> Option<usize> {
    let after_if = &source[if_keyword_end..];
    let paren_start = after_if.find('(')?;
    enter_if_block(source, if_keyword_end + paren_start, scopes, refs)
}

/// Collect the condition of an `@if (cond; as alias)` or `@else if (...)` block
/// whose `(` is at `paren_content_start`, and open the block scope with the
/// alias as a local. Returns the index after the opening `{`.
fn enter_if_block(
    source: &str,
    paren_content_start: usize,
    scopes: &mut Vec<Vec<String>>,
    refs: &mut AngularTemplateRefs,
) -> Option<usize> {
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

fn push_empty_control_flow_scope(
    source: &str,
    start: usize,
    keyword_end: usize,
    scopes: &mut Vec<Vec<String>>,
) -> Option<usize> {
    scopes.push(Vec::new());
    let rest_from = start + 1 + keyword_end;
    let brace_pos = source[rest_from..].find('{')?;
    Some(rest_from + brace_pos + 1)
}

fn handle_if_control_flow(
    source: &str,
    start: usize,
    keyword_end: usize,
    scopes: &mut Vec<Vec<String>>,
    refs: &mut AngularTemplateRefs,
) -> Option<usize> {
    let after_keyword = &source[start + 1 + keyword_end..];
    let paren_start = after_keyword.find('(')?;
    enter_if_block(source, start + 1 + keyword_end + paren_start, scopes, refs)
}

fn handle_for_control_flow(
    source: &str,
    start: usize,
    keyword_end: usize,
    scopes: &mut Vec<Vec<String>>,
    refs: &mut AngularTemplateRefs,
    iter_ctx: &mut IterationTypingCtx<'_>,
) -> Option<usize> {
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
        let iterable = caps.get(2).map_or("", |m| m.as_str()).trim();
        // When the loop variable types to a component-field element class (issue
        // #1712), keep it OUT of the block locals so `util.getName` survives as a
        // member access and remaps onto `Util` in the finalize pass; otherwise it
        // is a normal local shadowing any same-named component member.
        let typed = iter_ctx.try_type_loop_variable(binding, iterable);
        if !typed
            && (!is_bare_identifier(binding.trim())
                || !iter_ctx.preserves_untyped_iteration_sources())
        {
            locals_for_scope.push(binding.to_string());
        } else {
            locals_for_scope.push(loop_item_local(binding));
        }
        for implicit in &["$index", "$first", "$last", "$even", "$odd", "$count"] {
            locals_for_scope.push((*implicit).to_string());
        }
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
    iter_ctx: &mut IterationTypingCtx<'_>,
) {
    let locals = current_locals(scopes);

    for caps in ATTR_RE.captures_iter(tag) {
        if let Some(attr) = parse_template_attr(&caps, tag_start) {
            process_tag_attr(&attr, &locals, scopes, refs, iter_ctx);
        }
    }
}

struct AngularTemplateAttr<'a> {
    name: &'a str,
    value: &'a str,
    span: Option<(usize, usize)>,
}

fn parse_template_attr<'a>(
    caps: &'a regex::Captures<'_>,
    tag_start: usize,
) -> Option<AngularTemplateAttr<'a>> {
    let value = caps.get(2)?.as_str().trim();
    if value.is_empty() {
        return None;
    }

    let span = caps
        .get(0)
        .map(|attr| (tag_start + attr.start(), tag_start + attr.end()));

    Some(AngularTemplateAttr {
        name: caps.get(1).map_or("", |m| m.as_str()),
        value,
        span,
    })
}

fn process_tag_attr(
    attr: &AngularTemplateAttr<'_>,
    locals: &[String],
    scopes: &mut [Vec<String>],
    refs: &mut AngularTemplateRefs,
    iter_ctx: &mut IterationTypingCtx<'_>,
) {
    collect_inner_html_sink(attr, refs);

    if attr.name.starts_with('[') && !attr.name.starts_with("[(") {
        collect_expression_refs(attr.value, locals, refs);
        return;
    }

    if attr.name.starts_with('(') {
        collect_statement_refs(attr.value, locals, refs);
        return;
    }

    if attr.name.starts_with("[(") {
        collect_expression_refs(attr.value, locals, refs);
        return;
    }

    if matches!(attr.name, "*ngIf" | "*ngShow" | "*ngSwitch") {
        // `*ngIf="cond; else elseTpl"`: the `then` and `else` clauses can name
        // a component member that holds a `TemplateRef`.
        collect_microsyntax_refs(attr.value, locals, refs);
        return;
    }

    if attr.name == "*ngFor" {
        handle_ng_for(attr.value, locals, scopes, refs, iter_ctx);
        return;
    }

    if attr.name.starts_with('*') {
        collect_microsyntax_refs(attr.value, locals, refs);
        return;
    }

    if attr.name.starts_with("bind-") {
        collect_expression_refs(attr.value, locals, refs);
        return;
    }

    if attr.name.starts_with("on-") {
        collect_statement_refs(attr.value, locals, refs);
        return;
    }

    collect_attribute_interpolation_refs(attr.value, locals, refs);
}

/// Collect `{{ expr }}` interpolations inside a plain attribute value
/// (`title="{{ label }}"`, `class="base {{ extra }}"`). Angular compiles each
/// one into a property binding.
fn collect_attribute_interpolation_refs(
    value: &str,
    locals: &[String],
    refs: &mut AngularTemplateRefs,
) {
    let mut cursor = 0;
    while let Some(offset) = value[cursor..].find("{{") {
        let start = cursor + offset;
        let Some((expr, next)) = scan_curly_section(value, start, 2, 2) else {
            return;
        };
        collect_expression_refs(expr.trim(), locals, refs);
        cursor = next;
    }
}

/// Collect references from a structural directive microsyntax value
/// (`*ngTemplateOutlet="tpl; context: ctx"`). The first clause is the
/// directive expression. Each later clause is `key: expr`, `key expr`, or a
/// `let` declaration. A trailing `as alias` export is not an expression.
fn collect_microsyntax_refs(value: &str, locals: &[String], refs: &mut AngularTemplateRefs) {
    for (position, clause) in split_top_level_semicolons(value).into_iter().enumerate() {
        let clause = clause.trim();
        if let Some(caps) = NG_FOR_OF_RE.captures(clause) {
            // `let item of items` in a repeater such as `*cdkVirtualFor`.
            let mut iteration_locals = locals.to_vec();
            iteration_locals.push(caps.get(1).map_or("", |m| m.as_str()).to_string());
            let iterable = caps.get(2).map_or("", |m| m.as_str());
            collect_expression_refs(iterable.trim(), &iteration_locals, refs);
            continue;
        }
        if clause.is_empty()
            || clause.starts_with("let ")
            || (position > 0 && MICROSYNTAX_EXPORT_RE.is_match(clause))
        {
            continue;
        }
        let clause = MICROSYNTAX_AS_RE
            .captures(clause)
            .and_then(|caps| caps.get(1))
            .map_or(clause, |m| m.as_str());
        let expr = if position == 0 {
            clause
        } else {
            MICROSYNTAX_KEY_RE
                .captures(clause)
                .and_then(|caps| caps.get(1))
                .map_or(clause, |m| m.as_str())
        };
        collect_expression_refs(expr.trim(), locals, refs);
    }
}

fn split_top_level_semicolons(value: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut rest = value;
    while let Some(pos) = find_top_level_semicolon(rest) {
        parts.push(&rest[..pos]);
        rest = &rest[pos + 1..];
    }
    parts.push(rest);
    parts
}

fn collect_inner_html_sink(attr: &AngularTemplateAttr<'_>, refs: &mut AngularTemplateRefs) {
    let Some((start, end)) = attr.span else {
        return;
    };
    let sink = if attr.name == "[innerHTML]" {
        crate::template_usage::template_html_sink(attr.value, start, end)
    } else if attr.name.eq_ignore_ascii_case("[srcdoc]")
        || attr.name.eq_ignore_ascii_case("[attr.srcdoc]")
    {
        crate::template_usage::template_srcdoc_sink(attr.value, start, end)
    } else {
        None
    };
    if let Some(sink) = sink {
        refs.security_sinks.push(sink);
    }
}

/// Handle `*ngFor` microsyntax. Pushes bindings into the scope so subsequent
/// expressions within the element see them as locals.
fn handle_ng_for(
    value: &str,
    locals: &[String],
    scopes: &mut [Vec<String>],
    refs: &mut AngularTemplateRefs,
    iter_ctx: &mut IterationTypingCtx<'_>,
) {
    let clauses: Vec<&str> = value.split(';').collect();

    let mut ng_for_locals = locals.to_vec();
    let mut new_scope_locals = Vec::new();

    for clause in &clauses {
        let clause = clause.trim();

        if let Some(caps) = NG_FOR_OF_RE.captures(clause) {
            let binding = caps.get(1).map_or("", |m| m.as_str());
            let iterable = caps.get(2).map_or("", |m| m.as_str()).trim();
            // Type the `let item of items` loop variable to `items`'s element
            // class when it is a component field (issue #1712); a typed item is
            // kept OUT of the child scope so `{{ item.getName() }}` remaps onto
            // the element class. The iterable scan still runs with the item in
            // `ng_for_locals` so `items` (not `item`) is the credited reference.
            let typed = iter_ctx.try_type_loop_variable(binding, iterable);
            ng_for_locals.push(binding.to_string());
            if !typed
                && (!is_bare_identifier(binding.trim())
                    || !iter_ctx.preserves_untyped_iteration_sources())
            {
                new_scope_locals.push(binding.to_string());
            } else {
                new_scope_locals.push(loop_item_local(binding));
            }
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

    let (main_expr, pipe_segments) = split_pipe_segments(expr);
    collect_snippet_refs(main_expr, TemplateSnippetKind::Expression, locals, refs);

    for segment in pipe_segments {
        let mut parts = split_pipe_arguments(segment).into_iter();
        let pipe_name = parts.next().unwrap_or("").trim();
        if !pipe_name.is_empty() {
            refs.identifiers.insert(pipe_name.to_string());
        }
        for argument in parts {
            collect_snippet_refs(argument, TemplateSnippetKind::Expression, locals, refs);
        }
    }
}

/// Collect unresolved identifier references from a statement (event handler).
fn collect_statement_refs(stmt: &str, locals: &[String], refs: &mut AngularTemplateRefs) {
    collect_snippet_refs(stmt, TemplateSnippetKind::Statement, locals, refs);
}

/// Parse one pipe-free snippet and add its unresolved identifiers and member
/// chains to `refs`. Loop items marked with [`LOOP_ITEM_MARKER`] are not passed
/// to the parser as locals, so their member chains stay visible, but a bare
/// read of such an item is not a component reference.
fn collect_snippet_refs(
    snippet: &str,
    kind: TemplateSnippetKind,
    locals: &[String],
    refs: &mut AngularTemplateRefs,
) {
    let snippet = snippet.trim();
    if snippet.is_empty() {
        return;
    }
    let mut parser_locals = Vec::with_capacity(locals.len());
    let mut loop_items: Vec<&str> = Vec::new();
    for local in locals {
        match local.strip_prefix(LOOP_ITEM_MARKER) {
            Some(item) => loop_items.push(item),
            None => parser_locals.push(local.clone()),
        }
    }
    let (unresolved, member_accesses) =
        collect_unresolved_refs_and_accesses(snippet, kind, &parser_locals);
    refs.identifiers.extend(
        unresolved
            .into_iter()
            .filter(|name| !loop_items.contains(&name.as_str())),
    );
    for access in member_accesses {
        refs.add_member_access(access);
    }
    if snippet.contains("this") {
        let code = mask_string_contents(snippet);
        refs.identifiers.extend(
            THIS_MEMBER_RE
                .captures_iter(&code)
                .filter_map(|caps| caps.get(1))
                .map(|m| m.as_str().to_string()),
        );
    }
}

/// Split one pipe segment (`slice: start : end`) into the pipe name followed
/// by its arguments. A `:` that closes a top-level ternary (`a ? b : c`) is
/// part of the argument, not a separator.
fn split_pipe_arguments(segment: &str) -> Vec<&str> {
    let bytes = segment.as_bytes();
    let mut parts = Vec::new();
    let mut part_start = 0;
    let mut depth = 0u32;
    let mut open_ternaries = 0u32;
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
            // `?.` and `??` are not ternaries.
            b'?' if matches!(bytes.get(i + 1), Some(b'.' | b'?')) => i += 1,
            b'?' if depth == 0 => open_ternaries += 1,
            b':' if depth == 0 && open_ternaries > 0 => open_ternaries -= 1,
            b':' if depth == 0 => {
                parts.push(&segment[part_start..i]);
                part_start = i + 1;
            }
            _ => {}
        }
        i += 1;
    }
    parts.push(&segment[part_start..]);
    parts
}

/// The main expression and the pipe names of `expr`, as
/// [`collect_expression_refs`] reads them.
#[cfg(test)]
fn split_pipes(expr: &str) -> (&str, Vec<&str>) {
    let (main_expr, segments) = split_pipe_segments(expr);
    let names = segments
        .into_iter()
        .filter_map(|segment| {
            let name = split_pipe_arguments(segment).first().copied()?.trim();
            (!name.is_empty()).then_some(name)
        })
        .collect();
    (main_expr, names)
}

/// Split an Angular expression on top-level pipe operators (`|`). Returns the
/// main expression and each pipe segment (`date:'short'`) with its arguments.
/// Correctly distinguishes pipes from logical OR (`||`).
fn split_pipe_segments(expr: &str) -> (&str, Vec<&str>) {
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
    let mut segments = Vec::new();
    for (j, &pos) in pipe_positions.iter().enumerate() {
        let end = pipe_positions.get(j + 1).copied().unwrap_or(expr.len());
        let pipe_part = expr[pos + 1..end].trim();
        if !pipe_part.is_empty() {
            segments.push(pipe_part);
        }
    }

    (main_expr, segments)
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
    fn legacy_bind_and_on_attrs_extract_refs() {
        let refs = collect_angular_template_refs(
            r#"<button bind-title="title" on-click="submit(value)">"#,
        );
        assert!(refs.contains("title"));
        assert!(refs.contains("submit"));
        assert!(refs.contains("value"));
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

    // --- @for / *ngFor loop-variable element-type remap (issue #1712) ---

    fn field_types(pairs: &[(&str, &str)]) -> FxHashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect()
    }

    fn has_access(refs: &AngularTemplateRefs, object: &str, member: &str) -> bool {
        refs.member_accesses
            .iter()
            .any(|a| a.object == object && a.member == member)
    }

    #[test]
    fn control_for_item_typed_to_element_class_credits_member_access() {
        // `@for (util of utils; track util)` where `utils` is `Util[]`: member
        // accesses on the item remap onto the `Util` class (issue #1712).
        let refs = collect_angular_template_refs_with_field_types(
            r"@for (util of utils; track util) { <li>{{ util.getName() }} {{ util.getter }}</li> }",
            &field_types(&[("utils", "Util")]),
        );
        assert!(
            has_access(&refs, "Util", "getName"),
            "util.getName should map to Util.getName, found: {:?}",
            refs.member_accesses
        );
        assert!(
            has_access(&refs, "Util", "getter"),
            "util.getter should map to Util.getter, found: {:?}",
            refs.member_accesses
        );
        assert!(
            !refs.identifiers.contains("util"),
            "typed loop item must not leak as a component member ref, found: {:?}",
            refs.identifiers
        );
    }

    #[test]
    fn ng_for_item_typed_to_element_class_credits_member_access() {
        // Legacy `*ngFor="let util of utils"` where `utils` is `Util[]`.
        let refs = collect_angular_template_refs_with_field_types(
            r#"<li *ngFor="let util of utils">{{ util.getName() }}</li>"#,
            &field_types(&[("utils", "Util")]),
        );
        assert!(
            has_access(&refs, "Util", "getName"),
            "*ngFor util.getName should map to Util.getName, found: {:?}",
            refs.member_accesses
        );
        assert!(
            !refs.identifiers.contains("util"),
            "typed *ngFor item must not leak as a component member ref, found: {:?}",
            refs.identifiers
        );
    }

    #[test]
    fn for_untyped_source_leaves_item_unmapped() {
        // Neuter check: with no known element type for the source, the item stays
        // a local and its member accesses credit nothing (pre-fix behavior).
        let refs = collect_angular_template_refs_with_field_types(
            r"@for (util of utils; track util) { <li>{{ util.getName() }}</li> }",
            &FxHashMap::default(),
        );
        assert!(
            !refs.member_accesses.iter().any(|a| a.object == "Util"),
            "untyped @for item must not credit any class, found: {:?}",
            refs.member_accesses
        );
        assert!(
            !refs.member_accesses.iter().any(|a| a.object == "util"),
            "untyped @for item member access must stay suppressed as a local, found: {:?}",
            refs.member_accesses
        );
    }

    #[test]
    fn for_iterable_field_still_credited_as_component_ref() {
        // The source iterable (`utils`) is still a bare-identifier reference the
        // component must expose; typing the item does not drop the iterable ref.
        let refs = collect_angular_template_refs_with_field_types(
            r"@for (util of utils; track util) { <li>{{ util.getName() }}</li> }",
            &field_types(&[("utils", "Util")]),
        );
        assert!(
            refs.identifiers.contains("utils"),
            "the iterable `utils` must still be a component member ref, found: {:?}",
            refs.identifiers
        );
    }

    #[test]
    fn pipe_arguments_are_credited() {
        let refs = collect_angular_template_refs(
            "<span>{{ 'abc' | slice: start() : end | date: fmt ? long : short }}</span>",
        );
        for name in ["start", "end", "fmt", "long", "short", "slice", "date"] {
            assert!(
                refs.contains(name),
                "{name} missing: {:?}",
                refs.identifiers
            );
        }
    }

    #[test]
    fn attribute_interpolation_is_credited() {
        let refs = collect_angular_template_refs(
            r#"<div title="{{ attrInterp() }}" class="base {{ classInterp() }} x"></div>"#,
        );
        assert!(refs.contains("attrInterp"), "{:?}", refs.identifiers);
        assert!(refs.contains("classInterp"), "{:?}", refs.identifiers);
    }

    #[test]
    fn structural_microsyntax_with_keys_is_credited() {
        let refs = collect_angular_template_refs(
            r#"<ng-container *ngTemplateOutlet="tpl(); context: outletCtx()"></ng-container>"#,
        );
        assert!(refs.contains("tpl"), "{:?}", refs.identifiers);
        assert!(refs.contains("outletCtx"), "{:?}", refs.identifiers);
    }

    #[test]
    fn structural_microsyntax_let_of_clause_credits_iterable() {
        let refs = collect_angular_template_refs(
            r#"<li *cdkVirtualFor="let row of rows(); let i = index; trackBy: byId">x</li>"#,
        );
        assert!(refs.contains("rows"), "{:?}", refs.identifiers);
        assert!(refs.contains("byId"), "{:?}", refs.identifiers);
        assert!(!refs.contains("row"), "{:?}", refs.identifiers);
    }

    #[test]
    fn structural_microsyntax_context_export_is_not_a_reference() {
        let refs = collect_angular_template_refs(
            r#"<li *cdkVirtualFor="let row of rows(); index as i; count as total">x</li>"#,
        );
        assert!(refs.contains("rows"), "{:?}", refs.identifiers);
        for name in ["index", "count", "i", "total"] {
            assert!(!refs.contains(name), "{name}: {:?}", refs.identifiers);
        }
    }

    #[test]
    fn ng_if_else_template_clause_is_credited() {
        let refs = collect_angular_template_refs(
            r#"<p *ngIf="ready(); then readyTpl; else loadingTpl">x</p>"#,
        );
        for name in ["ready", "readyTpl", "loadingTpl"] {
            assert!(refs.contains(name), "{name}: {:?}", refs.identifiers);
        }
    }

    #[test]
    fn else_if_alias_condition_is_credited_and_alias_is_local() {
        let refs = collect_angular_template_refs(
            "@if (n > 1) { <b>a</b> } @else if (elseIfCond(); as c) { <b>{{ c }}</b> }",
        );
        assert!(refs.contains("elseIfCond"), "{:?}", refs.identifiers);
        assert!(
            !refs.contains("c"),
            "alias must stay local: {:?}",
            refs.identifiers
        );
    }

    #[test]
    fn style_binding_with_percent_unit_is_credited() {
        let refs = collect_angular_template_refs(r#"<div [style.width.%]="widthPct()"></div>"#);
        assert!(refs.contains("widthPct"), "{:?}", refs.identifiers);
    }

    #[test]
    fn read_outside_loop_survives_same_named_loop_variable() {
        let refs = collect_angular_template_refs(
            "<p>{{ shadowed() }}</p>\n@for (shadowed of items(); track shadowed) { <i>{{ shadowed }}</i> }",
        );
        assert!(refs.contains("shadowed"), "{:?}", refs.identifiers);
        assert!(refs.contains("items"), "{:?}", refs.identifiers);
    }

    #[test]
    fn loop_variable_read_inside_loop_is_not_credited() {
        let refs = collect_angular_template_refs(
            "@for (row of rows(); track row.id) { <i>{{ row }}</i> }\n<p>{{ other }}</p>",
        );
        assert!(!refs.contains("row"), "{:?}", refs.identifiers);
        assert!(refs.contains("other"), "{:?}", refs.identifiers);
    }
}
