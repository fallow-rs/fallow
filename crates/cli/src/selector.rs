//! The shared `FILE:SYMBOL` selector parser.
//!
//! `inspect --symbol`, `trace <target>`, `check --trace` and
//! `check --symbol-impact` all address a symbol the same way. A split per
//! command lets the emptiness guard drift between them. This module owns the single answer
//! so a selector accepted by one command is accepted by every other.

/// Split a `FILE:SYMBOL` selector into its two halves.
///
/// The symbol is everything after the LAST `:`, so Windows drive letters
/// (`C:\src\utils.ts:foo`) and workspace-qualified paths
/// (`packages/core:src/index.ts:foo`) keep their colons. Returns `None` when
/// there is no colon or when either half is empty or whitespace-only.
///
/// Surviving halves come back verbatim: the trim is a guard, not a
/// normalisation, because a path is matched against the discovered file set and
/// must not be silently rewritten here.
pub fn parse_file_symbol_selector(selector: &str) -> Option<(&str, &str)> {
    let (file, symbol) = selector.rsplit_once(':')?;
    if file.trim().is_empty() || symbol.trim().is_empty() {
        return None;
    }
    Some((file, symbol))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_on_the_last_colon() {
        assert_eq!(
            parse_file_symbol_selector("src/utils.ts:formatDate"),
            Some(("src/utils.ts", "formatDate"))
        );
        assert_eq!(
            parse_file_symbol_selector("src/component.tsx:default"),
            Some(("src/component.tsx", "default"))
        );
    }

    #[test]
    fn keeps_colons_that_belong_to_the_path() {
        assert_eq!(
            parse_file_symbol_selector("C:\\src\\utils.ts:foo"),
            Some(("C:\\src\\utils.ts", "foo"))
        );
        assert_eq!(
            parse_file_symbol_selector("C:/proj/src/a.ts:foo"),
            Some(("C:/proj/src/a.ts", "foo"))
        );
        assert_eq!(
            parse_file_symbol_selector("packages/core:src/index.ts:myExport"),
            Some(("packages/core:src/index.ts", "myExport"))
        );
    }

    #[test]
    fn rejects_a_selector_without_a_colon() {
        assert_eq!(parse_file_symbol_selector("src/utils.ts"), None);
        assert_eq!(parse_file_symbol_selector(""), None);
    }

    #[test]
    fn rejects_empty_and_whitespace_only_halves() {
        assert_eq!(parse_file_symbol_selector(":"), None);
        assert_eq!(parse_file_symbol_selector("src/utils.ts:"), None);
        assert_eq!(parse_file_symbol_selector(":foo"), None);
        assert_eq!(parse_file_symbol_selector(" : "), None);
        assert_eq!(parse_file_symbol_selector("src/utils.ts:\t"), None);
        assert_eq!(parse_file_symbol_selector("\t:foo"), None);
    }

    #[test]
    fn returns_surviving_halves_verbatim() {
        assert_eq!(
            parse_file_symbol_selector(" src/utils.ts : foo "),
            Some((" src/utils.ts ", " foo "))
        );
    }
}
