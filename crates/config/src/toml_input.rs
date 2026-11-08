//! TOML parsing for files that come from the analyzed repository.
//!
//! The `toml` crate parses arrays and inline tables recursively and stops at a
//! depth of 80. Its depth counter can go below zero: a closing bracket that
//! the parser skips after an error still lowers the counter. Each `[]` inside
//! an inline table that expects a key does this, so a crafted file can nest
//! far deeper than 80 levels and overflow the stack. [`check_nesting`] runs
//! before each parse and rejects such input.

use std::fmt;

use serde::de::DeserializeOwned;
use toml_parser::lexer::TokenKind;

/// The deepest bracket nesting that [`check_nesting`] accepts.
///
/// The value is above the depth limit of the `toml` crate (80), so every file
/// that the `toml` crate accepts also passes the check.
pub const MAX_NESTING: usize = 128;

/// The TOML source nests brackets deeper than [`MAX_NESTING`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NestingTooDeep;

impl fmt::Display for NestingTooDeep {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "TOML arrays and inline tables nest deeper than {MAX_NESTING} levels"
        )
    }
}

impl std::error::Error for NestingTooDeep {}

/// An error from [`from_str`].
#[derive(Debug)]
pub enum TomlInputError {
    /// The source nests brackets deeper than [`MAX_NESTING`].
    TooDeep(NestingTooDeep),
    /// The `toml` crate rejected the source.
    Parse(toml::de::Error),
}

impl fmt::Display for TomlInputError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooDeep(err) => err.fmt(f),
            Self::Parse(err) => err.fmt(f),
        }
    }
}

impl std::error::Error for TomlInputError {}

/// Reject `source` when its `[`/`{` brackets nest deeper than [`MAX_NESTING`].
///
/// The check reads the tokens of the lexer that the `toml` and `toml_edit`
/// parsers use, so a bracket in a string or a comment neither opens nor closes
/// a level, exactly as in the parser. A closing bracket closes the innermost
/// open bracket only when the two match. The result is an upper bound of the
/// recursion depth of both parsers, so a source that passes cannot overflow
/// the stack.
///
/// # Errors
///
/// Returns [`NestingTooDeep`] when the nesting exceeds [`MAX_NESTING`].
pub fn check_nesting(source: &str) -> Result<(), NestingTooDeep> {
    let mut open: Vec<TokenKind> = Vec::new();
    for token in toml_parser::Source::new(source).lex() {
        match token.kind() {
            kind @ (TokenKind::LeftSquareBracket | TokenKind::LeftCurlyBracket) => {
                if open.len() == MAX_NESTING {
                    return Err(NestingTooDeep);
                }
                open.push(kind);
            }
            TokenKind::RightSquareBracket if open.last() == Some(&TokenKind::LeftSquareBracket) => {
                open.pop();
            }
            TokenKind::RightCurlyBracket if open.last() == Some(&TokenKind::LeftCurlyBracket) => {
                open.pop();
            }
            _ => {}
        }
    }
    Ok(())
}

/// Check the nesting of `source`, then deserialize it with `toml::from_str`.
///
/// # Errors
///
/// Returns [`TomlInputError::TooDeep`] when [`check_nesting`] rejects the
/// source, and [`TomlInputError::Parse`] when the `toml` crate rejects it.
pub fn from_str<T: DeserializeOwned>(source: &str) -> Result<T, TomlInputError> {
    check_nesting(source).map_err(TomlInputError::TooDeep)?;
    toml::from_str(source).map_err(TomlInputError::Parse)
}

#[cfg(test)]
mod tests {
    use super::{MAX_NESTING, NestingTooDeep, TomlInputError, check_nesting, from_str};

    #[test]
    fn accepts_nesting_up_to_the_limit() {
        let source = format!("a = {}{}", "[".repeat(MAX_NESTING), "]".repeat(MAX_NESTING));
        assert_eq!(check_nesting(&source), Ok(()));
    }

    #[test]
    fn rejects_nesting_above_the_limit() {
        let depth = MAX_NESTING + 1;
        let source = format!("a = {}{}", "[".repeat(depth), "]".repeat(depth));
        assert_eq!(check_nesting(&source), Err(NestingTooDeep));
    }

    #[test]
    fn mismatched_closing_brackets_do_not_lower_the_depth() {
        let source = format!("a = [{}{}", "}".repeat(1_000), "[".repeat(MAX_NESTING));
        assert_eq!(check_nesting(&source), Err(NestingTooDeep));
    }

    #[test]
    fn sibling_tables_and_arrays_do_not_add_up() {
        let source = "[a]\nx = [1, [2, 3], {y = [4]}]\n[[b]]\nz = {w = {v = 1}}\n".repeat(1_000);
        assert_eq!(check_nesting(&source), Ok(()));
        let value: toml::Value = toml::from_str("x = [1, [2, 3], {y = [4]}]").unwrap();
        assert_eq!(
            from_str::<toml::Value>("x = [1, [2, 3], {y = [4]}]").unwrap(),
            value
        );
    }

    /// Each `[]` inside an inline table that expects a key lowers the depth
    /// counter of the `toml` parser by one. Without the check, the nested
    /// array that follows overflows the stack.
    #[test]
    fn rejects_input_that_evades_the_toml_depth_limit() {
        const REPEATS: usize = 200_000;
        let source = format!(
            "a = {{{}}}\nb = {}{}\n",
            "[]".repeat(REPEATS),
            "[".repeat(REPEATS),
            "]".repeat(REPEATS)
        );
        let err = from_str::<toml::Value>(&source).unwrap_err();
        assert!(matches!(err, TomlInputError::TooDeep(_)), "{err}");
    }

    /// A closing bracket inside a string or a comment is not structure. If the
    /// check read it as one, nested arrays with such strings would pass with
    /// any depth.
    #[test]
    fn closing_brackets_in_strings_and_comments_do_not_lower_the_depth() {
        let depth = MAX_NESTING + 1;
        for element in [
            "\"]\", ",
            "'}]', ",
            "\"\"\"]\n]\"\"\", ",
            "'''\n]]\n''', ",
            "\"\\\"]\", ",
            "# ]\n",
        ] {
            let source = format!(
                "a = {}{}",
                format!("[{element}").repeat(depth),
                "]".repeat(depth)
            );
            assert_eq!(check_nesting(&source), Err(NestingTooDeep), "{element:?}");
        }
    }

    #[test]
    fn brackets_in_strings_and_comments_are_not_counted() {
        let deep = "[".repeat(MAX_NESTING * 2);
        let source = format!(
            "# {deep}\na = \"{deep}\"\nb = '{deep}'\nc = \"\"\"\n{deep}\n\"\"\"\nd = '''{deep}'''\n"
        );
        assert_eq!(check_nesting(&source), Ok(()));
        assert!(from_str::<toml::Value>(&source).is_ok());
    }

    #[test]
    fn reports_toml_syntax_errors() {
        let err = from_str::<toml::Value>("a = [unquoted").unwrap_err();
        assert!(matches!(err, TomlInputError::Parse(_)), "{err}");
    }
}
