//! CSS value canonicalization (Phase 4): the `"style"` lexical path collapses
//! zero units (`0px` -> `0`) and expands hex colors (`#fff` -> `#ffffff`) so the
//! SA-IS clone engine catches near-miss / value-drifted CSS clones. The
//! canonicalization is scoped to the CSS path; the markup path (`css = false`) is
//! byte-identical, which is what protects JS-clone parity.

use super::*;
use std::path::PathBuf;

fn tokenize_css(code: &str) -> Vec<SourceToken> {
    tokenize_file(&PathBuf::from("test.css"), code, false).tokens
}

/// The token values of every `NumericLiteral` / `Identifier` token, in order.
fn value_tokens(tokens: &[SourceToken]) -> Vec<String> {
    tokens
        .iter()
        .filter_map(|t| match &t.kind {
            TokenKind::NumericLiteral(v) | TokenKind::Identifier(v) => Some(v.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn css_zero_unit_collapses_to_bare_zero() {
    let with_unit = value_tokens(&tokenize_css(
        ".a { margin: 0px; padding: 0em; inset: 0%; }",
    ));
    let bare = value_tokens(&tokenize_css(".a { margin: 0; padding: 0; inset: 0; }"));
    assert_eq!(with_unit, bare, "zero-with-unit must hash like bare 0");
    assert!(
        with_unit
            .iter()
            .all(|v| v != "0px" && v != "0em" && v != "0%"),
        "no zero-unit survives: {with_unit:?}"
    );
}

#[test]
fn css_nonzero_value_keeps_its_unit() {
    let tokens = value_tokens(&tokenize_css(".a { margin: 16px; opacity: 0.5; }"));
    assert!(
        tokens.contains(&"16px".to_string()),
        "nonzero keeps unit: {tokens:?}"
    );
    assert!(
        tokens.contains(&"0.5".to_string()),
        "fractional nonzero kept: {tokens:?}"
    );
}

#[test]
fn css_hex_color_shorthand_expands_and_lowercases() {
    let short = value_tokens(&tokenize_css(".a { color: #FAB; }"));
    let long = value_tokens(&tokenize_css(".a { color: #ffaabb; }"));
    assert!(
        short.contains(&"#ffaabb".to_string()),
        "3-digit expands: {short:?}"
    );
    assert_eq!(short, long, "#FAB and #ffaabb must hash equal");

    // 4-digit alpha shorthand expands to 8-digit.
    let alpha_short = value_tokens(&tokenize_css(".a { color: #abcd; }"));
    let alpha_long = value_tokens(&tokenize_css(".a { color: #aabbccdd; }"));
    assert_eq!(
        alpha_short, alpha_long,
        "#abcd and #aabbccdd must hash equal"
    );
}

#[test]
fn css_id_selector_is_not_treated_as_hex_color() {
    // `#main` is an id selector, not a hex color: it keeps the existing behavior
    // (the `#` is dropped, the identifier `main` remains), so no `#...` token.
    let tokens = value_tokens(&tokenize_css("#main { color: red; }"));
    assert!(
        tokens.contains(&"main".to_string()),
        "id name survives: {tokens:?}"
    );
    assert!(
        !tokens.iter().any(|v| v.starts_with('#')),
        "an id selector is not a hex-color token: {tokens:?}"
    );
}

#[test]
fn fuzzy_drifted_recipe_hashes_equal_under_css() {
    // The Phase 4 value: two box-shadow recipes that differ only by zero-unit and
    // hex notation drift now tokenize identically (so the SA-IS engine matches),
    // where the old character-naive tokenizer produced different streams.
    // Same selector, drifted values: after canonicalization the token streams are
    // identical, so the SA-IS engine sees a clone. (Distinct selectors still differ
    // on the selector token, which is correct; the engine matches the shared
    // declaration subsequence.)
    let a = value_tokens(&tokenize_css(".x { box-shadow: 0 1px 2px #000; }"));
    let b = value_tokens(&tokenize_css(".x { box-shadow: 0px 1px 2px #000000; }"));
    assert_eq!(
        a, b,
        "value-drifted box-shadow recipes must canonicalize equal"
    );
}

#[test]
fn markup_lexical_path_is_not_canonicalized() {
    // The markup path (`css = false`) must leave values byte-identical, which is
    // what keeps the JS/markup clone counts unchanged (criterion 2). Drive the
    // lexer directly with both flags over the same source.
    let src = "a { margin: 0px; color: #fff }";
    let css = super::lexical::tokenize_lexical_region(src, 0, true);
    let markup = super::lexical::tokenize_lexical_region(src, 0, false);
    let css_vals = value_tokens(&css);
    let markup_vals = value_tokens(&markup);
    assert!(
        css_vals.contains(&"0".to_string()),
        "css collapses 0px: {css_vals:?}"
    );
    assert!(
        css_vals.contains(&"#ffffff".to_string()),
        "css expands hex: {css_vals:?}"
    );
    assert!(
        markup_vals.contains(&"0px".to_string()),
        "markup keeps 0px verbatim: {markup_vals:?}"
    );
    assert!(
        !markup_vals.iter().any(|v| v.starts_with('#')),
        "markup does not synthesize a hex-color token: {markup_vals:?}"
    );
}
