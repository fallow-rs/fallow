//! Change coupling and change dispersion from the commits of the churn pass.
//!
//! Change dispersion is the number of files and lines in the commits that
//! touch a file. Change coupling is a pair of files that change together in
//! many commits. A coupled pair is hidden when no static edge (an import or a
//! re-export chain) connects the two files, so an agent that reads the import
//! graph cannot find the second file.
//!
//! The thresholds follow the Code Maat defaults: a file needs 5 revisions, a
//! pair needs 5 shared revisions and a coupling degree of at least 30%, and a
//! commit that touches more than 30 files is a bulk commit and is skipped.

use std::path::{Path, PathBuf};

use rustc_hash::{FxHashMap, FxHashSet};

use crate::churn::ChurnCommit;

/// Default maximum number of analyzed files in one commit. A larger commit
/// is a bulk change (a rename, a format pass, a dependency bump) and creates
/// pairs that are not real coupling.
pub const DEFAULT_MAX_FILES_PER_COMMIT: usize = 30;
/// Default minimum number of commits for each file of a pair.
pub const DEFAULT_MIN_REVISIONS: u32 = 5;
/// Default minimum number of commits that change both files of a pair.
pub const DEFAULT_MIN_SHARED_REVISIONS: u32 = 5;
/// Default minimum coupling degree, as a ratio in `[0, 1]`.
pub const DEFAULT_MIN_DEGREE: f64 = 0.3;
/// Coupling degree at or above which a hidden pair is strong. A strong pair
/// becomes a refactoring target.
pub const STRONG_COUPLING_DEGREE: f64 = 0.5;

/// Thresholds for change coupling.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ChangeCouplingSettings {
    /// Commits with more analyzed files than this are skipped.
    pub max_files_per_commit: usize,
    /// Minimum commits for each file of a pair.
    pub min_revisions: u32,
    /// Minimum commits that change both files.
    pub min_shared_revisions: u32,
    /// Minimum coupling degree in `[0, 1]`.
    pub min_degree: f64,
}

impl Default for ChangeCouplingSettings {
    fn default() -> Self {
        Self {
            max_files_per_commit: DEFAULT_MAX_FILES_PER_COMMIT,
            min_revisions: DEFAULT_MIN_REVISIONS,
            min_shared_revisions: DEFAULT_MIN_SHARED_REVISIONS,
            min_degree: DEFAULT_MIN_DEGREE,
        }
    }
}

impl From<&fallow_config::ChangeCouplingConfig> for ChangeCouplingSettings {
    fn from(config: &fallow_config::ChangeCouplingConfig) -> Self {
        Self {
            max_files_per_commit: config.max_files_per_commit,
            min_revisions: config.min_revisions,
            min_shared_revisions: config.min_shared_revisions,
            min_degree: config.min_degree,
        }
    }
}

/// A pair of files that change together. `path_a` sorts before `path_b`.
#[derive(Debug, Clone, PartialEq)]
pub struct CoChangePair {
    /// First file (absolute path).
    pub path_a: PathBuf,
    /// Second file (absolute path).
    pub path_b: PathBuf,
    /// Commits that change `path_a`.
    pub revisions_a: u32,
    /// Commits that change `path_b`.
    pub revisions_b: u32,
    /// Commits that change both files.
    pub shared_revisions: u32,
    /// `shared_revisions` divided by the mean of the two revision counts, in
    /// `[0, 1]`, rounded to two decimals.
    pub degree: f64,
}

impl CoChangePair {
    /// True when the degree reaches [`STRONG_COUPLING_DEGREE`].
    #[must_use]
    pub fn is_strong(&self) -> bool {
        self.degree >= STRONG_COUPLING_DEGREE
    }

    /// The other file of the pair, when `path` is one of the two files.
    #[must_use]
    pub fn partner_of(&self, path: &Path) -> Option<&Path> {
        if self.path_a == path {
            Some(&self.path_b)
        } else if self.path_b == path {
            Some(&self.path_a)
        } else {
            None
        }
    }
}

/// Change dispersion of one file.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FileDispersion {
    /// Median number of analyzed files in the commits that touch the file.
    pub median_files_per_commit: f64,
    /// Median number of changed lines (added plus deleted, analyzed files
    /// only) in the commits that touch the file.
    pub median_lines_per_commit: f64,
}

/// Result of one pass over the commit history.
#[derive(Debug, Clone, Default)]
pub struct ChangeHistoryAnalysis {
    /// Commits that touch at least one analyzed file and are not bulk commits.
    pub commits_analyzed: usize,
    /// Commits skipped because they touch more analyzed files than the cap.
    pub commits_skipped: usize,
    /// Dispersion per analyzed file (absolute path).
    pub dispersion: FxHashMap<PathBuf, FileDispersion>,
    /// Hidden pairs: thresholds met and no static edge. Sorted by degree,
    /// then shared revisions (both descending), then by path.
    pub hidden_pairs: Vec<CoChangePair>,
}

impl ChangeHistoryAnalysis {
    /// Hidden partners of `path`, strongest first.
    pub fn partners_of<'a>(
        &'a self,
        path: &'a Path,
    ) -> impl Iterator<Item = (&'a Path, &'a CoChangePair)> {
        self.hidden_pairs
            .iter()
            .filter_map(move |pair| pair.partner_of(path).map(|partner| (partner, pair)))
    }
}

/// Static edges between analyzed files, for the hidden-pair check.
///
/// Two files are statically linked when one imports the other, or imports a
/// file that re-exports the other through a re-export chain.
#[derive(Debug, Clone, Default)]
pub struct StaticLinkIndex {
    ids: FxHashMap<PathBuf, u32>,
    /// Direct import and re-export targets per file.
    targets: Vec<Vec<u32>>,
    /// Re-export sources per file, for chain traversal.
    re_export_sources: Vec<Vec<u32>>,
}

impl StaticLinkIndex {
    /// Build the index from the module graph that a dead-code pass retained.
    #[must_use]
    pub fn from_retained_graph(graph: &crate::module_graph::RetainedModuleGraph) -> Self {
        Self::from_graph(graph.as_graph())
    }

    /// Build the index from a module graph.
    #[must_use]
    pub(crate) fn from_graph(graph: &fallow_graph::graph::ModuleGraph) -> Self {
        let mut ids = FxHashMap::default();
        let mut targets = Vec::with_capacity(graph.modules.len());
        let mut re_export_sources = Vec::with_capacity(graph.modules.len());
        for node in &graph.modules {
            ids.insert(node.path.clone(), node.file_id.0);
            let mut node_targets: Vec<u32> = graph
                .edges_for(node.file_id)
                .iter()
                .map(|id| id.0)
                .collect();
            let sources: Vec<u32> = node
                .re_exports
                .iter()
                .map(|edge| edge.source_file.0)
                .collect();
            node_targets.extend(sources.iter().copied());
            node_targets.sort_unstable();
            node_targets.dedup();
            targets.push(node_targets);
            re_export_sources.push(sources);
        }
        Self {
            ids,
            targets,
            re_export_sources,
        }
    }

    /// Build an index from explicit links, for tests and embedders.
    #[must_use]
    pub fn from_links(
        files: &[PathBuf],
        imports: &[(usize, usize)],
        re_exports: &[(usize, usize)],
    ) -> Self {
        let ids = files
            .iter()
            .enumerate()
            .filter_map(|(idx, path)| u32::try_from(idx).ok().map(|idx| (path.clone(), idx)))
            .collect();
        let mut targets = vec![Vec::new(); files.len()];
        let mut re_export_sources = vec![Vec::new(); files.len()];
        for &(from, to) in imports {
            if let (Some(list), Ok(to)) = (targets.get_mut(from), u32::try_from(to)) {
                list.push(to);
            }
        }
        for &(from, to) in re_exports {
            if let (Some(list), Ok(to)) = (targets.get_mut(from), u32::try_from(to)) {
                list.push(to);
            }
            if let (Some(list), Ok(to)) = (re_export_sources.get_mut(from), u32::try_from(to)) {
                list.push(to);
            }
        }
        Self {
            ids,
            targets,
            re_export_sources,
        }
    }

    /// True when `path` is an analyzed file.
    #[must_use]
    pub fn contains(&self, path: &Path) -> bool {
        self.ids.contains_key(path)
    }

    /// True when a static edge connects `a` and `b` in either direction.
    #[must_use]
    pub fn linked(&self, a: &Path, b: &Path) -> bool {
        let (Some(&a), Some(&b)) = (self.ids.get(a), self.ids.get(b)) else {
            return false;
        };
        self.reaches(a, b) || self.reaches(b, a)
    }

    /// True when `from` imports `to`, or imports a file whose re-export chain
    /// reaches `to`.
    fn reaches(&self, from: u32, to: u32) -> bool {
        let Some(direct) = self.targets.get(from as usize) else {
            return false;
        };
        let mut seen: FxHashSet<u32> = FxHashSet::default();
        let mut queue: Vec<u32> = direct.clone();
        while let Some(current) = queue.pop() {
            if current == to {
                return true;
            }
            if !seen.insert(current) {
                continue;
            }
            if let Some(sources) = self.re_export_sources.get(current as usize) {
                queue.extend(sources.iter().copied().filter(|id| !seen.contains(id)));
            }
        }
        false
    }
}

/// Analyze change dispersion and hidden change coupling over `commits`.
///
/// Only files in `links` count. A commit with more analyzed files than
/// [`ChangeCouplingSettings::max_files_per_commit`] is skipped for both
/// dispersion and coupling.
#[must_use]
pub fn analyze_change_history(
    commits: &[ChurnCommit],
    links: &StaticLinkIndex,
    settings: &ChangeCouplingSettings,
) -> ChangeHistoryAnalysis {
    let mut interned: FxHashMap<&Path, u32> = FxHashMap::default();
    let mut paths: Vec<&Path> = Vec::new();
    let mut revisions: Vec<u32> = Vec::new();
    let mut samples: Vec<Vec<(u32, u32)>> = Vec::new();
    let mut shared: FxHashMap<(u32, u32), u32> = FxHashMap::default();
    let mut analysis = ChangeHistoryAnalysis::default();

    for commit in commits {
        let files: Vec<&crate::churn::ChurnCommitFile> = commit
            .files
            .iter()
            .filter(|file| links.contains(&file.path))
            .collect();
        if files.is_empty() {
            continue;
        }
        if files.len() > settings.max_files_per_commit {
            analysis.commits_skipped += 1;
            continue;
        }
        analysis.commits_analyzed += 1;
        let file_count = u32::try_from(files.len()).unwrap_or(u32::MAX);
        let lines = files
            .iter()
            .fold(0_u32, |sum, file| sum.saturating_add(file.lines_changed));
        let mut ids: Vec<u32> = files
            .iter()
            .map(|file| {
                *interned.entry(file.path.as_path()).or_insert_with(|| {
                    paths.push(file.path.as_path());
                    revisions.push(0);
                    samples.push(Vec::new());
                    u32::try_from(paths.len() - 1).unwrap_or(u32::MAX)
                })
            })
            .collect();
        ids.sort_unstable();
        ids.dedup();
        for &id in &ids {
            revisions[id as usize] += 1;
            samples[id as usize].push((file_count, lines));
        }
        for (index, &a) in ids.iter().enumerate() {
            for &b in &ids[index + 1..] {
                *shared.entry((a, b)).or_default() += 1;
            }
        }
    }

    for (id, file_samples) in samples.into_iter().enumerate() {
        let mut file_counts: Vec<u32> = file_samples.iter().map(|(files, _)| *files).collect();
        let mut line_counts: Vec<u32> = file_samples.iter().map(|(_, lines)| *lines).collect();
        analysis.dispersion.insert(
            paths[id].to_path_buf(),
            FileDispersion {
                median_files_per_commit: median(&mut file_counts),
                median_lines_per_commit: median(&mut line_counts),
            },
        );
    }

    for ((a, b), shared_revisions) in shared {
        let revisions_a = revisions[a as usize];
        let revisions_b = revisions[b as usize];
        if revisions_a < settings.min_revisions
            || revisions_b < settings.min_revisions
            || shared_revisions < settings.min_shared_revisions
        {
            continue;
        }
        let degree = coupling_degree(shared_revisions, revisions_a, revisions_b);
        if degree < settings.min_degree {
            continue;
        }
        let (path_a, path_b) = (paths[a as usize], paths[b as usize]);
        if links.linked(path_a, path_b) {
            continue;
        }
        let (path_a, path_b, revisions_a, revisions_b) = if path_a <= path_b {
            (path_a, path_b, revisions_a, revisions_b)
        } else {
            (path_b, path_a, revisions_b, revisions_a)
        };
        analysis.hidden_pairs.push(CoChangePair {
            path_a: path_a.to_path_buf(),
            path_b: path_b.to_path_buf(),
            revisions_a,
            revisions_b,
            shared_revisions,
            degree: (degree * 100.0).round() / 100.0,
        });
    }
    analysis.hidden_pairs.sort_by(|x, y| {
        y.degree
            .partial_cmp(&x.degree)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| y.shared_revisions.cmp(&x.shared_revisions))
            .then_with(|| x.path_a.cmp(&y.path_a))
            .then_with(|| x.path_b.cmp(&y.path_b))
    });
    analysis
}

/// A changed file whose hidden change-coupling partner is not in the change.
#[derive(Debug, Clone, PartialEq)]
pub struct CoChangeGap {
    /// The changed file (absolute path).
    pub changed: PathBuf,
    /// The hidden partner that the change does not touch (absolute path).
    pub partner: PathBuf,
    /// Commits that changed both files.
    pub shared_revisions: u32,
    /// Coupling degree in `[0, 1]`.
    pub degree: f64,
}

/// Scope filters for hidden pairs: `health.ignore`, the workspace filter, and
/// an optional diff.
pub struct PairScope<'a> {
    /// Project root. The ignore globs and the diff keys are relative to it.
    pub root: &'a Path,
    /// Compiled `health.ignore` globs, from [`health_ignore_set`].
    pub ignore_set: &'a globset::GlobSet,
    /// Workspace roots of `--workspace` or `--changed-workspaces`.
    pub ws_roots: Option<&'a [PathBuf]>,
    /// Changed lines of the diff, when the run is diff-scoped.
    pub diff_index: Option<&'a fallow_output::DiffIndex>,
}

impl PairScope<'_> {
    /// Keep a pair when neither file is ignored, at least one file is in the
    /// workspace scope, and (with a diff) at least one file is in the diff.
    #[must_use]
    pub fn contains(&self, path_a: &Path, path_b: &Path) -> bool {
        let ignored = |path: &Path| {
            !self.ignore_set.is_empty()
                && self
                    .ignore_set
                    .is_match(path.strip_prefix(self.root).unwrap_or(path))
        };
        if ignored(path_a) || ignored(path_b) {
            return false;
        }
        let in_workspace = |path: &Path| {
            self.ws_roots
                .is_none_or(|roots| roots.iter().any(|root| path.starts_with(root)))
        };
        if !in_workspace(path_a) && !in_workspace(path_b) {
            return false;
        }
        let Some(diff_index) = self.diff_index else {
            return true;
        };
        let in_diff = |path: &Path| match diff_index.key_for(path, self.root) {
            Some(rel) => diff_index.touches_file(&rel),
            None => true,
        };
        in_diff(path_a) || in_diff(path_b)
    }
}

/// Compile the `health.ignore` globs that [`PairScope`] applies.
#[must_use]
pub fn health_ignore_set(patterns: &[String]) -> globset::GlobSet {
    crate::health::ignore::build_ignore_set(patterns)
}

/// Hidden pairs with exactly one file in `changed`. Sorted by degree and then
/// shared revisions (both descending), then by changed file and partner, so a
/// cap keeps the strongest gaps.
#[must_use]
#[allow(
    clippy::implicit_hasher,
    reason = "callers pass the audit changed-file FxHashSet"
)]
pub fn co_change_gaps(
    analysis: &ChangeHistoryAnalysis,
    changed: &FxHashSet<PathBuf>,
) -> Vec<CoChangeGap> {
    let mut gaps: Vec<CoChangeGap> = analysis
        .hidden_pairs
        .iter()
        .filter_map(|pair| {
            let (changed_path, partner) = match (
                changed.contains(&pair.path_a),
                changed.contains(&pair.path_b),
            ) {
                (true, false) => (&pair.path_a, &pair.path_b),
                (false, true) => (&pair.path_b, &pair.path_a),
                _ => return None,
            };
            Some(CoChangeGap {
                changed: changed_path.clone(),
                partner: partner.clone(),
                shared_revisions: pair.shared_revisions,
                degree: pair.degree,
            })
        })
        .collect();
    gaps.sort_by(|a, b| {
        b.degree
            .partial_cmp(&a.degree)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| b.shared_revisions.cmp(&a.shared_revisions))
            .then_with(|| a.changed.cmp(&b.changed))
            .then_with(|| a.partner.cmp(&b.partner))
    });
    gaps
}

/// Code Maat coupling degree: shared commits over the mean revision count.
fn coupling_degree(shared: u32, revisions_a: u32, revisions_b: u32) -> f64 {
    let mean = f64::midpoint(f64::from(revisions_a), f64::from(revisions_b));
    if mean <= 0.0 {
        return 0.0;
    }
    (f64::from(shared) / mean).min(1.0)
}

/// Median of `values`; the mean of the two middle values for an even count.
/// Zero for an empty slice.
fn median(values: &mut [u32]) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    values.sort_unstable();
    let mid = values.len() / 2;
    if values.len() % 2 == 1 {
        f64::from(values[mid])
    } else {
        f64::midpoint(f64::from(values[mid - 1]), f64::from(values[mid]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::churn::{ChurnCommit, ChurnCommitFile};

    fn path(name: &str) -> PathBuf {
        PathBuf::from(format!("/project/src/{name}.ts"))
    }

    fn commit(id: usize, files: &[(&str, u32)]) -> ChurnCommit {
        ChurnCommit {
            sha: format!("{id:040x}"),
            committed_at: 1_700_000_000 + id as u64,
            files: files
                .iter()
                .map(|(name, lines)| ChurnCommitFile {
                    path: path(name),
                    lines_changed: *lines,
                })
                .collect(),
        }
    }

    fn index(names: &[&str]) -> StaticLinkIndex {
        let files: Vec<PathBuf> = names.iter().map(|name| path(name)).collect();
        StaticLinkIndex::from_links(&files, &[], &[])
    }

    /// `count` commits that change `a` and `b` together.
    fn together(start: usize, count: usize, a: &str, b: &str) -> Vec<ChurnCommit> {
        (start..start + count)
            .map(|id| commit(id, &[(a, 1), (b, 1)]))
            .collect()
    }

    #[test]
    fn a_pair_that_meets_every_threshold_without_a_static_edge_is_hidden() {
        let commits = together(0, 5, "a", "b");
        let analysis = analyze_change_history(
            &commits,
            &index(&["a", "b"]),
            &ChangeCouplingSettings::default(),
        );

        assert_eq!(analysis.hidden_pairs.len(), 1);
        let pair = &analysis.hidden_pairs[0];
        assert_eq!(
            (pair.path_a.clone(), pair.path_b.clone()),
            (path("a"), path("b"))
        );
        assert_eq!(pair.shared_revisions, 5);
        assert!((pair.degree - 1.0).abs() < f64::EPSILON);
        assert!(pair.is_strong());
    }

    #[test]
    fn a_pair_below_the_shared_revision_minimum_is_not_reported() {
        let commits = together(0, 4, "a", "b");
        let analysis = analyze_change_history(
            &commits,
            &index(&["a", "b"]),
            &ChangeCouplingSettings::default(),
        );

        assert!(analysis.hidden_pairs.is_empty());
    }

    #[test]
    fn a_pair_below_the_degree_minimum_is_not_reported() {
        // a: 5 shared + 30 solo = 35 revisions, b: 5. Degree 5 / 20 = 0.25.
        let mut commits = together(0, 5, "a", "b");
        commits.extend((100..130).map(|id| commit(id, &[("a", 1)])));
        let analysis = analyze_change_history(
            &commits,
            &index(&["a", "b"]),
            &ChangeCouplingSettings::default(),
        );

        assert!(analysis.hidden_pairs.is_empty());
    }

    #[test]
    fn an_import_in_either_direction_hides_nothing() {
        let commits = together(0, 6, "a", "b");
        let files = vec![path("a"), path("b")];
        let forward = StaticLinkIndex::from_links(&files, &[(0, 1)], &[]);
        let backward = StaticLinkIndex::from_links(&files, &[(1, 0)], &[]);

        for links in [forward, backward] {
            let analysis =
                analyze_change_history(&commits, &links, &ChangeCouplingSettings::default());
            assert!(analysis.hidden_pairs.is_empty());
        }
    }

    #[test]
    fn a_re_export_chain_counts_as_a_static_edge() {
        // a imports the barrel, the barrel re-exports mid, mid re-exports b.
        let commits = together(0, 6, "a", "b");
        let files = vec![path("a"), path("barrel"), path("mid"), path("b")];
        let links = StaticLinkIndex::from_links(&files, &[(0, 1)], &[(1, 2), (2, 3)]);

        let analysis = analyze_change_history(&commits, &links, &ChangeCouplingSettings::default());
        assert!(analysis.hidden_pairs.is_empty());
    }

    #[test]
    fn a_plain_import_of_an_importer_is_not_a_static_edge() {
        // a imports mid, mid imports b: no re-export, so a and b stay hidden.
        let commits = together(0, 6, "a", "b");
        let files = vec![path("a"), path("mid"), path("b")];
        let links = StaticLinkIndex::from_links(&files, &[(0, 1), (1, 2)], &[]);

        let analysis = analyze_change_history(&commits, &links, &ChangeCouplingSettings::default());
        assert_eq!(analysis.hidden_pairs.len(), 1);
    }

    #[test]
    fn bulk_commits_are_skipped_and_counted() {
        let mut commits = together(0, 4, "a", "b");
        let names: Vec<String> = (0..31).map(|i| format!("f{i}")).collect();
        let mut bulk: Vec<(&str, u32)> = names.iter().map(|name| (name.as_str(), 1)).collect();
        bulk.push(("a", 1));
        bulk.push(("b", 1));
        commits.push(commit(50, &bulk));
        let mut all: Vec<&str> = names.iter().map(String::as_str).collect();
        all.extend(["a", "b"]);

        let analysis =
            analyze_change_history(&commits, &index(&all), &ChangeCouplingSettings::default());
        assert_eq!(analysis.commits_skipped, 1);
        assert_eq!(analysis.commits_analyzed, 4);
        assert!(
            analysis.hidden_pairs.is_empty(),
            "the bulk commit must not lift the pair to 5 shared revisions"
        );
    }

    #[test]
    fn files_outside_the_analyzed_set_do_not_count() {
        // The lockfile is not analyzed: it neither pairs nor raises dispersion.
        let commits: Vec<ChurnCommit> = (0..5)
            .map(|id| commit(id, &[("a", 2), ("b", 3), ("lock", 500)]))
            .collect();
        let analysis = analyze_change_history(
            &commits,
            &index(&["a", "b"]),
            &ChangeCouplingSettings::default(),
        );

        assert_eq!(analysis.hidden_pairs.len(), 1);
        let dispersion = analysis.dispersion[&path("a")];
        assert!((dispersion.median_files_per_commit - 2.0).abs() < f64::EPSILON);
        assert!((dispersion.median_lines_per_commit - 5.0).abs() < f64::EPSILON);
        assert!(!analysis.dispersion.contains_key(&path("lock")));
    }

    #[test]
    fn dispersion_is_the_median_over_the_commits_that_touch_the_file() {
        let commits = vec![
            commit(0, &[("a", 1)]),
            commit(1, &[("a", 1), ("b", 1), ("c", 1)]),
            commit(2, &[("a", 10), ("b", 10)]),
            commit(3, &[("a", 1), ("b", 1), ("c", 1), ("d", 1)]),
        ];
        let analysis = analyze_change_history(
            &commits,
            &index(&["a", "b", "c", "d"]),
            &ChangeCouplingSettings::default(),
        );

        let a = analysis.dispersion[&path("a")];
        // File counts 1, 3, 2, 4: median 2.5. Line counts 1, 3, 20, 4: median 3.5.
        assert!((a.median_files_per_commit - 2.5).abs() < f64::EPSILON);
        assert!((a.median_lines_per_commit - 3.5).abs() < f64::EPSILON);
        let d = analysis.dispersion[&path("d")];
        assert!((d.median_files_per_commit - 4.0).abs() < f64::EPSILON);
    }

    #[test]
    fn custom_settings_lower_the_thresholds() {
        let commits = together(0, 2, "a", "b");
        let settings = ChangeCouplingSettings {
            max_files_per_commit: 30,
            min_revisions: 2,
            min_shared_revisions: 2,
            min_degree: 0.3,
        };
        let analysis = analyze_change_history(&commits, &index(&["a", "b"]), &settings);
        assert_eq!(analysis.hidden_pairs.len(), 1);
    }

    #[test]
    fn a_gap_needs_exactly_one_side_of_a_hidden_pair_in_the_change() {
        let mut commits = together(0, 5, "a", "b");
        commits.extend(together(10, 5, "c", "d"));
        let analysis = analyze_change_history(
            &commits,
            &index(&["a", "b", "c", "d"]),
            &ChangeCouplingSettings::default(),
        );
        let changed: FxHashSet<PathBuf> = [path("a"), path("c"), path("d")].into_iter().collect();

        let gaps = co_change_gaps(&analysis, &changed);
        assert_eq!(
            gaps.len(),
            1,
            "c and d changed together, so only a misses b"
        );
        assert_eq!(gaps[0].changed, path("a"));
        assert_eq!(gaps[0].partner, path("b"));
        assert_eq!(gaps[0].shared_revisions, 5);
    }

    #[test]
    fn pairs_sort_by_degree_then_shared_revisions_then_path() {
        let mut commits = together(0, 6, "c", "d");
        commits.extend(together(10, 5, "a", "b"));
        commits.extend((20..25).map(|id| commit(id, &[("e", 1), ("f", 1)])));
        commits.extend((30..35).map(|id| commit(id, &[("e", 1)])));
        let analysis = analyze_change_history(
            &commits,
            &index(&["a", "b", "c", "d", "e", "f"]),
            &ChangeCouplingSettings::default(),
        );

        let order: Vec<PathBuf> = analysis
            .hidden_pairs
            .iter()
            .map(|pair| pair.path_a.clone())
            .collect();
        assert_eq!(order, vec![path("c"), path("a"), path("e")]);
        assert!((analysis.hidden_pairs[2].degree - 0.67).abs() < f64::EPSILON);
    }
}
