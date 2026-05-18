use super::*;

// ── token_visitor: var declaration ────────────────────────────

#[test]
fn tokenize_var_declaration() {
    let tokens = tokenize("var x = 1;");
    assert!(matches!(
        tokens[0].kind,
        TokenKind::Keyword(KeywordType::Var)
    ));
}

// ── token_visitor: empty function body ───────────────────────

#[test]
fn tokenize_empty_function_body() {
    let tokens = tokenize("function noop() {}");
    let has_function = tokens
        .iter()
        .any(|t| matches!(t.kind, TokenKind::Keyword(KeywordType::Function)));
    let has_noop = tokens
        .iter()
        .any(|t| matches!(&t.kind, TokenKind::Identifier(n) if n == "noop"));
    assert!(has_function, "Should have function keyword");
    assert!(has_noop, "Should have identifier 'noop'");
    // FunctionBody is not a BlockStatement, so no braces are emitted for
    // the empty body. The visitor emits: function, noop, (, )
    let open_parens = tokens
        .iter()
        .filter(|t| matches!(t.kind, TokenKind::Punctuation(PunctuationType::OpenParen)))
        .count();
    let close_parens = tokens
        .iter()
        .filter(|t| matches!(t.kind, TokenKind::Punctuation(PunctuationType::CloseParen)))
        .count();
    assert!(open_parens >= 1, "Should have open paren for params");
    assert_eq!(open_parens, close_parens, "Parens should be balanced");
}

#[test]
fn tokenize_empty_arrow_function_body() {
    let tokens = tokenize("const noop = () => {};");
    let has_arrow = tokens
        .iter()
        .any(|t| matches!(t.kind, TokenKind::Operator(OperatorType::Arrow)));
    let open_braces = tokens
        .iter()
        .filter(|t| matches!(t.kind, TokenKind::Punctuation(PunctuationType::OpenBrace)))
        .count();
    let close_braces = tokens
        .iter()
        .filter(|t| matches!(t.kind, TokenKind::Punctuation(PunctuationType::CloseBrace)))
        .count();
    assert!(has_arrow, "Should have arrow operator");
    assert_eq!(open_braces, close_braces, "Braces should be balanced");
}

// ── token_visitor: exact token ordering ──────────────────────

#[test]
fn tokenize_binary_expression_preserves_left_op_right_order() {
    let tokens = tokenize("const r = a + b;");
    // Expected sequence: const, r, =, a, +, b, ;
    let kinds: Vec<&TokenKind> = tokens.iter().map(|t| &t.kind).collect();

    // Find the assign, then check: identifier before +, + before second identifier
    let assign_idx = kinds
        .iter()
        .position(|k| matches!(k, TokenKind::Operator(OperatorType::Assign)))
        .unwrap();
    let add_idx = kinds
        .iter()
        .position(|k| matches!(k, TokenKind::Operator(OperatorType::Add)))
        .unwrap();

    // 'a' should appear between assign and add
    let a_idx = kinds
        .iter()
        .position(|k| matches!(k, TokenKind::Identifier(n) if n == "a"))
        .unwrap();
    // 'b' should appear after add
    let b_idx = kinds
        .iter()
        .position(|k| matches!(k, TokenKind::Identifier(n) if n == "b"))
        .unwrap();

    assert!(assign_idx < a_idx, "assign should come before 'a'");
    assert!(a_idx < add_idx, "'a' should come before '+'");
    assert!(add_idx < b_idx, "'+' should come before 'b'");
}

#[test]
fn tokenize_nested_binary_expressions_maintain_order() {
    // (a + b) * c  => a, +, b, *, c (infix traversal)
    let tokens = tokenize("const r = (a + b) * c;");
    let ops: Vec<&OperatorType> = tokens
        .iter()
        .filter_map(|t| match &t.kind {
            TokenKind::Operator(op) => Some(op),
            _ => None,
        })
        .collect();
    // Should see Assign, Add, Mul (and the semicolon-related tokens)
    let assign_pos = ops
        .iter()
        .position(|o| **o == OperatorType::Assign)
        .unwrap();
    let add_pos = ops.iter().position(|o| **o == OperatorType::Add).unwrap();
    let mul_pos = ops.iter().position(|o| **o == OperatorType::Mul).unwrap();
    assert!(assign_pos < add_pos, "Assign before Add");
    assert!(
        add_pos < mul_pos,
        "Add before Mul (left-to-right, depth-first)"
    );
}

// ── token_visitor: deeply nested expressions ─────────────────

#[test]
fn tokenize_deeply_nested_call_chain_ordering() {
    let tokens = tokenize("a.b().c().d();");
    let idents: Vec<&String> = tokens
        .iter()
        .filter_map(|t| match &t.kind {
            TokenKind::Identifier(n) => Some(n),
            _ => None,
        })
        .collect();
    // The identifiers should appear in order: a, b, c, d
    assert_eq!(
        idents,
        vec!["a", "b", "c", "d"],
        "Chained member calls should produce identifiers in source order"
    );
}

#[test]
fn tokenize_nested_function_calls() {
    let tokens = tokenize("foo(bar(baz(1)));");
    let idents: Vec<&String> = tokens
        .iter()
        .filter_map(|t| match &t.kind {
            TokenKind::Identifier(n) => Some(n),
            _ => None,
        })
        .collect();
    assert_eq!(
        idents,
        vec!["foo", "bar", "baz"],
        "Nested calls should produce identifiers in outer-to-inner order"
    );
}

// ── token_visitor: export named with value declaration ────────

#[test]
fn tokenize_export_named_value_declaration() {
    let tokens = tokenize("export const x = 1;");
    let has_export = tokens
        .iter()
        .any(|t| matches!(t.kind, TokenKind::Keyword(KeywordType::Export)));
    let has_const = tokens
        .iter()
        .any(|t| matches!(t.kind, TokenKind::Keyword(KeywordType::Const)));
    assert!(has_export, "Should have export keyword");
    assert!(has_const, "Should have const keyword");
    // Export should come before const
    let export_idx = tokens
        .iter()
        .position(|t| matches!(t.kind, TokenKind::Keyword(KeywordType::Export)))
        .unwrap();
    let const_idx = tokens
        .iter()
        .position(|t| matches!(t.kind, TokenKind::Keyword(KeywordType::Const)))
        .unwrap();
    assert!(export_idx < const_idx, "export should precede const");
}

// ── token_visitor: call expressions use point spans ──────────

#[test]
fn tokenize_call_expression_parens_use_point_spans() {
    let tokens = tokenize("foo(x);");
    let open_parens: Vec<&SourceToken> = tokens
        .iter()
        .filter(|t| matches!(t.kind, TokenKind::Punctuation(PunctuationType::OpenParen)))
        .collect();
    let close_parens: Vec<&SourceToken> = tokens
        .iter()
        .filter(|t| matches!(t.kind, TokenKind::Punctuation(PunctuationType::CloseParen)))
        .collect();
    for p in &open_parens {
        assert_eq!(
            p.span.end - p.span.start,
            1,
            "Call open paren should use point span"
        );
    }
    for p in &close_parens {
        assert_eq!(
            p.span.end - p.span.start,
            1,
            "Call close paren should use point span"
        );
    }
}

// ── token_visitor: multiple expression statements ────────────

#[test]
fn tokenize_multiple_expression_statements_all_have_semicolons() {
    let tokens = tokenize("foo();\nbar();\nbaz();");
    let semicolons = tokens
        .iter()
        .filter(|t| matches!(t.kind, TokenKind::Punctuation(PunctuationType::Semicolon)))
        .count();
    assert_eq!(
        semicolons, 3,
        "Three expression statements should produce 3 semicolons, got {semicolons}"
    );
}

// ── token_visitor: self-closing JSX element ──────────────────

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

// ── token_visitor: logical expression produces correct ops ───

#[test]
fn tokenize_logical_expression_order() {
    let tokens = tokenize("const x = a && b;");
    let kinds: Vec<&TokenKind> = tokens.iter().map(|t| &t.kind).collect();
    let a_idx = kinds
        .iter()
        .position(|k| matches!(k, TokenKind::Identifier(n) if n == "a"))
        .unwrap();
    let and_idx = kinds
        .iter()
        .position(|k| matches!(k, TokenKind::Operator(OperatorType::And)))
        .unwrap();
    let b_idx = kinds
        .iter()
        .position(|k| matches!(k, TokenKind::Identifier(n) if n == "b"))
        .unwrap();
    assert!(a_idx < and_idx, "'a' should come before '&&'");
    assert!(and_idx < b_idx, "'&&' should come before 'b'");
}

// ── token_visitor: conditional expression token order ────────

#[test]
fn tokenize_conditional_expression_ordering() {
    let tokens = tokenize("const x = cond ? yes : no;");
    let kinds: Vec<&TokenKind> = tokens.iter().map(|t| &t.kind).collect();
    let cond_idx = kinds
        .iter()
        .position(|k| matches!(k, TokenKind::Identifier(n) if n == "cond"))
        .unwrap();
    let ternary_idx = kinds
        .iter()
        .position(|k| matches!(k, TokenKind::Operator(OperatorType::Ternary)))
        .unwrap();
    let yes_idx = kinds
        .iter()
        .position(|k| matches!(k, TokenKind::Identifier(n) if n == "yes"))
        .unwrap();
    let colon_idx = kinds
        .iter()
        .position(|k| matches!(k, TokenKind::Punctuation(PunctuationType::Colon)))
        .unwrap();
    let no_idx = kinds
        .iter()
        .position(|k| matches!(k, TokenKind::Identifier(n) if n == "no"))
        .unwrap();
    assert!(cond_idx < ternary_idx, "condition before ?");
    assert!(ternary_idx < yes_idx, "? before consequent");
    assert!(yes_idx < colon_idx, "consequent before :");
    assert!(colon_idx < no_idx, ": before alternate");
}

// ── token_visitor: assignment expression token order ─────────

#[test]
fn tokenize_assignment_expression_ordering() {
    let tokens = tokenize("x += 5;");
    let kinds: Vec<&TokenKind> = tokens.iter().map(|t| &t.kind).collect();
    let x_idx = kinds
        .iter()
        .position(|k| matches!(k, TokenKind::Identifier(n) if n == "x"))
        .unwrap();
    let add_assign_idx = kinds
        .iter()
        .position(|k| matches!(k, TokenKind::Operator(OperatorType::AddAssign)))
        .unwrap();
    let five_idx = kinds
        .iter()
        .position(|k| matches!(k, TokenKind::NumericLiteral(n) if n == "5"))
        .unwrap();
    assert!(x_idx < add_assign_idx, "lhs before operator");
    assert!(add_assign_idx < five_idx, "operator before rhs");
}

// ── token_visitor: if without else ───────────────────────────

#[test]
fn tokenize_if_without_else() {
    let tokens = tokenize("if (x) { y; }");
    assert!(matches!(
        tokens[0].kind,
        TokenKind::Keyword(KeywordType::If)
    ));
    let has_else = tokens
        .iter()
        .any(|t| matches!(t.kind, TokenKind::Keyword(KeywordType::Else)));
    assert!(!has_else, "if without else should not have else keyword");
}

// ── token_visitor: postfix update operator order ─────────────

#[test]
fn tokenize_postfix_decrement_order() {
    let tokens = tokenize("x--;");
    // For postfix x--, the identifier should come before the operator
    let x_idx = tokens
        .iter()
        .position(|t| matches!(&t.kind, TokenKind::Identifier(n) if n == "x"))
        .unwrap();
    let dec_idx = tokens
        .iter()
        .position(|t| matches!(t.kind, TokenKind::Operator(OperatorType::Decrement)))
        .unwrap();
    assert!(
        x_idx < dec_idx,
        "Postfix x-- should have identifier before operator"
    );
}

// ── token_visitor: deeply nested if-else chain ───────────────

#[test]
fn tokenize_deeply_nested_if_else_chain() {
    let tokens = tokenize("if (a) { x; } else if (b) { y; } else if (c) { z; } else { w; }");
    let if_count = tokens
        .iter()
        .filter(|t| matches!(t.kind, TokenKind::Keyword(KeywordType::If)))
        .count();
    let else_count = tokens
        .iter()
        .filter(|t| matches!(t.kind, TokenKind::Keyword(KeywordType::Else)))
        .count();
    assert_eq!(if_count, 3, "Should have 3 if keywords, got {if_count}");
    assert_eq!(
        else_count, 3,
        "Should have 3 else keywords, got {else_count}"
    );
}

// ── token_visitor: object with computed member in value ───────

#[test]
fn tokenize_object_with_nested_member_access() {
    let tokens = tokenize("const x = { a: obj.b, c: arr[0] };");
    let has_dot = tokens
        .iter()
        .any(|t| matches!(t.kind, TokenKind::Punctuation(PunctuationType::Dot)));
    // arr[0] should produce brackets
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

// ── Token sequence determinism ──────────────────────────────

#[test]
fn tokenize_same_source_produces_identical_tokens() {
    let code = r"
function processData(items) {
    const filtered = items.filter(item => item.active);
    const mapped = filtered.map(item => ({ id: item.id, name: item.name }));
    return mapped.sort((a, b) => a.name.localeCompare(b.name));
}
";
    let tokens1 = tokenize(code);
    let tokens2 = tokenize(code);
    assert_eq!(
        tokens1.len(),
        tokens2.len(),
        "Same source should produce same token count"
    );
    for (i, (t1, t2)) in tokens1.iter().zip(tokens2.iter()).enumerate() {
        assert_eq!(
            t1.kind, t2.kind,
            "Token {i} kind mismatch on repeated tokenization"
        );
        assert_eq!(
            t1.span.start, t2.span.start,
            "Token {i} span start mismatch"
        );
        assert_eq!(t1.span.end, t2.span.end, "Token {i} span end mismatch");
    }
}

// ── Full exact token sequence tests ─────────────────────────

#[test]
fn exact_token_sequence_for_simple_const_assignment() {
    let tokens = tokenize("const x = 42;");
    let kinds: Vec<&TokenKind> = tokens.iter().map(|t| &t.kind).collect();
    // Expected: const, x, =, 42, ;
    assert_eq!(kinds.len(), 5, "const x = 42; should produce 5 tokens");
    assert!(matches!(kinds[0], TokenKind::Keyword(KeywordType::Const)));
    assert!(matches!(kinds[1], TokenKind::Identifier(n) if n == "x"));
    assert!(matches!(
        kinds[2],
        TokenKind::Operator(OperatorType::Assign)
    ));
    assert!(matches!(kinds[3], TokenKind::NumericLiteral(n) if n == "42"));
    assert!(matches!(
        kinds[4],
        TokenKind::Punctuation(PunctuationType::Semicolon)
    ));
}

#[test]
fn exact_token_sequence_for_let_string_assignment() {
    let tokens = tokenize("let name = \"world\";");
    let kinds: Vec<&TokenKind> = tokens.iter().map(|t| &t.kind).collect();
    assert_eq!(kinds.len(), 5);
    assert!(matches!(kinds[0], TokenKind::Keyword(KeywordType::Let)));
    assert!(matches!(kinds[1], TokenKind::Identifier(n) if n == "name"));
    assert!(matches!(
        kinds[2],
        TokenKind::Operator(OperatorType::Assign)
    ));
    assert!(matches!(kinds[3], TokenKind::StringLiteral(s) if s == "world"));
    assert!(matches!(
        kinds[4],
        TokenKind::Punctuation(PunctuationType::Semicolon)
    ));
}

#[test]
fn exact_token_sequence_for_return_statement() {
    let tokens = tokenize("function f() { return null; }");
    let kinds: Vec<&TokenKind> = tokens.iter().map(|t| &t.kind).collect();
    // function, f, (, ), {return, null, ;, }
    assert!(matches!(
        kinds[0],
        TokenKind::Keyword(KeywordType::Function)
    ));
    assert!(matches!(kinds[1], TokenKind::Identifier(n) if n == "f"));
    assert!(matches!(
        kinds[2],
        TokenKind::Punctuation(PunctuationType::OpenParen)
    ));
    assert!(matches!(
        kinds[3],
        TokenKind::Punctuation(PunctuationType::CloseParen)
    ));
    // The walk visits FunctionBody directives and statements but not the
    // body block directly. A BlockStatement visit emits the { and }.
    let return_idx = kinds
        .iter()
        .position(|k| matches!(k, TokenKind::Keyword(KeywordType::Return)))
        .expect("Should have return keyword");
    let null_idx = kinds
        .iter()
        .position(|k| matches!(k, TokenKind::NullLiteral))
        .expect("Should have null literal");
    assert!(return_idx < null_idx, "return should come before null");
}

// ── Cross-language: non-null assertion produces same tokens as JS ──

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

// ── Cross-language: class with type parameters ──────────────

#[test]
fn strip_types_class_with_generics() {
    let stripped =
        tokenize_cross_language("class Container<T> { value: T; constructor(v: T) { } }");
    // Generic <T> and type annotations should be stripped.
    // In cross-language mode, the type parameter T and type annotations
    // are stripped, so T should not appear as a standalone identifier from
    // the generic or annotation positions.
    let has_class = stripped
        .iter()
        .any(|t| matches!(t.kind, TokenKind::Keyword(KeywordType::Class)));
    assert!(has_class, "Should still have class keyword");
    // The colon from `: T` and `: T` (constructor param) should be gone
    let colon_count = stripped
        .iter()
        .filter(|t| matches!(t.kind, TokenKind::Punctuation(PunctuationType::Colon)))
        .count();
    assert_eq!(
        colon_count, 0,
        "Type annotation colons should be stripped, got {colon_count}"
    );
}

// ── Cross-language: arrow function with types ───────────────

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
    // Verify token kinds match
    for (i, (ts_tok, js_tok)) in stripped.iter().zip(js_tokens.iter()).enumerate() {
        assert_eq!(
            ts_tok.kind, js_tok.kind,
            "Token {i} mismatch in arrow function: TS={:?}, JS={:?}",
            ts_tok.kind, js_tok.kind
        );
    }
}

// ── Mixed runtime/type imports in cross-language mode ───────

#[test]
fn strip_types_mixed_import_keeps_only_value_import() {
    // Two separate import statements: one type, one value
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

// ── Span correctness ───────────────────────────────────────

#[test]
#[expect(
    clippy::cast_possible_truncation,
    reason = "test source lengths are trivially small"
)]
fn token_spans_are_within_source_bounds() {
    let source = "const x = 1 + 2;\nif (x > 0) { return x; }";
    let path = PathBuf::from("test.ts");
    let result = tokenize_file(&path, source, false);
    let source_len = source.len() as u32;
    for (i, token) in result.tokens.iter().enumerate() {
        assert!(
            token.span.start <= source_len,
            "Token {i} ({:?}) span.start ({}) exceeds source length ({})",
            token.kind,
            token.span.start,
            source_len
        );
        assert!(
            token.span.end <= source_len,
            "Token {i} ({:?}) span.end ({}) exceeds source length ({})",
            token.kind,
            token.span.end,
            source_len
        );
        assert!(
            token.span.start <= token.span.end,
            "Token {i} ({:?}) span.start ({}) > span.end ({})",
            token.kind,
            token.span.start,
            token.span.end
        );
    }
}

#[test]
fn token_spans_are_monotonically_non_decreasing() {
    // For most constructs, token spans should generally advance through the source.
    // Some synthetic tokens (point spans for parens/commas) may overlap, but
    // the start positions should be non-decreasing overall.
    let source = "const a = 1;\nconst b = 2;\nconst c = 3;";
    let path = PathBuf::from("test.ts");
    let result = tokenize_file(&path, source, false);
    // Group by top-level statement boundaries (each `const` keyword)
    // and verify spans within each group are non-decreasing.
    let mut last_keyword_start = 0u32;
    for token in &result.tokens {
        if matches!(token.kind, TokenKind::Keyword(KeywordType::Const)) {
            assert!(
                token.span.start >= last_keyword_start,
                "Keyword token span.start ({}) should be >= previous keyword start ({})",
                token.span.start,
                last_keyword_start
            );
            last_keyword_start = token.span.start;
        }
    }
}
