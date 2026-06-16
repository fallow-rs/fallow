//! Per-component React hook profile derived at the health layer from the cached
//! `hook_uses` IR.
//!
//! This module is the SOLE reader of [`ModuleInfo::hook_uses`]. The extract
//! layer records every `use*` call inside an identified React component
//! (`crates/extract/src/visitor/react.rs`) with its kind, optional literal
//! dependency-array arity, and start byte offset, then caches it. Nothing else
//! consumed that vector; complexity.rs counts hooks inline into
//! `FunctionComplexity::react_hook_count`, ignoring kind and dep-array. Here we
//! attribute each cached `HookUse` to the innermost enclosing
//! `FunctionComplexity` frame and fold a per-kind breakdown plus the max
//! `useEffect` dependency-array arity onto the per-component complexity finding.
//!
//! Anti-numerology: this is DESCRIPTIVE context only. No rule key, threshold, or
//! severity is introduced; the profile rides the existing complexity finding
//! alongside `react_hook_count` / `react_prop_count`.

use crate::health_types::ReactHookProfile;
use fallow_core::extract::ModuleInfo;
use fallow_types::extract::{FunctionComplexity, HookUse, HookUseKind};

/// Accumulator mirroring [`ReactHookProfile`] but always present per frame; the
/// caller converts it to `Option<ReactHookProfile>` only when non-empty.
#[derive(Debug, Clone, Default)]
struct ProfileAccumulator {
    state: u16,
    effect: u16,
    memo: u16,
    callback: u16,
    custom: u16,
    max_effect_dep_arity: Option<u32>,
}

impl ProfileAccumulator {
    /// Fold one attributed hook into the running per-kind counts. For
    /// `useEffect`, raise `max_effect_dep_arity` when the call carries a literal
    /// deps array (`Some(arity)`); a `None` arity never lowers the max.
    fn add(&mut self, hook: &HookUse) {
        match hook.kind {
            HookUseKind::UseState => self.state = self.state.saturating_add(1),
            HookUseKind::UseEffect => {
                self.effect = self.effect.saturating_add(1);
                if let Some(arity) = hook.dep_array_arity {
                    self.max_effect_dep_arity = Some(match self.max_effect_dep_arity {
                        Some(current) => current.max(arity),
                        None => arity,
                    });
                }
            }
            HookUseKind::UseMemo => self.memo = self.memo.saturating_add(1),
            HookUseKind::UseCallback => self.callback = self.callback.saturating_add(1),
            HookUseKind::Custom => self.custom = self.custom.saturating_add(1),
        }
    }

    fn into_profile(self) -> ReactHookProfile {
        ReactHookProfile {
            state: self.state,
            effect: self.effect,
            memo: self.memo,
            callback: self.callback,
            custom: self.custom,
            max_effect_dep_arity: self.max_effect_dep_arity,
        }
    }
}

/// Build a per-function hook profile for every frame in `module.complexity`,
/// aligned by index to `module.complexity`. Index `i` of the returned vector is
/// the profile for `module.complexity[i]`, or `None` when no component-scope
/// hook was attributed to that frame.
///
/// Empty `module.hook_uses` (every non-React file) early-returns an all-`None`
/// vector at zero attribution cost. For React files the cost is bounded by
/// `hooks * functions` (one innermost-enclosing-frame lookup per hook); health
/// is not the audit hot path, and the early-return keeps non-React repos free.
#[must_use]
pub fn build_module_hook_profiles(module: &ModuleInfo) -> Vec<Option<ReactHookProfile>> {
    let frame_count = module.complexity.len();
    if module.hook_uses.is_empty() || frame_count == 0 {
        return vec![None; frame_count];
    }

    // Precompute each frame's byte span `[start_byte, end_byte)` once.
    let frame_spans: Vec<(u32, u32)> = module
        .complexity
        .iter()
        .map(|fc| frame_byte_span(&module.line_offsets, fc))
        .collect();

    let mut accumulators: Vec<ProfileAccumulator> =
        vec![ProfileAccumulator::default(); frame_count];
    for hook in &module.hook_uses {
        if let Some(frame_idx) = innermost_enclosing_frame(&frame_spans, hook.span_start) {
            accumulators[frame_idx].add(hook);
        }
    }

    accumulators
        .into_iter()
        .map(|acc| {
            let profile = acc.into_profile();
            if profile.is_empty() {
                None
            } else {
                Some(profile)
            }
        })
        .collect()
}

/// Compute a frame's byte span `[start_byte, end_byte)` from its `(line, col)`
/// start and `line_count`. `start_byte` is the exact call/function start;
/// `end_byte` is the start of the first line AFTER the body (or `u32::MAX` when
/// the body runs to end-of-file).
fn frame_byte_span(line_offsets: &[u32], fc: &FunctionComplexity) -> (u32, u32) {
    let start_line_idx = (fc.line.saturating_sub(1)) as usize;
    let start_byte = line_offsets
        .get(start_line_idx)
        .map_or(0, |offset| offset.saturating_add(fc.col));
    // The body's last line is `fc.line + fc.line_count - 1` (1-based); the line
    // AFTER it is index `fc.line + fc.line_count - 1` (0-based).
    let after_idx = (fc.line.saturating_add(fc.line_count).saturating_sub(1)) as usize;
    let end_byte = line_offsets.get(after_idx).copied().unwrap_or(u32::MAX);
    (start_byte, end_byte)
}

/// Find the index of the INNERMOST frame whose byte span `[start, end)` contains
/// `span_start`. "Innermost" = the smallest byte-span among all containing
/// frames, mirroring complexity.rs's per-frame counting: a hook accrues to the
/// frame whose BODY directly contains the call expression. Byte-precise
/// containment (not line-range) is load-bearing for the common
/// `useEffect(() => {...}, deps)` shape: the `useEffect(` call token sits before
/// the nested arrow's `(`, so it is contained by the component frame, not by the
/// callback arrow frame that starts later on the same line.
fn innermost_enclosing_frame(frame_spans: &[(u32, u32)], span_start: u32) -> Option<usize> {
    let mut best: Option<(usize, u32)> = None;
    for (idx, &(start, end)) in frame_spans.iter().enumerate() {
        if span_start < start || span_start >= end {
            continue;
        }
        let width = end.saturating_sub(start);
        match best {
            Some((_, best_width)) if width >= best_width => {}
            _ => best = Some((idx, width)),
        }
    }
    best.map(|(idx, _)| idx)
}

#[cfg(test)]
mod tests {
    use super::*;
    use fallow_types::extract::compute_line_offsets;

    fn fc(name: &str, line: u32, line_count: u32, react_hook_count: u16) -> FunctionComplexity {
        fc_at(name, line, 0, line_count, react_hook_count)
    }

    fn fc_at(
        name: &str,
        line: u32,
        col: u32,
        line_count: u32,
        react_hook_count: u16,
    ) -> FunctionComplexity {
        FunctionComplexity {
            name: name.to_string(),
            line,
            col,
            cyclomatic: 1,
            cognitive: 0,
            line_count,
            param_count: 0,
            react_hook_count,
            react_jsx_max_depth: 0,
            react_prop_count: 0,
            source_hash: None,
            contributions: Vec::new(),
        }
    }

    fn hook(kind: HookUseKind, dep_array_arity: Option<u32>, span_start: u32) -> HookUse {
        HookUse {
            kind,
            dep_array_arity,
            span_start,
        }
    }

    /// Build a `ModuleInfo` carrying only the fields this module reads, with line
    /// offsets computed from `source` so byte spans map to real lines. All other
    /// fields are empty/default.
    fn module_with(
        source: &str,
        complexity: Vec<FunctionComplexity>,
        hook_uses: Vec<HookUse>,
    ) -> ModuleInfo {
        ModuleInfo {
            file_id: fallow_core::discover::FileId(0),
            exports: vec![],
            imports: vec![],
            re_exports: vec![],
            dynamic_imports: vec![],
            dynamic_import_patterns: vec![],
            require_calls: vec![],
            package_path_references: vec![],
            member_accesses: vec![],
            whole_object_uses: vec![],
            has_cjs_exports: false,
            has_angular_component_template_url: false,
            content_hash: 0,
            suppressions: vec![],
            unknown_suppression_kinds: vec![],
            unused_import_bindings: vec![],
            type_referenced_import_bindings: vec![],
            value_referenced_import_bindings: vec![],
            line_offsets: compute_line_offsets(source),
            complexity,
            flag_uses: vec![],
            class_heritage: vec![],
            injection_tokens: vec![],
            local_type_declarations: Vec::new(),
            public_signature_type_references: Vec::new(),
            namespace_object_aliases: Vec::new(),
            iconify_prefixes: Vec::new(),
            iconify_icon_names: Vec::new(),
            auto_import_candidates: Vec::new(),
            directives: Vec::new(),
            client_only_dynamic_import_spans: Vec::new(),
            security_sinks: Vec::new(),
            security_sinks_skipped: 0,
            security_unresolved_callee_sites: Vec::new(),
            tainted_bindings: Vec::new(),
            sanitized_sink_args: Vec::new(),
            security_control_sites: Vec::new(),
            callee_uses: Vec::new(),
            misplaced_directives: Vec::new(),
            di_key_sites: Vec::new(),
            has_dynamic_provide: false,
            referenced_import_bindings: Vec::new(),
            component_props: Vec::new(),
            has_props_attrs_fallthrough: false,
            has_define_expose: false,
            has_define_model: false,
            has_unharvestable_props: false,
            component_emits: Vec::new(),
            angular_inputs: Vec::new(),
            angular_outputs: Vec::new(),
            has_unharvestable_emits: false,
            has_dynamic_emit: false,
            has_emit_whole_object_use: false,
            load_return_keys: Vec::new(),
            has_unharvestable_load: false,
            has_load_data_whole_use: false,
            has_page_data_store_whole_use: false,
            component_functions: Vec::new(),
            react_props: Vec::new(),
            hook_uses,
            render_edges: Vec::new(),
        }
    }

    #[test]
    fn empty_hook_uses_yields_all_none() {
        let module = module_with(
            "line one\nline two\nline three\n",
            vec![fc("Foo", 1, 3, 0)],
            Vec::new(),
        );
        let profiles = build_module_hook_profiles(&module);
        assert_eq!(profiles.len(), 1);
        assert!(profiles[0].is_none());
    }

    #[test]
    fn empty_complexity_yields_empty_vec() {
        let module = module_with(
            "const x = 1;\n",
            Vec::new(),
            vec![hook(HookUseKind::UseState, None, 0)],
        );
        let profiles = build_module_hook_profiles(&module);
        assert!(profiles.is_empty());
    }

    #[test]
    fn per_kind_breakdown_and_max_effect_arity() {
        // 6 lines; one component spanning all of them. Hooks at varying offsets.
        let source = "function Comp() {\n  useState();\n  useState();\n  useEffect([a,b]);\n  useEffect([a,b,c]);\n  useMemo();\n}\n";
        let offsets = compute_line_offsets(source);
        // Offsets at the START of lines 2..=6 (1-based line -> index line-1).
        let line_start = |line: u32| offsets[(line - 1) as usize];
        let module = module_with(
            source,
            vec![fc("Comp", 1, 7, 5)],
            vec![
                hook(HookUseKind::UseState, None, line_start(2)),
                hook(HookUseKind::UseState, None, line_start(3)),
                hook(HookUseKind::UseEffect, Some(2), line_start(4)),
                hook(HookUseKind::UseEffect, Some(3), line_start(5)),
                hook(HookUseKind::UseMemo, None, line_start(6)),
            ],
        );
        let profiles = build_module_hook_profiles(&module);
        let profile = profiles[0].as_ref().expect("profile present");
        assert_eq!(profile.state, 2);
        assert_eq!(profile.effect, 2);
        assert_eq!(profile.memo, 1);
        assert_eq!(profile.callback, 0);
        assert_eq!(profile.custom, 0);
        // 5 attributed hooks; breakdown total equals react_hook_count for a
        // pure-component file.
        assert_eq!(profile.total(), 5);
        // Max over Some(2), Some(3) = 3.
        assert_eq!(profile.max_effect_dep_arity, Some(3));
    }

    #[test]
    fn none_arity_never_lowers_max() {
        let source =
            "function Comp() {\n  useEffect([a,b,c]);\n  useEffect();\n  useEffect(deps);\n}\n";
        let offsets = compute_line_offsets(source);
        let line_start = |line: u32| offsets[(line - 1) as usize];
        let module = module_with(
            source,
            vec![fc("Comp", 1, 5, 3)],
            vec![
                hook(HookUseKind::UseEffect, Some(3), line_start(2)),
                hook(HookUseKind::UseEffect, None, line_start(3)),
                hook(HookUseKind::UseEffect, None, line_start(4)),
            ],
        );
        let profiles = build_module_hook_profiles(&module);
        let profile = profiles[0].as_ref().expect("profile present");
        assert_eq!(profile.effect, 3);
        assert_eq!(profile.max_effect_dep_arity, Some(3));
    }

    #[test]
    fn no_literal_deps_leaves_arity_none() {
        let source = "function Comp() {\n  useEffect();\n}\n";
        let offsets = compute_line_offsets(source);
        let module = module_with(
            source,
            vec![fc("Comp", 1, 3, 1)],
            vec![hook(HookUseKind::UseEffect, None, offsets[1])],
        );
        let profiles = build_module_hook_profiles(&module);
        let profile = profiles[0].as_ref().expect("profile present");
        assert_eq!(profile.effect, 1);
        assert!(profile.max_effect_dep_arity.is_none());
    }

    #[test]
    fn nested_render_prop_arrow_gets_its_own_frame() {
        // Parent component lines 1..=7; nested render-prop arrow lines 3..=6 that
        // calls its own hook in its body. Byte-precise attribution routes the
        // inner hook (inside the arrow body) to the arrow, the outer hook to the
        // parent.
        let source = "function Parent() {\n  useState();\n  return <List render={() => {\n    useMemo();\n    return null;\n  }} />;\n}\n";
        let offsets = compute_line_offsets(source);
        let line_start = |line: u32| offsets[(line - 1) as usize];
        // Byte offset of the `useMemo` token on line 4 (4 leading spaces).
        let use_memo_byte = line_start(4) + 4;
        // The arrow `() => {` starts at the `(` after `render=` on line 3. Its
        // body span must contain the `useMemo` call on line 4 but start AFTER the
        // parent's `useState` on line 2.
        let arrow_col = "  return <List render=".len() as u32;
        let module = module_with(
            source,
            vec![
                fc("Parent", 1, 7, 1),
                // Inner arrow: line 3, col after `render=`, spans lines 3..=6.
                fc_at("<anonymous>", 3, arrow_col, 4, 1),
            ],
            vec![
                // useState on line 2 (col 2), only Parent encloses.
                hook(HookUseKind::UseState, None, line_start(2) + 2),
                // useMemo inside the arrow body on line 4.
                hook(HookUseKind::UseMemo, None, use_memo_byte),
            ],
        );
        let profiles = build_module_hook_profiles(&module);
        let parent = profiles[0].as_ref().expect("parent profile");
        let inner = profiles[1].as_ref().expect("inner profile");
        // useState on line 2 -> only Parent encloses -> Parent.
        assert_eq!(parent.state, 1);
        assert_eq!(parent.memo, 0);
        // useMemo in the arrow body -> both enclose, inner is smaller byte span.
        assert_eq!(inner.memo, 1);
        assert_eq!(inner.state, 0);
    }

    #[test]
    fn effect_call_attributes_to_component_not_callback_arrow() {
        // The common `useEffect(() => {...}, deps)` shape: the `useEffect(` call
        // token sits BEFORE the nested callback arrow's `(` on the same line, so
        // byte-precise containment attributes the hook to the component frame,
        // NOT the callback arrow frame that starts later on the same line. A
        // line-range heuristic would misattribute this (the dominant case).
        let source = "function Comp() {\n  useEffect(() => {\n    doThing();\n  }, [a, b]);\n}\n";
        let offsets = compute_line_offsets(source);
        // `useEffect` token on line 2 at col 2.
        let use_effect_byte = offsets[1] + 2;
        // The callback arrow `() => {` starts at col after `  useEffect(`.
        let arrow_col = "  useEffect(".len() as u32;
        let module = module_with(
            source,
            vec![
                fc("Comp", 1, 5, 1),
                // Callback arrow: line 2, col after `useEffect(`, body lines 2..=4.
                fc_at("<anonymous>", 2, arrow_col, 3, 0),
            ],
            vec![hook(HookUseKind::UseEffect, Some(2), use_effect_byte)],
        );
        let profiles = build_module_hook_profiles(&module);
        let comp = profiles[0].as_ref().expect("component profile");
        // useEffect attributed to the component, not the callback arrow.
        assert_eq!(comp.effect, 1);
        assert_eq!(comp.max_effect_dep_arity, Some(2));
        // The callback arrow got no hook (its body holds doThing(), not the call).
        assert!(profiles[1].is_none());
    }

    #[test]
    fn breakdown_total_equals_react_hook_count_for_a_component() {
        // Invariant for the common case: every hook recorded in `hook_uses`
        // (component scope) is attributed, so the breakdown total equals the
        // frame's `react_hook_count` for an identified React component.
        let source = "function Comp() {\n  useState();\n  useEffect([a]);\n  useRouter();\n  useMemo();\n}\n";
        let offsets = compute_line_offsets(source);
        let line_start = |line: u32| offsets[(line - 1) as usize];
        let react_hook_count = 4;
        let module = module_with(
            source,
            vec![fc("Comp", 1, 6, react_hook_count)],
            vec![
                hook(HookUseKind::UseState, None, line_start(2) + 2),
                hook(HookUseKind::UseEffect, Some(1), line_start(3) + 2),
                hook(HookUseKind::Custom, None, line_start(4) + 2),
                hook(HookUseKind::UseMemo, None, line_start(5) + 2),
            ],
        );
        let profiles = build_module_hook_profiles(&module);
        let profile = profiles[0].as_ref().expect("profile present");
        assert_eq!(profile.total(), react_hook_count);
    }

    #[test]
    fn breakdown_may_sum_to_less_than_react_hook_count_for_helper_hooks() {
        // Documented divergence: complexity.rs counts a `use*` call inside a plain
        // helper into `react_hook_count`, but react.rs does NOT push a `HookUse`
        // for it (no enclosing component). Modeled here as a frame whose
        // `react_hook_count` exceeds the attributed component-scope hooks. The
        // breakdown is an additive refinement, never a replacement, so summing to
        // LESS is tolerated.
        let source = "function useThing() {\n  useState();\n}\n";
        let module = module_with(
            source,
            // The frame claims 1 hook (complexity.rs counted it) but `hook_uses`
            // is empty (react.rs recorded nothing: not a component).
            vec![fc("useThing", 1, 3, 1)],
            Vec::new(),
        );
        let profiles = build_module_hook_profiles(&module);
        // No profile attributed; breakdown total (0) < react_hook_count (1).
        assert!(profiles[0].is_none());
    }

    #[test]
    fn custom_hooks_counted_separately() {
        let source = "function Comp() {\n  useRouter();\n  useTranslation();\n}\n";
        let offsets = compute_line_offsets(source);
        let line_start = |line: u32| offsets[(line - 1) as usize];
        let module = module_with(
            source,
            vec![fc("Comp", 1, 4, 2)],
            vec![
                hook(HookUseKind::Custom, None, line_start(2)),
                hook(HookUseKind::Custom, None, line_start(3)),
            ],
        );
        let profiles = build_module_hook_profiles(&module);
        let profile = profiles[0].as_ref().expect("profile present");
        assert_eq!(profile.custom, 2);
        assert_eq!(profile.total(), 2);
    }
}
