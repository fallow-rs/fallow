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
//!
//! The same scan also counts the `<` tokens that the TypeScript parser can
//! try as nested type-argument lists, because each such level multiplies the
//! parse time. [`MAX_TYPE_ARGUMENT_NESTING`] caps that count, and
//! [`reread_budget`] caps the bytes that the parser reads again after a
//! failed list.
//!
//! In a JSX file the parser reads the text of an element as text, so quotes,
//! comment openers and closing brackets there are not tokens. A plain scan
//! cannot tell such text from code. Each script source is therefore scanned
//! twice: once as code and once with JSX elements, and a source is refused
//! when either view passes a limit. A `.ts` file can be parsed with JSX after
//! a parse without JSX finds nothing, so both views apply to every source.
//!
//! A fragment that the extraction parses on its own, such as a template
//! expression or a component script block, gets its own scan through
//! [`fragment_within_limits`], because the scan of the whole file reads it in
//! another context.

use std::cell::Cell;
use std::path::Path;

use fallow_types::discover::FileId;
use fallow_types::extract::ModuleInfo;

use crate::visitor::ModuleInfoExtractor;

#[path = "nesting_jsx.rs"]
mod nesting_jsx;

/// Nesting depth of possible type-argument lists past which a file is not
/// parsed.
///
/// In TypeScript, the parser reads `a<` in an expression first as the start
/// of a type-argument list, and reads it again as a comparison when that
/// fails. A type can hold an expression (a computed key in `{ [key]: T }`),
/// so each `<` nested in such a list multiplies the work, and a few hundred
/// bytes such as `t<{[t<{[t<{[...` keep the parser busy for minutes. The scan
/// counts a `<` that is still open where the next one opens behind a deeper
/// `(` or `[`. A grouping `(` before `{`, `[` or `(` adds one more level:
/// the parser also tries it as a parameter list, so each `<` of
/// `t<({[t<({[...` costs about three times the level inside it. With that
/// count, one level costs at most about two times the level inside it, and a
/// source at the cap parses in less than a millisecond in a release build.
/// Real code, minified bundles and generated declaration files included,
/// stays below this cap.
const MAX_TYPE_ARGUMENT_NESTING: u32 = 10;

/// Bytes that the parser may read again after failed type-argument lists in
/// any source, see [`reread_budget`].
const BASE_TYPE_ARGUMENT_REREADS: u64 = 256 * 1024;

/// Bytes per source byte that the parser may read again after failed
/// type-argument lists, see [`reread_budget`].
const TYPE_ARGUMENT_REREADS_PER_BYTE: u64 = 16;

/// The number of bytes that the parser may read again after failed
/// type-argument lists before a source of `len` bytes is not parsed.
///
/// The parser reads each `a<` of a flat chain such as `a<a<a<...<a;` as a
/// list that holds the rest of the chain, fails at the `;`, and reads the
/// rest again from the next `<`. The nodes of each failed attempt stay
/// allocated until the parse ends, so the memory grows with the square of the
/// chain: 16 KB of such a chain take about 2 GB. The scan adds each byte once
/// for each list that is open there, and drops the bytes of a list that the
/// parser keeps, see [`Scanner::close_type_arguments`]. The budget allows a
/// fixed amount plus a small multiple of the source, so the extra work stays
/// linear. A list can end long after the bytes that it counted, so the scan
/// compares the sum with the budget only at its end.
const fn reread_budget(len: usize) -> u64 {
    BASE_TYPE_ARGUMENT_REREADS.saturating_add(TYPE_ARGUMENT_REREADS_PER_BYTE * len as u64)
}

thread_local! {
    /// Whether [`fragment_within_limits`] refused a fragment on this thread
    /// since the last [`take_refused_fragment`].
    static REFUSED_FRAGMENT: Cell<bool> = const { Cell::new(false) };
}

/// Whether a fragment that the extraction parses on its own, such as a
/// template expression or a script block, is within the nesting limits.
/// A refusal is recorded for `take_refused_fragment`, so the parse of the
/// whole file can be refused too.
pub fn fragment_within_limits(fragment: &str) -> bool {
    let within = !scan_nesting(fragment, Dialect::Script).exceeds_limits();
    if !within {
        REFUSED_FRAGMENT.with(|refused| refused.set(true));
    }
    within
}

/// Returns and clears whether [`fragment_within_limits`] refused a fragment
/// on this thread.
pub fn take_refused_fragment() -> bool {
    REFUSED_FRAGMENT.with(|refused| refused.replace(false))
}

/// Bracket nesting depth past which a file is not parsed.
///
/// A release build on a 16 MiB worker stack overflows at about 6000 levels for
/// the most expensive shape (nested object literals), and at about half that
/// on the 8 MiB main thread that parses small projects. Real code stays far
/// below this cap.
pub const MAX_NESTING_DEPTH: u32 = 1024;

/// Nesting depth of CSS color functions past which lightningcss does not parse
/// a stylesheet.
///
/// When the content of `light-dark(`, `rgb(` or `hsl(` is not a valid color,
/// lightningcss parses it a second time as a plain function. Each level of
/// such nesting doubles the parse time. Real stylesheets nest a few color
/// functions at most, for example `color-mix(in srgb, light-dark(rgb(...)))`.
pub const MAX_COLOR_FUNCTION_DEPTH: usize = 8;

/// Estimated extra parse work of one outermost color function past which
/// lightningcss does not parse the stylesheet.
///
/// Each nested color function doubles the parse time of the text inside it,
/// so a byte inside `k` open color functions costs about `2^k`, see
/// [`Scanner::color_cost`]. Real color functions are short, so real
/// stylesheets stay far below this budget, also when they hold many of them.
/// Eight unclosed functions in front of 5 KB of text pass it.
const MAX_COLOR_PARSE_COST: usize = 1 << 20;

/// The CSS functions that lightningcss tries to parse as a color first.
const COLOR_FUNCTIONS: &[&[u8]] = &[
    b"rgb",
    b"rgba",
    b"hsl",
    b"hsla",
    b"hwb",
    b"lab",
    b"lch",
    b"oklab",
    b"oklch",
    b"color",
    b"color-mix",
    b"light-dark",
    b"alpha",
];

/// The module of a file whose bracket nesting exceeds [`MAX_NESTING_DEPTH`].
/// The module is a failed parse with no facts, so the file shows up as
/// degraded instead of aborting the run.
pub fn too_deep_module(file_id: FileId, source: &str, content_hash: u64) -> ModuleInfo {
    let mut info = ModuleInfoExtractor::new().into_module_info(
        file_id,
        content_hash,
        crate::suppress::ParsedSuppressions::default(),
    );
    info.parse_error_count = 1;
    info.parse_panicked = true;
    info.parse_nesting_too_deep = true;
    info.line_offsets = fallow_types::extract::compute_line_offsets(source);
    info
}

/// Returns `true` when the bracket nesting of `source` exceeds
/// [`MAX_NESTING_DEPTH`] or its type-argument nesting exceeds
/// [`MAX_TYPE_ARGUMENT_NESTING`].
#[cfg(test)]
#[must_use]
fn exceeds_nesting_limit(source: &str) -> bool {
    scan_nesting(source, Dialect::Script).exceeds_limits()
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
/// exceeds [`MAX_NESTING_DEPTH`], its type-argument nesting exceeds
/// `MAX_TYPE_ARGUMENT_NESTING` (10), or the stack it needs exceeds the cap in
/// `stack.rs`, so the caller treats the file as a failed parse.
///
/// Every parse of user source goes through this function. It sizes the stack
/// from the source length and the bracket depth, and it moves the work to a
/// thread with that stack when the current thread has too little left. A
/// parse of a fragment inside `parse` runs on the same stack and needs no
/// second call.
pub fn parse_bounded<R: Send>(source: &str, parse: impl FnOnce() -> R + Send) -> Option<R> {
    parse_bounded_as(source, Dialect::Script, false, parse)
}

/// [`parse_bounded`] for a lightningcss parse of stylesheet source. The
/// nesting scan uses stylesheet rules. Returns `None` also when the token
/// walk in `crate::style_tokens` finds the stylesheet unsafe to parse, see
/// `style_parse_is_safe`.
pub fn parse_style_bounded<R: Send>(source: &str, parse: impl FnOnce() -> R + Send) -> Option<R> {
    let scan = scan_depth(source, MAX_NESTING_DEPTH, Dialect::Style);
    if scan.max_colors > MAX_COLOR_FUNCTION_DEPTH || scan.color_cost > MAX_COLOR_PARSE_COST {
        return None;
    }
    run_bounded(source, scan.max_depth, || {
        crate::style_tokens::style_parse_is_safe(source).then(parse)
    })
    .flatten()
}

/// [`parse_bounded`] for the file at `path`. A CSS, Sass or Less file is
/// scanned with stylesheet rules. A markup host gets no reread budget, see
/// [`parse_bounded_for_file`].
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
    parse_bounded_as(source, dialect, is_markup_host(path), parse)
}

/// [`parse_bounded`] for a parse of the file at `path` that splits a markup
/// host into its fragments.
///
/// The text of an HTML, MDX, Vue, Svelte or Astro file is markup, and its
/// prose can hold many bare `<`, such as `the width is < 3 px`. The scan of
/// the whole file reads them as type-argument lists that stay open, so the
/// `reread_budget` does not apply to such a file. Each fragment of it that
/// is parsed gets its own scan with the budget: through [`parse_bounded`],
/// or through [`fragment_within_limits`].
pub fn parse_bounded_for_file<R: Send>(
    path: &Path,
    source: &str,
    parse: impl FnOnce() -> R + Send,
) -> Option<R> {
    parse_bounded_as(source, Dialect::Script, is_markup_host(path), parse)
}

/// Whether `path` is a markup file whose scripts and expressions are
/// fragments, see [`parse_bounded_for_file`].
fn is_markup_host(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "html" | "htm" | "mdx" | "vue" | "svelte" | "astro"
            )
        })
}

fn parse_bounded_as<R: Send>(
    source: &str,
    dialect: Dialect,
    markup_host: bool,
    parse: impl FnOnce() -> R + Send,
) -> Option<R> {
    let mut nesting = scan_nesting(source, dialect);
    if markup_host {
        nesting.rereads_exceeded = false;
    }
    if nesting.exceeds_limits() {
        return None;
    }
    run_bounded(source, nesting.brackets, parse)
}

fn run_bounded<R: Send>(source: &str, depth: u32, parse: impl FnOnce() -> R + Send) -> Option<R> {
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

/// The nesting that the parse of a source has to handle.
struct Nesting {
    /// Maximum bracket nesting depth.
    brackets: u32,
    /// Maximum nesting of `<` that can open a type-argument list.
    type_arguments: u32,
    /// Whether the bytes that the parser can read again after failed
    /// type-argument lists pass [`reread_budget`].
    rereads_exceeded: bool,
    /// Whether the scan saw a `<` where a JSX element can start.
    jsx_tag: bool,
}

impl Nesting {
    const fn exceeds_limits(&self) -> bool {
        self.brackets > MAX_NESTING_DEPTH
            || self.type_arguments > MAX_TYPE_ARGUMENT_NESTING
            || self.rereads_exceeded
    }
}

/// Measures the nesting of `source`. The scan stops as soon as a value passes
/// its limit, so each value is at most its limit + 1. A script source is
/// scanned as code and with JSX elements, and each value is the higher one.
/// The two views read the same bytes in the same way up to the first `<`
/// where a JSX element can start, so the JSX view runs only when the code
/// view saw such a `<`.
fn scan_nesting(source: &str, dialect: Dialect) -> Nesting {
    let code = scan_view(source, dialect, false);
    if dialect != Dialect::Script || code.exceeds_limits() || !code.jsx_tag {
        return code;
    }
    let jsx = scan_view(source, dialect, true);
    Nesting {
        brackets: code.brackets.max(jsx.brackets),
        type_arguments: code.type_arguments.max(jsx.type_arguments),
        rereads_exceeded: jsx.rereads_exceeded,
        jsx_tag: true,
    }
}

fn scan_view(source: &str, dialect: Dialect, jsx: bool) -> Nesting {
    let mut scanner = Scanner::new(source, MAX_NESTING_DEPTH, dialect);
    scanner.jsx = jsx;
    scanner.skip_hashbang();
    scanner.scan();
    Nesting {
        brackets: scanner.max_depth,
        type_arguments: scanner.max_type_arguments,
        rereads_exceeded: scanner.rereads > scanner.reread_budget,
        jsx_tag: scanner.jsx_tag,
    }
}

/// Maximum bracket nesting depth of `source`. The scan stops as soon as the
/// depth passes `limit`, so the result is at most `limit + 1`.
#[cfg(test)]
fn max_depth(source: &str, limit: u32, dialect: Dialect) -> u32 {
    scan_depth(source, limit, dialect).max_depth
}

fn scan_depth(source: &str, limit: u32, dialect: Dialect) -> Scanner<'_> {
    let mut scanner = Scanner::new(source, limit, dialect);
    scanner.scan();
    scanner.end_open_color_functions();
    scanner
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
    /// Each open `<` that can start a type-argument list, outermost first.
    /// No entry is deeper than the one after it.
    type_arguments: Vec<TypeArguments>,
    max_type_arguments: u32,
    /// Depth of each open `(` and `[`, innermost last. Only these brackets
    /// can hold an expression inside a type.
    expression_brackets: Vec<u32>,
    /// Depth of each open `(` that the parser can also try as a parameter
    /// list, innermost last, see [`Scanner::opens_parameter_list`].
    parameter_lists: Vec<u32>,
    /// Depth of each open `(` of an `if`, `while`, `for` or `with` header,
    /// innermost last. A `/` after its `)` starts a regex.
    control_headers: Vec<u32>,
    /// Whether this scanner reads the body of a guessed regex as code, see
    /// [`Scanner::count_regex_type_arguments`].
    regex_body: bool,
    /// Whether this scan reads JSX elements, see [`Scanner::scan_jsx`].
    jsx: bool,
    /// Whether the scan saw a `<` where a JSX element can start.
    jsx_tag: bool,
    /// The elements of the innermost JSX expression that are open.
    jsx_elements: u32,
    /// Each open `{` container of a JSX element, innermost last. The `}` that
    /// brings the depth back to its value resumes the element.
    jsx_containers: Vec<JsxContainer>,
    /// The bytes that the parser can read again after failed type-argument
    /// lists: each byte once for each list that is open there, without the
    /// lists that the parser keeps.
    rereads: u64,
    reread_budget: u64,
    /// For each source byte, the literal guesses that failed after they read
    /// it, as [`GuessState`] bits. Empty until a guess fails.
    failed_guesses: Vec<u8>,
    /// Each byte that the current literal guess read, with its state.
    guess_trail: Vec<(usize, u8)>,
    /// Depth and `(` offset of each open stylesheet color function,
    /// innermost last.
    colors: Vec<(u32, usize)>,
    /// Maximum number of color functions open at one time.
    max_colors: usize,
    /// The highest extra parse work of one outermost color function: the sum
    /// over the color functions nested in it of the byte length of each,
    /// doubled once for each color function around it. A byte inside `k`
    /// nested color functions adds `2^k - 2`.
    color_cost: usize,
    /// The extra parse work of the outermost color function that is open.
    open_color_cost: usize,
    /// The closing byte of each open stylesheet block, innermost last. In CSS
    /// only the matching byte ends a block, so a `}` inside an open `(` is a
    /// plain token.
    style_closers: Vec<u8>,
}

/// An open `<` that can start a type-argument list.
#[derive(Clone, Copy)]
struct TypeArguments {
    /// Bracket depth of the `<`.
    depth: u32,
    /// How many open lists, this one included, nest behind a `(` or `[`.
    level: u32,
    /// The depth and level of the innermost open list at a lower depth.
    outer: Option<(u32, u32)>,
    /// The `extends` of conditional types in this list that wait for their
    /// `?`.
    conditions: u32,
    /// The `?` of conditional types in this list that wait for their `:`.
    branches: u32,
    /// Offset after the `<`.
    start: usize,
    /// [`Scanner::rereads`] at the `<`.
    rereads_at_start: u64,
}

/// The part of a JSX element that a scan reads.
#[derive(Clone, Copy, PartialEq, Eq)]
enum JsxPart {
    /// The attributes of an opening tag.
    Attributes,
    /// The children of an element.
    Children,
}

/// An open `{` container in a JSX element.
#[derive(Clone, Copy)]
struct JsxContainer {
    /// Bracket depth inside the `{`.
    depth: u32,
    /// The part of the element that the `}` resumes.
    part: JsxPart,
    /// The open elements of the JSX expression around the container.
    elements: u32,
}

/// The states of a literal guess, as bits of [`Scanner::failed_guesses`].
struct GuessState;

impl GuessState {
    const SINGLE_QUOTE: u8 = 1;
    const DOUBLE_QUOTE: u8 = 2;
    const REGEX: u8 = 4;
    const REGEX_CLASS: u8 = 8;
}

impl<'a> Scanner<'a> {
    fn new(source: &'a str, limit: u32, dialect: Dialect) -> Self {
        Scanner {
            bytes: source.as_bytes(),
            pos: 0,
            depth: 0,
            max_depth: 0,
            limit,
            dialect,
            templates: Vec::new(),
            regex_allowed: true,
            type_arguments: Vec::new(),
            max_type_arguments: 0,
            expression_brackets: Vec::new(),
            parameter_lists: Vec::new(),
            control_headers: Vec::new(),
            regex_body: false,
            jsx: false,
            jsx_tag: false,
            jsx_elements: 0,
            jsx_containers: Vec::new(),
            rereads: 0,
            reread_budget: reread_budget(source.len()),
            failed_guesses: Vec::new(),
            guess_trail: Vec::new(),
            colors: Vec::new(),
            max_colors: 0,
            color_cost: 0,
            open_color_cost: 0,
            style_closers: Vec::new(),
        }
    }
}

impl Scanner<'_> {
    fn scan(&mut self) {
        while let Some(&byte) = self.bytes.get(self.pos) {
            if self.passed_limit() {
                return;
            }
            let start = self.pos;
            self.pos += 1;
            match byte {
                b'\\' | b'(' | b'/'
                    if self.dialect == Dialect::Style && self.scan_style_byte(byte) => {}
                b'(' | b'[' | b'{' => self.open_bracket(byte),
                b')' | b']' | b'}'
                    if self.dialect == Dialect::Style && !self.close_style_block(byte) => {}
                b'}' if self.resume_after_brace() => {}
                b')' | b']' | b'}' => self.close_bracket(byte),
                b'<' | b'>' | b';' | b'=' | b'&' | b'|' | b'?' | b':' | b'`' | b's'
                    if self.dialect == Dialect::Script =>
                {
                    self.scan_script_byte(byte);
                }
                b'\'' | b'"' => {
                    self.skip_string(byte);
                    self.regex_allowed = false;
                }
                b'/' => self.skip_slash(),
                b' ' | b'\t' | b'\n' | b'\r' => {}
                _ => self.regex_allowed = starts_expression(byte),
            }
            self.count_rereads(start);
        }
    }

    /// Ends the `${` substitution or the JSX `{` container that the `}` at
    /// `pos - 1` closes, and reads on in its template or element. Returns
    /// false for a `}` that closes a plain bracket.
    fn resume_after_brace(&mut self) -> bool {
        if self.templates.last() == Some(&self.depth) {
            self.templates.pop();
            self.depth -= 1;
            self.close_type_arguments_above_depth();
            self.skip_template_text();
            return true;
        }
        if self
            .jsx_containers
            .last()
            .is_some_and(|container| container.depth == self.depth)
        {
            self.close_jsx_container();
            return true;
        }
        false
    }

    /// Closes the bracket that the `byte` at `pos - 1` ends.
    fn close_bracket(&mut self, byte: u8) {
        let header = byte == b')' && self.control_headers.last() == Some(&self.depth);
        self.depth = self.depth.saturating_sub(1);
        self.close_type_arguments_above_depth();
        self.regex_allowed = byte == b'}' || header;
        self.close_color_functions();
    }

    fn open(&mut self) {
        self.depth += 1;
        self.max_depth = self.max_depth.max(self.depth);
    }

    /// Whether a nesting value passed its limit, so the scan can stop.
    const fn passed_limit(&self) -> bool {
        self.max_depth > self.limit || self.max_type_arguments > MAX_TYPE_ARGUMENT_NESTING
    }

    /// Adds the bytes from `start` to `pos` once for each open type-argument
    /// list, because a failed list makes the parser read them again.
    fn count_rereads(&mut self, start: usize) {
        let open = self.type_arguments.len() as u64;
        let read = (self.pos - start) as u64;
        self.rereads = self.rereads.saturating_add(open.saturating_mul(read));
    }

    /// Skips a `#!` line at the start of the source, which the parser reads
    /// as a comment.
    fn skip_hashbang(&mut self) {
        if self.dialect != Dialect::Script || !self.bytes.starts_with(b"#!") {
            return;
        }
        while self.pos < self.bytes.len() && !self.at_line_terminator() {
            self.pos += 1;
        }
    }

    /// Opens a bracket. A `(` or `[` can hold an expression inside a type.
    fn open_bracket(&mut self, byte: u8) {
        let parameter_list = byte == b'(' && self.opens_parameter_list();
        let header = byte == b'(' && self.opens_control_header();
        self.open();
        if byte != b'{' {
            self.expression_brackets.push(self.depth);
        }
        if parameter_list {
            self.parameter_lists.push(self.depth);
        }
        if header {
            self.control_headers.push(self.depth);
        }
        if self.dialect == Dialect::Style {
            self.style_closers.push(match byte {
                b'(' => b')',
                b'[' => b']',
                _ => b'}',
            });
        }
        self.regex_allowed = true;
    }

    /// Ends the innermost stylesheet block when `closer` matches it. Returns
    /// false for a closing byte that CSS reads as a plain token.
    fn close_style_block(&mut self, closer: u8) -> bool {
        if self.style_closers.last() != Some(&closer) {
            return false;
        }
        self.style_closers.pop();
        true
    }

    /// Whether the `(` at `pos - 1` can open a parameter list that holds an
    /// expression. A `(` that does not follow an operand groups an
    /// expression, and the parser also tries it as the parameters of an
    /// arrow function or a function type. A computed key or a default value
    /// in such a pattern holds an expression, so a `{`, `[` or `(` after the
    /// `(` makes the parser read the inner part once more. A `/` can start a
    /// comment before such a pattern, so it counts too. A call such as
    /// `f({` holds no parameters.
    fn opens_parameter_list(&self) -> bool {
        if self.dialect != Dialect::Script || !(self.regex_allowed || self.after_keyword()) {
            return false;
        }
        let rest = self.bytes[self.pos..].trim_ascii_start();
        matches!(rest.first(), Some(b'{' | b'[' | b'(' | b'/'))
    }

    /// Whether the `(` at `pos - 1` opens the header of an `if`, `while`,
    /// `for` or `with` statement. A member such as `a.if(` is a call.
    fn opens_control_header(&self) -> bool {
        if self.dialect != Dialect::Script {
            return false;
        }
        let (word, start) = self.word_before(self.pos - 1);
        let member = self.bytes[..start].trim_ascii_end().last() == Some(&b'.');
        !member && matches!(word, b"if" | b"while" | b"for" | b"with")
    }

    /// Handles the script tokens that stylesheets do not have: a template
    /// literal, the tokens that open or end a type-argument list, and the
    /// last byte of an `extends` keyword.
    fn scan_script_byte(&mut self, byte: u8) {
        match byte {
            b'`' => self.skip_template_text(),
            b's' => {
                self.regex_allowed = false;
                self.note_extends();
            }
            b'<' if (self.jsx || !self.jsx_tag) && self.at_jsx_tag() => {
                self.jsx_tag = true;
                if self.jsx {
                    self.open_jsx_element();
                } else {
                    self.open_type_arguments();
                    self.regex_allowed = true;
                }
            }
            b'<' => {
                self.open_type_arguments();
                self.regex_allowed = true;
            }
            _ => {
                self.end_type_arguments(byte);
                self.regex_allowed = true;
            }
        }
    }

    /// Records the `<` at `pos - 1` as a possible type-argument list. A `<`
    /// at the depth of an open one keeps its level, because the parser
    /// retries it inside the same brackets. A `<` in a deeper bracket nests
    /// one level deeper only behind a `(` or `[` opened after the enclosing
    /// `<`: a type such as `A<{ b: C<D> }>` holds no expression, so the
    /// parser does not retry it. `<=` and `<<=` cannot open a list. The
    /// parser splits `<<` into two `<`, and the second one opens a list
    /// only before the name of a type parameter, so `x << 1` is a shift.
    fn open_type_arguments(&mut self) {
        if self.bytes.get(self.pos) == Some(&b'<') {
            self.pos += 1;
            let rest = self.bytes[self.pos..].trim_ascii_start();
            if !rest.first().copied().is_some_and(is_identifier_start) {
                return;
            }
        }
        if self.bytes.get(self.pos) == Some(&b'=') {
            return;
        }
        let sibling = self
            .type_arguments
            .last()
            .filter(|open| open.depth == self.depth)
            .map_or(0, |open| open.level);
        let outer = self.enclosing_type_arguments(self.depth);
        let level = outer
            .map_or(1, |(depth, level)| {
                let behind =
                    |brackets: &[u32]| brackets.last().is_some_and(|&bracket| bracket > depth);
                level
                    + u32::from(behind(&self.expression_brackets))
                    + u32::from(behind(&self.parameter_lists))
            })
            .max(sibling);
        self.push_type_arguments(self.depth, level);
    }

    /// The depth and level of the innermost open `<` below `depth`. A list
    /// at `depth` stores it, so siblings do not search the stack.
    fn enclosing_type_arguments(&self, depth: u32) -> Option<(u32, u32)> {
        let last = self.type_arguments.last()?;
        if last.depth < depth {
            Some((last.depth, last.level))
        } else {
            last.outer
        }
    }

    fn push_type_arguments(&mut self, depth: u32, level: u32) {
        let outer = self.enclosing_type_arguments(depth);
        self.type_arguments.push(TypeArguments {
            depth,
            level,
            outer,
            conditions: 0,
            branches: 0,
            start: self.pos,
            rereads_at_start: self.rereads,
        });
        self.max_type_arguments = self.max_type_arguments.max(level);
    }

    /// Ends open `<` at the current depth when the `byte` at `pos - 1` closes
    /// a type-argument list or cannot appear in one. The parser stops its
    /// attempt at such a token, so a later `<` is not nested in it.
    ///
    /// - `>` closes the innermost list.
    /// - `;`, `=`, `&&`, `||` and `??` end every list. The arrow `=>` is
    ///   valid in a type and ends nothing, and a single `&` or `|` is valid
    ///   in a type too.
    /// - `?` before the start of a type and `?.` end every list, unless the
    ///   `?` belongs to a conditional type `A extends B ? C : D`. A `?`
    ///   before `,`, `>` or a closer can be a postfix `T?` and ends nothing.
    /// - `:` ends every list, unless it belongs to a conditional type.
    fn end_type_arguments(&mut self, byte: u8) {
        let next = self.bytes.get(self.pos).copied();
        if matches!(byte, b'&' | b'|' | b'?') && next == Some(byte) {
            self.pos += 1;
            self.end_all_type_arguments_at_depth();
            return;
        }
        match byte {
            b'>' if self.pos >= 2 && self.bytes[self.pos - 2] == b'=' => {}
            b'>' => self.close_type_arguments(),
            b'=' if next == Some(b'>') => {}
            b'&' | b'|' => {}
            b'?' => self.end_type_arguments_at_question_mark(),
            b':' => {
                if let Some(open) = self.innermost_type_arguments_at_depth()
                    && open.branches > 0
                {
                    open.branches -= 1;
                } else {
                    self.end_all_type_arguments_at_depth();
                }
            }
            _ => self.end_all_type_arguments_at_depth(),
        }
    }

    /// Closes the innermost open `<` at the current depth at the `>` at
    /// `pos - 1`. When a token that can follow type arguments comes next,
    /// the parser keeps the list and reads its bytes once, also the bytes of
    /// the lists inside it, so they are not read again.
    fn close_type_arguments(&mut self) {
        let Some(open) = self.innermost_type_arguments_at_depth().copied() else {
            return;
        };
        self.type_arguments.pop();
        if !self.type_arguments_may_end_here() {
            return;
        }
        let outer = self.type_arguments.len() as u64;
        let span = (self.pos - open.start) as u64;
        self.rereads = open
            .rereads_at_start
            .saturating_add(outer.saturating_mul(span));
    }

    /// Whether the parser keeps a type-argument list that a `>` at `pos - 1`
    /// closes: the next token is a call, a template, an operator, or a token
    /// that cannot start an expression, or it stands on the next line. A
    /// `>=`, `>`, `<`, `+`, `-` or the start of an operand rejects the list.
    fn type_arguments_may_end_here(&self) -> bool {
        let rest = &self.bytes[self.pos..];
        if rest.first() == Some(&b'=') {
            return false;
        }
        let spaces = rest
            .iter()
            .take_while(|&&byte| matches!(byte, b' ' | b'\t' | 0x0B | 0x0C))
            .count();
        let rest = &rest[spaces..];
        let Some(&next) = rest.first() else {
            return true;
        };
        match next {
            b'\n' | b'\r' | b'(' | b'`' => true,
            b'<' | b'>' | b'+' | b'-' | b'[' | b'{' | b'~' | b'@' | b'#' | b'\'' | b'"' => false,
            b'!' => rest.get(1) == Some(&b'='),
            b'.' => !rest.get(1).is_some_and(u8::is_ascii_digit),
            _ if next.is_ascii_digit() => false,
            _ if is_identifier_start(next) => {
                let word_len = rest
                    .iter()
                    .take_while(|&&byte| is_identifier_byte(byte))
                    .count();
                matches!(
                    &rest[..word_len],
                    b"in" | b"instanceof" | b"as" | b"satisfies"
                )
            }
            _ => true,
        }
    }

    /// Handles a single `?` at `pos - 1`, see [`Scanner::end_type_arguments`].
    fn end_type_arguments_at_question_mark(&mut self) {
        let rest = &self.bytes[self.pos..];
        let optional_chain =
            rest.first() == Some(&b'.') && !rest.get(1).is_some_and(u8::is_ascii_digit);
        if optional_chain {
            self.end_all_type_arguments_at_depth();
            return;
        }
        let postfix = matches!(
            rest.trim_ascii_start().first(),
            None | Some(b',' | b'>' | b')' | b']' | b'}' | b'=' | b';' | b':' | b'?')
        );
        if postfix {
            return;
        }
        if let Some(open) = self.innermost_type_arguments_at_depth()
            && open.conditions > 0
        {
            open.conditions -= 1;
            open.branches += 1;
        } else {
            self.end_all_type_arguments_at_depth();
        }
    }

    /// The innermost open `<` at the current depth.
    fn innermost_type_arguments_at_depth(&mut self) -> Option<&mut TypeArguments> {
        let depth = self.depth;
        self.type_arguments
            .last_mut()
            .filter(|open| open.depth == depth)
    }

    fn end_all_type_arguments_at_depth(&mut self) {
        while self
            .type_arguments
            .last()
            .is_some_and(|open| open.depth == self.depth)
        {
            self.type_arguments.pop();
        }
    }

    /// Records an `extends` keyword that ends at `pos - 1`. In an open list
    /// it starts a conditional type, whose `?` and `:` stay in the list.
    fn note_extends(&mut self) {
        let end = self.pos;
        let keyword = b"extends";
        if !self.bytes[..end].ends_with(keyword)
            || self.bytes.get(end).copied().is_some_and(is_identifier_byte)
            || end
                .checked_sub(keyword.len() + 1)
                .is_some_and(|before| is_identifier_byte(self.bytes[before]))
        {
            return;
        }
        if let Some(open) = self.innermost_type_arguments_at_depth() {
            open.conditions += 1;
        }
    }

    /// A closing bracket ends every `<` and every bracket opened inside it.
    fn close_type_arguments_above_depth(&mut self) {
        while self
            .type_arguments
            .last()
            .is_some_and(|open| open.depth > self.depth)
        {
            self.type_arguments.pop();
        }
        for brackets in [
            &mut self.expression_brackets,
            &mut self.parameter_lists,
            &mut self.control_headers,
        ] {
            while brackets.last().is_some_and(|&bracket| bracket > self.depth) {
                brackets.pop();
            }
        }
    }

    /// Handles the stylesheet tokens that differ from script: an escape, a
    /// `/* */` comment, an unquoted `url(` body and a color function. Returns
    /// false for a `(` that opens a plain bracket.
    fn scan_style_byte(&mut self, byte: u8) -> bool {
        match byte {
            b'\\' => self.skip_escape(),
            b'/' => self.skip_block_comment(),
            _ if self.at_unquoted_url() => self.skip_unquoted_url(),
            _ if self.at_color_function() => {
                self.open();
                self.style_closers.push(b')');
                self.colors.push((self.depth, self.pos - 1));
                self.max_colors = self.max_colors.max(self.colors.len());
            }
            _ => return false,
        }
        true
    }

    /// Ends the color functions that a closing bracket closed.
    fn close_color_functions(&mut self) {
        while self
            .colors
            .last()
            .is_some_and(|(open, _)| *open > self.depth)
        {
            self.end_color_function(self.pos);
        }
    }

    /// Ends the color functions that are still open at the end of the source.
    /// lightningcss reads each of them to the end.
    fn end_open_color_functions(&mut self) {
        while !self.colors.is_empty() {
            self.end_color_function(self.bytes.len());
        }
    }

    /// Ends the innermost color function at the byte offset `end`. A nested
    /// function adds its weighted span to the work of its outermost function,
    /// and an outermost function updates [`Scanner::color_cost`].
    fn end_color_function(&mut self, end: usize) {
        let Some((_, start)) = self.colors.pop() else {
            return;
        };
        let outer = self.colors.len();
        if outer == 0 {
            self.color_cost = self.color_cost.max(self.open_color_cost);
            self.open_color_cost = 0;
            return;
        }
        let span = end.saturating_sub(start);
        let weight = 1usize.checked_shl(outer as u32).unwrap_or(usize::MAX);
        self.open_color_cost = self
            .open_color_cost
            .saturating_add(span.saturating_mul(weight));
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
        // A name byte or an escape before `url` makes a longer name, such as
        // `a\url(` or `\61 url(`, which is a plain function.
        let starts_word = open.checked_sub(4).is_none_or(|before| {
            let byte = self.bytes[before];
            !is_css_name_byte(byte) && byte != b'\\' && self.escape_whitespace_len(before) == 0
        });
        let body = self.bytes[self.pos..].trim_ascii_start();
        name.eq_ignore_ascii_case(b"url")
            && starts_word
            && !matches!(body.first(), Some(b'\'' | b'"'))
    }

    /// Whether the `(` at `pos - 1` opens one of the [`COLOR_FUNCTIONS`]. A
    /// name with a CSS escape such as `r\67 b(` can spell any function, so it
    /// counts as a color function.
    fn at_color_function(&self) -> bool {
        let open = self.pos - 1;
        let mut start = open;
        loop {
            while start > 0 && is_css_name_byte_or_escape(self.bytes[start - 1]) {
                start -= 1;
            }
            // The whitespace after a hex escape belongs to the name.
            match start
                .checked_sub(1)
                .map(|before| self.escape_whitespace_len(before))
            {
                Some(len) if len > 0 => start -= len,
                _ => break,
            }
        }
        let name = &self.bytes[start..open];
        name.contains(&b'\\')
            || COLOR_FUNCTIONS
                .iter()
                .any(|color| name.eq_ignore_ascii_case(color))
    }

    /// The length of the whitespace that ends at `last` when it ends a hex
    /// escape such as `\67 `, else 0. CSS reads `\r\n` as one whitespace, so
    /// it has length 2.
    fn escape_whitespace_len(&self, last: usize) -> usize {
        match self.bytes[last] {
            b'\n'
                if last > 0 && self.bytes[last - 1] == b'\r' && self.ends_hex_escape(last - 1) =>
            {
                2
            }
            b' ' | b'\t' | b'\n' | b'\r' | 0x0C if self.ends_hex_escape(last) => 1,
            _ => 0,
        }
    }

    /// Whether a hex escape such as `\67` ends right before `end`.
    fn ends_hex_escape(&self, end: usize) -> bool {
        let digits = self.bytes[..end]
            .iter()
            .rev()
            .take(MAX_HEX_ESCAPE_DIGITS + 1)
            .take_while(|byte| byte.is_ascii_hexdigit())
            .count();
        (1..=MAX_HEX_ESCAPE_DIGITS).contains(&digits)
            && end
                .checked_sub(digits + 1)
                .is_some_and(|slash| self.bytes[slash] == b'\\')
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
        // A body that reaches the end of the source leaves its brackets open.
        self.style_closers
            .extend(std::iter::repeat_n(b')', local as usize));
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

    /// Skips a string body. In script, a quote without a closing quote on its
    /// line is not a string, so the scan goes on after the quote as code. In
    /// a stylesheet such a quote starts a bad string, which ends before the
    /// newline, as in the CSS tokenizer.
    fn skip_string(&mut self, quote: u8) {
        let start = self.pos;
        let state = if quote == b'\'' {
            GuessState::SINGLE_QUOTE
        } else {
            GuessState::DOUBLE_QUOTE
        };
        self.guess_trail.clear();
        while let Some(&byte) = self.bytes.get(self.pos) {
            crate::cost::walk_step();
            if self.dialect == Dialect::Script {
                if self.guess_failed_here(state) {
                    break;
                }
                self.guess_trail.push((self.pos, state));
            }
            self.pos += 1;
            match byte {
                b'\\' if self.dialect == Dialect::Style => self.skip_style_escape(),
                b'\\' => self.skip_escape(),
                // A string may hold U+2028 and U+2029 since ES2019. A form
                // feed is a newline in CSS.
                b'\n' | b'\r' | 0x0C if self.dialect == Dialect::Style => {
                    self.pos -= 1;
                    return;
                }
                b'\n' | b'\r' => break,
                _ if byte == quote => return,
                _ => {}
            }
        }
        if self.dialect == Dialect::Script {
            self.pos = start;
            self.record_failed_guess();
        }
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
        self.guess_trail.clear();
        while let Some(&byte) = self.bytes.get(self.pos) {
            crate::cost::walk_step();
            let state = if class_base.is_some() {
                GuessState::REGEX_CLASS
            } else {
                GuessState::REGEX
            };
            if self.at_line_terminator() || self.guess_failed_here(state) {
                break;
            }
            self.guess_trail.push((self.pos, state));
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
                    let base = self.depth;
                    self.depth += local;
                    self.count_regex_type_arguments(start, self.pos - 1, base);
                    return true;
                }
                _ => {}
            }
        }
        self.pos = start;
        self.record_failed_guess();
        false
    }

    /// Whether a literal guess that failed read the byte at `pos` in `state`.
    /// The rest of a guess depends only on its position and state, so a new
    /// guess that reaches the same byte in the same state fails too.
    fn guess_failed_here(&self, state: u8) -> bool {
        self.failed_guesses
            .get(self.pos)
            .is_some_and(|states| states & state != 0)
    }

    /// Records the bytes in [`Scanner::guess_trail`] for the guess that
    /// failed.
    fn record_failed_guess(&mut self) {
        if self.guess_trail.is_empty() {
            return;
        }
        if self.failed_guesses.is_empty() {
            self.failed_guesses = vec![0; self.bytes.len()];
        }
        for &(pos, state) in &self.guess_trail {
            self.failed_guesses[pos] |= state;
        }
    }

    /// Counts the `<` of the regex body `start..end` as type arguments. The
    /// parser can read the `/` as a division, so the body can be code whose
    /// type arguments nest. The body is scanned as code from bracket depth
    /// `base`, its nesting adds to the level that is open here, and the lists
    /// that stay open at its end stay open after the `/`. Inside such a scan,
    /// a further regex body counts each of its `<` as one more open level, so
    /// the work stays linear.
    fn count_regex_type_arguments(&mut self, start: usize, end: usize, base: u32) {
        let body = &self.bytes[start..end];
        if !body.contains(&b'<') {
            return;
        }
        let open = self.type_arguments.last().map_or(0, |outer| outer.level);
        if self.regex_body {
            let count = body.iter().fold(0_u32, |count, &byte| {
                count.saturating_add(u32::from(byte == b'<'))
            });
            self.carry_type_arguments(self.depth, open.saturating_add(count));
            return;
        }
        // The body starts and ends next to an ASCII `/`, so it is UTF-8.
        let Ok(text) = std::str::from_utf8(body) else {
            return;
        };
        let mut scanner = Scanner::new(text, u32::MAX, Dialect::Script);
        scanner.regex_body = true;
        scanner.jsx = self.jsx;
        scanner.reread_budget = u64::MAX;
        scanner.scan();
        self.rereads = self.rereads.saturating_add(scanner.rereads);
        self.jsx_tag |= scanner.jsx_tag;
        for list in &scanner.type_arguments {
            self.carry_type_arguments(base + list.depth, open.saturating_add(list.level));
        }
        carry_brackets(
            &mut self.expression_brackets,
            &scanner.expression_brackets,
            base,
            self.depth,
        );
        carry_brackets(
            &mut self.parameter_lists,
            &scanner.parameter_lists,
            base,
            self.depth,
        );
        self.max_type_arguments = self
            .max_type_arguments
            .max(open.saturating_add(scanner.max_type_arguments));
    }

    /// Keeps a type-argument list of a regex body open at `depth` with
    /// `level`. The depth is at most the running depth, so a later closing
    /// bracket ends it. A list at the same depth or deeper is merged into it
    /// and keeps the higher level.
    fn carry_type_arguments(&mut self, depth: u32, level: u32) {
        let depth = depth.min(self.depth);
        let mut level = level;
        while let Some(last) = self.type_arguments.last().copied()
            && last.depth >= depth
        {
            level = level.max(last.level);
            self.type_arguments.pop();
        }
        self.push_type_arguments(depth, level);
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

/// Keeps the brackets that stay open at the end of a regex body open after
/// it. A body bracket at `depth` from `base` is open at `base + depth`, at
/// most the running `depth`. The entries of `open` stay in depth order.
fn carry_brackets(open: &mut Vec<u32>, body: &[u32], base: u32, depth: u32) {
    for &bracket in body {
        let bracket = (base + bracket).min(depth);
        if open.last().is_none_or(|&last| last < bracket) {
            open.push(bracket);
        }
    }
}

/// Whether `byte` can be part of a CSS identifier, so `myurl(` is not `url(`.
const fn is_css_name_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_') || byte >= 0x80
}

/// Maximum number of hex digits in a CSS escape such as `\67`.
const MAX_HEX_ESCAPE_DIGITS: usize = 6;

const fn is_css_name_byte_or_escape(byte: u8) -> bool {
    is_css_name_byte(byte) || byte == b'\\'
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

    /// Skips a CSS escape after the `\` at `pos - 1`. A hex escape such as
    /// `\a` takes up to six hex digits and one whitespace after them, so a
    /// newline there does not end a string.
    fn skip_style_escape(&mut self) {
        let digits = self.bytes[self.pos..]
            .iter()
            .take(MAX_HEX_ESCAPE_DIGITS)
            .take_while(|byte| byte.is_ascii_hexdigit())
            .count();
        if digits == 0 {
            self.skip_escape();
            return;
        }
        self.pos += digits;
        self.pos += match self.bytes.get(self.pos..) {
            Some([b'\r', b'\n', ..]) => 2,
            Some([b' ' | b'\t' | b'\n' | b'\r' | 0x0C, ..]) => 1,
            _ => 0,
        };
    }

    /// Whether the word before the `/` at `pos - 1` is a keyword that a regex
    /// literal can follow.
    fn after_keyword(&self) -> bool {
        REGEX_KEYWORDS.contains(&self.word_before(self.pos - 1).0)
    }

    /// The identifier that ends before `end`, after optional whitespace, and
    /// its start offset. The word is empty when no identifier ends there.
    fn word_before(&self, end: usize) -> (&[u8], usize) {
        let before = self.bytes[..end].trim_ascii_end();
        let start = before
            .iter()
            .rposition(|&byte| !is_identifier_byte(byte))
            .map_or(0, |index| index + 1);
        (&before[start..], start)
    }
}

/// Whether `byte` can be part of a JavaScript identifier. A byte of a
/// non-ASCII character counts, so a word is never cut inside one.
const fn is_identifier_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'$') || byte >= 0x80
}

/// Whether `byte` can start a JavaScript identifier or a JSX tag name.
const fn is_identifier_start(byte: u8) -> bool {
    byte.is_ascii_alphabetic() || matches!(byte, b'_' | b'$') || byte >= 0x80
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

    fn color_depth_of(source: &str) -> usize {
        scan_depth(source, u32::MAX, Dialect::Style).max_colors
    }

    #[test]
    fn counts_nested_color_functions() {
        assert_eq!(color_depth_of(".a{color:red}"), 0);
        assert_eq!(
            color_depth_of(".a{color:color-mix(in srgb, LIGHT-DARK(rgb(0 0 0), red), red)}"),
            3
        );
        // Sibling functions and plain brackets do not add depth.
        assert_eq!(color_depth_of(".a{x:rgb(1 2 3) hsl(1 2% 3%) calc((1))}"), 1);
        assert_eq!(color_depth_of(".a{x:rgb(var(--a, hsl(1 2% 3%)))}"), 2);
        // A longer name, a string and a comment are not color functions.
        assert_eq!(color_depth_of(".a{x:myrgb(no-hsl(1))}"), 0);
        assert_eq!(color_depth_of(".a{content:\"rgb(rgb(\"}/* hsl(hsl( */"), 0);
        // An unclosed color function stays open.
        assert_eq!(color_depth_of(".a{x:light-dark(((light-dark(}"), 2);
    }

    fn color_cost_of(source: &str) -> usize {
        scan_depth(source, u32::MAX, Dialect::Style).color_cost
    }

    #[test]
    fn color_cost_doubles_with_each_nested_color_function() {
        assert_eq!(color_cost_of(".a{color:red}"), 0);
        // An outermost color function costs no extra work.
        assert_eq!(color_cost_of(".a{x:rgb(1)}"), 0);
        // The inner arguments `(1)` span 3 bytes, doubled at the second level.
        assert_eq!(color_cost_of(".a{x:rgb(rgb(1))}"), 6);
        // A third level doubles again.
        assert_eq!(color_cost_of(".a{x:rgb(rgb(rgb(1)))}"), 2 * 8 + 4 * 3);
        // Sibling functions do not add up: the most expensive one counts.
        assert_eq!(color_cost_of(".a{x:rgb(rgb(1)) rgb(rgb(1))}"), 6);
        // An unclosed color function spans the rest of the source.
        assert_eq!(color_cost_of("rgb(rgb(1"), 2 * 2);
    }

    /// OSS-Fuzz test case 6315655184580608: eight nested `light-dark(` calls
    /// that are never closed, with 90 KB of text after them, made the parse
    /// take more than a minute.
    #[test]
    fn deeply_nested_unclosed_color_functions_skip_the_parse() {
        let source = format!("@x {}{}", "light-dark(".repeat(8), "a b ".repeat(20_000));
        assert!(crate::parse_style_bounded(&source, || ()).is_none());
        // A large stylesheet of many short color functions stays parsed.
        let flat = ".a{color:color-mix(in oklab, light-dark(red, blue) 50%, transparent)}\n"
            .repeat(60_000);
        assert!(crate::parse_style_bounded(&flat, || ()).is_some());
    }

    #[test]
    fn an_escaped_function_name_counts_as_a_color_function() {
        assert_eq!(color_depth_of(".a{x:r\\67 b(r\\gb(\\72\tgb(1)))}"), 3);
        assert_eq!(color_depth_of(".a{x:f\\(1)}"), 0);
        // CSS reads `\r\n` after a hex escape as one whitespace.
        assert_eq!(color_depth_of(".a{x:r\\67\r\nb(r\\67\r\nb(1))}"), 2);
    }

    /// An escape before `url` makes a longer name such as `aurl`, a plain
    /// function whose body lightningcss parses. Its color functions count.
    #[test]
    fn an_escaped_name_that_ends_in_url_is_not_a_url() {
        let nested = "rgb(".repeat(3);
        for prefix in ["a\\", "\\61 ", "\\61\r\n", "\\61\t"] {
            let source = format!(".a{{x:{prefix}url({nested}");
            // The escaped name itself can also spell a color function.
            assert!(color_depth_of(&source) >= 3, "{prefix:?}");
        }
        assert_eq!(color_depth_of(".a{x:url(rgb(rgb(}"), 0);
    }

    #[test]
    fn color_function_depth_at_the_cap_is_allowed() {
        let at_limit = format!(".a{{x:{}", "rgb(".repeat(MAX_COLOR_FUNCTION_DEPTH));
        assert!(parse_style_bounded(&at_limit, || ()).is_some());
        assert!(parse_style_bounded(&format!("{at_limit}hsl("), || ()).is_none());
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

    fn type_argument_nesting_of(source: &str) -> u32 {
        scan_nesting(source, Dialect::Script).type_arguments
    }

    #[test]
    fn type_argument_nesting_counts_open_lists_behind_expression_brackets() {
        assert_eq!(type_argument_nesting_of(&"t<{[".repeat(5)), 5);
        // A `<` at the same depth replaces the open one.
        assert_eq!(type_argument_nesting_of("f(a<b, c<d, e<(g<h))"), 2);
        // `&&`, `||`, `??` and `=` cannot appear in a type, so they end the
        // open list.
        assert_eq!(
            type_argument_nesting_of("a < b && f(c < d || g(e < f ?? h(i = j < k)))"),
            1
        );
        // An object type holds no expression, so it adds no level.
        assert_eq!(
            type_argument_nesting_of(
                "useQuery<{ data: Array<{ id: string; tags: Map<string, Set<number>> }> }>()"
            ),
            1
        );
        assert_eq!(
            type_argument_nesting_of(&format!("{}x{}", "Z<{ a: ".repeat(40), " }>".repeat(40))),
            1
        );
        // `>`, `;` and closing brackets end a list. `=>`, `<=` and `<<=` do
        // not open or end one.
        assert_eq!(
            type_argument_nesting_of(
                "for (let i = 0; i < n; i++) { if (x[i] <= y) { f(() => a <<= b) } }"
            ),
            1
        );
        assert_eq!(
            type_argument_nesting_of("<ul>{xs.map(i => <li key={i}>{i < 2}</li>)}</ul>"),
            1
        );
        // Stylesheets have no type arguments.
        assert_eq!(
            scan_nesting(&"a>{[".repeat(40), Dialect::Style).type_arguments,
            0
        );
    }

    /// The parser also tries a grouping `(` before a pattern as a parameter
    /// list, so it reads each level of `t<({[` three times.
    #[test]
    fn a_parenthesized_pattern_adds_a_type_argument_level() {
        assert_eq!(type_argument_nesting_of(&"t<({[".repeat(5)), 9);
        assert_eq!(type_argument_nesting_of(&"t<( /**/ [{[".repeat(5)), 9);
        assert_eq!(type_argument_nesting_of(&"t<(({[".repeat(5)), 9);
        assert!(exceeds_nesting_limit(&"t<({[".repeat(6)));
        assert_eq!(type_argument_nesting_of("a < ({ [k]: b < c })"), 3);
        assert_eq!(type_argument_nesting_of("a < (typeof ({ [k]: b < c }))"), 3);
        // A call and a `(` before an operand hold no pattern of parameters.
        assert_eq!(type_argument_nesting_of(&"t<f({[".repeat(5)), 5);
        assert_eq!(type_argument_nesting_of(&"t<(a, {[".repeat(5)), 5);
        assert_eq!(type_argument_nesting_of("a < (b) + f(c < d)"), 2);
    }

    #[test]
    fn type_argument_nesting_at_the_cap_is_allowed() {
        let at_limit = "t<{[".repeat(MAX_TYPE_ARGUMENT_NESTING as usize);
        assert!(!exceeds_nesting_limit(&at_limit));
        assert!(exceeds_nesting_limit(&format!("{at_limit}t<")));
    }

    /// After `{}` the scanner reads `/` as the start of a regex, but the
    /// parser can read it as a division. The `<` in such a body must still
    /// count.
    #[test]
    fn type_arguments_in_a_guessed_regex_body_count() {
        let nested = "t<{[".repeat(MAX_TYPE_ARGUMENT_NESTING as usize + 2);
        let close = "]".repeat(MAX_TYPE_ARGUMENT_NESTING as usize + 2);
        assert!(exceeds_nesting_limit(&format!(
            "x = {{}} / {nested}{close}/ 1"
        )));
        assert!(!exceeds_nesting_limit("const re = /<a<b<c>/; f(re)"));
    }

    /// The type-argument lists that stay open at the end of a guessed regex
    /// body stay open after its `/`, as in the parse of a division.
    #[test]
    fn open_type_arguments_of_a_guessed_regex_body_stay_open() {
        let half = MAX_TYPE_ARGUMENT_NESTING as usize / 2 + 1;
        let body = format!("{}]", "t<{[".repeat(half));
        let after = "t<{[".repeat(half);
        assert!(exceeds_nesting_limit(&format!(
            "x = {{}} / {body}/ {after}"
        )));
        // A regex class can hold a further guessed regex after an escaped
        // `\]`, which ends the class bracket only in code. That regex body
        // carries its open lists too.
        let inner = format!("[\\] {{}} / {}/ ]", "t<".repeat(half));
        assert!(exceeds_nesting_limit(&format!(
            "x = {{}} / {inner}/ {after}"
        )));
    }

    #[test]
    fn depth_at_the_cap_is_allowed() {
        let at_limit = "[".repeat(MAX_NESTING_DEPTH as usize);
        assert!(!exceeds_nesting_limit(&at_limit));
        assert!(exceeds_nesting_limit(&format!("{at_limit}[")));
    }

    /// Nested type-argument lists far past the cap: `t<{[` 40 times, then
    /// `1` and the closing brackets.
    fn nested_type_arguments() -> String {
        format!("{}1{}", "t<{[".repeat(40), "]".repeat(40))
    }

    /// The parser tries each `a<` of a flat chain as a type-argument list that
    /// holds the rest of the chain, so it reads the chain once per `<`.
    #[test]
    fn a_flat_less_than_chain_exceeds_the_reread_budget() {
        let chain = format!("let a = 1;\nexport const x = {}a;\n", "a<".repeat(12_000));
        assert!(exceeds_nesting_limit(&chain));
        let list = format!("export const x = [{}];\n", "a<a,".repeat(6_000));
        assert!(exceeds_nesting_limit(&list));
        let closed = format!(
            "export const x = {}a{} a;\n",
            "a<".repeat(4_000),
            ">".repeat(4_000)
        );
        assert!(exceeds_nesting_limit(&closed));
    }

    /// Each `<` of a flat chain finds its enclosing list in constant time, so
    /// the scan of a long chain stays linear.
    #[test]
    fn a_long_flat_chain_scans_in_time() {
        let chain = format!("x = {}a;", "a<".repeat(1_000_000));
        let (done, finished) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = done.send(exceeds_nesting_limit(&chain));
        });
        let refused = finished
            .recv_timeout(std::time::Duration::from_secs(20))
            .expect("the scan must end in time");
        assert!(refused);
    }

    /// Flat comparisons that the parser reads once stay under the budget.
    #[test]
    fn ordinary_comparisons_stay_under_the_reread_budget() {
        let ternary = "x < 10 ? 1 : ".repeat(400);
        assert!(!exceeds_nesting_limit(&format!("const y = {ternary}0;")));
        let entries = "a < 10, ".repeat(200);
        assert!(!exceeds_nesting_limit(&format!("const y = [{entries}];")));
        let object = "k: a < 10, f() { return [1, 2, 3].map((v) => v * 2); },\n".repeat(2_000);
        assert!(!exceeds_nesting_limit(&format!(
            "const y = {{ {object} }};"
        )));
        let shifts = "K.a << 1, -1, -1, -1, -1, -1, -1, -1, -1, -1,\n".repeat(2_000);
        assert!(!exceeds_nesting_limit(&format!("const t = [{shifts}];")));
        let generic = format!("type T = {}x{};", "A<".repeat(200), ">".repeat(200));
        assert!(!exceeds_nesting_limit(&generic));
        // The parser keeps a list that a call follows, and reads it once.
        let call = format!(
            "const v = f<{}x{}>();",
            "A<".repeat(5_000),
            ">".repeat(5_000)
        );
        assert!(!exceeds_nesting_limit(&call));
        let conditional = "T extends U ? A<B> : C<D> | ".repeat(500);
        assert!(!exceeds_nesting_limit(&format!(
            "type X<T> = F<{conditional} never>;"
        )));
    }

    /// A string or regex guess that does not close reads at most to the end
    /// of its line. A later guess that reaches a byte in the same state ends
    /// the same way, so each byte is read a bounded number of times.
    #[test]
    fn failed_literal_guesses_read_each_byte_a_bounded_number_of_times() {
        let shapes = [
            ("/[}".repeat(4_000), Dialect::Script),
            ("\"\\\"".repeat(4_000), Dialect::Script),
            ("'\\'".repeat(4_000), Dialect::Script),
            ("\"\\\"\\\n".repeat(3_000), Dialect::Script),
            ("x = {} / a /[(".repeat(1_000), Dialect::Script),
            ("\"\\\"\\\n".repeat(3_000), Dialect::Style),
        ];
        for (source, dialect) in shapes {
            let (_, work) = crate::cost::measure(|| scan_nesting(&source, dialect));
            assert!(
                work.walk_steps <= 4 * source.len(),
                "{} steps for {} bytes of {:?}",
                work.walk_steps,
                source.len(),
                &source[..12]
            );
        }
    }

    /// A guess that fails does not change the result of a later guess in
    /// another state.
    #[test]
    fn a_failed_guess_does_not_stop_a_later_guess_that_closes() {
        assert_eq!(depth_of("a = b / c; d = /'/; e = '/' + (((1)))"), 3);
        assert_eq!(depth_of("a = \"x; b = 'x\"(' + (1)"), 2);
    }

    /// JSX text is not code. A quote pair, a comment opener or a backtick
    /// there does not start a literal for the parser, so it must not hide
    /// the expression container that follows it.
    #[test]
    fn literal_openers_in_jsx_text_do_not_hide_type_arguments() {
        let nested = nested_type_arguments();
        for (open, close) in [
            ("'", "'"),
            ("\"", "\""),
            ("//", ""),
            ("/*", "*/"),
            ("`", "`"),
        ] {
            let source = format!("export const x = <p>{open} {{{nested}}} {close}</p>;\n");
            assert!(exceeds_nesting_limit(&source), "{open}");
        }
        // Text and attribute strings of real markup stay under the cap.
        let markup = "<a href=\"x\" title='it&apos;s'>it's {n} items (of {total})</a>\n";
        assert!(!exceeds_nesting_limit(&format!(
            "export const x = <div>{}</div>;",
            markup.repeat(500)
        )));
    }

    /// A `)` in JSX text is text, so it does not close the `(` that opened
    /// before the element.
    #[test]
    fn a_closer_in_jsx_text_does_not_end_type_arguments() {
        let source = format!("export const x = {}1;\n", "t<(a=<p>)</p>+".repeat(40));
        assert!(exceeds_nesting_limit(&source));
    }

    /// A `/` after the `)` of an `if`, `while`, `for` or `with` header starts
    /// a regex, so a quote in it does not start a string.
    #[test]
    fn a_regex_after_a_control_header_does_not_hide_type_arguments() {
        let nested = nested_type_arguments();
        for keyword in ["if", "while", "for", "with"] {
            let source =
                format!("{keyword} (a) /'/.test(b); {nested}; {keyword} (a) /'/.test(b);\n");
            assert!(exceeds_nesting_limit(&source), "{keyword}");
        }
        // A `)` of a call still ends an operand.
        assert_eq!(depth_of("f(a) / ((b)) / c"), 2);
    }

    /// The parser reads the `#!` line at the start of a source as a comment.
    #[test]
    fn a_backtick_on_the_hashbang_line_does_not_hide_type_arguments() {
        let source = format!(
            "#!/usr/bin/env node `\n{};\n// `\n",
            nested_type_arguments()
        );
        assert!(exceeds_nesting_limit(&source));
    }
}
