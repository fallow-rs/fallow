use super::*;

#[test]
fn tokenize_ts_as_expression() {
    let tokens = tokenize("const x = value as string;");
    let has_as = tokens
        .iter()
        .any(|t| matches!(t.kind, TokenKind::Keyword(KeywordType::As)));
    assert!(has_as, "Should contain 'as' keyword");
}

#[test]
fn tokenize_ts_satisfies_expression() {
    let tokens = tokenize("const config = {} satisfies Config;");
    let has_satisfies = tokens
        .iter()
        .any(|t| matches!(t.kind, TokenKind::Keyword(KeywordType::Satisfies)));
    assert!(has_satisfies, "Should contain 'satisfies' keyword");
}

#[test]
fn tokenize_ts_non_null_assertion() {
    let ts_tokens = tokenize("const x = value!.toString();");
    let has_value = ts_tokens
        .iter()
        .any(|t| matches!(&t.kind, TokenKind::Identifier(n) if n == "value"));
    assert!(has_value, "Should contain 'value' identifier");
}

#[test]
fn tokenize_ts_generic_type_parameters() {
    let tokens = tokenize("function identity<T>(x: T): T { return x; }");
    let t_count = tokens
        .iter()
        .filter(|t| matches!(&t.kind, TokenKind::Identifier(n) if n == "T"))
        .count();
    assert!(
        t_count >= 1,
        "Generic type parameter T should appear in tokens"
    );
}

#[test]
fn tokenize_ts_type_keywords() {
    let tokens = tokenize(
        "type T = string | number | boolean | any | void | null | undefined | never | unknown;",
    );
    let idents: Vec<&String> = tokens
        .iter()
        .filter_map(|t| match &t.kind {
            TokenKind::Identifier(name) => Some(name),
            _ => None,
        })
        .collect();
    assert!(idents.contains(&&"string".to_string()));
    assert!(idents.contains(&&"number".to_string()));
    assert!(idents.contains(&&"boolean".to_string()));
    assert!(idents.contains(&&"any".to_string()));
    assert!(idents.contains(&&"void".to_string()));
    assert!(idents.contains(&&"undefined".to_string()));
    assert!(idents.contains(&&"never".to_string()));
    assert!(idents.contains(&&"unknown".to_string()));
    let has_null = tokens
        .iter()
        .any(|t| matches!(t.kind, TokenKind::NullLiteral));
    assert!(has_null, "null keyword should produce NullLiteral token");
}

#[test]
fn tokenize_ts_property_signatures_in_interface() {
    let tokens = tokenize("interface Foo { bar: string; baz: number; }");
    let semicolons = tokens
        .iter()
        .filter(|t| matches!(t.kind, TokenKind::Punctuation(PunctuationType::Semicolon)))
        .count();
    assert!(
        semicolons >= 2,
        "Interface property signatures should produce semicolons, got {semicolons}"
    );
}

#[test]
fn tokenize_ts_enum_with_initializers() {
    let tokens = tokenize("enum Status { Active = 'ACTIVE', Inactive = 'INACTIVE' }");
    let has_enum = tokens
        .iter()
        .any(|t| matches!(t.kind, TokenKind::Keyword(KeywordType::Enum)));
    assert!(has_enum);
    let has_active_str = tokens
        .iter()
        .any(|t| matches!(&t.kind, TokenKind::StringLiteral(s) if s == "ACTIVE"));
    assert!(has_active_str, "Should contain string initializer 'ACTIVE'");
}

#[test]
fn tokenize_ts_as_const() {
    let tokens = tokenize("const colors = ['red', 'green', 'blue'] as const;");
    let has_as = tokens
        .iter()
        .any(|t| matches!(t.kind, TokenKind::Keyword(KeywordType::As)));
    assert!(has_as, "as const should produce 'as' keyword");
    let has_const_decl = tokens
        .iter()
        .any(|t| matches!(t.kind, TokenKind::Keyword(KeywordType::Const)));
    assert!(
        has_const_decl,
        "Should have Const keyword for the declaration"
    );
}

#[test]
fn tokenize_ts_conditional_type_without_strip() {
    let tokens = tokenize("type IsString<T> = T extends string ? true : false;");
    let has_type = tokens
        .iter()
        .any(|t| matches!(t.kind, TokenKind::Keyword(KeywordType::Type)));
    assert!(has_type, "Should contain type keyword");
    let has_true_bool = tokens
        .iter()
        .any(|t| matches!(t.kind, TokenKind::BooleanLiteral(true)));
    let has_false_bool = tokens
        .iter()
        .any(|t| matches!(t.kind, TokenKind::BooleanLiteral(false)));
    assert!(
        has_true_bool,
        "Conditional type should contain true literal"
    );
    assert!(
        has_false_bool,
        "Conditional type should contain false literal"
    );
}

#[test]
fn tokenize_ts_module_declaration_not_stripped_when_not_declare() {
    let tokens = tokenize("namespace Foo { export const x = 1; }");
    let has_const = tokens
        .iter()
        .any(|t| matches!(t.kind, TokenKind::Keyword(KeywordType::Const)));
    assert!(
        has_const,
        "Non-declare namespace contents should be preserved"
    );
}

#[test]
fn tokenize_ts_interface_body_has_braces() {
    let tokens = tokenize("interface I { x: number; }");
    let open_braces = tokens
        .iter()
        .filter(|t| matches!(t.kind, TokenKind::Punctuation(PunctuationType::OpenBrace)))
        .count();
    let close_braces = tokens
        .iter()
        .filter(|t| matches!(t.kind, TokenKind::Punctuation(PunctuationType::CloseBrace)))
        .count();
    assert!(open_braces >= 1, "Interface body should have open brace");
    assert_eq!(
        open_braces, close_braces,
        "Interface body braces should be balanced"
    );
}

#[test]
fn tokenize_ts_enum_body_has_braces() {
    let tokens = tokenize("enum E { A, B }");
    let open_braces = tokens
        .iter()
        .filter(|t| matches!(t.kind, TokenKind::Punctuation(PunctuationType::OpenBrace)))
        .count();
    assert!(open_braces >= 1, "Enum body should have open brace");
}
