use super::*;

#[test]
fn point_span_at_zero() {
    let span = point_span(0);
    assert_eq!(span.start, 0);
    assert_eq!(span.end, 1);
}

#[test]
fn point_span_at_large_offset() {
    let span = point_span(1_000_000);
    assert_eq!(span.start, 1_000_000);
    assert_eq!(span.end, 1_000_001);
    assert_eq!(span.end - span.start, 1);
}

#[test]
fn point_span_near_u32_max() {
    let span = point_span(u32::MAX - 1);
    assert_eq!(span.start, u32::MAX - 1);
    assert_eq!(span.end, u32::MAX);
}

#[test]
fn source_token_construction_and_field_access() {
    let token = SourceToken {
        kind: TokenKind::Keyword(KeywordType::Const),
        span: Span::new(10, 15),
    };
    assert!(matches!(token.kind, TokenKind::Keyword(KeywordType::Const)));
    assert_eq!(token.span.start, 10);
    assert_eq!(token.span.end, 15);
}

#[test]
fn source_token_clone() {
    let token = SourceToken {
        kind: TokenKind::Identifier("foo".to_string()),
        span: Span::new(0, 3),
    };
    let cloned = token.clone();
    assert_eq!(cloned.span.start, token.span.start);
    assert_eq!(cloned.span.end, token.span.end);
    assert!(matches!(&cloned.kind, TokenKind::Identifier(n) if n == "foo"));
}

#[test]
fn file_tokens_direct_construction() {
    let tokens = vec![
        SourceToken {
            kind: TokenKind::Keyword(KeywordType::Const),
            span: Span::new(0, 5),
        },
        SourceToken {
            kind: TokenKind::Identifier("x".to_string()),
            span: Span::new(6, 7),
        },
    ];
    let ft = FileTokens {
        tokens,
        atomic_invocation_spans: Vec::new(),
        source: "const x".to_string(),
        line_count: 1,
    };
    assert_eq!(ft.tokens.len(), 2);
    assert_eq!(ft.source, "const x");
    assert_eq!(ft.line_count, 1);
}

#[test]
fn file_tokens_empty_construction() {
    let ft = FileTokens {
        tokens: Vec::new(),
        atomic_invocation_spans: Vec::new(),
        source: String::new(),
        line_count: 0,
    };
    assert!(ft.tokens.is_empty());
    assert!(ft.source.is_empty());
    assert_eq!(ft.line_count, 0);
}

#[test]
fn file_tokens_clone() {
    let ft = FileTokens {
        tokens: vec![SourceToken {
            kind: TokenKind::NullLiteral,
            span: Span::new(0, 4),
        }],
        atomic_invocation_spans: Vec::new(),
        source: "null".to_string(),
        line_count: 1,
    };
    let cloned = ft;
    assert_eq!(cloned.tokens.len(), 1);
    assert_eq!(cloned.source, "null");
    assert_eq!(cloned.line_count, 1);
}

#[test]
fn token_kind_equality_and_hash() {
    use rustc_hash::FxHashSet;

    let mut set = FxHashSet::default();
    set.insert(TokenKind::Keyword(KeywordType::Const));
    set.insert(TokenKind::Keyword(KeywordType::Let));
    set.insert(TokenKind::Keyword(KeywordType::Const)); // duplicate

    assert_eq!(
        set.len(),
        2,
        "HashSet should deduplicate identical TokenKinds"
    );

    assert_eq!(
        TokenKind::NullLiteral,
        TokenKind::NullLiteral,
        "Same variants should be equal"
    );
    assert_ne!(
        TokenKind::BooleanLiteral(true),
        TokenKind::BooleanLiteral(false),
        "Different boolean values should not be equal"
    );
    assert_eq!(
        TokenKind::StringLiteral("hello".to_string()),
        TokenKind::StringLiteral("hello".to_string()),
        "Same string values should be equal"
    );
    assert_ne!(
        TokenKind::StringLiteral("a".to_string()),
        TokenKind::StringLiteral("b".to_string()),
        "Different string values should not be equal"
    );
}
