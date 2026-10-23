use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use globset::{Glob, GlobSet, GlobSetBuilder};
use rustc_hash::FxHashMap;

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
    /// Exact package name to the config indices of the entries that name it.
    exact: FxHashMap<String, Vec<usize>>,
    globs: GlobSet,
    /// Config index of each glob in `globs`, in build order.
    glob_entries: Vec<usize>,
    usage: Option<Arc<EntryUsage>>,
}

/// Per-entry hit state, shared across clones of the resolved config.
#[derive(Debug)]
struct EntryUsage {
    entries: Vec<String>,
    /// Set when a glob entry matched a package name.
    matched: Vec<AtomicBool>,
    /// Set when the entry hid a dependency finding that the run would
    /// otherwise report.
    suppressed: Vec<AtomicBool>,
    /// Set when the declared dependencies of a manifest were checked, so an
    /// unmatched glob or an entry that hid nothing is a real signal and not
    /// only a skipped detector.
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
        let mut exact: FxHashMap<String, Vec<usize>> = FxHashMap::default();
        let mut builder = GlobSetBuilder::new();
        let mut glob_entries = Vec::new();
        for (index, entry) in entries.iter().enumerate() {
            if is_dependency_glob(entry) {
                builder.add(
                    Glob::new(entry)
                        .expect("ignoreDependencies glob was validated at config load time"),
                );
                glob_entries.push(index);
            } else {
                exact.entry(entry.clone()).or_default().push(index);
            }
        }
        let globs = builder
            .build()
            .expect("ignoreDependencies globs were validated at config load time");
        let usage = (!entries.is_empty()).then(|| {
            Arc::new(EntryUsage {
                matched: entries.iter().map(|_| AtomicBool::new(false)).collect(),
                suppressed: entries.iter().map(|_| AtomicBool::new(false)).collect(),
                entries: entries.to_vec(),
                declared_checked: AtomicBool::new(false),
            })
        });
        Self {
            exact,
            globs,
            glob_entries,
            usage,
        }
    }

    /// Whether no entries were configured.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.usage.is_none()
    }

    /// Config indices of the entries that match `package_name`.
    fn matching_entries(&self, package_name: &str) -> Vec<usize> {
        let mut indices = self.exact.get(package_name).cloned().unwrap_or_default();
        if !self.glob_entries.is_empty() {
            indices.extend(
                self.globs
                    .matches(package_name)
                    .into_iter()
                    .filter_map(|glob_index| self.glob_entries.get(glob_index).copied()),
            );
        }
        indices
    }

    /// Whether `package_name` is excluded by an exact entry or a glob entry.
    #[must_use]
    pub fn is_ignored(&self, package_name: &str) -> bool {
        if self.exact.contains_key(package_name) {
            return true;
        }
        if self.glob_entries.is_empty() {
            return false;
        }
        let indices = self.globs.matches(package_name);
        if let Some(usage) = self.usage.as_deref() {
            for &glob_index in &indices {
                if let Some(flag) = self
                    .glob_entries
                    .get(glob_index)
                    .and_then(|&index| usage.matched.get(index))
                {
                    flag.store(true, Ordering::Relaxed);
                }
            }
        }
        !indices.is_empty()
    }

    /// Record a name declared in a `package.json` dependency section. Only
    /// this check makes [`Self::unmatched_globs`] and
    /// [`Self::redundant_entries`] report anything.
    pub fn note_declared(&self, package_name: &str) {
        if let Some(usage) = self.usage.as_deref() {
            usage.declared_checked.store(true, Ordering::Relaxed);
        }
        let _ = self.is_ignored(package_name);
    }

    /// Whether an entry hides the finding that a detector found for
    /// `package_name`. Call it only for a name that the detector reports when
    /// no entry matches it: every matching entry is then recorded as one that
    /// had an effect.
    #[must_use]
    pub fn suppresses(&self, package_name: &str) -> bool {
        if !self.is_ignored(package_name) {
            return false;
        }
        if let Some(usage) = self.usage.as_deref() {
            for index in self.matching_entries(package_name) {
                if let Some(flag) = usage.suppressed.get(index) {
                    flag.store(true, Ordering::Relaxed);
                }
            }
        }
        true
    }

    /// Forget the hits of an earlier analysis pass.
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
        for flag in usage.matched.iter().chain(&usage.suppressed) {
            flag.store(false, Ordering::Relaxed);
        }
    }

    /// Glob entries that matched no dependency this run, in config order.
    ///
    /// Empty when no glob was configured or when no declared dependency was
    /// checked, because such a run says nothing about a typo.
    #[must_use]
    pub fn unmatched_globs(&self) -> Vec<&str> {
        let Some(usage) = self.checked_usage() else {
            return Vec::new();
        };
        self.glob_entries
            .iter()
            .filter(|&&index| !usage.matched[index].load(Ordering::Relaxed))
            .map(|&index| usage.entries[index].as_str())
            .collect()
    }

    /// Entries that hid no dependency finding this run, in config order.
    ///
    /// A glob that matched no package name is left out, because
    /// [`Self::unmatched_globs`] reports it. Empty when no declared
    /// dependency was checked.
    #[must_use]
    pub fn redundant_entries(&self) -> Vec<&str> {
        let Some(usage) = self.checked_usage() else {
            return Vec::new();
        };
        let unmatched_glob = |index: usize| {
            self.glob_entries.contains(&index) && !usage.matched[index].load(Ordering::Relaxed)
        };
        usage
            .entries
            .iter()
            .enumerate()
            .filter(|&(index, _)| {
                !usage.suppressed[index].load(Ordering::Relaxed) && !unmatched_glob(index)
            })
            .map(|(_, entry)| entry.as_str())
            .collect()
    }

    fn checked_usage(&self) -> Option<&EntryUsage> {
        self.usage
            .as_deref()
            .filter(|usage| usage.declared_checked.load(Ordering::Relaxed))
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
        m.note_declared("react");
        assert_eq!(m.unmatched_globs(), vec!["@typo/*"]);
    }

    #[test]
    fn clones_share_glob_usage() {
        let m = matcher(&["@acme/*"]);
        let clone = m.clone();
        clone.note_declared("@acme/lib");
        assert!(m.unmatched_globs().is_empty());
    }

    #[test]
    fn reset_usage_forgets_hits_of_an_earlier_pass() {
        let m = matcher(&["@acme/*", "lodash"]);
        m.note_declared("@acme/lib");
        assert!(m.suppresses("lodash"));
        assert!(m.unmatched_globs().is_empty());
        assert_eq!(m.redundant_entries(), vec!["@acme/*"]);

        m.reset_usage();
        assert!(
            m.unmatched_globs().is_empty() && m.redundant_entries().is_empty(),
            "a pass that checked no declared dependency reports nothing"
        );
        m.note_declared("react");
        assert_eq!(m.unmatched_globs(), vec!["@acme/*"]);
        assert_eq!(m.redundant_entries(), vec!["lodash"]);
    }

    #[test]
    fn redundant_entries_are_the_entries_that_hid_no_finding() {
        let m = matcher(&["left-pad", "@acme/*", "@typo/*", "lodash", "react"]);
        m.note_declared("left-pad");
        m.note_declared("@acme/lib");
        assert!(m.suppresses("lodash"));
        assert!(!m.suppresses("vue"));
        assert_eq!(
            m.redundant_entries(),
            vec!["left-pad", "@acme/*", "react"],
            "an unmatched glob is reported by unmatched_globs instead"
        );
        assert_eq!(m.unmatched_globs(), vec!["@typo/*"]);
    }

    #[test]
    fn a_suppressed_name_marks_every_matching_entry() {
        let m = matcher(&["@acme/*", "@acme/lib", "@acme/{lib,ui}"]);
        m.note_declared("@acme/lib");
        assert!(m.suppresses("@acme/lib"));
        assert!(m.redundant_entries().is_empty());
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
