//! Function inventory walker for `fallow coverage upload-inventory`.
//!
//! Emits one [`InventoryEntry`] per function (declaration, expression, arrow,
//! method) whose name matches what `oxc-coverage-instrument` produces at
//! instrument time. This is the **static side** of the three-state production
//! coverage story: uploaded inventory minus runtime-seen functions equals
//! `untracked`.
//!
//! # Naming contract
//!
//! The cloud stores function identity as
//! `(filePath, functionName, lineNumber)`. This walker is responsible for the
//! `functionName` and `lineNumber` parts of that contract. Anonymous functions
//! are named `(anonymous_N)` where `N` is a file-scoped monotonic counter that
//! starts at 0 and increments in pre-order AST traversal each time a function
//! is entered without a resolvable explicit name. Name resolution precedence:
//!
//! 1. Parent-provided `pending_name` (from `MethodDefinition`,
//!    `VariableDeclarator`), same pattern as the internal complexity visitor.
//! 2. The function's own `id` (named `function foo() {}`, named function
//!    expression `const x = function named() {}`).
//! 3. `(anonymous_N)` with the current counter value; counter then increments.
//!
//! Counter scope is per-file. Reference implementation:
//! `oxc-coverage-instrument/src/transform.rs` (`fn_counter` field; lines 201
//! and 612 at the time of writing).

use std::path::Path;

use oxc_allocator::Allocator;
#[allow(clippy::wildcard_imports, reason = "many AST types used")]
use oxc_ast::ast::*;
use oxc_ast_visit::{Visit, walk};
use oxc_parser::Parser;
use oxc_semantic::ScopeFlags;
use oxc_span::{SourceType, Span};
use rustc_hash::FxHashMap;

/// A single static-inventory entry for one function.
///
/// `name` is beacon-compatible (see the module docs for the naming rule).
/// `line` is 1-based, matching the AST span start. The `start_column` /
/// `end_line` / `end_column` fields carry the function-node span in the
/// 1-indexed UTF-16 convention the cross-surface `FunctionIdentity` join key
/// expects (see `fallow_cov_protocol::FunctionIdentity::start_column`). They
/// are descriptive metadata: the join hash is `(file, name, line)` only, so
/// column fidelity never affects the join, only display / same-line
/// disambiguation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InventoryEntry {
    /// Beacon-compatible function name.
    pub name: String,
    /// 1-based source line of the function declaration (node `span.start`).
    pub line: u32,
    /// 1-indexed UTF-16 column of the function node start.
    pub start_column: u32,
    /// 1-based source line where the function node ends.
    pub end_line: u32,
    /// 1-indexed UTF-16 column of the function node end.
    pub end_column: u32,
    /// Content digest of the function's full-span source slice
    /// (`&source[span.start..span.end]`): first 8 bytes of SHA-256 as 16
    /// lowercase hex characters, via `fallow_cov_protocol::source_hash_for`.
    /// The slice is the canonical body bytes (signature line + body + closing
    /// brace, no whitespace normalization), identical for `Function` and
    /// `ArrowFunctionExpression`. Stable across line moves, so a
    /// moved-but-unedited function keeps the same hash.
    pub source_hash: String,
}

/// Visitor that collects [`InventoryEntry`] values in file traversal order.
struct InventoryVisitor<'a> {
    source: &'a str,
    line_offsets: &'a [u32],
    entries: Vec<InventoryEntry>,
    /// Parent-provided name override (method key, variable binding, etc.).
    pending_name: Option<String>,
    /// File-scoped monotonic counter for unnamed functions.
    anonymous_counter: u32,
}

impl<'a> InventoryVisitor<'a> {
    const fn new(source: &'a str, line_offsets: &'a [u32]) -> Self {
        Self {
            source,
            line_offsets,
            entries: Vec::new(),
            pending_name: None,
            anonymous_counter: 0,
        }
    }

    /// Resolve a function's name and advance the counter.
    ///
    /// Mirrors `oxc-coverage-instrument`'s two-step flow: `resolve_function_name`
    /// reads the current counter value for the anonymous-case name, and
    /// `add_function` advances the counter unconditionally on every
    /// instrumented function (named or not). We collapse both into one call.
    ///
    /// Name precedence: parent `pending_name` (method key / variable binding)
    /// → function's own `id` → counter.
    fn resolve_name(&mut self, explicit: Option<&str>) -> String {
        let n = self.anonymous_counter;
        self.anonymous_counter += 1;
        if let Some(pending) = self.pending_name.take() {
            return pending;
        }
        if let Some(name) = explicit {
            return name.to_owned();
        }
        format!("(anonymous_{n})")
    }

    fn record(&mut self, name: String, span: Span) {
        let (line, start_column) = self.line_col_utf16(span.start);
        let (end_line, end_column) = self.line_col_utf16(span.end);
        let source_hash = self
            .source
            .get(span.start as usize..span.end as usize)
            .map_or_else(
                || fallow_cov_protocol::source_hash_for(b""),
                |slice| fallow_cov_protocol::source_hash_for(slice.as_bytes()),
            );
        self.entries.push(InventoryEntry {
            name,
            line,
            start_column,
            end_line,
            end_column,
            source_hash,
        });
    }

    /// Map a UTF-8 byte offset to `(1-based line, 1-indexed UTF-16 column)`.
    ///
    /// The line comes from the precomputed offset table; the column counts
    /// UTF-16 code units from the line start to `byte_offset`, matching the
    /// `FunctionIdentity` column convention (Istanbul / V8 / oxc all normalize
    /// to 1-indexed UTF-16). A byte offset that does not fall on a char
    /// boundary (it always should for an AST span) clamps to the nearest
    /// boundary at or before it rather than panicking.
    fn line_col_utf16(&self, byte_offset: u32) -> (u32, u32) {
        let line_idx = match self.line_offsets.binary_search(&byte_offset) {
            Ok(idx) => idx,
            Err(idx) => idx.saturating_sub(1),
        };
        let line = line_idx as u32 + 1;
        let line_start = self.line_offsets[line_idx] as usize;
        let mut end = byte_offset as usize;
        while end > line_start && !self.source.is_char_boundary(end) {
            end -= 1;
        }
        let col_utf16 = self
            .source
            .get(line_start..end)
            .map_or(0, |slice| slice.encode_utf16().count());
        (line, col_utf16 as u32 + 1)
    }
}

impl<'ast> Visit<'ast> for InventoryVisitor<'_> {
    fn visit_function(&mut self, func: &Function<'ast>, flags: ScopeFlags) {
        if func.body.is_none() {
            walk::walk_function(self, func, flags);
            return;
        }
        let name = self.resolve_name(func.id.as_ref().map(|id| id.name.as_str()));
        self.record(name, func.span);
        walk::walk_function(self, func, flags);
    }

    fn visit_arrow_function_expression(&mut self, arrow: &ArrowFunctionExpression<'ast>) {
        let name = self.resolve_name(None);
        self.record(name, arrow.span);
        walk::walk_arrow_function_expression(self, arrow);
    }

    fn visit_method_definition(&mut self, method: &MethodDefinition<'ast>) {
        if let Some(name) = method.key.static_name() {
            self.pending_name = Some(name.to_string());
        }
        walk::walk_method_definition(self, method);
        self.pending_name = None;
    }

    fn visit_variable_declarator(&mut self, decl: &VariableDeclarator<'ast>) {
        if let Some(id) = decl.id.get_binding_identifier()
            && decl.init.as_ref().is_some_and(|init| {
                matches!(
                    init,
                    Expression::ArrowFunctionExpression(_) | Expression::FunctionExpression(_)
                )
            })
        {
            self.pending_name = Some(id.name.to_string());
        }
        walk::walk_variable_declarator(self, decl);
        self.pending_name = None;
    }

    fn visit_object_property(&mut self, prop: &ObjectProperty<'ast>) {
        self.pending_name = None;
        walk::walk_object_property(self, prop);
        self.pending_name = None;
    }
}

/// Per-function static complexity collected alongside the inventory walk.
///
/// Keyed to an [`InventoryEntry`] by its `source_hash`, which both this and the
/// inventory walk derive from the identical full-span byte slice over the same
/// parsed program (see [`InventoryEntry::source_hash`]). The hash is stable
/// across line moves, so the pairing survives reformatting that shifts line
/// numbers. `cyclomatic` and `cognitive` are descriptive context for downstream
/// importance weighting, never thresholds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InventoryComplexity {
    /// `McCabe` cyclomatic complexity (1 + decision points).
    pub cyclomatic: u16,
    /// `SonarSource` cognitive complexity (structural + nesting penalty).
    pub cognitive: u16,
}

/// Parse `source` at `path` and return every function as an [`InventoryEntry`].
///
/// Only plain JS/TS/JSX/TSX sources are supported. Callers should skip SFC,
/// Astro, MDX, CSS, HTML, and other non-JS inputs; those use different
/// instrumentation paths and are out of scope for the first inventory release.
///
/// Errors are swallowed: the returned vector covers whatever could be parsed.
/// This mirrors how the rest of the extract pipeline handles partial parse
/// results.
#[must_use]
pub fn walk_source(path: &Path, source: &str) -> Vec<InventoryEntry> {
    walk_source_with_complexity(path, source).0
}

/// Parse `source` at `path` once and return every function as an
/// [`InventoryEntry`] together with a `source_hash -> InventoryComplexity` map.
///
/// Both the inventory entries and the complexity map come from the SAME parse
/// (including the JSX fallback retry), so the per-function `source_hash` values
/// line up exactly and a caller can enrich each entry's metrics by a hash
/// lookup. Functions whose span slice could not be sliced share the empty-input
/// hash and simply don't pair; that degrades to "no metrics", never a panic.
///
/// Errors are swallowed, matching [`walk_source`]: the returned data covers
/// whatever could be parsed.
#[must_use]
pub fn walk_source_with_complexity(
    path: &Path,
    source: &str,
) -> (Vec<InventoryEntry>, FxHashMap<String, InventoryComplexity>) {
    let source_type = SourceType::from_path(path).unwrap_or_default();
    let line_offsets = fallow_types::extract::compute_line_offsets(source);

    let primary = walk_one_parse(source, source_type, &line_offsets);
    if primary.0.is_empty() && !source_type.is_jsx() {
        let jsx_type = if source_type.is_typescript() {
            SourceType::tsx()
        } else {
            SourceType::jsx()
        };
        let retry = walk_one_parse(source, jsx_type, &line_offsets);
        if !retry.0.is_empty() {
            return retry;
        }
    }

    primary
}

/// Run both the inventory and complexity visitors over a single parse of
/// `source` under `source_type`, pairing them by `source_hash`.
fn walk_one_parse(
    source: &str,
    source_type: SourceType,
    line_offsets: &[u32],
) -> (Vec<InventoryEntry>, FxHashMap<String, InventoryComplexity>) {
    let allocator = Allocator::default();
    let parser_return = Parser::new(&allocator, source, source_type).parse();

    let mut visitor = InventoryVisitor::new(source, line_offsets);
    visitor.visit_program(&parser_return.program);

    let complexity =
        crate::complexity::compute_complexity(&parser_return.program, source, line_offsets);
    let metrics: FxHashMap<String, InventoryComplexity> = complexity
        .into_iter()
        .filter_map(|fc| {
            fc.source_hash.map(|hash| {
                (
                    hash,
                    InventoryComplexity {
                        cyclomatic: fc.cyclomatic,
                        cognitive: fc.cognitive,
                    },
                )
            })
        })
        .collect();

    (visitor.entries, metrics)
}

#[cfg(all(test, not(miri)))]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn walk(source: &str) -> Vec<InventoryEntry> {
        walk_source(&PathBuf::from("test.ts"), source)
    }

    #[test]
    fn named_function_declaration_uses_its_own_name() {
        let entries = walk("function foo() { return 1; }");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "foo");
        assert_eq!(entries[0].line, 1);
    }

    #[test]
    fn const_arrow_captures_binding_name() {
        let entries = walk("const bar = () => 42;");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "bar");
    }

    #[test]
    fn const_function_expression_captures_binding_name_not_fn_id() {
        let entries = walk("const outer = function inner() { return 1; };");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "outer");
    }

    #[test]
    fn class_methods_use_method_names() {
        let entries = walk(
            r"
            class Foo {
              bar() { return 1; }
              baz() { return 2; }
            }",
        );
        let names: Vec<_> = entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, vec!["bar", "baz"]);
    }

    #[test]
    fn anonymous_arrow_passed_as_argument_uses_counter() {
        let entries = walk("setTimeout(() => { console.log('hi'); }, 10);");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "(anonymous_0)");
    }

    #[test]
    fn multiple_anonymous_functions_increment_counter_in_source_order() {
        let entries = walk(
            r"
            [1, 2, 3].map(() => 1);
            [4, 5, 6].filter(() => true);
            ",
        );
        let names: Vec<_> = entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, vec!["(anonymous_0)", "(anonymous_1)"]);
    }

    #[test]
    fn named_function_still_advances_counter_matching_instrumenter() {
        let entries = walk(
            r"
            function named() { return 1; }
            [1].map(() => 2);
            ",
        );
        let names: Vec<_> = entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, vec!["named", "(anonymous_1)"]);
    }

    #[test]
    fn anonymous_after_named_chain_uses_next_counter_value() {
        let entries = walk(
            r"
            function a() {}
            function b() {}
            function c() {}
            const d = () => 4;
            ",
        );
        let names: Vec<_> = entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, vec!["a", "b", "c", "d"]);
    }

    #[test]
    fn typescript_overload_signatures_dont_emit_or_advance_counter() {
        let entries = walk(
            r"
            function foo(): number;
            function foo(s: string): string;
            function foo(s?: string): number | string { return s ? s : 1; }
            [1].map(() => 2);
            ",
        );
        let names: Vec<_> = entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, vec!["foo", "(anonymous_1)"]);
    }

    #[test]
    fn export_default_named_function_keeps_explicit_name() {
        let entries = walk("export default function foo() { return 1; }");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "foo");
    }

    #[test]
    fn export_default_anonymous_function_uses_counter() {
        let entries = walk("export default function() { return 1; }");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "(anonymous_0)");
    }

    #[test]
    fn nested_function_numbered_after_parent_in_traversal_order() {
        let entries = walk(
            r"
            function outer() {
              return function() { return 1; };
            }",
        );
        let names: Vec<_> = entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, vec!["outer", "(anonymous_1)"]);
    }

    #[test]
    fn line_number_is_one_based_from_source_start() {
        let entries = walk("\n\nfunction atLineThree() {}");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].line, 3);
    }

    #[test]
    fn short_jsx_in_js_file_retries_with_jsx_parser() {
        let entries = walk_source(&PathBuf::from("component.js"), "const A = () => <div />;");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "A");
        assert_eq!(entries[0].line, 1);
    }

    #[test]
    fn object_method_shorthand_uses_anonymous_counter() {
        let entries = walk("const obj = { run() { return 1; } };");
        let names: Vec<_> = entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, vec!["(anonymous_0)"]);
    }

    #[test]
    fn class_property_arrow_uses_anonymous_counter() {
        let entries = walk(
            r"
            class Foo {
              bar = () => 1;
            }",
        );
        let names: Vec<_> = entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, vec!["(anonymous_0)"]);
    }

    #[test]
    fn records_one_indexed_utf16_columns() {
        let entries = walk("function foo() { return 1; }");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].start_column, 1);
        assert_eq!(entries[0].end_line, 1);
        assert!(entries[0].end_column > entries[0].start_column);
    }

    #[test]
    fn utf16_column_counts_code_units_not_bytes() {
        let entries = walk("const e = \"\u{1F600}\"; const f = () => 1;");
        let f = entries.iter().find(|e| e.name == "f").expect("f present");
        let byte_prefix_len = "const e = \"\u{1F600}\"; const f = ".len() as u32;
        assert!(f.start_column < byte_prefix_len + 1);
    }

    #[test]
    fn same_line_distinct_named_functions_have_distinct_positions() {
        let entries = walk("function a() {} function b() {}");
        let a = entries.iter().find(|e| e.name == "a").expect("a present");
        let b = entries.iter().find(|e| e.name == "b").expect("b present");
        assert_eq!(a.line, b.line, "both on line 1");
        assert_ne!(
            a.start_column, b.start_column,
            "same-line functions are column-disambiguated"
        );
    }

    #[test]
    fn same_line_anonymous_functions_stay_distinct_via_counter() {
        let entries = walk("const xs = [() => 1, () => 2];");
        let names: Vec<_> = entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, vec!["(anonymous_0)", "(anonymous_1)"]);
        assert_eq!(entries[0].line, entries[1].line, "both on line 1");
        assert_ne!(
            entries[0].name, entries[1].name,
            "counter keeps them distinct"
        );
    }

    #[test]
    fn source_hash_is_the_content_digest_of_the_function_span() {
        let src = "function foo() { return 1; }";
        let entries = walk(src);
        assert_eq!(entries.len(), 1);
        assert_eq!(
            entries[0].source_hash,
            fallow_cov_protocol::source_hash_for(src.as_bytes())
        );
        assert_eq!(entries[0].source_hash.len(), 16);
        assert!(
            entries[0]
                .source_hash
                .chars()
                .all(|c| c.is_ascii_hexdigit())
        );
    }

    #[test]
    fn source_hash_survives_line_moves_and_tracks_body_edits() {
        let original = walk("function foo() { return 1; }");
        let moved = walk("\n\nfunction foo() { return 1; }");
        assert_eq!(
            original[0].source_hash, moved[0].source_hash,
            "a moved-but-unedited function must keep its source_hash"
        );
        let edited = walk("function foo() { return 2; }");
        assert_ne!(
            original[0].source_hash, edited[0].source_hash,
            "an edited body must change the source_hash"
        );
    }
}
