use super::jsdoc_visibility::classify_jsdoc_visibility_tag;
use super::{advance_jsdoc_brace_stack, scan_jsdoc_import_tags_in, scan_jsdoc_imports_in};
use crate::parse_source_to_module;
use fallow_types::discover::FileId;
use fallow_types::extract::{ImportInfo, ImportedName, VisibilityTag};
use std::path::Path;

#[test]
fn classify_jsdoc_visibility_tag_requires_a_bare_tag() {
    let cases = [
        (" * @public", Some(VisibilityTag::Public)),
        (" * @api public", Some(VisibilityTag::Public)),
        (" * @publicly", None),
        (" * @apipublic", None),
        (" * public", None),
        (" * @internal", Some(VisibilityTag::Internal)),
        (" * @internal-only", Some(VisibilityTag::Internal)),
        (" * @internalizer", None),
        (" * @internalFoo", None),
        (" * internal", None),
        (" * @beta", Some(VisibilityTag::Beta)),
        (" * @betaware", None),
        (" * @beta_x", None),
        (" * beta", None),
        ("@alpha", Some(VisibilityTag::Alpha)),
        ("@alpha Some description", Some(VisibilityTag::Alpha)),
        ("@alphabet", None),
        (" * alpha", None),
        (" * @expected-unused", Some(VisibilityTag::ExpectedUnused)),
        (" * @expected-unusedX", None),
    ];
    for (text, expected) in cases {
        let actual = classify_jsdoc_visibility_tag(text).map(|(tag, _)| tag);
        assert_eq!(actual, expected, "{text:?}");
    }
}

#[test]
fn classify_jsdoc_visibility_tag_keeps_the_expected_unused_reason() {
    assert_eq!(
        classify_jsdoc_visibility_tag(" * @expected-unused -- kept for the plugin API"),
        Some((
            VisibilityTag::ExpectedUnused,
            Some("kept for the plugin API".to_string())
        ))
    );
    assert_eq!(
        classify_jsdoc_visibility_tag(" * @expected-unused"),
        Some((VisibilityTag::ExpectedUnused, None))
    );
}

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
fn script_auto_import_candidates_capture_zero_import_value_refs() {
    let info = parse_source_to_module(
        FileId(0),
        Path::new("pages/index.ts"),
        r"
                useCounter();
                const price = formatPrice(10);
                const localOnly = () => null;
                localOnly();
                type Local = UseTypeOnly;
            ",
        0,
        false,
    );

    assert!(
        info.auto_import_candidates
            .contains(&"formatPrice".to_string())
    );
    assert!(
        info.auto_import_candidates
            .contains(&"useCounter".to_string())
    );
    assert!(
        !info
            .auto_import_candidates
            .contains(&"UseTypeOnly".to_string())
    );
    assert!(
        !info
            .auto_import_candidates
            .contains(&"localOnly".to_string())
    );
}

#[test]
fn script_auto_import_candidates_skip_explicit_imports() {
    let info = parse_source_to_module(
        FileId(0),
        Path::new("pages/index.ts"),
        "import { useCounter } from '../composables/useCounter';\nuseCounter();\nuseOther();\n",
        0,
        false,
    );

    assert!(
        !info
            .auto_import_candidates
            .contains(&"useCounter".to_string())
    );
    assert!(
        info.auto_import_candidates
            .contains(&"useOther".to_string())
    );
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

fn scan_tags(body: &str) -> Vec<(String, ImportedName)> {
    let mut imports = Vec::new();
    let mut namespaces = Vec::new();
    scan_jsdoc_import_tags_in(body, &mut imports, &mut namespaces);
    assert!(namespaces.is_empty());
    assert!(imports.iter().all(|import| import.is_type_only));
    assert!(imports.iter().all(|import| import.local_name.is_empty()));
    imports
        .into_iter()
        .map(|import| (import.source, import.imported_name))
        .collect()
}

fn named(source: &str, name: &str) -> (String, ImportedName) {
    (source.to_string(), ImportedName::Named(name.to_string()))
}

#[test]
fn scan_jsdoc_import_tag_named_bindings() {
    assert_eq!(
        scan_tags(" @import { A, B as C, type D, 'e-f' as E } from '../types/foo' "),
        vec![
            named("../types/foo", "A"),
            named("../types/foo", "B"),
            named("../types/foo", "D"),
            named("../types/foo", "e-f"),
        ]
    );
}

#[test]
fn scan_jsdoc_import_tag_namespace_default_and_mixed() {
    assert_eq!(
        scan_tags(" @import Def from './def' "),
        vec![("./def".to_string(), ImportedName::Default)]
    );
    assert_eq!(
        scan_tags(" @import Def, { $A, default as B } from './mixed' "),
        vec![
            ("./mixed".to_string(), ImportedName::Default),
            named("./mixed", "$A"),
            ("./mixed".to_string(), ImportedName::Default),
        ]
    );
    assert_eq!(
        scan_tags(" @import {} from './empty' "),
        vec![("./empty".to_string(), ImportedName::SideEffect)]
    );
}

fn jsdoc_tag_imports(source: &str) -> Vec<(String, ImportedName)> {
    let info = parse_source_to_module(FileId(0), Path::new("src/index.js"), source, 0, false);
    info.imports
        .into_iter()
        .filter(|import| import.is_type_only && import.local_name.is_empty())
        .map(|import| (import.source, import.imported_name))
        .collect()
}

#[test]
fn jsdoc_namespace_import_tag_credits_only_the_members_in_types() {
    let source = r#"/** @import * as ns from "./ns" */
/** @import * as unread from './unread' */
/**
 * Read ns.Prose in a sentence: no credit.
 * @param {ns.Shape | ns.Line} a
 * @returns {ns.Shape}
 */
export function draw(a) { return a; }
/** @type {Array<ns.Point>} */
export const points = [];
"#;
    assert_eq!(
        jsdoc_tag_imports(source),
        vec![
            ("./ns".to_string(), ImportedName::SideEffect),
            ("./unread".to_string(), ImportedName::SideEffect),
            named("./ns", "Shape"),
            named("./ns", "Line"),
            named("./ns", "Point"),
        ]
    );
}

#[test]
fn jsdoc_namespace_import_tag_keeps_default_binding() {
    assert_eq!(
        jsdoc_tag_imports(
            "/** @import Def, * as ns from './mod' */
/** @type {ns.A} */
export const a = 1;
"
        ),
        vec![
            ("./mod".to_string(), ImportedName::Default),
            ("./mod".to_string(), ImportedName::SideEffect),
            named("./mod", "A"),
        ]
    );
}

#[test]
fn scan_jsdoc_import_tag_spans_lines_and_tags() {
    let body = "\n * @import {\n *   A,\n *   B,\n * } from './multi'\n * @import { C } from './next'\n * @param {A} a\n ";
    assert_eq!(
        scan_tags(body),
        vec![
            named("./multi", "A"),
            named("./multi", "B"),
            named("./next", "C"),
        ]
    );
}

#[test]
fn scan_jsdoc_import_tag_ignores_non_tags_and_bad_clauses() {
    for body in [
        " Use @import { A } from './prose' in a sentence ",
        " @imports { A } from './plural' ",
        " @import { A } './missing-from' ",
        " @import { A } from '' ",
        " @import { A from './unclosed' ",
        " @import { A }\n * @param {A} from './next-tag'",
        " @import { A } from './truncated",
        " @import",
        " @import * from './no-alias' ",
    ] {
        assert!(scan_tags(body).is_empty(), "{body:?}");
    }
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
    let imports = scan(" * @type {import('./truncated");
    assert!(imports.is_empty());
}

#[test]
fn scan_jsdoc_missing_closing_paren_is_skipped() {
    let imports = scan(" * @type {import('./types'.Foo}");
    assert!(imports.is_empty());
}

#[test]
fn scan_jsdoc_whitespace_between_paren_and_dot() {
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
    let imports = scan(" * @type {import( './types').Foo}");
    assert_eq!(imports.len(), 1);
    assert_eq!(imports[0].source, "./types");
}

#[test]
fn scan_jsdoc_non_quote_after_paren_skipped() {
    let imports = scan(" * @type {import(foo).Bar}");
    assert!(imports.is_empty());
}

#[test]
fn scan_jsdoc_ignores_prose_with_import_word() {
    let imports = scan(" * This is an important note about imports.");
    assert!(imports.is_empty());
}

#[test]
fn scan_jsdoc_utf8_path_works() {
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

/// Regression: `import('...')` in JSDoc prose (outside any `{...}` brace
/// group) is documentation/example syntax, not a type annotation. It must
/// not be reported as a real import. Without this scoping check, files
/// whose header doc documents which import forms they handle would surface
/// false-positive unresolved-import findings.
#[test]
fn scan_jsdoc_prose_import_outside_braces_is_skipped() {
    // Mirrors the exact shape of an extractor's header doc that lists
    // import forms as bullet-point examples.
    let body = "\n * Handles:\n * - Dynamic imports (await import('./prose')) \n * - Barrel exports (export * from './prose')\n";
    let imports = scan(body);
    assert!(
        imports.is_empty(),
        "prose import() should not be matched; got: {:?}",
        imports
            .iter()
            .map(|i| i.source.as_str())
            .collect::<Vec<_>>()
    );
}

#[test]
fn scan_jsdoc_prose_import_inside_example_object_is_skipped() {
    let body = "\n * @example\n * const loaders = {\n *   admin: () => import('./prose')\n * }";
    let imports = scan(body);
    assert!(
        imports.is_empty(),
        "object-literal example import() should not be matched; got: {:?}",
        imports
            .iter()
            .map(|i| i.source.as_str())
            .collect::<Vec<_>>()
    );
}

#[test]
fn scan_jsdoc_prose_import_inside_inline_braces_is_skipped() {
    let imports = scan(" * Use {import('./prose')} as an example string.");
    assert!(imports.is_empty());
}

#[test]
fn scan_jsdoc_bare_example_brace_import_is_skipped() {
    let imports = scan("\n * @example\n * { import('./prose') }\n");
    assert!(imports.is_empty());
}

/// A real `{@type ...}` annotation following a prose mention of `import()`
/// must still be matched. The fix narrows scope without breaking the
/// intended JSDoc type-annotation behavior.
#[test]
fn scan_jsdoc_braced_import_after_prose_is_still_matched() {
    let body = " * Note: dynamic imports like import('./prose') are not types.\n * @type {import('./real').Foo}";
    let imports = scan(body);
    assert_eq!(imports.len(), 1, "got: {imports:?}");
    assert_eq!(imports[0].source, "./real");
    assert_eq!(
        imports[0].imported_name,
        ImportedName::Named("Foo".to_string())
    );
}

#[test]
fn scan_jsdoc_multiline_braced_type_tag_is_still_matched() {
    let body = "\n * @returns {\n *   import('./real').Foo\n * }";
    let imports = scan(body);
    assert_eq!(imports.len(), 1, "got: {imports:?}");
    assert_eq!(imports[0].source, "./real");
    assert_eq!(
        imports[0].imported_name,
        ImportedName::Named("Foo".to_string())
    );
}

#[test]
fn scan_jsdoc_type_tag_before_brace_line_is_still_matched() {
    let body = "\n * @type\n * { import('./real').Foo }\n";
    let imports = scan(body);
    assert_eq!(imports.len(), 1, "got: {imports:?}");
    assert_eq!(imports[0].source, "./real");
    assert_eq!(
        imports[0].imported_name,
        ImportedName::Named("Foo".to_string())
    );
}

#[test]
fn scan_jsdoc_satisfies_type_tag_is_still_matched() {
    let imports = scan(" * @satisfies {import('./real').Foo}");
    assert_eq!(imports.len(), 1, "got: {imports:?}");
    assert_eq!(imports[0].source, "./real");
    assert_eq!(
        imports[0].imported_name,
        ImportedName::Named("Foo".to_string())
    );
}

#[test]
fn scan_jsdoc_template_constraint_type_tag_is_still_matched() {
    let imports = scan(" * @template {import('./real').Foo} T");
    assert_eq!(imports.len(), 1, "got: {imports:?}");
    assert_eq!(imports[0].source, "./real");
    assert_eq!(
        imports[0].imported_name,
        ImportedName::Named("Foo".to_string())
    );
}

#[test]
fn scan_jsdoc_enum_type_tag_is_still_matched() {
    let imports = scan(" * @enum {import('./real').Foo}");
    assert_eq!(imports.len(), 1, "got: {imports:?}");
    assert_eq!(imports[0].source, "./real");
    assert_eq!(
        imports[0].imported_name,
        ImportedName::Named("Foo".to_string())
    );
}

#[test]
fn scan_jsdoc_appends_to_existing_imports() {
    let mut imports = vec![ImportInfo {
        source: "existing".to_string(),
        imported_name: ImportedName::Default,
        local_name: "existing".to_string(),
        is_type_only: false,
        is_type_only_star: false,
        from_style: false,
        span: oxc_span::Span::default(),
        source_span: oxc_span::Span::default(),
    }];
    scan_jsdoc_imports_in(" * @type {import('./new').Foo}", &mut imports);
    assert_eq!(imports.len(), 2);
    assert_eq!(imports[0].source, "existing");
    assert_eq!(imports[1].source, "./new");
}

#[test]
fn scan_jsdoc_ident_boundary_stops_at_bracket() {
    let imports = scan(" * @type {import('./t').Abc}");
    assert_eq!(imports.len(), 1);
    assert_eq!(
        imports[0].imported_name,
        ImportedName::Named("Abc".to_string())
    );
}

#[test]
fn scan_jsdoc_empty_member_name_is_skipped() {
    let imports = scan(" * @type {import('./x').}");
    assert!(imports.is_empty());
}

#[test]
fn scan_jsdoc_many_imports_incremental_brace_stack_is_identical() {
    // Regression for the issue #1843 follow-up: the enclosing-brace lookup
    // is maintained incrementally across the whole comment rather than
    // rescanning every prefix. A comment packed with many `import(...)` type
    // refs must still extract exactly one import per `{...}` type group, in
    // order, with the same paths and member names as before.
    use std::fmt::Write as _;
    let mut body = String::from("/**\n");
    for i in 0..200 {
        let _ = writeln!(body, " * @param a{i} {{import('./m{i}').T{i}}} description");
    }
    // A prose `import(` outside any type brace group and a nested brace
    // must not add spurious imports or shift the enclosing-brace tracking.
    body.push_str(" * @remarks import('./ignored') appears in prose here\n");
    body.push_str(" * @typedef {{ nested: { deep: import('./deep').D } }} Obj\n");
    body.push_str(" */\n");

    let imports = scan(&body);
    assert_eq!(imports.len(), 201, "got: {imports:?}");
    for (i, import) in imports.iter().take(200).enumerate() {
        assert_eq!(import.source, format!("./m{i}"));
        assert_eq!(import.imported_name, ImportedName::Named(format!("T{i}")));
        assert!(import.is_type_only);
        assert!(import.local_name.is_empty());
    }
    // The nested-brace occurrence still resolves against its enclosing group.
    assert_eq!(imports[200].source, "./deep");
    assert_eq!(
        imports[200].imported_name,
        ImportedName::Named("D".to_string())
    );
}

#[test]
fn scan_jsdoc_brace_stack_matches_offset_zero_rescan() {
    // Cross-checks the incremental brace stack against an independent
    // offset-zero rescan over the full prefix, on inputs where the
    // `import(` cursor skips over intervening braces (issue #1843 follow-up).
    let cases = [
        " * @type {import('./a').A} and {plain} then {import('./b').B}",
        " * @remarks { import('./skip') } @param x {import('./c').C}",
        " * text } stray close { import('./d').D } trailing",
        " * @type {{ a: import('./e').E, b: { c: import('./f').F } }}",
    ];
    for body in cases {
        let bytes = body.as_bytes();
        let mut cursor = 0;
        while let Some(rel) = body[cursor..].find("import(") {
            let import_pos = cursor + rel;
            // Independent offset-zero rescan reproducing the old helper.
            let mut fresh = Vec::new();
            for (idx, &b) in bytes[..import_pos].iter().enumerate() {
                match b {
                    b'{' => fresh.push(idx),
                    b'}' => {
                        fresh.pop();
                    }
                    _ => {}
                }
            }
            let mut stack = Vec::new();
            let mut scanned = 0;
            advance_jsdoc_brace_stack(bytes, &mut stack, &mut scanned, import_pos);
            assert_eq!(
                stack.last().copied(),
                fresh.last().copied(),
                "enclosing brace mismatch at {import_pos} in {body:?}"
            );
            cursor = import_pos + "import(".len();
        }
    }
}
