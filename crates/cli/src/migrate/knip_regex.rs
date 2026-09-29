//! Conversion of knip `ignoreDependencies` regexes to fallow globs.
//!
//! knip turns a string entry into a `RegExp` when it contains one of
//! `* + \ ( | { ^ $`, and matches it unanchored against the package name.
//! fallow accepts globs, so a regex converts only when a glob matches exactly
//! the same package names. Every other regex stays unconverted.

/// Characters that make knip read a string entry as a regex.
const KNIP_REGEX_LIKE: [char; 8] = ['*', '+', '\\', '(', '|', '{', '^', '$'];

/// Literal characters that a converted glob can contain without a change in
/// meaning. npm package names use only these characters.
const fn is_plain_name_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '/' | '@' | '~')
}

/// One parsed element of a supported regex body.
#[derive(Debug, PartialEq, Eq)]
enum Token {
    Literal(char),
    /// `.*`: any run of characters, also an empty one.
    AnyRun,
    /// `.+`: a run of at least one character.
    NonEmptyRun,
    /// `.`: exactly one character.
    AnyChar,
}

/// The regex source of a knip `ignoreDependencies` entry, or `None` when knip
/// reads the entry as an exact package name.
///
/// Both the `/body/` form and the bare knip form (`@org/.+`) are accepted.
pub(super) fn knip_regex_source(entry: &str) -> Option<&str> {
    if entry.len() >= 2 && entry.starts_with('/') && entry.ends_with('/') {
        return Some(&entry[1..entry.len() - 1]);
    }
    entry.contains(KNIP_REGEX_LIKE).then_some(entry)
}

/// Convert a knip regex source to a glob that matches exactly the same npm
/// package names, or `None` when no such glob exists.
pub(super) fn regex_to_exact_glob(source: &str) -> Option<String> {
    let (anchored_start, body) = source
        .strip_prefix('^')
        .map_or((false, source), |rest| (true, rest));
    let (anchored_end, body) = match body.strip_suffix('$') {
        Some(rest) if !rest.ends_with('\\') => (true, rest),
        _ => (false, body),
    };
    let tokens = tokenize(body)?;
    if tokens.is_empty() {
        return None;
    }

    let mut glob = String::new();
    // An unanchored regex also matches in the middle of a name. A leading `@`
    // can only be the first character of a scoped name, so no prefix is needed.
    if !anchored_start && tokens.first() != Some(&Token::Literal('@')) {
        glob.push('*');
    }
    for (index, token) in tokens.iter().enumerate() {
        match token {
            Token::Literal(c) => glob.push(*c),
            Token::AnyRun => glob.push('*'),
            Token::AnyChar => glob.push('?'),
            Token::NonEmptyRun => {
                // `.+` equals `*` only after a trailing scope slash, because a
                // package name never ends with `/`.
                let is_last = index + 1 == tokens.len();
                let after_slash = index > 0 && tokens[index - 1] == Token::Literal('/');
                if !(is_last && after_slash) {
                    return None;
                }
                glob.push('*');
            }
        }
    }
    if !anchored_end {
        glob.push('*');
    }
    Some(collapse_stars(&glob))
}

fn tokenize(body: &str) -> Option<Vec<Token>> {
    let mut tokens = Vec::new();
    let mut chars = body.chars().peekable();
    while let Some(c) = chars.next() {
        let token = match c {
            '\\' => {
                let escaped = chars.next()?;
                if !is_plain_name_char(escaped) || escaped.is_ascii_alphanumeric() {
                    // `\d`, `\w` and similar are classes, not literals.
                    return None;
                }
                Token::Literal(escaped)
            }
            '.' => match chars.peek() {
                Some('*') => {
                    chars.next();
                    Token::AnyRun
                }
                Some('+') => {
                    chars.next();
                    Token::NonEmptyRun
                }
                Some('?' | '{') => return None,
                _ => Token::AnyChar,
            },
            c if is_plain_name_char(c) => {
                if matches!(chars.peek(), Some('*' | '+' | '?' | '{')) {
                    // A quantifier on a literal has no glob form.
                    return None;
                }
                Token::Literal(c)
            }
            _ => return None,
        };
        tokens.push(token);
    }
    Some(tokens)
}

fn collapse_stars(glob: &str) -> String {
    let mut out = String::with_capacity(glob.len());
    for c in glob.chars() {
        if c == '*' && out.ends_with('*') {
            continue;
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn convert(entry: &str) -> Option<String> {
        regex_to_exact_glob(knip_regex_source(entry)?)
    }

    #[test]
    fn plain_names_are_not_regexes() {
        assert_eq!(knip_regex_source("lodash"), None);
        assert_eq!(knip_regex_source("@org/lib"), None);
        assert_eq!(knip_regex_source("lodash.get"), None);
    }

    #[test]
    fn scope_regexes_convert_to_scope_globs() {
        assert_eq!(convert("@org/.+").as_deref(), Some("@org/*"));
        assert_eq!(convert("^@org/.+$").as_deref(), Some("@org/*"));
        assert_eq!(convert("^@org/.*").as_deref(), Some("@org/*"));
        assert_eq!(convert("/^@org\\//").as_deref(), Some("@org/*"));
        // The regex literal `/^@org/` also matches `@organization/lib`.
        assert_eq!(convert("/^@org/").as_deref(), Some("@org*"));
        assert_eq!(convert("^@org\\/").as_deref(), Some("@org/*"));
    }

    #[test]
    fn prefix_and_suffix_regexes_convert() {
        assert_eq!(
            convert("^eslint-plugin-.*$").as_deref(),
            Some("eslint-plugin-*")
        );
        assert_eq!(convert("^@types/").as_deref(), Some("@types/*"));
        assert_eq!(convert("-loader$").as_deref(), Some("*-loader"));
        assert_eq!(convert("^react$").as_deref(), Some("react"));
        assert_eq!(convert("^lodash\\.get$").as_deref(), Some("lodash.get"));
        assert_eq!(convert("^pkg-.$").as_deref(), Some("pkg-?"));
    }

    #[test]
    fn regexes_without_an_exact_glob_are_rejected() {
        for entry in [
            "^(react|vue)$",
            "^@org/[a-z]+$",
            "^lib\\d$",
            "^react-.+$",
            "^@org/.+-plugin$",
            "^colou?r$",
            "^a+$",
            "/^@org//i",
            "^$",
        ] {
            assert_eq!(convert(entry), None, "{entry} must not convert");
        }
    }
}
