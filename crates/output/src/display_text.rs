//! Display forms of project strings for line-oriented text output.
//!
//! A file path, an import specifier or an export name comes from the analyzed
//! project and can hold any character. In a text format or on stderr, a line
//! break in such a value starts a new output line, and a CI runner reads some
//! line prefixes (for example `::warning`) as commands. A terminal escape
//! sequence can also move the cursor or change the window title. The helpers
//! here keep each value on its line and keep it readable: a control character
//! becomes a visible escape such as `\n` or `\u{001B}`.

use std::borrow::Cow;
use std::fmt::Write as _;

/// Whether `ch` must not reach a text line as it is: a control character
/// other than a tab, or a Unicode line or paragraph separator. A tab cannot
/// end a line, and source text often holds one.
fn needs_escape(ch: char) -> bool {
    (ch.is_control() && ch != '\t') || matches!(ch, '\u{2028}' | '\u{2029}')
}

fn push_escaped(out: &mut String, ch: char) {
    match ch {
        '\n' => out.push_str("\\n"),
        '\r' => out.push_str("\\r"),
        _ => {
            let _ = write!(out, "\\u{{{:04X}}}", u32::from(ch));
        }
    }
}

/// A project string for a text line: each control character other than a tab,
/// and each line or paragraph separator, becomes a visible escape (`\n`, `\r`
/// or `\u{XXXX}`). A string without such a character is returned unchanged.
///
/// Use this for a value from the analyzed project in the human, compact and
/// Markdown formats and on stderr. JSON output keeps the raw value, because
/// JSON escapes it already.
#[must_use]
pub fn display_text(value: &str) -> Cow<'_, str> {
    if !value.chars().any(needs_escape) {
        return Cow::Borrowed(value);
    }
    let mut out = String::with_capacity(value.len() + 8);
    for ch in value.chars() {
        if needs_escape(ch) {
            push_escaped(&mut out, ch);
        } else {
            out.push(ch);
        }
    }
    Cow::Owned(out)
}

/// The length in bytes of the color sequence (`ESC [ <digits and ;> m`) at
/// the start of `rest`, or `None` when `rest` does not start with one.
fn color_sequence_len(rest: &str) -> Option<usize> {
    let params = rest.strip_prefix("\u{1B}[")?;
    let end = params.find(|ch: char| !(ch.is_ascii_digit() || ch == ';'))?;
    params[end..].starts_with('m').then_some(2 + end + 1)
}

/// A rendered text line whose values come from the analyzed project: the
/// [`display_text`] escapes for every control character, except a color
/// sequence (`ESC [ <digits and ;> m`). The terminal renderer adds such
/// sequences for colors, and they cannot move the cursor or start a line.
///
/// Use this where a renderer builds a colored line first and the values in it
/// are not escaped one by one.
#[must_use]
pub fn display_line(line: &str) -> Cow<'_, str> {
    if !line.chars().any(needs_escape) {
        return Cow::Borrowed(line);
    }
    let mut out = String::with_capacity(line.len() + 8);
    let mut rest = line;
    while let Some(ch) = rest.chars().next() {
        if let Some(len) = color_sequence_len(rest) {
            out.push_str(&rest[..len]);
            rest = &rest[len..];
            continue;
        }
        if needs_escape(ch) {
            push_escaped(&mut out, ch);
        } else {
            out.push(ch);
        }
        rest = &rest[ch.len_utf8()..];
    }
    Cow::Owned(out)
}

/// `value` as one POSIX shell word: unchanged when every character is plain,
/// else in single quotes with each `'` written as `'\''`.
///
/// Returns `None` for a value with a control character or a line separator:
/// such a value cannot stay on one command line, so a caller skips the
/// command.
#[must_use]
pub fn shell_word(value: &str) -> Option<Cow<'_, str>> {
    if value.chars().any(needs_escape) {
        return None;
    }
    let plain = !value.is_empty()
        && value.chars().all(|ch| {
            ch.is_ascii_alphanumeric()
                || matches!(
                    ch,
                    '_' | '.' | '/' | '-' | '@' | '+' | ':' | ',' | '=' | '%'
                )
        });
    if plain {
        Some(Cow::Borrowed(value))
    } else {
        Some(Cow::Owned(format!("'{}'", value.replace('\'', r"'\''"))))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_text_is_borrowed() {
        assert!(matches!(display_text("src/a b.ts"), Cow::Borrowed(_)));
        assert!(matches!(display_line("src/a.ts"), Cow::Borrowed(_)));
    }

    #[test]
    fn control_characters_become_visible_escapes() {
        assert_eq!(
            display_text("a\nb\rc\td\u{1B}]0;t\u{7}e\u{2028}f\u{85}"),
            "a\\nb\\rc\td\\u{001B}]0;t\\u{0007}e\\u{2028}f\\u{0085}"
        );
    }

    #[test]
    fn non_ascii_text_passes_through() {
        assert_eq!(display_text("src/naïve/日本.ts"), "src/naïve/日本.ts");
    }

    #[test]
    fn a_line_keeps_color_sequences_and_escapes_other_controls() {
        let line = "\u{1B}[1m./helper\n::notice\u{1B}[0m \u{1B}[2J";
        assert_eq!(
            display_line(line),
            "\u{1B}[1m./helper\\n::notice\u{1B}[0m \\u{001B}[2J"
        );
    }

    #[test]
    fn an_unterminated_escape_at_the_end_is_escaped() {
        assert_eq!(display_line("a\u{1B}[1"), r"a\u{001B}[1");
        assert_eq!(display_line("a\u{1B}"), r"a\u{001B}");
    }

    #[test]
    fn shell_word_keeps_a_plain_value() {
        assert_eq!(
            shell_word("src/lib.js:unusedHelper").as_deref(),
            Some("src/lib.js:unusedHelper")
        );
    }

    #[test]
    fn shell_word_quotes_shell_syntax() {
        assert_eq!(
            shell_word("src/(auth)/a b.ts:x; touch f #").as_deref(),
            Some("'src/(auth)/a b.ts:x; touch f #'")
        );
        assert_eq!(shell_word("it's").as_deref(), Some(r"'it'\''s'"));
        assert_eq!(shell_word("$(id)").as_deref(), Some("'$(id)'"));
        assert_eq!(shell_word("").as_deref(), Some("''"));
    }

    #[test]
    fn shell_word_refuses_a_line_break() {
        assert_eq!(shell_word("a\nb"), None);
        assert_eq!(shell_word("a\u{2028}b"), None);
        assert_eq!(shell_word("a\tb").as_deref(), Some("'a\tb'"));
    }
}
