use std::path::{Path, PathBuf};

use rustc_hash::FxHashSet;

use crate::baseline::{HealthBaselineData, filter_new_runtime_coverage_findings};

/// Inputs to runtime-coverage post-processing. Boxed into a struct so the
/// signature does not creep past the workspace `clippy::too_many_arguments`
/// threshold as new filter axes land.
pub(super) struct RuntimeCoverageFilterContext<'a> {
    pub baseline: Option<&'a HealthBaselineData>,
    pub root: &'a Path,
    pub top: Option<usize>,
    pub changed_files: Option<&'a FxHashSet<PathBuf>>,
    pub diff_index: Option<&'a fallow_output::DiffIndex>,
}

impl<'a> RuntimeCoverageFilterContext<'a> {
    pub(super) fn new(root: &'a Path) -> Self {
        Self {
            baseline: None,
            root,
            top: None,
            changed_files: None,
            diff_index: None,
        }
    }

    pub(super) fn with_baseline(mut self, baseline: Option<&'a HealthBaselineData>) -> Self {
        self.baseline = baseline;
        self
    }

    pub(super) fn with_top(mut self, top: Option<usize>) -> Self {
        self.top = top;
        self
    }

    pub(super) fn with_changed_files(
        mut self,
        changed_files: Option<&'a FxHashSet<PathBuf>>,
    ) -> Self {
        self.changed_files = changed_files;
        self
    }

    pub(super) fn with_diff_index(
        mut self,
        diff_index: Option<&'a fallow_output::DiffIndex>,
    ) -> Self {
        self.diff_index = diff_index;
        self
    }

    /// True when ANY change-scope signal is in play. Used by the verdict logic
    /// to disambiguate PR-review context from standalone analysis.
    fn has_change_scope(&self) -> bool {
        self.diff_index.is_some() || self.changed_files.is_some()
    }
}

pub(super) fn apply_runtime_coverage_filters(
    report: &mut fallow_output::RuntimeCoverageReport,
    ctx: &RuntimeCoverageFilterContext<'_>,
) {
    if let Some(baseline) = ctx.baseline {
        report.findings = filter_new_runtime_coverage_findings(
            std::mem::take(&mut report.findings),
            baseline,
            ctx.root,
        );
    }

    let changed_review = retain_hot_paths_in_change_scope(report, ctx);

    refresh_runtime_coverage_verdict(report, changed_review);

    if let Some(top) = ctx.top {
        report.findings.truncate(top);
        report.hot_paths.truncate(top);
    }
}

fn retain_hot_paths_in_change_scope(
    report: &mut fallow_output::RuntimeCoverageReport,
    ctx: &RuntimeCoverageFilterContext<'_>,
) -> bool {
    if !ctx.has_change_scope() {
        return false;
    }

    report.hot_paths.retain(|hot_path| {
        if let Some(diff_index) = ctx.diff_index
            && let Some(rel) = relative_to_root(&hot_path.path, ctx.root)
            && diff_index.touches_file(&rel)
        {
            let Some(added) = diff_index.added_lines_in(&rel) else {
                return false;
            };
            let start = u64::from(hot_path.line);
            let end = if hot_path.end_line == 0 {
                start
            } else {
                u64::from(hot_path.end_line)
            };
            return added.iter().any(|&line| line >= start && line <= end);
        }

        if let Some(changed_files) = ctx.changed_files {
            let absolute = if fallow_types::path_util::is_absolute_path_any_platform(&hot_path.path)
            {
                hot_path.path.clone()
            } else {
                ctx.root.join(&hot_path.path)
            };
            return changed_files.contains(&absolute) || changed_files.contains(&hot_path.path);
        }

        false
    });

    true
}

pub(super) fn relative_to_root(path: &Path, root: &Path) -> Option<String> {
    if let Ok(stripped) = path.strip_prefix(root) {
        return Some(stripped.to_string_lossy().replace('\\', "/"));
    }
    if fallow_types::path_util::is_absolute_path_any_platform(path) {
        return None;
    }
    Some(path.to_string_lossy().replace('\\', "/"))
}

fn refresh_runtime_coverage_verdict(
    report: &mut fallow_output::RuntimeCoverageReport,
    pr_context: bool,
) {
    let has_cold_signal = report.findings.iter().any(|finding| {
        matches!(
            finding.verdict,
            fallow_output::RuntimeCoverageVerdict::SafeToDelete
                | fallow_output::RuntimeCoverageVerdict::ReviewRequired
                | fallow_output::RuntimeCoverageVerdict::LowTraffic
        )
    });
    let has_changed_hot_path = pr_context && !report.hot_paths.is_empty();
    let has_license_grace = matches!(
        report.verdict,
        fallow_output::RuntimeCoverageReportVerdict::LicenseExpiredGrace
    ) || matches!(
        report.watermark,
        Some(fallow_output::RuntimeCoverageWatermark::LicenseExpiredGrace)
    );

    report.signals =
        build_runtime_coverage_signals(has_license_grace, has_cold_signal, has_changed_hot_path);

    report.verdict = pick_primary_verdict(
        has_license_grace,
        has_cold_signal,
        has_changed_hot_path,
        pr_context,
    );
}

fn build_runtime_coverage_signals(
    has_license_grace: bool,
    has_cold_signal: bool,
    has_changed_hot_path: bool,
) -> Vec<fallow_output::RuntimeCoverageSignal> {
    let mut signals = Vec::new();
    if has_license_grace {
        signals.push(fallow_output::RuntimeCoverageSignal::LicenseExpiredGrace);
    }
    if has_cold_signal {
        signals.push(fallow_output::RuntimeCoverageSignal::ColdCodeDetected);
    }
    if has_changed_hot_path {
        signals.push(fallow_output::RuntimeCoverageSignal::HotPathTouched);
    }
    signals
}

fn pick_primary_verdict(
    has_license_grace: bool,
    has_cold_signal: bool,
    has_changed_hot_path: bool,
    pr_context: bool,
) -> fallow_output::RuntimeCoverageReportVerdict {
    if has_license_grace {
        return fallow_output::RuntimeCoverageReportVerdict::LicenseExpiredGrace;
    }
    if pr_context {
        if has_changed_hot_path {
            return fallow_output::RuntimeCoverageReportVerdict::HotPathTouched;
        }
        if has_cold_signal {
            return fallow_output::RuntimeCoverageReportVerdict::ColdCodeDetected;
        }
    } else {
        if has_cold_signal {
            return fallow_output::RuntimeCoverageReportVerdict::ColdCodeDetected;
        }
        if has_changed_hot_path {
            return fallow_output::RuntimeCoverageReportVerdict::HotPathTouched;
        }
    }
    fallow_output::RuntimeCoverageReportVerdict::Clean
}
