use super::*;

#[test]
fn tokenize_for_in_statement() {
    let tokens = tokenize("for (const key in obj) { console.log(key); }");
    assert!(matches!(
        tokens[0].kind,
        TokenKind::Keyword(KeywordType::For)
    ));
    let has_in = tokens
        .iter()
        .any(|t| matches!(t.kind, TokenKind::Keyword(KeywordType::In)));
    assert!(has_in, "Should contain 'in' keyword");
}

#[test]
fn tokenize_for_of_statement() {
    let tokens = tokenize("for (const item of items) { process(item); }");
    assert!(matches!(
        tokens[0].kind,
        TokenKind::Keyword(KeywordType::For)
    ));
    let has_of = tokens
        .iter()
        .any(|t| matches!(t.kind, TokenKind::Keyword(KeywordType::Of)));
    assert!(has_of, "Should contain 'of' keyword");
}

#[test]
fn tokenize_while_statement() {
    let tokens = tokenize("while (x > 0) { x--; }");
    assert!(matches!(
        tokens[0].kind,
        TokenKind::Keyword(KeywordType::While)
    ));
    let has_gt = tokens
        .iter()
        .any(|t| matches!(t.kind, TokenKind::Operator(OperatorType::Gt)));
    assert!(has_gt, "Should contain greater-than operator");
}

#[test]
fn tokenize_do_while_statement() {
    let tokens = tokenize("do { x++; } while (x < 10);");
    assert!(matches!(
        tokens[0].kind,
        TokenKind::Keyword(KeywordType::Do)
    ));
    let has_increment = tokens
        .iter()
        .any(|t| matches!(t.kind, TokenKind::Operator(OperatorType::Increment)));
    assert!(has_increment, "do-while body should contain increment");
    let has_lt = tokens
        .iter()
        .any(|t| matches!(t.kind, TokenKind::Operator(OperatorType::Lt)));
    assert!(has_lt, "do-while condition should contain < operator");
}

#[test]
fn tokenize_switch_case_default() {
    let tokens = tokenize("switch (x) { case 1: break; case 2: break; default: return; }");
    assert!(matches!(
        tokens[0].kind,
        TokenKind::Keyword(KeywordType::Switch)
    ));
    let case_count = tokens
        .iter()
        .filter(|t| matches!(t.kind, TokenKind::Keyword(KeywordType::Case)))
        .count();
    assert_eq!(case_count, 2, "Should have two case keywords");
    let has_default = tokens
        .iter()
        .any(|t| matches!(t.kind, TokenKind::Keyword(KeywordType::Default)));
    assert!(has_default, "Should have default keyword");
    let has_break = tokens
        .iter()
        .any(|t| matches!(t.kind, TokenKind::Keyword(KeywordType::Break)));
    assert!(has_break, "Should have break keyword");
    let colon_count = tokens
        .iter()
        .filter(|t| matches!(t.kind, TokenKind::Punctuation(PunctuationType::Colon)))
        .count();
    assert!(
        colon_count >= 3,
        "Should have at least 3 colons (case, case, default), got {colon_count}"
    );
}

#[test]
fn tokenize_continue_statement() {
    let tokens = tokenize("for (let i = 0; i < 10; i++) { if (i === 5) continue; }");
    let has_continue = tokens
        .iter()
        .any(|t| matches!(t.kind, TokenKind::Keyword(KeywordType::Continue)));
    assert!(has_continue, "Should contain continue keyword");
}

#[test]
fn tokenize_try_catch_finally() {
    let tokens = tokenize("try { foo(); } catch (e) { bar(); } finally { baz(); }");
    let has_try = tokens
        .iter()
        .any(|t| matches!(t.kind, TokenKind::Keyword(KeywordType::Try)));
    let has_catch = tokens
        .iter()
        .any(|t| matches!(t.kind, TokenKind::Keyword(KeywordType::Catch)));
    let has_finally = tokens
        .iter()
        .any(|t| matches!(t.kind, TokenKind::Keyword(KeywordType::Finally)));
    assert!(has_try, "Should contain try keyword");
    assert!(has_catch, "Should contain catch keyword");
    assert!(
        !has_finally,
        "Finally keyword is not emitted (no visitor override)"
    );
}

#[test]
fn tokenize_throw_statement() {
    let tokens = tokenize("throw new Error('fail');");
    assert!(matches!(
        tokens[0].kind,
        TokenKind::Keyword(KeywordType::Throw)
    ));
    let has_new = tokens
        .iter()
        .any(|t| matches!(t.kind, TokenKind::Keyword(KeywordType::New)));
    assert!(has_new, "Should contain new keyword");
}

#[test]
fn tokenize_for_statement_with_all_clauses() {
    let tokens = tokenize("for (let i = 0; i < 10; i++) { console.log(i); }");
    assert!(matches!(
        tokens[0].kind,
        TokenKind::Keyword(KeywordType::For)
    ));
    let has_open_paren = tokens
        .iter()
        .any(|t| matches!(t.kind, TokenKind::Punctuation(PunctuationType::OpenParen)));
    let has_close_paren = tokens
        .iter()
        .any(|t| matches!(t.kind, TokenKind::Punctuation(PunctuationType::CloseParen)));
    assert!(has_open_paren, "For statement should have open paren");
    assert!(has_close_paren, "For statement should have close paren");
}

#[test]
fn tokenize_switch_with_open_close_parens() {
    let tokens = tokenize("switch (x) { case 1: break; }");
    let has_open_paren = tokens
        .iter()
        .any(|t| matches!(t.kind, TokenKind::Punctuation(PunctuationType::OpenParen)));
    let has_close_paren = tokens
        .iter()
        .any(|t| matches!(t.kind, TokenKind::Punctuation(PunctuationType::CloseParen)));
    assert!(
        has_open_paren,
        "Switch should have open paren for discriminant"
    );
    assert!(
        has_close_paren,
        "Switch should have close paren for discriminant"
    );
}

#[test]
fn tokenize_while_has_parens_around_condition() {
    let tokens = tokenize("while (true) { break; }");
    let has_open_paren = tokens
        .iter()
        .any(|t| matches!(t.kind, TokenKind::Punctuation(PunctuationType::OpenParen)));
    let has_close_paren = tokens
        .iter()
        .any(|t| matches!(t.kind, TokenKind::Punctuation(PunctuationType::CloseParen)));
    assert!(has_open_paren, "While should have open paren");
    assert!(has_close_paren, "While should have close paren");
}

#[test]
fn tokenize_for_in_has_parens() {
    let tokens = tokenize("for (const k in obj) {}");
    let open_parens = tokens
        .iter()
        .filter(|t| matches!(t.kind, TokenKind::Punctuation(PunctuationType::OpenParen)))
        .count();
    let close_parens = tokens
        .iter()
        .filter(|t| matches!(t.kind, TokenKind::Punctuation(PunctuationType::CloseParen)))
        .count();
    assert!(open_parens >= 1, "for-in should have open paren");
    assert!(close_parens >= 1, "for-in should have close paren");
}

#[test]
fn tokenize_for_of_has_parens() {
    let tokens = tokenize("for (const v of arr) {}");
    let open_parens = tokens
        .iter()
        .filter(|t| matches!(t.kind, TokenKind::Punctuation(PunctuationType::OpenParen)))
        .count();
    let close_parens = tokens
        .iter()
        .filter(|t| matches!(t.kind, TokenKind::Punctuation(PunctuationType::CloseParen)))
        .count();
    assert!(open_parens >= 1, "for-of should have open paren");
    assert!(close_parens >= 1, "for-of should have close paren");
}

#[test]
fn tokenize_for_statement_empty_clauses() {
    let tokens = tokenize("for (;;) { break; }");
    let has_for = tokens
        .iter()
        .any(|t| matches!(t.kind, TokenKind::Keyword(KeywordType::For)));
    let has_break = tokens
        .iter()
        .any(|t| matches!(t.kind, TokenKind::Keyword(KeywordType::Break)));
    assert!(has_for, "Should have for keyword");
    assert!(has_break, "Should have break keyword");
}

#[test]
fn tokenize_labeled_statement() {
    let tokens = tokenize("outer: for (let i = 0; i < 10; i++) { continue outer; }");
    let has_for = tokens
        .iter()
        .any(|t| matches!(t.kind, TokenKind::Keyword(KeywordType::For)));
    let has_continue = tokens
        .iter()
        .any(|t| matches!(t.kind, TokenKind::Keyword(KeywordType::Continue)));
    assert!(has_for, "Should have for keyword in labeled loop");
    assert!(has_continue, "Should have continue keyword");
}
