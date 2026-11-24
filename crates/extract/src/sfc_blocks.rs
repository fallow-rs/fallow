//! Linear-time scan of `<script>` and `<style>` blocks in Vue and Svelte files.
//!
//! The scan gives the same blocks as these leftmost-first regexes:
//!
//! ```text
//! (?is)<script\b(?P<attrs>(?:[^>"'{]|"[^"]*"|'[^']*'|BRACE|\{)*)>(?P<body>[\s\S]*?)</script>
//! (?is)<style\b(?P<attrs>(?:[^>"']|"[^"]*"|'[^']*')*)>(?P<body>[\s\S]*?)</style>
//! ```
//!
//! `BRACE` is a brace expression up to 3 levels deep. Its quoted values (with
//! `"`, `'` or a backtick) can hold `>` (#4828). The innermost level is
//! `\{[^{}]*\}`.
//!
//! A regex search keeps a thread alive for each tag start whose quoted value
//! is not closed. Each search then reads far past the block that it reports,
//! and the next search reads that text again. On a file with many unclosed
//! quotes, the cost is quadratic. This scan stores each attribute position
//! that cannot reach a usable `>`, so it reads each position a bounded number
//! of times.

use std::ops::Range;
use std::sync::LazyLock;

static SCRIPT_OPEN_RE: LazyLock<regex::Regex> =
    LazyLock::new(|| crate::static_regex(r"(?i)<script\b"));
static SCRIPT_CLOSE_RE: LazyLock<regex::Regex> =
    LazyLock::new(|| crate::static_regex(r"(?i)</script>"));
static STYLE_OPEN_RE: LazyLock<regex::Regex> =
    LazyLock::new(|| crate::static_regex(r"(?i)<style\b"));
static STYLE_CLOSE_RE: LazyLock<regex::Regex> =
    LazyLock::new(|| crate::static_regex(r"(?i)</style>"));

/// Deepest brace level that a script attribute expression can have.
const MAX_BRACE_DEPTH: u8 = 3;

/// Memo value for a frame position that has no result yet.
const UNKNOWN: usize = 0;
/// Memo value for a frame position whose brace does not close.
const UNCLOSED: usize = usize::MAX;

/// One `<script>` or `<style>` block. All ranges are byte ranges in the source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SfcBlock {
    /// Byte range of the whole block, from `<` of the opening tag to the end
    /// of the closing tag.
    pub span: Range<usize>,
    /// Byte range of the opening-tag attributes, without the final `>`.
    pub attrs: Range<usize>,
    /// Byte range of the block body.
    pub body: Range<usize>,
}

/// All `<script>` blocks of `source`, in source order.
pub fn script_blocks(source: &str) -> Vec<SfcBlock> {
    scan(source, &SCRIPT_OPEN_RE, &SCRIPT_CLOSE_RE, true)
}

/// All `<style>` blocks of `source`, in source order.
pub fn style_blocks(source: &str) -> Vec<SfcBlock> {
    scan(source, &STYLE_OPEN_RE, &STYLE_CLOSE_RE, false)
}

fn scan(source: &str, open: &regex::Regex, close: &regex::Regex, braces: bool) -> Vec<SfcBlock> {
    let closes: Vec<Range<usize>> = close.find_iter(source).map(|m| m.range()).collect();
    let Some(last_close) = closes.last() else {
        return Vec::new();
    };
    let mut attrs = AttrScan::new(source.as_bytes(), last_close.start, braces);
    let mut blocks = Vec::new();
    let mut cursor = 0;
    for opening in open.find_iter(source) {
        if opening.start() < cursor {
            continue;
        }
        let Some(gt) = attrs.end_from(opening.end()) else {
            continue;
        };
        let index = closes.partition_point(|range| range.start <= gt);
        let Some(closing) = closes.get(index) else {
            continue;
        };
        blocks.push(SfcBlock {
            span: opening.start()..closing.end,
            attrs: opening.end()..gt,
            body: gt + 1..closing.start,
        });
        cursor = closing.end;
    }
    blocks
}

/// Finds the `>` that ends the attributes of an opening tag.
///
/// The attributes match only at a `>` before `limit`, the start of the last
/// closing tag, because a block needs a closing tag after its `>`.
struct AttrScan<'a> {
    bytes: &'a [u8],
    limit: usize,
    braces: bool,
    /// Attribute positions that a scan visited. A visited position cannot
    /// reach a usable `>`: the scan stops at the first usable `>`, and the
    /// next search starts after the block that it found.
    visited: Vec<u64>,
    /// Frame results for depths 2 and 3, filled on the first brace.
    frames: [Vec<usize>; 2],
}

impl<'a> AttrScan<'a> {
    fn new(bytes: &'a [u8], limit: usize, braces: bool) -> Self {
        Self {
            bytes,
            limit,
            braces,
            visited: vec![0; limit / 64 + 1],
            frames: [Vec::new(), Vec::new()],
        }
    }

    /// The position of the `>` that ends attributes starting at `start`.
    ///
    /// A `{` is the only choice point: the regex first tries a closed brace
    /// expression, then a plain `{`. The scan tries them in that order and
    /// keeps the plain-`{` alternatives on a stack.
    fn end_from(&mut self, start: usize) -> Option<usize> {
        let mut alternatives = Vec::new();
        let mut pos = start;
        loop {
            if let Some(gt) = self.walk(pos, &mut alternatives) {
                return Some(gt);
            }
            pos = alternatives.pop()?;
        }
    }

    fn walk(&mut self, mut pos: usize, alternatives: &mut Vec<usize>) -> Option<usize> {
        while pos < self.limit && !self.visit(pos) {
            match self.bytes[pos] {
                b'>' => return Some(pos),
                quote @ (b'"' | b'\'') => pos = self.after_quote(quote, pos)?,
                b'{' if self.braces => {
                    if let Some(end) = self.frame(MAX_BRACE_DEPTH, pos + 1) {
                        alternatives.push(pos + 1);
                        pos = end;
                    } else {
                        pos += 1;
                    }
                }
                _ => pos += 1,
            }
        }
        None
    }

    /// Marks `pos` as visited and returns whether it was visited before.
    fn visit(&mut self, pos: usize) -> bool {
        let (word, bit) = (pos / 64, 1u64 << (pos % 64));
        let seen = self.visited[word] & bit != 0;
        self.visited[word] |= bit;
        seen
    }

    /// The position after the quote that closes the quote at `pos`.
    fn after_quote(&self, quote: u8, pos: usize) -> Option<usize> {
        self.bytes[pos + 1..self.limit]
            .iter()
            .position(|&byte| byte == quote)
            .map(|offset| pos + offset + 2)
    }

    /// The position after the `}` that closes a brace expression at `depth`,
    /// for a scan that is at `from` in that expression.
    fn frame(&mut self, depth: u8, from: usize) -> Option<usize> {
        if depth == 1 {
            return self.innermost(from);
        }
        if self.frames[0].is_empty() {
            self.frames = [vec![UNKNOWN; self.limit], vec![UNKNOWN; self.limit]];
        }
        let memo = usize::from(depth - 2);
        let mut walked = Vec::new();
        let mut pos = from;
        let end = loop {
            if pos >= self.limit {
                break None;
            }
            match self.frames[memo][pos] {
                UNKNOWN => {}
                UNCLOSED => break None,
                end => break Some(end),
            }
            walked.push(pos);
            match self.bytes[pos] {
                b'}' => break Some(pos + 1),
                b'{' => match self.frame(depth - 1, pos + 1) {
                    Some(end) => pos = end,
                    None => break None,
                },
                quote @ (b'"' | b'\'' | b'`') => match self.after_quote(quote, pos) {
                    Some(end) => pos = end,
                    None => break None,
                },
                _ => pos += 1,
            }
        };
        let value = end.unwrap_or(UNCLOSED);
        for pos in walked {
            self.frames[memo][pos] = value;
        }
        end
    }

    /// The position after the `}` that closes `\{[^{}]*\}` at `from`.
    fn innermost(&self, from: usize) -> Option<usize> {
        let offset = self.bytes[from.min(self.limit)..self.limit]
            .iter()
            .position(|&byte| matches!(byte, b'{' | b'}'))?;
        let pos = from + offset;
        (self.bytes[pos] == b'}').then_some(pos + 1)
    }
}
