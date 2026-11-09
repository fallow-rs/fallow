//! Attach optional facts while reusing the semantic pass.
use super::*;

pub fn compute_semantic_usage_for_extractor_with_mode(
    program: &Program<'_>,
    extractor: &mut ModuleInfoExtractor,
    template_used: &rustc_hash::FxHashSet<String>,
    mode: fallow_types::shared_state::ExtractionMode,
) -> SemanticUsage {
    let computed_enum_key_spans = extractor.computed_enum_key_reference_spans();
    let subclass_candidates = extractor.subclass_reference_candidates();
    let callback_candidates = extractor.callback_capture_reference_candidates();
    let imported_call_candidates = extractor.imported_call_reference_candidates();
    let require_namespace_bindings = extractor.require_namespace_bindings();
    let mut semantic_usage = compute_semantic_usage_with_candidates(
        program,
        &extractor.imports,
        &extractor.exports,
        &require_namespace_bindings,
        template_used,
        SemanticReferenceCandidates {
            mode,
            module_bindings: &computed_enum_key_spans,
            subclass_bindings: &subclass_candidates,
            callback_captures: &callback_candidates,
            imported_calls: &imported_call_candidates,
        },
    );
    extractor.resolve_computed_enum_key_uses(&semantic_usage.module_binding_reference_spans);
    extractor.set_subclass_binding_provenance(
        std::mem::take(&mut semantic_usage.subclass_binding_reference_spans),
        std::mem::take(&mut semantic_usage.subclass_namespace_reference_spans),
        std::mem::take(&mut semantic_usage.subclass_immutable_reference_spans),
        std::mem::take(&mut semantic_usage.builtin_container_reference_spans),
        std::mem::take(&mut semantic_usage.subclass_binding_writes),
    );
    extractor.resolve_imported_call_sites(&semantic_usage.imported_call_reference_spans);
    extractor.set_import_binding_references(std::mem::take(
        &mut semantic_usage.import_binding_references,
    ));
    extractor.push_shared_state(std::mem::take(&mut semantic_usage.shared_state));
    extractor.push_namespace_member_uses(std::mem::take(&mut semantic_usage.namespace_member_uses));
    report_unreferenced_import_equals_bindings(
        &mut semantic_usage,
        &extractor.exported_import_equals_names,
    );
    semantic_usage
}
