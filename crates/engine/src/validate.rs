use std::path::PathBuf;

/// Validate a user-supplied git ref before it reaches a git subprocess:
/// rejects empty refs, option-like refs starting with `-`, and characters
/// outside the ref-syntax allowlist.
///
/// # Errors
///
/// Returns a message describing why the ref was rejected.
pub fn validate_git_ref(s: &str) -> Result<&str, String> {
    crate::changed_files::validate_git_ref(s)
}

/// Canonicalize a project root and require it to be an existing directory.
///
/// # Errors
///
/// Returns a message when the path cannot be canonicalized or is not a
/// directory.
pub fn validate_root(root: &std::path::Path) -> Result<PathBuf, String> {
    let canonical = dunce::canonicalize(root)
        .map_err(|e| format!("invalid root path '{}': {e}", root.display()))?;
    if !canonical.is_dir() {
        return Err(format!("root path '{}' is not a directory", root.display()));
    }
    Ok(canonical)
}

/// Reject strings containing control characters (bytes < 0x20) except
/// newline (0x0A) and tab (0x09). This prevents agents from accidentally
/// passing invisible characters in CLI arguments.
pub fn validate_no_control_chars(s: &str, arg_name: &str) -> Result<(), String> {
    for (i, byte) in s.bytes().enumerate() {
        if byte < 0x20 && byte != b'\n' && byte != b'\t' {
            return Err(format!(
                "{arg_name} contains control character (byte 0x{byte:02x}) at position {i}"
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn control_chars_rejects_bytes_below_space_except_newline_and_tab() {
        for input in [
            "test\x07ref",
            "\x1b[31mred",
            "main\rinjected",
            "abc\x0cdef",
            "abc\x08def",
        ] {
            assert!(
                validate_no_control_chars(input, "--arg").is_err(),
                "{input:?} must be rejected"
            );
        }
    }

    #[test]
    fn control_chars_allows_printable_text_newline_and_tab() {
        for input in [
            "main",
            "line1\nline2",
            "col1\tcol2",
            "",
            "my-package-日本語",
            "./path/to/config.toml",
            "hello world",
        ] {
            assert_eq!(
                validate_no_control_chars(input, "--arg"),
                Ok(()),
                "{input:?} must be accepted"
            );
        }
    }

    #[test]
    fn git_ref_rejects_shell_metacharacters_and_option_like_refs() {
        for input in [
            "main;rm -rf /",
            "main`whoami`",
            "main$HOME",
            "main|cat /etc/passwd",
            "main&&echo pwned",
            "$(whoami)",
            "--upload-pack=evil",
            "-flag",
        ] {
            assert!(
                validate_git_ref(input).is_err(),
                "{input:?} must be rejected"
            );
        }
    }

    #[test]
    fn git_ref_allows_ref_syntax_and_reflog_selectors() {
        for input in [
            "main",
            "feature/my-branch",
            "HEAD~3",
            "HEAD^2",
            "abc123def456",
            "v1.2.3",
            "feature_branch",
            "HEAD@{0}~3",
            "origin/main@{0}",
            "HEAD@{2025-01-01}",
            "HEAD@{1 week ago}",
            "HEAD@{3 days ago}",
        ] {
            assert_eq!(
                validate_git_ref(input),
                Ok(input),
                "{input:?} must be accepted"
            );
        }
    }

    #[test]
    fn control_chars_rejects_null_byte() {
        let result = validate_no_control_chars("main\x00branch", "--changed-since");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.contains("0x00"));
        assert!(err.contains("--changed-since"));
    }

    #[test]
    fn git_ref_rejects_unclosed_brace() {
        let result = validate_git_ref("HEAD@{");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.contains("unclosed"),
            "Error should mention unclosed brace, got: {err}"
        );
    }

    #[test]
    fn git_ref_rejects_colon_outside_braces() {
        let result = validate_git_ref("HEAD:file.txt");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.contains("disallowed character"),
            "Error should mention disallowed character, got: {err}"
        );
        assert!(
            err.contains(':'),
            "Error should mention the colon, got: {err}"
        );
    }

    #[test]
    fn git_ref_rejects_space_outside_braces() {
        let result = validate_git_ref("some ref");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.contains("disallowed character"),
            "Error should mention disallowed character, got: {err}"
        );
    }

    #[test]
    fn git_ref_rejects_empty() {
        let result = validate_git_ref("");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("empty"));
    }

    #[test]
    fn git_ref_rejects_leading_dash() {
        let result = validate_git_ref("--evil-flag");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("start with '-'"));
    }

    #[test]
    fn validate_root_nonexistent_path() {
        let result = validate_root(std::path::Path::new(
            "/nonexistent/path/that/does/not/exist",
        ));
        assert!(result.is_err());
    }

    #[test]
    fn validate_root_valid_dir() {
        let temp = std::env::temp_dir();
        let result = validate_root(&temp);
        assert!(result.is_ok());
    }

    #[test]
    fn control_chars_error_includes_position() {
        let result = validate_no_control_chars("ab\x01cd", "--test");
        let err = result.unwrap_err();
        assert!(err.contains("position 2"), "got: {err}");
        assert!(err.contains("--test"), "got: {err}");
    }
}
