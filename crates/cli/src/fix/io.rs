use std::path::Path;

/// Read a source file, validate it is within the project root, and detect line endings.
///
/// Returns `None` (with a warning) if the path is outside the project root or unreadable.
pub(super) fn read_source(root: &Path, path: &Path) -> Option<(String, &'static str)> {
    if !path.starts_with(root) {
        tracing::warn!(path = %path.display(), "Skipping fix for path outside project root");
        return None;
    }
    let content = std::fs::read_to_string(path).ok()?;
    let line_ending = if content.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    Some((content, line_ending))
}

/// Join modified lines, preserve the original trailing newline, and atomically write the result.
pub(super) fn write_fixed_content(
    path: &Path,
    lines: &[String],
    line_ending: &str,
    original_content: &str,
) -> std::io::Result<()> {
    let mut result = lines.join(line_ending);
    if original_content.ends_with(line_ending) && !result.ends_with(line_ending) {
        result.push_str(line_ending);
    }
    atomic_write(path, result.as_bytes())
}

pub(super) use fallow_config::atomic_write;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atomic_write_creates_file_with_content() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.ts");
        atomic_write(&path, b"hello world").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "hello world");
    }

    #[test]
    fn atomic_write_overwrites_existing_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.ts");
        std::fs::write(&path, "old content").unwrap();
        atomic_write(&path, b"new content").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "new content");
    }

    #[test]
    fn atomic_write_no_leftover_temp_on_success() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.ts");
        atomic_write(&path, b"data").unwrap();
        // Only the target file should exist — no stray temp files
        let entries: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(Result::ok)
            .collect();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].file_name(), "test.ts");
    }

    #[test]
    fn atomic_write_to_nonexistent_dir_fails() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nonexistent_dir").join("file.ts");
        let result = atomic_write(&path, b"content");
        assert!(result.is_err());
    }

    #[test]
    fn atomic_write_empty_content() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("empty.ts");
        atomic_write(&path, b"").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "");
    }

    #[test]
    fn atomic_write_binary_content() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("binary.dat");
        let data: Vec<u8> = (0..=255).collect();
        atomic_write(&path, &data).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), data);
    }

    // -- read_source tests ---------------------------------------------------

    #[test]
    fn read_source_returns_none_for_path_outside_root() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("project");
        std::fs::create_dir_all(&root).unwrap();
        let outside = dir.path().join("outside.ts");
        std::fs::write(&outside, "content").unwrap();

        let result = read_source(&root, &outside);
        assert!(result.is_none());
    }

    #[test]
    fn read_source_returns_none_for_nonexistent_file() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let missing = root.join("missing.ts");

        let result = read_source(root, &missing);
        assert!(result.is_none());
    }

    #[test]
    fn read_source_detects_lf_line_ending() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let file = root.join("lf.ts");
        std::fs::write(&file, "line1\nline2\n").unwrap();

        let (content, ending) = read_source(root, &file).unwrap();
        assert_eq!(ending, "\n");
        assert_eq!(content, "line1\nline2\n");
    }

    #[test]
    fn read_source_detects_crlf_line_ending() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let file = root.join("crlf.ts");
        std::fs::write(&file, "line1\r\nline2\r\n").unwrap();

        let (content, ending) = read_source(root, &file).unwrap();
        assert_eq!(ending, "\r\n");
        assert_eq!(content, "line1\r\nline2\r\n");
    }

    #[test]
    fn read_source_empty_file() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let file = root.join("empty.ts");
        std::fs::write(&file, "").unwrap();

        let (content, ending) = read_source(root, &file).unwrap();
        assert_eq!(content, "");
        assert_eq!(ending, "\n"); // defaults to LF when no line endings found
    }

    // -- write_fixed_content tests -------------------------------------------

    #[test]
    fn write_fixed_content_preserves_trailing_newline() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.ts");
        let lines = vec!["line1".to_string(), "line2".to_string()];
        let original = "line1\nline2\n";

        write_fixed_content(&path, &lines, "\n", original).unwrap();
        let content = std::fs::read_to_string(&path).unwrap();
        assert_eq!(content, "line1\nline2\n");
    }

    #[test]
    fn write_fixed_content_no_trailing_newline_when_original_has_none() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.ts");
        let lines = vec!["line1".to_string(), "line2".to_string()];
        let original = "line1\nline2";

        write_fixed_content(&path, &lines, "\n", original).unwrap();
        let content = std::fs::read_to_string(&path).unwrap();
        assert_eq!(content, "line1\nline2");
    }

    #[test]
    fn write_fixed_content_preserves_crlf_trailing_newline() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.ts");
        let lines = vec!["line1".to_string(), "line2".to_string()];
        let original = "line1\r\nline2\r\n";

        write_fixed_content(&path, &lines, "\r\n", original).unwrap();
        let content = std::fs::read_to_string(&path).unwrap();
        assert_eq!(content, "line1\r\nline2\r\n");
    }

    #[test]
    fn write_fixed_content_single_line() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.ts");
        let lines = vec!["only line".to_string()];
        let original = "only line\n";

        write_fixed_content(&path, &lines, "\n", original).unwrap();
        let content = std::fs::read_to_string(&path).unwrap();
        assert_eq!(content, "only line\n");
    }

    #[test]
    fn write_fixed_content_empty_lines() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.ts");
        let lines: Vec<String> = vec![];
        let original = "\n";

        write_fixed_content(&path, &lines, "\n", original).unwrap();
        let content = std::fs::read_to_string(&path).unwrap();
        assert_eq!(content, "\n");
    }
}
