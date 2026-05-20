//! Validation of user-supplied glob patterns from the config file.
//!
//! Fallow accepts glob patterns in several config fields (`entry`,
//! `ignorePatterns`, `dynamicallyLoaded`, `duplicates.ignore`, `health.ignore`,
//! `boundaries.zones[].patterns`, `overrides[].files`, `ignoreExports[].file`,
//! `ignoreCatalogReferences[].consumer`). All of these are matched against
//! project-root-relative file paths. The matcher cannot reach outside the
//! project root by construction, but a malicious config can still slip in
//! absolute paths or `..` traversal segments that silently no-op today and
//! mask user intent.
//!
//! This module rejects such patterns at config-load time so users get a clear
//! error instead of a silent no-match. Invalid glob syntax also fails loud
//! here, replacing the historical `if let Ok(glob) = Glob::new(pattern)` drop
//! patterns scattered across the codebase.
//!
//! See issue #463 for the threat model.

use std::fmt;
use std::path::{Component, Path};

use globset::Glob;

/// Validation failure for a single user-supplied glob pattern.
#[derive(Debug)]
pub enum GlobValidationError {
    /// Pattern is an absolute path (`/foo`, `\foo`, `C:\foo`, `\\share`).
    AbsolutePath {
        field: &'static str,
        pattern: String,
    },
    /// Pattern contains a `..` path segment.
    TraversalSegment {
        field: &'static str,
        pattern: String,
    },
    /// Pattern is not valid glob syntax.
    InvalidSyntax {
        field: &'static str,
        pattern: String,
        source: globset::Error,
    },
}

impl fmt::Display for GlobValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AbsolutePath { field, pattern } => {
                write!(
                    f,
                    "{field}: '{pattern}' is an absolute path; \
                     use a pattern relative to the project root (e.g. 'src/**')"
                )
            }
            Self::TraversalSegment { field, pattern } => {
                write!(
                    f,
                    "{field}: '{pattern}' contains a '..' segment; \
                     rewrite the pattern to stay inside the project root, \
                     or run fallow with --root pointing at the directory you want to scan"
                )
            }
            Self::InvalidSyntax {
                field,
                pattern,
                source,
            } => {
                // `globset::Error`'s Display re-quotes the pattern, so strip
                // the `error parsing glob '...': ` prefix to avoid showing
                // the pattern twice. The kind tail (e.g. "unclosed character
                // class; missing ']'") is the actionable bit.
                let source_msg = source.to_string();
                let tail = source_msg
                    .find("': ")
                    .map_or(source_msg.as_str(), |idx| &source_msg[idx + 3..]);
                write!(
                    f,
                    "{field}: invalid glob '{pattern}': {tail}; \
                     fix the syntax (see https://docs.rs/globset for the supported grammar)"
                )
            }
        }
    }
}

impl std::error::Error for GlobValidationError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::InvalidSyntax { source, .. } => Some(source),
            _ => None,
        }
    }
}

/// Detect absolute paths cross-platform without relying on `Path::is_absolute`
/// (which is platform-specific: on Unix, `C:\foo` would be treated as relative).
///
/// Rejected shapes:
/// - Unix root: `/foo`
/// - Windows backslash root: `\foo`
/// - UNC: `\\share\path` or `//share/path`
/// - Drive letter: `C:\foo`, `c:/foo`, `D:foo`
fn is_absolute_pattern(pattern: &str) -> bool {
    if pattern.starts_with('/') || pattern.starts_with('\\') {
        return true;
    }
    let bytes = pattern.as_bytes();
    if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
        return true;
    }
    false
}

/// Return `true` if any segment of `pattern` is `..`.
///
/// We split on BOTH `/` and `\` so a backslash-separated traversal pattern
/// (`..\foo`) authored on a Windows machine is rejected even when fallow runs
/// on Unix. `Path::components` on Unix treats `\` as a regular character, so
/// it cannot be relied on as a cross-platform separator detector.
///
/// Glob meta characters (`*`, `**`, `[abc]`, `{a,b}`) pass through unchanged
/// because the split only inspects separators.
fn has_traversal_segment(pattern: &str) -> bool {
    pattern.split(['/', '\\']).any(|seg| seg == "..")
        || Path::new(pattern)
            .components()
            .any(|c| matches!(c, Component::ParentDir))
}

/// Validate that `pattern` is a relative, non-traversal, syntactically valid
/// glob; return the compiled glob on success.
///
/// `field` is the dotted-path name of the config field the pattern came from
/// (e.g. `"entry"`, `"ignorePatterns"`, `"duplicates.ignore"`); it appears
/// verbatim in the error message so users can locate the bad value.
///
/// # Errors
///
/// Returns:
/// - `AbsolutePath` if the pattern is rooted at `/`, `\`, `\\`, `//`, or a
///   Windows drive letter
/// - `TraversalSegment` if any path segment of the pattern is `..`
/// - `InvalidSyntax` if `globset::Glob::new` rejects the pattern
pub fn compile_user_glob(pattern: &str, field: &'static str) -> Result<Glob, GlobValidationError> {
    if is_absolute_pattern(pattern) {
        return Err(GlobValidationError::AbsolutePath {
            field,
            pattern: pattern.to_owned(),
        });
    }
    if has_traversal_segment(pattern) {
        return Err(GlobValidationError::TraversalSegment {
            field,
            pattern: pattern.to_owned(),
        });
    }
    Glob::new(pattern).map_err(|source| GlobValidationError::InvalidSyntax {
        field,
        pattern: pattern.to_owned(),
        source,
    })
}

/// Validate a slice of patterns, accumulating ALL errors so the user sees
/// every offending pattern in one run rather than fixing them one at a time.
pub fn validate_user_globs(
    patterns: &[String],
    field: &'static str,
    errors: &mut Vec<GlobValidationError>,
) {
    for pattern in patterns {
        if let Err(e) = compile_user_glob(pattern, field) {
            errors.push(e);
        }
    }
}

/// Validate a user-supplied DIRECTORY PATH (not a glob). Same absolute-path
/// and traversal checks as `compile_user_glob`, but skips the glob-syntax
/// check because the value is a literal path, not a pattern.
///
/// Used for fields like `boundaries.zones[].root` and
/// `boundaries.zones[].autoDiscover` that name a directory subtree rather
/// than a match pattern.
///
/// # Errors
///
/// Returns `AbsolutePath` or `TraversalSegment` for the same shapes
/// `compile_user_glob` rejects. Never returns `InvalidSyntax`.
pub fn validate_user_path(path: &str, field: &'static str) -> Result<(), GlobValidationError> {
    if is_absolute_pattern(path) {
        return Err(GlobValidationError::AbsolutePath {
            field,
            pattern: path.to_owned(),
        });
    }
    if has_traversal_segment(path) {
        return Err(GlobValidationError::TraversalSegment {
            field,
            pattern: path.to_owned(),
        });
    }
    Ok(())
}

/// Same as `validate_user_path` but accumulates errors over a slice.
pub fn validate_user_paths(
    paths: &[String],
    field: &'static str,
    errors: &mut Vec<GlobValidationError>,
) {
    for path in paths {
        if let Err(e) = validate_user_path(path, field) {
            errors.push(e);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_glob_accepted() {
        assert!(compile_user_glob("src/**/*.ts", "entry").is_ok());
        assert!(compile_user_glob("**/*.test.ts", "entry").is_ok());
        assert!(compile_user_glob("./src/main.ts", "entry").is_ok());
        assert!(compile_user_glob("packages/*/src/index.ts", "entry").is_ok());
        assert!(compile_user_glob("**/{a,b}.ts", "entry").is_ok());
    }

    #[test]
    fn bracket_character_class_accepted() {
        // Library authors use bracket character classes for PascalCase
        // component file globs (`[A-Z]*.tsx`). Make sure the validator
        // doesn't confuse a legitimate `[A-Z]` opening with an unclosed
        // class. See user-panel review (Aisha's case).
        assert!(compile_user_glob("[A-Z]*.tsx", "entry").is_ok());
        assert!(compile_user_glob("src/**/[A-Z]*.{ts,tsx}", "ignoreExports[].file").is_ok());
        assert!(compile_user_glob("**/[0-9][0-9]*.md", "entry").is_ok());
    }

    #[test]
    fn validate_user_path_rejects_traversal_and_absolute() {
        assert!(validate_user_path("../escape", "boundaries.zones[].root").is_err());
        assert!(validate_user_path("/abs/dir", "boundaries.zones[].root").is_err());
        assert!(validate_user_path("packages/ui", "boundaries.zones[].root").is_ok());
        // Non-glob paths skip syntax check, so `[abc]` is fine as a literal name.
        assert!(validate_user_path("[brackets-literal]/dir", "boundaries.zones[].root").is_ok());
    }

    #[test]
    fn absolute_unix_path_rejected() {
        let err = compile_user_glob("/etc/passwd", "entry").unwrap_err();
        assert!(matches!(err, GlobValidationError::AbsolutePath { .. }));
        let msg = err.to_string();
        assert!(msg.contains("/etc/passwd"), "msg: {msg}");
        assert!(msg.contains("entry"), "msg: {msg}");
        assert!(msg.contains("absolute"), "msg: {msg}");
        assert!(msg.contains("relative to the project root"), "msg: {msg}");
    }

    #[test]
    fn absolute_unix_glob_rejected() {
        let err = compile_user_glob("/root/.ssh/**", "ignorePatterns").unwrap_err();
        assert!(matches!(err, GlobValidationError::AbsolutePath { .. }));
    }

    #[test]
    fn absolute_windows_backslash_path_rejected() {
        let err = compile_user_glob("\\Windows\\System32", "entry").unwrap_err();
        assert!(matches!(err, GlobValidationError::AbsolutePath { .. }));
    }

    #[test]
    fn unc_path_rejected() {
        let err = compile_user_glob("\\\\share\\secrets", "entry").unwrap_err();
        assert!(matches!(err, GlobValidationError::AbsolutePath { .. }));
    }

    #[test]
    fn unc_forward_slash_rejected() {
        let err = compile_user_glob("//share/secrets", "entry").unwrap_err();
        assert!(matches!(err, GlobValidationError::AbsolutePath { .. }));
    }

    #[test]
    fn windows_drive_letter_rejected() {
        for pat in ["C:\\Users", "c:/Users", "D:foo", "Z:\\"] {
            let err = compile_user_glob(pat, "entry").unwrap_err();
            assert!(
                matches!(err, GlobValidationError::AbsolutePath { .. }),
                "expected AbsolutePath for {pat}, got {err:?}"
            );
        }
    }

    #[test]
    fn traversal_segment_rejected() {
        let err = compile_user_glob("../foo", "entry").unwrap_err();
        assert!(matches!(err, GlobValidationError::TraversalSegment { .. }));
        assert!(err.to_string().contains("../foo"));
    }

    #[test]
    fn traversal_in_middle_rejected() {
        let err = compile_user_glob("src/../../../etc", "ignorePatterns").unwrap_err();
        assert!(matches!(err, GlobValidationError::TraversalSegment { .. }));
    }

    #[test]
    fn traversal_with_backslash_rejected() {
        let err = compile_user_glob("..\\foo", "entry").unwrap_err();
        assert!(matches!(err, GlobValidationError::TraversalSegment { .. }));
    }

    #[test]
    fn traversal_in_glob_pattern_rejected() {
        let err = compile_user_glob("**/../secrets", "entry").unwrap_err();
        assert!(matches!(err, GlobValidationError::TraversalSegment { .. }));
    }

    #[test]
    fn double_dot_filename_accepted() {
        // `..` is a path-segment marker; `foo..bar` is a regular filename
        // (extension separator) and must NOT be flagged.
        assert!(compile_user_glob("foo..bar", "entry").is_ok());
        assert!(compile_user_glob("src/file.with..dots.ts", "entry").is_ok());
    }

    #[test]
    fn current_dir_dot_accepted() {
        // `./` is a no-op prefix; `Component::CurDir`, not `ParentDir`.
        assert!(compile_user_glob("./src/**", "entry").is_ok());
    }

    #[test]
    fn invalid_glob_syntax_rejected() {
        let err = compile_user_glob("[invalid", "entry").unwrap_err();
        assert!(matches!(err, GlobValidationError::InvalidSyntax { .. }));
        let msg = err.to_string();
        assert!(msg.contains("entry"), "msg: {msg}");
        // Pattern appears once (inside `'[invalid'`), not twice.
        assert_eq!(msg.matches("[invalid").count(), 1, "msg: {msg}");
        assert!(msg.contains("unclosed character class"), "msg: {msg}");
    }

    #[test]
    fn empty_pattern_accepted_as_globset_handles_it() {
        // globset accepts the empty pattern (matches the empty string); we
        // pass it through rather than special-casing here. The downstream
        // matcher will never see an empty relative path so the practical
        // effect is a no-op.
        assert!(compile_user_glob("", "entry").is_ok());
    }

    #[test]
    fn validate_user_globs_collects_all_errors() {
        let patterns = vec![
            "src/**".to_owned(),
            "../foo".to_owned(),
            "/abs".to_owned(),
            "[bad".to_owned(),
            "**/*.ts".to_owned(),
        ];
        let mut errors = Vec::new();
        validate_user_globs(&patterns, "ignorePatterns", &mut errors);
        assert_eq!(errors.len(), 3);
        assert!(matches!(
            errors[0],
            GlobValidationError::TraversalSegment { .. }
        ));
        assert!(matches!(
            errors[1],
            GlobValidationError::AbsolutePath { .. }
        ));
        assert!(matches!(
            errors[2],
            GlobValidationError::InvalidSyntax { .. }
        ));
    }

    #[test]
    fn field_name_in_error_message() {
        let err = compile_user_glob("../oops", "duplicates.ignore").unwrap_err();
        assert!(err.to_string().starts_with("duplicates.ignore:"));
    }
}
