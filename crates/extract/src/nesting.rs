//! Nesting-depth guard that runs before the Oxc parser sees a source.
//!
//! The Oxc parser and the AST visitors recurse once per nesting level and
//! have no depth limit (oxc-project/oxc#21953, oxc-project/oxc#24375). A
//! file with a few thousand nested brackets overflows the stack, and because
//! a stack overflow aborts the process, one such file kills the whole run.
//! Extraction therefore measures the bracket nesting first and does not parse
//! a file past [`MAX_NESTING_DEPTH`].
//!
//! The scan is a lexer approximation. Brackets in strings, comments and
//! template text do not count. A string or regex literal that does not end on
//! its line is not a literal, so the scan reads it again as code: a stray
//! quote in JSX text or a division read as a regex cannot hide brackets. A
//! regex body keeps its unclosed openers open, so a division that looks like
//! a regex cannot hide brackets either. It does not need to be
//! exact, because real code stays far below the cap. Nesting without brackets
//! (TypeScript generics, JSX elements, long member or call chains) is not
//! capped. [`parse_bounded`] runs such a parse on a stack that is large
//! enough for it.

use std::path::Path;

use fallow_types::discover::FileId;
use fallow_types::extract::ModuleInfo;

use crate::visitor::ModuleInfoExtractor;

/// Bracket nesting depth past which a file is not parsed.
///
/// A release build on a 16 MiB worker stack overflows at about 6000 levels for
/// the most expensive shape (nested object literals), and at about half that
/// on the 8 MiB main thread that parses small projects. Real code stays far
/// below this cap.
pub const MAX_NESTING_DEPTH: u32 = 1024;

/// The module of a file whose bracket nesting exceeds [`MAX_NESTING_DEPTH`].
/// The module is a failed parse with no facts, so the file shows up as
/// degraded instead of aborting the run.
pub fn too_deep_module(file_id: FileId, source: &str, content_hash: u64) -> ModuleInfo {
    let source = crate::strip_bom(source);
    let mut info = ModuleInfoExtractor::new().into_module_info(
        file_id,
        content_hash,
        crate::suppress::ParsedSuppressions::default(),
    );
    info.parse_error_count = 1;
    info.parse_panicked = true;
    info.line_offsets = fallow_types::extract::compute_line_offsets(source);
    info
}

/// Returns `true` when the bracket nesting of `source` exceeds
/// [`MAX_NESTING_DEPTH`].
#[must_use]
pub fn exceeds_nesting_limit(source: &str) -> bool {
    max_depth(source, MAX_NESTING_DEPTH, Dialect::Script) > MAX_NESTING_DEPTH
}

/// The lexical rules of the scanned source.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Dialect {
    /// JavaScript, TypeScript and the component formats that embed them.
    Script,
    /// CSS, Sass and Less. A stylesheet has no regex or template literals, and
    /// `//` is not a comment in `url(//host/a.png)`, so only strings and
    /// `/* */` comments are skipped. A Sass `//` comment is scanned as code,
    /// which can only over-count.
    Style,
}

/// Run `parse`, which parses `source` with Oxc and walks the result, without
/// a stack overflow. Returns `None` when the bracket nesting of `source`
/// exceeds [`MAX_NESTING_DEPTH`] or the stack it needs exceeds the cap in
/// `stack.rs`, so the caller treats the file as a failed parse.
///
/// Every parse of user source goes through this function. It sizes the stack
/// from the source length and the bracket depth, and it moves the work to a
/// thread with that stack when the current thread has too little left. A
/// parse of a fragment inside `parse` runs on the same stack and needs no
/// second call.
pub fn parse_bounded<R: Send>(source: &str, parse: impl FnOnce() -> R + Send) -> Option<R> {
    parse_bounded_as(source, Dialect::Script, parse)
}

/// [`parse_bounded`] for a lightningcss parse of stylesheet source. The
/// nesting scan uses stylesheet rules.
pub fn parse_style_bounded<R: Send>(source: &str, parse: impl FnOnce() -> R + Send) -> Option<R> {
    parse_bounded_as(source, Dialect::Style, parse)
}

/// [`parse_bounded`] for the file at `path`. A CSS, Sass or Less file is
/// scanned with stylesheet rules.
pub fn parse_file_bounded<R: Send>(
    path: &Path,
    source: &str,
    parse: impl FnOnce() -> R + Send,
) -> Option<R> {
    let dialect = if crate::css::is_css_file(path) {
        Dialect::Style
    } else {
        Dialect::Script
    };
    parse_bounded_as(source, dialect, parse)
}

fn parse_bounded_as<R: Send>(
    source: &str,
    dialect: Dialect,
    parse: impl FnOnce() -> R + Send,
) -> Option<R> {
    let depth = max_depth(source, MAX_NESTING_DEPTH, dialect);
    if depth > MAX_NESTING_DEPTH {
        return None;
    }
    let need = crate::stack::stack_need(source.len(), depth as usize)?;
    Some(crate::stack::run_with_stack(need, parse))
}

/// Parse `source` with Oxc and call `walk` with the result, through
/// [`parse_bounded`]. Returns `T::default()` when the bracket nesting exceeds
/// [`MAX_NESTING_DEPTH`].
pub fn parse_bounded_or_default<T: Send + Default>(
    source: &str,
    source_type: oxc_span::SourceType,
    walk: impl FnOnce(&oxc_parser::ParserReturn<'_>) -> T + Send,
) -> T {
    parse_bounded(source, || {
        let allocator = oxc_allocator::Allocator::default();
        let parsed = oxc_parser::Parser::new(&allocator, source, source_type).parse();
        walk(&parsed)
    })
    .unwrap_or_default()
}

/// Maximum bracket nesting depth of `source`. The scan stops as soon as the
/// depth passes `limit`, so the result is at most `limit + 1`.
fn max_depth(source: &str, limit: u32, dialect: Dialect) -> u32 {
    let mut scanner = Scanner {
        bytes: source.as_bytes(),
        pos: 0,
        depth: 0,
        max_depth: 0,
        limit,
        dialect,
        templates: Vec::new(),
        regex_allowed: true,
    };
    scanner.scan();
    scanner.max_depth
}

struct Scanner<'a> {
    bytes: &'a [u8],
    pos: usize,
    depth: u32,
    max_depth: u32,
    limit: u32,
    dialect: Dialect,
    /// Depth of each open `${` substitution, innermost last. The `}` that
    /// brings the depth back to this value resumes the template text.
    templates: Vec<u32>,
    /// Whether a `/` at this point starts a regex literal instead of a
    /// division, judged from the previous significant byte.
    regex_allowed: bool,
}

impl Scanner<'_> {
    fn scan(&mut self) {
        while let Some(&byte) = self.bytes.get(self.pos) {
            if self.max_depth > self.limit {
                return;
            }
            self.pos += 1;
            match byte {
                b'\\' if self.dialect == Dialect::Style => self.skip_escape(),
                b'(' if self.dialect == Dialect::Style && self.at_unquoted_url() => {
                    self.skip_unquoted_url();
                }
                b'(' | b'[' | b'{' => {
                    self.open();
                    self.regex_allowed = true;
                }
                b'}' if self.templates.last() == Some(&self.depth) => {
                    self.templates.pop();
                    self.depth -= 1;
                    self.skip_template_text();
                }
                b')' | b']' | b'}' => {
                    self.depth = self.depth.saturating_sub(1);
                    self.regex_allowed = byte == b'}';
                }
                b'\'' | b'"' => {
                    self.skip_string(byte);
                    self.regex_allowed = false;
                }
                b'`' if self.dialect == Dialect::Script => self.skip_template_text(),
                b'/' if self.dialect == Dialect::Style => self.skip_block_comment(),
                b'/' => self.skip_slash(),
                b' ' | b'\t' | b'\n' | b'\r' => {}
                _ => self.regex_allowed = starts_expression(byte),
            }
        }
    }

    fn open(&mut self) {
        self.depth += 1;
        self.max_depth = self.max_depth.max(self.depth);
    }

    /// Skips a comment, a regex literal, or a division operator.
    fn skip_slash(&mut self) {
        match self.bytes.get(self.pos) {
            Some(b'/') => {
                while self.pos < self.bytes.len() && !self.at_line_terminator() {
                    self.pos += 1;
                }
            }
            Some(b'*') => self.skip_block_comment(),
            _ if self.may_start_regex() && self.skip_regex() => {
                self.regex_allowed = false;
            }
            _ => self.regex_allowed = true,
        }
    }

    /// Whether the `(` at `pos - 1` opens an unquoted CSS `url(`, whose body is
    /// raw text: quotes, `/*` and brackets in it are not tokens.
    fn at_unquoted_url(&self) -> bool {
        let open = self.pos - 1;
        let Some(name) = open.checked_sub(3).map(|start| &self.bytes[start..open]) else {
            return false;
        };
        let starts_word = open
            .checked_sub(4)
            .is_none_or(|before| !is_css_name_byte(self.bytes[before]));
        let body = self.bytes[self.pos..].trim_ascii_start();
        name.eq_ignore_ascii_case(b"url")
            && starts_word
            && !matches!(body.first(), Some(b'\'' | b'"'))
    }

    /// Skips an unquoted `url(` body up to its `)`. Quotes and `/*` in it are
    /// raw text. Its brackets still count, as in a regex body: a `(` that only
    /// looks like `url(` must not hide nesting. A CSS escape skips the next
    /// character.
    fn skip_unquoted_url(&mut self) {
        self.open();
        let mut local = 0_u32;
        let mut peak = 0_u32;
        while let Some(&byte) = self.bytes.get(self.pos) {
            self.pos += 1;
            match byte {
                b'\\' => self.skip_escape(),
                b'(' | b'[' | b'{' => {
                    local += 1;
                    peak = peak.max(local);
                }
                b')' if local == 0 => break,
                b')' | b']' | b'}' => local = local.saturating_sub(1),
                _ => {}
            }
        }
        self.max_depth = self.max_depth.max(self.depth + peak);
        self.depth = self.depth - 1 + local;
    }

    /// Skips a `/* */` comment when one starts at the `/` at `pos - 1`.
    fn skip_block_comment(&mut self) {
        if self.bytes.get(self.pos) != Some(&b'*') {
            return;
        }
        self.pos += 1;
        while self.pos < self.bytes.len() && !self.bytes[self.pos..].starts_with(b"*/") {
            self.pos += 1;
        }
        self.pos = (self.pos + 2).min(self.bytes.len());
    }

    /// Skips a string body. A quote without a closing quote on its line is
    /// not a string, so the scan goes on after the quote as code.
    fn skip_string(&mut self, quote: u8) {
        let start = self.pos;
        while let Some(&byte) = self.bytes.get(self.pos) {
            self.pos += 1;
            match byte {
                b'\\' => self.skip_escape(),
                // A string may hold U+2028 and U+2029 since ES2019. A form
                // feed is a newline in CSS.
                b'\n' | b'\r' => break,
                0x0C if self.dialect == Dialect::Style => break,
                _ if byte == quote => return,
                _ => {}
            }
        }
        self.pos = start;
    }

    /// Skips a regex body. A real regex balances its own groups, so its
    /// brackets leave the running depth as it was. A division that only looks
    /// like a regex must not hide brackets, so the body raises the peak depth
    /// and its unclosed openers stay open. A body without a closing `/` on its
    /// line is a division.
    fn skip_regex(&mut self) -> bool {
        let start = self.pos;
        // Depth before the open character class. A class closes at its first
        // `]` and its brackets are literal, so the close restores this depth.
        let mut class_base = None;
        let mut local = 0_u32;
        let mut peak = 0_u32;
        while let Some(&byte) = self.bytes.get(self.pos) {
            if self.at_line_terminator() {
                break;
            }
            self.pos += 1;
            match byte {
                // A regex cannot continue on the next line.
                b'\\' if self.at_line_terminator() => break,
                b'\\' => self.pos += 1,
                b']' if class_base.is_some() => local = class_base.take().unwrap_or(0),
                b'(' | b'[' | b'{' => {
                    if byte == b'[' && class_base.is_none() {
                        class_base = Some(local);
                    }
                    local += 1;
                    peak = peak.max(local);
                }
                b')' | b']' | b'}' => local = local.saturating_sub(1),
                b'/' if class_base.is_none() => {
                    self.max_depth = self.max_depth.max(self.depth + peak);
                    self.depth += local;
                    return true;
                }
                _ => {}
            }
        }
        self.pos = start;
        false
    }

    /// Skips template text up to the closing backtick or the next `${`,
    /// which opens one nesting level.
    fn skip_template_text(&mut self) {
        while let Some(&byte) = self.bytes.get(self.pos) {
            self.pos += 1;
            match byte {
                b'\\' => self.skip_escape(),
                b'`' => {
                    self.regex_allowed = false;
                    return;
                }
                b'$' if self.bytes.get(self.pos) == Some(&b'{') => {
                    self.pos += 1;
                    self.open();
                    self.templates.push(self.depth);
                    self.regex_allowed = true;
                    return;
                }
                _ => {}
            }
        }
    }
}

/// Whether `byte` can be part of a CSS identifier, so `myurl(` is not `url(`.
const fn is_css_name_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_') || byte >= 0x80
}

/// Keywords after which a `/` starts a regex literal.
const REGEX_KEYWORDS: &[&[u8]] = &[
    b"await",
    b"case",
    b"delete",
    b"do",
    b"else",
    b"in",
    b"instanceof",
    b"new",
    b"of",
    b"return",
    b"throw",
    b"typeof",
    b"void",
    b"yield",
];

impl Scanner<'_> {
    /// Whether the `/` at `pos - 1` can start a regex literal. In `</` and
    /// `/>` it is part of a JSX tag.
    fn may_start_regex(&self) -> bool {
        let jsx_tag = self.pos >= 2 && self.bytes[self.pos - 2] == b'<'
            || self.bytes.get(self.pos) == Some(&b'>');
        !jsx_tag && (self.regex_allowed || self.after_keyword())
    }

    /// Whether an ECMAScript line terminator starts at `pos`: LF, CR, U+2028
    /// or U+2029. Oxc ends a comment or a regex at each of them.
    fn at_line_terminator(&self) -> bool {
        self.line_terminator_len() > 0
    }

    /// Byte length of the line terminator at `pos`, or 0. CR LF is one
    /// terminator.
    fn line_terminator_len(&self) -> usize {
        match self.bytes.get(self.pos..) {
            Some([b'\r', b'\n', ..]) => 2,
            Some([b'\n' | b'\r', ..]) => 1,
            Some([0xE2, 0x80, 0xA8 | 0xA9, ..]) => 3,
            _ => 0,
        }
    }

    /// Steps over the character after a backslash at `pos - 1`. A backslash
    /// before a line terminator continues a string or template on the next
    /// line, so the whole terminator is skipped.
    fn skip_escape(&mut self) {
        self.pos += self.line_terminator_len().max(1);
    }

    /// Whether the word before the `/` at `pos - 1` is a keyword that a regex
    /// literal can follow.
    fn after_keyword(&self) -> bool {
        let before = &self.bytes[..self.pos - 1];
        let before = before.trim_ascii_end();
        let start = before
            .iter()
            .rposition(|byte| !(byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'$')))
            .map_or(0, |index| index + 1);
        REGEX_KEYWORDS.contains(&&before[start..])
    }
}

/// Whether a `/` after `byte` starts a regex literal. After an identifier,
/// a number or a closing bracket it is a division.
const fn starts_expression(byte: u8) -> bool {
    !(byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'$' | b'.' | b')' | b']'))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn depth_of(source: &str) -> u32 {
        max_depth(source, u32::MAX, Dialect::Script)
    }

    fn style_depth_of(source: &str) -> u32 {
        max_depth(source, u32::MAX, Dialect::Style)
    }

    #[test]
    fn style_sources_skip_strings_and_block_comments_only() {
        assert_eq!(style_depth_of(".a{content:\"(((\"}/* [[[ */"), 1);
        assert_eq!(
            style_depth_of(".b{background:url(//cdn.example.com/a.png)}.c{a:b(c(d))}"),
            3
        );
        assert_eq!(style_depth_of(".d{width:calc(100%/3)}"), 2);
        assert_eq!(
            style_depth_of(".e::before{content:'it\\'s'}.f{g:h((i))}"),
            3
        );
        // A CSS escape is not a quote, and an unquoted url() body is raw text.
        assert_eq!(style_depth_of(".g\\\"{a:b((c))}"), 3);
        assert_eq!(style_depth_of(".h{a:url(/*x.png)}.i{a:b((c))}"), 3);
        assert_eq!(style_depth_of(".j{a:url(it's.png)}.k{a:b((c))}"), 3);
        assert_eq!(style_depth_of(".l{a:URL( \"(((\" )}"), 2);
        assert_eq!(style_depth_of(".m{a:myurl(((b)))}"), 4);
        // A url() body still counts its brackets, so a misread url( hides
        // nothing.
        assert_eq!(style_depth_of("#url({{{{"), 5);
        assert_eq!(style_depth_of(".n{a:\"x\u{c}((((1"), 5);
    }

    #[test]
    fn counts_code_brackets_only() {
        assert_eq!(depth_of("f(a[b{c}])"), 3);
        assert_eq!(
            depth_of("'((('; \"[[[\"; `{{{`; /\\(\\(/; // (((\n/* ((( */"),
            0
        );
        assert_eq!(depth_of("`a${b(`c${d}`)}`"), 3);
        assert_eq!(depth_of("a / (b) / c"), 1);
        assert_eq!(depth_of("x = /[/(]/ + (1)"), 2);
        assert_eq!(depth_of("return /[(]/.test(a)"), 2);
    }

    #[test]
    fn misread_literals_do_not_hide_brackets() {
        assert_eq!(depth_of("x = i++ / (((1"), 3);
        assert_eq!(depth_of("x = i++ / (((1 / 2)))"), 3);
        assert_eq!(depth_of("<p>it's {{{a}}}</p>"), 3);
        let lines = "x = i++ / ((1 / 2\n".repeat(500);
        assert_eq!(depth_of(&lines), 1000);
        for terminator in ["\r", "\u{2028}", "\u{2029}"] {
            assert_eq!(depth_of(&format!("// a{terminator}(((1")), 3);
            assert_eq!(depth_of(&format!("x = i++ / a{terminator}(((1")), 3);
        }
        assert_eq!(depth_of("'a\rb' + (((1"), 3);
        assert_eq!(depth_of("((('a\\\r\n)))' + (((1"), 6);
        assert_eq!(depth_of("((('a\\\u{2028})))' + (((1"), 6);
        assert_eq!(depth_of("x = i++ /a\\\n(((1 /"), 3);
    }

    #[test]
    fn regex_character_classes_do_not_accumulate() {
        let regex = format!("x = /{}/;\n", "[a-z(][^)]".repeat(2000));
        assert_eq!(depth_of(&regex), 2);
    }

    #[test]
    fn compact_jsx_closers_keep_the_depth_balanced() {
        let line = "<ul>{xs.map(i => (<li>{i}</li>))}</ul>\n{xs.map(i => <Item {...i} />)}\n";
        assert_eq!(depth_of(line), 4);
        assert_eq!(depth_of(&line.repeat(2000)), 4);
    }

    #[test]
    fn depth_at_the_cap_is_allowed() {
        let at_limit = "[".repeat(MAX_NESTING_DEPTH as usize);
        assert!(!exceeds_nesting_limit(&at_limit));
        assert!(exceeds_nesting_limit(&format!("{at_limit}[")));
    }
}
