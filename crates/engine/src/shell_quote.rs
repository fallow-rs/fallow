//! Quote words for a command line that a person or an agent copies and runs.
//!
//! Paths in such a command come from the project config or the environment. A
//! path must stay one argument and must never add a command of its own.

/// Quote `word` for the shell of this platform: a POSIX shell on Unix, and
/// `cmd` or PowerShell on Windows. A plain word stays as it is.
#[must_use]
pub fn shell_quote(word: &str) -> String {
    if cfg!(windows) {
        windows_quote(word)
    } else {
        posix_quote(word)
    }
}

/// Single quotes, with an inner `'` written as `'\''`. A POSIX shell expands
/// nothing inside single quotes.
#[must_use]
pub fn posix_quote(word: &str) -> String {
    if is_plain(word, &[]) {
        word.to_owned()
    } else {
        format!("'{}'", word.replace('\'', "'\\''"))
    }
}

/// Quote for `cmd` and PowerShell. Backslashes stay single, because Windows
/// paths use them.
///
/// Double quotes keep spaces and `& | < > ;` inside one argument in both
/// shells, so a path such as `C:\Program Files\...` works in either one. Inside
/// double quotes PowerShell still expands `$` and backticks, and `cmd` expands
/// `%` and `!`. A word with one of those characters, or with `"`, gets
/// PowerShell single quotes (an inner `'` doubled), which PowerShell reads
/// literally. `cmd` has no literal quoting, so it can still expand `%VAR%` in
/// such a word.
#[must_use]
pub fn windows_quote(word: &str) -> String {
    if is_plain(word, &['\\', ':']) {
        word.to_owned()
    } else if word.contains(['$', '`', '%', '!', '"']) {
        format!("'{}'", word.replace('\'', "''"))
    } else {
        format!("\"{word}\"")
    }
}

fn is_plain(word: &str, extra: &[char]) -> bool {
    !word.is_empty()
        && word.chars().all(|c| {
            c.is_ascii_alphanumeric()
                || matches!(c, '_' | '-' | '.' | '/' | '@' | '+' | '=')
                || extra.contains(&c)
        })
}

#[cfg(test)]
mod tests {
    use super::{posix_quote, windows_quote};

    #[test]
    fn posix_quote_keeps_plain_words_and_quotes_the_rest() {
        assert_eq!(
            posix_quote("baselines/dead-code.json"),
            "baselines/dead-code.json"
        );
        assert_eq!(posix_quote("--root=/srv/app"), "--root=/srv/app");
        assert_eq!(posix_quote("a b.json"), "'a b.json'");
        assert_eq!(posix_quote("x;rm -rf ~"), "'x;rm -rf ~'");
        assert_eq!(posix_quote("a|b&c"), "'a|b&c'");
        assert_eq!(posix_quote("$(id)"), "'$(id)'");
        assert_eq!(posix_quote("`id`"), "'`id`'");
        assert_eq!(posix_quote("it's"), "'it'\\''s'");
        assert_eq!(posix_quote(""), "''");
    }

    #[test]
    fn windows_quote_keeps_paths_and_quotes_shell_characters() {
        assert_eq!(
            windows_quote(r"C:\tools\fallow-mcp.exe"),
            r"C:\tools\fallow-mcp.exe"
        );
        assert_eq!(
            windows_quote(r"C:\Program Files\nodejs\npx.cmd"),
            r#""C:\Program Files\nodejs\npx.cmd""#
        );
        assert_eq!(windows_quote("a&b"), "\"a&b\"");
        assert_eq!(windows_quote("x;y|z"), "\"x;y|z\"");
        assert_eq!(windows_quote("say \"hi\""), "'say \"hi\"'");
        assert_eq!(windows_quote("$(calc)"), "'$(calc)'");
        assert_eq!(windows_quote("`calc`"), "'`calc`'");
        assert_eq!(windows_quote("%PATH%"), "'%PATH%'");
        assert_eq!(windows_quote("it's $x"), "'it''s $x'");
        assert_eq!(windows_quote(""), "\"\"");
    }
}
