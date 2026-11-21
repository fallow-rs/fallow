//! Run of the enum, class, store, namespace and protected member detectors.

use super::*;

pub(super) fn run_member_detectors(input: MemberDetectorInput<'_>) -> AnalysisResults {
    let mut results = AnalysisResults::default();
    let store_members_active = store_member_rule_is_active(input.config, input.declared_deps);
    if member_rules_are_disabled(input.config, store_members_active) {
        return results;
    }

    let member_results = find_unused_members_with_public_api_entry_points(UnusedMemberScanInput {
        graph: input.graph,
        resolved_modules: input.resolved_modules,
        modules: input.modules,
        suppressions: input.suppressions,
        line_offsets_by_file: input.line_offsets_by_file,
        user_class_member_allowlist: input.user_class_members,
        semantic_framework_candidates: input.semantic_framework_candidates,
        ignore_decorators: &input.config.ignore_decorators,
        public_api_entry_points: input.public_api_entry_points,
        entry_exports_config: Some(input.config),
        lit_active: lit_is_declared(input.declared_deps),
        report_unmatched_patterns: true,
    });
    populate_unused_enum_member_findings(&mut results, input.config, member_results.enum_members);
    populate_unused_class_member_findings(&mut results, input.config, member_results.class_members);
    populate_unused_store_member_findings(
        &mut results,
        store_members_active,
        member_results.store_members,
    );
    if input.config.rules.unused_namespace_members != Severity::Off {
        results.unused_namespace_members = member_results
            .namespace_members
            .into_iter()
            .map(UnusedNamespaceMemberFinding::with_actions)
            .collect();
    }
    if input.config.rules.unused_protected_class_members != Severity::Off {
        results.unused_protected_class_members = member_results
            .protected_class_members
            .into_iter()
            .map(UnusedProtectedClassMemberFinding::with_actions)
            .collect();
    }
    results
}
