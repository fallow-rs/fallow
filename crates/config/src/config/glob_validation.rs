//! Validation of user-supplied glob patterns from the config file.
//!
//! Fallow accepts filesystem glob patterns in several config fields (`entry`,
//! `ignorePatterns`, `ignoreFindings`, `dynamicallyLoaded`, `duplicates.ignore`, `health.ignore`,
//! `health.thresholdOverrides[].files`, `boundaries.zones[].patterns`,
//! `overrides[].files`, `ignoreExports[].file`, `ignoreCatalogReferences[].consumer`).
//! All of these are matched against
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

use std::borrow::Cow;
use std::fmt;
use std::path::{Component, Path};

use globset::{Glob, GlobBuilder};

use super::finding_ignore::FindingIgnoreMatcher;

/// Validation failure for user-supplied glob configuration.
#[derive(Debug)]
pub enum GlobValidationError {
    /// Pattern is an absolute path (`/foo`, `\foo`, `C:\foo`, `\\share`).
    AbsolutePath {
        /// Config field the pattern came from (e.g. `entry`), named in the error.
        field: &'static str,
        /// The offending pattern as written in the config.
        pattern: String,
    },
    /// Pattern contains a `..` path segment.
    TraversalSegment {
        /// Config field the pattern came from, named in the error.
        field: &'static str,
        /// The offending pattern as written in the config.
        pattern: String,
    },
    /// Pattern is not valid glob syntax.
    InvalidSyntax {
        /// Config field the pattern came from, named in the error.
        field: &'static str,
        /// The offending pattern as written in the config.
        pattern: String,
        /// Underlying glob parse error.
        source: globset::Error,
    },
    /// A finding-ignore exception contains `!` without a pattern body.
    EmptyNegation {
        /// Config field the pattern came from, named in the error.
        field: &'static str,
        /// The offending pattern as written in the config.
        pattern: String,
    },
    /// An `ignorePatterns` exception names a `node_modules` or `.git`
    /// segment, which no exception can lift.
    UnliftableNegation {
        /// Config field the pattern came from, named in the error.
        field: &'static str,
        /// The offending pattern as written in the config.
        pattern: String,
    },
    /// A workspace root key is not written as an exact root: it has an empty
    /// or `.` segment, a trailing slash, or a backslash.
    InexactWorkspaceRoot {
        /// Config field the key came from, named in the error.
        field: &'static str,
        /// The offending key as written in the config.
        pattern: String,
    },
    /// Individually valid patterns cannot be compiled into one matcher.
    PatternSetCompilation {
        /// Config field whose pattern set failed to build, named in the error.
        field: &'static str,
        /// Underlying glob-set build error.
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
            Self::EmptyNegation { field, pattern } => write!(
                f,
                "{field}: invalid glob '{pattern}': a negated pattern requires a pattern after '!'"
            ),
            Self::UnliftableNegation { field, pattern } => write!(
                f,
                "{field}: '{pattern}' can not be lifted: fallow never analyzes files under \
                 node_modules or .git; remove this entry"
            ),
            Self::InexactWorkspaceRoot { field, pattern } => write!(
                f,
                "{field}: '{pattern}' is not an exact workspace root; write it relative to the \
                 project root with '/' separators, as `fallow list --workspaces` prints it \
                 (e.g. 'packages/web')"
            ),
            Self::PatternSetCompilation { field, source } => write!(
                f,
                "{field}: glob patterns cannot be compiled together: {source}; simplify the pattern set"
            ),
        }
    }
}

impl std::error::Error for GlobValidationError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::InvalidSyntax { source, .. } | Self::PatternSetCompilation { source, .. } => {
                Some(source)
            }
            Self::AbsolutePath { .. }
            | Self::TraversalSegment { .. }
            | Self::EmptyNegation { .. }
            | Self::UnliftableNegation { .. }
            | Self::InexactWorkspaceRoot { .. } => None,
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

/// Compile a glob that matches project-root-relative file paths.
///
/// A single `*` stays within one path segment, and only `**` crosses
/// directories. This is the rule of picomatch and the plugin entry patterns.
/// The glob matches file paths only, not directories, so `src/generated/*`
/// does not match the files in the subfolders of `src/generated` (gitignore
/// ignores them, because it skips a matched directory). Every path glob that a user writes in the config compiles
/// through this function, so all of these fields match a path the same way.
///
/// # Errors
///
/// Returns the `globset` error when the pattern is not valid glob syntax.
pub fn compile_path_glob(pattern: &str) -> Result<Glob, globset::Error> {
    GlobBuilder::new(pattern).literal_separator(true).build()
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
/// - `InvalidSyntax` if [`compile_path_glob`] rejects the pattern
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
    compile_path_glob(pattern).map_err(|source| GlobValidationError::InvalidSyntax {
        field,
        pattern: pattern.to_owned(),
        source,
    })
}

/// Validate a glob pattern that matches a raw import specifier, not a
/// filesystem path.
///
/// Specifiers such as `../generated/foo` are valid import strings, so this
/// intentionally skips the absolute-path and traversal-segment checks used for
/// project-root-relative file globs.
///
/// # Errors
///
/// Returns `InvalidSyntax` if `globset::Glob::new` rejects the pattern.
pub fn compile_user_specifier_glob(
    pattern: &str,
    field: &'static str,
) -> Result<Glob, GlobValidationError> {
    Glob::new(pattern).map_err(|source| GlobValidationError::InvalidSyntax {
        field,
        pattern: pattern.to_owned(),
        source,
    })
}

/// Validate a slice of import-specifier patterns, accumulating syntax errors.
pub fn validate_user_specifier_globs(
    patterns: &[String],
    field: &'static str,
    errors: &mut Vec<GlobValidationError>,
) {
    for pattern in patterns {
        if let Err(e) = compile_user_specifier_glob(pattern, field) {
            errors.push(e);
        }
    }
}

/// Validate the glob entries of `ignoreDependencies`, accumulating syntax
/// errors. Entries without glob metacharacters are exact package names and
/// are not validated as globs.
pub fn validate_user_dependency_globs(
    entries: &[String],
    field: &'static str,
    errors: &mut Vec<GlobValidationError>,
) {
    for entry in entries {
        if super::is_dependency_glob(entry)
            && let Err(e) = compile_user_specifier_glob(entry, field)
        {
            errors.push(e);
        }
    }
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

/// Validate `ignorePatterns`, treating a leading `!` as an exception that
/// lifts an ignore (issue #2940). An exception body must be a valid glob and
/// must not name a `node_modules` or `.git` segment, because those paths can
/// never be lifted.
pub fn validate_user_ignore_pattern_globs(
    patterns: &[String],
    field: &'static str,
    errors: &mut Vec<GlobValidationError>,
) {
    for pattern in patterns {
        let Some(body) = pattern.strip_prefix('!') else {
            if let Err(error) = compile_user_glob(pattern, field) {
                errors.push(error);
            }
            continue;
        };
        if body.is_empty() {
            errors.push(GlobValidationError::EmptyNegation {
                field,
                pattern: pattern.clone(),
            });
            continue;
        }
        let unliftable = body
            .split(['/', '\\'])
            .any(|segment| super::ignore_patterns::UNLIFTABLE_IGNORE_SEGMENTS.contains(&segment));
        if unliftable {
            errors.push(GlobValidationError::UnliftableNegation {
                field,
                pattern: pattern.clone(),
            });
            continue;
        }
        if let Err(error) = compile_user_glob(body, field) {
            errors.push(error);
        }
    }
}

/// Validate finding-ignore patterns, treating a leading `!` as a report
/// exception, validating each project-relative body, and proving both matcher
/// sets can be compiled together.
///
/// An empty body is only an error after an explicit `!`. A bare empty pattern is
/// a glob that matches nothing, which `ignorePatterns` already accepts.
pub fn validate_user_finding_ignore_globs(
    patterns: &[String],
    field: &'static str,
    errors: &mut Vec<GlobValidationError>,
) {
    let initial_error_count = errors.len();
    for pattern in patterns {
        let negated_body = pattern.strip_prefix('!');
        if negated_body.is_some_and(str::is_empty) {
            errors.push(GlobValidationError::EmptyNegation {
                field,
                pattern: pattern.clone(),
            });
            continue;
        }
        if let Err(error) = compile_user_glob(negated_body.unwrap_or(pattern.as_str()), field) {
            errors.push(error);
        }
    }

    if errors.len() == initial_error_count
        && let Err(source) = FindingIgnoreMatcher::validate_compilation(patterns)
    {
        errors.push(GlobValidationError::PatternSetCompilation { field, source });
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

/// Rewrites a validated, root-relative glob for expansion on disk with the
/// `glob` crate.
///
/// The `glob` crate also tests the special entries `.` and `..` against a path
/// segment that starts with a literal `.`. Thus `.*/x` expands to `../x`, and
/// a pattern that passed validation can name a file outside the root. A `[.]`
/// class matches the same directory entries as a leading `.`, but the crate
/// never tests the special entries against it. Segments without a wildcard
/// stay unchanged, so a literal `.` or `.git` segment keeps its meaning.
#[must_use]
pub fn disk_glob_pattern(pattern: &str) -> Cow<'_, str> {
    let needs_rewrite = |segment: &str| {
        let body = segment.trim_end_matches(['/', '\\']);
        body.starts_with('.') && body.contains(['*', '?', '['])
    };
    if !pattern.split_inclusive(['/', '\\']).any(needs_rewrite) {
        return Cow::Borrowed(pattern);
    }
    let mut rewritten = String::with_capacity(pattern.len() + 4);
    for segment in pattern.split_inclusive(['/', '\\']) {
        if needs_rewrite(segment) {
            rewritten.push_str("[.]");
            rewritten.push_str(&segment[1..]);
        } else {
            rewritten.push_str(segment);
        }
    }
    Cow::Owned(rewritten)
}

/// Whether `key` is written as an exact, project-relative workspace root:
/// non-empty, relative, `/`-separated, with no empty, `.` or `..` segment.
#[must_use]
pub fn is_exact_workspace_root(key: &str) -> bool {
    !key.is_empty()
        && !key.contains('\\')
        && !is_absolute_pattern(key)
        && key
            .split('/')
            .all(|segment| !segment.is_empty() && segment != "." && segment != "..")
}

/// Validate a workspace root key, such as a `workspaces.changedSince` key.
///
/// # Errors
///
/// Returns `AbsolutePath` or `TraversalSegment` for the shapes that
/// [`validate_user_path`] rejects, and `InexactWorkspaceRoot` for any other key
/// that [`is_exact_workspace_root`] rejects.
pub fn validate_workspace_root_key(
    key: &str,
    field: &'static str,
) -> Result<(), GlobValidationError> {
    validate_user_path(key, field)?;
    if is_exact_workspace_root(key) {
        Ok(())
    } else {
        Err(GlobValidationError::InexactWorkspaceRoot {
            field,
            pattern: key.to_owned(),
        })
    }
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
    fn disk_glob_pattern_rewrites_only_leading_dot_wildcard_segments() {
        assert_eq!(disk_glob_pattern(".*/x"), "[.]*/x");
        assert_eq!(disk_glob_pattern("a/..*/b"), "a/[.].*/b");
        assert_eq!(disk_glob_pattern("a\\.?\\b"), "a\\[.]?\\b");
        assert_eq!(disk_glob_pattern("src/.[a-z]*"), "src/[.][a-z]*");
        for unchanged in ["./src/*.ts", ".git/config", "src/**/*.ts", "a.*", "**/.env"] {
            assert!(matches!(disk_glob_pattern(unchanged), Cow::Borrowed(p) if p == unchanged));
        }
    }

    #[test]
    fn disk_glob_pattern_never_expands_outside_root() {
        let base = tempfile::tempdir().expect("create temp dir");
        let root = base.path().join("root");
        std::fs::create_dir_all(root.join(".hidden")).expect("create root dirs");
        std::fs::write(base.path().join("secret"), "").expect("write secret");
        std::fs::write(root.join(".hidden").join("secret"), "").expect("write inside");

        for pattern in [".*/secret", "..*/secret", ".?/secret", ".[.]/secret"] {
            validate_user_path(pattern, "test").expect("pattern passes validation");
            let full = root.join(disk_glob_pattern(pattern).as_ref());
            let matches: Vec<_> = glob::glob(&full.to_string_lossy())
                .expect("valid glob")
                .filter_map(Result::ok)
                .collect();
            assert!(
                matches.iter().all(|path| !path
                    .components()
                    .any(|c| matches!(c, Component::ParentDir | Component::CurDir))),
                "{pattern} expanded outside the root: {matches:?}"
            );
        }
        let full = root.join(disk_glob_pattern(".*/secret").as_ref());
        let matches: Vec<_> = glob::glob(&full.to_string_lossy())
            .expect("valid glob")
            .filter_map(Result::ok)
            .collect();
        assert_eq!(matches, vec![root.join(".hidden").join("secret")]);
    }

    #[test]
    fn relative_glob_accepted() {
        assert!(compile_user_glob("src/**/*.ts", "entry").is_ok());
        assert!(compile_user_glob("**/*.test.ts", "entry").is_ok());
        assert!(compile_user_glob("./src/main.ts", "entry").is_ok());
        assert!(compile_user_glob("packages/*/src/index.ts", "entry").is_ok());
        assert!(compile_user_glob("**/{a,b}.ts", "entry").is_ok());
    }

    #[test]
    fn finding_ignore_globs_validate_negated_pattern_bodies() {
        let mut errors = Vec::new();
        validate_user_finding_ignore_globs(
            &["**/*.test.ts".to_string(), "!src/public/**".to_string()],
            "ignoreFindings",
            &mut errors,
        );

        assert!(errors.is_empty());
    }

    #[test]
    fn finding_ignore_globs_reject_bare_negation() {
        let mut errors = Vec::new();
        validate_user_finding_ignore_globs(&["!".to_string()], "ignoreFindings", &mut errors);

        assert!(matches!(
            errors.as_slice(),
            [GlobValidationError::EmptyNegation { .. }]
        ));
    }

    #[test]
    fn finding_ignore_globs_validate_negated_paths_and_syntax() {
        let cases = ["!/absolute/**", "!../outside/**", "![unclosed"];

        for pattern in cases {
            let mut errors = Vec::new();
            validate_user_finding_ignore_globs(
                &[pattern.to_string()],
                "ignoreFindings",
                &mut errors,
            );
            assert_eq!(errors.len(), 1, "pattern: {pattern}");
        }
    }

    #[test]
    fn bracket_character_class_accepted() {
        assert!(compile_user_glob("[A-Z]*.tsx", "entry").is_ok());
        assert!(compile_user_glob("src/**/[A-Z]*.{ts,tsx}", "ignoreExports[].file").is_ok());
        assert!(compile_user_glob("**/[0-9][0-9]*.md", "entry").is_ok());
    }

    #[test]
    fn validate_user_path_rejects_traversal_and_absolute() {
        assert!(validate_user_path("../escape", "boundaries.zones[].root").is_err());
        assert!(validate_user_path("/abs/dir", "boundaries.zones[].root").is_err());
        assert!(validate_user_path("packages/ui", "boundaries.zones[].root").is_ok());
        assert!(validate_user_path("[brackets-literal]/dir", "boundaries.zones[].root").is_ok());
    }

    #[test]
    fn workspace_root_keys_must_be_exact() {
        let field = "workspaces.changedSince";
        assert!(validate_workspace_root_key("packages/web", field).is_ok());
        for key in [
            "",
            "./packages/web",
            "packages/web/",
            "packages//web",
            "packages\\web",
        ] {
            assert!(
                matches!(
                    validate_workspace_root_key(key, field),
                    Err(GlobValidationError::InexactWorkspaceRoot { .. })
                ),
                "{key:?} must be rejected as inexact"
            );
        }
        assert!(matches!(
            validate_workspace_root_key("/abs/web", field),
            Err(GlobValidationError::AbsolutePath { .. })
        ));
        assert!(matches!(
            validate_workspace_root_key("packages/../web", field),
            Err(GlobValidationError::TraversalSegment { .. })
        ));
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
        assert!(compile_user_glob("foo..bar", "entry").is_ok());
        assert!(compile_user_glob("src/file.with..dots.ts", "entry").is_ok());
    }

    #[test]
    fn current_dir_dot_accepted() {
        assert!(compile_user_glob("./src/**", "entry").is_ok());
    }

    #[test]
    fn invalid_glob_syntax_rejected() {
        let err = compile_user_glob("[invalid", "entry").unwrap_err();
        assert!(matches!(err, GlobValidationError::InvalidSyntax { .. }));
        let msg = err.to_string();
        assert!(msg.contains("entry"), "msg: {msg}");
        assert_eq!(msg.matches("[invalid").count(), 1, "msg: {msg}");
        assert!(msg.contains("unclosed character class"), "msg: {msg}");
    }

    #[test]
    fn empty_pattern_accepted_as_globset_handles_it() {
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

    /// `ignorePatterns` accepts an empty pattern, so `ignoreFindings` rejecting it
    /// was both an asymmetry and a misleading message about a `!` it lacks.
    #[test]
    fn finding_ignore_globs_accept_empty_pattern_like_ignore_patterns() {
        let mut findings_errors = Vec::new();
        validate_user_finding_ignore_globs(
            &[String::new()],
            "ignoreFindings",
            &mut findings_errors,
        );

        let mut patterns_errors = Vec::new();
        validate_user_globs(&[String::new()], "ignorePatterns", &mut patterns_errors);

        assert!(findings_errors.is_empty(), "errors: {findings_errors:?}");
        assert!(patterns_errors.is_empty(), "errors: {patterns_errors:?}");
    }

    #[test]
    fn finding_ignore_globs_reject_empty_body_only_after_bang() {
        let mut errors = Vec::new();
        validate_user_finding_ignore_globs(
            &[String::new(), "!".to_string(), "src/**".to_string()],
            "ignoreFindings",
            &mut errors,
        );
        assert_eq!(errors.len(), 1, "only the bare `!` is invalid: {errors:?}");
        assert!(matches!(
            errors[0],
            GlobValidationError::EmptyNegation { .. }
        ));
    }
}
