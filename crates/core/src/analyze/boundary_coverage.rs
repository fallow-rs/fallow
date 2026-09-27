use fallow_config::ResolvedConfig;
use fallow_types::results::BoundaryCoverageViolation;

use crate::graph::ModuleGraph;
use crate::suppress::{IssueKind, SuppressionContext};

/// Detect analyzed files that are not assigned to any architecture zone.
///
/// Reachability does not gate the check: a file that no entry point reaches
/// still needs a zone (issue #2937).
pub fn find_boundary_coverage_violations(
    graph: &ModuleGraph,
    config: &ResolvedConfig,
    suppressions: &SuppressionContext<'_>,
) -> Vec<BoundaryCoverageViolation> {
    if !config.boundaries.coverage.require_all_files {
        return Vec::new();
    }

    let mut violations = Vec::new();
    for node in &graph.modules {
        if suppressions.is_file_suppressed(node.file_id, IssueKind::BoundaryViolation)
            || suppressions.is_suppressed(node.file_id, 1, IssueKind::BoundaryViolation)
        {
            continue;
        }

        let Ok(relative) = node.path.strip_prefix(&config.root) else {
            continue;
        };
        let relative = relative.to_string_lossy().replace('\\', "/");
        if config.boundaries.classify_zone(&relative).is_some()
            || config.boundaries.allows_unmatched(&relative)
        {
            continue;
        }

        violations.push(BoundaryCoverageViolation {
            path: node.path.clone(),
            line: 1,
            col: 0,
        });
    }

    violations
}
