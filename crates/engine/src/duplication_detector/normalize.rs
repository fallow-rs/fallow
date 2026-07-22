use xxhash_rust::xxh3::xxh3_64;

use super::tokenize::{SourceToken, TokenKind};
use fallow_config::{DetectionMode, NormalizationConfig, ResolvedNormalization};

/// A token with a precomputed hash for use in the detection engine.
#[derive(Debug, Clone)]
pub struct HashedToken {
    /// Hash of the normalized token.
    pub hash: u64,
    /// Index of this token in the original (pre-normalization) token sequence.
    pub original_index: usize,
}

/// Normalize and hash a token sequence according to the detection mode.
///
/// Returns a vector of `HashedToken` values ready for the Rabin-Karp sliding window.
/// Tokens that should be skipped (based on mode) are excluded from the output.
#[must_use]
#[cfg_attr(
    not(test),
    allow(
        dead_code,
        reason = "mode-based normalization entry point is exercised by tests"
    )
)]
pub fn normalize_and_hash(tokens: &[SourceToken], mode: DetectionMode) -> Vec<HashedToken> {
    let resolved = ResolvedNormalization::resolve(mode, &NormalizationConfig::default());
    normalize_and_hash_resolved(tokens, resolved)
}

/// Normalize and hash with explicit resolved normalization flags.
///
/// This is the primary normalization entry point when using configurable overrides.
#[must_use]
pub(crate) fn normalize_and_hash_resolved(
    tokens: &[SourceToken],
    normalization: ResolvedNormalization,
) -> Vec<HashedToken> {
    let mut result = Vec::with_capacity(tokens.len());
    let mut namespace = "js";

    for (i, token) in tokens.iter().enumerate() {
        if let TokenKind::Boundary(name) = &token.kind {
            namespace = name;
        }
        let hash = hash_token_resolved(&token.kind, normalization, namespace);
        result.push(HashedToken {
            hash,
            original_index: i,
        });
    }

    result
}

/// Hash a single token using resolved normalization flags.
fn hash_token_resolved(kind: &TokenKind, norm: ResolvedNormalization, namespace: &str) -> u64 {
    match kind {
        TokenKind::Keyword(kw) => hash_namespaced(namespace, &[0, *kw as u8]),
        TokenKind::Identifier(name) => {
            if norm.ignore_identifiers {
                hash_namespaced(namespace, &[1, 0])
            } else {
                let mut buf = vec![1];
                buf.extend_from_slice(name.as_bytes());
                hash_namespaced(namespace, &buf)
            }
        }
        TokenKind::StringLiteral(val) => {
            if norm.ignore_string_values {
                hash_namespaced(namespace, &[2, 0])
            } else {
                let mut buf = vec![2];
                buf.extend_from_slice(val.as_bytes());
                hash_namespaced(namespace, &buf)
            }
        }
        TokenKind::NumericLiteral(val) => {
            if norm.ignore_numeric_values {
                hash_namespaced(namespace, &[3, 0])
            } else {
                let mut buf = vec![3];
                buf.extend_from_slice(val.as_bytes());
                hash_namespaced(namespace, &buf)
            }
        }
        TokenKind::BooleanLiteral(val) => hash_namespaced(namespace, &[4, u8::from(*val)]),
        TokenKind::NullLiteral => hash_namespaced(namespace, &[5]),
        TokenKind::TemplateLiteral => hash_namespaced(namespace, &[6]),
        TokenKind::RegExpLiteral => hash_namespaced(namespace, &[7]),
        TokenKind::Operator(op) => hash_namespaced(namespace, &[8, *op as u8]),
        TokenKind::Punctuation(p) => hash_namespaced(namespace, &[9, *p as u8]),
        TokenKind::Boundary(name) => {
            let mut buf = vec![10];
            buf.extend_from_slice(name.as_bytes());
            hash_bytes(&buf)
        }
    }
}

fn hash_namespaced(namespace: &str, data: &[u8]) -> u64 {
    let mut buf = Vec::with_capacity(namespace.len() + 1 + data.len());
    buf.extend_from_slice(namespace.as_bytes());
    buf.push(0);
    buf.extend_from_slice(data);
    hash_bytes(&buf)
}

/// Hash a byte slice using xxh3.
fn hash_bytes(data: &[u8]) -> u64 {
    xxh3_64(data)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::duplicates::tokenize::{KeywordType, OperatorType, PunctuationType};
    use oxc_span::Span;

    fn make_token(kind: TokenKind) -> SourceToken {
        SourceToken {
            kind,
            span: Span::new(0, 0),
        }
    }

    #[test]
    fn strict_mode_preserves_identifiers() {
        let tokens = vec![
            make_token(TokenKind::Identifier("foo".to_string())),
            make_token(TokenKind::Identifier("bar".to_string())),
        ];

        let hashed = normalize_and_hash(&tokens, DetectionMode::Strict);
        assert_eq!(hashed.len(), 2);
        assert_ne!(hashed[0].hash, hashed[1].hash);
    }

    #[test]
    fn semantic_mode_blinds_identifiers() {
        let tokens = vec![
            make_token(TokenKind::Identifier("foo".to_string())),
            make_token(TokenKind::Identifier("bar".to_string())),
        ];

        let hashed = normalize_and_hash(&tokens, DetectionMode::Semantic);
        assert_eq!(hashed.len(), 2);
        assert_eq!(hashed[0].hash, hashed[1].hash);
    }

    #[test]
    fn semantic_mode_blinds_string_literals() {
        let tokens = vec![
            make_token(TokenKind::StringLiteral("hello".to_string())),
            make_token(TokenKind::StringLiteral("world".to_string())),
        ];

        let hashed = normalize_and_hash(&tokens, DetectionMode::Semantic);
        assert_eq!(hashed.len(), 2);
        assert_eq!(hashed[0].hash, hashed[1].hash);
    }

    #[test]
    fn semantic_mode_blinds_numeric_literals() {
        let tokens = vec![
            make_token(TokenKind::NumericLiteral("42".to_string())),
            make_token(TokenKind::NumericLiteral("99".to_string())),
        ];

        let hashed = normalize_and_hash(&tokens, DetectionMode::Semantic);
        assert_eq!(hashed.len(), 2);
        assert_eq!(hashed[0].hash, hashed[1].hash);
    }

    #[test]
    fn semantic_mode_preserves_booleans() {
        let tokens = vec![
            make_token(TokenKind::BooleanLiteral(true)),
            make_token(TokenKind::BooleanLiteral(false)),
        ];

        let hashed = normalize_and_hash(&tokens, DetectionMode::Semantic);
        assert_eq!(hashed.len(), 2);
        assert_ne!(hashed[0].hash, hashed[1].hash);
    }

    #[test]
    fn semantic_mode_preserves_keywords() {
        let tokens = vec![
            make_token(TokenKind::Keyword(KeywordType::If)),
            make_token(TokenKind::Keyword(KeywordType::While)),
        ];

        let hashed = normalize_and_hash(&tokens, DetectionMode::Semantic);
        assert_eq!(hashed.len(), 2);
        assert_ne!(hashed[0].hash, hashed[1].hash);
    }

    #[test]
    fn preserves_original_indices() {
        let tokens = vec![
            make_token(TokenKind::Keyword(KeywordType::Const)),
            make_token(TokenKind::Identifier("x".to_string())),
            make_token(TokenKind::Operator(OperatorType::Assign)),
        ];

        let hashed = normalize_and_hash(&tokens, DetectionMode::Mild);
        assert_eq!(hashed.len(), 3);
        assert_eq!(hashed[0].original_index, 0);
        assert_eq!(hashed[1].original_index, 1);
        assert_eq!(hashed[2].original_index, 2);
    }

    #[test]
    fn empty_input_produces_empty_output() {
        let tokens: Vec<SourceToken> = vec![];
        let hashed = normalize_and_hash(&tokens, DetectionMode::Mild);
        assert!(hashed.is_empty());
    }

    #[test]
    fn operators_have_distinct_hashes() {
        let tokens = vec![
            make_token(TokenKind::Operator(OperatorType::Add)),
            make_token(TokenKind::Operator(OperatorType::Sub)),
        ];

        let hashed = normalize_and_hash(&tokens, DetectionMode::Strict);
        assert_ne!(hashed[0].hash, hashed[1].hash);
    }

    #[test]
    fn punctuation_has_distinct_hashes() {
        let tokens = vec![
            make_token(TokenKind::Punctuation(PunctuationType::OpenParen)),
            make_token(TokenKind::Punctuation(PunctuationType::CloseParen)),
        ];

        let hashed = normalize_and_hash(&tokens, DetectionMode::Strict);
        assert_ne!(hashed[0].hash, hashed[1].hash);
    }

    #[test]
    fn null_literal_has_stable_hash() {
        let tokens = vec![make_token(TokenKind::NullLiteral)];
        let h1 = normalize_and_hash(&tokens, DetectionMode::Strict);
        let h2 = normalize_and_hash(&tokens, DetectionMode::Semantic);
        assert_eq!(h1[0].hash, h2[0].hash);
    }

    #[test]
    fn template_literal_has_stable_hash() {
        let tokens = vec![make_token(TokenKind::TemplateLiteral)];
        let h1 = normalize_and_hash(&tokens, DetectionMode::Strict);
        let h2 = normalize_and_hash(&tokens, DetectionMode::Semantic);
        assert_eq!(h1[0].hash, h2[0].hash);
    }

    #[test]
    fn regexp_literal_has_stable_hash() {
        let tokens = vec![make_token(TokenKind::RegExpLiteral)];
        let h1 = normalize_and_hash(&tokens, DetectionMode::Strict);
        let h2 = normalize_and_hash(&tokens, DetectionMode::Semantic);
        assert_eq!(h1[0].hash, h2[0].hash);
    }

    #[test]
    fn null_template_regexp_have_distinct_hashes() {
        let tokens = vec![
            make_token(TokenKind::NullLiteral),
            make_token(TokenKind::TemplateLiteral),
            make_token(TokenKind::RegExpLiteral),
        ];
        let hashed = normalize_and_hash(&tokens, DetectionMode::Strict);
        assert_ne!(hashed[0].hash, hashed[1].hash);
        assert_ne!(hashed[1].hash, hashed[2].hash);
        assert_ne!(hashed[0].hash, hashed[2].hash);
    }

    #[test]
    fn mild_mode_equivalent_to_strict() {
        let id_tokens = vec![
            make_token(TokenKind::Identifier("foo".to_string())),
            make_token(TokenKind::Identifier("bar".to_string())),
        ];
        let hashed = normalize_and_hash(&id_tokens, DetectionMode::Mild);
        assert_ne!(hashed[0].hash, hashed[1].hash);

        let str_tokens = vec![
            make_token(TokenKind::StringLiteral("hello".to_string())),
            make_token(TokenKind::StringLiteral("world".to_string())),
        ];
        let hashed = normalize_and_hash(&str_tokens, DetectionMode::Mild);
        assert_ne!(hashed[0].hash, hashed[1].hash);

        let num_tokens = vec![
            make_token(TokenKind::NumericLiteral("42".to_string())),
            make_token(TokenKind::NumericLiteral("99".to_string())),
        ];
        let hashed = normalize_and_hash(&num_tokens, DetectionMode::Mild);
        assert_ne!(hashed[0].hash, hashed[1].hash);
    }

    #[test]
    fn weak_mode_blinds_strings_only() {
        let id_tokens = vec![
            make_token(TokenKind::Identifier("foo".to_string())),
            make_token(TokenKind::Identifier("bar".to_string())),
        ];
        let hashed = normalize_and_hash(&id_tokens, DetectionMode::Weak);
        assert_ne!(hashed[0].hash, hashed[1].hash, "Weak preserves identifiers");

        let num_tokens = vec![
            make_token(TokenKind::NumericLiteral("42".to_string())),
            make_token(TokenKind::NumericLiteral("99".to_string())),
        ];
        let hashed = normalize_and_hash(&num_tokens, DetectionMode::Weak);
        assert_ne!(hashed[0].hash, hashed[1].hash, "Weak preserves numbers");
    }

    #[test]
    fn different_token_kinds_produce_distinct_hashes() {
        let tokens = vec![
            make_token(TokenKind::Keyword(KeywordType::Const)),
            make_token(TokenKind::Identifier("x".to_string())),
            make_token(TokenKind::StringLiteral("x".to_string())),
            make_token(TokenKind::NumericLiteral("1".to_string())),
            make_token(TokenKind::BooleanLiteral(true)),
            make_token(TokenKind::NullLiteral),
            make_token(TokenKind::TemplateLiteral),
            make_token(TokenKind::RegExpLiteral),
            make_token(TokenKind::Operator(OperatorType::Add)),
            make_token(TokenKind::Punctuation(PunctuationType::OpenParen)),
        ];
        let hashed = normalize_and_hash(&tokens, DetectionMode::Strict);
        for i in 0..hashed.len() {
            for j in (i + 1)..hashed.len() {
                assert_ne!(
                    hashed[i].hash, hashed[j].hash,
                    "Token at index {i} and {j} should have distinct hashes"
                );
            }
        }
    }

    #[test]
    fn resolved_strict_with_ignore_identifiers_override() {
        let norm = ResolvedNormalization {
            ignore_identifiers: true,
            ignore_string_values: false,
            ignore_numeric_values: false,
        };
        let tokens = vec![
            make_token(TokenKind::Identifier("foo".to_string())),
            make_token(TokenKind::Identifier("bar".to_string())),
        ];

        let hashed = normalize_and_hash_resolved(&tokens, norm);
        assert_eq!(hashed.len(), 2);
        assert_eq!(hashed[0].hash, hashed[1].hash);
    }

    #[test]
    fn resolved_strict_with_ignore_strings_override() {
        let norm = ResolvedNormalization {
            ignore_identifiers: false,
            ignore_string_values: true,
            ignore_numeric_values: false,
        };
        let tokens = vec![
            make_token(TokenKind::StringLiteral("hello".to_string())),
            make_token(TokenKind::StringLiteral("world".to_string())),
        ];

        let hashed = normalize_and_hash_resolved(&tokens, norm);
        assert_eq!(hashed[0].hash, hashed[1].hash);
    }

    #[test]
    fn resolved_strict_with_ignore_numbers_override() {
        let norm = ResolvedNormalization {
            ignore_identifiers: false,
            ignore_string_values: false,
            ignore_numeric_values: true,
        };
        let tokens = vec![
            make_token(TokenKind::NumericLiteral("42".to_string())),
            make_token(TokenKind::NumericLiteral("99".to_string())),
        ];

        let hashed = normalize_and_hash_resolved(&tokens, norm);
        assert_eq!(hashed[0].hash, hashed[1].hash);
    }

    #[test]
    fn resolved_semantic_with_preserve_identifiers_override() {
        let norm = ResolvedNormalization {
            ignore_identifiers: false,
            ignore_string_values: true,
            ignore_numeric_values: true,
        };
        let tokens = vec![
            make_token(TokenKind::Identifier("foo".to_string())),
            make_token(TokenKind::Identifier("bar".to_string())),
        ];

        let hashed = normalize_and_hash_resolved(&tokens, norm);
        assert_ne!(hashed[0].hash, hashed[1].hash);
    }

    #[test]
    fn resolved_normalization_from_mode_defaults() {
        use fallow_config::NormalizationConfig;

        let norm =
            ResolvedNormalization::resolve(DetectionMode::Strict, &NormalizationConfig::default());
        assert!(!norm.ignore_identifiers);
        assert!(!norm.ignore_string_values);
        assert!(!norm.ignore_numeric_values);

        let norm =
            ResolvedNormalization::resolve(DetectionMode::Weak, &NormalizationConfig::default());
        assert!(!norm.ignore_identifiers);
        assert!(norm.ignore_string_values);
        assert!(!norm.ignore_numeric_values);

        let norm = ResolvedNormalization::resolve(
            DetectionMode::Semantic,
            &NormalizationConfig::default(),
        );
        assert!(norm.ignore_identifiers);
        assert!(norm.ignore_string_values);
        assert!(norm.ignore_numeric_values);
    }

    #[test]
    fn resolved_normalization_overrides_mode_defaults() {
        use fallow_config::NormalizationConfig;

        let overrides = NormalizationConfig {
            ignore_identifiers: Some(true),
            ignore_string_values: None,
            ignore_numeric_values: None,
        };
        let norm = ResolvedNormalization::resolve(DetectionMode::Strict, &overrides);
        assert!(norm.ignore_identifiers);
        assert!(!norm.ignore_string_values);
        assert!(!norm.ignore_numeric_values);
    }

    mod proptests {
        use super::*;
        use crate::duplicates::tokenize::{KeywordType, OperatorType, PunctuationType};
        use oxc_span::Span;
        use proptest::prelude::*;

        fn make_token(kind: TokenKind) -> SourceToken {
            SourceToken {
                kind,
                span: Span::new(0, 0),
            }
        }

        fn arb_detection_mode() -> impl Strategy<Value = DetectionMode> {
            prop::sample::select(vec![
                DetectionMode::Strict,
                DetectionMode::Mild,
                DetectionMode::Weak,
                DetectionMode::Semantic,
            ])
        }

        fn arb_normalization() -> impl Strategy<Value = ResolvedNormalization> {
            (any::<bool>(), any::<bool>(), any::<bool>()).prop_map(|(ids, strings, nums)| {
                ResolvedNormalization {
                    ignore_identifiers: ids,
                    ignore_string_values: strings,
                    ignore_numeric_values: nums,
                }
            })
        }

        fn arb_token_kind() -> impl Strategy<Value = TokenKind> {
            prop_oneof![
                Just(TokenKind::Keyword(KeywordType::Const)),
                Just(TokenKind::Keyword(KeywordType::If)),
                Just(TokenKind::Keyword(KeywordType::Return)),
                "[a-zA-Z_][a-zA-Z0-9_]{0,30}".prop_map(TokenKind::Identifier),
                "[a-zA-Z0-9 _.,!?]{0,50}".prop_map(TokenKind::StringLiteral),
                "[0-9]{1,10}(\\.[0-9]{1,5})?".prop_map(TokenKind::NumericLiteral),
                any::<bool>().prop_map(TokenKind::BooleanLiteral),
                Just(TokenKind::NullLiteral),
                Just(TokenKind::TemplateLiteral),
                Just(TokenKind::RegExpLiteral),
                Just(TokenKind::Operator(OperatorType::Add)),
                Just(TokenKind::Operator(OperatorType::Assign)),
                Just(TokenKind::Punctuation(PunctuationType::OpenParen)),
                Just(TokenKind::Punctuation(PunctuationType::CloseParen)),
            ]
        }

        proptest! {
            /// Normalizing a token twice produces the same result as normalizing once (idempotency).
            #[test]
            fn normalization_is_idempotent(
                kind in arb_token_kind(),
                norm in arb_normalization(),
            ) {
                let token = make_token(kind);
                let first = normalize_and_hash_resolved(std::slice::from_ref(&token), norm);
                let second = normalize_and_hash_resolved(&[token], norm);
                prop_assert_eq!(first.len(), second.len());
                for (a, b) in first.iter().zip(second.iter()) {
                    prop_assert_eq!(a.hash, b.hash, "Normalization should be idempotent");
                }
            }

            /// Same input always produces the same output (determinism).
            #[test]
            fn normalization_is_deterministic(
                kinds in prop::collection::vec(arb_token_kind(), 1..20),
                mode in arb_detection_mode(),
            ) {
                let tokens: Vec<SourceToken> = kinds.into_iter().map(make_token).collect();
                let result1 = normalize_and_hash(&tokens, mode);
                let result2 = normalize_and_hash(&tokens, mode);
                prop_assert_eq!(result1.len(), result2.len());
                for (a, b) in result1.iter().zip(result2.iter()) {
                    prop_assert_eq!(a.hash, b.hash, "Same input must produce same hash");
                    prop_assert_eq!(a.original_index, b.original_index);
                }
            }

            /// Output length always equals input length (no tokens are filtered).
            #[test]
            fn output_length_matches_input(
                kinds in prop::collection::vec(arb_token_kind(), 0..30),
                mode in arb_detection_mode(),
            ) {
                let tokens: Vec<SourceToken> = kinds.into_iter().map(make_token).collect();
                let result = normalize_and_hash(&tokens, mode);
                prop_assert_eq!(
                    result.len(), tokens.len(),
                    "Output should have same length as input"
                );
            }

            /// Original indices should be sequential 0..n.
            #[test]
            fn original_indices_are_sequential(
                kinds in prop::collection::vec(arb_token_kind(), 1..20),
                norm in arb_normalization(),
            ) {
                let tokens: Vec<SourceToken> = kinds.into_iter().map(make_token).collect();
                let result = normalize_and_hash_resolved(&tokens, norm);
                for (i, hashed) in result.iter().enumerate() {
                    prop_assert_eq!(hashed.original_index, i);
                }
            }
        }
    }
}
