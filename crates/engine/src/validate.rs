use std::path::PathBuf;

pub fn validate_git_ref(s: &str) -> Result<&str, String> {
    crate::validate_git_ref(s)
}

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
    fn control_chars_rejects_null_byte() {
        let result = validate_no_control_chars("main\x00branch", "--changed-since");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.contains("0x00"));
        assert!(err.contains("--changed-since"));
    }

    #[test]
    fn control_chars_rejects_bell() {
        assert!(validate_no_control_chars("test\x07ref", "--workspace").is_err());
    }

    #[test]
    fn control_chars_rejects_escape() {
        assert!(validate_no_control_chars("\x1b[31mred", "--config").is_err());
    }

    #[test]
    fn control_chars_rejects_carriage_return() {
        assert!(validate_no_control_chars("main\rinjected", "--changed-since").is_err());
    }

    #[test]
    fn control_chars_allows_normal_text() {
        assert!(validate_no_control_chars("main", "--changed-since").is_ok());
    }

    #[test]
    fn control_chars_allows_newline() {
        assert!(validate_no_control_chars("line1\nline2", "--config").is_ok());
    }

    #[test]
    fn control_chars_allows_tab() {
        assert!(validate_no_control_chars("col1\tcol2", "--config").is_ok());
    }

    #[test]
    fn control_chars_allows_empty_string() {
        assert!(validate_no_control_chars("", "--workspace").is_ok());
    }

    #[test]
    fn control_chars_allows_unicode() {
        assert!(validate_no_control_chars("my-package-日本語", "--workspace").is_ok());
    }

    #[test]
    fn control_chars_allows_paths_with_dots_and_slashes() {
        assert!(validate_no_control_chars("./path/to/config.toml", "--config").is_ok());
    }

    #[test]
    fn git_ref_allows_reflog_timestamp() {
        assert_eq!(
            validate_git_ref("HEAD@{2025-01-01}").unwrap(),
            "HEAD@{2025-01-01}"
        );
    }

    #[test]
    fn git_ref_allows_reflog_relative_date() {
        assert_eq!(
            validate_git_ref("HEAD@{1 week ago}").unwrap(),
            "HEAD@{1 week ago}"
        );
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
    fn git_ref_allows_reflog_index() {
        assert_eq!(
            validate_git_ref("origin/main@{0}").unwrap(),
            "origin/main@{0}"
        );
    }

    #[test]
    fn git_ref_allows_simple_branch_names() {
        assert_eq!(validate_git_ref("main").unwrap(), "main");
        assert_eq!(
            validate_git_ref("feature/my-branch").unwrap(),
            "feature/my-branch"
        );
    }

    #[test]
    fn git_ref_allows_head_tilde_caret() {
        assert_eq!(validate_git_ref("HEAD~3").unwrap(), "HEAD~3");
        assert_eq!(validate_git_ref("HEAD^2").unwrap(), "HEAD^2");
    }

    #[test]
    fn git_ref_allows_commit_sha() {
        assert_eq!(validate_git_ref("abc123def456").unwrap(), "abc123def456");
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
    fn git_ref_allows_multiple_braces_segments() {
        assert!(validate_git_ref("HEAD@{0}~3").is_ok());
    }

    #[test]
    fn git_ref_allows_space_in_complex_reflog() {
        assert_eq!(
            validate_git_ref("HEAD@{3 days ago}").unwrap(),
            "HEAD@{3 days ago}"
        );
    }

    #[test]
    fn git_ref_rejects_semicolon() {
        let result = validate_git_ref("main;rm -rf /");
        assert!(result.is_err());
    }

    #[test]
    fn git_ref_rejects_backtick() {
        let result = validate_git_ref("main`whoami`");
        assert!(result.is_err());
    }

    #[test]
    fn git_ref_rejects_dollar_sign() {
        let result = validate_git_ref("main$HOME");
        assert!(result.is_err());
    }

    #[test]
    fn git_ref_rejects_pipe() {
        let result = validate_git_ref("main|cat /etc/passwd");
        assert!(result.is_err());
    }

    #[test]
    fn git_ref_rejects_ampersand() {
        let result = validate_git_ref("main&&echo pwned");
        assert!(result.is_err());
    }

    #[test]
    fn git_ref_rejects_parentheses() {
        let result = validate_git_ref("$(whoami)");
        assert!(result.is_err());
    }

    #[test]
    fn git_ref_allows_dots_in_branch() {
        assert_eq!(validate_git_ref("v1.2.3").unwrap(), "v1.2.3");
    }

    #[test]
    fn git_ref_allows_underscores() {
        assert_eq!(
            validate_git_ref("feature_branch").unwrap(),
            "feature_branch"
        );
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
    fn control_chars_rejects_form_feed() {
        assert!(validate_no_control_chars("abc\x0cdef", "--arg").is_err());
    }

    #[test]
    fn control_chars_rejects_backspace() {
        assert!(validate_no_control_chars("abc\x08def", "--arg").is_err());
    }

    #[test]
    fn control_chars_allows_space() {
        assert!(validate_no_control_chars("hello world", "--arg").is_ok());
    }

    #[test]
    fn control_chars_error_includes_position() {
        let result = validate_no_control_chars("ab\x01cd", "--test");
        let err = result.unwrap_err();
        assert!(err.contains("position 2"), "got: {err}");
        assert!(err.contains("--test"), "got: {err}");
    }
}
