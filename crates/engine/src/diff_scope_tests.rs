use std::path::{Path, PathBuf};

use fallow_output::DiffIndex;
use fallow_types::output_dead_code::*;
use fallow_types::results::*;
use rustc_hash::FxHashSet;

use super::filter_dead_code_by_diff;

const ROOT: &str = "/project";

/// A diff that adds line 11 of `src/a.ts`.
fn diff_of_a() -> DiffIndex {
    DiffIndex::from_unified_diff(
        "diff --git a/src/a.ts b/src/a.ts\n\
         --- a/src/a.ts\n\
         +++ b/src/a.ts\n\
         @@ -10,1 +10,2 @@\n\
         \x20ctx\n\
         +touched\n",
    )
}

/// A diff that renames `app/a/page.tsx` to `app/b/page.tsx` with no hunk.
fn diff_of_pure_rename() -> DiffIndex {
    DiffIndex::from_unified_diff(
        "diff --git a/app/a/page.tsx b/app/b/page.tsx\n\
         similarity index 100%\n\
         rename from app/a/page.tsx\n\
         rename to app/b/page.tsx\n",
    )
}

fn boundary_call(path: &str, line: u32) -> BoundaryCallViolationFinding {
    BoundaryCallViolationFinding::with_actions(BoundaryCallViolation {
        path: PathBuf::from(path),
        line,
        col: 0,
        zone: "ui".to_owned(),
        callee: "cp.exec".to_owned(),
        pattern: "child_process.*".to_owned(),
    })
}

fn policy(path: &str, line: u32) -> PolicyViolationFinding {
    PolicyViolationFinding::with_actions(PolicyViolation {
        path: PathBuf::from(path),
        line,
        col: 0,
        pack: "team".to_owned(),
        rule_id: "no-moment".to_owned(),
        kind: PolicyRuleKind::BannedImport,
        matched: "moment".to_owned(),
        severity: PolicyViolationSeverity::Error,
        message: None,
    })
}

fn thin_wrapper(path: &str, line: u32, severity: EffectiveSeverity) -> ThinWrapperFinding {
    let mut finding = ThinWrapperFinding::with_actions(ThinWrapper {
        file: PathBuf::from(path),
        line,
        component: "Wrapper".to_owned(),
        child_component: "Child".to_owned(),
    });
    finding.effective_severity = Some(severity);
    finding
}

fn prop_shape(path: &str, line: u32, severity: EffectiveSeverity) -> DuplicatePropShapeFinding {
    let mut finding = DuplicatePropShapeFinding::with_actions(DuplicatePropShape {
        file: PathBuf::from(path),
        line,
        component: "Card".to_owned(),
        shape: vec!["title".to_owned(), "subtitle".to_owned()],
        group_size: 2,
        sharing_components: Vec::new(),
    });
    finding.effective_severity = Some(severity);
    finding
}

fn component_findings(severity: EffectiveSeverity) -> AnalysisResults {
    let mut results = AnalysisResults::default();
    for (path, line) in [("/project/src/a.ts", 11), ("/project/src/old.ts", 3)] {
        results
            .thin_wrappers
            .push(thin_wrapper(path, line, severity));
        results
            .duplicate_prop_shapes
            .push(prop_shape(path, line, severity));
    }
    results
}

/// Route findings on the renamed `app/b/page.tsx` and on an untouched route.
fn route_findings() -> AnalysisResults {
    let mut results = AnalysisResults::default();
    for path in ["/project/app/b/page.tsx", "/project/app/other/page.tsx"] {
        results
            .route_collisions
            .push(RouteCollisionFinding::with_actions(RouteCollision {
                path: PathBuf::from(path),
                url: "/b".to_owned(),
                conflicting_paths: vec![PathBuf::from("/project/app/(g)/b/page.tsx")],
                line: 1,
                col: 0,
            }));
        results.dynamic_segment_name_conflicts.push(
            DynamicSegmentNameConflictFinding::with_actions(DynamicSegmentNameConflict {
                path: PathBuf::from(path),
                position: "/[id]".to_owned(),
                conflicting_segments: vec!["id".to_owned(), "slug".to_owned()],
                conflicting_paths: Vec::new(),
                line: 1,
                col: 0,
            }),
        );
    }
    results
}

fn route_paths(results: &AnalysisResults) -> (Vec<PathBuf>, Vec<PathBuf>) {
    (
        results
            .route_collisions
            .iter()
            .map(|f| f.collision.path.clone())
            .collect(),
        results
            .dynamic_segment_name_conflicts
            .iter()
            .map(|f| f.conflict.path.clone())
            .collect(),
    )
}

#[test]
fn diff_scope_drops_boundary_call_and_policy_findings_outside_the_diff() {
    let mut results = AnalysisResults::default();
    for (path, line) in [
        ("/project/src/a.ts", 11),
        ("/project/src/a.ts", 30),
        ("/project/src/old.ts", 3),
    ] {
        results
            .boundary_call_violations
            .push(boundary_call(path, line));
        results.policy_violations.push(policy(path, line));
    }

    filter_dead_code_by_diff(&mut results, &diff_of_a(), Path::new(ROOT));

    assert_eq!(results.boundary_call_violations.len(), 1);
    assert_eq!(results.boundary_call_violations[0].violation.line, 11);
    assert_eq!(results.policy_violations.len(), 1);
    assert_eq!(results.policy_violations[0].violation.line, 11);
}

#[test]
fn diff_scope_narrows_boundary_coverage_by_touched_file() {
    let mut results = AnalysisResults::default();
    for path in ["/project/src/a.ts", "/project/src/old.ts"] {
        results
            .boundary_coverage_violations
            .push(BoundaryCoverageViolationFinding::with_actions(
                BoundaryCoverageViolation {
                    path: PathBuf::from(path),
                    line: 1,
                    col: 0,
                },
            ));
    }

    filter_dead_code_by_diff(&mut results, &diff_of_a(), Path::new(ROOT));

    assert_eq!(results.boundary_coverage_violations.len(), 1);
    assert_eq!(
        results.boundary_coverage_violations[0].violation.path,
        PathBuf::from("/project/src/a.ts")
    );
}

#[test]
fn diff_scope_drops_enforced_component_findings_outside_the_diff() {
    let mut results = component_findings(EffectiveSeverity::Error);

    filter_dead_code_by_diff(&mut results, &diff_of_a(), Path::new(ROOT));

    assert_eq!(results.thin_wrappers.len(), 1);
    assert_eq!(
        results.thin_wrappers[0].wrapper.file,
        PathBuf::from("/project/src/a.ts")
    );
    assert_eq!(results.duplicate_prop_shapes.len(), 1);
    assert_eq!(
        results.duplicate_prop_shapes[0].shape.file,
        PathBuf::from("/project/src/a.ts")
    );
}

#[test]
fn diff_scope_keeps_advisory_component_findings() {
    let mut results = component_findings(EffectiveSeverity::Warn);

    filter_dead_code_by_diff(&mut results, &diff_of_a(), Path::new(ROOT));

    assert_eq!(results.thin_wrappers.len(), 2);
    assert_eq!(results.duplicate_prop_shapes.len(), 2);
}

#[test]
fn diff_scope_keeps_route_conflict_of_a_pure_rename() {
    let mut results = route_findings();

    filter_dead_code_by_diff(&mut results, &diff_of_pure_rename(), Path::new(ROOT));

    let renamed = vec![PathBuf::from("/project/app/b/page.tsx")];
    assert_eq!(route_paths(&results), (renamed.clone(), renamed));
}

#[test]
fn diff_and_changed_file_scopes_agree_on_route_findings() {
    let mut by_diff = route_findings();
    let mut by_changed_files = by_diff.clone();
    let changed: FxHashSet<PathBuf> =
        std::iter::once(PathBuf::from("/project/app/b/page.tsx")).collect();

    filter_dead_code_by_diff(&mut by_diff, &diff_of_pure_rename(), Path::new(ROOT));
    crate::changed_files::filter_results_by_changed_files(&mut by_changed_files, &changed);

    assert_eq!(route_paths(&by_diff), route_paths(&by_changed_files));
}

/// An alias finding with no locations falls back to the line of the alias, so
/// the diff keeps it only when that line is added.
#[test]
fn diff_scope_judges_an_alias_without_locations_by_its_own_line() {
    let mut results = AnalysisResults::default();
    for line in [11, 30] {
        results
            .duplicate_export_aliases
            .push(DuplicateExportAliasFinding::with_actions(
                DuplicateExportAlias {
                    path: PathBuf::from("/project/src/a.ts"),
                    local_name: "value".to_owned(),
                    export_names: vec!["a".to_owned(), "b".to_owned()],
                    line,
                    col: 0,
                    locations: Vec::new(),
                },
            ));
    }

    filter_dead_code_by_diff(&mut results, &diff_of_a(), Path::new(ROOT));

    let kept: Vec<u32> = results
        .duplicate_export_aliases
        .iter()
        .map(|f| f.alias.line)
        .collect();
    assert_eq!(kept, vec![11]);
}
