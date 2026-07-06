use super::*;

#[test]
fn strip_types_removes_parameter_type_annotations() {
    let ts_tokens = tokenize("function foo(x: string) { return x; }");
    let stripped = tokenize_cross_language("function foo(x: string) { return x; }");

    assert!(
        stripped.len() < ts_tokens.len(),
        "Stripped tokens ({}) should be fewer than full tokens ({})",
        stripped.len(),
        ts_tokens.len()
    );

    let has_colon_before_string = ts_tokens.windows(2).any(|w| {
        matches!(w[0].kind, TokenKind::Punctuation(PunctuationType::Colon))
            && matches!(&w[1].kind, TokenKind::Identifier(n) if n == "string")
    });
    assert!(has_colon_before_string, "Original should have `: string`");

    let js_tokens = {
        let path = PathBuf::from("test.js");
        tokenize_file(&path, "function foo(x) { return x; }", false).tokens
    };
    assert_eq!(
        stripped.len(),
        js_tokens.len(),
        "Stripped TS should produce same token count as JS"
    );
}

#[test]
fn strip_types_removes_return_type_annotations() {
    let stripped = tokenize_cross_language("function foo(): string { return 'hello'; }");
    let has_string_type = stripped.iter().enumerate().any(|(i, t)| {
        matches!(&t.kind, TokenKind::Identifier(n) if n == "string")
            && i > 0
            && matches!(
                stripped[i - 1].kind,
                TokenKind::Punctuation(PunctuationType::Colon)
            )
    });
    assert!(
        !has_string_type,
        "Stripped version should not have return type annotation"
    );
}

#[test]
fn strip_types_removes_interface_declarations() {
    let stripped = tokenize_cross_language("interface Foo { bar: string; }\nconst x = 42;");
    let has_interface = stripped
        .iter()
        .any(|t| matches!(t.kind, TokenKind::Keyword(KeywordType::Interface)));
    assert!(
        !has_interface,
        "Stripped version should not contain interface declaration"
    );
    let has_const = stripped
        .iter()
        .any(|t| matches!(t.kind, TokenKind::Keyword(KeywordType::Const)));
    assert!(has_const, "Should still contain const keyword");
}

#[test]
fn strip_types_removes_type_alias_declarations() {
    let stripped = tokenize_cross_language("type Result = string | number;\nconst x = 42;");
    let has_type = stripped
        .iter()
        .any(|t| matches!(t.kind, TokenKind::Keyword(KeywordType::Type)));
    assert!(!has_type, "Stripped version should not contain type alias");
    let has_const = stripped
        .iter()
        .any(|t| matches!(t.kind, TokenKind::Keyword(KeywordType::Const)));
    assert!(has_const, "Should still contain const keyword");
}

#[test]
fn strip_types_preserves_runtime_code() {
    let stripped = tokenize_cross_language("const x: number = 42;\nif (x > 0) { console.log(x); }");
    let has_const = stripped
        .iter()
        .any(|t| matches!(t.kind, TokenKind::Keyword(KeywordType::Const)));
    let has_if = stripped
        .iter()
        .any(|t| matches!(t.kind, TokenKind::Keyword(KeywordType::If)));
    let has_42 = stripped
        .iter()
        .any(|t| matches!(&t.kind, TokenKind::NumericLiteral(n) if n == "42"));
    assert!(has_const, "Should preserve const");
    assert!(has_if, "Should preserve if");
    assert!(has_42, "Should preserve numeric literal");
}

#[test]
fn strip_types_preserves_enums() {
    let stripped = tokenize_cross_language("enum Color { Red, Green, Blue }");
    let has_enum = stripped
        .iter()
        .any(|t| matches!(t.kind, TokenKind::Keyword(KeywordType::Enum)));
    assert!(
        has_enum,
        "Enums should be preserved (they have runtime semantics)"
    );
}

#[test]
fn strip_types_removes_import_type() {
    let stripped = tokenize_cross_language("import type { Foo } from './foo';\nconst x = 42;");
    let import_count = stripped
        .iter()
        .filter(|t| matches!(t.kind, TokenKind::Keyword(KeywordType::Import)))
        .count();
    assert_eq!(import_count, 0, "import type should be stripped");
    let has_const = stripped
        .iter()
        .any(|t| matches!(t.kind, TokenKind::Keyword(KeywordType::Const)));
    assert!(has_const, "Runtime code should be preserved");
}

#[test]
fn strip_types_preserves_value_imports() {
    let stripped = tokenize_cross_language("import { foo } from './foo';\nconst x = foo();");
    let has_import = stripped
        .iter()
        .any(|t| matches!(t.kind, TokenKind::Keyword(KeywordType::Import)));
    assert!(has_import, "Value imports should be preserved");
}

#[test]
fn strip_types_removes_export_type() {
    let stripped = tokenize_cross_language("export type { Foo };\nconst x = 42;");
    let export_count = stripped
        .iter()
        .filter(|t| matches!(t.kind, TokenKind::Keyword(KeywordType::Export)))
        .count();
    assert_eq!(export_count, 0, "export type should be stripped");
}

#[test]
fn strip_types_removes_declare_module() {
    let stripped = tokenize_cross_language(
        "declare module 'foo' { export function bar(): void; }\nconst x = 42;",
    );
    let has_function_keyword = stripped
        .iter()
        .any(|t| matches!(t.kind, TokenKind::Keyword(KeywordType::Function)));
    assert!(
        !has_function_keyword,
        "declare module contents should be stripped"
    );
    let has_const = stripped
        .iter()
        .any(|t| matches!(t.kind, TokenKind::Keyword(KeywordType::Const)));
    assert!(has_const, "Runtime code should be preserved");
}

#[test]
fn strip_types_removes_generic_type_parameters() {
    let stripped = tokenize_cross_language("function identity<T>(x: T): T { return x; }");
    let js_tokens = {
        let path = PathBuf::from("test.js");
        tokenize_file(&path, "function identity(x) { return x; }", false).tokens
    };
    assert_eq!(
        stripped.len(),
        js_tokens.len(),
        "Stripped TS with generics should match JS token count: stripped={}, js={}",
        stripped.len(),
        js_tokens.len()
    );
}

#[test]
fn strip_types_removes_generic_type_arguments() {
    let stripped = tokenize_cross_language("const x = new Map<string, number>();");
    let has_string_ident = stripped
        .iter()
        .any(|t| matches!(&t.kind, TokenKind::Identifier(n) if n == "string"));
    let has_map = stripped
        .iter()
        .any(|t| matches!(&t.kind, TokenKind::Identifier(n) if n == "Map"));
    assert!(has_map, "Map identifier should be preserved");
    assert!(
        !has_string_ident,
        "Type argument 'string' should be stripped"
    );
}

#[test]
fn strip_types_removes_as_expression() {
    let stripped = tokenize_cross_language("const x = value as string;");
    let has_as = stripped
        .iter()
        .any(|t| matches!(t.kind, TokenKind::Keyword(KeywordType::As)));
    assert!(!has_as, "'as' expression should be stripped");
}

#[test]
fn strip_types_removes_satisfies_expression() {
    let stripped = tokenize_cross_language("const config = {} satisfies Config;");
    let has_satisfies = stripped
        .iter()
        .any(|t| matches!(t.kind, TokenKind::Keyword(KeywordType::Satisfies)));
    assert!(!has_satisfies, "'satisfies' expression should be stripped");
}

#[test]
fn strip_types_ts_and_js_produce_identical_token_kinds() {
    let ts_code = r#"
function greet(name: string, age: number): string {
const msg: string = `Hello ${name}`;
if (age > 18) {
    return msg;
}
return "too young";
}
"#;
    let js_code = r#"
function greet(name, age) {
const msg = `Hello ${name}`;
if (age > 18) {
    return msg;
}
return "too young";
}
"#;
    let stripped = tokenize_cross_language(ts_code);
    let js_tokens = {
        let path = PathBuf::from("test.js");
        tokenize_file(&path, js_code, false).tokens
    };

    assert_eq!(
        stripped.len(),
        js_tokens.len(),
        "Stripped TS and JS should produce same number of tokens"
    );

    for (i, (ts_tok, js_tok)) in stripped.iter().zip(js_tokens.iter()).enumerate() {
        assert_eq!(
            ts_tok.kind, js_tok.kind,
            "Token {i} mismatch: TS={:?}, JS={:?}",
            ts_tok.kind, js_tok.kind
        );
    }
}

#[test]
fn strip_types_removes_export_type_but_keeps_export_value() {
    let stripped =
        tokenize_cross_language("export type { Foo };\nexport { bar };\nexport const x = 1;");
    let export_count = stripped
        .iter()
        .filter(|t| matches!(t.kind, TokenKind::Keyword(KeywordType::Export)))
        .count();
    assert_eq!(
        export_count, 2,
        "Should have 2 value exports, got {export_count}"
    );
}

#[test]
fn strip_types_removes_complex_generics() {
    let stripped = tokenize_cross_language(
        "function merge<T extends object, U extends object>(a: T, b: U): T & U { return Object.assign(a, b); }",
    );
    let js_tokens = {
        let path = PathBuf::from("test.js");
        tokenize_file(
            &path,
            "function merge(a, b) { return Object.assign(a, b); }",
            false,
        )
        .tokens
    };
    assert_eq!(
        stripped.len(),
        js_tokens.len(),
        "Complex generics should be fully stripped: stripped={}, js={}",
        stripped.len(),
        js_tokens.len()
    );
}

#[test]
fn strip_types_removes_conditional_type() {
    let stripped = tokenize_cross_language(
        "type IsString<T> = T extends string ? true : false;\nconst x = 1;",
    );
    let has_type = stripped
        .iter()
        .any(|t| matches!(t.kind, TokenKind::Keyword(KeywordType::Type)));
    assert!(!has_type, "Conditional type alias should be fully stripped");
}

#[test]
fn tokenize_vue_sfc_with_cross_language_stripping() {
    let vue_source = r#"<template><div/></template>
<script lang="ts">
import type { Ref } from 'vue';
import { ref } from 'vue';
const count: Ref<number> = ref(0);
</script>"#;
    let path = PathBuf::from("Component.vue");
    let result = tokenize_file_cross_language(&path, vue_source, true, false);
    let import_count = result
        .tokens
        .iter()
        .filter(|t| matches!(t.kind, TokenKind::Keyword(KeywordType::Import)))
        .count();
    assert_eq!(
        import_count, 1,
        "import type should be stripped, leaving only 1 value import, got {import_count}"
    );
}

#[test]
fn tokenize_cross_language_produces_correct_metadata() {
    let path = PathBuf::from("test.ts");
    let source = "const x: number = 1;\nconst y: string = 'hello';";
    let result = tokenize_file_cross_language(&path, source, true, false);
    assert_eq!(result.line_count, 2);
    assert_eq!(result.source, source);
    assert!(!result.tokens.is_empty());
}

#[test]
fn cross_language_preserves_non_declare_namespace() {
    let stripped = tokenize_cross_language("namespace Foo { export const x = 1; }");
    let has_const = stripped
        .iter()
        .any(|t| matches!(t.kind, TokenKind::Keyword(KeywordType::Const)));
    assert!(
        has_const,
        "Non-declare namespace contents should be preserved in cross-language mode"
    );
}

#[test]
fn strip_types_removes_ts_type_annotation_colon() {
    let stripped = tokenize_cross_language("const x: number = 1;");
    let colon_count = stripped
        .iter()
        .filter(|t| matches!(t.kind, TokenKind::Punctuation(PunctuationType::Colon)))
        .count();
    assert_eq!(
        colon_count, 0,
        "Type annotation colons should be stripped, got {colon_count}"
    );
}

#[test]
fn strip_types_removes_as_const() {
    let stripped = tokenize_cross_language("const colors = ['red', 'green', 'blue'] as const;");
    let has_as = stripped
        .iter()
        .any(|t| matches!(t.kind, TokenKind::Keyword(KeywordType::As)));
    assert!(
        !has_as,
        "'as const' should be stripped in cross-language mode"
    );
}

#[test]
fn strip_types_non_null_assertion_matches_js() {
    let stripped = tokenize_cross_language("const x = value!;");
    let js_tokens = {
        let path = PathBuf::from("test.js");
        tokenize_file(&path, "const x = value;", false).tokens
    };
    assert_eq!(
        stripped.len(),
        js_tokens.len(),
        "TS non-null assertion stripped should match JS token count: stripped={}, js={}",
        stripped.len(),
        js_tokens.len()
    );
}

#[test]
fn strip_types_class_with_generics() {
    let stripped =
        tokenize_cross_language("class Container<T> { value: T; constructor(v: T) { } }");
    let has_class = stripped
        .iter()
        .any(|t| matches!(t.kind, TokenKind::Keyword(KeywordType::Class)));
    assert!(has_class, "Should still have class keyword");
    let colon_count = stripped
        .iter()
        .filter(|t| matches!(t.kind, TokenKind::Punctuation(PunctuationType::Colon)))
        .count();
    assert_eq!(
        colon_count, 0,
        "Type annotation colons should be stripped, got {colon_count}"
    );
}

#[test]
fn strip_types_arrow_function_matches_js() {
    let stripped = tokenize_cross_language("const add = (a: number, b: number): number => a + b;");
    let js_tokens = {
        let path = PathBuf::from("test.js");
        tokenize_file(&path, "const add = (a, b) => a + b;", false).tokens
    };
    assert_eq!(
        stripped.len(),
        js_tokens.len(),
        "Stripped arrow function should match JS: stripped={}, js={}",
        stripped.len(),
        js_tokens.len()
    );
    for (i, (ts_tok, js_tok)) in stripped.iter().zip(js_tokens.iter()).enumerate() {
        assert_eq!(
            ts_tok.kind, js_tok.kind,
            "Token {i} mismatch in arrow function: TS={:?}, JS={:?}",
            ts_tok.kind, js_tok.kind
        );
    }
}

#[test]
fn strip_types_mixed_import_keeps_only_value_import() {
    let stripped = tokenize_cross_language(
        "import type { Type } from './mod';\nimport { value } from './mod';",
    );
    let import_count = stripped
        .iter()
        .filter(|t| matches!(t.kind, TokenKind::Keyword(KeywordType::Import)))
        .count();
    assert_eq!(
        import_count, 1,
        "Only value import should remain, got {import_count}"
    );
}
