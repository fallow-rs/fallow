//! Effect finding population.

use super::*;

/// Populate the three Effect rules. Gated on the project declaring `effect`
/// inside the detector (see [`effect::find_effect_findings`]). A line-level
/// or file-level suppression drops a finding. A suppressed
/// `unused-tagged-error` finding does not cover its `catchTag` handlers.
pub(super) fn populate_effect_findings(input: &mut FrameworkSpecificFindingsInput<'_>) {
    let rules = &input.config.rules;
    let layers_enabled = rules.unused_effect_layers != Severity::Off;
    let errors_enabled = rules.unused_tagged_errors != Severity::Off;
    let catch_tags_enabled = rules.unproduced_catch_tags != Severity::Off;
    if !layers_enabled && !errors_enabled && !catch_tags_enabled {
        return;
    }
    let effect_input = effect::EffectInput {
        graph: input.graph,
        modules: input.modules,
        resolved_modules: input.resolved_modules,
        declared_deps: input.declared_deps,
        public_api_entry_points: input.public_api_entry_points,
        line_offsets_by_file: input.line_offsets_by_file,
        layers_enabled,
        errors_enabled,
        catch_tags_enabled,
    };
    let suppressions = input.suppressions;
    let findings = effect::find_effect_findings(&effect_input, |file_id, line| {
        suppressions.is_suppressed(file_id, line, IssueKind::UnusedTaggedError)
            || suppressions.is_file_suppressed(file_id, IssueKind::UnusedTaggedError)
    });
    input.results.unused_effect_layers =
        retain_unsuppressed(input, findings.layers, IssueKind::UnusedEffectLayer, |f| {
            Some((f.path.as_path(), f.line))
        })
        .into_iter()
        .map(UnusedEffectLayerFinding::with_actions)
        .collect();
    input.results.unused_tagged_errors = findings
        .errors
        .into_iter()
        .map(UnusedTaggedErrorFinding::with_actions)
        .collect();
    input.results.unproduced_catch_tags = retain_unsuppressed(
        input,
        findings.catch_tags,
        IssueKind::UnproducedCatchTag,
        |f| Some((f.path.as_path(), f.line)),
    )
    .into_iter()
    .map(UnproducedCatchTagFinding::with_actions)
    .collect();
}
