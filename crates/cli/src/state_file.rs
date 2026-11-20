//! Readers for a file that a command reads and then writes again.
//!
//! Only a missing file means that the command can create one. A read error
//! or a parse error is a separate result, so a write path can stop before
//! it replaces data that it did not read.

use std::io::ErrorKind;
use std::path::Path;

/// Read `path` as text. A missing file is `Ok(None)`. Each other read
/// error, which includes invalid UTF-8, is `Err`.
pub fn read_optional_text(path: &Path) -> std::io::Result<Option<String>> {
    match std::fs::read_to_string(path) {
        Ok(contents) => Ok(Some(contents)),
        Err(err) if err.kind() == ErrorKind::NotFound => Ok(None),
        Err(err) => Err(err),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn a_missing_file_reads_as_none() {
        let dir = tempdir().unwrap();
        let read = read_optional_text(&dir.path().join("absent.json")).unwrap();
        assert!(read.is_none());
    }

    #[test]
    fn invalid_utf8_is_a_read_error() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("state.json");
        std::fs::write(&path, [0xff, 0xfe, 0xfd]).unwrap();

        let err = read_optional_text(&path).expect_err("invalid UTF-8 should error");
        assert_eq!(err.kind(), ErrorKind::InvalidData);
    }
}
