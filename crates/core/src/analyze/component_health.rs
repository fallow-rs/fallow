use fallow_config::Severity;
use fallow_types::output_dead_code::{DuplicatePropShapeFinding, ThinWrapperFinding};

use crate::suppress::IssueKind;

use super::{
    FrameworkSpecificFindingsInput, duplicate_prop_shape::find_duplicate_prop_shapes,
    retain_unsuppressed, thin_wrapper::find_thin_wrappers,
};

pub(super) fn populate_thin_wrapper_findings(input: &mut FrameworkSpecificFindingsInput<'_>) {
    if input.config.rules.thin_wrapper == Severity::Off
        && !input.config.overrides.iter().any(|override_config| {
            override_config
                .rules
                .thin_wrapper
                .is_some_and(|severity| severity != Severity::Off)
        })
    {
        return;
    }
    let findings = collect_thin_wrapper_findings(input);
    input.results.thin_wrappers =
        retain_unsuppressed(input, findings, IssueKind::ThinWrapper, |f| {
            Some((f.wrapper.file.as_path(), f.wrapper.line))
        });
}

fn collect_thin_wrapper_findings(
    input: &FrameworkSpecificFindingsInput<'_>,
) -> Vec<ThinWrapperFinding> {
    let scan = find_thin_wrappers(
        input.graph,
        input.modules,
        input.resolved_modules,
        input.declared_deps,
        input.line_offsets_by_file,
    );
    if scan.components_scanned > 0 {
        tracing::info!(
            components_scanned = scan.components_scanned,
            thin_wrappers = scan.wrappers.len(),
            "React detected, {} component(s) scanned for thin wrappers",
            scan.components_scanned
        );
    }
    scan.wrappers
        .into_iter()
        .map(ThinWrapperFinding::with_actions)
        .collect()
}

/// Multi-file suppress model: a per-member finding
/// is dropped by a line-level (`// fallow-ignore-next-line duplicate-prop-shape`
/// at its component definition) or a file-level
/// (`// fallow-ignore-file duplicate-prop-shape`) suppress, but the suppressed
/// member STILL appears in its siblings' `sharing_components`, because the
/// `sharing_components` roster is built at emit time (before this filter) and
/// the group is real regardless of suppression.
pub(super) fn populate_duplicate_prop_shape_findings(
    input: &mut FrameworkSpecificFindingsInput<'_>,
) {
    if input.config.rules.duplicate_prop_shape == Severity::Off
        && !input.config.overrides.iter().any(|override_config| {
            override_config
                .rules
                .duplicate_prop_shape
                .is_some_and(|severity| severity != Severity::Off)
        })
    {
        return;
    }
    let scan = find_duplicate_prop_shapes(
        input.graph,
        input.modules,
        input.declared_deps,
        input.line_offsets_by_file,
    );
    if scan.components_scanned > 0 {
        tracing::info!(
            components_scanned = scan.components_scanned,
            duplicate_prop_shapes = scan.groups.len(),
            "React detected, {} component(s) scanned for duplicate prop shapes",
            scan.components_scanned
        );
    }
    // A suppression drops THIS member only. Its slot in the siblings'
    // `sharing_components` is unaffected, because the roster was built at emit
    // time.
    let findings = scan
        .groups
        .into_iter()
        .map(DuplicatePropShapeFinding::with_actions)
        .collect();
    input.results.duplicate_prop_shapes =
        retain_unsuppressed(input, findings, IssueKind::DuplicatePropShape, |f| {
            Some((f.shape.file.as_path(), f.shape.line))
        });
}
