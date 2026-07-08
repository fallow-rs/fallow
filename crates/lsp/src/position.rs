//! Byte-column to UTF-16 conversion at the LSP protocol boundary.
//!
//! Analysis results carry 0-based byte columns. The LSP wire contract uses
//! UTF-16 code units, so conversion happens here and only here.

use std::path::{Path, PathBuf};

use rustc_hash::FxHashMap;

/// Lazily maps byte columns from analysis results into LSP UTF-16 columns.
#[derive(Default)]
pub struct PositionMapper {
    files: FxHashMap<PathBuf, Option<String>>,
}

impl PositionMapper {
    /// Convert a 0-based byte column on a 0-based line to a UTF-16 column.
    pub fn utf16_col(&mut self, path: &Path, line0: u32, byte_col: u32) -> u32 {
        let Some(content) = self.file_content(path) else {
            return byte_col;
        };
        byte_col_to_utf16(content, line0, byte_col)
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

    fn file_content(&mut self, path: &Path) -> Option<&str> {
        if !self.files.contains_key(path) {
            let content = std::fs::read_to_string(path).ok();
            self.files.insert(path.to_path_buf(), content);
        }
        self.files.get(path).and_then(Option::as_deref)
    }
}

fn byte_col_to_utf16(content: &str, line0: u32, byte_col: u32) -> u32 {
    let Some(line) = content.split('\n').nth(line0 as usize) else {
        return byte_col;
    };
    let mut col = (byte_col as usize).min(line.len());
    while col > 0 && !line.is_char_boundary(col) {
        col -= 1;
    }
    u32::try_from(line[..col].encode_utf16().count()).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

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
