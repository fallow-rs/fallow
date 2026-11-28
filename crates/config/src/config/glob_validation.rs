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

use globset::{Glob, GlobBuilder, GlobMatcher};

use super::finding_ignore::FindingIgnoreMatcher;

/// Bytes allowed in one user glob.
///
/// A glob set matches literal prefixes with an Aho-Corasick DFA, and its
/// build time grows with the square of the pattern length. Real config globs
/// are far shorter.
pub const MAX_GLOB_BYTES: usize = 512;

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
    /// A pattern starts with `!` or `./` in a list that supports neither
    /// form, so the pattern matches no path.
    UnmatchablePrefix {
        /// Config field the pattern came from, named in the error.
        field: &'static str,
        /// The offending pattern as written in the config.
        pattern: String,
    },
    /// Pattern is longer than [`MAX_GLOB_BYTES`].
    TooLong {
        /// Config field the pattern came from, named in the error.
        field: &'static str,
        /// The first characters of the pattern, as written in the config.
        start: String,
    },
    /// The brace groups of a pattern cannot be checked: a group is not
    /// closed, or the groups expand to more than [`MAX_BRACE_EXPANSIONS`]
    /// texts.
    UncheckableBraces {
        /// Config field the pattern came from, named in the error.
        field: &'static str,
        /// The offending pattern as written in the config.
        pattern: String,
    },
    /// Pattern parses, but its matcher cannot be compiled, for example
    /// because its brace groups nest too deep.
    Uncompilable {
        /// Config field the pattern came from, named in the error.
        field: &'static str,
        /// The offending pattern as written in the config.
        pattern: String,
        /// Underlying regex compile error.
        source: regex::Error,
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
            Self::UnmatchablePrefix { field, pattern } => match pattern.strip_prefix("./") {
                Some(rest) => write!(
                    f,
                    "{field}: '{pattern}' starts with './' and matches no path; \
                     write it relative to the project root without './' (e.g. '{rest}')"
                ),
                None => write!(
                    f,
                    "{field}: '{pattern}' is a negated pattern, which this list does not \
                     support, so it matches no path; list only the paths to exclude"
                ),
            },
            Self::TooLong { field, start } => write!(
                f,
                "{field}: '{start}...' is longer than {MAX_GLOB_BYTES} bytes; shorten the pattern"
            ),
            Self::UncheckableBraces { field, pattern } => write!(
                f,
                "{field}: the brace groups of '{pattern}' are not closed or expand to more \
                 than {MAX_BRACE_EXPANSIONS} patterns; split the pattern into simpler patterns"
            ),
            Self::Uncompilable { field, pattern, .. } => write!(
                f,
                "{field}: glob '{pattern}' cannot be compiled: its brace groups nest too deep \
                 or it is too large; split the pattern into simpler patterns"
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
            Self::Uncompilable { source, .. } => Some(source),
            Self::AbsolutePath { .. }
            | Self::TraversalSegment { .. }
            | Self::EmptyNegation { .. }
            | Self::UnliftableNegation { .. }
            | Self::InexactWorkspaceRoot { .. }
            | Self::UnmatchablePrefix { .. }
            | Self::TooLong { .. }
            | Self::UncheckableBraces { .. } => None,
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
    bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':'
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

/// Whether `pattern` is an absolute path, or has `..` segments that can lead
/// above the directory it starts from.
///
/// Workspace discovery uses this test to skip an entry of `package.json`
/// `workspaces` or `pnpm-workspace.yaml` `packages` before it reads the disk.
/// The test is lexical. A `..` after a named or single-wildcard segment goes
/// back one level, so `packages/../libs/*` stays inside the root. A `**`
/// segment can match zero directories, so it counts as no level, and
/// `**/../x` can escape.
///
/// Config glob validation is stricter: it rejects every `..` segment.
#[must_use]
pub fn escapes_project_root(pattern: &str) -> bool {
    if is_absolute_pattern(pattern) {
        return true;
    }
    let mut depth: usize = 0;
    for segment in pattern.split(['/', '\\']) {
        match segment {
            "" | "." | "**" => {}
            ".." => match depth.checked_sub(1) {
                Some(up) => depth = up,
                None => return true,
            },
            _ => depth += 1,
        }
    }
    false
}

/// The form of a path glob that the matchers see: the pattern without its
/// leading `./` segments. Project-root-relative paths have no such segment.
#[must_use]
pub fn normalize_path_pattern(pattern: &str) -> &str {
    let mut rest = pattern;
    while let Some(stripped) = rest.strip_prefix("./") {
        rest = stripped;
    }
    rest
}

/// Compile a glob that matches project-root-relative file paths.
///
/// A single `*` stays within one path segment, and only `**` crosses
/// directories. This is the rule of picomatch and the plugin entry patterns.
/// The glob matches file paths only, not directories, so `src/generated/*`
/// does not match the files in the subfolders of `src/generated` (gitignore
/// ignores them, because it skips a matched directory). Every path glob that a user writes in the config compiles
/// through this function, so all of these fields match a path the same way.
/// A leading `./` is not part of the match, so `./src/**` and `src/**` match
/// the same paths.
///
/// # Errors
///
/// Returns the `globset` error when the pattern is not valid glob syntax.
pub fn compile_path_glob(pattern: &str) -> Result<Glob, globset::Error> {
    GlobBuilder::new(normalize_path_pattern(pattern))
        .literal_separator(true)
        .build()
}

/// The NFA size limit that `globset` sets for the regex of one glob.
const GLOB_REGEX_SIZE_LIMIT: usize = 10 * (1 << 20);

/// Compile the matcher of `glob` without a panic.
///
/// `Glob::compile_matcher` panics when the regex of the glob does not
/// compile: brace groups that nest past the nesting limit of the regex
/// parser, or a regex past the size limit. This function compiles the regex
/// first, with the settings of `globset`, and returns that failure as an
/// error. Only a brace group makes nested regex groups, so a glob without
/// `{` that is not longer than [`MAX_GLOB_BYTES`] skips the check.
///
/// # Errors
///
/// Returns the regex error when the regex of the glob does not compile.
pub fn try_compile_matcher(glob: &Glob) -> Result<GlobMatcher, regex::Error> {
    let text = glob.glob();
    if text.contains('{') || text.len() > MAX_GLOB_BYTES {
        regex::bytes::RegexBuilder::new(glob.regex())
            .dot_matches_new_line(true)
            .size_limit(GLOB_REGEX_SIZE_LIMIT)
            .build()?;
    }
    Ok(glob.compile_matcher())
}

/// Compile the matcher of a config path glob. Config validation refuses a
/// glob that does not parse or compile, so this gives `None` only for a glob
/// that the code built after the validation. That glob matches no path, and
/// a warning names it.
pub(crate) fn compile_config_matcher(pattern: &str, field: &str) -> Option<GlobMatcher> {
    let compiled = compile_path_glob(pattern)
        .map_err(|error| error.to_string())
        .and_then(|glob| try_compile_matcher(&glob).map_err(|error| error.to_string()));
    match compiled {
        Ok(matcher) => Some(matcher),
        Err(error) => {
            tracing::warn!(
                "{field}: glob '{pattern}' cannot be compiled and matches no path: {error}"
            );
            None
        }
    }
}

/// Check that the matcher of a validated glob compiles.
fn check_compiles(
    glob: Glob,
    pattern: &str,
    field: &'static str,
) -> Result<Glob, GlobValidationError> {
    match try_compile_matcher(&glob) {
        Ok(_) => Ok(glob),
        Err(source) => Err(GlobValidationError::Uncompilable {
            field,
            pattern: pattern.to_owned(),
            source,
        }),
    }
}

/// Join a literal directory and a glob tail into one path glob. Each glob
/// character of `dir` is escaped, so a directory such as `app/[id]`
/// matches only itself. An empty `dir` gives `tail`.
#[must_use]
pub fn literal_dir_glob(dir: &str, tail: &str) -> String {
    let dir = dir.trim_end_matches('/');
    if dir.is_empty() {
        return tail.to_owned();
    }
    format!("{}/{tail}", globset::escape(dir))
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
/// - `TooLong` if the pattern is longer than [`MAX_GLOB_BYTES`]
/// - `Uncompilable` if the matcher of the pattern does not compile
pub fn compile_user_glob(pattern: &str, field: &'static str) -> Result<Glob, GlobValidationError> {
    check_glob_length(pattern, field)?;
    // A brace group such as `{..,x}` can expand to a `..` or `/` segment
    // that the raw text does not show, so each expansion is checked too.
    let Some(expansions) = brace_expansions(pattern) else {
        return Err(GlobValidationError::UncheckableBraces {
            field,
            pattern: pattern.to_owned(),
        });
    };
    let texts = expansions
        .iter()
        .map(String::as_str)
        .chain(std::iter::once(pattern));
    for text in texts {
        if is_absolute_pattern(text) || folds_to_absolute_path(text) {
            return Err(GlobValidationError::AbsolutePath {
                field,
                pattern: pattern.to_owned(),
            });
        }
        if has_traversal_segment(text) {
            return Err(GlobValidationError::TraversalSegment {
                field,
                pattern: pattern.to_owned(),
            });
        }
    }
    let glob = compile_path_glob(pattern).map_err(|source| GlobValidationError::InvalidSyntax {
        field,
        pattern: pattern.to_owned(),
        source,
    })?;
    check_compiles(glob, pattern, field)
}

/// The most expansions that the brace expansion of one pattern builds.
/// Real config globs have a few. A pattern over the limit is refused.
pub const MAX_BRACE_EXPANSIONS: usize = 1024;

/// The texts that the `{a,b}` groups of `pattern` expand to, with nested
/// groups expanded too. A pattern without a group gives no text. A pattern
/// with an unclosed group, or with more than [`MAX_BRACE_EXPANSIONS`]
/// expansions, gives `None`, because its expansions cannot be checked.
fn brace_expansions(pattern: &str) -> Option<Vec<String>> {
    if !pattern.contains('{') {
        return Some(Vec::new());
    }
    let mut out = Vec::new();
    expand_into(pattern, &mut out).then_some(out)
}

/// Push the expansions of `pattern` to `out`. Returns `false` when a group
/// is not closed or the expansions pass [`MAX_BRACE_EXPANSIONS`].
fn expand_into(pattern: &str, out: &mut Vec<String>) -> bool {
    let Some(open) = pattern.find('{') else {
        if out.len() >= MAX_BRACE_EXPANSIONS {
            return false;
        }
        out.push(pattern.to_owned());
        return true;
    };
    let mut depth = 0usize;
    let mut close = None;
    let mut splits = Vec::new();
    for (offset, ch) in pattern[open..].char_indices() {
        let index = open + offset;
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    close = Some(index);
                    break;
                }
            }
            ',' if depth == 1 => splits.push(index),
            _ => {}
        }
    }
    let Some(close) = close else {
        return false;
    };
    let (prefix, suffix) = (&pattern[..open], &pattern[close + 1..]);
    let mut start = open + 1;
    for end in splits.into_iter().chain(std::iter::once(close)) {
        let option = &pattern[start..end];
        if !expand_into(&format!("{prefix}{option}{suffix}"), out) {
            return false;
        }
        start = end + 1;
    }
    true
}

fn check_glob_length(pattern: &str, field: &'static str) -> Result<(), GlobValidationError> {
    if pattern.len() > MAX_GLOB_BYTES {
        return Err(GlobValidationError::TooLong {
            field,
            start: pattern.chars().take(40).collect(),
        });
    }
    Ok(())
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
/// Returns `TooLong` if the pattern is longer than [`MAX_GLOB_BYTES`],
/// `InvalidSyntax` if `globset::Glob::new` rejects the pattern, and
/// `Uncompilable` if the matcher of the pattern does not compile.
pub fn compile_user_specifier_glob(
    pattern: &str,
    field: &'static str,
) -> Result<Glob, GlobValidationError> {
    check_glob_length(pattern, field)?;
    let glob = Glob::new(pattern).map_err(|source| GlobValidationError::InvalidSyntax {
        field,
        pattern: pattern.to_owned(),
        source,
    })?;
    check_compiles(glob, pattern, field)
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
/// are not validated as globs. When each glob is valid, the glob set of all
/// of them must also compile.
pub fn validate_user_dependency_globs(
    entries: &[String],
    field: &'static str,
    errors: &mut Vec<GlobValidationError>,
) {
    let mut set = globset::GlobSetBuilder::new();
    let mut valid = true;
    for entry in entries
        .iter()
        .filter(|entry| super::is_dependency_glob(entry))
    {
        match compile_user_specifier_glob(entry, field) {
            Ok(glob) => {
                set.add(glob);
            }
            Err(error) => {
                errors.push(error);
                valid = false;
            }
        }
    }
    if valid && let Err(source) = set.build() {
        errors.push(GlobValidationError::PatternSetCompilation { field, source });
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

/// Validate globs of a list that supports no negation and no `./` prefix,
/// such as `production.ignore`. Both forms match no path, so each one is an
/// error.
pub fn validate_user_plain_globs(
    patterns: &[String],
    field: &'static str,
    errors: &mut Vec<GlobValidationError>,
) {
    for pattern in patterns {
        if pattern.starts_with('!') || pattern.starts_with("./") {
            errors.push(GlobValidationError::UnmatchablePrefix {
                field,
                pattern: pattern.clone(),
            });
        } else if let Err(error) = compile_user_glob(pattern, field) {
            errors.push(error);
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

/// Whether `text` is absolute after the loaders fold `\` to `/` and strip each
/// leading `./`. For example, `.//home` becomes `/home`.
fn folds_to_absolute_path(text: &str) -> bool {
    let folded = text.replace('\\', "/");
    is_absolute_pattern(folded.trim_start_matches("./"))
}

/// Validate a user-supplied DIRECTORY PATH (not a glob). Same absolute-path
/// and traversal checks as `compile_user_glob`, but skips the glob-syntax
/// check because the value is a literal path, not a pattern.
///
/// Used for fields like `boundaries.zones[].root` and
/// `boundaries.zones[].autoDiscover` that name a directory subtree rather
/// than a match pattern.
///
/// The checks also run on the text with `\` folded to `/` and each leading
/// `./` removed, because the loaders read that text.
///
/// # Errors
///
/// Returns `AbsolutePath` or `TraversalSegment` for the same shapes
/// `compile_user_glob` rejects. Never returns `InvalidSyntax`.
pub fn validate_user_path(path: &str, field: &'static str) -> Result<(), GlobValidationError> {
    if is_absolute_pattern(path) || folds_to_absolute_path(path) {
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

/// Expands a root-relative glob on disk with the `glob` crate.
///
/// Use this function for each on-disk expansion of a config or repository
/// glob. It escapes the root, so a `[`, `*` or `?` in the root path matches
/// literally. It rewrites the pattern with [`disk_glob_pattern`], so no match
/// is in the parent of the root. Returns `None` for an absolute pattern or a
/// pattern with a Windows drive prefix, because such a pattern names a path
/// outside the root, for a `..` segment or a brace expansion that can lead
/// out of the root, and for a pattern that is not valid glob syntax.
#[must_use]
#[expect(
    clippy::disallowed_methods,
    reason = "the one wrapper that calls glob::glob, after the root escape and the dot rewrite"
)]
pub fn expand_disk_glob(
    root: &Path,
    pattern: &str,
) -> Option<impl Iterator<Item = glob::GlobResult> + use<>> {
    let pattern = pattern_below_root(pattern)?;
    if pattern.split(['/', '\\']).any(|segment| segment == "**") {
        let walk = disk_walk::walk_recursive_glob(root, &pattern)?;
        return Some(DiskGlobPaths::Walk(walk));
    }
    let full = format!("{}/{}", escaped_root(root), disk_glob_pattern(&pattern));
    glob::glob(&full).ok().map(DiskGlobPaths::Glob)
}

/// The matches of [`expand_disk_glob`]: from the `glob` crate, or from the
/// bounded walk for a pattern with a `**` segment.
enum DiskGlobPaths {
    Glob(glob::Paths),
    Walk(disk_walk::RecursiveGlobWalk),
}

impl Iterator for DiskGlobPaths {
    type Item = glob::GlobResult;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::Glob(paths) => paths.next(),
            Self::Walk(paths) => paths.next().map(Ok),
        }
    }
}

/// `pattern` with each `..` segment applied to the literal segment before
/// it, so `packages/../libs/*` gives `libs/*`. Returns `None` for an absolute
/// pattern, for a `..` that would leave the root or that follows a wildcard
/// segment, and for a brace expansion that is absolute or has a `..`
/// segment. The `glob` crate does not expand braces, but a later caller can.
fn pattern_below_root(pattern: &str) -> Option<String> {
    if is_absolute_pattern(pattern) {
        return None;
    }
    let expansions = brace_expansions(pattern)?;
    if expansions
        .iter()
        .any(|text| is_absolute_pattern(text) || has_traversal_segment(text))
    {
        return None;
    }
    if !has_traversal_segment(pattern) {
        return Some(pattern.to_owned());
    }
    let mut kept: Vec<&str> = Vec::new();
    for segment in pattern.split(['/', '\\']) {
        if segment == ".." {
            let previous = kept.pop()?;
            if previous.contains(['*', '?', '[', '{']) || previous == "." || previous.is_empty() {
                return None;
            }
        } else {
            kept.push(segment);
        }
    }
    let normalized = kept.join("/");
    (!normalized.is_empty() && !has_traversal_segment(&normalized)).then_some(normalized)
}

/// Escapes the glob characters of `root` after its Windows path prefix. The
/// `glob` crate reads a prefix such as `\\?\C:` as a prefix only when it is
/// not escaped, and `std::fs::canonicalize` returns that form on Windows.
fn escaped_root(root: &Path) -> String {
    let text = root.to_string_lossy();
    let prefix = match root.components().next() {
        Some(Component::Prefix(prefix)) => prefix.as_os_str().to_string_lossy().into_owned(),
        _ => String::new(),
    };
    let rest = text.strip_prefix(prefix.as_str()).unwrap_or(&text);
    format!("{prefix}{}", glob::Pattern::escape(rest))
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

#[path = "disk_glob_walk.rs"]
mod disk_walk;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg_attr(miri, ignore = "tempdir is blocked by Miri isolation")]
    fn disk_expansion_refuses_patterns_that_leave_the_root() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("project");
        std::fs::create_dir(&root).unwrap();
        std::fs::write(dir.path().join("outside.json"), "").unwrap();
        for pattern in [
            "../outside.json",
            "../*.json",
            "src/../../outside.json",
            "{..,x}/*.json",
        ] {
            assert!(expand_disk_glob(&root, pattern).is_none(), "{pattern}");
        }
        std::fs::write(root.join("a.json"), "").unwrap();
        let hits = expand_disk_glob(&root, "*.json")
            .unwrap()
            .filter_map(Result::ok)
            .count();
        assert_eq!(hits, 1);
        std::fs::create_dir(root.join("libs")).unwrap();
        std::fs::write(root.join("libs/b.json"), "").unwrap();
        let hits: Vec<_> = expand_disk_glob(&root, "packages/../libs/*.json")
            .unwrap()
            .filter_map(Result::ok)
            .collect();
        assert_eq!(hits, vec![root.join("libs/b.json")]);
    }

    #[test]
    fn a_glob_that_folds_to_an_absolute_path_is_refused() {
        for pattern in [".//", ".//home/*", "././/etc/x", ".\\/x"] {
            assert!(
                validate_user_path(pattern, "entry").is_err(),
                "path check accepted {pattern:?}"
            );
            assert!(
                compile_user_glob(pattern, "entry").is_err(),
                "glob check accepted {pattern:?}"
            );
        }
        // Only the glob check expands braces; a path keeps them as a name.
        assert!(compile_user_glob("{.//,a}/b", "entry").is_err());
        assert!(compile_user_glob("./src/**/*.ts", "entry").is_ok());
    }

    #[test]
    fn a_pattern_whose_braces_cannot_be_checked_is_refused() {
        let many = "{a,b,c,d}".repeat(6);
        assert!(compile_user_glob(&many, "entry").is_err(), "4^6 expansions");
        assert!(compile_user_glob("{..,x", "entry").is_err());
    }

    #[test]
    fn each_brace_expansion_must_stay_inside_the_root() {
        for pattern in [
            "{..,x}/outside/notes_*",
            "{/abs/name,x}",
            ".{.,x}/y",
            "a/{b,{c,..}}/d",
            "{.,x}{.,y}/z",
        ] {
            assert!(
                compile_user_glob(pattern, "entry").is_err(),
                "{pattern} must be refused"
            );
        }
        for pattern in [
            "src/{a,b}/*.ts",
            "index.{ts,tsx}",
            "{src,lib}/**/{a,b{c,d}}.js",
        ] {
            assert!(compile_user_glob(pattern, "entry").is_ok(), "{pattern}");
        }
    }

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
    fn expand_disk_glob_never_expands_outside_root() {
        let base = tempfile::tempdir().expect("create temp dir");
        let root = base.path().join("root");
        std::fs::create_dir_all(root.join(".hidden")).expect("create root dirs");
        std::fs::write(base.path().join("secret"), "").expect("write secret");
        std::fs::write(root.join(".hidden").join("secret"), "").expect("write inside");

        let expand = |pattern: &str| -> Vec<std::path::PathBuf> {
            expand_disk_glob(&root, pattern)
                .expect("valid glob")
                .filter_map(Result::ok)
                .collect()
        };
        for pattern in [".*/secret", "..*/secret", ".?/secret", ".[.]/secret"] {
            validate_user_path(pattern, "test").expect("pattern passes validation");
            let matches = expand(pattern);
            assert!(
                matches.iter().all(|path| !path
                    .components()
                    .any(|c| matches!(c, Component::ParentDir | Component::CurDir))),
                "{pattern} expanded outside the root: {matches:?}"
            );
        }
        assert_eq!(
            expand(".*/secret"),
            vec![root.join(".hidden").join("secret")]
        );
        let outside = base.path().join("secret");
        assert!(expand_disk_glob(&root, &outside.to_string_lossy()).is_none());
        assert!(expand_disk_glob(&root, "/secret").is_none());
        assert!(expand_disk_glob(&root, "C:secret").is_none());
    }

    #[test]
    fn expand_disk_glob_matches_a_root_with_glob_characters() {
        let base = tempfile::tempdir().expect("create temp dir");
        let root = base.path().join("app [1]");
        std::fs::create_dir_all(root.join("src")).expect("create root dirs");
        std::fs::write(root.join("src").join("a.ts"), "").expect("write file");

        // `canonicalize` gives a verbatim `\\?\` root on Windows.
        let canonical = std::fs::canonicalize(&root).expect("canonicalize root");
        for root in [&root, &canonical] {
            let matches: Vec<_> = expand_disk_glob(root, "src/*.ts")
                .expect("valid glob")
                .filter_map(Result::ok)
                .collect();
            assert_eq!(matches, vec![root.join("src").join("a.ts")]);
        }
    }

    #[test]
    fn escapes_project_root_follows_the_dot_dot_segments() {
        for pattern in [
            "../*",
            "../../x",
            "packages/../../x",
            "**/../x",
            "..\\x",
            "/abs/*",
            "C:/abs",
        ] {
            assert!(escapes_project_root(pattern), "{pattern}");
        }
        for pattern in [
            "packages/*",
            "./packages/*",
            "packages/../libs/*",
            "*/..",
            "a/**",
            "..x/*",
        ] {
            assert!(!escapes_project_root(pattern), "{pattern}");
        }
    }

    #[test]
    fn compile_path_glob_drops_the_leading_dot_slash() {
        let main = compile_path_glob("./src/main.ts")
            .unwrap()
            .compile_matcher();
        assert!(main.is_match("src/main.ts"));

        let star = compile_path_glob("././src/*.ts").unwrap().compile_matcher();
        assert!(star.is_match("src/a.ts"));
        assert!(!star.is_match("src/nested/a.ts"));
    }

    #[test]
    fn normalize_path_pattern_strips_only_leading_dot_slash_segments() {
        assert_eq!(normalize_path_pattern("./a"), "a");
        assert_eq!(normalize_path_pattern("././a"), "a");
        assert_eq!(normalize_path_pattern("a/./b"), "a/./b");
        assert_eq!(normalize_path_pattern(".hidden/x"), ".hidden/x");
        assert_eq!(normalize_path_pattern("../a"), "../a");
        assert_eq!(normalize_path_pattern("./"), "");
    }

    #[test]
    fn literal_dir_glob_escapes_the_directory() {
        let zone = compile_path_glob(&literal_dir_glob("app/[id]", "**"))
            .unwrap()
            .compile_matcher();
        assert!(zone.is_match("app/[id]/page.tsx"));
        assert!(!zone.is_match("app/i/page.tsx"));

        assert_eq!(literal_dir_glob("", "**/*.md"), "**/*.md");
        assert_eq!(literal_dir_glob("docs/", "*.md"), "docs/*.md");

        let brace = compile_path_glob(&literal_dir_glob("src", "{a,b}.ts"))
            .unwrap()
            .compile_matcher();
        assert!(brace.is_match("src/a.ts"));
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

    /// The glob set build time grows with the square of the pattern length.
    /// OSS-Fuzz issue 578568203 had a 36 KB `ignoreFindings` pattern.
    #[test]
    fn globs_longer_than_the_limit_are_rejected() {
        let long = format!("{}/**", "0".repeat(4_096));
        let patterns = [long.clone(), format!("!x/{long}")];
        let mut errors = Vec::new();
        validate_user_finding_ignore_globs(&patterns, "ignoreFindings", &mut errors);
        assert_eq!(errors.len(), 2);
        assert!(
            errors
                .iter()
                .all(|error| error.to_string().contains("longer than 512 bytes")),
            "{}",
            errors[0].to_string().chars().take(200).collect::<String>()
        );

        let fits = format!("{}/**", "0".repeat(MAX_GLOB_BYTES - 3));
        assert!(compile_user_glob(&fits, "entry").is_ok());
    }

    /// `ignoreDependencies` globs also go into one glob set.
    #[test]
    fn specifier_globs_longer_than_the_limit_are_rejected() {
        let long = format!("{}*", "0".repeat(4_096));
        let error = compile_user_specifier_glob(&long, "ignoreDependencies").unwrap_err();
        assert!(error.to_string().contains("longer than 512 bytes"));
        assert!(compile_user_specifier_glob("@scope/*", "ignoreDependencies").is_ok());
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

    /// A glob whose brace groups nest deeper than the regex parser allows.
    /// The glob parses, but its regex does not compile.
    fn deeply_nested_glob() -> String {
        format!("{}b{}", "a{".repeat(130), "}".repeat(130))
    }

    #[test]
    fn try_compile_matcher_returns_an_error_instead_of_a_panic() {
        let nested = compile_path_glob(&deeply_nested_glob()).unwrap();
        assert!(try_compile_matcher(&nested).is_err());
        let braces = compile_path_glob("src/{a,b}/*.ts").unwrap();
        assert!(try_compile_matcher(&braces).unwrap().is_match("src/a/x.ts"));
        let long = compile_path_glob(&"a".repeat(MAX_GLOB_BYTES + 1)).unwrap();
        assert!(try_compile_matcher(&long).is_ok());
    }

    #[test]
    #[cfg_attr(miri, ignore = "tempdir is blocked by Miri isolation")]
    fn a_config_with_a_glob_that_does_not_compile_fails_to_load() {
        let nested = deeply_nested_glob();
        let fields = [
            (
                "overrides[].files",
                serde_json::json!({ "overrides": [{ "files": [nested] }] }),
            ),
            ("entry", serde_json::json!({ "entry": [nested] })),
            (
                "ignoreExports[].file",
                serde_json::json!({ "ignoreExports": [{ "file": nested, "exports": ["*"] }] }),
            ),
            (
                "ignoreDependencies",
                serde_json::json!({ "ignoreDependencies": [nested] }),
            ),
            (
                "ignoreUnresolvedImports",
                serde_json::json!({ "ignoreUnresolvedImports": [nested] }),
            ),
            (
                "boundaries.zones[].patterns",
                serde_json::json!({ "boundaries": { "zones": [{ "name": "a", "patterns": [nested] }] } }),
            ),
        ];
        for (field, config) in fields {
            let dir = tempfile::tempdir().unwrap();
            std::fs::create_dir(dir.path().join(".git")).unwrap();
            std::fs::write(dir.path().join(".fallowrc.json"), config.to_string()).unwrap();
            let error = crate::FallowConfig::find_and_load(dir.path())
                .err()
                .unwrap_or_else(|| panic!("{field}: the config loaded"));
            assert!(error.contains(field), "{field}: {error}");
            assert!(error.contains("cannot be compiled"), "{field}: {error}");
        }
    }

    /// Glob texts from tokens that make regex classes, groups, repeats and
    /// multibyte literals, with a fixed seed.
    fn generated_globs(count: usize) -> Vec<String> {
        const TOKENS: &[&str] = &[
            "a",
            "/",
            "*",
            "**",
            "**/",
            "?",
            "[a-z]",
            "[!a]",
            "[[]",
            "[]]",
            "[\u{e9}-\u{fc}]",
            "[\u{65e5}\u{672c}]",
            "\u{65e5}",
            "\\*",
            "@(",
            "|",
            ")",
            "!(",
            "+(",
            "-",
            ".",
            "[",
            "]",
            "[^/]",
            "\u{1f600}",
            "[\u{1f600}-\u{1f602}]",
        ];
        let mut state: u64 = 0x9E37_79B9_7F4A_7C15;
        let mut next = move || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            usize::try_from(state % 1024).unwrap_or(0)
        };
        let mut globs = Vec::with_capacity(count + 8);
        for _ in 0..count {
            let len = next() % 520;
            let mut text = String::new();
            while text.len() < len {
                text.push_str(TOKENS[next() % TOKENS.len()]);
            }
            globs.push(text);
        }
        globs.extend([
            "*".repeat(512),
            "**/".repeat(170),
            "?".repeat(512),
            "[a-z]".repeat(102),
            "[!a]".repeat(128),
            "\u{65e5}".repeat(170),
            "[\u{65e5}-\u{672c}]".repeat(56),
            "\u{1f600}".repeat(128),
        ]);
        globs
    }

    /// `try_compile_matcher` skips the regex check for a glob without `{`
    /// that is not longer than `MAX_GLOB_BYTES`. The matcher of each such
    /// glob must build.
    #[test]
    #[cfg_attr(miri, ignore = "thousands of regex builds are too slow under Miri")]
    fn a_glob_without_braces_within_the_limit_always_compiles() {
        let mut checked = 0;
        for text in generated_globs(1_000) {
            if text.contains('{') || text.len() > MAX_GLOB_BYTES {
                continue;
            }
            for literal_separator in [true, false] {
                let Ok(glob) = GlobBuilder::new(&text)
                    .literal_separator(literal_separator)
                    .empty_alternates(true)
                    .build()
                else {
                    continue;
                };
                let regex = regex::bytes::RegexBuilder::new(glob.regex())
                    .dot_matches_new_line(true)
                    .size_limit(GLOB_REGEX_SIZE_LIMIT)
                    .build();
                assert!(regex.is_ok(), "{text}");
                let _ = glob.compile_matcher();
                checked += 1;
            }
        }
        assert!(checked > 300, "only {checked} globs parsed");
    }

    /// The regex check refuses exactly the brace globs whose matcher build
    /// panics in `globset`.
    #[test]
    fn try_compile_matcher_agrees_with_the_matcher_build() {
        for depth in [120, 124, 125, 126, 130] {
            for text in [
                format!("{}b{}", "a{".repeat(depth), "}".repeat(depth)),
                format!("{}b{}", "{a,".repeat(depth), "}".repeat(depth)),
            ] {
                let glob = compile_path_glob(&text).unwrap();
                let checked = try_compile_matcher(&glob).is_ok();
                let built = std::panic::catch_unwind(|| glob.compile_matcher()).is_ok();
                assert_eq!(checked, built, "depth {depth}: {text}");
            }
        }
    }
}

#[cfg(test)]
#[path = "disk_glob_walk_tests.rs"]
mod disk_walk_tests;
