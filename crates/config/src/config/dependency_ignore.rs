use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use globset::{Glob, GlobSet, GlobSetBuilder};
use rustc_hash::FxHashSet;

/// Characters that turn an `ignoreDependencies` entry into a glob.
///
/// npm package names can not contain any of these, so an entry without them
/// keeps the historical exact-name match.
const GLOB_METACHARACTERS: [char; 4] = ['*', '?', '[', '{'];

/// Whether an `ignoreDependencies` entry is a glob pattern rather than an
/// exact package name.
#[must_use]
pub fn is_dependency_glob(entry: &str) -> bool {
    entry.contains(GLOB_METACHARACTERS)
}

/// Compiled `ignoreDependencies` entries.
///
/// Entries without glob metacharacters match a package name exactly. Entries
/// with glob metacharacters (`@acme/*`, `@types/*`) use the same glob syntax as
/// `ignorePatterns`, matched against the package name.
#[derive(Debug, Clone, Default)]
pub struct IgnoreDependencyMatcher {
    exact: FxHashSet<String>,
    globs: GlobSet,
    usage: Option<Arc<GlobUsage>>,
}

/// Per-glob hit state, shared across clones of the resolved config.
#[derive(Debug)]
struct GlobUsage {
    patterns: Vec<String>,
    matched: Vec<AtomicBool>,
    /// Set when the declared dependencies of a manifest were checked, so an
    /// unmatched glob is a real signal and not only a skipped detector.
    declared_checked: AtomicBool,
}

impl IgnoreDependencyMatcher {
    /// Compile the configured `ignoreDependencies` entries.
    ///
    /// # Panics
    ///
    /// Panics when a glob entry is invalid. Config loading validates every
    /// glob entry before resolution, so this does not happen for a loaded
    /// config.
    #[expect(
        clippy::expect_used,
        reason = "ignoreDependencies globs are validated before config resolution"
    )]
    #[must_use]
    pub fn compile(entries: &[String]) -> Self {
        let mut exact = FxHashSet::default();
        let mut builder = GlobSetBuilder::new();
        let mut patterns = Vec::new();
        for entry in entries {
            if is_dependency_glob(entry) {
                builder.add(
                    Glob::new(entry)
                        .expect("ignoreDependencies glob was validated at config load time"),
                );
                patterns.push(entry.clone());
            } else {
                exact.insert(entry.clone());
            }
        }
        let globs = builder
            .build()
            .expect("ignoreDependencies globs were validated at config load time");
        let usage = (!patterns.is_empty()).then(|| {
            Arc::new(GlobUsage {
                matched: patterns.iter().map(|_| AtomicBool::new(false)).collect(),
                patterns,
                declared_checked: AtomicBool::new(false),
            })
        });
        Self {
            exact,
            globs,
            usage,
        }
    }

    /// Whether no entries were configured.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.exact.is_empty() && self.usage.is_none()
    }

    /// Whether `package_name` is excluded by an exact entry or a glob entry.
    #[must_use]
    pub fn is_ignored(&self, package_name: &str) -> bool {
        if self.exact.contains(package_name) {
            return true;
        }
        let Some(usage) = self.usage.as_deref() else {
            return false;
        };
        let indices = self.globs.matches(package_name);
        for &index in &indices {
            if let Some(flag) = usage.matched.get(index) {
                flag.store(true, Ordering::Relaxed);
            }
        }
        !indices.is_empty()
    }

    /// Like [`Self::is_ignored`] for a name declared in a `package.json`
    /// dependency section. Only this check makes
    /// [`Self::unmatched_globs`] report anything.
    #[must_use]
    pub fn is_declared_ignored(&self, package_name: &str) -> bool {
        if let Some(usage) = self.usage.as_deref() {
            usage.declared_checked.store(true, Ordering::Relaxed);
        }
        self.is_ignored(package_name)
    }

    /// Forget the glob hits of an earlier analysis pass.
    ///
    /// A long-lived process (watch mode, the LSP, an engine session) keeps one
    /// resolved config for many passes. Each dead-code pass calls this first,
    /// so a glob that matched in an earlier pass is reported again when its
    /// dependency is gone.
    pub fn reset_usage(&self) {
        let Some(usage) = self.usage.as_deref() else {
            return;
        };
        usage.declared_checked.store(false, Ordering::Relaxed);
        for flag in &usage.matched {
            flag.store(false, Ordering::Relaxed);
        }
    }

    /// Glob entries that matched no dependency this run, in config order.
    ///
    /// Empty when no glob was configured or when no declared dependency was
    /// checked, because such a run says nothing about a typo.
    #[must_use]
    pub fn unmatched_globs(&self) -> Vec<&str> {
        let Some(usage) = self.usage.as_deref() else {
            return Vec::new();
        };
        if !usage.declared_checked.load(Ordering::Relaxed) {
            return Vec::new();
        }
        usage
            .patterns
            .iter()
            .zip(&usage.matched)
            .filter(|(_, matched)| !matched.load(Ordering::Relaxed))
            .map(|(pattern, _)| pattern.as_str())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn matcher(entries: &[&str]) -> IgnoreDependencyMatcher {
        IgnoreDependencyMatcher::compile(
            &entries
                .iter()
                .map(|entry| (*entry).to_string())
                .collect::<Vec<_>>(),
        )
    }

    #[test]
    fn empty_matcher_ignores_nothing() {
        let m = matcher(&[]);
        assert!(m.is_empty());
        assert!(!m.is_ignored("lodash"));
        assert!(m.unmatched_globs().is_empty());
    }

    #[test]
    fn exact_entry_matches_only_the_same_name() {
        let m = matcher(&["@acme/lib"]);
        assert!(m.is_ignored("@acme/lib"));
        assert!(!m.is_ignored("@acme/lib-extra"));
        assert!(!m.is_ignored("@acme/other"));
    }

    #[test]
    fn scope_glob_matches_every_package_in_the_scope() {
        let m = matcher(&["@acme/*"]);
        assert!(m.is_ignored("@acme/lib"));
        assert!(m.is_ignored("@acme/ui"));
        assert!(!m.is_ignored("@other/lib"));
        assert!(!m.is_ignored("acme"));
    }

    #[test]
    fn prefix_and_alternation_globs_match() {
        let m = matcher(&["eslint-plugin-*", "{react,react-dom}"]);
        assert!(m.is_ignored("eslint-plugin-react"));
        assert!(m.is_ignored("react-dom"));
        assert!(!m.is_ignored("eslint"));
    }

    #[test]
    fn unmatched_globs_needs_a_declared_check() {
        let m = matcher(&["@acme/*", "@typo/*", "lodash"]);
        assert!(m.is_ignored("@acme/lib"));
        assert!(
            m.unmatched_globs().is_empty(),
            "no declared dependency was checked yet"
        );
        assert!(!m.is_declared_ignored("react"));
        assert_eq!(m.unmatched_globs(), vec!["@typo/*"]);
    }

    #[test]
    fn clones_share_glob_usage() {
        let m = matcher(&["@acme/*"]);
        let clone = m.clone();
        assert!(clone.is_declared_ignored("@acme/lib"));
        assert!(m.unmatched_globs().is_empty());
    }

    #[test]
    fn reset_usage_forgets_hits_of_an_earlier_pass() {
        let m = matcher(&["@acme/*"]);
        assert!(m.is_declared_ignored("@acme/lib"));
        assert!(m.unmatched_globs().is_empty());

        m.reset_usage();
        assert!(
            m.unmatched_globs().is_empty(),
            "a pass that checked no declared dependency reports nothing"
        );
        assert!(!m.is_declared_ignored("react"));
        assert_eq!(m.unmatched_globs(), vec!["@acme/*"]);
    }

    #[test]
    fn metacharacter_detection() {
        assert!(is_dependency_glob("@acme/*"));
        assert!(is_dependency_glob("pkg-?"));
        assert!(is_dependency_glob("pkg-[ab]"));
        assert!(is_dependency_glob("{a,b}"));
        assert!(!is_dependency_glob("@acme/lib"));
        assert!(!is_dependency_glob("bun:sqlite"));
    }
}
