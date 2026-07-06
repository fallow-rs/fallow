use super::*;

#[test]
fn tokenize_this_expression() {
    let tokens = tokenize("const x = this.foo;");
    let has_this = tokens
        .iter()
        .any(|t| matches!(t.kind, TokenKind::Keyword(KeywordType::This)));
    assert!(has_this, "Should contain this keyword");
}

#[test]
fn tokenize_super_expression() {
    let tokens = tokenize("class Child extends Parent { constructor() { super(); } }");
    let has_super = tokens
        .iter()
        .any(|t| matches!(t.kind, TokenKind::Keyword(KeywordType::Super)));
    assert!(has_super, "Should contain super keyword");
}

#[test]
fn tokenize_array_expression() {
    let tokens = tokenize("const arr = [1, 2, 3];");
    let open_bracket = tokens
        .iter()
        .any(|t| matches!(t.kind, TokenKind::Punctuation(PunctuationType::OpenBracket)));
    let close_bracket = tokens.iter().any(|t| {
        matches!(
            t.kind,
            TokenKind::Punctuation(PunctuationType::CloseBracket)
        )
    });
    assert!(open_bracket, "Should contain open bracket");
    assert!(close_bracket, "Should contain close bracket");
}

#[test]
fn tokenize_object_expression() {
    let tokens = tokenize("const obj = { a: 1, b: 2 };");
    let open_brace = tokens
        .iter()
        .filter(|t| matches!(t.kind, TokenKind::Punctuation(PunctuationType::OpenBrace)))
        .count();
    let close_brace = tokens
        .iter()
        .filter(|t| matches!(t.kind, TokenKind::Punctuation(PunctuationType::CloseBrace)))
        .count();
    assert!(open_brace >= 1, "Should have open brace for object");
    assert!(close_brace >= 1, "Should have close brace for object");
}

#[test]
fn tokenize_computed_member_expression() {
    let tokens = tokenize("const x = obj[key];");
    let open_bracket = tokens
        .iter()
        .any(|t| matches!(t.kind, TokenKind::Punctuation(PunctuationType::OpenBracket)));
    let close_bracket = tokens.iter().any(|t| {
        matches!(
            t.kind,
            TokenKind::Punctuation(PunctuationType::CloseBracket)
        )
    });
    assert!(
        open_bracket,
        "Should contain open bracket for computed member"
    );
    assert!(
        close_bracket,
        "Should contain close bracket for computed member"
    );
}

#[test]
fn tokenize_static_member_expression() {
    let tokens = tokenize("const x = obj.prop;");
    let has_dot = tokens
        .iter()
        .any(|t| matches!(t.kind, TokenKind::Punctuation(PunctuationType::Dot)));
    let has_prop = tokens
        .iter()
        .any(|t| matches!(&t.kind, TokenKind::Identifier(n) if n == "prop"));
    assert!(has_dot, "Should contain dot for member access");
    assert!(has_prop, "Should contain property name 'prop'");
}

#[test]
fn tokenize_new_expression() {
    let tokens = tokenize("const d = new Date(2024, 1, 1);");
    let has_new = tokens
        .iter()
        .any(|t| matches!(t.kind, TokenKind::Keyword(KeywordType::New)));
    assert!(has_new, "Should contain new keyword");
    let has_date = tokens
        .iter()
        .any(|t| matches!(&t.kind, TokenKind::Identifier(n) if n == "Date"));
    assert!(has_date, "Should contain identifier 'Date'");
}

#[test]
fn tokenize_template_literal() {
    let tokens = tokenize("const s = `hello ${name}`;");
    let has_template = tokens
        .iter()
        .any(|t| matches!(t.kind, TokenKind::TemplateLiteral));
    assert!(has_template, "Should contain template literal token");
}

#[test]
fn tokenize_regex_literal() {
    let tokens = tokenize("const re = /foo[a-z]+/gi;");
    let has_regex = tokens
        .iter()
        .any(|t| matches!(t.kind, TokenKind::RegExpLiteral));
    assert!(has_regex, "Should contain regex literal token");
}

#[test]
fn tokenize_conditional_ternary_expression() {
    let tokens = tokenize("const x = a ? b : c;");
    let has_ternary = tokens
        .iter()
        .any(|t| matches!(t.kind, TokenKind::Operator(OperatorType::Ternary)));
    let has_colon = tokens
        .iter()
        .any(|t| matches!(t.kind, TokenKind::Punctuation(PunctuationType::Colon)));
    assert!(has_ternary, "Should contain ternary operator");
    assert!(has_colon, "Should contain colon for ternary");
}

#[test]
fn tokenize_sequence_expression() {
    let tokens = tokenize("for (let i = 0, j = 10; i < j; i++, j--) {}");
    let comma_count = tokens
        .iter()
        .filter(|t| matches!(t.kind, TokenKind::Operator(OperatorType::Comma)))
        .count();
    assert!(
        comma_count >= 1,
        "Sequence expression should produce comma operators"
    );
}

#[test]
fn tokenize_spread_element() {
    let tokens = tokenize("const arr = [...other, 1, 2];");
    let has_spread = tokens
        .iter()
        .any(|t| matches!(t.kind, TokenKind::Operator(OperatorType::Spread)));
    assert!(has_spread, "Should contain spread operator");
}

#[test]
fn tokenize_yield_expression() {
    let tokens = tokenize("function* gen() { yield 42; }");
    let has_yield = tokens
        .iter()
        .any(|t| matches!(t.kind, TokenKind::Keyword(KeywordType::Yield)));
    assert!(has_yield, "Should contain yield keyword");
}

#[test]
fn tokenize_await_expression() {
    let tokens = tokenize("async function run() { const x = await fetch(); }");
    let has_async = tokens
        .iter()
        .any(|t| matches!(t.kind, TokenKind::Keyword(KeywordType::Async)));
    let has_await = tokens
        .iter()
        .any(|t| matches!(t.kind, TokenKind::Keyword(KeywordType::Await)));
    assert!(has_async, "Should contain async keyword");
    assert!(has_await, "Should contain await keyword");
}

#[test]
fn tokenize_async_arrow_function() {
    let tokens = tokenize("const f = async () => { await fetch(); };");
    let has_async = tokens
        .iter()
        .any(|t| matches!(t.kind, TokenKind::Keyword(KeywordType::Async)));
    let has_arrow = tokens
        .iter()
        .any(|t| matches!(t.kind, TokenKind::Operator(OperatorType::Arrow)));
    assert!(has_async, "Should contain async keyword before arrow");
    assert!(has_arrow, "Should contain arrow operator");
}

#[test]
fn tokenize_tagged_template_literal() {
    let tokens = tokenize("const x = html`<div>${content}</div>`;");
    let has_template = tokens
        .iter()
        .any(|t| matches!(t.kind, TokenKind::TemplateLiteral));
    let has_html = tokens
        .iter()
        .any(|t| matches!(&t.kind, TokenKind::Identifier(n) if n == "html"));
    assert!(has_template, "Should contain template literal token");
    assert!(has_html, "Should contain tag identifier 'html'");
}

#[test]
fn tokenize_template_literal_with_multiple_expressions() {
    let tokens = tokenize("const s = `${a} + ${b} = ${a + b}`;");
    let has_template = tokens
        .iter()
        .any(|t| matches!(t.kind, TokenKind::TemplateLiteral));
    let has_add = tokens
        .iter()
        .any(|t| matches!(t.kind, TokenKind::Operator(OperatorType::Add)));
    assert!(
        has_template,
        "Should contain template literal with expressions"
    );
    assert!(has_add, "Should tokenize expressions within template");
}

#[test]
fn tokenize_regex_with_flags() {
    let tokens = tokenize("const re = /^[a-z]+$/gim;");
    let has_regex = tokens
        .iter()
        .any(|t| matches!(t.kind, TokenKind::RegExpLiteral));
    assert!(has_regex, "Should contain regex with flags");
}

#[test]
fn tokenize_regex_in_condition() {
    let tokens = tokenize("if (/test/.test(str)) { console.log(str); }");
    let has_regex = tokens
        .iter()
        .any(|t| matches!(t.kind, TokenKind::RegExpLiteral));
    let has_if = tokens
        .iter()
        .any(|t| matches!(t.kind, TokenKind::Keyword(KeywordType::If)));
    assert!(has_regex, "Should contain regex in condition");
    assert!(has_if, "Should contain if keyword");
}

#[test]
fn tokenize_call_expression_with_arguments() {
    let tokens = tokenize("foo(1, 'hello', true);");
    let has_open_paren = tokens
        .iter()
        .any(|t| matches!(t.kind, TokenKind::Punctuation(PunctuationType::OpenParen)));
    let has_close_paren = tokens
        .iter()
        .any(|t| matches!(t.kind, TokenKind::Punctuation(PunctuationType::CloseParen)));
    let comma_count = tokens
        .iter()
        .filter(|t| matches!(t.kind, TokenKind::Operator(OperatorType::Comma)))
        .count();
    assert!(has_open_paren, "Call should have open paren");
    assert!(has_close_paren, "Call should have close paren");
    assert!(
        comma_count >= 3,
        "3 arguments should produce at least 3 commas (one per arg), got {comma_count}"
    );
}

#[test]
fn tokenize_new_expression_with_arguments() {
    let tokens = tokenize("new Foo(1, 2);");
    let has_new = tokens
        .iter()
        .any(|t| matches!(t.kind, TokenKind::Keyword(KeywordType::New)));
    let comma_count = tokens
        .iter()
        .filter(|t| matches!(t.kind, TokenKind::Operator(OperatorType::Comma)))
        .count();
    assert!(has_new);
    assert!(
        comma_count >= 2,
        "2 arguments should produce at least 2 commas, got {comma_count}"
    );
}

#[test]
fn tokenize_arrow_function_params_produce_commas() {
    let tokens = tokenize("const f = (a, b, c) => a;");
    let comma_count = tokens
        .iter()
        .filter(|t| matches!(t.kind, TokenKind::Operator(OperatorType::Comma)))
        .count();
    assert!(
        comma_count >= 3,
        "Arrow function with 3 params should produce at least 3 commas, got {comma_count}"
    );
}

#[test]
fn tokenize_function_params_produce_commas() {
    let tokens = tokenize("function f(a, b) { return a + b; }");
    let comma_count = tokens
        .iter()
        .filter(|t| matches!(t.kind, TokenKind::Operator(OperatorType::Comma)))
        .count();
    assert!(
        comma_count >= 2,
        "Function with 2 params should produce at least 2 commas, got {comma_count}"
    );
}
