//! Byte-column to UTF-16 conversion at the LSP protocol boundary.
//!
//! Analysis results carry 0-based byte columns. The LSP wire contract uses
//! UTF-16 code units, so conversion happens here and only here.

use std::path::{Path, PathBuf};

use ls_types::{Position, Range};
use rustc_hash::FxHashMap;

/// Range covering a byte column through to the end of its line, converted to
/// UTF-16 at the protocol boundary. `u32::MAX` is the LSP idiom for end-of-line.
pub fn line_range_from_byte_col(
    mapper: &mut PositionMapper,
    path: &Path,
    line: u32,
    col: u32,
) -> Range {
    Range {
        start: Position {
            line,
            character: mapper.utf16_col(path, line, col),
        },
        end: Position {
            line,
            character: u32::MAX,
        },
    }
}

/// A finding that points at one identifier: a 1-based line and a 0-based byte
/// column, as the analysis results report them.
pub struct NamedAnchor<'a> {
    pub path: &'a Path,
    pub line: u32,
    pub col: u32,
    pub name: &'a str,
}

/// Lazily maps byte columns from analysis results into LSP UTF-16 columns.
///
/// Each file is read once per mapper, and its line starts are indexed once,
/// so a lookup does not scan the file up to the line again.
#[derive(Default)]
pub struct PositionMapper {
    files: FxHashMap<PathBuf, Option<LineIndex>>,
}

impl PositionMapper {
    /// Convert a 0-based byte column on a 0-based line to a UTF-16 column.
    pub fn utf16_col(&mut self, path: &Path, line0: u32, byte_col: u32) -> u32 {
        let Some(index) = self.line_index(path) else {
            return byte_col;
        };
        index.utf16_col(line0, byte_col)
    }

    /// Convert a 0-based byte span on a 0-based line to a UTF-16 span.
    pub fn utf16_col_span(
        &mut self,
        path: &Path,
        line0: u32,
        byte_col: u32,
        ident: &str,
    ) -> (u32, u32) {
        let start = self.utf16_col(path, line0, byte_col);
        let width = u32::try_from(ident.encode_utf16().count()).unwrap_or(u32::MAX);
        (start, start.saturating_add(width))
    }

    fn line_index(&mut self, path: &Path) -> Option<&LineIndex> {
        if !self.files.contains_key(path) {
            let index = std::fs::read_to_string(path).ok().map(LineIndex::new);
            self.files.insert(path.to_path_buf(), index);
        }
        self.files.get(path).and_then(Option::as_ref)
    }
}

/// File text with the byte offset of each line start. Lines split on `\n`
/// only, so a `\r` before it stays part of the line.
struct LineIndex {
    content: String,
    line_starts: Vec<usize>,
}

impl LineIndex {
    fn new(content: String) -> Self {
        let line_starts = std::iter::once(0)
            .chain(
                content
                    .bytes()
                    .enumerate()
                    .filter(|&(_, byte)| byte == b'\n')
                    .map(|(offset, _)| offset + 1),
            )
            .collect();
        Self {
            content,
            line_starts,
        }
    }

    fn line(&self, line0: u32) -> Option<&str> {
        let line0 = line0 as usize;
        let start = *self.line_starts.get(line0)?;
        let end = self
            .line_starts
            .get(line0 + 1)
            .map_or(self.content.len(), |next_start| next_start - 1);
        self.content.get(start..end)
    }

    fn utf16_col(&self, line0: u32, byte_col: u32) -> u32 {
        let Some(line) = self.line(line0) else {
            return byte_col;
        };
        let mut col = (byte_col as usize).min(line.len());
        while col > 0 && !line.is_char_boundary(col) {
            col -= 1;
        }
        u32::try_from(line[..col].encode_utf16().count()).unwrap_or(u32::MAX)
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    fn byte_col_to_utf16(content: &str, line0: u32, byte_col: u32) -> u32 {
        LineIndex::new(content.to_string()).utf16_col(line0, byte_col)
    }

    /// The conversion before the line index: scan to the line, then count
    /// the UTF-16 units of the prefix.
    fn reference_utf16_col(content: &str, line0: u32, byte_col: u32) -> u32 {
        let Some(line) = content.split('\n').nth(line0 as usize) else {
            return byte_col;
        };
        let mut col = (byte_col as usize).min(line.len());
        while col > 0 && !line.is_char_boundary(col) {
            col -= 1;
        }
        u32::try_from(line[..col].encode_utf16().count()).unwrap_or(u32::MAX)
    }

    proptest! {
        #[test]
        fn line_index_matches_the_reference_conversion(
            content in "(?s)[a-z \\t\\r\\n\u{e9}\u{4e2d}\u{1f389}]{0,120}",
            line0 in 0u32..12,
            byte_col in 0u32..60,
        ) {
            prop_assert_eq!(
                byte_col_to_utf16(&content, line0, byte_col),
                reference_utf16_col(&content, line0, byte_col),
            );
        }
    }

    #[test]
    fn crlf_line_keeps_the_carriage_return() {
        assert_eq!(byte_col_to_utf16("ab\r\ncd\r\n", 0, 3), 3);
        assert_eq!(byte_col_to_utf16("ab\r\ncd\r\n", 1, 1), 1);
    }

    #[test]
    fn trailing_newline_has_an_empty_last_line() {
        assert_eq!(byte_col_to_utf16("ab\n", 1, 5), 0);
        assert_eq!(byte_col_to_utf16("ab\n", 2, 5), 5);
    }

    #[test]
    fn ascii_columns_pass_through() {
        assert_eq!(byte_col_to_utf16("const helper = 1;\n", 0, 6), 6);
    }

    #[test]
    fn emoji_before_token_converts_to_utf16() {
        assert_eq!(
            byte_col_to_utf16("const emoji = \"🎉\"; helper\n", 0, 22),
            20
        );
    }

    #[test]
    fn byte_col_past_line_end_clamps() {
        assert_eq!(byte_col_to_utf16("🎉\n", 0, 200), 2);
    }

    #[test]
    fn missing_line_falls_back_to_byte_col() {
        assert_eq!(byte_col_to_utf16("only one line", 4, 42), 42);
    }

    #[test]
    fn unreadable_path_falls_back_to_byte_col() {
        let mut mapper = PositionMapper::default();
        assert_eq!(
            mapper.utf16_col(Path::new("/definitely/missing.ts"), 0, 9),
            9
        );
    }
}
