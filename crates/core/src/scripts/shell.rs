//! Shell tokenization: splitting on operators, skipping env wrappers and package managers.

use super::ENV_WRAPPERS;
use std::borrow::Cow;

/// A literal shell argument with its original position for script forwarding.
pub(super) struct ShellWord<'a> {
    pub value: Cow<'a, str>,
    pub start: usize,
}

/// Preserve the literal argument prefix without evaluating shell expansions.
pub(super) fn split_words(source: &str) -> Vec<ShellWord<'_>> {
    let mut words = Vec::new();
    let mut rest = source.trim_start();
    while !rest.is_empty() {
        let Some(end) = shell_word_end(rest) else {
            break;
        };
        words.push(ShellWord {
            value: decode_shell_word(&rest[..end]),
            start: source.len() - rest.len(),
        });
        rest = rest[end..].trim_start();
    }
    words
}

fn shell_word_end(source: &str) -> Option<usize> {
    let mut quote = None;
    let windows_path = starts_windows_path(source);
    let mut chars = source.char_indices().peekable();
    while let Some((index, ch)) = chars.next() {
        match ch {
            '\\' if quote != Some('\'') && !windows_path => {
                chars.next()?;
            }
            '\'' | '"' if quote.is_none() => quote = Some(ch),
            ch if Some(ch) == quote => quote = None,
            // Dynamic command substitutions can produce arbitrary argument boundaries.
            '`' if quote != Some('\'') => return None,
            '$' if quote != Some('\'') && chars.peek().is_some_and(|(_, ch)| *ch == '(') => {
                return None;
            }
            ch if quote.is_none() && ch.is_whitespace() => return Some(index),
            _ => {}
        }
    }
    quote.is_none().then_some(source.len())
}

fn decode_shell_word(raw: &str) -> Cow<'_, str> {
    // package.json scripts also run under cmd.exe, where these are path separators.
    if starts_windows_path(raw) {
        return Cow::Borrowed(
            raw.strip_prefix('"')
                .and_then(|path| path.strip_suffix('"'))
                .unwrap_or(raw),
        );
    }
    if !raw.contains(['\'', '"', '\\']) {
        return Cow::Borrowed(raw);
    }
    let mut value = String::with_capacity(raw.len());
    let mut quote = None;
    let mut chars = raw.chars();
    while let Some(ch) = chars.next() {
        match ch {
            '\\' if quote != Some('\'') => {
                if let Some(next) = chars.next() {
                    if quote == Some('"') && !matches!(next, '"' | '\\' | '$' | '`' | '\n') {
                        value.push('\\');
                    }
                    if next != '\n' {
                        value.push(next);
                    }
                }
            }
            '\'' | '"' if quote.is_none() => quote = Some(ch),
            ch if Some(ch) == quote => quote = None,
            _ => value.push(ch),
        }
    }
    Cow::Owned(value)
}

fn starts_windows_path(source: &str) -> bool {
    let source = source.strip_prefix('"').unwrap_or(source);
    source.starts_with(".\\")
        || source.starts_with("..\\")
        || source.starts_with("\\\\")
        || matches!(source.as_bytes(), [drive, b':', b'\\', ..] if drive.is_ascii_alphabetic())
}

/// Bun runtime boolean flags that may precede an executed file/binary
/// (`bun --bun <bin>`, `bun --watch <file>`, `bun --hot run dev`). Bun documents
/// these as flags that go immediately after `bun`, before the `run`/file/binary
/// target. None consume a value, so they can be skipped to reach the target.
/// Value-taking flags (`--filter <glob>`) are deliberately absent: an unrecognized
/// leading flag makes the parser treat the command as a script delegation rather
/// than guess where the binary starts. Source: Bun runtime docs (oven-sh/bun
/// docs/runtime/index.mdx, watch-mode.mdx).
pub(super) const BUN_RUNTIME_FLAGS: &[&str] =
    &["--bun", "--watch", "--hot", "--smol", "--no-clear-screen"];

/// Check if a command is a shell built-in (not an npm package).
pub(super) fn is_builtin_command(cmd: &str) -> bool {
    matches!(
        cmd,
        "echo"
            | "cat"
            | "cp"
            | "mv"
            | "rm"
            | "mkdir"
            | "rmdir"
            | "ls"
            | "cd"
            | "pwd"
            | "test"
            | "true"
            | "false"
            | "exit"
            | "export"
            | "source"
            | "which"
            | "chmod"
            | "chown"
            | "touch"
            | "find"
            | "grep"
            | "sed"
            | "awk"
            | "xargs"
            | "tee"
            | "sort"
            | "uniq"
            | "wc"
            | "head"
            | "tail"
            | "sleep"
            | "wait"
            | "kill"
            | "sh"
            | "bash"
            | "zsh"
    )
}

/// The kind of an open unquoted parenthesis in a script.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Paren {
    /// `(` in command position opens a subshell: a new command starts in it.
    /// `case_depth` is the number of `case` commands open at the `(`.
    Subshell { case_depth: usize },
    /// `(` that starts a `case` pattern (`(lint) eslint .;;`).
    Pattern,
    /// `$(`, `$((`, `((` and every other `(`: the text stays in the current
    /// segment.
    Other,
}

/// The parser state that carries from one line of a shell file to the next:
/// the open quotes, parentheses and `case` commands.
#[derive(Default)]
pub(super) struct ShellState {
    parens: Vec<Paren>,
    case_depth: usize,
    in_single_quote: bool,
    in_double_quote: bool,
}

/// One command segment of a script, with the subshells that close and open
/// between the previous segment and this one.
pub(super) struct ShellSegment<'a> {
    pub text: &'a str,
    /// The number of subshells that close before this segment.
    pub closed: usize,
    /// The number of subshells that open before this segment, after the
    /// closed ones.
    pub opened: usize,
}

/// Collects the segments of one script.
struct SegmentSink<'a, 's> {
    segments: Vec<ShellSegment<'a>>,
    closed: usize,
    opened: usize,
    state: &'s mut ShellState,
}

impl<'a> SegmentSink<'a, '_> {
    fn push(&mut self, text: &'a str) {
        match first_command_word(text) {
            Some("case") => self.state.case_depth += 1,
            Some("esac") => self.state.case_depth = self.state.case_depth.saturating_sub(1),
            _ => {}
        }
        self.segments.push(ShellSegment {
            text,
            closed: std::mem::take(&mut self.closed),
            opened: std::mem::take(&mut self.opened),
        });
    }

    /// Push the subshell changes that no segment after them carries.
    fn finish(mut self) -> Vec<ShellSegment<'a>> {
        if self.closed > 0 || self.opened > 0 {
            self.push("");
        }
        self.segments
    }
}

/// The first word of a segment after the reserved words that start it.
fn first_command_word(text: &str) -> Option<&str> {
    text.split_whitespace()
        .find(|word| !COMMAND_PREFIX_WORDS.contains(word))
}

/// The words of `segment` after the reserved words that start it and after
/// a `case ... in` header. `None` when the header has no `in` yet. The flag
/// is true in a `case` body, where the next word can be a pattern.
fn words_after_case_header(segment: &str, case_depth: usize) -> Option<(Vec<&str>, bool)> {
    let mut words = segment
        .split_whitespace()
        .skip_while(|word| COMMAND_PREFIX_WORDS.contains(word))
        .peekable();
    if words.peek() == Some(&"case") {
        words.by_ref().find(|word| *word == "in")?;
        return Some((words.collect(), true));
    }
    Some((words.collect(), case_depth > 0))
}

/// What a `(` at the start of a word opens. `segment` is the text of the
/// current segment before the `(`, and `after` is the text after it.
fn paren_kind(segment: &str, after: &str, state: &ShellState) -> Paren {
    if after.starts_with('(')
        || !state
            .parens
            .iter()
            .all(|open| matches!(open, Paren::Subshell { .. }))
    {
        return Paren::Other;
    }
    let Some((words, in_case_body)) = words_after_case_header(segment, state.case_depth) else {
        return Paren::Other;
    };
    let at_command = match words.as_slice() {
        [] => true,
        // `a) (cd app && tsc);;`: a command follows the pattern.
        [pattern] => in_case_body && ends_case_pattern(pattern),
        _ => false,
    };
    if !at_command {
        Paren::Other
    } else if is_case_pattern(after) || (in_case_body && words.is_empty() && is_header_end(segment))
    {
        Paren::Pattern
    } else {
        Paren::Subshell {
            case_depth: state.case_depth,
        }
    }
}

/// Whether `segment` starts with a `case` header, where the first word after
/// `in` is always a pattern.
fn is_header_end(segment: &str) -> bool {
    first_command_word(segment) == Some("case")
}

/// Whether the text after a `(` closes a `case` pattern (`(lint) eslint .`).
/// The shell allows only an operator or a redirection after the `)` of a
/// subshell, so a pattern is a single word with a command after it. This
/// finds a pattern in a segment that is parsed without its `case` header.
fn is_case_pattern(after_paren: &str) -> bool {
    let Some(close) = after_paren.find(')') else {
        return false;
    };
    let pattern = &after_paren[..close];
    if pattern.is_empty()
        || pattern.contains(|ch: char| ch.is_whitespace() || "()'\"`$;&|<>\\".contains(ch))
    {
        return false;
    }
    let rest = after_paren[close + 1..].trim_start();
    let Some(next) = rest.split_whitespace().next() else {
        return false;
    };
    !next.starts_with([';', '&', '|', ')', '#']) && redirection_tokens(&[next]).is_none()
}

/// Whether the `)` that follows `segment` ends a `case` pattern
/// (`build) tsc;;`) and not a subshell. A pattern is the first word after the
/// `in` of the header, after a `;;` or at the start of a line in a `case`
/// body. `before` is the script text before `segment`.
fn ends_pattern(before: &str, segment: &str, case_depth: usize) -> bool {
    let before = before.trim_end();
    let pattern_start = is_header_end(segment) || before.is_empty() || before.ends_with(";;");
    match words_after_case_header(segment, case_depth) {
        Some((words, true)) if pattern_start => {
            matches!(words.as_slice(), [word] if *word != "esac")
        }
        _ => false,
    }
}

/// Split a script string on shell operators (`&&`, `||`, `;`, `|`, `&`).
/// Respects single and double quotes. A subshell (`(cd app && build)`)
/// starts and ends a segment.
pub fn split_shell_operators(script: &str) -> Vec<&str> {
    split_shell_commands(script, &mut ShellState::default())
        .into_iter()
        .map(|segment| segment.text)
        .collect()
}

/// Split a script as [`split_shell_operators`] does, and report where each
/// subshell opens and closes. `state` carries the open subshells and `case`
/// commands from the previous line of a shell file.
pub(super) fn split_shell_commands<'a>(
    script: &'a str,
    state: &mut ShellState,
) -> Vec<ShellSegment<'a>> {
    let mut sink = SegmentSink {
        segments: Vec::new(),
        closed: 0,
        opened: 0,
        state,
    };
    let mut start = 0;
    let bytes = script.as_bytes();
    let len = bytes.len();
    let mut i = 0;
    let mut word_start = 0;
    let mut quotes = Quotes {
        single: sink.state.in_single_quote,
        double: sink.state.in_double_quote,
    };

    while i < len {
        let b = bytes[i];

        if let Some(next) = quotes.step(script, i, word_start) {
            i = next;
            continue;
        }

        if b == b'(' {
            let kind = if i == word_start {
                paren_kind(&script[start..i], &script[i + 1..], sink.state)
            } else {
                Paren::Other
            };
            sink.state.parens.push(kind);
            if matches!(kind, Paren::Subshell { .. }) {
                // Only reserved words and a pattern come before the `(`, so
                // this pushes no command.
                sink.push(&script[start..i]);
                sink.opened += 1;
                i += 1;
                start = i;
                word_start = i;
                continue;
            }
        }
        if b == b')' && closes_subshell(&script[..start], &script[start..i], sink.state) {
            sink.push(&script[start..i]);
            sink.closed += 1;
            i += 1;
            start = i;
            word_start = i;
            continue;
        }

        if let Some(op_len) = shell_operator_len(bytes, i) {
            sink.push(&script[start..i]);
            i += op_len;
            start = i;
            word_start = i;
            continue;
        }

        if b.is_ascii_whitespace() {
            word_start = i + 1;
        }
        i += 1;
    }

    if start < len {
        sink.push(&script[start..]);
    }

    sink.state.in_single_quote = quotes.single;
    sink.state.in_double_quote = quotes.double;
    sink.finish()
}

/// The quote state of [`split_shell_commands`].
struct Quotes {
    single: bool,
    double: bool,
}

impl Quotes {
    /// Consume the byte at `i` when it is an escape, a quote, or a byte inside
    /// quotes, and return the index after it. Return `None` for a byte that
    /// the caller must read as shell syntax.
    fn step(&mut self, script: &str, i: usize, word_start: usize) -> Option<usize> {
        let len = script.len();
        match script.as_bytes()[i] {
            b'\\' if !self.single && !starts_windows_path(&script[word_start..]) => {
                Some((i + 2).min(len))
            }
            b'\'' if !self.double => {
                self.single = !self.single;
                Some(i + 1)
            }
            b'"' if !self.single => {
                self.double = !self.double;
                Some(i + 1)
            }
            _ if self.single || self.double => Some(i + 1),
            _ => None,
        }
    }
}

/// Whether the `)` after `segment` closes a subshell, and pop the `(` that it
/// matches. A `)` that ends a `case` pattern matches no `(` or a pattern `(`.
/// Only a `case` that opens in the subshell can hold such a pattern. A `)`
/// without an open `(` closes nothing.
fn closes_subshell(before: &str, segment: &str, state: &mut ShellState) -> bool {
    match state.parens.last().copied() {
        Some(Paren::Subshell { case_depth })
            if (state.case_depth > case_depth || is_header_end(segment))
                && ends_pattern(before, segment, state.case_depth) =>
        {
            false
        }
        Some(kind) => {
            state.parens.pop();
            matches!(kind, Paren::Subshell { .. })
        }
        None => false,
    }
}

/// Command substitutions that print the `node_modules/.bin` directory of the
/// package (`npm bin`, removed in npm 9 but still in older scripts).
const NPM_BIN_SUBSTITUTIONS: &[&str] = &["$(npm bin)/", "`npm bin`/"];

/// Replace each `$(npm bin)/` prefix in `segment` with the literal
/// `node_modules/.bin/` directory that it prints. Then `$(npm bin)/tool`
/// is a static path to the binary and not a dynamic word.
pub(super) fn expand_npm_bin(segment: &str) -> Cow<'_, str> {
    let mut expanded = Cow::Borrowed(segment);
    for substitution in NPM_BIN_SUBSTITUTIONS {
        if expanded.contains(substitution) {
            expanded = Cow::Owned(expanded.replace(substitution, "node_modules/.bin/"));
        }
    }
    expanded
}

/// Return the bodies of the outermost `$(...)` and backtick command
/// substitutions in `script`.
///
/// The shell runs these bodies as commands, also inside double quotes. Text in
/// single quotes does not run, so it is skipped. `$((...))` arithmetic and
/// `${...}` parameter expressions are skipped too. A substitution inside a body
/// is not returned: parse the body again to get it. An unclosed substitution
/// gives no body.
pub fn command_substitutions(script: &str) -> Vec<&str> {
    let bytes = script.as_bytes();
    let mut bodies = Vec::new();
    let mut in_double_quote = false;
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => i += 2,
            b'\'' if !in_double_quote => {
                let Some(end) = find_byte(bytes, i + 1, b'\'') else {
                    break;
                };
                i = end + 1;
            }
            b'"' => {
                in_double_quote = !in_double_quote;
                i += 1;
            }
            b'`' => {
                let Some(end) = backtick_end(bytes, i + 1) else {
                    break;
                };
                bodies.push(&script[i + 1..end]);
                i = end + 1;
            }
            b'$' if bytes.get(i + 1) == Some(&b'{') => {
                let Some(end) = closing_delimiter(bytes, i + 2, b'{', b'}') else {
                    break;
                };
                i = end + 1;
            }
            b'$' if bytes.get(i + 1) == Some(&b'(') => {
                let start = i + 2;
                let Some(end) = closing_delimiter(bytes, start, b'(', b')') else {
                    break;
                };
                if bytes.get(start) != Some(&b'(') {
                    bodies.push(&script[start..end]);
                }
                i = end + 1;
            }
            _ => i += 1,
        }
    }
    bodies
}

fn find_byte(bytes: &[u8], from: usize, needle: u8) -> Option<usize> {
    bytes
        .get(from..)?
        .iter()
        .position(|&b| b == needle)
        .map(|offset| from + offset)
}

/// Index of the backtick that closes a backtick substitution whose body
/// starts at `from`. A backslash escapes the next byte.
fn backtick_end(bytes: &[u8], from: usize) -> Option<usize> {
    let mut i = from;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => i += 2,
            b'`' => return Some(i),
            _ => i += 1,
        }
    }
    None
}

/// Index of the `close` byte that matches an `open` byte before `from`.
///
/// Quotes in the body start a new quoting context, as in the shell, so a
/// delimiter inside quotes does not count.
fn closing_delimiter(bytes: &[u8], from: usize, open: u8, close: u8) -> Option<usize> {
    let mut depth = 1usize;
    let mut in_double_quote = false;
    let mut i = from;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'\\' {
            i += 2;
            continue;
        }
        if b == b'"' {
            in_double_quote = !in_double_quote;
        } else if !in_double_quote {
            if b == b'\'' {
                i = find_byte(bytes, i + 1, b'\'')?;
            } else if b == open {
                depth += 1;
            } else if b == close {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
        }
        i += 1;
    }
    None
}

/// Return the byte length of a shell operator at position `i`, or `None`.
///
/// Checks two-char operators (`&&`, `||`) before single-char ones (`&`, `|`, `;`)
/// to avoid splitting `&&` as two `&` operators.
fn shell_operator_len(bytes: &[u8], i: usize) -> Option<usize> {
    let b = bytes[i];
    let next = bytes.get(i + 1).copied();

    if matches!((b, next), (b'&', Some(b'&')) | (b'|', Some(b'|'))) {
        return Some(2);
    }

    if b == b';' {
        return Some(1);
    }
    if b == b'|' && next != Some(b'|') {
        return Some(1);
    }
    // `2>&1`, `<&3` and `&> log` are redirections, not a background `&`.
    // An escaped `\>` before the `&` is a literal character, not an operator.
    let prev = i.checked_sub(1).map(|prev| bytes[prev]);
    let prev_escaped = i.checked_sub(2).map(|before| bytes[before]) == Some(b'\\');
    let redirection = (matches!(prev, Some(b'>' | b'<')) && !prev_escaped) || next == Some(b'>');
    if b == b'&' && next != Some(b'&') && !redirection {
        return Some(1);
    }

    None
}

/// Shell reserved words that a command follows in the same segment
/// (`then eslint .`, `do tsc; done`, `! grep x`).
const COMMAND_PREFIX_WORDS: &[&str] = &[
    "if", "then", "else", "elif", "do", "while", "until", "!", "{",
];

/// Shell reserved words after which the segment holds no command: the end of
/// a compound command (`fi`, `done`, `esac`, `}`) and the headers whose words
/// are not commands (`for app in lib/*`).
const NON_COMMAND_WORDS: &[&str] = &["fi", "done", "esac", "}", "for", "select", "in"];

/// Redirection operators, longest first, after an optional file descriptor
/// number (`2>&1`, `&> log`, `> log`).
const REDIRECTION_OPERATORS: &[&str] = &[
    "&>>", "<<<", "&>", ">>", ">&", "<&", ">|", "<>", "<<", ">", "<",
];

/// The number of tokens that the redirection at the start of `tokens` takes:
/// 1 when the target is in the same token (`2>&1`, `>log`), 2 when it is the
/// next token (`> log`), or `None` when `tokens` does not start with one.
pub(super) fn redirection_tokens(tokens: &[&str]) -> Option<usize> {
    let token = tokens
        .first()?
        .trim_start_matches(|ch: char| ch.is_ascii_digit());
    let operator = REDIRECTION_OPERATORS
        .iter()
        .find(|operator| token.starts_with(**operator))?;
    Some(if token.len() == operator.len() { 2 } else { 1 })
}

/// Whether `token` ends a `case` pattern (`build)`, `(lint)`, `*)`) or is a
/// function header (`build()`). An unquoted `)` cannot end a command name,
/// but it does end a command substitution (`$(npm bin)`).
fn ends_case_pattern(token: &str) -> bool {
    token.ends_with(')') && !token.contains("$(") && !token.contains('`')
}

/// Return the index of the command at the start of a segment, after the
/// shell reserved words (`then`, `do`), a `case` pattern, a function
/// header (`function name`, `name()`) and the
/// redirections before the command (`> log`). Return `None` when the segment
/// runs no command. The shell reads a reserved word only in command
/// position, so a caller passes the tokens of a whole segment.
pub(super) fn command_start(tokens: &[&str]) -> Option<usize> {
    let mut idx = 0;
    while let Some(token) = tokens.get(idx) {
        idx += if COMMAND_PREFIX_WORDS.contains(token) {
            1
        } else if *token == "function" {
            2
        } else if *token == "case" {
            // `case "$1" in build) tsc`: the command follows the pattern.
            tokens[idx..].iter().position(|word| *word == "in")? + 1
        } else if ends_case_pattern(token) {
            1
        } else if let Some(taken) = redirection_tokens(&tokens[idx..]) {
            taken
        } else {
            break;
        };
    }
    let token = tokens.get(idx)?;
    (!NON_COMMAND_WORDS.contains(token)).then_some(idx)
}

/// Package-manager commands that download and run a package that the project
/// does not need to install (`npx foo`, `pnpm dlx foo`).
pub(super) fn downloads_its_command(segment: &str) -> bool {
    let words = split_words(segment);
    let words: Vec<&str> = words.iter().map(|word| word.value.as_ref()).collect();
    let Some(start) = command_start(&words) else {
        return false;
    };
    let mut tokens = words[start..].iter().copied().skip_while(|token| {
        super::ENV_WRAPPERS.contains(token)
            || *token == "--"
            || (token.contains('=') && !token.starts_with('-'))
    });
    let Some(manager) = tokens.next() else {
        return false;
    };
    // Flags such as `--filter web` can come before the subcommand, so the
    // subcommand is the first token that is not a flag or a flag value.
    let mut subcommands = tokens.filter(|token| !token.starts_with('-'));
    match manager {
        "npx" | "pnpx" | "bunx" => true,
        "pnpm" | "yarn" => subcommands.any(|token| token == "dlx"),
        // `npm x` and `bun x` are the aliases of `npm exec` and `bunx`.
        "npm" | "bun" => subcommands.next().is_some_and(|token| token == "x"),
        _ => false,
    }
}

/// Skip shell reserved words, env var assignments (`KEY=value`) and env
/// wrapper commands (`cross-env`, `dotenv`, `env`) at the start of a token
/// list. Returns the index of the first real command token, or `None` if all
/// tokens were consumed. Reserved words count only at index 0, the start of a
/// segment: after a wrapper, `for` is the name of a command.
pub fn skip_initial_wrappers(tokens: &[&str], mut idx: usize) -> Option<usize> {
    if idx == 0 {
        idx = command_start(tokens)?;
    }
    while idx < tokens.len() && super::is_env_assignment(tokens[idx]) {
        idx += 1;
    }
    if idx >= tokens.len() {
        return None;
    }

    while idx < tokens.len() && ENV_WRAPPERS.contains(&tokens[idx]) {
        let wrapper = tokens[idx];
        idx += 1;
        loop {
            let before = idx;
            while idx < tokens.len() && super::is_env_assignment(tokens[idx]) {
                idx += 1;
            }
            idx = skip_env_wrapper_flag(wrapper, tokens, idx);
            if idx == before {
                break;
            }
        }
        if idx < tokens.len() && tokens[idx] == "--" {
            idx += 1;
        }
    }
    if idx >= tokens.len() {
        return None;
    }

    Some(idx)
}

/// `dotenv-cli` flags that take a value (`dotenv -e .env.ci -- eslint src`).
const DOTENV_VALUE_FLAGS: &[&str] = &["-e", "-v", "-p"];
/// `dotenv-cli` flags without a value.
const DOTENV_BOOLEAN_FLAGS: &[&str] = &["-o", "--override", "--debug", "--no-expand"];
/// `env` flags that take a value (`env -u HOME eslint src`).
const ENV_VALUE_FLAGS: &[&str] = &["-u", "--unset", "-C", "--chdir"];
/// `env` flags without a value.
const ENV_BOOLEAN_FLAGS: &[&str] = &["-i", "--ignore-environment", "-0", "--null", "-"];

/// Return the index after one option of an env wrapper at `idx`, or `idx`
/// when the token there is not an option of that wrapper.
fn skip_env_wrapper_flag(wrapper: &str, tokens: &[&str], idx: usize) -> usize {
    let Some(&token) = tokens.get(idx) else {
        return idx;
    };
    let (value_flags, boolean_flags) = match wrapper {
        "dotenv" => (DOTENV_VALUE_FLAGS, DOTENV_BOOLEAN_FLAGS),
        "env" => (ENV_VALUE_FLAGS, ENV_BOOLEAN_FLAGS),
        _ => return idx,
    };
    if boolean_flags.contains(&token) {
        return idx + 1;
    }
    if value_flags.contains(&token) {
        return (idx + 2).min(tokens.len());
    }
    if token
        .split_once('=')
        .is_some_and(|(flag, _)| value_flags.contains(&flag))
    {
        return idx + 1;
    }
    // `dotenv -c [environment]`: the environment name is optional.
    if wrapper == "dotenv" && token == "-c" {
        let has_value = tokens
            .get(idx + 1)
            .is_some_and(|next| *next != "--" && !next.starts_with('-'));
        return if has_value { idx + 2 } else { idx + 1 };
    }
    idx
}

/// Advance past package manager prefixes (`npx`, `pnpx`, `bunx`, `yarn exec`, `pnpm dlx`, etc.).
/// Returns the index of the actual binary token, or `None` if the command delegates to a named
/// script (e.g., `npm run build`, `yarn build`).
pub fn advance_past_package_manager(tokens: &[&str], mut idx: usize) -> Option<usize> {
    let token = tokens[idx];
    // `run <name>` in a Yarn 2+ script is `yarn run <name>`.
    if super::yarn_run::is_shell_run(tokens, idx) {
        return None;
    }
    if matches!(token, "npx" | "pnpx" | "bunx") {
        idx += 1;
        while idx < tokens.len() && tokens[idx].starts_with('-') {
            let flag = tokens[idx];
            idx += 1;
            if matches!(flag, "--package" | "-p") && idx < tokens.len() {
                idx += 1;
            }
        }
    } else if token == "bun" {
        idx += 1;
        let mut saw_runtime_flag = false;
        while idx < tokens.len() && BUN_RUNTIME_FLAGS.contains(&tokens[idx]) {
            idx += 1;
            saw_runtime_flag = true;
        }
        if idx >= tokens.len() {
            return None;
        }
        let subcmd = tokens[idx];
        if subcmd == "exec" || subcmd == "x" {
            idx += 1;
        } else if matches!(subcmd, "run" | "run-script") || !saw_runtime_flag {
            return None;
        }
    } else if matches!(token, "yarn" | "pnpm" | "npm") {
        if idx + 1 < tokens.len() {
            let subcmd = tokens[idx + 1];
            if subcmd == "exec" || subcmd == "dlx" || (token == "npm" && subcmd == "x") {
                idx += 2;
                // `npm exec -- eslint` passes everything after `--` to the binary.
                if tokens.get(idx) == Some(&"--") {
                    idx += 1;
                }
            } else {
                return None;
            }
        } else {
            return None;
        }
    }
    if idx >= tokens.len() {
        return None;
    }

    Some(idx)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literal_words_preserve_quoted_and_escaped_boundaries() {
        for (source, expected) in [
            (
                r#"varlock run -p "./env -- is-ci ignored" -- publint"#,
                vec![
                    "varlock",
                    "run",
                    "-p",
                    "./env -- is-ci ignored",
                    "--",
                    "publint",
                ],
            ),
            (
                r"varlock run -p ./env\ --\ is-ci\ ignored -- publint",
                vec![
                    "varlock",
                    "run",
                    "-p",
                    "./env -- is-ci ignored",
                    "--",
                    "publint",
                ],
            ),
            (
                r#"va"r"lock run -- 'is-ci'"#,
                vec!["varlock", "run", "--", "is-ci"],
            ),
            (r#"echo "one\q" """#, vec!["echo", r"one\q", ""]),
            (
                r#""C:\Program Files\tool.exe" arg"#,
                vec![r"C:\Program Files\tool.exe", "arg"],
            ),
            (
                r".\tools\runner.exe arg",
                vec![r".\tools\runner.exe", "arg"],
            ),
            (
                r"\\server\share\runner.exe arg",
                vec![r"\\server\share\runner.exe", "arg"],
            ),
        ] {
            let words = split_words(source);
            assert_eq!(
                words
                    .iter()
                    .map(|word| word.value.as_ref())
                    .collect::<Vec<_>>(),
                expected,
                "{source}"
            );
        }
    }

    #[test]
    fn literal_words_keep_raw_offsets_for_forwarding() {
        let source = r#" npm --prefix "./path with spaces" run task -- -p "./env -- ignored""#;
        let words = split_words(source);
        assert_eq!(&source[words[6].start..], r#"-p "./env -- ignored""#);
    }

    #[test]
    fn literal_words_stop_at_opaque_argument_after_known_prefix() {
        for source in [
            "vite $(pwd) -- is-ci",
            "vite `pwd` -- is-ci",
            "vite 'unclosed",
            "vite \\",
        ] {
            let words = split_words(source);
            assert_eq!(
                words
                    .iter()
                    .map(|word| word.value.as_ref())
                    .collect::<Vec<_>>(),
                vec!["vite"],
                "{source}"
            );
        }
    }

    #[test]
    fn escaped_operators_remain_inside_the_argument() {
        assert_eq!(
            split_shell_operators(r"echo value\;tail && publint"),
            vec![r"echo value\;tail ", " publint"]
        );
    }

    #[test]
    fn quoted_windows_path_retains_following_shell_command() {
        let source = r#""C:\Program Files\" && publint"#;
        assert_eq!(
            split_shell_operators(source),
            vec![r#""C:\Program Files\" "#, " publint"]
        );
        let commands = super::super::parse_script(source);
        assert_eq!(
            commands
                .iter()
                .map(|command| command.binary.as_str())
                .collect::<Vec<_>>(),
            vec!["C:\\Program Files\\", "publint"]
        );
    }

    #[test]
    fn operator_len_double_ampersand() {
        assert_eq!(shell_operator_len(b"&&", 0), Some(2));
    }

    #[test]
    fn operator_len_double_pipe() {
        assert_eq!(shell_operator_len(b"||", 0), Some(2));
    }

    #[test]
    fn operator_len_semicolon() {
        assert_eq!(shell_operator_len(b";", 0), Some(1));
    }

    #[test]
    fn operator_len_single_pipe() {
        assert_eq!(shell_operator_len(b"|x", 0), Some(1));
    }

    #[test]
    fn operator_len_single_ampersand() {
        assert_eq!(shell_operator_len(b"&x", 0), Some(1));
    }

    #[test]
    fn operator_len_non_operator() {
        assert_eq!(shell_operator_len(b"abc", 0), None);
        assert_eq!(shell_operator_len(b"xyz", 1), None);
    }

    #[test]
    fn operator_len_ampersand_at_end_of_slice() {
        assert_eq!(shell_operator_len(b"&", 0), Some(1));
    }

    #[test]
    fn operator_len_pipe_at_end_of_slice() {
        assert_eq!(shell_operator_len(b"|", 0), Some(1));
    }

    #[test]
    fn substitutions_read_dollar_paren_and_backtick_bodies() {
        assert_eq!(
            command_substitutions("a=$(node one.ts) b=`node two.ts`"),
            vec!["node one.ts", "node two.ts"]
        );
    }

    #[test]
    fn substitutions_read_bodies_inside_double_quotes() {
        assert_eq!(
            command_substitutions(r#"x="$(node one.ts arg)""#),
            vec!["node one.ts arg"]
        );
        assert_eq!(
            command_substitutions(r#"x="`node two.ts`""#),
            vec!["node two.ts"]
        );
    }

    #[test]
    fn substitutions_return_the_outer_body_of_a_nested_substitution() {
        let outer = command_substitutions(r#"x="$(echo "$(node inner.ts)" | tr a b)""#);
        assert_eq!(outer, vec![r#"echo "$(node inner.ts)" | tr a b"#]);
        assert_eq!(command_substitutions(outer[0]), vec!["node inner.ts"]);
    }

    #[test]
    fn substitutions_match_parens_in_quotes_and_subshells() {
        assert_eq!(
            command_substitutions("x=$(echo ')' \")\" && (cd a; node b.ts))"),
            vec!["echo ')' \")\" && (cd a; node b.ts)"]
        );
    }

    #[test]
    fn substitutions_skip_single_quoted_text() {
        assert!(command_substitutions("echo '$(node none.ts) `node none.ts`'").is_empty());
        assert_eq!(
            command_substitutions(r#"echo "it's $(node one.ts)""#),
            vec!["node one.ts"]
        );
    }

    #[test]
    fn substitutions_skip_arithmetic_and_parameter_expressions() {
        assert!(command_substitutions("echo $((1 + (2 * 3))) ${HOME} ${x:-)}").is_empty());
        assert_eq!(
            command_substitutions("echo $((1 + 2)) $(node one.ts)"),
            vec!["node one.ts"]
        );
    }

    #[test]
    fn substitutions_skip_escaped_and_unclosed_forms() {
        assert!(command_substitutions("echo \\$(node none.ts) \\`x`").is_empty());
        assert!(command_substitutions("x=$(node none.ts").is_empty());
        assert!(command_substitutions("x=`node none.ts").is_empty());
    }

    #[test]
    fn split_subshell_parentheses() {
        let segments: Vec<&str> = split_shell_operators(r#"([ -n "$CI" ] || (skill install))"#)
            .into_iter()
            .map(str::trim)
            .filter(|segment| !segment.is_empty())
            .collect();
        assert_eq!(segments, vec![r#"[ -n "$CI" ]"#, "skill install"]);
    }

    #[test]
    fn split_keeps_substitution_and_word_parentheses() {
        assert_eq!(
            split_shell_operators("$(npm bin)/tsc -p ."),
            vec!["$(npm bin)/tsc -p ."]
        );
        assert_eq!(
            split_shell_operators("echo $((1 + 2))"),
            vec!["echo $((1 + 2))"]
        );
        assert_eq!(
            split_shell_operators("tool 'a (b) c'"),
            vec!["tool 'a (b) c'"]
        );
        assert_eq!(split_shell_operators(r"tool \(b\)"), vec![r"tool \(b\)"]);
        assert_eq!(split_shell_operators("a) tool"), vec!["a) tool"]);
    }

    #[test]
    fn skip_reserved_words_at_segment_start() {
        assert_eq!(skip_initial_wrappers(&["then", "eslint", "."], 0), Some(1));
        assert_eq!(
            skip_initial_wrappers(&["do", "if", "!", "grep"], 0),
            Some(3)
        );
        assert_eq!(
            skip_initial_wrappers(&["then", "A=1", "eslint"], 0),
            Some(2)
        );
        assert_eq!(
            skip_initial_wrappers(&["for", "app", "in", "lib/*"], 0),
            None
        );
        assert_eq!(skip_initial_wrappers(&["done", "<", "list.txt"], 0), None);
        assert_eq!(skip_initial_wrappers(&["fi"], 0), None);
        assert_eq!(skip_initial_wrappers(&["nohup", "done"], 1), Some(1));
    }

    #[test]
    fn split_empty_input() {
        let segments = split_shell_operators("");
        assert!(segments.is_empty());
    }

    #[test]
    fn split_only_operators() {
        let segments = split_shell_operators("&&||;");
        assert!(segments.iter().all(|s| s.is_empty()));
    }

    #[test]
    fn split_single_quoted_operators_preserved() {
        let segments = split_shell_operators("echo 'a && b || c'");
        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0], "echo 'a && b || c'");
    }

    #[test]
    fn split_double_quoted_operators_preserved() {
        let segments = split_shell_operators("echo \"a | b ; c\"");
        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0], "echo \"a | b ; c\"");
    }

    #[test]
    fn split_nested_single_in_double_quotes() {
        let segments = split_shell_operators("echo \"it's fine\" && jest");
        assert_eq!(segments.len(), 2);
        assert_eq!(segments[1].trim(), "jest");
    }

    #[test]
    fn split_nested_double_in_single_quotes() {
        let segments = split_shell_operators("echo 'say \"hello\"' && jest");
        assert_eq!(segments.len(), 2);
        assert_eq!(segments[1].trim(), "jest");
    }

    #[test]
    fn split_no_operators() {
        let segments = split_shell_operators("webpack --mode production");
        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0], "webpack --mode production");
    }

    #[test]
    fn split_trailing_operator() {
        let segments = split_shell_operators("server &");
        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0], "server ");
    }

    #[test]
    fn split_mixed_operators() {
        let segments = split_shell_operators("a && b || c ; d | e & f");
        assert_eq!(segments.len(), 6);
        assert_eq!(segments[0].trim(), "a");
        assert_eq!(segments[1].trim(), "b");
        assert_eq!(segments[2].trim(), "c");
        assert_eq!(segments[3].trim(), "d");
        assert_eq!(segments[4].trim(), "e");
        assert_eq!(segments[5].trim(), "f");
    }

    #[test]
    fn skip_wrappers_no_wrappers() {
        let tokens = vec!["webpack", "--mode", "production"];
        assert_eq!(skip_initial_wrappers(&tokens, 0), Some(0));
    }

    #[test]
    fn skip_wrappers_env_prefix() {
        let tokens = vec!["env", "NODE_ENV=production", "webpack"];
        assert_eq!(skip_initial_wrappers(&tokens, 0), Some(2));
    }

    #[test]
    fn skip_wrappers_cross_env_prefix() {
        let tokens = vec!["cross-env", "NODE_ENV=production", "webpack"];
        assert_eq!(skip_initial_wrappers(&tokens, 0), Some(2));
    }

    #[test]
    fn skip_wrappers_dotenv_with_separator() {
        let tokens = vec!["dotenv", "--", "webpack"];
        assert_eq!(skip_initial_wrappers(&tokens, 0), Some(2));
    }

    #[test]
    fn skip_wrappers_env_var_only() {
        let tokens = vec!["NODE_ENV=production", "CI=true"];
        assert_eq!(skip_initial_wrappers(&tokens, 0), None);
    }

    #[test]
    fn skip_wrappers_cross_env_only() {
        let tokens = vec!["cross-env", "NODE_ENV=production"];
        assert_eq!(skip_initial_wrappers(&tokens, 0), None);
    }

    #[test]
    fn skip_wrappers_multiple_env_vars_then_binary() {
        let tokens = vec!["NODE_ENV=test", "CI=true", "DEBUG=1", "jest"];
        assert_eq!(skip_initial_wrappers(&tokens, 0), Some(3));
    }

    #[test]
    fn skip_wrappers_starting_at_nonzero_index() {
        let tokens = vec!["ignored", "cross-env", "NODE_ENV=prod", "webpack"];
        assert_eq!(skip_initial_wrappers(&tokens, 1), Some(3));
    }

    #[test]
    fn advance_npm_run_returns_none() {
        let tokens = vec!["npm", "run", "build"];
        assert_eq!(advance_past_package_manager(&tokens, 0), None);
    }

    #[test]
    fn advance_npm_run_script_returns_none() {
        let tokens = vec!["npm", "run-script", "test"];
        assert_eq!(advance_past_package_manager(&tokens, 0), None);
    }

    #[test]
    fn advance_yarn_bare_returns_none() {
        let tokens = vec!["yarn", "build"];
        assert_eq!(advance_past_package_manager(&tokens, 0), None);
    }

    #[test]
    fn advance_yarn_exec() {
        let tokens = vec!["yarn", "exec", "jest", "--coverage"];
        assert_eq!(advance_past_package_manager(&tokens, 0), Some(2));
    }

    #[test]
    fn advance_pnpm_exec() {
        let tokens = vec!["pnpm", "exec", "vitest", "run"];
        assert_eq!(advance_past_package_manager(&tokens, 0), Some(2));
    }

    #[test]
    fn advance_pnpm_dlx() {
        let tokens = vec!["pnpm", "dlx", "create-react-app"];
        assert_eq!(advance_past_package_manager(&tokens, 0), Some(2));
    }

    #[test]
    fn advance_npx_simple() {
        let tokens = vec!["npx", "eslint", "src"];
        assert_eq!(advance_past_package_manager(&tokens, 0), Some(1));
    }

    #[test]
    fn advance_npx_with_flags() {
        let tokens = vec!["npx", "--yes", "--package", "@scope/tool", "eslint"];
        assert_eq!(advance_past_package_manager(&tokens, 0), Some(4));
    }

    #[test]
    fn advance_pnpx_simple() {
        let tokens = vec!["pnpx", "vitest"];
        assert_eq!(advance_past_package_manager(&tokens, 0), Some(1));
    }

    #[test]
    fn advance_bunx_simple() {
        let tokens = vec!["bunx", "esbuild", "src/index.ts"];
        assert_eq!(advance_past_package_manager(&tokens, 0), Some(1));
    }

    #[test]
    fn advance_no_package_manager() {
        let tokens = vec!["webpack", "--mode", "production"];
        assert_eq!(advance_past_package_manager(&tokens, 0), Some(0));
    }

    #[test]
    fn advance_bare_npm_returns_none() {
        let tokens = vec!["npm"];
        assert_eq!(advance_past_package_manager(&tokens, 0), None);
    }

    #[test]
    fn advance_bare_yarn_returns_none() {
        let tokens = vec!["yarn"];
        assert_eq!(advance_past_package_manager(&tokens, 0), None);
    }

    #[test]
    fn advance_npx_with_only_flags() {
        let tokens = vec!["npx", "--yes"];
        assert_eq!(advance_past_package_manager(&tokens, 0), None);
    }

    #[test]
    fn advance_bun_exec() {
        let tokens = vec!["bun", "exec", "jest"];
        assert_eq!(advance_past_package_manager(&tokens, 0), Some(2));
    }

    #[test]
    fn advance_bun_run_returns_none() {
        let tokens = vec!["bun", "run", "dev"];
        assert_eq!(advance_past_package_manager(&tokens, 0), None);
    }

    #[test]
    fn advance_bun_runtime_flag_then_binary() {
        let tokens = vec!["bun", "--bun", "prek", "install"];
        assert_eq!(advance_past_package_manager(&tokens, 0), Some(2));
    }

    #[test]
    fn advance_bun_multiple_runtime_flags_then_binary() {
        let tokens = vec!["bun", "--bun", "--watch", "prek"];
        assert_eq!(advance_past_package_manager(&tokens, 0), Some(3));
    }

    #[test]
    fn advance_bun_runtime_flag_then_run_is_script() {
        let tokens = vec!["bun", "--watch", "run", "dev"];
        assert_eq!(advance_past_package_manager(&tokens, 0), None);
    }

    #[test]
    fn advance_bun_x_executes_binary() {
        let tokens = vec!["bun", "x", "cowsay"];
        assert_eq!(advance_past_package_manager(&tokens, 0), Some(2));
    }

    #[test]
    fn advance_bun_unknown_leading_flag_returns_none() {
        let tokens = vec!["bun", "--filter", "foo", "run", "build"];
        assert_eq!(advance_past_package_manager(&tokens, 0), None);
    }

    #[test]
    fn advance_bun_bare_name_returns_none() {
        let tokens = vec!["bun", "scripts/build.ts"];
        assert_eq!(advance_past_package_manager(&tokens, 0), None);
    }

    #[test]
    fn advance_bun_runtime_flag_only_returns_none() {
        let tokens = vec!["bun", "--watch"];
        assert_eq!(advance_past_package_manager(&tokens, 0), None);
    }
}
