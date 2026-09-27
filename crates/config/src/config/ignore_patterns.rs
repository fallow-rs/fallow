//! The compiled discovery ignore set: the built-in defaults, the project's own
//! `ignorePatterns`, and the `!` exceptions from `ignorePatterns`.
//!
//! The order is the gitignore order. A path is ignored when a built-in default
//! or a project pattern matches it, unless a `!` exception also matches it
//! (issue #2940). A `!` exception can also add a hidden directory to traversal
//! (issue #2452). Paths under `node_modules` or `.git` are never lifted.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use globset::{GlobSet, GlobSetBuilder};

/// Path components that a `!` exception can never lift.
pub const UNLIFTABLE_IGNORE_SEGMENTS: &[&str] = &["node_modules", ".git"];

/// Characters that make a pattern segment a glob instead of a literal name.
const GLOB_META: &[char] = &['*', '?', '[', ']', '{', '}', '\\'];

/// What one `!` exception can reach, read from its literal segments. Used to
/// decide which hidden directories the walk must open.
#[derive(Debug, Clone)]
struct ExceptionScope {
    /// The leading segments of the pattern that have no glob syntax.
    prefix: PathBuf,
    /// The segments after `prefix` contain `**`.
    has_globstar: bool,
    /// Literal hidden directory names in the segments after `prefix`.
    hidden_names: Vec<String>,
}

impl ExceptionScope {
    fn parse(body: &str) -> Self {
        let mut prefix = PathBuf::new();
        let mut segments = body.split('/').filter(|segment| !segment.is_empty());
        let mut rest: Vec<&str> = Vec::new();
        for segment in segments.by_ref() {
            if segment.contains(GLOB_META) {
                rest.push(segment);
                break;
            }
            prefix.push(segment);
        }
        rest.extend(segments);
        Self {
            prefix,
            has_globstar: rest.iter().any(|segment| segment.contains("**")),
            hidden_names: rest
                .iter()
                .filter(|segment| segment.starts_with('.') && !segment.contains(GLOB_META))
                .map(|segment| (*segment).to_owned())
                .collect(),
        }
    }

    fn admits_hidden_dir(&self, dir: &Path) -> bool {
        if self.prefix.starts_with(dir) {
            return true;
        }
        if !dir.starts_with(&self.prefix) {
            return false;
        }
        let named = dir
            .file_name()
            .and_then(OsStr::to_str)
            .is_some_and(|name| self.hidden_names.iter().any(|hidden| hidden == name));
        named || (self.has_globstar && !self.prefix.as_os_str().is_empty())
    }
}

/// The discovery ignore set with its `!` exceptions.
///
/// `matches_into` and `len` read only the ignore globs, in the index layout
/// that [`crate::DEFAULT_IGNORE_PATTERNS`] documents. `is_match` applies the
/// exceptions too, so every caller that asks "is this path ignored" gets the
/// same answer.
#[derive(Debug, Clone, Default)]
pub struct IgnorePatternSet {
    patterns: GlobSet,
    exceptions: GlobSet,
    exception_scopes: Vec<ExceptionScope>,
}

impl IgnorePatternSet {
    /// A set that ignores nothing.
    #[must_use]
    pub fn empty() -> Self {
        Self::default()
    }

    /// Build a set from ignore globs and `!` exception bodies (without the
    /// `!`). Exception bodies that do not compile are dropped, because config
    /// load validates them first.
    #[must_use]
    pub fn new(patterns: GlobSet, exception_bodies: &[&str]) -> Self {
        let mut builder = GlobSetBuilder::new();
        let mut exception_scopes = Vec::with_capacity(exception_bodies.len());
        for body in exception_bodies {
            if let Ok(glob) = globset::Glob::new(body) {
                builder.add(glob);
                exception_scopes.push(ExceptionScope::parse(body));
            }
        }
        Self {
            patterns,
            exceptions: builder.build().unwrap_or_default(),
            exception_scopes,
        }
    }

    /// True when the path is ignored: an ignore glob matches it and no `!`
    /// exception lifts it.
    pub fn is_match(&self, path: impl AsRef<Path>) -> bool {
        let path = path.as_ref();
        self.patterns.is_match(path) && !self.is_lifted(path)
    }

    /// Indices of the ignore globs that match the path, without the
    /// exceptions. Callers use it only for a path that `is_match` rejected.
    pub fn matches_into(&self, path: impl AsRef<Path>, into: &mut Vec<usize>) {
        self.patterns.matches_into(path.as_ref(), into);
    }

    /// Indices of the ignore globs that match the path, without the
    /// exceptions.
    #[must_use]
    pub fn matches(&self, path: impl AsRef<Path>) -> Vec<usize> {
        self.patterns.matches(path.as_ref())
    }

    /// Number of ignore globs, without the exceptions.
    #[must_use]
    pub fn len(&self) -> usize {
        self.patterns.len()
    }

    /// True when the set has no ignore globs.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.patterns.is_empty()
    }

    /// True when the project wrote at least one `!` exception.
    #[must_use]
    pub fn has_exceptions(&self) -> bool {
        !self.exceptions.is_empty()
    }

    /// True when a `!` exception matches the path and the path is not under
    /// `node_modules` or `.git`.
    pub fn is_lifted(&self, path: impl AsRef<Path>) -> bool {
        let path = path.as_ref();
        !self.exceptions.is_empty() && self.exceptions.is_match(path) && !is_unliftable(path)
    }

    /// True when the walk must open the hidden directory `dir` (a path
    /// relative to the project root) because a `!` exception can match a file
    /// inside it. The walk keeps only the files that an exception matches.
    #[must_use]
    pub fn admits_hidden_dir(&self, dir: &Path) -> bool {
        !is_unliftable(dir)
            && self
                .exception_scopes
                .iter()
                .any(|scope| scope.admits_hidden_dir(dir))
    }
}

impl From<GlobSet> for IgnorePatternSet {
    fn from(patterns: GlobSet) -> Self {
        Self::new(patterns, &[])
    }
}

fn is_unliftable(path: &Path) -> bool {
    path.components().any(|component| {
        UNLIFTABLE_IGNORE_SEGMENTS
            .iter()
            .any(|segment| component.as_os_str() == OsStr::new(segment))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(patterns: &[&str], exceptions: &[&str]) -> IgnorePatternSet {
        let mut builder = GlobSetBuilder::new();
        for pattern in patterns {
            builder.add(globset::Glob::new(pattern).expect("valid glob"));
        }
        IgnorePatternSet::new(builder.build().expect("valid set"), exceptions)
    }

    #[test]
    fn an_exception_lifts_a_match() {
        let ignores = set(&["**/coverage/**"], &["src/coverage/**"]);
        assert!(!ignores.is_match("src/coverage/a.ts"));
        assert!(ignores.is_match("coverage/a.ts"));
        assert_eq!(ignores.matches("src/coverage/a.ts"), vec![0]);
    }

    #[test]
    fn node_modules_and_git_are_never_lifted() {
        let ignores = set(&["**/node_modules/**", "**/.git/**"], &["**"]);
        assert!(ignores.is_match("node_modules/dep/index.ts"));
        assert!(ignores.is_match(".git/hooks/pre-commit.js"));
        assert!(!ignores.admits_hidden_dir(Path::new(".git")));
    }

    #[test]
    fn hidden_directories_open_only_on_a_named_path() {
        let exact = set(&[], &[".config/**"]);
        assert!(exact.admits_hidden_dir(Path::new(".config")));
        assert!(exact.admits_hidden_dir(Path::new(".config/.inner")));
        assert!(!exact.admits_hidden_dir(Path::new(".cache")));
        assert!(!exact.admits_hidden_dir(Path::new("packages/a/.config")));

        let any_depth = set(&[], &["**/.config/**"]);
        assert!(any_depth.admits_hidden_dir(Path::new("packages/a/.config")));
        assert!(!any_depth.admits_hidden_dir(Path::new(".next")));

        let broad = set(&[], &["**/*.ts"]);
        assert!(!broad.admits_hidden_dir(Path::new(".next")));

        let file = set(&[], &["a/.b/c.ts"]);
        assert!(file.admits_hidden_dir(Path::new("a/.b")));
        assert!(!file.admits_hidden_dir(Path::new("a/.c")));
    }
}
