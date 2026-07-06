use super::*;
use proptest::prelude::*;
use std::path::PathBuf;

#[expect(
    clippy::cast_possible_truncation,
    reason = "test source lengths are trivially small"
)]
mod inner {
    use super::*;

    proptest! {
        /// Tokenizing the same source twice always produces the same token sequence.
        #[test]
        fn tokenize_is_deterministic(source in "[a-zA-Z0-9 (){};=+\\-*/'\",.<>:!?\\n]{1,200}") {
            let path = PathBuf::from("test.ts");
            let tokens1 = tokenize_file(&path, &source, false).tokens;
            let tokens2 = tokenize_file(&path, &source, false).tokens;
            prop_assert_eq!(tokens1.len(), tokens2.len(), "Token count should be deterministic");
            for (a, b) in tokens1.iter().zip(tokens2.iter()) {
                prop_assert_eq!(&a.kind, &b.kind, "Token kinds should match");
                prop_assert_eq!(a.span.start, b.span.start, "Span starts should match");
                prop_assert_eq!(a.span.end, b.span.end, "Span ends should match");
            }
        }

        /// All token spans must be within source byte bounds.
        #[test]
        fn all_spans_within_source_bounds(source in "[a-zA-Z0-9 (){};=+\\-*/'\",.<>:!?\\n]{1,200}") {
            let path = PathBuf::from("test.ts");
            let file_tokens = tokenize_file(&path, &source, false);
            let source_len = file_tokens.source.len() as u32;
            for token in &file_tokens.tokens {
                prop_assert!(
                    token.span.start <= source_len,
                    "Span start {} exceeds source length {}",
                    token.span.start, source_len
                );
                prop_assert!(
                    token.span.end <= source_len,
                    "Span end {} exceeds source length {}",
                    token.span.end, source_len
                );
                prop_assert!(
                    token.span.start <= token.span.end,
                    "Span start {} > end {}",
                    token.span.start, token.span.end
                );
            }
        }

        /// Tokenizing never panics on arbitrary JS-like input.
        #[test]
        fn tokenize_never_panics(source in "[a-zA-Z0-9 (){};=+\\-*/'\",.<>:!?\\[\\]\\n]{0,300}") {
            let path = PathBuf::from("test.ts");
            let _ = tokenize_file(&path, &source, false);
        }

        /// Line count is always >= 1.
        #[test]
        fn line_count_at_least_one(source in ".*") {
            let path = PathBuf::from("test.ts");
            let ft = tokenize_file(&path, &source, false);
            prop_assert!(ft.line_count >= 1, "line_count should be at least 1");
        }
    }
}
