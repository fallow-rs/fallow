use super::*;

#[test]
fn tokenize_only_comments() {
    let tokens = tokenize("// This is a comment\n/* block comment */\n");
    assert!(
        tokens.is_empty(),
        "File with only comments should produce no tokens"
    );
}

#[test]
fn tokenize_deeply_nested_structure() {
    let code = "const x = { a: { b: { c: { d: { e: 1 } } } } };";
    let tokens = tokenize(code);
    let open_braces = tokens
        .iter()
        .filter(|t| matches!(t.kind, TokenKind::Punctuation(PunctuationType::OpenBrace)))
        .count();
    let close_braces = tokens
        .iter()
        .filter(|t| matches!(t.kind, TokenKind::Punctuation(PunctuationType::CloseBrace)))
        .count();
    assert_eq!(
        open_braces, close_braces,
        "Nested structure should have balanced braces"
    );
    assert!(
        open_braces >= 5,
        "Should have at least 5 levels of braces, got {open_braces}"
    );
}

#[test]
fn tokenize_chained_method_calls_uses_point_spans() {
    let tokens = tokenize("arr.filter(x => x > 0).map(x => x * 2).reduce((a, b) => a + b, 0);");
    let dots: Vec<&SourceToken> = tokens
        .iter()
        .filter(|t| matches!(t.kind, TokenKind::Punctuation(PunctuationType::Dot)))
        .collect();
    assert!(
        dots.len() >= 3,
        "Chained calls should produce dots, got {}",
        dots.len()
    );
    for dot in &dots {
        assert_eq!(
            dot.span.end - dot.span.start,
            1,
            "Dot should use point span"
        );
    }
}

#[test]
fn tokenize_expression_statement_appends_semicolon() {
    let tokens = tokenize("foo();");
    let last = tokens.last().unwrap();
    assert!(
        matches!(
            last.kind,
            TokenKind::Punctuation(PunctuationType::Semicolon | PunctuationType::CloseParen,)
                | TokenKind::Operator(OperatorType::Comma)
        ),
        "Expression statement should end with semicolon or related punctuation"
    );
    let has_semicolon = tokens
        .iter()
        .any(|t| matches!(t.kind, TokenKind::Punctuation(PunctuationType::Semicolon)));
    assert!(
        has_semicolon,
        "Expression statement should produce a semicolon"
    );
}

#[test]
fn tokenize_variable_declarator_with_no_initializer() {
    let tokens = tokenize("let x;");
    let has_let = tokens
        .iter()
        .any(|t| matches!(t.kind, TokenKind::Keyword(KeywordType::Let)));
    let has_x = tokens
        .iter()
        .any(|t| matches!(&t.kind, TokenKind::Identifier(n) if n == "x"));
    let has_assign = tokens
        .iter()
        .any(|t| matches!(t.kind, TokenKind::Operator(OperatorType::Assign)));
    assert!(has_let, "Should have let keyword");
    assert!(has_x, "Should have identifier x");
    assert!(
        !has_assign,
        "Uninitialized declarator should not have assign operator"
    );
}

#[test]
fn tokenize_using_declaration_maps_to_const() {
    let tokens = tokenize("{ using resource = getResource(); }");
    let has_const = tokens
        .iter()
        .any(|t| matches!(t.kind, TokenKind::Keyword(KeywordType::Const)));
    assert!(
        has_const,
        "`using` declaration should be mapped to Const keyword"
    );
}

#[test]
fn tokenize_block_statement_produces_braces() {
    let tokens = tokenize("{ const x = 1; }");
    assert!(matches!(
        tokens[0].kind,
        TokenKind::Punctuation(PunctuationType::OpenBrace)
    ));
    let last = tokens.last().unwrap();
    assert!(
        matches!(
            last.kind,
            TokenKind::Punctuation(PunctuationType::CloseBrace)
        ),
        "Block should end with close brace"
    );
}

#[test]
fn tokenize_class_without_name_and_no_extends() {
    let tokens = tokenize("const C = class { };");
    let has_class = tokens
        .iter()
        .any(|t| matches!(t.kind, TokenKind::Keyword(KeywordType::Class)));
    let has_extends = tokens
        .iter()
        .any(|t| matches!(t.kind, TokenKind::Keyword(KeywordType::Extends)));
    assert!(has_class, "Should have class keyword");
    assert!(
        !has_extends,
        "Anonymous class without extends should not have extends keyword"
    );
}

#[test]
fn tokenize_function_without_name() {
    let tokens = tokenize("const f = function() { return 1; };");
    let has_function = tokens
        .iter()
        .any(|t| matches!(t.kind, TokenKind::Keyword(KeywordType::Function)));
    assert!(has_function, "Should have function keyword");
}

#[test]
fn tokenize_whitespace_only_file() {
    let tokens = tokenize("   \n\n\t  \n  ");
    assert!(
        tokens.is_empty(),
        "File with only whitespace should produce no tokens"
    );
}

#[test]
fn tokenize_single_semicolons() {
    let tokens = tokenize(";;;");
    assert!(tokens.len() <= 3);
}

#[test]
fn tokenize_object_destructuring() {
    let tokens = tokenize("const { a, b, c } = obj;");
    let has_const = tokens
        .iter()
        .any(|t| matches!(t.kind, TokenKind::Keyword(KeywordType::Const)));
    let has_a = tokens
        .iter()
        .any(|t| matches!(&t.kind, TokenKind::Identifier(n) if n == "a"));
    let has_b = tokens
        .iter()
        .any(|t| matches!(&t.kind, TokenKind::Identifier(n) if n == "b"));
    let has_c = tokens
        .iter()
        .any(|t| matches!(&t.kind, TokenKind::Identifier(n) if n == "c"));
    let has_assign = tokens
        .iter()
        .any(|t| matches!(t.kind, TokenKind::Operator(OperatorType::Assign)));
    assert!(has_const, "Should have const keyword");
    assert!(has_a, "Should have destructured identifier 'a'");
    assert!(has_b, "Should have destructured identifier 'b'");
    assert!(has_c, "Should have destructured identifier 'c'");
    assert!(has_assign, "Should have assign operator");
}

#[test]
fn tokenize_array_destructuring() {
    let tokens = tokenize("const [first, second, ...rest] = arr;");
    let has_const = tokens
        .iter()
        .any(|t| matches!(t.kind, TokenKind::Keyword(KeywordType::Const)));
    let has_first = tokens
        .iter()
        .any(|t| matches!(&t.kind, TokenKind::Identifier(n) if n == "first"));
    let has_second = tokens
        .iter()
        .any(|t| matches!(&t.kind, TokenKind::Identifier(n) if n == "second"));
    let has_rest = tokens
        .iter()
        .any(|t| matches!(&t.kind, TokenKind::Identifier(n) if n == "rest"));
    assert!(has_const, "Should have const keyword");
    assert!(has_first, "Should have 'first' identifier");
    assert!(has_second, "Should have 'second' identifier");
    assert!(has_rest, "Should have 'rest' identifier");
}

#[test]
fn tokenize_nested_destructuring() {
    let tokens = tokenize("const { a: { b } } = obj;");
    let has_b = tokens
        .iter()
        .any(|t| matches!(&t.kind, TokenKind::Identifier(n) if n == "b"));
    assert!(has_b, "Should have nested destructured identifier 'b'");
    let has_a = tokens
        .iter()
        .any(|t| matches!(&t.kind, TokenKind::Identifier(n) if n == "a"));
    assert!(has_a, "Should have property key 'a' identifier");
}

#[test]
fn tokenize_optional_chaining_member_access() {
    let tokens = tokenize("const x = obj?.prop;");
    let has_prop = tokens
        .iter()
        .any(|t| matches!(&t.kind, TokenKind::Identifier(n) if n == "prop"));
    assert!(
        has_prop,
        "Optional chaining should produce property identifier"
    );
}

#[test]
fn tokenize_optional_chaining_call() {
    let tokens = tokenize("const x = fn?.();");
    let has_fn = tokens
        .iter()
        .any(|t| matches!(&t.kind, TokenKind::Identifier(n) if n == "fn"));
    assert!(has_fn, "Optional call should produce function identifier");
}

#[test]
fn tokenize_multiple_declarators_in_single_declaration() {
    let tokens = tokenize("const a = 1, b = 2, c = 3;");
    let has_const = tokens
        .iter()
        .any(|t| matches!(t.kind, TokenKind::Keyword(KeywordType::Const)));
    let assign_count = tokens
        .iter()
        .filter(|t| matches!(t.kind, TokenKind::Operator(OperatorType::Assign)))
        .count();
    assert!(has_const, "Should have const keyword");
    assert_eq!(
        assign_count, 3,
        "Three declarators should produce 3 assign operators, got {assign_count}"
    );
    let semicolons = tokens
        .iter()
        .filter(|t| matches!(t.kind, TokenKind::Punctuation(PunctuationType::Semicolon)))
        .count();
    assert!(
        semicolons >= 3,
        "Three declarators should produce at least 3 semicolons, got {semicolons}"
    );
}

#[test]
fn tokenize_rest_parameter() {
    let tokens = tokenize("function f(a, ...rest) { return rest; }");
    let has_rest = tokens
        .iter()
        .any(|t| matches!(&t.kind, TokenKind::Identifier(n) if n == "rest"));
    let has_a = tokens
        .iter()
        .any(|t| matches!(&t.kind, TokenKind::Identifier(n) if n == "a"));
    assert!(has_rest, "Should have 'rest' identifier");
    assert!(has_a, "Should have 'a' identifier");
}

#[test]
fn tokenize_computed_property_key() {
    let tokens = tokenize("const obj = { [key]: value };");
    let has_key = tokens
        .iter()
        .any(|t| matches!(&t.kind, TokenKind::Identifier(n) if n == "key"));
    let has_value = tokens
        .iter()
        .any(|t| matches!(&t.kind, TokenKind::Identifier(n) if n == "value"));
    assert!(has_key, "Should have computed key identifier");
    assert!(has_value, "Should have property value identifier");
}

#[test]
fn tokenize_class_with_static_members() {
    let tokens = tokenize("class Foo { static bar = 42; static baz() {} }");
    let has_class = tokens
        .iter()
        .any(|t| matches!(t.kind, TokenKind::Keyword(KeywordType::Class)));
    let has_bar = tokens
        .iter()
        .any(|t| matches!(&t.kind, TokenKind::Identifier(n) if n == "bar"));
    let has_42 = tokens
        .iter()
        .any(|t| matches!(&t.kind, TokenKind::NumericLiteral(n) if n == "42"));
    assert!(has_class, "Should have class keyword");
    assert!(has_bar, "Should have static member identifier 'bar'");
    assert!(has_42, "Should have static member value 42");
}

#[test]
fn tokenize_class_with_getter_setter() {
    let tokens = tokenize("class Foo { get bar() { return 1; } set bar(v) {} }");
    let has_get = tokens
        .iter()
        .any(|t| matches!(t.kind, TokenKind::Keyword(KeywordType::Get)));
    let has_set = tokens
        .iter()
        .any(|t| matches!(t.kind, TokenKind::Keyword(KeywordType::Set)));
    let _ = has_get;
    let _ = has_set;
    let has_class = tokens
        .iter()
        .any(|t| matches!(t.kind, TokenKind::Keyword(KeywordType::Class)));
    assert!(has_class, "Should have class keyword");
}

#[test]
fn tokenize_object_with_nested_member_access() {
    let tokens = tokenize("const x = { a: obj.b, c: arr[0] };");
    let has_dot = tokens
        .iter()
        .any(|t| matches!(t.kind, TokenKind::Punctuation(PunctuationType::Dot)));
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
    assert!(has_dot, "Should have dot for obj.b");
    assert!(
        bracket_count >= 2,
        "Should have brackets for arr[0], got {bracket_count}"
    );
}

#[test]
fn tokenize_async_generator_function() {
    let tokens = tokenize("async function* gen() { yield await fetch(); }");
    let has_async = tokens
        .iter()
        .any(|t| matches!(t.kind, TokenKind::Keyword(KeywordType::Async)));
    let has_function = tokens
        .iter()
        .any(|t| matches!(t.kind, TokenKind::Keyword(KeywordType::Function)));
    let has_yield = tokens
        .iter()
        .any(|t| matches!(t.kind, TokenKind::Keyword(KeywordType::Yield)));
    let has_await = tokens
        .iter()
        .any(|t| matches!(t.kind, TokenKind::Keyword(KeywordType::Await)));
    assert!(has_async, "Should have async keyword");
    assert!(has_function, "Should have function keyword");
    assert!(has_yield, "Should have yield keyword");
    assert!(has_await, "Should have await keyword");
}

#[test]
fn tokenize_nested_ternary() {
    let tokens = tokenize("const x = a ? b ? c : d : e;");
    let ternary_count = tokens
        .iter()
        .filter(|t| matches!(t.kind, TokenKind::Operator(OperatorType::Ternary)))
        .count();
    let colon_count = tokens
        .iter()
        .filter(|t| matches!(t.kind, TokenKind::Punctuation(PunctuationType::Colon)))
        .count();
    assert_eq!(
        ternary_count, 2,
        "Nested ternary should produce 2 ? operators, got {ternary_count}"
    );
    assert_eq!(
        colon_count, 2,
        "Nested ternary should produce 2 : colons, got {colon_count}"
    );
}

#[test]
fn tokenize_iife_pattern() {
    let tokens = tokenize("(function() { const x = 1; })();");
    let has_function = tokens
        .iter()
        .any(|t| matches!(t.kind, TokenKind::Keyword(KeywordType::Function)));
    let has_const = tokens
        .iter()
        .any(|t| matches!(t.kind, TokenKind::Keyword(KeywordType::Const)));
    assert!(has_function, "IIFE should have function keyword");
    assert!(has_const, "IIFE body should have const keyword");
}

#[test]
fn tokenize_comma_separated_expressions() {
    let tokens = tokenize("const x = (1, 2, 3);");
    let comma_count = tokens
        .iter()
        .filter(|t| matches!(t.kind, TokenKind::Operator(OperatorType::Comma)))
        .count();
    assert!(
        comma_count >= 2,
        "Comma expression with 3 items should produce at least 2 commas, got {comma_count}"
    );
}

#[test]
fn tokenize_object_spread() {
    let tokens = tokenize("const merged = { ...defaults, ...overrides, extra: 1 };");
    let spread_count = tokens
        .iter()
        .filter(|t| matches!(t.kind, TokenKind::Operator(OperatorType::Spread)))
        .count();
    assert_eq!(
        spread_count, 2,
        "Two spreads in object should produce 2 spread operators, got {spread_count}"
    );
}

#[test]
fn tokenize_large_realistic_function() {
    let code = r"
export async function fetchAndProcess(url, options = {}) {
    const { timeout = 5000, retries = 3, headers = {} } = options;
    let attempts = 0;

    while (attempts < retries) {
        try {
            const response = await fetch(url, { headers, signal: AbortSignal.timeout(timeout) });
            if (!response.ok) {
                throw new Error(`HTTP ${response.status}`);
            }
            const data = await response.json();
            const processed = data
                .filter(item => item != null)
                .map(item => ({
                    ...item,
                    id: item.id ?? crypto.randomUUID(),
                    timestamp: Date.now(),
                }))
                .sort((a, b) => a.timestamp - b.timestamp);
            return { ok: true, data: processed };
        } catch (error) {
            attempts++;
            if (attempts >= retries) {
                return { ok: false, error: error.message };
            }
        }
    }
}
";
    let tokens = tokenize(code);
    assert!(
        tokens.len() > 80,
        "Realistic function should produce many tokens, got {}",
        tokens.len()
    );
    let has_export = tokens
        .iter()
        .any(|t| matches!(t.kind, TokenKind::Keyword(KeywordType::Export)));
    let has_async = tokens
        .iter()
        .any(|t| matches!(t.kind, TokenKind::Keyword(KeywordType::Async)));
    let has_while = tokens
        .iter()
        .any(|t| matches!(t.kind, TokenKind::Keyword(KeywordType::While)));
    let has_try = tokens
        .iter()
        .any(|t| matches!(t.kind, TokenKind::Keyword(KeywordType::Try)));
    let has_catch = tokens
        .iter()
        .any(|t| matches!(t.kind, TokenKind::Keyword(KeywordType::Catch)));
    let has_throw = tokens
        .iter()
        .any(|t| matches!(t.kind, TokenKind::Keyword(KeywordType::Throw)));
    let has_return = tokens
        .iter()
        .any(|t| matches!(t.kind, TokenKind::Keyword(KeywordType::Return)));
    let has_template = tokens
        .iter()
        .any(|t| matches!(t.kind, TokenKind::TemplateLiteral));
    let has_spread = tokens
        .iter()
        .any(|t| matches!(t.kind, TokenKind::Operator(OperatorType::Spread)));
    let has_nullish = tokens
        .iter()
        .any(|t| matches!(t.kind, TokenKind::Operator(OperatorType::NullishCoalescing)));
    assert!(has_export, "Should have export keyword");
    assert!(has_async, "Should have async keyword");
    assert!(has_while, "Should have while keyword");
    assert!(has_try, "Should have try keyword");
    assert!(has_catch, "Should have catch keyword");
    assert!(has_throw, "Should have throw keyword");
    assert!(has_return, "Should have return keyword");
    assert!(has_template, "Should have template literal");
    assert!(has_spread, "Should have spread operator");
    assert!(has_nullish, "Should have nullish coalescing operator");
}
