use super::*;

#[test]
fn tokenize_jsx_fragment() {
    let tokens = tokenize_tsx("const x = <><div>Hello</div></>;");
    let bracket_count = tokens
        .iter()
        .filter(|t| {
            matches!(
                t.kind,
                TokenKind::Punctuation(
                    PunctuationType::OpenBracket | PunctuationType::CloseBracket,
                )
            )
        })
        .count();
    assert!(
        bracket_count >= 4,
        "JSX fragment should produce bracket tokens, got {bracket_count}"
    );
}

#[test]
fn tokenize_jsx_spread_attribute() {
    let tokens = tokenize_tsx("const x = <div {...props}>Hello</div>;");
    let has_spread = tokens
        .iter()
        .any(|t| matches!(t.kind, TokenKind::Operator(OperatorType::Spread)));
    assert!(
        has_spread,
        "JSX spread attribute should produce spread operator"
    );
}

#[test]
fn tokenize_jsx_expression_container() {
    let tokens = tokenize_tsx("const x = <div>{count > 0 ? 'yes' : 'no'}</div>;");
    let has_ternary = tokens
        .iter()
        .any(|t| matches!(t.kind, TokenKind::Operator(OperatorType::Ternary)));
    assert!(
        has_ternary,
        "Expression in JSX should be tokenized (ternary)"
    );
}

#[test]
fn tokenize_jsx_self_closing_element() {
    let tokens = tokenize_tsx("const x = <Input type=\"text\" />;");
    let has_input = tokens
        .iter()
        .any(|t| matches!(&t.kind, TokenKind::Identifier(n) if n == "Input"));
    let has_type = tokens
        .iter()
        .any(|t| matches!(&t.kind, TokenKind::Identifier(n) if n == "type"));
    assert!(has_input, "Should contain JSX element name 'Input'");
    assert!(has_type, "Should contain JSX attribute name 'type'");
}
