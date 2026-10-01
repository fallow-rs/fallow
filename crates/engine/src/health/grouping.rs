//! Per-group health computation for `--group-by`.
//!
//! Partitions the project's analyzed files by an ownership resolver and
//! produces a [`HealthGroup`] for each bucket. Each group computes its own
//! `VitalSigns` / `HealthScore` from the files in that group, mirroring
//! how `--workspace` already scopes a single subset (`SubsetFilter::Paths`
//! is the underlying primitive in both cases).

use std::path::{Path, PathBuf};

use rustc_hash::{FxHashMap, FxHashSet};

use super::scoring::FileScoreOutput;
use super::{
    HealthGroupResolver, SubsetFilter, VitalSignsAndCountsInput, apply_duplication_metrics,
    compute_vital_signs_and_counts,
};
use crate::vital_signs;
use crate::{discover::FileId, duplicates::DuplicationReport, source::ModuleInfo};
use fallow_output::{
    ComplexityViolation, FileHealthScore, FindingSeverity, GroupTrendStatus, HealthActionsMeta,
    HealthFinding, HealthGroup, HealthGrouping, HealthScore, HotspotEntry, HotspotFinding,
    LargeFunctionEntry, RefactoringTarget, RefactoringTargetFinding, VitalSigns, VitalSignsCounts,
    summarize_coverage_source_consistency,
};
use fallow_types::workspace::{WorkspaceDiagnostic, WorkspaceDiagnosticKind};

/// Bucket of file paths sharing a resolver key.
struct GroupBucket {
    key: String,
    owners: Option<Vec<String>>,
    paths: FxHashSet<PathBuf>,
}

pub(super) struct HealthGroupingInput<'a> {
    pub modules: &'a [ModuleInfo],
    pub file_paths: &'a FxHashMap<FileId, &'a PathBuf>,
    pub score_output: Option<&'a FileScoreOutput>,
    pub file_scores: &'a [FileHealthScore],
    /// Findings after the baseline filter and before `--top`.
    pub findings: &'a [ComplexityViolation],
    /// Ranked hotspots before `--top`.
    pub hotspots: &'a [HotspotEntry],
    pub large_functions: &'a [LargeFunctionEntry],
    /// Refactoring targets before `--top`.
    pub targets: &'a [RefactoringTarget],
    pub score_requested: bool,
    pub dupes_report: Option<&'a DuplicationReport>,
    pub needs_file_scores: bool,
    pub needs_hotspots: bool,
    pub show_vital_signs: bool,
    pub action_ctx: &'a fallow_output::HealthActionContext,
    /// `--group` selector patterns.
    pub group_filter: Option<&'a [String]>,
    /// `--top`, applied to the lists of each group.
    pub top: Option<usize>,
}

/// The metrics of one group that the snapshot and the group trend need.
///
/// `HealthGroup::vital_signs` is absent on a score-only run, so the vital
/// signs travel here as well.
pub(super) struct GroupVitals {
    pub key: String,
    pub files_analyzed: usize,
    pub vital_signs: VitalSigns,
    pub counts: VitalSignsCounts,
    pub health_score: Option<HealthScore>,
    pub severity_critical_count: usize,
    pub hotspot_count: usize,
}

/// Grouped output and the per-group metrics behind it, in the same order.
pub(super) struct BuiltHealthGrouping {
    pub grouping: HealthGrouping,
    pub vitals: Vec<GroupVitals>,
}

/// Keep a complete copy of a list for the groups of a `--group-by` run when
/// `--top` will remove entries from the project list.
///
/// `None` means the project list is already complete, so the groups read it.
pub(super) fn untruncated_for_groups<T: Clone>(
    opts: &super::HealthExecutionOptions<'_>,
    items: &[T],
) -> Option<Vec<T>> {
    (opts.group_by.is_some() && opts.top.is_some_and(|top| items.len() > top))
        .then(|| items.to_vec())
}

/// Check the `--group` selector patterns before the analysis runs.
///
/// # Errors
///
/// Returns a message that names the first pattern that is not a valid glob.
pub fn validate_group_filter(patterns: &[String]) -> Result<(), String> {
    GroupSelector::compile(patterns).map(|_| ())
}

/// Compiled `--group` selector: exact keys, globs and `!`-prefixed
/// negations, with the `--workspace` semantics.
///
/// A key is kept when it matches a positive pattern (or when there is no
/// positive pattern) and matches no negative pattern. An exact key match wins
/// before glob matching, so a key that contains glob characters still matches
/// itself.
struct GroupSelector {
    positive: Vec<KeyPattern>,
    negative: Vec<KeyPattern>,
}

struct KeyPattern {
    raw: String,
    glob: globset::GlobMatcher,
}

impl KeyPattern {
    fn compile(raw: &str) -> Result<Self, String> {
        let glob = globset::Glob::new(raw)
            .map_err(|err| format!("invalid --group pattern '{raw}': {err}"))?
            .compile_matcher();
        Ok(Self {
            raw: raw.to_owned(),
            glob,
        })
    }

    fn matches(&self, key: &str) -> bool {
        self.raw == key || self.glob.is_match(key)
    }
}

impl GroupSelector {
    fn compile(patterns: &[String]) -> Result<Self, String> {
        let mut positive = Vec::new();
        let mut negative = Vec::new();
        for pattern in patterns {
            let trimmed = pattern.trim();
            if trimmed.is_empty() {
                continue;
            }
            if let Some(rest) = trimmed.strip_prefix('!') {
                let rest = rest.trim();
                if !rest.is_empty() {
                    negative.push(KeyPattern::compile(rest)?);
                }
            } else {
                positive.push(KeyPattern::compile(trimmed)?);
            }
        }
        Ok(Self { positive, negative })
    }

    fn keeps(&self, key: &str) -> bool {
        let included =
            self.positive.is_empty() || self.positive.iter().any(|pattern| pattern.matches(key));
        included && !self.negative.iter().any(|pattern| pattern.matches(key))
    }

    /// The positive patterns that match no key of this run.
    fn unmatched(&self, keys: &[&str]) -> Vec<String> {
        self.positive
            .iter()
            .filter(|pattern| !keys.iter().any(|key| pattern.matches(key)))
            .map(|pattern| pattern.raw.clone())
            .collect()
    }
}

/// Build [`HealthGrouping`] for the resolved `--group-by` mode.
///
/// `candidate_paths` is the set of files that already passed
/// workspace / changed-since / ignore filters, that is, the files that
/// contribute to the project-level report. Anything outside this set is
/// dropped before resolution so groups never include files the user has
/// excluded from the run.
///
/// The `--group` selector removes buckets before any per-group work, so a
/// large CODEOWNERS file costs only the groups the run keeps. The parse,
/// graph and churn work stays project-wide: the per-group metrics read
/// project-wide signals such as fan-in and dead files.
pub(super) fn build_health_grouping(
    resolver: &dyn HealthGroupResolver,
    project_root: &Path,
    candidate_paths: &FxHashSet<PathBuf>,
    input: &HealthGroupingInput<'_>,
) -> BuiltHealthGrouping {
    let mut buckets = bucket_paths(resolver, project_root, candidate_paths);
    let mut unmatched_filters = Vec::new();
    let filter = input.group_filter.map(<[String]>::to_vec);
    if let Some(patterns) = input.group_filter {
        // The CLI validates the patterns before the run, so a compile error
        // here comes only from an embedder. The run then keeps every group.
        if let Ok(selector) = GroupSelector::compile(patterns) {
            let keys: Vec<&str> = buckets.iter().map(|bucket| bucket.key.as_str()).collect();
            unmatched_filters = selector.unmatched(&keys);
            buckets.retain(|bucket| selector.keeps(&bucket.key));
        }
    }

    let (groups, vitals): (Vec<HealthGroup>, Vec<GroupVitals>) = buckets
        .into_iter()
        .map(|bucket| build_group(bucket, project_root, input))
        .unzip();

    BuiltHealthGrouping {
        grouping: HealthGrouping {
            mode: resolver.mode_label(),
            groups,
            filter,
            unmatched_filters,
        },
        vitals,
    }
}

/// Bucket every candidate path by the resolver key.
///
/// Output is sorted by descending file count with the unowned bucket pushed
/// last (matches the `dead-code` grouped output's ordering convention so that
/// human / JSON consumers see the same row ordering across analyses).
fn bucket_paths(
    resolver: &dyn HealthGroupResolver,
    project_root: &Path,
    candidate_paths: &FxHashSet<PathBuf>,
) -> Vec<GroupBucket> {
    let mut by_key: FxHashMap<String, GroupBucket> = FxHashMap::default();
    for path in candidate_paths {
        let rel = path.strip_prefix(project_root).unwrap_or(path);
        let (key, _rule) = resolver.resolve_with_rule(rel);
        let entry = by_key.entry(key.clone()).or_insert_with(|| GroupBucket {
            key: key.clone(),
            owners: resolver.section_owners_of(rel).map(<[_]>::to_vec),
            paths: FxHashSet::default(),
        });
        entry.paths.insert(path.clone());
    }
    let mut out: Vec<GroupBucket> = by_key.into_values().collect();
    out.sort_by(|a, b| {
        let unowned_a = is_unowned_label(&a.key);
        let unowned_b = is_unowned_label(&b.key);
        match (unowned_a, unowned_b) {
            (true, false) => std::cmp::Ordering::Greater,
            (false, true) => std::cmp::Ordering::Less,
            _ => b.paths.len().cmp(&a.paths.len()).then(a.key.cmp(&b.key)),
        }
    });
    out
}

fn is_unowned_label(key: &str) -> bool {
    key == crate::codeowners::UNOWNED_LABEL
}

fn build_group(
    bucket: GroupBucket,
    project_root: &Path,
    input: &HealthGroupingInput<'_>,
) -> (HealthGroup, GroupVitals) {
    let GroupBucket { key, owners, paths } = bucket;
    let subset = SubsetFilter::Paths(&paths);

    let mut group_findings = filter_group_items(input.findings, &paths, |finding| &finding.path);
    let mut group_file_scores = filter_group_items(input.file_scores, &paths, |score| &score.path);
    let mut group_hotspots = filter_group_items(input.hotspots, &paths, |hotspot| &hotspot.path);
    let mut group_targets = filter_group_items(input.targets, &paths, |target| &target.path);
    let group_large_functions =
        filter_group_items(input.large_functions, &paths, |function| &function.path);
    let total_files = paths.len();
    let (vital_signs, counts) =
        compute_group_vital_signs(input, &paths, &subset, &group_file_scores, &group_hotspots);
    let health_score = input
        .score_requested
        .then(|| vital_signs::compute_health_score(&vital_signs, total_files));

    let (severity_critical_count, severity_high_count, severity_moderate_count) =
        count_severities(&group_findings);
    let hotspot_count = group_hotspots.len();
    if let Some(top) = input.top {
        group_findings.truncate(top);
        group_file_scores.truncate(top);
        group_hotspots.truncate(top);
        group_targets.truncate(top);
    }

    let functions_above_threshold = group_findings.len();
    let coverage_source_consistency = summarize_coverage_source_consistency(
        group_findings
            .iter()
            .filter_map(|finding| finding.coverage_source),
    );

    let vitals = GroupVitals {
        key: key.clone(),
        files_analyzed: total_files,
        vital_signs: vital_signs.clone(),
        counts,
        health_score: health_score.clone(),
        severity_critical_count,
        hotspot_count,
    };
    let group = HealthGroup {
        key,
        owners,
        files_analyzed: total_files,
        functions_above_threshold,
        severity_critical_count,
        severity_high_count,
        severity_moderate_count,
        hotspot_count,
        coverage_source_consistency,
        vital_signs: input.show_vital_signs.then_some(vital_signs),
        health_score,
        trend: None,
        trend_status: None,
        findings: wrap_group_findings(group_findings, input),
        file_scores: group_file_scores,
        hotspots: wrap_group_hotspots(group_hotspots, project_root),
        large_functions: group_large_functions,
        targets: group_targets
            .into_iter()
            .map(RefactoringTargetFinding::with_actions)
            .collect(),
        actions_meta: group_actions_meta(input),
    };
    (group, vitals)
}

fn count_severities(findings: &[ComplexityViolation]) -> (usize, usize, usize) {
    findings.iter().fold(
        (0, 0, 0),
        |(critical, high, moderate), finding| match finding.severity {
            FindingSeverity::Critical => (critical + 1, high, moderate),
            FindingSeverity::High => (critical, high + 1, moderate),
            FindingSeverity::Moderate => (critical, high, moderate + 1),
        },
    )
}

fn filter_group_items<T: Clone>(
    items: &[T],
    paths: &FxHashSet<PathBuf>,
    path: impl Fn(&T) -> &PathBuf,
) -> Vec<T> {
    items
        .iter()
        .filter(|item| paths.contains(path(item)))
        .cloned()
        .collect()
}

fn compute_group_vital_signs(
    input: &HealthGroupingInput<'_>,
    paths: &FxHashSet<PathBuf>,
    subset: &SubsetFilter<'_>,
    group_file_scores: &[FileHealthScore],
    group_hotspots: &[HotspotEntry],
) -> (VitalSigns, VitalSignsCounts) {
    let vital_signs_input = VitalSignsAndCountsInput {
        score_output: input.score_output,
        modules: input.modules,
        file_paths: input.file_paths,
        needs_file_scores: input.needs_file_scores,
        file_scores_slice: group_file_scores,
        needs_hotspots: input.needs_hotspots,
        hotspots: group_hotspots,
        total_files: paths.len(),
        subset,
    };
    let (mut vital_signs, mut counts) = compute_vital_signs_and_counts(&vital_signs_input);
    apply_group_duplication_metrics(input, paths, &mut vital_signs, &mut counts);
    (vital_signs, counts)
}

fn apply_group_duplication_metrics(
    input: &HealthGroupingInput<'_>,
    paths: &FxHashSet<PathBuf>,
    vital_signs: &mut VitalSigns,
    counts: &mut VitalSignsCounts,
) {
    let Some(report) = input.dupes_report else {
        return;
    };
    let dupes_report = subset_duplication_report(report, input, paths);
    apply_duplication_metrics(vital_signs, counts, &dupes_report);
}

fn subset_duplication_report(
    report: &DuplicationReport,
    input: &HealthGroupingInput<'_>,
    paths: &FxHashSet<PathBuf>,
) -> DuplicationReport {
    let clone_groups = report
        .clone_groups
        .iter()
        .filter_map(|group| {
            let instances = group
                .instances
                .iter()
                .filter(|instance| paths.contains(&instance.file))
                .cloned()
                .collect::<Vec<_>>();
            (instances.len() > 1).then_some(fallow_types::duplicates::CloneGroup {
                instances,
                token_count: group.token_count,
                line_count: group.line_count,
                similarity: group.similarity,
            })
        })
        .collect::<Vec<_>>();
    DuplicationReport {
        stats: subset_duplication_stats(report, input, paths, &clone_groups),
        clone_groups,
        clone_families: Vec::new(),
        mirrored_directories: Vec::new(),
    }
}

fn subset_duplication_stats(
    report: &DuplicationReport,
    input: &HealthGroupingInput<'_>,
    paths: &FxHashSet<PathBuf>,
    clone_groups: &[fallow_types::duplicates::CloneGroup],
) -> fallow_types::duplicates::DuplicationStats {
    let mut files_with_clones: FxHashSet<&Path> = FxHashSet::default();
    let mut file_dup_lines: rustc_hash::FxHashMap<&Path, FxHashSet<usize>> =
        rustc_hash::FxHashMap::default();
    let mut duplicated_tokens = 0usize;
    let mut clone_instances = 0usize;

    for group in clone_groups {
        for instance in &group.instances {
            files_with_clones.insert(&instance.file);
            clone_instances += 1;
            let lines = file_dup_lines.entry(&instance.file).or_default();
            for line in instance.start_line..=instance.end_line {
                lines.insert(line);
            }
        }
        duplicated_tokens += group.token_count * group.instances.len().saturating_sub(1);
    }

    let duplicated_lines = file_dup_lines.values().map(FxHashSet::len).sum::<usize>();
    let total_lines = total_lines_for_paths(input, paths);

    fallow_types::duplicates::DuplicationStats {
        total_files: paths.len(),
        files_with_clones: files_with_clones.len(),
        total_lines,
        duplicated_lines,
        total_tokens: report.stats.total_tokens,
        duplicated_tokens: duplicated_tokens.min(report.stats.total_tokens),
        clone_groups: clone_groups.len(),
        // The scoped report carries no families, so nothing is withheld from an
        // empty array.
        clone_families: 0,
        clone_instances,
        duplication_percentage: if total_lines > 0 {
            (duplicated_lines as f64 / total_lines as f64) * 100.0
        } else {
            0.0
        },
        clone_groups_below_min_occurrences: report.stats.clone_groups_below_min_occurrences,
        clone_groups_ignored: report.stats.clone_groups_ignored,
        near_candidates_skipped: report.stats.near_candidates_skipped,
    }
}

fn total_lines_for_paths(input: &HealthGroupingInput<'_>, paths: &FxHashSet<PathBuf>) -> usize {
    input
        .modules
        .iter()
        .filter_map(|module| {
            let path = input.file_paths.get(&module.file_id)?;
            paths.contains(*path).then_some(module.line_offsets.len())
        })
        .sum()
}

fn wrap_group_findings(
    findings: Vec<ComplexityViolation>,
    input: &HealthGroupingInput<'_>,
) -> Vec<HealthFinding> {
    findings
        .into_iter()
        .map(|finding| HealthFinding::with_actions(finding, input.action_ctx))
        .collect()
}

fn wrap_group_hotspots(hotspots: Vec<HotspotEntry>, project_root: &Path) -> Vec<HotspotFinding> {
    hotspots
        .into_iter()
        .map(|hotspot| HotspotFinding::with_actions(hotspot, project_root))
        .collect()
}

fn group_actions_meta(input: &HealthGroupingInput<'_>) -> Option<HealthActionsMeta> {
    input
        .action_ctx
        .opts
        .omit_suppress_line
        .then(|| HealthActionsMeta {
            suppression_hints_omitted: true,
            reason: input
                .action_ctx
                .opts
                .omit_reason
                .unwrap_or("unspecified")
                .to_string(),
            scope: "health-findings".to_string(),
        })
}

/// Group data for the snapshot of a grouped run.
pub(super) fn snapshot_grouping(built: &BuiltHealthGrouping) -> fallow_output::SnapshotGrouping {
    fallow_output::SnapshotGrouping {
        grouped_by: built.grouping.mode.to_owned(),
        group_filter: built.grouping.filter.clone(),
        groups: built
            .vitals
            .iter()
            .map(|vitals| fallow_output::GroupSnapshot {
                key: vitals.key.clone(),
                files_analyzed: vitals.files_analyzed,
                vital_signs: vitals.vital_signs.clone(),
                counts: vitals.counts.clone(),
                score: vitals.health_score.as_ref().map(|score| score.score),
                grade: vitals
                    .health_score
                    .as_ref()
                    .map(|score| score.grade.to_string()),
                severity_critical_count: vitals.severity_critical_count,
                hotspot_count: vitals.hotspot_count,
            })
            .collect(),
    }
}

/// Compare each group against the same group key in the trend baseline.
///
/// Groups match by key, and only when the baseline was saved with the same
/// `--group-by` mode. When the baseline holds no usable group data, every
/// group gets `trend_status: no_group_baseline` and the run records one
/// `trend-group-baseline-unavailable` diagnostic. A group that the `--group`
/// selection of the baseline run left out also gets `no_group_baseline`,
/// because that run did not measure it. The project trend is not affected.
#[expect(
    clippy::print_stderr,
    reason = "the stderr note mirrors the diagnostic for a human reader, as the other trend notes do"
)]
pub(super) fn apply_group_trends(
    built: &mut BuiltHealthGrouping,
    baseline: &vital_signs::TrendBaseline,
    root: &Path,
    quiet: bool,
) {
    let mode = built.grouping.mode;
    let stored = baseline
        .snapshot
        .groups
        .as_ref()
        .filter(|stored| stored.grouped_by == mode);
    let Some(stored) = stored else {
        let cause = if baseline.snapshot.groups.is_some() {
            "grouped-by-mismatch"
        } else {
            "snapshot-has-no-groups"
        };
        let kind = WorkspaceDiagnosticKind::TrendGroupBaselineUnavailable {
            cause: cause.to_owned(),
        };
        if !quiet {
            let diagnostic = WorkspaceDiagnostic::new(root, baseline.path.clone(), kind.clone());
            eprintln!("note: {}", diagnostic.message);
        }
        super::diagnostics::record_health_diagnostic(root, Some(&baseline.path), kind);
        for group in &mut built.grouping.groups {
            group.trend_status = Some(GroupTrendStatus::NoGroupBaseline);
        }
        return;
    };
    let base = baseline.without_groups();
    let stored_selection = stored_group_selector(stored);
    for (group, vitals) in built.grouping.groups.iter_mut().zip(&built.vitals) {
        let previous = stored
            .groups
            .iter()
            .find(|previous| previous.key == vitals.key);
        if let Some(previous) = previous {
            group.trend = Some(vital_signs::compute_group_trend(
                &base,
                baseline.snapshots_loaded,
                previous,
                &vitals.vital_signs,
                &vitals.counts,
                vitals.health_score.as_ref().map(|score| score.score),
            ));
            group.trend_status = Some(GroupTrendStatus::Compared);
        } else {
            group.trend_status = Some(absent_group_status(&stored_selection, &vitals.key));
        }
    }
}

/// The `--group` selection of the baseline run.
enum StoredSelection {
    /// The baseline run kept every group.
    All,
    /// The baseline run kept only the groups this selector keeps.
    Filtered(GroupSelector),
    /// The stored patterns do not compile, so the groups that the baseline
    /// measured are not known.
    Unknown,
}

fn stored_group_selector(stored: &fallow_output::SnapshotGrouping) -> StoredSelection {
    match stored.group_filter.as_deref() {
        None => StoredSelection::All,
        Some(patterns) => GroupSelector::compile(patterns)
            .map_or(StoredSelection::Unknown, StoredSelection::Filtered),
    }
}

/// The trend status of a group that the baseline does not hold.
///
/// The group is new only when the baseline run kept its key. When the stored
/// selection left the key out, or cannot be read, the baseline did not
/// measure the group.
fn absent_group_status(selection: &StoredSelection, key: &str) -> GroupTrendStatus {
    match selection {
        StoredSelection::All => GroupTrendStatus::NewGroup,
        StoredSelection::Filtered(selector) if selector.keeps(key) => GroupTrendStatus::NewGroup,
        StoredSelection::Filtered(_) | StoredSelection::Unknown => {
            GroupTrendStatus::NoGroupBaseline
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn selector(patterns: &[&str]) -> GroupSelector {
        let patterns: Vec<String> = patterns.iter().map(|p| (*p).to_owned()).collect();
        GroupSelector::compile(&patterns).expect("valid patterns")
    }

    #[test]
    fn selector_keeps_exact_glob_and_negated_keys() {
        let exact = selector(&["@elastic/search-ml-ux"]);
        assert!(exact.keeps("@elastic/search-ml-ux"));
        assert!(!exact.keeps("@elastic/kibana-core"));

        let glob = selector(&["@elastic/search-*"]);
        assert!(glob.keeps("@elastic/search-ml-ux"));
        assert!(!glob.keeps("@elastic/kibana-core"));

        let negated = selector(&["!(unowned)"]);
        assert!(negated.keeps("@elastic/kibana-core"));
        assert!(!negated.keeps("(unowned)"));

        let both = selector(&["@elastic/*", "!@elastic/kibana-core"]);
        assert!(both.keeps("@elastic/search-ml-ux"));
        assert!(!both.keeps("@elastic/kibana-core"));
    }

    #[test]
    fn absent_group_is_new_only_when_the_stored_selection_kept_it() {
        let filtered = StoredSelection::Filtered(selector(&["@team/*"]));
        assert_eq!(
            absent_group_status(&filtered, "@team/c"),
            GroupTrendStatus::NewGroup
        );
        assert_eq!(
            absent_group_status(&filtered, "@other/d"),
            GroupTrendStatus::NoGroupBaseline
        );
        assert_eq!(
            absent_group_status(&StoredSelection::All, "@other/d"),
            GroupTrendStatus::NewGroup
        );
        assert_eq!(
            absent_group_status(&StoredSelection::Unknown, "@team/c"),
            GroupTrendStatus::NoGroupBaseline
        );
    }

    #[test]
    fn selector_matches_a_key_with_glob_characters_exactly() {
        let exact = selector(&["web-[staging]"]);
        assert!(exact.keeps("web-[staging]"));
    }

    #[test]
    fn selector_reports_positive_patterns_without_a_match() {
        let s = selector(&["@team/a", "@nobody/*", "!@team/b"]);
        assert_eq!(s.unmatched(&["@team/a", "@team/b"]), vec!["@nobody/*"]);
    }

    #[test]
    fn invalid_glob_is_a_validation_error() {
        let error = validate_group_filter(&["src/[".to_owned()]).expect_err("invalid glob");
        assert!(error.contains("invalid --group pattern 'src/['"), "{error}");
    }
}
