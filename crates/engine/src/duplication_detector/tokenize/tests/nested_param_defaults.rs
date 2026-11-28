use super::*;

/// `({a=({a=...})=>1})=>1` with `levels` functions.
fn nested_destructured_defaults(levels: usize) -> String {
    format!(
        "export const f = {}1{};",
        "({a=".repeat(levels),
        "})=>1".repeat(levels)
    )
}

#[test]
fn nested_destructured_parameter_defaults_keep_the_token_count_linear() {
    let levels = 16;
    let tokens = tokenize(&nested_destructured_defaults(levels));
    // Each parameter pattern is tokenized twice. Without a limit the copy of
    // a pattern also copies the functions in its defaults, so the count
    // doubles per level.
    assert!(tokens.len() <= 64 * 11 * levels, "tokens: {}", tokens.len());
}

/// Code with a few nested defaults keeps the tokens it had before the limit.
#[test]
fn shallow_destructured_parameter_defaults_keep_their_tokens() {
    for (levels, expected) in [(1, 17), (2, 39), (3, 83), (4, 171)] {
        let tokens = tokenize(&nested_destructured_defaults(levels));
        assert_eq!(tokens.len(), expected, "levels: {levels}");
    }
}
