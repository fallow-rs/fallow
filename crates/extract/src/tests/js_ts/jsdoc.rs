use std::fmt::Write as _;

use fallow_types::extract::{ExportName, ImportedName, VisibilityTag};

use crate::tests::parse_ts as parse_source;

#[test]
fn jsdoc_public_tag_on_named_export() {
    let info = parse_source("/** @public */\nexport const foo = 1;");
    assert_eq!(info.exports.len(), 1);
    assert_eq!(info.exports[0].visibility, VisibilityTag::Public);
}

#[test]
fn jsdoc_public_tag_on_function_export() {
    let info = parse_source("/** @public */\nexport function bar() {}");
    assert_eq!(info.exports.len(), 1);
    assert_eq!(info.exports[0].visibility, VisibilityTag::Public);
}

#[test]
fn jsdoc_public_tag_on_default_export() {
    let info = parse_source("/** @public */\nexport default function main() {}");
    assert_eq!(info.exports.len(), 1);
    assert_eq!(info.exports[0].visibility, VisibilityTag::Public);
}

#[test]
fn jsdoc_public_tag_on_class_export() {
    let info = parse_source("/** @public */\nexport class Foo {}");
    assert_eq!(info.exports.len(), 1);
    assert_eq!(info.exports[0].visibility, VisibilityTag::Public);
}

#[test]
fn jsdoc_public_tag_on_type_export() {
    let info = parse_source("/** @public */\nexport type Foo = string;");
    assert_eq!(info.exports.len(), 1);
    assert_eq!(info.exports[0].visibility, VisibilityTag::Public);
}

#[test]
fn jsdoc_public_tag_on_interface_export() {
    let info = parse_source("/** @public */\nexport interface Bar {}");
    assert_eq!(info.exports.len(), 1);
    assert_eq!(info.exports[0].visibility, VisibilityTag::Public);
}

#[test]
fn jsdoc_public_tag_on_enum_export() {
    let info = parse_source("/** @public */\nexport enum Status { Active, Inactive }");
    assert_eq!(info.exports.len(), 1);
    assert_eq!(info.exports[0].visibility, VisibilityTag::Public);
}

#[test]
fn jsdoc_public_tag_multiline() {
    let info = parse_source("/**\n * Some description.\n * @public\n */\nexport const foo = 1;");
    assert_eq!(info.exports.len(), 1);
    assert_eq!(info.exports[0].visibility, VisibilityTag::Public);
}

#[test]
fn jsdoc_public_tag_with_other_tags() {
    let info = parse_source("/** @deprecated @public */\nexport const foo = 1;");
    assert_eq!(info.exports.len(), 1);
    assert_eq!(info.exports[0].visibility, VisibilityTag::Public);
}

#[test]
fn jsdoc_api_public_tag() {
    let info = parse_source("/** @api public */\nexport const foo = 1;");
    assert_eq!(info.exports.len(), 1);
    assert_eq!(info.exports[0].visibility, VisibilityTag::Public);
}

#[test]
fn no_jsdoc_tag_not_public() {
    let info = parse_source("export const foo = 1;");
    assert_eq!(info.exports.len(), 1);
    assert_eq!(info.exports[0].visibility, VisibilityTag::None);
}

#[test]
fn line_comment_not_jsdoc() {
    let info = parse_source("// @public\nexport const foo = 1;");
    assert_eq!(info.exports.len(), 1);
    assert_eq!(info.exports[0].visibility, VisibilityTag::None);
}

#[test]
fn jsdoc_public_does_not_match_public_foo() {
    let info = parse_source("/** @publicFoo */\nexport const foo = 1;");
    assert_eq!(info.exports.len(), 1);
    assert_eq!(info.exports[0].visibility, VisibilityTag::None);
}

#[test]
fn jsdoc_public_does_not_match_public_underscore() {
    let info = parse_source("/** @public_api */\nexport const foo = 1;");
    assert_eq!(info.exports.len(), 1);
    assert_eq!(info.exports[0].visibility, VisibilityTag::None);
}

#[test]
fn jsdoc_apipublic_no_space_does_not_match() {
    let info = parse_source("/** @apipublic */\nexport const foo = 1;");
    assert_eq!(info.exports.len(), 1);
    assert_eq!(info.exports[0].visibility, VisibilityTag::None);
}

#[test]
fn jsdoc_public_on_export_specifier_list() {
    let source = "const foo = 1;\nconst bar = 2;\n/** @public */\nexport { foo, bar };";
    let info = parse_source(source);
    assert_eq!(info.exports.len(), 2);
    assert_eq!(info.exports[0].visibility, VisibilityTag::Public);
    assert_eq!(info.exports[1].visibility, VisibilityTag::Public);
}

#[test]
fn jsdoc_public_only_applies_to_attached_export() {
    let source = "/** @public */\nexport const foo = 1;\nexport const bar = 2;";
    let info = parse_source(source);
    assert_eq!(info.exports.len(), 2);
    assert_eq!(info.exports[0].visibility, VisibilityTag::Public);
    assert_eq!(info.exports[1].visibility, VisibilityTag::None);
}

#[test]
fn jsdoc_public_block_comment_not_jsdoc() {
    let info = parse_source("/* @public */\nexport const foo = 1;");
    assert_eq!(info.exports.len(), 1);
    assert_eq!(info.exports[0].visibility, VisibilityTag::None);
}

#[test]
fn jsdoc_public_on_anonymous_default_export() {
    let info = parse_source("/** @public */\nexport default function() {}");
    assert_eq!(info.exports.len(), 1);
    assert_eq!(info.exports[0].name, ExportName::Default);
    assert_eq!(info.exports[0].visibility, VisibilityTag::Public);
}

#[test]
fn jsdoc_public_on_arrow_default_export() {
    let info = parse_source("/** @public */\nexport default () => {};");
    assert_eq!(info.exports.len(), 1);
    assert_eq!(info.exports[0].name, ExportName::Default);
    assert_eq!(info.exports[0].visibility, VisibilityTag::Public);
}

#[test]
fn jsdoc_public_on_default_expression_export() {
    let info = parse_source("/** @public */\nexport default 42;");
    assert_eq!(info.exports.len(), 1);
    assert_eq!(info.exports[0].name, ExportName::Default);
    assert_eq!(info.exports[0].visibility, VisibilityTag::Public);
}

#[test]
fn jsdoc_public_on_let_export() {
    let info = parse_source("/** @public */\nexport let count = 0;");
    assert_eq!(info.exports.len(), 1);
    assert_eq!(info.exports[0].name, ExportName::Named("count".to_string()));
    assert_eq!(info.exports[0].visibility, VisibilityTag::Public);
}

#[test]
fn jsdoc_public_with_trailing_description() {
    let info = parse_source("/** @public This is always exported */\nexport const foo = 1;");
    assert_eq!(info.exports.len(), 1);
    assert_eq!(info.exports[0].visibility, VisibilityTag::Public);
}

#[test]
fn jsdoc_api_public_with_extra_whitespace() {
    let info = parse_source("/** @api   public */\nexport const foo = 1;");
    assert_eq!(info.exports.len(), 1);
    assert_eq!(info.exports[0].visibility, VisibilityTag::Public);
}

#[test]
fn jsdoc_api_public_with_newline() {
    let info = parse_source("/**\n * @api\n * public\n */\nexport const foo = 1;");
    assert_eq!(info.exports.len(), 1);
    assert_eq!(info.exports[0].visibility, VisibilityTag::None);
}

#[test]
fn jsdoc_api_publicfoo_does_not_match() {
    let info = parse_source("/** @api publicFoo */\nexport const foo = 1;");
    assert_eq!(info.exports.len(), 1);
    assert_eq!(info.exports[0].visibility, VisibilityTag::None);
}

#[test]
fn jsdoc_public_multiple_exports_all_tagged() {
    let source = "/** @public */\nexport const a = 1;\n/** @public */\nexport const b = 2;";
    let info = parse_source(source);
    assert_eq!(info.exports.len(), 2);
    assert_eq!(info.exports[0].visibility, VisibilityTag::Public);
    assert_eq!(info.exports[1].visibility, VisibilityTag::Public);
}

#[test]
fn jsdoc_public_mixed_three_exports() {
    let source = "/** @public */\nexport const a = 1;\nexport const b = 2;\n/** @public */\nexport const c = 3;";
    let info = parse_source(source);
    assert_eq!(info.exports.len(), 3);
    assert_eq!(info.exports[0].visibility, VisibilityTag::Public);
    assert_eq!(info.exports[1].visibility, VisibilityTag::None);
    assert_eq!(info.exports[2].visibility, VisibilityTag::Public);
}

#[test]
fn jsdoc_public_does_not_match_numeric_suffix() {
    let info = parse_source("/** @public2 */\nexport const foo = 1;");
    assert_eq!(info.exports.len(), 1);
    assert_eq!(info.exports[0].visibility, VisibilityTag::None);
}

#[test]
fn jsdoc_public_on_async_function_export() {
    let info = parse_source("/** @public */\nexport async function fetchData() {}");
    assert_eq!(info.exports.len(), 1);
    assert_eq!(info.exports[0].visibility, VisibilityTag::Public);
}

#[test]
fn jsdoc_public_on_abstract_class_export() {
    let info = parse_source("/** @public */\nexport abstract class Base {}");
    assert_eq!(info.exports.len(), 1);
    assert_eq!(info.exports[0].visibility, VisibilityTag::Public);
}

#[test]
fn jsdoc_public_star_prefix_in_multiline() {
    let info = parse_source(
        "/**\n * @param x - the value\n * @returns the result\n * @public\n */\nexport const foo = 1;",
    );
    assert_eq!(info.exports.len(), 1);
    assert_eq!(info.exports[0].visibility, VisibilityTag::Public);
}

#[test]
fn jsdoc_public_on_type_alias_union() {
    let info = parse_source("/** @public */\nexport type Status = 'active' | 'inactive';");
    assert_eq!(info.exports.len(), 1);
    assert_eq!(info.exports[0].visibility, VisibilityTag::Public);
}

#[test]
fn jsdoc_api_public_on_function() {
    let info = parse_source("/** @api public */\nexport function handler() {}");
    assert_eq!(info.exports.len(), 1);
    assert_eq!(info.exports[0].visibility, VisibilityTag::Public);
}

#[test]
fn jsdoc_api_private_does_not_set_public() {
    let info = parse_source("/** @api private */\nexport const foo = 1;");
    assert_eq!(info.exports.len(), 1);
    assert_eq!(info.exports[0].visibility, VisibilityTag::None);
}

#[test]
fn jsdoc_public_not_leaked_across_statements() {
    let source = "/** @public */\nconst internal = 1;\nexport const foo = internal;";
    let info = parse_source(source);
    assert_eq!(info.exports.len(), 1);
    assert_eq!(info.exports[0].visibility, VisibilityTag::None);
}

#[test]
fn jsdoc_public_tag_marks_export_public() {
    let info = parse_source(
        r"/** @public */
export const foo = 1;",
    );
    assert_eq!(info.exports.len(), 1);
    assert_eq!(
        info.exports[0].visibility,
        VisibilityTag::Public,
        "Export with @public JSDoc tag should be marked as public"
    );
}

#[test]
fn jsdoc_api_public_tag_marks_export_public() {
    let info = parse_source(
        r"/** @api public */
export const bar = 2;",
    );
    assert_eq!(info.exports.len(), 1);
    assert_eq!(
        info.exports[0].visibility,
        VisibilityTag::Public,
        "Export with @api public tag should be marked as public"
    );
}

#[test]
fn jsdoc_no_public_tag_not_marked() {
    let info = parse_source(
        r"/** Regular comment */
export const baz = 3;",
    );
    assert_eq!(info.exports.len(), 1);
    assert_eq!(
        info.exports[0].visibility,
        VisibilityTag::None,
        "Export without @public tag should not be marked as public"
    );
}

#[test]
fn jsdoc_public_partial_word_not_matched() {
    let info = parse_source(
        r"/** @publicize this */
export const qux = 4;",
    );
    assert_eq!(info.exports.len(), 1);
    assert_eq!(
        info.exports[0].visibility,
        VisibilityTag::None,
        "@publicize should not match @public (it's followed by an ident char)"
    );
}

#[test]
fn jsdoc_public_on_function_export() {
    let info = parse_source(
        r"/** @public */
export function myFunc() { return 1; }",
    );
    let f = info
        .exports
        .iter()
        .find(|e| matches!(&e.name, ExportName::Named(n) if n == "myFunc"));
    assert!(f.is_some());
    assert_eq!(
        f.unwrap().visibility,
        VisibilityTag::Public,
        "Function export with @public should be marked as public"
    );
}

#[test]
fn jsdoc_public_on_class_export() {
    let info = parse_source(
        r"/** @public */
export class MyClass { doWork() {} }",
    );
    let c = info
        .exports
        .iter()
        .find(|e| matches!(&e.name, ExportName::Named(n) if n == "MyClass"));
    assert!(c.is_some());
    assert_eq!(c.unwrap().visibility, VisibilityTag::Public);
}

#[test]
fn export_without_jsdoc_not_public() {
    let info = parse_source("export const plain = 42;");
    assert_eq!(info.exports.len(), 1);
    assert_eq!(info.exports[0].visibility, VisibilityTag::None);
}

#[test]
fn jsdoc_import_type_in_param_recorded_as_type_import() {
    let info = parse_source(
        "/**\n * @param foo {import('./types.js').Foo}\n */\nfunction bar(foo) { return foo; }",
    );
    let imp = info
        .imports
        .iter()
        .find(|i| i.source == "./types.js")
        .expect("JSDoc import() should produce an ImportInfo");
    assert!(imp.is_type_only);
    assert_eq!(imp.imported_name, ImportedName::Named("Foo".to_string()));
    assert!(imp.local_name.is_empty());
}

#[test]
fn jsdoc_import_type_double_quoted_path() {
    let info = parse_source("/**\n * @type {import(\"./types\").Foo}\n */\nlet x;");
    let imp = info.imports.iter().find(|i| i.source == "./types");
    assert!(imp.is_some());
    assert_eq!(
        imp.unwrap().imported_name,
        ImportedName::Named("Foo".to_string())
    );
}

#[test]
fn jsdoc_import_type_returns_tag() {
    let info = parse_source(
        "/**\n * @returns {import('./types').Bar}\n */\nfunction make() { return null; }",
    );
    let imp = info.imports.iter().find(|i| i.source == "./types");
    assert!(imp.is_some());
    assert_eq!(
        imp.unwrap().imported_name,
        ImportedName::Named("Bar".to_string())
    );
}

#[test]
fn jsdoc_import_type_typedef_tag() {
    let info =
        parse_source("/**\n * @typedef {import('./lib').Config} Cfg\n */\nexport const v = 1;");
    let imp = info.imports.iter().find(|i| i.source == "./lib");
    assert!(imp.is_some());
    assert_eq!(
        imp.unwrap().imported_name,
        ImportedName::Named("Config".to_string())
    );
}

#[test]
fn jsdoc_multiple_import_types_in_one_comment() {
    let info = parse_source(
        "/**\n * @param a {import('./a').A}\n * @param b {import('./b').B}\n */\nfunction f(a, b) { return [a, b]; }",
    );
    let a = info.imports.iter().find(|i| i.source == "./a");
    let b = info.imports.iter().find(|i| i.source == "./b");
    assert!(a.is_some());
    assert!(b.is_some());
    assert_eq!(
        a.unwrap().imported_name,
        ImportedName::Named("A".to_string())
    );
    assert_eq!(
        b.unwrap().imported_name,
        ImportedName::Named("B".to_string())
    );
}

#[test]
fn jsdoc_import_types_union_in_one_annotation() {
    let info = parse_source(
        "/**\n * @param x {import('./a').A | import('./b').B}\n */\nfunction f(x) { return x; }",
    );
    assert!(info.imports.iter().any(|i| i.source == "./a"));
    assert!(info.imports.iter().any(|i| i.source == "./b"));
}

#[test]
fn jsdoc_import_type_bare_specifier() {
    let info = parse_source(
        "/**\n * @param c {import('@scope/pkg').Client}\n */\nfunction f(c) { return c; }",
    );
    let imp = info.imports.iter().find(|i| i.source == "@scope/pkg");
    assert!(imp.is_some());
    assert_eq!(
        imp.unwrap().imported_name,
        ImportedName::Named("Client".to_string())
    );
    assert!(imp.unwrap().is_type_only);
}

#[test]
fn jsdoc_import_type_relative_parent() {
    let info = parse_source("/**\n * @type {import('../lib/types.js').Foo}\n */\nlet y;");
    let imp = info.imports.iter().find(|i| i.source == "../lib/types.js");
    assert!(imp.is_some());
}

#[test]
fn jsdoc_import_type_nested_member_uses_first_segment() {
    let info = parse_source(
        "/**\n * @param x {import('./types').ns.Foo}\n */\nfunction f(x) { return x; }",
    );
    let imp = info.imports.iter().find(|i| i.source == "./types");
    assert!(imp.is_some());
    assert_eq!(
        imp.unwrap().imported_name,
        ImportedName::Named("ns".to_string())
    );
}

#[test]
fn jsdoc_import_without_member_recorded_as_side_effect() {
    let info =
        parse_source("/**\n * @param x {import('./types')}\n */\nfunction f(x) { return x; }");
    let imp = info.imports.iter().find(|i| i.source == "./types");
    assert!(imp.is_some());
    assert_eq!(imp.unwrap().imported_name, ImportedName::SideEffect);
    assert!(imp.unwrap().is_type_only);
}

#[test]
fn jsdoc_import_type_not_extracted_from_plain_comment() {
    let info =
        parse_source("/* @param foo {import('./types').Foo} */\nfunction bar() { return 1; }");
    assert!(info.imports.iter().all(|i| i.source != "./types"));
}

#[test]
fn jsdoc_import_type_coexists_with_public_tag() {
    let info = parse_source(
        "/**\n * @public\n * @param foo {import('./types').Foo}\n */\nexport function bar(foo) { return foo; }",
    );
    let imp = info.imports.iter().find(|i| i.source == "./types");
    assert!(imp.is_some());
    let exp = info
        .exports
        .iter()
        .find(|e| matches!(&e.name, ExportName::Named(n) if n == "bar"))
        .unwrap();
    assert_eq!(exp.visibility, VisibilityTag::Public);
}

#[test]
fn jsdoc_import_type_empty_path_ignored() {
    let info = parse_source("/**\n * @param x {import('').Foo}\n */\nfunction f(x) { return x; }");
    assert!(info.imports.is_empty());
}

#[test]
fn internal_tag_basic() {
    let info = parse_source("/** @internal */ export const foo = 1;");
    assert_eq!(info.exports[0].visibility, VisibilityTag::Internal);
}

#[test]
fn internal_tag_multiline() {
    let info = parse_source("/**\n * Some description.\n * @internal\n */\nexport const foo = 1;");
    assert_eq!(info.exports[0].visibility, VisibilityTag::Internal);
}

#[test]
fn internal_tag_not_internalizer() {
    let info = parse_source("/** @internalizer */ export const foo = 1;");
    assert_eq!(info.exports[0].visibility, VisibilityTag::None);
}

#[test]
fn internal_tag_on_function_export() {
    let info = parse_source("/** @internal */\nexport function bar() {}");
    assert_eq!(info.exports[0].visibility, VisibilityTag::Internal);
}

#[test]
fn internal_tag_on_default_export() {
    let info = parse_source("/** @internal */\nexport default function main() {}");
    assert_eq!(info.exports[0].visibility, VisibilityTag::Internal);
}

#[test]
fn beta_tag_basic() {
    let info = parse_source("/** @beta */ export const foo = 1;");
    assert_eq!(info.exports[0].visibility, VisibilityTag::Beta);
}

#[test]
fn beta_tag_multiline() {
    let info = parse_source("/**\n * Experimental API.\n * @beta\n */\nexport const foo = 1;");
    assert_eq!(info.exports[0].visibility, VisibilityTag::Beta);
}

#[test]
fn beta_tag_not_betaware() {
    let info = parse_source("/** @betaware */ export const foo = 1;");
    assert_eq!(info.exports[0].visibility, VisibilityTag::None);
}

#[test]
fn beta_tag_on_function_export() {
    let info = parse_source("/** @beta */\nexport function bar() {}");
    assert_eq!(info.exports[0].visibility, VisibilityTag::Beta);
}

#[test]
fn public_tag_still_works() {
    let info = parse_source("/** @public */ export const foo = 1;");
    assert_eq!(info.exports[0].visibility, VisibilityTag::Public);
}

#[test]
fn no_visibility_tag() {
    let info = parse_source("/** Some docs */ export const foo = 1;");
    assert_eq!(info.exports[0].visibility, VisibilityTag::None);
}

#[test]
fn expected_unused_tag_basic() {
    let info = parse_source("/** @expected-unused */ export const foo = 1;");
    assert_eq!(info.exports[0].visibility, VisibilityTag::ExpectedUnused);
    assert_eq!(info.exports[0].expected_unused_reason, None);
}

#[test]
fn expected_unused_tag_with_reason() {
    let info =
        parse_source("/** @expected-unused -- public package entry */ export const foo = 1;");
    assert_eq!(info.exports[0].visibility, VisibilityTag::ExpectedUnused);
    assert_eq!(
        info.exports[0].expected_unused_reason.as_deref(),
        Some("public package entry")
    );
}

#[test]
fn expected_unused_tag_empty_reason_is_missing() {
    let info = parse_source("/** @expected-unused -- */ export const foo = 1;");
    assert_eq!(info.exports[0].visibility, VisibilityTag::ExpectedUnused);
    assert_eq!(info.exports[0].expected_unused_reason, None);
}

#[test]
fn public_takes_priority_over_internal() {
    let info = parse_source("/** @public @internal */ export const foo = 1;");
    assert_eq!(info.exports[0].visibility, VisibilityTag::Public);
}

#[test]
fn internal_takes_priority_over_beta() {
    let info = parse_source("/** @internal @beta */ export const foo = 1;");
    assert_eq!(info.exports[0].visibility, VisibilityTag::Internal);
}

#[test]
fn alpha_tag_basic() {
    let info = parse_source("/** @alpha */ export const foo = 1;");
    assert_eq!(info.exports[0].visibility, VisibilityTag::Alpha);
}

#[test]
fn alpha_tag_not_alphabet() {
    let info = parse_source("/** @alphabet */ export const foo = 1;");
    assert_eq!(info.exports[0].visibility, VisibilityTag::None);
}

#[test]
fn alpha_tag_on_function_export() {
    let info = parse_source("/** @alpha */ export function foo() {}");
    assert_eq!(info.exports[0].visibility, VisibilityTag::Alpha);
}

#[test]
fn alpha_takes_priority_over_beta() {
    let info = parse_source("/** @alpha @beta */ export const foo = 1;");
    assert_eq!(info.exports[0].visibility, VisibilityTag::Alpha);
}

#[test]
fn internal_takes_priority_over_alpha() {
    let info = parse_source("/** @internal @alpha */ export const foo = 1;");
    assert_eq!(info.exports[0].visibility, VisibilityTag::Internal);
}

#[test]
fn public_takes_priority_over_alpha() {
    let info = parse_source("/** @public @alpha */ export const foo = 1;");
    assert_eq!(info.exports[0].visibility, VisibilityTag::Public);
}

fn deprecation_of(source: &str, name: &str) -> (bool, Option<String>) {
    let info = parse_source(source);
    let export = info
        .exports
        .iter()
        .find(|e| matches!(&e.name, ExportName::Named(n) if n == name))
        .unwrap_or_else(|| panic!("export {name} not found"));
    (
        export.deprecated,
        export.deprecated_reason.as_deref().map(str::to_string),
    )
}

#[test]
fn deprecated_tag_marks_export_with_message() {
    assert_eq!(
        deprecation_of(
            "/**\n * Old.\n * @deprecated Use {@link b} instead.\n * @see b\n */\nexport function a() {}\nexport function b() {}",
            "a"
        ),
        (true, Some("Use b instead.".to_string()))
    );
    assert_eq!(
        deprecation_of("/** @deprecated */\nexport const c = 1;", "c"),
        (true, None)
    );
}

#[test]
fn deprecated_tag_is_orthogonal_to_visibility() {
    let info = parse_source("/** @public @deprecated use y */\nexport const x = 1;");
    assert_eq!(info.exports[0].visibility, VisibilityTag::Public);
    assert!(info.exports[0].deprecated);
    assert_eq!(info.exports[0].deprecated_reason.as_deref(), Some("use y"));
}

#[test]
fn deprecated_tag_guards() {
    // identifier-boundary check
    assert_eq!(
        deprecation_of("/** @deprecatedFoo */\nexport const a = 1;", "a"),
        (false, None)
    );
    // the tag text inside a string is not a comment
    assert_eq!(
        deprecation_of("export const a = '/** @deprecated */';", "a"),
        (false, None)
    );
    // a tag on the previous statement does not leak to the next export
    assert_eq!(
        deprecation_of(
            "/** @deprecated */\nconst old = 1;\nexport const a = old;",
            "a"
        ),
        (false, None)
    );
    // a tag after `{` inside an export list belongs to no declaration
    assert_eq!(
        deprecation_of(
            "const a = 1;\nexport {\n  /** @deprecated */\n  a,\n};",
            "a"
        ),
        (false, None)
    );
    // a plain block comment is not JSDoc
    assert_eq!(
        deprecation_of("/* @deprecated */\nexport const a = 1;", "a"),
        (false, None)
    );
}

#[test]
fn deprecated_tag_on_declared_then_exported_statement_prefix() {
    assert_eq!(
        deprecation_of(
            "/** @deprecated gone soon */\nexport async function a() {}",
            "a"
        ),
        (true, Some("gone soon".to_string()))
    );
    assert_eq!(
        deprecation_of("/** @deprecated */\nexport type T = string;", "T"),
        (true, None)
    );
}

/// Files without semicolons (Prettier `semi: false`, StandardJS): a tag on
/// one export statement must not reach the next export statements.
#[test]
fn deprecated_tag_stays_on_its_statement_without_semicolons() {
    let source = "/** @deprecated use c */\nexport const a = 1\n\n// note\nexport const b = 2\nexport type T = string\nexport declare function f(): void\nexport const c = 3\n";
    assert_eq!(
        deprecation_of(source, "a"),
        (true, Some("use c".to_string()))
    );
    for name in ["b", "T", "f", "c"] {
        assert_eq!(deprecation_of(source, name), (false, None), "{name}");
    }
}

#[test]
fn visibility_tag_stays_on_its_statement_without_semicolons() {
    let info = parse_source(
        "/** @internal */\nexport const a = 1\nexport const b = 2\nexport function c() {}\n",
    );
    let visibility = |name: &str| {
        info.exports
            .iter()
            .find(|e| matches!(&e.name, ExportName::Named(n) if n == name))
            .map(|e| e.visibility)
            .expect("export")
    };
    assert_eq!(visibility("a"), VisibilityTag::Internal);
    assert_eq!(visibility("b"), VisibilityTag::None);
    assert_eq!(visibility("c"), VisibilityTag::None);
}

#[test]
fn tag_on_an_export_list_covers_every_specifier_of_that_statement() {
    let info = parse_source(
        "const a = 1\nconst b = 2\n/** @public */\nexport { a, b }\nexport const c = 3\n",
    );
    for export in info.exports.iter() {
        let expected = if matches!(&export.name, ExportName::Named(n) if n == "c") {
            VisibilityTag::None
        } else {
            VisibilityTag::Public
        };
        assert_eq!(export.visibility, expected, "{:?}", export.name);
    }
}

/// Two tagged JSDoc blocks before one export: the last block wins, on every
/// run. Many exports make an unstable sort and a binary search on duplicate
/// keys show the nondeterminism.
#[test]
fn last_of_two_tagged_blocks_wins_for_every_export() {
    let mut source = String::new();
    for i in 0..40 {
        let _ = write!(
            source,
            "/** @deprecated first {i} */\n/** @deprecated second {i} */\nexport const d{i} = {i};\n/** @public */\n/** @internal */\nexport const v{i} = {i};\n"
        );
    }
    for _ in 0..5 {
        let info = parse_source(&source);
        for i in 0..40 {
            assert_eq!(
                deprecation_of(&source, &format!("d{i}")),
                (true, Some(format!("second {i}")))
            );
            let visibility = info
                .exports
                .iter()
                .find(|e| matches!(&e.name, ExportName::Named(n) if *n == format!("v{i}")))
                .map(|e| e.visibility)
                .expect("export");
            assert_eq!(visibility, VisibilityTag::Internal, "v{i}");
        }
    }
}

fn visibility_of(source: &str, name: &str) -> VisibilityTag {
    parse_source(source)
        .exports
        .iter()
        .find(|e| matches!(&e.name, ExportName::Named(n) if n == name))
        .map_or_else(|| panic!("export {name} not found"), |e| e.visibility)
}

#[test]
fn jsdoc_before_a_decorator_with_arguments_attaches_to_the_export() {
    let source = "/** @public */\n@Component({ selector: 'x' })\nexport class Widget {}\n";
    assert_eq!(visibility_of(source, "Widget"), VisibilityTag::Public);
    let source = "/** @internal */\n@Injectable()\nexport class Service {}\n";
    assert_eq!(visibility_of(source, "Service"), VisibilityTag::Internal);
    let source =
        "/** @deprecated use Other */\n@Component({ selector: 'x' })\nexport class Old {}\n";
    assert_eq!(
        deprecation_of(source, "Old"),
        (true, Some("use Other".to_string()))
    );
}

#[test]
fn jsdoc_before_stacked_decorators_attaches_to_the_export() {
    for sep in [";", ""] {
        let source = format!(
            "import {{ A, B }} from './d'{sep}\nconst x = 1{sep}\n/** @public */\n@A()\n@B({{ x }})\n@C\nexport class Widget {{}}\n/** @deprecated gone */\n@A()\n@B()\nexport default class Main {{}}\n"
        );
        assert_eq!(
            visibility_of(&source, "Widget"),
            VisibilityTag::Public,
            "sep {sep:?}"
        );
        let info = parse_source(&source);
        let main = info
            .exports
            .iter()
            .find(|e| matches!(e.name, ExportName::Default))
            .expect("default export");
        assert!(main.deprecated, "sep {sep:?}");
    }
}

#[test]
fn jsdoc_before_an_earlier_statement_does_not_reach_a_decorated_export() {
    for sep in [";", ""] {
        let source = format!(
            "/** @public */\nconst x = 1{sep}\n@Component({{ selector: 'x' }})\nexport class Widget {{}}\n/** @deprecated old */\nfoo(){sep}\n@A()\nexport class Other {{}}\n"
        );
        assert_eq!(
            visibility_of(&source, "Widget"),
            VisibilityTag::None,
            "sep {sep:?}"
        );
        assert_eq!(
            deprecation_of(&source, "Other"),
            (false, None),
            "sep {sep:?}"
        );
    }
}

/// TypeScript gives no tags to a JSDoc block between a decorator and
/// `export`, so such a block does not attach.
#[test]
fn jsdoc_between_a_decorator_and_export_does_not_attach() {
    for source in [
        "@A()\n/** @public */\nexport class X {}\n",
        "@A()\n/** @public */\n@B()\nexport class X {}\n",
    ] {
        assert_eq!(visibility_of(source, "X"), VisibilityTag::None, "{source}");
    }
    let source = "@A()\n/** @deprecated old */\nexport class Y {}\n";
    assert_eq!(deprecation_of(source, "Y"), (false, None));
}

#[test]
fn jsdoc_on_a_decorated_export_does_not_reach_the_next_export() {
    let source = "/** @public */\n@A()\nexport class First {}\n@B()\nexport class Second {}\n";
    assert_eq!(visibility_of(source, "First"), VisibilityTag::Public);
    assert_eq!(visibility_of(source, "Second"), VisibilityTag::None);
}
