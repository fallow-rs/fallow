//! The compiled discovery ignore set: the built-in defaults, the project's own
//! `ignorePatterns`, and the `!` exceptions from `ignorePatterns`.
//!
//! The order is the gitignore order. A path is ignored when a built-in default
//! or a project pattern matches it, unless a `!` exception also matches it
//! (issue #2940). A `!` exception can also add a hidden directory to traversal
//! (issue #2452). Paths under `node_modules` or `.git` are never lifted.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use globset::{GlobSet, GlobSetBuilder};

use super::compile_path_glob;
use super::resolution::{DEFAULT_IGNORE_PATTERNS, normalize_user_glob_pattern};

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
    user: Option<Arc<UserPatternUsage>>,
}

/// Which of the project's own `ignorePatterns` globs excluded a path that no
/// built-in ignore excludes (issue #3824). The globs are the front of
/// `patterns`, in config order.
///
/// Shared across clones of the resolved config, so every stage that excludes
/// paths credits the same globs. A credit is never taken back: a long-lived
/// process (the LSP, watch mode) can only report fewer globs as having no
/// effect, never more.
#[derive(Debug)]
struct UserPatternUsage {
    patterns: Vec<String>,
    effective: Vec<AtomicBool>,
    /// Set when source discovery ran, so a glob without a credit is a real
    /// signal and not only a skipped walk.
    walked: AtomicBool,
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
            if let Ok(glob) = super::compile_path_glob(body) {
                builder.add(glob);
                exception_scopes.push(ExceptionScope::parse(body));
            }
        }
        Self {
            patterns,
            exceptions: builder.build().unwrap_or_default(),
            exception_scopes,
            user: None,
        }
    }

    /// Name the project's own globs, which are the first `patterns.len()`
    /// globs of the set, as written in `ignorePatterns`.
    #[must_use]
    pub fn with_user_patterns(mut self, patterns: Vec<String>) -> Self {
        self.user = (!patterns.is_empty()).then(|| {
            Arc::new(UserPatternUsage {
                effective: patterns.iter().map(|_| AtomicBool::new(false)).collect(),
                patterns,
                walked: AtomicBool::new(false),
            })
        });
        self
    }

    /// Number of the project's own globs at the front of the set.
    #[must_use]
    pub fn user_pattern_count(&self) -> usize {
        self.user.as_deref().map_or(0, |user| user.patterns.len())
    }

    /// Credit the project's own globs among `indices` (from
    /// [`Self::excluded_matches_into`] for one excluded path), unless a
    /// built-in glob is among them too: the built-in excludes the path anyway.
    pub fn credit_user_matches(&self, indices: &[usize]) {
        let Some(user) = self.user.as_deref() else {
            return;
        };
        if indices.iter().any(|&index| index >= user.patterns.len()) {
            return;
        }
        for &index in indices {
            if let Some(flag) = user.effective.get(index) {
                flag.store(true, Ordering::Relaxed);
            }
        }
    }

    /// [`Self::is_match`], and when the path is ignored, credit the
    /// project's own globs that match it, as [`Self::credit_user_matches`]
    /// does.
    pub fn excludes(&self, path: impl AsRef<Path>) -> bool {
        let path = path.as_ref();
        let ignored = self.is_match(path);
        if ignored && self.user.is_some() {
            self.credit_user_matches(&self.patterns.matches(path));
        }
        ignored
    }

    /// Record that source discovery ran with this set.
    pub fn mark_walked(&self) {
        if let Some(user) = self.user.as_deref() {
            user.walked.store(true, Ordering::Relaxed);
        }
    }

    /// The project's own globs that excluded no path that the run would
    /// otherwise keep, in config order. Empty until source discovery ran.
    ///
    /// A glob that names a hidden directory (`.cache/**`) is never in the
    /// list. The walk does not open most hidden directories, so it cannot
    /// credit such a glob, but the glob still silences the
    /// skipped-source-dotdir advisory for that directory.
    #[must_use]
    pub fn redundant_user_patterns(&self) -> Vec<&str> {
        let Some(user) = self.user.as_deref() else {
            return Vec::new();
        };
        if !user.walked.load(Ordering::Relaxed) {
            return Vec::new();
        }
        user.patterns
            .iter()
            .zip(&user.effective)
            .filter(|(pattern, effective)| {
                !effective.load(Ordering::Relaxed) && !names_hidden_directory(pattern)
            })
            .map(|(pattern, _)| pattern.as_str())
            .collect()
    }

    /// True when the path is ignored: an ignore glob matches it and no `!`
    /// exception lifts it.
    pub fn is_match(&self, path: impl AsRef<Path>) -> bool {
        let path = path.as_ref();
        self.patterns.is_match(path) && !self.is_lifted(path)
    }

    /// Indices of the ignore globs that match the path, without the
    /// exceptions. Callers use it only for a path that `is_match` rejected,
    /// so it also credits the project's own globs among the indices.
    pub fn excluded_matches_into(&self, path: impl AsRef<Path>, into: &mut Vec<usize>) {
        self.patterns.matches_into(path.as_ref(), into);
        self.credit_user_matches(into);
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

/// Compile `ignorePatterns` and the built-in defaults into one set, and return
/// the number of the project's own globs at the front of the set.
#[expect(
    clippy::expect_used,
    reason = "user glob patterns are validated before config resolution"
)]
pub(super) fn compile_ignore_patterns(ignore_patterns: &[String]) -> (IgnorePatternSet, usize) {
    let mut ignore_builder = GlobSetBuilder::new();
    let mut user_patterns = Vec::new();
    let mut exceptions: Vec<&str> = Vec::new();
    for pattern in ignore_patterns {
        if let Some(body) = pattern.strip_prefix('!') {
            exceptions.push(normalize_user_glob_pattern(body));
            continue;
        }
        let normalized = normalize_user_glob_pattern(pattern);
        ignore_builder.add(
            compile_path_glob(normalized)
                .expect("ignorePatterns entry was validated at config load time"),
        );
        user_patterns.push(pattern.clone());
    }

    for pattern in DEFAULT_IGNORE_PATTERNS {
        ignore_builder.add(compile_path_glob(pattern).expect("default ignore pattern is valid"));
    }

    let user_pattern_count = user_patterns.len();
    let set = IgnorePatternSet::new(ignore_builder.build().unwrap_or_default(), &exceptions)
        .with_user_patterns(user_patterns);
    (set, user_pattern_count)
}

impl From<GlobSet> for IgnorePatternSet {
    fn from(patterns: GlobSet) -> Self {
        Self::new(patterns, &[])
    }
}

/// Whether a glob has a literal path segment that starts with a dot, such as
/// `.cache` in `.cache/**` or `**/.turbo/**`.
fn names_hidden_directory(pattern: &str) -> bool {
    pattern
        .trim_start_matches("./")
        .split('/')
        .any(|segment| segment.len() > 1 && segment.starts_with('.') && segment != "..")
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

    fn user_set(user: &[&str], built_in: &[&str]) -> IgnorePatternSet {
        let mut builder = GlobSetBuilder::new();
        for pattern in user.iter().chain(built_in) {
            builder.add(globset::Glob::new(pattern).expect("valid glob"));
        }
        IgnorePatternSet::new(builder.build().expect("valid set"), &[])
            .with_user_patterns(user.iter().map(|pattern| (*pattern).to_owned()).collect())
    }

    #[test]
    fn redundant_user_patterns_need_a_walk() {
        let ignores = user_set(&["gen/**"], &["**/dist/**"]);
        assert!(ignores.redundant_user_patterns().is_empty());
        ignores.mark_walked();
        assert_eq!(ignores.redundant_user_patterns(), vec!["gen/**"]);
    }

    #[test]
    fn a_user_glob_gets_credit_only_where_no_built_in_matches() {
        let ignores = user_set(&["dist/**", "gen/**", "legacy/**"], &["**/dist/**"]);
        ignores.mark_walked();
        assert!(ignores.excludes("dist/a.ts"));
        assert!(ignores.excludes("gen/a.ts"));
        assert_eq!(
            ignores.redundant_user_patterns(),
            vec!["dist/**", "legacy/**"]
        );
        let clone = ignores.clone();
        assert!(clone.excludes("legacy/a.ts"));
        assert_eq!(ignores.redundant_user_patterns(), vec!["dist/**"]);
    }

    #[test]
    fn a_glob_that_names_a_hidden_directory_is_never_redundant() {
        let ignores = user_set(&[".cache/**", "**/.turbo/**", "*.min.ts", "./out/**"], &[]);
        ignores.mark_walked();
        assert_eq!(
            ignores.redundant_user_patterns(),
            vec!["*.min.ts", "./out/**"]
        );
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
