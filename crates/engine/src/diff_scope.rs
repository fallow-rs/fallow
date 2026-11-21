//! Diff line filters shared by every surface that accepts a unified diff.
//!
//! The CLI (`--diff-file`, `--diff-stdin` and the CI diff) and the programmatic
//! API (`analysis.diffFile`) narrow a report to the lines a change added. Both
//! call the functions in this module, so a finding cannot pass the filter on
//! one surface and fail it on another.

use std::path::Path;

use fallow_output::DiffIndex;
use fallow_types::duplicates::{CloneInstance, DuplicationReport};
use fallow_types::results::{AnalysisResults, SecurityFinding, TraceHopRole};

use crate::result_scope::{
    Anchor, ComponentRule, OwnerRule, ProjectFinding, ResultScope, SiteRule,
};

/// Drop the dead-code findings whose source line is not on an added line of
/// the diff.
///
/// Range-shaped findings (clone instances, complexity hotspots) have their own
/// filters. This filter governs the per-file findings on [`AnalysisResults`].
/// A finding with no real line (an unused file, a route file, an unzoned file,
/// a member of a cycle) stays when the diff touches its file.
///
/// [`DiffIndex::key_for`] gives the diff key of a finding path, relative to the
/// base of the diff and not to `root`. When a path has no key (a different
/// drive, or a path outside the base), the finding stays: a path that the
/// filter cannot judge is better shown than hidden.
pub fn filter_dead_code_by_diff(results: &mut AnalysisResults, diff: &DiffIndex, root: &Path) {
    crate::result_scope::retain_in_scope(results, &DiffLines { diff, root });
}

/// A unified diff as a result scope.
struct DiffLines<'a> {
    diff: &'a DiffIndex,
    root: &'a Path,
}

impl DiffLines<'_> {
    fn touches_file(&self, path: &Path) -> bool {
        self.diff
            .key_for(path, self.root)
            .is_none_or(|rel| self.diff.touches_file(&rel))
    }

    fn line_in_diff(&self, path: &Path, line: u32) -> bool {
        self.diff
            .key_for(path, self.root)
            .is_none_or(|rel| self.diff.line_is_added(&rel, u64::from(line)))
    }
}

impl ResultScope for DiffLines<'_> {
    fn keeps(&self, anchor: Anchor<'_>) -> bool {
        match anchor {
            Anchor::Line { path, line } => self.line_in_diff(path, line),
            Anchor::File { path } => self.touches_file(path),
        }
    }

    /// Project-level findings bypass the filter. A change that deletes the
    /// last consumer of a package causes the `unused-dependency` finding, even
    /// when the diff does not touch `package.json`. The same holds for catalog
    /// entries, dependency overrides, type-only dependencies and test-only
    /// dependencies. The line filter reduces noise for source-anchored
    /// findings. CI must still fail on the project-level findings that the
    /// change caused.
    fn keeps_project_finding(&self, _finding: ProjectFinding<'_>) -> bool {
        true
    }

    fn keeps_security_finding(&self, finding: &SecurityFinding) -> bool {
        self.line_in_diff(&finding.path, finding.line)
            || finding.trace.iter().any(|hop| {
                self.line_in_diff(&hop.path, hop.line)
                    || (matches!(hop.role, TraceHopRole::SecretSource)
                        && self.touches_file(&hop.path))
            })
            || finding.reachability.as_ref().is_some_and(|reachability| {
                // Any hop on an added line keeps the finding for display, whatever
                // its role. Do not add a role check here: unlike the strict
                // `--gate new` filter of `fallow security`, the display must show a
                // finding whose module-level source sits on an added line.
                reachability
                    .untrusted_source_trace
                    .iter()
                    .any(|hop| self.line_in_diff(&hop.path, hop.line))
            })
    }

    fn site_rule(&self) -> SiteRule {
        SiteRule::TrimSites
    }

    fn owner_rule(&self) -> OwnerRule {
        OwnerRule::AnyOwner
    }

    fn component_rule(&self) -> ComponentRule {
        ComponentRule::EnforcedOnly
    }
}

/// Keep only the clone groups with at least one instance whose line range
/// overlaps an added line of the diff.
///
/// The filter keeps the whole group when one instance overlaps, so a reviewer
/// sees the full clone family in the context of the change. Clone families,
/// statistics and the order are rebuilt from the groups that stay, so the
/// duplication percentage describes the scoped slice.
pub fn filter_duplication_by_diff(report: &mut DuplicationReport, diff: &DiffIndex, root: &Path) {
    let instance_overlaps = |instance: &CloneInstance| -> bool {
        let Some(rel) = diff.key_for(&instance.file, root) else {
            return true;
        };
        let start = u64::try_from(instance.start_line).unwrap_or(u64::MAX);
        let end = u64::try_from(instance.end_line).unwrap_or(u64::MAX);
        diff.range_overlaps_added(&rel, start, end)
    };
    report
        .clone_groups
        .retain(|group| group.instances.iter().any(instance_overlaps));
    report.range_marker_problems.retain(|finding| {
        diff.key_for(&finding.path, root).is_none_or(|rel| {
            diff.range_overlaps_added(
                &rel,
                u64::from(finding.problem.line),
                u64::from(finding.problem.line),
            )
        })
    });
    crate::duplicates::refresh_scoped_report(report, root);
}

#[cfg(test)]
#[path = "diff_scope_tests.rs"]
mod tests;
