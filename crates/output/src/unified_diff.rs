//! One reader for unified diff text.
//!
//! The reader owns the header grammar, the path decoding and the hunk state.
//! It reads a file header only outside a hunk, so an added source line such
//! as `++count;` (which arrives as `+++count;`) stays an added line. It
//! decodes the C-quoted paths of git (`"b/src/caf\303\251.ts"`) and removes
//! the tab that git writes after a path with a space.

use std::borrow::Cow;

/// The header of one file section of a unified diff.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DiffFileHeader<'a> {
    /// Base path, from the `--- a/<path>` line.
    pub old_path: Option<Cow<'a, str>>,
    /// Head path, from the `+++ b/<path>` line. A section with rename lines
    /// and no `+++` line takes it from the `rename to` line.
    pub new_path: Option<Cow<'a, str>>,
    /// Base path of a rename. Set only when the section has a `rename from`
    /// line and a `rename to` line.
    pub renamed_from: Option<Cow<'a, str>>,
    /// The section has a `--- /dev/null` line.
    pub is_new: bool,
    /// The section has a `+++ /dev/null` line.
    pub is_deleted: bool,
}

/// One item of a unified diff, in text order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiffEvent<'a> {
    /// The header of a file section. It comes before the hunks of the file.
    File(DiffFileHeader<'a>),
    /// A `@@ -a,b +c,d @@` line. A side without a length has length 1.
    Hunk {
        /// First base line of the hunk (`a`).
        old_start: u64,
        /// Number of base lines in the hunk (`b`).
        old_len: u64,
        /// First head line of the hunk (`c`).
        new_start: u64,
        /// Number of head lines in the hunk (`d`).
        new_len: u64,
    },
    /// An added line, without its `+`.
    Added(&'a str),
    /// A removed line, without its `-`.
    Removed(&'a str),
    /// An unchanged line, without its leading space.
    Context(&'a str),
    /// A `\ No newline at end of file` line.
    NoNewline,
}

/// Read `diff` as a sequence of [`DiffEvent`]s.
#[must_use]
pub fn unified_diff_events(diff: &str) -> UnifiedDiffEvents<'_> {
    UnifiedDiffEvents {
        lines: diff.lines(),
        pending: None,
        queued: None,
        rename_from: None,
        in_hunk: false,
        old_left: 0,
        new_left: 0,
    }
}

/// The iterator of [`unified_diff_events`].
#[derive(Debug)]
pub struct UnifiedDiffEvents<'a> {
    lines: std::str::Lines<'a>,
    /// The header of the current section, until its first hunk.
    pending: Option<DiffFileHeader<'a>>,
    /// The hunk that follows a `File` event of the same `@@` line.
    queued: Option<DiffEvent<'a>>,
    /// The `rename from` path of the current section.
    rename_from: Option<Cow<'a, str>>,
    /// A `@@` line came after the last header line.
    in_hunk: bool,
    /// Base lines that the current hunk still expects.
    old_left: u64,
    /// Head lines that the current hunk still expects.
    new_left: u64,
}

impl<'a> Iterator for UnifiedDiffEvents<'a> {
    type Item = DiffEvent<'a>;

    fn next(&mut self) -> Option<DiffEvent<'a>> {
        if let Some(event) = self.queued.take() {
            return Some(event);
        }
        loop {
            let Some(line) = self.lines.next() else {
                // `take` clears `pending`, so the next call returns `None`.
                return self.pending.take().map(DiffEvent::File);
            };
            if self.in_hunk
                && let Some(event) = self.hunk_line(line)
            {
                return Some(event);
            }
            if let Some(event) = self.header_line(line) {
                return Some(event);
            }
        }
    }
}

impl<'a> UnifiedDiffEvents<'a> {
    /// Classify a line inside a hunk. `None` when the line is not hunk
    /// content. The counters decide only if a `--- ` or `+++ ` line is a
    /// header: a hunk that counts too few lines stays open.
    fn hunk_line(&mut self, line: &'a str) -> Option<DiffEvent<'a>> {
        match line.as_bytes().first() {
            Some(b'+') => {
                if self.new_left == 0 && line.starts_with("+++ ") {
                    return None;
                }
                self.new_left = self.new_left.saturating_sub(1);
                Some(DiffEvent::Added(&line[1..]))
            }
            Some(b'-') => {
                if self.old_left == 0 && line.starts_with("--- ") {
                    return None;
                }
                self.old_left = self.old_left.saturating_sub(1);
                Some(DiffEvent::Removed(&line[1..]))
            }
            Some(b' ') | None => {
                self.old_left = self.old_left.saturating_sub(1);
                self.new_left = self.new_left.saturating_sub(1);
                Some(DiffEvent::Context(line.get(1..).unwrap_or("")))
            }
            Some(b'\\') => Some(DiffEvent::NoNewline),
            Some(_) => None,
        }
    }

    fn header_line(&mut self, line: &'a str) -> Option<DiffEvent<'a>> {
        if line.starts_with("diff --git ") {
            self.in_hunk = false;
            self.rename_from = None;
            // `replace` stores the header of the new section and returns the
            // header of the section before it.
            return self
                .pending
                .replace(DiffFileHeader::default())
                .map(DiffEvent::File);
        }
        if let Some(rest) = line.strip_prefix("rename from ") {
            self.rename_from = decode_git_path(rest);
            return None;
        }
        if let Some(rest) = line.strip_prefix("rename to ") {
            if let (Some(from), Some(to)) = (self.rename_from.take(), decode_git_path(rest)) {
                let header = self.pending.get_or_insert_with(DiffFileHeader::default);
                header.renamed_from = Some(from);
                header.new_path = Some(to);
            }
            return None;
        }
        if let Some(rest) = line.strip_prefix("--- ") {
            self.in_hunk = false;
            let header = self.pending.get_or_insert_with(DiffFileHeader::default);
            match header_side(rest, "a/") {
                HeaderSide::DevNull => header.is_new = true,
                HeaderSide::Path(path) => header.old_path = Some(path),
                HeaderSide::Other => {}
            }
            return None;
        }
        if let Some(rest) = line.strip_prefix("+++ ") {
            self.in_hunk = false;
            let header = self.pending.get_or_insert_with(DiffFileHeader::default);
            match header_side(rest, "b/") {
                HeaderSide::DevNull => header.is_deleted = true,
                HeaderSide::Path(path) => header.new_path = Some(path),
                HeaderSide::Other => {}
            }
            return None;
        }
        if let Some(rest) = line.strip_prefix("@@ ") {
            let (old_start, old_len, new_start, new_len) = parse_hunk_header(rest)?;
            self.in_hunk = true;
            self.old_left = old_len;
            self.new_left = new_len;
            let hunk = DiffEvent::Hunk {
                old_start,
                old_len,
                new_start,
                new_len,
            };
            // `take` clears `pending`: each section gives one `File` event.
            return Some(match self.pending.take() {
                Some(header) => {
                    self.queued = Some(hunk);
                    DiffEvent::File(header)
                }
                None => hunk,
            });
        }
        line.starts_with('\\').then_some(DiffEvent::NoNewline)
    }
}

/// One side of a `---` or `+++` line.
enum HeaderSide<'a> {
    DevNull,
    Path(Cow<'a, str>),
    Other,
}

/// Decode the path of a `---` or `+++` line and strip its `a/` or `b/`
/// prefix. A quoted path ends at its closing quote. An unquoted path ends at
/// the first tab, which git writes after a path with a space.
fn header_side<'a>(rest: &'a str, prefix: &str) -> HeaderSide<'a> {
    if rest.starts_with('"') {
        let Some(decoded) = unquote_git_path(rest) else {
            return HeaderSide::Other;
        };
        return decoded
            .strip_prefix(prefix)
            .map_or(HeaderSide::Other, |path| {
                HeaderSide::Path(Cow::Owned(path.to_owned()))
            });
    }
    let text = rest.split_once('\t').map_or(rest, |(path, _)| path);
    if text == "/dev/null" {
        return HeaderSide::DevNull;
    }
    text.strip_prefix(prefix).map_or(HeaderSide::Other, |path| {
        HeaderSide::Path(Cow::Borrowed(path))
    })
}

/// Decode the path of a `rename from` or `rename to` line. `None` for a
/// quoted path that does not decode.
fn decode_git_path(text: &str) -> Option<Cow<'_, str>> {
    if text.starts_with('"') {
        return unquote_git_path(text).map(Cow::Owned);
    }
    Some(Cow::Borrowed(text))
}

/// Decode a path in the C quoting of git, from the opening quote to the
/// closing quote. Text after the closing quote is ignored. `None` for a path
/// with no closing quote or with an unknown escape.
fn unquote_git_path(quoted: &str) -> Option<String> {
    let mut chars = quoted.strip_prefix('"')?.bytes();
    let mut bytes = Vec::with_capacity(quoted.len());
    loop {
        match chars.next()? {
            b'"' => return Some(String::from_utf8_lossy(&bytes).into_owned()),
            b'\\' => {
                let byte = match chars.next()? {
                    b'\\' => b'\\',
                    b'"' => b'"',
                    b'a' => 0x07,
                    b'b' => 0x08,
                    b'f' => 0x0c,
                    b'n' => b'\n',
                    b'r' => b'\r',
                    b't' => b'\t',
                    b'v' => 0x0b,
                    first @ b'0'..=b'3' => {
                        let mut value = first - b'0';
                        for _ in 0..2 {
                            let digit = chars.next()?;
                            if !(b'0'..=b'7').contains(&digit) {
                                return None;
                            }
                            value = value * 8 + (digit - b'0');
                        }
                        value
                    }
                    _ => return None,
                };
                bytes.push(byte);
            }
            byte => bytes.push(byte),
        }
    }
}

/// Parse `-a[,b] +c[,d]` from the first two words of the text after `@@ `.
fn parse_hunk_header(rest: &str) -> Option<(u64, u64, u64, u64)> {
    let mut words = rest.split_whitespace();
    let (old_start, old_len) = parse_hunk_range(words.next()?.strip_prefix('-')?)?;
    let (new_start, new_len) = parse_hunk_range(words.next()?.strip_prefix('+')?)?;
    Some((old_start, old_len, new_start, new_len))
}

/// Parse `start,len`, or `start` with length 1.
fn parse_hunk_range(range: &str) -> Option<(u64, u64)> {
    match range.split_once(',') {
        Some((start, len)) => Some((start.parse().ok()?, len.parse().ok()?)),
        None => Some((range.parse().ok()?, 1)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn events(diff: &str) -> Vec<DiffEvent<'_>> {
        unified_diff_events(diff).collect()
    }

    fn headers(diff: &str) -> Vec<DiffFileHeader<'_>> {
        unified_diff_events(diff)
            .filter_map(|event| match event {
                DiffEvent::File(header) => Some(header),
                _ => None,
            })
            .collect()
    }

    fn header(old: Option<&str>, new: Option<&str>) -> DiffFileHeader<'static> {
        DiffFileHeader {
            old_path: old.map(|path| Cow::Owned(path.to_owned())),
            new_path: new.map(|path| Cow::Owned(path.to_owned())),
            ..DiffFileHeader::default()
        }
    }

    #[test]
    fn plus_plus_line_in_a_hunk_is_an_added_line() {
        let diff = "diff --git a/src/a.js b/src/a.js\n--- a/src/a.js\n+++ b/src/a.js\n@@ -1,0 +2,2 @@ let count = 0;\n+++count;\n+--count;\n";
        assert_eq!(
            events(diff),
            vec![
                DiffEvent::File(header(Some("src/a.js"), Some("src/a.js"))),
                DiffEvent::Hunk {
                    old_start: 1,
                    old_len: 0,
                    new_start: 2,
                    new_len: 2
                },
                DiffEvent::Added("++count;"),
                DiffEvent::Added("--count;"),
            ]
        );
    }

    #[test]
    fn minus_minus_line_in_a_hunk_is_a_removed_line() {
        let diff = "diff --git a/a.js b/a.js\n--- a/a.js\n+++ b/a.js\n@@ -1,2 +0,0 @@\n---count;\n--- a/other.js\n";
        let all = events(diff);
        assert_eq!(
            all.iter()
                .filter(|event| matches!(event, DiffEvent::File(_)))
                .count(),
            1
        );
        assert_eq!(
            &all[2..],
            &[
                DiffEvent::Removed("--count;"),
                DiffEvent::Removed("-- a/other.js")
            ]
        );
    }

    #[test]
    fn header_shaped_added_line_does_not_start_a_file() {
        let diff = "diff --git a/a.js b/a.js\n--- a/a.js\n+++ b/a.js\n@@ -0,0 +1,2 @@\n+++ b/other.js\n+x\n";
        let all = events(diff);
        assert_eq!(
            all.iter()
                .filter(|event| matches!(event, DiffEvent::File(_)))
                .count(),
            1
        );
        assert_eq!(
            &all[2..],
            &[DiffEvent::Added("++ b/other.js"), DiffEvent::Added("x")]
        );
    }

    #[test]
    fn quoted_path_is_decoded() {
        let diff =
            "--- \"a/src/caf\\303\\251.ts\"\n+++ \"b/src/caf\\303\\251.ts\"\n@@ -1,0 +2 @@\n+x2\n";
        assert_eq!(
            headers(diff),
            vec![header(Some("src/café.ts"), Some("src/café.ts"))]
        );
        let quote = headers("+++ \"b/src/q\\\"uote.ts\"\n");
        assert_eq!(quote[0].new_path.as_deref(), Some("src/q\"uote.ts"));
        let tab = headers("+++ \"b/src/tab\\tname.ts\"\n");
        assert_eq!(tab[0].new_path.as_deref(), Some("src/tab\tname.ts"));
    }

    #[test]
    fn tab_after_a_path_is_not_part_of_the_path() {
        let space = headers("+++ b/src/my file.ts\t\n");
        assert_eq!(space[0].new_path.as_deref(), Some("src/my file.ts"));
        let quoted = headers("--- \"a/src/del \\303\\251.ts\"\t\n");
        assert_eq!(quoted[0].old_path.as_deref(), Some("src/del é.ts"));
    }

    #[test]
    fn rename_lines_set_the_old_and_new_path() {
        let diff = "diff --git a/src/old name.ts b/src/new name.ts\nsimilarity index 82%\nrename from src/old name.ts\nrename to src/new name.ts\n--- a/src/old name.ts\t\n+++ b/src/new name.ts\t\n@@ -6,0 +7 @@ six\n+seven\n";
        let found = headers(diff);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].renamed_from.as_deref(), Some("src/old name.ts"));
        assert_eq!(found[0].new_path.as_deref(), Some("src/new name.ts"));

        let pure =
            "diff --git a/x.ts b/y.ts\nsimilarity index 100%\nrename from x.ts\nrename to y.ts\n";
        let found = headers(pure);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].renamed_from.as_deref(), Some("x.ts"));
        assert_eq!(found[0].new_path.as_deref(), Some("y.ts"));

        let quoted =
            "diff --git a b\nrename from \"src/caf\\303\\251.ts\"\nrename to src/cafe.ts\n";
        let found = headers(quoted);
        assert_eq!(found[0].renamed_from.as_deref(), Some("src/café.ts"));
        assert_eq!(found[0].new_path.as_deref(), Some("src/cafe.ts"));
    }

    #[test]
    fn new_and_deleted_files_set_their_flags() {
        let added =
            headers("diff --git a/n.ts b/n.ts\n--- /dev/null\n+++ b/n.ts\n@@ -0,0 +1 @@\n+n\n");
        assert!(added[0].is_new);
        assert!(!added[0].is_deleted);
        assert_eq!(added[0].new_path.as_deref(), Some("n.ts"));

        let deleted =
            headers("diff --git a/d.ts b/d.ts\n--- a/d.ts\n+++ /dev/null\n@@ -1 +0,0 @@\n-d\n");
        assert!(deleted[0].is_deleted);
        assert!(!deleted[0].is_new);
        assert_eq!(deleted[0].old_path.as_deref(), Some("d.ts"));
        assert_eq!(deleted[0].new_path, None);
    }

    #[test]
    fn no_newline_marker_after_the_last_hunk_line_is_one_event() {
        let diff = "diff --git a/src/my file.ts b/src/my file.ts\n--- a/src/my file.ts\t\n+++ b/src/my file.ts\t\n@@ -1,0 +2 @@ y\n+y2\n\\ No newline at end of file\n";
        let all = events(diff);
        assert_eq!(all.last(), Some(&DiffEvent::NoNewline));
        assert_eq!(
            all.iter()
                .filter(|event| **event == DiffEvent::NoNewline)
                .count(),
            1
        );
    }

    #[test]
    fn second_file_without_a_git_header_starts_after_a_complete_hunk() {
        let diff = "--- a/x\n+++ b/x\n@@ -1 +1 @@\n-a\n+b\n--- a/y\n+++ b/y\n@@ -1 +1 @@\n-c\n+d\n";
        let paths: Vec<_> = headers(diff)
            .into_iter()
            .map(|header| header.new_path.map(Cow::into_owned))
            .collect();
        assert_eq!(paths, vec![Some("x".to_owned()), Some("y".to_owned())]);
    }

    #[test]
    fn hunk_header_that_counts_too_few_lines_keeps_the_hunk_open() {
        let diff = "--- a/x\n+++ b/x\n@@ -0,0 +1 @@\n+a\n+b\n+c\n";
        let added = events(diff)
            .into_iter()
            .filter(|event| matches!(event, DiffEvent::Added(_)))
            .count();
        assert_eq!(added, 3);
    }

    #[test]
    fn hunk_header_with_section_text_is_parsed() {
        let all = events("--- a/x\n+++ b/x\n@@ -6,0 +7 @@ six\n+seven\n");
        assert_eq!(
            all[1],
            DiffEvent::Hunk {
                old_start: 6,
                old_len: 0,
                new_start: 7,
                new_len: 1
            }
        );
    }
}
