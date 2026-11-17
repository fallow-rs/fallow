//! Merge script-local extraction into host-file coordinates.
use super::*;

pub(super) struct SfcScriptMergeInput<'a> {
    pub(super) kind: SfcKind,
    pub(super) script: &'a SfcScript,
    /// Whether the SFC has a Vue `<script setup>` block.
    pub(super) has_setup_script: bool,
    pub(super) combined: &'a mut ModuleInfo,
    pub(super) shared_scope_references: &'a mut SharedScopeReferences,
    pub(super) template_visible_imports: &'a mut FxHashSet<String>,
    pub(super) template_visible_bound_targets: &'a mut FxHashMap<String, String>,
    pub(super) template_visible_iterable_types: &'a mut FxHashMap<String, String>,
    pub(super) props_return_binding: &'a mut Option<String>,
    pub(super) emit_return_binding: &'a mut Option<String>,
    pub(super) need_complexity: bool,
    pub(super) mode: fallow_types::shared_state::ExtractionMode,
}

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
    extractor.set_template_reads_root(
        input.kind == SfcKind::Svelte || is_template_visible_script(input.kind, input.script),
    );
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
    input
        .shared_scope_references
        .record(input.kind, input.script, &extractor, &mut semantic_usage);
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

    if shares_template_scope(input.kind, input.script, input.has_setup_script) {
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

/// The SFC compilers put the top level of every script block into one module
/// scope: Vue joins `<script>` and `<script setup>`, Svelte joins the module
/// script and the instance script. The scope is visible in one direction only:
/// `<script setup>` sees the plain `<script>` block, and the Svelte instance
/// script sees the module script. Each block is parsed alone, so a name that
/// the seeing block uses and the seen block imports is an unresolved reference
/// in the seeing block. This set collects those names and the imports of the
/// seen block.
#[derive(Default)]
pub(super) struct SharedScopeReferences {
    values: FxHashSet<String>,
    types: FxHashSet<String>,
    visible_imports: FxHashSet<String>,
}

impl SharedScopeReferences {
    fn record(
        &mut self,
        kind: SfcKind,
        script: &SfcScript,
        extractor: &ModuleInfoExtractor,
        semantic_usage: &mut crate::parse::SemanticUsage,
    ) {
        if !is_template_visible_script(kind, script) {
            self.visible_imports.extend(
                extractor
                    .imports
                    .iter()
                    .map(|import| &import.local_name)
                    .chain(&extractor.import_equals_bindings)
                    .filter(|name| !name.is_empty())
                    .cloned(),
            );
            return;
        }
        self.record_references(kind, semantic_usage);
    }

    /// Record the names that a seeing block uses but does not declare. The
    /// Vue `generic="..."` probe of `<script setup>` passes its names here too.
    pub(super) fn record_references(
        &mut self,
        kind: SfcKind,
        semantic_usage: &mut crate::parse::SemanticUsage,
    ) {
        for name in &semantic_usage.auto_import_candidates {
            // A Svelte `$name` reference subscribes to the store `name`.
            if kind == SfcKind::Svelte
                && let Some(store) = name.strip_prefix('$').filter(|store| !store.is_empty())
            {
                self.values.insert(store.to_string());
            }
            self.values.insert(name.clone());
        }
        self.types
            .extend(std::mem::take(&mut semantic_usage.unresolved_type_names));
    }
}

/// Credit each import of the seen script block that the seeing block uses.
/// Such a binding is not unused, and it gets the value or type classification
/// of the references.
pub(super) fn credit_shared_scope_references(
    references: &SharedScopeReferences,
    combined: &mut ModuleInfo,
) {
    let mut names: Vec<&String> = references
        .visible_imports
        .iter()
        .filter(|name| references.values.contains(*name) || references.types.contains(*name))
        .collect();
    if names.is_empty() {
        return;
    }
    names.sort_unstable();
    combined
        .unused_import_bindings
        .retain(|name| names.binary_search(&name).is_err());
    for name in names {
        if references.types.contains(name)
            && !combined.type_referenced_import_bindings.contains(name)
        {
            combined.type_referenced_import_bindings.push(name.clone());
        }
        if references.values.contains(name)
            && !combined.value_referenced_import_bindings.contains(name)
        {
            combined.value_referenced_import_bindings.push(name.clone());
        }
    }
}

/// Whether the template can read the top-level bindings of `script`. The
/// template of a Vue SFC with `<script setup>` sees the bindings of both script
/// blocks. A Vue SFC without `<script setup>` exposes only the component
/// instance. The Svelte markup sees the bindings of both the module script and
/// the instance script.
fn shares_template_scope(kind: SfcKind, script: &SfcScript, has_setup_script: bool) -> bool {
    match kind {
        SfcKind::Vue => script.is_setup || has_setup_script,
        SfcKind::Svelte => true,
    }
}
