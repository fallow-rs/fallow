use std::path::Path;

use oxc_allocator::Allocator;
use oxc_ast::ast::{Comment, Program};
use oxc_ast_visit::Visit;
use oxc_parser::Parser;
use oxc_span::SourceType;

use crate::ExportInfo;
use crate::ModuleInfo;
use crate::astro::{is_astro_file, parse_astro_to_module};
use crate::css::{is_css_file, parse_css_to_module};
use crate::html::{is_html_file, parse_html_to_module};
use crate::mdx::{is_mdx_file, parse_mdx_to_module};
use crate::sfc::{is_sfc_file, parse_sfc_to_module};
use crate::visitor::ModuleInfoExtractor;
use fallow_types::discover::FileId;
use fallow_types::extract::{ImportInfo, VisibilityTag};

/// Parse source text into a [`ModuleInfo`].
///
/// When `need_complexity` is false the per-function complexity visitor is
/// skipped, saving one full AST walk per file.  The dead-code analysis
/// pipeline never consumes complexity data, so callers that only need
/// imports/exports should pass `false`.
pub fn parse_source_to_module(
    file_id: FileId,
    path: &Path,
    source: &str,
    content_hash: u64,
    need_complexity: bool,
) -> ModuleInfo {
    if is_sfc_file(path) {
        return parse_sfc_to_module(file_id, path, source, content_hash);
    }
    if is_astro_file(path) {
        return parse_astro_to_module(file_id, source, content_hash);
    }
    if is_mdx_file(path) {
        return parse_mdx_to_module(file_id, source, content_hash);
    }
    if is_css_file(path) {
        return parse_css_to_module(file_id, path, source, content_hash);
    }
    if is_html_file(path) {
        return parse_html_to_module(file_id, source, content_hash);
    }

    let source_type = SourceType::from_path(path).unwrap_or_default();
    let allocator = Allocator::default();
    let parser_return = Parser::new(&allocator, source, source_type).parse();

    // Parse suppression comments from AST comments initially;
    // re-parsed from retry comments below if JSX retry succeeds.
    let mut suppressions =
        crate::suppress::parse_suppressions(&parser_return.program.comments, source);

    // Extract imports/exports even if there are parse errors
    let mut extractor = ModuleInfoExtractor::new();
    extractor.visit_program(&parser_return.program);

    // Detect unused import bindings via oxc_semantic scope analysis
    let mut unused_bindings =
        compute_unused_import_bindings(&parser_return.program, &extractor.imports);

    // Line offsets are always needed (error location reporting in analysis).
    let line_offsets = fallow_types::extract::compute_line_offsets(source);

    // Per-function complexity metrics: only computed when the caller needs them
    // (e.g. the `health` command).  The dead-code pipeline never reads this.
    let mut complexity = if need_complexity {
        crate::complexity::compute_complexity(&parser_return.program, &line_offsets)
    } else {
        Vec::new()
    };

    // Feature flag detection: always extracted (lightweight pattern matching).
    // Custom SDK patterns/env prefixes are applied post-parse via config.
    let mut flag_uses = crate::flags::extract_flags(
        &parser_return.program,
        &line_offsets,
        &[],   // built-in patterns only at parse time
        &[],   // built-in prefixes only at parse time
        false, // config object heuristics off at parse time (opt-in via config)
    );

    // If parsing produced very few results relative to source size (likely parse errors
    // from Flow types or JSX in .js files), retry with JSX/TSX source type as a fallback.
    let total_extracted =
        extractor.exports.len() + extractor.imports.len() + extractor.re_exports.len();
    let mut used_retry = false;
    if total_extracted == 0 && source.len() > 100 && !source_type.is_jsx() {
        let jsx_type = if source_type.is_typescript() {
            SourceType::tsx()
        } else {
            SourceType::jsx()
        };
        let allocator2 = Allocator::default();
        let retry_return = Parser::new(&allocator2, source, jsx_type).parse();
        let mut retry_extractor = ModuleInfoExtractor::new();
        retry_extractor.visit_program(&retry_return.program);
        let retry_total = retry_extractor.exports.len()
            + retry_extractor.imports.len()
            + retry_extractor.re_exports.len();
        if retry_total > total_extracted {
            unused_bindings =
                compute_unused_import_bindings(&retry_return.program, &retry_extractor.imports);
            // Recompute complexity from the successful retry parse (only if requested)
            if need_complexity {
                complexity =
                    crate::complexity::compute_complexity(&retry_return.program, &line_offsets);
            }
            // Recompute flag extraction from the successful retry parse
            flag_uses =
                crate::flags::extract_flags(&retry_return.program, &line_offsets, &[], &[], false);
            // Re-parse suppressions from the retry's comments (not the original failed parse)
            suppressions =
                crate::suppress::parse_suppressions(&retry_return.program.comments, source);
            // Apply visibility tags from the retry parse's comments (not the original failed parse)
            apply_jsdoc_visibility_tags(
                &mut retry_extractor.exports,
                &retry_return.program.comments,
                source,
            );
            // Extract JSDoc `import()` type references from the retry parse's comments
            extract_jsdoc_import_types(
                &mut retry_extractor.imports,
                &retry_return.program.comments,
                source,
            );
            extractor = retry_extractor;
            used_retry = true;
        }
    }

    // Apply JSDoc visibility tags from the original parse (skip if retry was used above)
    if !used_retry {
        apply_jsdoc_visibility_tags(
            &mut extractor.exports,
            &parser_return.program.comments,
            source,
        );
        extract_jsdoc_import_types(
            &mut extractor.imports,
            &parser_return.program.comments,
            source,
        );
    }

    let mut info = extractor.into_module_info(file_id, content_hash, suppressions);
    info.unused_import_bindings = unused_bindings;
    info.line_offsets = line_offsets;
    info.complexity = complexity;
    info.flag_uses = flag_uses;

    info
}

/// Apply JSDoc visibility tags (`@public`, `@internal`, `@alpha`, `@beta`) to exports by
/// matching leading JSDoc comments.
///
/// `Comment.attached_to` points to the `export` keyword byte offset, while
/// `ExportInfo.span` stores the identifier byte offset (e.g., `foo` in
/// `export const foo`). This function bridges the gap: it collects visibility
/// comment attachment offsets with their tag, then for each export finds the
/// nearest preceding attachment point and validates it's part of the same
/// export statement.
fn apply_jsdoc_visibility_tags(exports: &mut [ExportInfo], comments: &[Comment], source: &str) {
    if exports.is_empty() || comments.is_empty() {
        return;
    }

    // Collect byte offsets where visibility JSDoc comments attach, with tag.
    // Priority: Public > Internal > Alpha > Beta (if multiple tags on one comment).
    let mut tag_offsets: Vec<(u32, VisibilityTag)> = Vec::new();
    for comment in comments {
        if comment.is_jsdoc() {
            let content_span = comment.content_span();
            let start = content_span.start as usize;
            let end = (content_span.end as usize).min(source.len());
            if start < end {
                let text = &source[start..end];
                let tag = if has_public_tag(text) {
                    VisibilityTag::Public
                } else if has_internal_tag(text) {
                    VisibilityTag::Internal
                } else if has_alpha_tag(text) {
                    VisibilityTag::Alpha
                } else if has_beta_tag(text) {
                    VisibilityTag::Beta
                } else if has_expected_unused_tag(text) {
                    VisibilityTag::ExpectedUnused
                } else {
                    continue;
                };
                tag_offsets.push((comment.attached_to, tag));
            }
        }
    }

    if tag_offsets.is_empty() {
        return;
    }

    tag_offsets.sort_unstable_by_key(|&(offset, _)| offset);

    for export in exports.iter_mut() {
        // Skip synthetic exports (re-export entries with span 0..0)
        if export.span.start == 0 && export.span.end == 0 {
            continue;
        }

        // Check for exact match first (e.g., `export default` where span = decl span)
        if let Ok(idx) = tag_offsets.binary_search_by_key(&export.span.start, |&(o, _)| o) {
            export.visibility = tag_offsets[idx].1;
            continue;
        }

        // Find the largest tagged offset that is <= this export's span start
        let idx = tag_offsets.partition_point(|&(o, _)| o <= export.span.start);
        if idx > 0 {
            let (offset, tag) = tag_offsets[idx - 1];
            let offset = offset as usize;
            let export_start = export.span.start as usize;
            if offset < export_start && export_start <= source.len() {
                let between = &source[offset..export_start];
                // Validate: the text between the comment attachment and the identifier
                // should be a clean export preamble (e.g., "export const ") with no
                // statement boundaries separating them.
                if between.starts_with("export") && !between.contains(';') && !between.contains('}')
                {
                    export.visibility = tag;
                }
            }
        }
    }
}

/// Check if a JSDoc comment body contains an `@internal` tag.
fn has_internal_tag(comment_text: &str) -> bool {
    for (i, _) in comment_text.match_indices("@internal") {
        let after = i + "@internal".len();
        if after >= comment_text.len() || !is_ident_char(comment_text.as_bytes()[after]) {
            return true;
        }
    }
    false
}

/// Check if a JSDoc comment body contains a `@beta` tag.
fn has_beta_tag(comment_text: &str) -> bool {
    for (i, _) in comment_text.match_indices("@beta") {
        let after = i + "@beta".len();
        if after >= comment_text.len() || !is_ident_char(comment_text.as_bytes()[after]) {
            return true;
        }
    }
    false
}

/// Check if a JSDoc comment body contains an `@alpha` tag.
fn has_alpha_tag(comment_text: &str) -> bool {
    for (i, _) in comment_text.match_indices("@alpha") {
        let after = i + "@alpha".len();
        if after >= comment_text.len() || !is_ident_char(comment_text.as_bytes()[after]) {
            return true;
        }
    }
    false
}

/// Check if a JSDoc comment body contains an `@expected-unused` tag.
fn has_expected_unused_tag(comment_text: &str) -> bool {
    for (i, _) in comment_text.match_indices("@expected-unused") {
        let after = i + "@expected-unused".len();
        if after >= comment_text.len() || !is_ident_char(comment_text.as_bytes()[after]) {
            return true;
        }
    }
    false
}

/// Check if a byte is an identifier-continuation character (alphanumeric or `_`).
const fn is_ident_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// Scan JSDoc comments for `import('./path').Member` type expressions and push
/// them onto `imports` as type-only imports.
///
/// JSDoc supports referencing types from other modules via `import()` expressions
/// embedded in tag annotations, e.g.:
///
/// ```js
/// /**
///  * @param foo {import('./types.js').Foo}
///  * @returns {import('./types').Bar}
///  */
/// ```
///
/// Without this scanner, the referenced export (`Foo`, `Bar`) is flagged as
/// unused because no ES `import` statement binds it. The synthesized
/// `ImportInfo` has `is_type_only: true` and an empty `local_name` so it does
/// not interfere with `compute_unused_import_bindings` (which skips imports
/// with empty local names) and does not add a cyclic-dependency edge.
///
/// All JSDoc tag contexts (`@param`, `@returns`, `@type`, `@typedef`,
/// `@callback`, etc.) use the same `{type}` annotation syntax, so scanning
/// the full comment body covers every call site in a single pass.
fn extract_jsdoc_import_types(imports: &mut Vec<ImportInfo>, comments: &[Comment], source: &str) {
    if comments.is_empty() {
        return;
    }

    for comment in comments {
        if !comment.is_jsdoc() {
            continue;
        }
        let content_span = comment.content_span();
        let start = content_span.start as usize;
        let end = (content_span.end as usize).min(source.len());
        if start >= end {
            continue;
        }
        scan_jsdoc_imports_in(&source[start..end], imports);
    }
}

/// Parse a single JSDoc comment body for `import('...').Member` expressions.
///
/// Matches both single and double quoted path literals and extracts the first
/// identifier segment after `)\.` as the imported member name. Nested member
/// access (`import('./x').ns.Foo`) yields `ns` as the imported name, which is
/// correct for fallow's syntactic analysis since the resolver still adds the
/// edge to the target module.
fn scan_jsdoc_imports_in(body: &str, imports: &mut Vec<ImportInfo>) {
    let bytes = body.as_bytes();
    let mut cursor = 0;
    while let Some(rel) = body[cursor..].find("import(") {
        let open = cursor + rel + "import(".len();
        cursor = open;
        if open >= bytes.len() {
            break;
        }
        // Skip whitespace between `(` and the quote.
        let mut i = open;
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= bytes.len() {
            break;
        }
        let quote = bytes[i];
        if quote != b'\'' && quote != b'"' {
            continue;
        }
        let path_start = i + 1;
        let Some(rel_close) = body[path_start..].find(quote as char) else {
            break;
        };
        let path_end = path_start + rel_close;
        let path = &body[path_start..path_end];
        if path.is_empty() {
            cursor = path_end + 1;
            continue;
        }
        // Walk past the closing quote, optional whitespace, and the `)`.
        let mut j = path_end + 1;
        while j < bytes.len() && bytes[j].is_ascii_whitespace() {
            j += 1;
        }
        if j >= bytes.len() || bytes[j] != b')' {
            cursor = path_end + 1;
            continue;
        }
        j += 1;
        // Optional whitespace before `.`.
        while j < bytes.len() && bytes[j].is_ascii_whitespace() {
            j += 1;
        }
        cursor = j;
        if j >= bytes.len() || bytes[j] != b'.' {
            // `import('./x')` with no member access: treat as side-effect-style
            // reachability hint (still useful to keep the file reachable).
            imports.push(ImportInfo {
                source: path.to_string(),
                imported_name: fallow_types::extract::ImportedName::SideEffect,
                local_name: String::new(),
                is_type_only: true,
                span: oxc_span::Span::default(),
                source_span: oxc_span::Span::default(),
            });
            continue;
        }
        j += 1;
        // Parse the member identifier (first segment only).
        let name_start = j;
        while j < bytes.len() && is_ident_char(bytes[j]) {
            j += 1;
        }
        if name_start == j {
            continue;
        }
        let member = &body[name_start..j];
        cursor = j;
        imports.push(ImportInfo {
            source: path.to_string(),
            imported_name: fallow_types::extract::ImportedName::Named(member.to_string()),
            local_name: String::new(),
            is_type_only: true,
            span: oxc_span::Span::default(),
            source_span: oxc_span::Span::default(),
        });
    }
}

/// Check if a JSDoc comment body contains a `@public` or `@api public` tag.
fn has_public_tag(comment_text: &str) -> bool {
    // Check for @public (standalone tag, not part of another word)
    for (i, _) in comment_text.match_indices("@public") {
        let after = i + "@public".len();
        // Must not be followed by an identifier char (alphanumeric or _)
        if after >= comment_text.len() || !is_ident_char(comment_text.as_bytes()[after]) {
            return true;
        }
    }
    // Check for @api public (TSDoc convention)
    for (i, _) in comment_text.match_indices("@api") {
        let after = i + "@api".len();
        // @api must be a standalone tag (not @apipublic, @api_foo)
        if after < comment_text.len() && !is_ident_char(comment_text.as_bytes()[after]) {
            let rest = comment_text[after..].trim_start();
            if rest.starts_with("public") {
                let after_public = "public".len();
                if after_public >= rest.len() || !is_ident_char(rest.as_bytes()[after_public]) {
                    return true;
                }
            }
        }
    }
    false
}

/// Use `oxc_semantic` to find import bindings that are never referenced in the file.
///
/// An import like `import { foo } from './utils'` where `foo` is never used
/// anywhere in the file should not count as a reference to the `foo` export.
/// This improves unused-export detection precision.
///
/// Note: `get_resolved_references` counts both value-context and type-context
/// references. A value import used only as a type annotation (`const x: Foo`)
/// will have a type-position reference and will NOT appear in the unused list.
/// This is correct: `import { Foo }` (without `type`) may be needed at runtime.
pub fn compute_unused_import_bindings(
    program: &Program<'_>,
    imports: &[ImportInfo],
) -> Vec<String> {
    use oxc_semantic::SemanticBuilder;

    // Skip files with no imports
    if imports.is_empty() {
        return Vec::new();
    }

    let semantic_ret = SemanticBuilder::new().build(program);
    let semantic = semantic_ret.semantic;
    let scoping = semantic.scoping();
    let root_scope = scoping.root_scope_id();

    let mut unused = Vec::new();
    for import in imports {
        // Side-effect imports have no binding
        if import.local_name.is_empty() {
            continue;
        }
        // Look up the import binding in the module scope
        let name = oxc_str::Ident::from(import.local_name.as_str());
        if let Some(symbol_id) = scoping.get_binding(root_scope, name)
            && scoping.get_resolved_references(symbol_id).count() == 0
        {
            unused.push(import.local_name.clone());
        }
    }
    unused
}

#[cfg(test)]
mod tests {
    use super::{
        has_alpha_tag, has_beta_tag, has_internal_tag, has_public_tag, scan_jsdoc_imports_in,
    };
    use fallow_types::extract::{ImportInfo, ImportedName};

    // ── has_public_tag ────────────────────────────────────────────

    #[test]
    fn has_public_tag_matches_bare_tag() {
        assert!(has_public_tag(" * @public"));
    }

    #[test]
    fn has_public_tag_matches_api_public_variant() {
        assert!(has_public_tag(" * @api public"));
    }

    #[test]
    fn has_public_tag_rejects_partial_word() {
        assert!(!has_public_tag(" * @publicly"));
    }

    #[test]
    fn has_public_tag_rejects_at_apipublic() {
        assert!(!has_public_tag(" * @apipublic"));
    }

    #[test]
    fn has_public_tag_rejects_missing_at() {
        assert!(!has_public_tag(" * public"));
    }

    // ── has_internal_tag ──────────────────────────────────────────

    #[test]
    fn has_internal_tag_matches_bare_tag() {
        assert!(has_internal_tag(" * @internal"));
    }

    #[test]
    fn has_internal_tag_rejects_partial_word() {
        assert!(!has_internal_tag(" * @internalizer"));
    }

    #[test]
    fn has_internal_tag_rejects_missing_at() {
        assert!(!has_internal_tag(" * internal"));
    }

    // ── has_beta_tag ─────────────────────────────────────────────

    #[test]
    fn has_beta_tag_matches_bare_tag() {
        assert!(has_beta_tag(" * @beta"));
    }

    #[test]
    fn has_beta_tag_rejects_partial_word() {
        assert!(!has_beta_tag(" * @betaware"));
    }

    #[test]
    fn has_beta_tag_rejects_missing_at() {
        assert!(!has_beta_tag(" * beta"));
    }

    // ── has_alpha_tag ─────────────────────────────────────────────

    #[test]
    fn alpha_tag_standalone() {
        assert!(has_alpha_tag("@alpha"));
    }

    #[test]
    fn alpha_tag_with_text() {
        assert!(has_alpha_tag("@alpha Some description"));
    }

    #[test]
    fn alpha_tag_not_prefix() {
        assert!(!has_alpha_tag("@alphabet"));
    }

    #[test]
    fn has_alpha_tag_rejects_missing_at() {
        assert!(!has_alpha_tag(" * alpha"));
    }

    // ── scan_jsdoc_imports_in ─────────────────────────────────────

    fn scan(body: &str) -> Vec<ImportInfo> {
        let mut imports = Vec::new();
        scan_jsdoc_imports_in(body, &mut imports);
        imports
    }

    #[test]
    fn scan_jsdoc_single_import_with_member() {
        let imports = scan(" * @param foo {import('./types').Foo}");
        assert_eq!(imports.len(), 1);
        assert_eq!(imports[0].source, "./types");
        assert_eq!(
            imports[0].imported_name,
            ImportedName::Named("Foo".to_string())
        );
        assert!(imports[0].is_type_only);
        assert!(imports[0].local_name.is_empty());
    }

    #[test]
    fn scan_jsdoc_double_quoted_path() {
        let imports = scan(r#" * @type {import("./types").Foo}"#);
        assert_eq!(imports.len(), 1);
        assert_eq!(imports[0].source, "./types");
    }

    #[test]
    fn scan_jsdoc_multiple_imports_in_same_body() {
        let imports = scan(" * @param a {import('./a').A} @param b {import('./b').B}");
        assert_eq!(imports.len(), 2);
        assert_eq!(imports[0].source, "./a");
        assert_eq!(imports[1].source, "./b");
    }

    #[test]
    fn scan_jsdoc_union_annotation_captures_both_members() {
        let imports = scan(" * @type {import('./a').A | import('./b').B}");
        assert_eq!(imports.len(), 2);
        assert_eq!(
            imports[0].imported_name,
            ImportedName::Named("A".to_string())
        );
        assert_eq!(
            imports[1].imported_name,
            ImportedName::Named("B".to_string())
        );
    }

    #[test]
    fn scan_jsdoc_nested_member_uses_first_segment() {
        let imports = scan(" * @type {import('./types').ns.Foo}");
        assert_eq!(imports.len(), 1);
        assert_eq!(
            imports[0].imported_name,
            ImportedName::Named("ns".to_string())
        );
    }

    #[test]
    fn scan_jsdoc_parent_relative_path() {
        let imports = scan(" * @type {import('../lib/types.js').Foo}");
        assert_eq!(imports.len(), 1);
        assert_eq!(imports[0].source, "../lib/types.js");
    }

    #[test]
    fn scan_jsdoc_bare_package_specifier() {
        let imports = scan(" * @type {import('@scope/pkg').Client}");
        assert_eq!(imports.len(), 1);
        assert_eq!(imports[0].source, "@scope/pkg");
        assert_eq!(
            imports[0].imported_name,
            ImportedName::Named("Client".to_string())
        );
    }

    #[test]
    fn scan_jsdoc_without_member_is_side_effect() {
        let imports = scan(" * @type {import('./types')}");
        assert_eq!(imports.len(), 1);
        assert_eq!(imports[0].source, "./types");
        assert_eq!(imports[0].imported_name, ImportedName::SideEffect);
        assert!(imports[0].is_type_only);
    }

    #[test]
    fn scan_jsdoc_empty_path_is_skipped() {
        let imports = scan(" * @type {import('').Foo}");
        assert!(imports.is_empty());
    }

    #[test]
    fn scan_jsdoc_truncated_no_closing_quote_does_not_panic() {
        // `find(quote)` returns None, inner loop breaks, no panic.
        let imports = scan(" * @type {import('./truncated");
        assert!(imports.is_empty());
    }

    #[test]
    fn scan_jsdoc_missing_closing_paren_is_skipped() {
        // After the path, we expect `)`. If missing, skip this match and
        // continue the outer loop.
        let imports = scan(" * @type {import('./types'.Foo}");
        assert!(imports.is_empty());
    }

    #[test]
    fn scan_jsdoc_whitespace_between_paren_and_dot() {
        // `import('./types') .Foo` with whitespace before the dot.
        let imports = scan(" * @type {import('./types') .Foo}");
        assert_eq!(imports.len(), 1);
        assert_eq!(imports[0].source, "./types");
        assert_eq!(
            imports[0].imported_name,
            ImportedName::Named("Foo".to_string())
        );
    }

    #[test]
    fn scan_jsdoc_whitespace_between_paren_and_quote() {
        // `import( './types')` with whitespace between open-paren and the
        // quote — less common but valid in loose formatters.
        let imports = scan(" * @type {import( './types').Foo}");
        assert_eq!(imports.len(), 1);
        assert_eq!(imports[0].source, "./types");
    }

    #[test]
    fn scan_jsdoc_non_quote_after_paren_skipped() {
        // `import(identifier)` is not a string-literal form, so skip.
        let imports = scan(" * @type {import(foo).Bar}");
        assert!(imports.is_empty());
    }

    #[test]
    fn scan_jsdoc_ignores_prose_with_import_word() {
        // The word "import" not followed by `(` must not match.
        let imports = scan(" * This is an important note about imports.");
        assert!(imports.is_empty());
    }

    #[test]
    fn scan_jsdoc_utf8_path_works() {
        // Multi-byte characters in the path must not panic on slicing.
        let imports = scan(" * @type {import('./héllo').Foo}");
        assert_eq!(imports.len(), 1);
        assert_eq!(imports[0].source, "./héllo");
    }

    #[test]
    fn scan_jsdoc_empty_body_is_empty() {
        assert!(scan("").is_empty());
    }

    #[test]
    fn scan_jsdoc_no_import_in_body_is_empty() {
        assert!(scan(" * @param foo The foo parameter").is_empty());
    }

    #[test]
    fn scan_jsdoc_appends_to_existing_imports() {
        // Ensures the scanner appends rather than replaces.
        let mut imports = vec![ImportInfo {
            source: "existing".to_string(),
            imported_name: ImportedName::Default,
            local_name: "existing".to_string(),
            is_type_only: false,
            span: oxc_span::Span::default(),
            source_span: oxc_span::Span::default(),
        }];
        scan_jsdoc_imports_in(" * {import('./new').Foo}", &mut imports);
        assert_eq!(imports.len(), 2);
        assert_eq!(imports[0].source, "existing");
        assert_eq!(imports[1].source, "./new");
    }

    #[test]
    fn scan_jsdoc_ident_boundary_stops_at_bracket() {
        // Member parse stops at the first non-ident char (`}` here).
        let imports = scan(" * @type {import('./t').Abc}");
        assert_eq!(imports.len(), 1);
        assert_eq!(
            imports[0].imported_name,
            ImportedName::Named("Abc".to_string())
        );
    }

    #[test]
    fn scan_jsdoc_empty_member_name_is_skipped() {
        // `import('./x').` with no ident after the dot: member name is empty,
        // should be skipped (no ImportInfo pushed).
        let imports = scan(" * @type {import('./x').}");
        assert!(imports.is_empty());
    }
}
