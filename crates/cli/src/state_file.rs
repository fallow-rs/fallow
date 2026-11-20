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

/// Read `path` as bytes. A missing file is `Ok(None)`.
pub fn read_optional_bytes(path: &Path) -> std::io::Result<Option<Vec<u8>>> {
    match std::fs::read(path) {
        Ok(contents) => Ok(Some(contents)),
        Err(err) if err.kind() == ErrorKind::NotFound => Ok(None),
        Err(err) => Err(err),
    }
}

/// What a command found at the path of a file that it writes again.
#[derive(Debug)]
pub enum Existing<T> {
    /// No file is at the path. The command can create one.
    Absent,
    /// The file is there and the parser accepted it.
    Loaded(T),
    /// The file is there and the read failed. Invalid UTF-8 is a read
    /// failure.
    Unreadable(std::io::Error),
    /// The file is there and the parser refused it. The text is the reason.
    Invalid(String),
}

/// Read `path` and parse the text. A read path can map `Unreadable` and
/// `Invalid` to a default. A write path must stop or keep the file.
pub fn load_existing<T>(path: &Path, parse: impl FnOnce(&str) -> Result<T, String>) -> Existing<T> {
    match read_optional_text(path) {
        Ok(None) => Existing::Absent,
        Ok(Some(text)) => match parse(&text) {
            Ok(value) => Existing::Loaded(value),
            Err(reason) => Existing::Invalid(reason),
        },
        Err(err) => Existing::Unreadable(err),
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

    #[test]
    fn load_existing_reports_a_missing_file_as_absent() {
        let dir = tempdir().unwrap();
        let result = load_existing(&dir.path().join("absent.json"), |text| Ok(text.len()));
        assert!(matches!(result, Existing::Absent));
    }

    #[test]
    fn load_existing_returns_the_parsed_value() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("state.json");
        std::fs::write(&path, "abc").unwrap();
        let result = load_existing(&path, |text| Ok(text.len()));
        assert!(matches!(result, Existing::Loaded(3)));
    }

    #[test]
    fn load_existing_reports_a_refused_parse_as_invalid() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("state.json");
        std::fs::write(&path, "abc").unwrap();
        let result = load_existing(&path, |_| Err::<(), String>("bad".to_string()));
        assert!(matches!(result, Existing::Invalid(reason) if reason == "bad"));
    }

    #[test]
    fn load_existing_reports_invalid_utf8_as_unreadable() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("state.json");
        std::fs::write(&path, [0xff, 0xfe, 0xfd]).unwrap();
        let result = load_existing(&path, |_| -> Result<(), String> {
            panic!("the parser must not run")
        });
        assert!(
            matches!(result, Existing::Unreadable(err) if err.kind() == ErrorKind::InvalidData)
        );
    }

    #[test]
    fn read_optional_bytes_reads_invalid_utf8() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("state.json");
        std::fs::write(&path, [0xff, 0xfe, 0xfd]).unwrap();
        assert_eq!(
            read_optional_bytes(&path).unwrap(),
            Some(vec![0xff, 0xfe, 0xfd])
        );
    }

    #[test]
    fn read_optional_bytes_reports_a_missing_file_as_none() {
        let dir = tempdir().unwrap();
        assert_eq!(
            read_optional_bytes(&dir.path().join("absent")).unwrap(),
            None
        );
    }
}
