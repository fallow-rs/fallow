//! Merge script-local extraction into host-file coordinates.
use super::*;

pub(super) fn merge_script_into_module(input: &mut SfcScriptMergeInput<'_>) {
    if input.kind == SfcKind::Vue
        && let Some(src) = &input.script.src
    {
        add_script_src_import(input.combined, src, input.script.src_span);
    }

    let allocator = Allocator::default();
    let parser_return = Parser::new(
        &allocator,
        &input.script.body,
        source_type_for_script(input.script),
    )
    .parse();
    crate::parse_degradation::ParseDegradation::from_parser(&parser_return)
        .apply_to(input.combined);
    let mut extractor = ModuleInfoExtractor::new();
    extractor.set_top_level_is_component_body(is_template_visible_script(input.kind, input.script));
    extractor.visit_program(&parser_return.program);
    let empty_template_used = FxHashSet::default();
    let mut semantic_usage = crate::parse::compute_semantic_usage_for_extractor_with_mode(
        &parser_return.program,
        &mut extractor,
        &empty_template_used,
        input.mode,
    );
    if input.kind == SfcKind::Vue || is_template_visible_script(input.kind, input.script) {
        crate::component_contracts::merge(
            &mut input.combined.component_contracts,
            std::mem::take(&mut semantic_usage.component_contracts),
            input.script.byte_offset as u32,
            Some(match input.kind {
                SfcKind::Vue => fallow_types::extract::ComponentFramework::Vue,
                SfcKind::Svelte => fallow_types::extract::ComponentFramework::Svelte,
            }),
        );
    }
    let extraction = ExtractionResult::contiguous(&input.script.body, input.script.byte_offset);
    extractor.remap_spans_with(|span| extraction.remap_span(span));
    extractor.resolve_typed_destructure_bindings();

    merge_script_binding_usage(
        input,
        &allocator,
        &extractor.imports,
        &extractor.import_equals_bindings,
        semantic_usage,
    );
    if input.need_complexity {
        input
            .combined
            .complexity
            .extend(translate_script_complexity(
                input.script,
                &parser_return.program,
                &input.combined.line_offsets,
            ));
    }

    // Vue prop/emit harvesting (`<script setup>` macros + Options API) for the
    // `unused-component-prop` / `unused-component-emit` detectors. Extracted to a
    // helper to keep this function under the unit-size lint.
    if input.kind == SfcKind::Vue {
        merge_vue_props_emits_into(input, &parser_return.program, &mut extractor);
    }

    // Svelte 5 `$props()` rune harvesting for `unused-component-prop`. `$props`
    // is an instance-only rune, so harvest ONLY the template-visible instance
    // script, never the module script (`<script context="module">` /
    // `<script module>`).
    if input.kind == SfcKind::Svelte && is_template_visible_script(input.kind, input.script) {
        merge_svelte_props_into(
            input.combined,
            &parser_return.program,
            input.script.byte_offset,
        );
    }

    if is_template_visible_script(input.kind, input.script) {
        harvest_template_visible_bindings(input, &extractor);
    }

    // Dispatched events recorded by the visitor carry body-relative spans, like
    // props/emits above. Remap the entries this script contributes onto the SFC
    // source via the script byte offset so the finding line/col points at the
    // real `dispatch(...)` call, not a body-relative position.
    let dispatch_base = input.combined.svelte_dispatched_events.len();
    extractor.merge_into(input.combined);
    crate::visitor::mark_template_callers(input.combined);
    for event in &mut input.combined.svelte_dispatched_events[dispatch_base..] {
        event.span_start += input.script.byte_offset as u32;
    }
}
