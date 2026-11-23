//! The final step of entry-point discovery: drop duplicate paths, and keep
//! a fixture-only path only when no other entry rule seeds it.
//!
//! A plugin fixture pattern (such as `**/fixtures/**`) seeds each matching
//! file as a support entry point. The graph build checks the exports of a
//! file that only such a pattern seeds when a module outside the fixture
//! directories imports a name from it (issue #5082).

use std::path::{Path, PathBuf};

use rustc_hash::{FxHashMap, FxHashSet};

use super::{CategorizedEntryPoints, dedup_entry_paths};

impl CategorizedEntryPoints {
    #[must_use]
    pub(crate) fn dedup(mut self) -> Self {
        self.retain_fixture_only_paths();
        dedup_entry_paths(&mut self.all);
        dedup_entry_paths(&mut self.runtime);
        dedup_entry_paths(&mut self.test);
        dedup_entry_paths(&mut self.support);
        if !self.production_excluded.is_empty() {
            let seeded: FxHashSet<&Path> = self.all.iter().map(|ep| ep.path.as_path()).collect();
            self.production_excluded
                .retain(|path| !seeded.contains(path.as_path()));
            self.production_excluded.sort_unstable();
            self.production_excluded.dedup();
        }
        self
    }

    /// Keep a fixture-only path only when every entry of `all` for that path
    /// comes from a fixture pattern. Each fixture-only push adds one entry to
    /// `all`, so the two counts are equal exactly then.
    fn retain_fixture_only_paths(&mut self) {
        if self.fixture_only.is_empty() {
            return;
        }
        let mut fixture_counts: FxHashMap<&Path, usize> = FxHashMap::default();
        for path in &self.fixture_only {
            *fixture_counts.entry(path.as_path()).or_default() += 1;
        }
        let mut entry_counts: FxHashMap<&Path, usize> = FxHashMap::default();
        for entry in &self.all {
            if fixture_counts.contains_key(entry.path.as_path()) {
                *entry_counts.entry(entry.path.as_path()).or_default() += 1;
            }
        }
        let kept: FxHashSet<PathBuf> = fixture_counts
            .iter()
            .filter(|(path, count)| entry_counts.get(*path) == Some(count))
            .map(|(path, _)| path.to_path_buf())
            .collect();
        self.fixture_only.retain(|path| kept.contains(path));
        self.fixture_only.sort_unstable();
        self.fixture_only.dedup();
    }

    /// Files that only a plugin fixture pattern seeds.
    #[must_use]
    pub(crate) fn fixture_only(&self) -> &[PathBuf] {
        &self.fixture_only
    }
}
