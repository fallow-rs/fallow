//! Template-visible import-usage scanner for Glimmer `<template>` blocks in
//! `.gts` / `.gjs` single-file components.
//!
//! Glimmer/Handlebars syntax is not JavaScript, so we cannot reuse the Oxc-
//! backed expression analyzer that powers Vue and Svelte template scanning.
//! Instead this module walks each `<template>...</template>` body with a
//! purpose-built tokenizer that recognises the constructs which can legally
//! reference a JS-scope binding:
//!
//! - `<HelloWorld />` and `<HelloWorld>...</HelloWorld>`: PascalCase tag
//!   invocations credit a binding by tag name. Strict-mode `.gts` / `.gjs`
//!   components are JavaScript bindings, so namespaced tags (`<Forms::Input />`)
//!   and member-style tags (`<Buttons.Primary />`) are deliberately out of
//!   scope. They're a classic-resolver / `.hbs` concept and `.hbs` is itself
//!   a known limitation. Strict-mode code that wants that shape imports the
//!   leaf component directly (`import Input from './forms/input'; <Input />`).
//! - `{{capitalize x}}`: Handlebars helper invocation credits each bareword
//!   identifier that is not a built-in keyword, a literal, a `this.` chain,
//!   an `@arg`, or a named-argument key.
//! - `{{if (and a b) "yes" "no"}}`: sub-expressions are scanned recursively
//!   inside `(...)`.
//! - `<button {{on "click" handleClick}} />`: modifier mustaches inside
//!   element-attribute position scan the same as regular mustaches.
//! - `{{utils.formatDate value}}`: dotted member references credit the base
//!   binding and emit a `MemberAccess { object: utils, member: formatDate }`.
//!
//! Block-parameter introductions (`{{#each items as |item index|}}`) are
//! accumulated as template-scope locals so they shadow same-named imports.
//! The scope is intentionally template-wide rather than block-precise: a
//! local introduced in one `{{#each}}` will (pessimistically) shadow the
//! same-named import elsewhere in the same template. This trades a small
//! false-negative for staying out of the parser business; promoting to a
//! block-precise scope is a follow-up.

use std::ops::Range;

use rustc_hash::FxHashSet;

use crate::MemberAccess;
use crate::template_usage::TemplateUsage;

/// Handlebars / Glimmer keywords that must never be resolved as imports,
/// scoped to **strict mode** (`.gts` / `.gjs`) semantics.
///
/// Includes language keywords (control flow, scope) and the literal-name
/// keywords. Helpers that are ambient in classic `.hbs` but require explicit
/// imports from `@ember/helper` in strict mode (`hash`, `array`, `concat`,
/// `fn`, `mut`, `get`) are deliberately omitted: if the user imported them
/// the template scanner SHOULD credit those imports, and if they didn't the
/// `imported_bindings` lookup short-circuits anyway. Same reasoning for the
/// built-in components `Input` and `Textarea` (imported from
/// `@ember/component` in strict mode). Plain `input` / `textarea` are HTML
/// DOM elements, never Ember tokens, and aren't listed.
const BUILTIN_KEYWORDS: &[&str] = &[
    // Control-flow blocks and inline forms
    "if",
    "unless",
    "else",
    "each",
    "each-in",
    "let",
    "with",
    "in-element",
    "key",
    // Component / yield machinery (`component`, `helper`, `modifier` are
    // template-language keywords that introduce a `(component "name" ...)`
    // / `(helper ...)` / `(modifier ...)` sub-expression. They are NOT the
    // same as the same-named JS bindings a user might import).
    "yield",
    "outlet",
    "component",
    "helper",
    "modifier",
    "mount",
    "unbound",
    // Built-in components and helpers that are still ambient in strict mode
    "link-to",
    "LinkTo",
    // Debugging
    "debugger",
    "log",
    // Literal-name keywords
    "true",
    "false",
    "null",
    "undefined",
    "this",
];

/// Collect template-visible import usage for every `<template>...</template>`
/// block in a Glimmer (`.gts` / `.gjs`) source file.
///
/// `template_ranges` MUST be the byte ranges previously captured by
/// `crate::glimmer::find_template_ranges(source)`; the caller is expected to
/// pass them through rather than re-scan the file.
#[must_use]
pub fn collect_glimmer_template_usage(
    source: &str,
    template_ranges: &[Range<usize>],
    imported_bindings: &FxHashSet<String>,
) -> TemplateUsage {
    let mut usage = TemplateUsage::default();
    if template_ranges.is_empty() {
        return usage;
    }
    // Note: an empty `imported_bindings` set used to short-circuit here, but
    // the scanner ALSO emits `this.<member>` accesses for class-member
    // tracking. Those don't depend on imports, so we always walk every
    // template body. `credit_token` / `credit_tag_name` handle empty
    // imports cheaply (one set lookup short-circuits before any allocation).

    for range in template_ranges {
        let Some(body) = template_body(source, range.clone()) else {
            continue;
        };

        let locals = extract_block_params(body);
        scan_tags(body, imported_bindings, &locals, &mut usage);
        scan_mustaches(body, imported_bindings, &locals, &mut usage);
    }

    usage
}

/// Slice the inner body of a `<template>...</template>` range, stripping the
/// outer tags. Returns `None` if the range does not look like a well-formed
/// template wrapper (e.g. unclosed at end-of-file) or carries no content
/// between the opening `>` and the closing `</template>`.
fn template_body(source: &str, range: Range<usize>) -> Option<&str> {
    let slice = source.get(range)?;
    let body_start = slice.find('>').map(|i| i + 1)?;
    let body_end = slice.rfind("</template>").unwrap_or(slice.len());
    // `<=` because an empty body (`<template></template>`) has no tokens to
    // credit, so skipping it here avoids two redundant scanner passes.
    if body_end <= body_start {
        return None;
    }
    slice.get(body_start..body_end)
}

// -- block params ---------------------------------------------------------

/// Walk a template body and harvest every identifier introduced via
/// `as |x y|` block-parameter syntax. The scan is purely textual and does
/// not respect `{{#each}}` block boundaries, so locals introduced in one
/// block effectively shadow the same name across the whole template. This
/// is a deliberate trade-off (see module doc).
fn extract_block_params(body: &str) -> Vec<String> {
    let mut locals = Vec::new();
    let bytes = body.as_bytes();
    let mut cursor = 0;

    while let Some(relative) = body[cursor..].find("as ") {
        let absolute = cursor + relative;
        let after_as = absolute + "as ".len();
        // Find the next pipe; the matching close pipe defines the param list.
        let Some(open_pipe_rel) = body[after_as..].find('|') else {
            break;
        };
        let open_pipe = after_as + open_pipe_rel;
        // Only whitespace may appear between `as ` and the opening pipe.
        if body[after_as..open_pipe]
            .bytes()
            .any(|b| !b.is_ascii_whitespace())
        {
            cursor = open_pipe + 1;
            continue;
        }
        let Some(close_pipe_rel) = body[open_pipe + 1..].find('|') else {
            break;
        };
        let close_pipe = open_pipe + 1 + close_pipe_rel;
        let inner = &body[open_pipe + 1..close_pipe];
        for token in inner.split_whitespace() {
            if is_plain_identifier(token) {
                locals.push(token.to_string());
            }
        }
        cursor = close_pipe + 1;
        if cursor >= bytes.len() {
            break;
        }
    }

    locals
}

// -- tag scanning ---------------------------------------------------------

/// Scan opening element tags for PascalCase component invocations.
/// `<HelloWorld @x="y" />` credits binding `HelloWorld`.
///
/// Only plain identifier tag names are recognised. Strict-mode `.gts` /
/// `.gjs` components are JavaScript bindings, so namespaced or member-style
/// tag invocations (`<Forms::Input />`, `<Buttons.Primary />`) are out of
/// scope. They're a classic-resolver / `.hbs` concept and `.hbs` is itself a
/// known limitation of the plugin. Strict-mode code that wants the same
/// shape simply imports the leaf component directly (`import Input from
/// './forms/input'; <Input />`).
fn scan_tags(
    body: &str,
    imported_bindings: &FxHashSet<String>,
    locals: &[String],
    usage: &mut TemplateUsage,
) {
    let bytes = body.as_bytes();
    let mut index = 0;

    while index < bytes.len() {
        if bytes[index] != b'<' {
            index += 1;
            continue;
        }
        // HTML comment: `<!-- ... -->`. Skip the entire comment body so a
        // PascalCase tag-shape inside a comment (`<!-- <Foo /> -->`) does
        // not credit `Foo`. Both Glimmer and HTML treat the comment body as
        // opaque, so this is correct.
        if bytes[index..].starts_with(b"<!--") {
            let after_open = index + b"<!--".len();
            index = body[after_open..]
                .find("-->")
                .map_or(bytes.len(), |rel| after_open + rel + b"-->".len());
            continue;
        }
        // Skip closing tags, other `<!` shapes (doctype, CDATA), and
        // processing instructions. Advance by one byte and keep scanning;
        // the loop will re-anchor on the next `<`.
        let next = bytes.get(index + 1).copied();
        if matches!(next, Some(b'/' | b'!' | b'?')) {
            index += 1;
            continue;
        }
        let Some(first) = next else { break };
        if !first.is_ascii_uppercase() {
            index += 1;
            continue;
        }

        // Read a plain identifier tag name (A-Z, a-z, 0-9, underscore, $).
        let name_start = index + 1;
        let mut end = name_start;
        while end < bytes.len() {
            let byte = bytes[end];
            if !(byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'$') {
                break;
            }
            end += 1;
        }
        if end > name_start {
            credit_binding(&body[name_start..end], imported_bindings, locals, usage);
        }
        // `end >= name_start + 1 = index + 2` whenever we matched a tag, and
        // when we didn't `end == name_start == index + 1`. Either way `end`
        // already advances past the current `<`, no `.max()` needed.
        index = end;
    }
}

// -- mustache scanning ----------------------------------------------------

/// Walk every `{{ ... }}` section in the template body and credit any
/// imported bindings or member-accesses referenced inside. Triple-stash
/// `{{{ ... }}}` (unescaped HTML output) is handled by the same code path
/// because the inner content scans identically.
fn scan_mustaches(
    body: &str,
    imported_bindings: &FxHashSet<String>,
    locals: &[String],
    usage: &mut TemplateUsage,
) {
    let bytes = body.as_bytes();
    let mut index = 0;
    while index + 1 < bytes.len() {
        if bytes[index] != b'{' || bytes[index + 1] != b'{' {
            index += 1;
            continue;
        }
        let triple_stash = matches!(bytes.get(index + 2), Some(b'{'));
        let after_open = index + if triple_stash { 3 } else { 2 };
        let close_token = if triple_stash { "}}}" } else { "}}" };
        let comment_form = !triple_stash && matches!(bytes.get(after_open), Some(b'!'));
        let Some(close_rel) = body[after_open..].find(close_token) else {
            break;
        };
        let close = after_open + close_rel;
        if comment_form {
            index = close + close_token.len();
            continue;
        }
        let inner = &body[after_open..close];
        scan_mustache_inner(inner, imported_bindings, locals, usage);
        index = close + close_token.len();
    }
}

/// Tokenize the contents of one `{{ ... }}` (or one `( ... )` sub-expression)
/// and credit each token that resolves to an imported binding.
fn scan_mustache_inner(
    inner: &str,
    imported_bindings: &FxHashSet<String>,
    locals: &[String],
    usage: &mut TemplateUsage,
) {
    let inner = inner.trim_matches(|c: char| c.is_whitespace() || c == '~');
    if inner.is_empty() {
        return;
    }
    // Block markers: `#each`, `/each`, `^else`. Skip the leading sigil but
    // keep tokenizing the rest of the line. The helper name itself (e.g.
    // `each`) is a built-in we filter below, but its arguments aren't.
    let inner = inner
        .strip_prefix('#')
        .or_else(|| inner.strip_prefix('/'))
        .or_else(|| inner.strip_prefix('^'))
        .unwrap_or(inner);

    for token in TokenSplitter::new(inner) {
        credit_token(token, imported_bindings, locals, usage);
    }
}

fn credit_token(
    token: &str,
    imported_bindings: &FxHashSet<String>,
    locals: &[String],
    usage: &mut TemplateUsage,
) {
    let token = token.trim();
    if token.is_empty() {
        return;
    }

    // Built-in keywords (`if`, `each`, `yield`, ...). Checked first so the
    // hot path for control-flow blocks short-circuits before any of the
    // shape-dispatch branches below run. Keywords are plain identifiers, so
    // none of the later branches (sub-expressions in parens, `key=value`
    // named args, `@arg`, `this.*`, dotted references) would consume them
    // anyway. Moving the check up just avoids the wasted work.
    if BUILTIN_KEYWORDS.contains(&token) {
        return;
    }

    // Sub-expression: recurse into the parens.
    if let Some(stripped) = token.strip_prefix('(').and_then(|s| s.strip_suffix(')')) {
        scan_mustache_inner(stripped, imported_bindings, locals, usage);
        return;
    }

    // Named argument: `key=value`; credit the value, drop the key.
    if let Some((_key, value)) = token.split_once('=')
        && !value.is_empty()
    {
        credit_token(value, imported_bindings, locals, usage);
        return;
    }

    // Literals: numbers, strings, hashbar negatives.
    if is_literal(token) {
        return;
    }

    // `@arg` references are template arguments. Never resolve to an import
    // binding and never emit passing the component.
    if token.starts_with('@') || token == "this" {
        return;
    }

    // `this.<member>` references a class field / method on the component.
    // It's NOT an import binding, but each hop along the chain IS a
    // class-member access. Emit one `MemberAccess` per segment so that:
    //
    //   `{{this.foo}}`         -> `(this, foo)`
    //   `{{this.foo.bar}}`     -> `(this, foo)`, `(this.foo, bar)`
    //   `{{this.deps.svc.x}}`  -> `(this, deps)`, `(this.deps, svc)`,
    //                             `(this.deps.svc, x)`
    //
    // Mirrors the JS visitor's per-hop `visit_static_member_expression`
    // emission (`crates/extract/src/visitor/visit_impl.rs`), which is what
    // typed-instance-binding resolution and inherited-member propagation
    // depend on. Without the deeper hops, a class field on a service
    // referenced only through the template via
    // `<Child @x={{this.svc.method}} />` would be flagged unused.
    if token.strip_prefix("this.").is_some() {
        emit_chain_member_accesses(token, usage);
        return;
    }

    // Dotted reference: credit the head and emit member accesses along the
    // chain. `utils.formatters.date` credits `utils` and emits
    // `(utils, formatters)` + `(utils.formatters, date)`, matching the JS
    // visitor's per-hop emission so cross-namespace member chains are
    // tracked the same way through the template scanner. The head is still
    // checked against `BUILTIN_KEYWORDS` because a dotted form like
    // `each.foo` would not have been caught by the bare-keyword
    // short-circuit at the top of this function.
    if let Some((head, _rest)) = token.split_once('.')
        && is_plain_identifier(head)
    {
        if BUILTIN_KEYWORDS.contains(&head) {
            return;
        }
        credit_binding(head, imported_bindings, locals, usage);
        emit_chain_member_accesses(token, usage);
        return;
    }

    // Bare identifier.
    if is_plain_identifier(token) {
        credit_binding(token, imported_bindings, locals, usage);
    }
}

/// Iterator that yields whitespace-separated top-level tokens from a
/// Handlebars expression body, treating `(...)` as a single atomic token
/// and respecting string literals (single, double, backtick).
struct TokenSplitter<'a> {
    bytes: &'a [u8],
    source: &'a str,
    index: usize,
}

impl<'a> TokenSplitter<'a> {
    fn new(source: &'a str) -> Self {
        Self {
            bytes: source.as_bytes(),
            source,
            index: 0,
        }
    }
}

impl<'a> Iterator for TokenSplitter<'a> {
    type Item = &'a str;

    fn next(&mut self) -> Option<Self::Item> {
        while self.index < self.bytes.len() && self.bytes[self.index].is_ascii_whitespace() {
            self.index += 1;
        }
        if self.index >= self.bytes.len() {
            return None;
        }
        let start = self.index;
        let mut paren_depth: u32 = 0;
        let mut in_quote: Option<u8> = None;
        let mut escape = false;

        while self.index < self.bytes.len() {
            let byte = self.bytes[self.index];

            if let Some(quote) = in_quote {
                if escape {
                    escape = false;
                } else if byte == b'\\' {
                    escape = true;
                } else if byte == quote {
                    in_quote = None;
                }
                self.index += 1;
                continue;
            }

            match byte {
                b'(' => paren_depth += 1,
                b')' => paren_depth = paren_depth.saturating_sub(1),
                b'"' | b'\'' | b'`' => in_quote = Some(byte),
                b if b.is_ascii_whitespace() && paren_depth == 0 => break,
                _ => {}
            }
            self.index += 1;
        }

        Some(&self.source[start..self.index])
    }
}

// -- helpers --------------------------------------------------------------

fn is_plain_identifier(token: &str) -> bool {
    let mut chars = token.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !(first.is_ascii_alphabetic() || first == '_' || first == '$') {
        return false;
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$')
}

fn is_literal(token: &str) -> bool {
    let first = token.as_bytes().first().copied();
    matches!(
        first,
        Some(b'"' | b'\'' | b'`' | b'0'..=b'9' | b'-' | b'+' | b'.')
    )
}

fn credit_binding(
    name: &str,
    imported_bindings: &FxHashSet<String>,
    locals: &[String],
    usage: &mut TemplateUsage,
) {
    if name.is_empty()
        || locals.iter().any(|local| local == name)
        || !imported_bindings.contains(name)
    {
        return;
    }
    usage.used_bindings.insert(name.to_string());
}

/// Emit one `MemberAccess` per hop along a dotted chain such as
/// `this.foo.bar` or `utils.formatters.date`. Each emitted access pairs the
/// dotted-path object name (`this`, `this.foo`, `utils`, `utils.formatters`,
/// ...) with the next single-segment member, matching the JS visitor's
/// per-hop `visit_static_member_expression` emission. Stops at the first
/// segment that is not a plain identifier (e.g. would-be `this.foo()`).
fn emit_chain_member_accesses(token: &str, usage: &mut TemplateUsage) {
    let mut segments = token.split('.');
    let Some(first) = segments.next() else {
        return;
    };
    let mut object_end = first.len();
    for member in segments {
        if !is_plain_identifier(member) {
            return;
        }
        push_member_access(usage, &token[..object_end], member);
        // Advance the object slice by `.<member>`.
        object_end += 1 + member.len();
    }
}

fn push_member_access(usage: &mut TemplateUsage, object: &str, member: &str) {
    let already_present = usage
        .member_accesses
        .iter()
        .any(|existing| existing.object == object && existing.member == member);
    if already_present {
        return;
    }
    usage.member_accesses.push(MemberAccess {
        object: object.to_string(),
        member: member.to_string(),
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn imported(names: &[&str]) -> FxHashSet<String> {
        names.iter().map(|name| (*name).to_string()).collect()
    }

    fn usage_for(source: &str, names: &[&str]) -> TemplateUsage {
        let ranges = crate::glimmer::find_template_ranges(source);
        collect_glimmer_template_usage(source, &ranges, &imported(names))
    }

    #[test]
    fn pascal_case_tag_credits_binding() {
        let usage = usage_for(
            "<template><HelloWorld @name=\"x\" /></template>",
            &["HelloWorld"],
        );
        assert!(usage.used_bindings.contains("HelloWorld"));
    }

    #[test]
    fn namespaced_tag_credits_only_head_segment() {
        // Strict-mode `.gts` / `.gjs` does not use classic-resolver namespaced
        // tags; if a user wrote one anyway we should at minimum credit the
        // leading identifier (so a half-migrated file doesn't surface false
        // unused-imports) and skip the rest.
        let usage = usage_for("<template><Forms::Input /></template>", &["Forms"]);
        assert!(usage.used_bindings.contains("Forms"));
        assert!(
            usage.member_accesses.is_empty(),
            "member-style splits are intentionally not tracked",
        );
    }

    #[test]
    fn member_style_tag_credits_only_head_segment() {
        // Same shape, dot separator. The leaf component should be imported
        // directly in strict mode; if someone writes `<Buttons.Primary />`
        // anyway, credit `Buttons` and leave `Primary` alone.
        let usage = usage_for("<template><Buttons.Primary /></template>", &["Buttons"]);
        assert!(usage.used_bindings.contains("Buttons"));
        assert!(usage.member_accesses.is_empty());
    }

    #[test]
    fn mustache_identifier_credits_binding() {
        let usage = usage_for("<template>{{capitalize name}}</template>", &["capitalize"]);
        assert!(usage.used_bindings.contains("capitalize"));
    }

    #[test]
    fn triple_stash_helper_credits_binding() {
        let usage = usage_for(
            "<template>{{{formatHtml body}}}</template>",
            &["formatHtml"],
        );
        assert!(usage.used_bindings.contains("formatHtml"));
    }

    #[test]
    fn sub_expression_helper_credits_binding() {
        let usage = usage_for(
            "<template>{{if (and a b) \"yes\" \"no\"}}</template>",
            &["and"],
        );
        assert!(usage.used_bindings.contains("and"));
    }

    #[test]
    fn modifier_invocation_credits_binding() {
        let usage = usage_for(
            "<template><button {{on \"click\" handleClick}} /></template>",
            &["on"],
        );
        assert!(usage.used_bindings.contains("on"));
    }

    #[test]
    fn this_and_arg_references_are_not_credited_as_imports() {
        // Neither `this.name` nor `@arg` should credit the imported binding
        // (these aren't module-scope identifiers). `this.name` DOES emit a
        // `MemberAccess { this, name }` so class-member tracking can pick it
        // up; `@arg` emits nothing.
        let usage = usage_for(
            "<template>{{this.name}} {{@arg}}</template>",
            &["name", "arg"],
        );
        assert!(usage.used_bindings.is_empty());
    }

    #[test]
    fn this_dot_member_emits_member_access() {
        // `{{this.handleClick}}` references a class field on the component.
        // The JS visitor records `MemberAccess { object: "this", member: "x" }`
        // for in-code `this.x` reads, and the Glimmer scanner mirrors that
        // so the unused-class-members analyzer credits members referenced
        // ONLY through the template (e.g. arrow-function class fields
        // passed to a child component via `@onClick={{this.handleClick}}`).
        let usage = usage_for(
            "<template>{{this.handleClick}} {{this.tab}}</template>",
            &[],
        );
        assert!(usage.used_bindings.is_empty());
        let access_keys: Vec<(&str, &str)> = usage
            .member_accesses
            .iter()
            .map(|a| (a.object.as_str(), a.member.as_str()))
            .collect();
        assert!(access_keys.contains(&("this", "handleClick")));
        assert!(access_keys.contains(&("this", "tab")));
    }

    #[test]
    fn this_dot_chained_emits_one_member_access_per_hop() {
        // For `{{this.foo.bar.baz}}` we emit one `MemberAccess` per hop along
        // the chain: `(this, foo)`, `(this.foo, bar)`, `(this.foo.bar, baz)`.
        // Mirrors the JS visitor's per-hop emission so typed-instance-binding
        // resolution (`this.deps.svc.method()`) and inherited-member
        // propagation work the same way for template references as they do
        // for code references.
        let usage = usage_for("<template>{{this.foo.bar.baz}}</template>", &[]);
        let access_keys: Vec<(&str, &str)> = usage
            .member_accesses
            .iter()
            .map(|a| (a.object.as_str(), a.member.as_str()))
            .collect();
        assert!(access_keys.contains(&("this", "foo")));
        assert!(access_keys.contains(&("this.foo", "bar")));
        assert!(access_keys.contains(&("this.foo.bar", "baz")));
    }

    #[test]
    fn block_params_shadow_imports_template_wide() {
        let usage = usage_for(
            "<template>{{#each items as |item|}}{{item}}{{/each}}</template>",
            &["item", "items"],
        );
        assert!(usage.used_bindings.contains("items"));
        assert!(!usage.used_bindings.contains("item"));
    }

    #[test]
    fn dotted_namespace_credits_binding_and_member() {
        let usage = usage_for(
            "<template>{{utils.formatDate value}}</template>",
            &["utils"],
        );
        assert!(usage.used_bindings.contains("utils"));
        assert_eq!(usage.member_accesses.len(), 1);
        assert_eq!(usage.member_accesses[0].object, "utils");
        assert_eq!(usage.member_accesses[0].member, "formatDate");
    }

    #[test]
    fn deep_dotted_namespace_emits_chain_member_accesses() {
        // `{{utils.formatters.date value}}` credits `utils` as a binding AND
        // emits per-hop member accesses so cross-namespace chains work the
        // same way as the JS visitor: `(utils, formatters)` and
        // `(utils.formatters, date)`. Without this, the deeper hop is
        // invisible and `date` on a nested namespace export goes unflagged.
        let usage = usage_for(
            "<template>{{utils.formatters.date value}}</template>",
            &["utils"],
        );
        assert!(usage.used_bindings.contains("utils"));
        let access_keys: Vec<(&str, &str)> = usage
            .member_accesses
            .iter()
            .map(|a| (a.object.as_str(), a.member.as_str()))
            .collect();
        assert!(access_keys.contains(&("utils", "formatters")));
        assert!(access_keys.contains(&("utils.formatters", "date")));
    }

    #[test]
    fn multiple_template_blocks_all_contribute() {
        let usage = usage_for(
            "<template><Foo /></template>\nconst x = 1;\n<template>{{bar}}</template>",
            &["Foo", "bar"],
        );
        assert!(usage.used_bindings.contains("Foo"));
        assert!(usage.used_bindings.contains("bar"));
    }

    #[test]
    fn malformed_template_produces_empty_usage() {
        let usage = usage_for("<template>{{ unclosed", &["unclosed"]);
        // Unclosed mustache yields no credits; the scan must not panic.
        assert!(usage.used_bindings.is_empty());
    }

    #[test]
    fn html_comment_in_template_does_not_credit_inner_tag() {
        // PascalCase tag-shapes inside an HTML comment must NOT credit the
        // import. Without the `<!--` skip in `scan_tags` the scanner would
        // walk through the comment body and credit `Foo` from
        // `<!-- <Foo /> -->`.
        let usage = usage_for(
            "<template><!-- <Foo /> --><Bar /></template>",
            &["Foo", "Bar"],
        );
        assert!(!usage.used_bindings.contains("Foo"));
        assert!(usage.used_bindings.contains("Bar"));
    }

    #[test]
    fn handlebars_comment_is_skipped() {
        let usage = usage_for(
            "<template>{{!-- {{capitalize x}} --}}</template>",
            &["capitalize"],
        );
        assert!(usage.used_bindings.is_empty());
    }

    #[test]
    fn builtin_keywords_are_not_credited() {
        // Even if an import shadow-names a built-in keyword, the template's
        // bareword should not be credited; that would mask real unused
        // imports for users who happen to name a helper `if`.
        let usage = usage_for("<template>{{if condition \"a\" \"b\"}}</template>", &["if"]);
        assert!(usage.used_bindings.is_empty());
    }

    #[test]
    fn strict_mode_helper_imports_are_credited() {
        // `hash`, `array`, `concat`, `fn`, `mut`, `get` are ambient in classic
        // `.hbs` but must be imported from `@ember/helper` in strict mode.
        // The scanner must credit them when used in `<template>`, otherwise
        // every strict-mode app surfaces these imports as `unused-import`.
        for name in ["hash", "array", "concat", "fn", "mut", "get"] {
            let template = format!("<template>{{{{{name} a=1}}}}</template>");
            let usage = usage_for(&template, &[name]);
            assert!(
                usage.used_bindings.contains(name),
                "expected strict-mode helper `{name}` to be credited",
            );
        }
    }

    #[test]
    fn strict_mode_input_textarea_tag_imports_are_credited() {
        // `<Input />` and `<Textarea />` are ambient in classic `.hbs` but
        // imported from `@ember/component` in strict mode. Same goes for
        // their use inside a containing element.
        let usage = usage_for(
            "<template><form><Input /><Textarea /></form></template>",
            &["Input", "Textarea"],
        );
        assert!(usage.used_bindings.contains("Input"));
        assert!(usage.used_bindings.contains("Textarea"));
    }

    #[test]
    fn lowercase_dom_tags_are_never_credited() {
        // `input` and `textarea` are HTML DOM elements, never Ember imports.
        // Even if someone has `import { input } from '...'`, the scanner
        // can't see a tag-name match because tag-name scanning only fires on
        // ASCII-uppercase first chars.
        let usage = usage_for(
            "<template><input /><textarea /></template>",
            &["input", "textarea"],
        );
        assert!(usage.used_bindings.is_empty());
    }

    #[test]
    fn named_arg_value_is_credited_not_key() {
        let usage = usage_for(
            "<template>{{my-helper key=binding}}</template>",
            &["my-helper", "binding", "key"],
        );
        assert!(usage.used_bindings.contains("binding"));
        // The key `key` is the attr name, not a value reference.
        assert!(!usage.used_bindings.contains("key"));
    }

    #[test]
    fn pascal_case_block_tag_with_closing_does_not_double_count() {
        let usage = usage_for("<template><MyMenu>inner</MyMenu></template>", &["MyMenu"]);
        assert!(usage.used_bindings.contains("MyMenu"));
    }

    #[test]
    fn render_template_expression_form_credits_bindings() {
        // Modern Ember integration tests use the `<template>...</template>`
        // expression form inside a JS expression (e.g. `await render(...)`)
        // rather than as a top-level component body. The scanner walks the
        // ranges captured by `find_template_ranges`, which match the same
        // bytes regardless of surrounding context, so bindings referenced
        // ONLY through such an inline render call must still be credited.
        let usage = usage_for(
            "import { module, test } from 'qunit';\n\
             import { render } from '@ember/test-helpers';\n\
             import HelloWorld from './hello-world';\n\
             module('it', function (hooks) {\n  \
               test('renders', async function (assert) {\n    \
                 await render(<template><HelloWorld @name=\"x\" /></template>);\n    \
                 assert.ok(true);\n  \
               });\n\
             });\n",
            &["HelloWorld", "render", "module", "test"],
        );
        assert!(
            usage.used_bindings.contains("HelloWorld"),
            "HelloWorld inside an inline `render(<template>...</template>)` \
             expression must be credited; used_bindings = {:?}",
            usage.used_bindings,
        );
    }

    #[test]
    fn empty_imports_short_circuits() {
        let usage = usage_for("<template>{{foo}} <Bar /></template>", &[]);
        assert!(usage.used_bindings.is_empty());
        assert!(usage.member_accesses.is_empty());
    }
}
