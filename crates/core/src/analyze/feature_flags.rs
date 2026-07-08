//! Feature flag collection and cross-reference with dead code findings.
//!
//! Collects per-file flag uses from parsed modules and builds
//! project-level `FeatureFlag` results. Optionally correlates with
//! dead code findings to identify flags guarding unused code.

use std::path::PathBuf;

use fallow_types::extract::{FlagUse, FlagUseKind, ModuleInfo, byte_offset_to_line_col};
use fallow_types::results::{AnalysisResults, FeatureFlag, FlagConfidence, FlagKind};

use crate::graph::ModuleGraph;

/// Collect feature flag uses from all parsed modules into `FeatureFlag` results.
///
/// Maps extraction-level `FlagUse` (per-file, no path) to result-level
/// `FeatureFlag` (with full path, confidence). Resolves guard span byte
/// offsets to line numbers using per-file line offset tables.
#[deprecated(
    since = "2.76.0",
    note = "fallow_core is internal; use fallow_api::run_feature_flags for typed output; serialize with fallow_api::serialize_feature_flags_programmatic_json for JSON output. See docs/fallow-core-migration.md."
)]
pub fn collect_feature_flags(modules: &[ModuleInfo], graph: &ModuleGraph) -> Vec<FeatureFlag> {
    let mut flags = Vec::new();

    for module in modules {
        if module.flag_uses.is_empty() {
            continue;
        }

        let idx = module.file_id.0 as usize;
        let Some(node) = graph.modules.get(idx) else {
            continue;
        };

        for flag_use in &module.flag_uses {
            let mut flag = flag_use_to_feature_flag(flag_use, node.path.clone());

            if let (Some(start), Some(end)) = (flag_use.guard_span_start, flag_use.guard_span_end)
                && !module.line_offsets.is_empty()
            {
                let (start_line, _) = byte_offset_to_line_col(&module.line_offsets, start);
                let (end_line, _) = byte_offset_to_line_col(&module.line_offsets, end);
                flag.guard_line_start = Some(start_line);
                flag.guard_line_end = Some(end_line);
            }

            flags.push(flag);
        }
    }

    flags
}

/// Correlate feature flags with dead code findings.
///
/// For each flag that guards a code span, check if any dead code findings
/// (unused exports) fall within that span. Populates `guarded_dead_exports`
/// on each flag.
#[deprecated(
    since = "2.76.0",
    note = "fallow_core is internal; use fallow_api::run_feature_flags for typed output; serialize with fallow_api::serialize_feature_flags_programmatic_json for JSON output. The `guarded_dead_exports` field carries the same correlation. See docs/fallow-core-migration.md."
)]
pub fn correlate_with_dead_code(flags: &mut [FeatureFlag], results: &AnalysisResults) {
    if results.unused_exports.is_empty() && results.unused_types.is_empty() {
        return;
    }

    for flag in flags.iter_mut() {
        let (Some(guard_start), Some(guard_end)) = (flag.guard_line_start, flag.guard_line_end)
        else {
            continue;
        };

        for export in &results.unused_exports {
            if export.export.path == flag.path
                && export.export.line >= guard_start
                && export.export.line <= guard_end
            {
                flag.guarded_dead_exports
                    .push(export.export.export_name.clone());
            }
        }

        for export in &results.unused_types {
            if export.export.path == flag.path
                && export.export.line >= guard_start
                && export.export.line <= guard_end
            {
                flag.guarded_dead_exports
                    .push(export.export.export_name.clone());
            }
        }
    }
}

/// Convert an extraction-level `FlagUse` to a result-level `FeatureFlag`.
fn flag_use_to_feature_flag(flag_use: &FlagUse, path: PathBuf) -> FeatureFlag {
    let (kind, confidence) = match flag_use.kind {
        FlagUseKind::EnvVar => (FlagKind::EnvironmentVariable, FlagConfidence::High),
        FlagUseKind::SdkCall => (FlagKind::SdkCall, FlagConfidence::High),
        FlagUseKind::ConfigObject => (FlagKind::ConfigObject, FlagConfidence::Low),
    };

    FeatureFlag {
        path,
        flag_name: flag_use.flag_name.clone(),
        kind,
        confidence,
        line: flag_use.line,
        col: flag_use.col,
        guard_span_start: flag_use.guard_span_start,
        guard_span_end: flag_use.guard_span_end,
        sdk_name: flag_use.sdk_name.clone(),
        guard_line_start: None,
        guard_line_end: None,
        guarded_dead_exports: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use fallow_types::discover::{DiscoveredFile, EntryPoint, FileId};
    use fallow_types::extract::compute_line_offsets;
    use fallow_types::output_dead_code::UnusedExportFinding;
    use fallow_types::results::{AnalysisResults, UnusedExport};

    use crate::graph::ModuleGraph;
    use crate::resolve::ResolvedModule;

    use super::*;

    // ---------------------------------------------------------------------------
    // Helpers
    // ---------------------------------------------------------------------------

    /// Build a minimal [`ModuleGraph`] that has exactly one module at index
    /// `file_id.0` with the given absolute path.  No imports or exports are
    /// wired; we only need `graph.modules[idx].path` to exist.
    fn graph_with_module(file_id: FileId, path: PathBuf) -> ModuleGraph {
        let files = vec![DiscoveredFile {
            id: file_id,
            path: path.clone(),
            size_bytes: 0,
        }];
        let resolved = vec![ResolvedModule {
            file_id,
            path,
            ..Default::default()
        }];
        let entry_points: Vec<EntryPoint> = vec![];
        ModuleGraph::build(&resolved, &entry_points, &files)
    }

    /// Minimal [`ModuleInfo`] with the given `file_id` and `flag_uses`, all
    /// other fields defaulting to empty / false.
    fn module_with_flags(file_id: FileId, flag_uses: Vec<FlagUse>) -> ModuleInfo {
        ModuleInfo {
            file_id,
            flag_uses,
            exports: Vec::new(),
            imports: Vec::new(),
            re_exports: Vec::new(),
            dynamic_imports: Vec::new(),
            dynamic_import_patterns: Vec::new(),
            require_calls: Vec::new(),
            package_path_references: Box::default(),
            member_accesses: Vec::new(),
            semantic_facts: Box::default(),
            whole_object_uses: Box::default(),
            has_cjs_exports: false,
            has_angular_component_template_url: false,
            content_hash: 0,
            suppressions: Vec::new(),
            unknown_suppression_kinds: Vec::new(),
            unused_import_bindings: Vec::new(),
            type_referenced_import_bindings: Vec::new(),
            value_referenced_import_bindings: Vec::new(),
            line_offsets: Vec::new(),
            complexity: Vec::new(),
            class_heritage: Vec::new(),
            exported_factory_returns: Box::default(),
            type_member_types: Box::default(),
            injection_tokens: Vec::new(),
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
            inline_server_action_exports: Vec::new(),
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
            hook_uses: Vec::new(),
            render_edges: Vec::new(),
            svelte_dispatched_events: Vec::new(),
            svelte_listened_events: Vec::new(),
            angular_component_selectors: Vec::new(),
            registered_custom_elements: Vec::new(),
            used_custom_element_tags: Vec::new(),
            angular_used_selectors: Vec::new(),
            angular_entry_component_refs: Vec::new(),
            has_dynamic_component_render: false,
            has_dynamic_dispatch: false,
        }
    }

    fn make_unused_export(path: PathBuf, export_name: &str, line: u32) -> UnusedExportFinding {
        UnusedExportFinding::with_actions(UnusedExport {
            path,
            export_name: export_name.to_string(),
            is_type_only: false,
            line,
            col: 0,
            span_start: 0,
            is_re_export: false,
        })
    }

    // ---------------------------------------------------------------------------
    // collect_feature_flags: positive detection path
    // ---------------------------------------------------------------------------

    /// A module with no flag uses is skipped entirely.
    #[test]
    #[expect(deprecated, reason = "testing the deprecated public API")]
    fn collect_feature_flags_empty_flag_uses_skipped() {
        let graph = graph_with_module(FileId(0), PathBuf::from("/project/src/empty.ts"));
        let module = module_with_flags(FileId(0), vec![]);
        let flags = collect_feature_flags(&[module], &graph);
        assert!(
            flags.is_empty(),
            "module with no flag_uses should produce no flags"
        );
    }

    /// A module whose file_id index falls outside graph.modules is skipped.
    #[test]
    #[expect(deprecated, reason = "testing the deprecated public API")]
    fn collect_feature_flags_missing_graph_node_skipped() {
        // Graph has a module at index 0, but the ModuleInfo claims file_id 5.
        let path = PathBuf::from("/project/src/file.ts");
        let graph = graph_with_module(FileId(0), path);
        let flag_use = FlagUse {
            flag_name: "MY_FLAG".to_string(),
            kind: FlagUseKind::EnvVar,
            line: 1,
            col: 0,
            guard_span_start: None,
            guard_span_end: None,
            sdk_name: None,
        };
        // FileId(5) maps to index 5, but graph only has index 0: should be skipped.
        let module = module_with_flags(FileId(5), vec![flag_use]);
        let flags = collect_feature_flags(&[module], &graph);
        assert!(
            flags.is_empty(),
            "module whose file_id has no graph node should be skipped"
        );
    }

    /// A module with an EnvVar flag use produces one FeatureFlag with High confidence.
    #[test]
    #[expect(deprecated, reason = "testing the deprecated public API")]
    fn collect_feature_flags_produces_flag_from_env_var() {
        let graph = graph_with_module(FileId(0), PathBuf::from("/project/src/config.ts"));
        let flag_use = FlagUse {
            flag_name: "ENABLE_DARK_MODE".to_string(),
            kind: FlagUseKind::EnvVar,
            line: 3,
            col: 8,
            guard_span_start: None,
            guard_span_end: None,
            sdk_name: None,
        };
        let module = module_with_flags(FileId(0), vec![flag_use]);
        let flags = collect_feature_flags(&[module], &graph);
        assert_eq!(flags.len(), 1);
        let flag = &flags[0];
        assert_eq!(flag.flag_name, "ENABLE_DARK_MODE");
        assert_eq!(flag.kind, FlagKind::EnvironmentVariable);
        assert_eq!(flag.confidence, FlagConfidence::High);
        assert_eq!(flag.line, 3);
        assert_eq!(flag.col, 8);
        // Path must come from the graph node, not the ModuleInfo.
        assert_eq!(
            flag.path.to_string_lossy().replace('\\', "/"),
            "/project/src/config.ts"
        );
    }

    /// An SdkCall flag use is mapped correctly including sdk_name.
    #[test]
    #[expect(deprecated, reason = "testing the deprecated public API")]
    fn collect_feature_flags_sdk_call_maps_sdk_name() {
        let graph = graph_with_module(FileId(0), PathBuf::from("/project/src/feature.ts"));
        let flag_use = FlagUse {
            flag_name: "new-onboarding".to_string(),
            kind: FlagUseKind::SdkCall,
            line: 7,
            col: 0,
            guard_span_start: None,
            guard_span_end: None,
            sdk_name: Some("Unleash".to_string()),
        };
        let module = module_with_flags(FileId(0), vec![flag_use]);
        let flags = collect_feature_flags(&[module], &graph);
        assert_eq!(flags.len(), 1);
        assert_eq!(flags[0].kind, FlagKind::SdkCall);
        assert_eq!(flags[0].confidence, FlagConfidence::High);
        assert_eq!(flags[0].sdk_name.as_deref(), Some("Unleash"));
    }

    /// A ConfigObject flag use maps to Low confidence.
    #[test]
    #[expect(deprecated, reason = "testing the deprecated public API")]
    fn collect_feature_flags_config_object_has_low_confidence() {
        let graph = graph_with_module(FileId(0), PathBuf::from("/project/src/flags.ts"));
        let flag_use = FlagUse {
            flag_name: "feature.beta".to_string(),
            kind: FlagUseKind::ConfigObject,
            line: 12,
            col: 4,
            guard_span_start: None,
            guard_span_end: None,
            sdk_name: None,
        };
        let module = module_with_flags(FileId(0), vec![flag_use]);
        let flags = collect_feature_flags(&[module], &graph);
        assert_eq!(flags.len(), 1);
        assert_eq!(flags[0].confidence, FlagConfidence::Low);
    }

    /// Multiple flag uses in one module all produce individual FeatureFlag entries.
    #[test]
    #[expect(deprecated, reason = "testing the deprecated public API")]
    fn collect_feature_flags_multiple_uses_in_one_module() {
        let graph = graph_with_module(FileId(0), PathBuf::from("/project/src/multi.ts"));
        let flag_uses = vec![
            FlagUse {
                flag_name: "FLAG_A".to_string(),
                kind: FlagUseKind::EnvVar,
                line: 1,
                col: 0,
                guard_span_start: None,
                guard_span_end: None,
                sdk_name: None,
            },
            FlagUse {
                flag_name: "FLAG_B".to_string(),
                kind: FlagUseKind::SdkCall,
                line: 2,
                col: 0,
                guard_span_start: None,
                guard_span_end: None,
                sdk_name: None,
            },
        ];
        let module = module_with_flags(FileId(0), flag_uses);
        let flags = collect_feature_flags(&[module], &graph);
        assert_eq!(flags.len(), 2);
        let names: Vec<&str> = flags.iter().map(|f| f.flag_name.as_str()).collect();
        assert!(names.contains(&"FLAG_A"));
        assert!(names.contains(&"FLAG_B"));
    }

    /// Guard span byte offsets are resolved to line numbers when line_offsets
    /// are non-empty.  Source "abc\ndef\n" produces offsets [0, 4, 8].
    /// Byte 1 is on line 1; byte 5 is on line 2.
    #[test]
    #[expect(deprecated, reason = "testing the deprecated public API")]
    fn collect_feature_flags_resolves_guard_span_to_line_numbers() {
        let source = "abc\ndef\n";
        let line_offsets = compute_line_offsets(source);

        let graph = graph_with_module(FileId(0), PathBuf::from("/project/src/guarded.ts"));
        let flag_use = FlagUse {
            flag_name: "GUARDED_FLAG".to_string(),
            kind: FlagUseKind::EnvVar,
            line: 1,
            col: 0,
            guard_span_start: Some(1),
            guard_span_end: Some(5),
            sdk_name: None,
        };
        let mut module = module_with_flags(FileId(0), vec![flag_use]);
        module.line_offsets = line_offsets;

        let flags = collect_feature_flags(&[module], &graph);
        assert_eq!(flags.len(), 1);
        let flag = &flags[0];
        assert!(
            flag.guard_line_start.is_some(),
            "guard_line_start should be resolved from byte offset"
        );
        assert!(
            flag.guard_line_end.is_some(),
            "guard_line_end should be resolved from byte offset"
        );
    }

    /// When guard span is present but line_offsets is empty, guard lines stay None.
    #[test]
    #[expect(deprecated, reason = "testing the deprecated public API")]
    fn collect_feature_flags_no_guard_lines_when_offsets_empty() {
        let graph = graph_with_module(FileId(0), PathBuf::from("/project/src/no_offsets.ts"));
        let flag_use = FlagUse {
            flag_name: "NO_OFFSETS_FLAG".to_string(),
            kind: FlagUseKind::EnvVar,
            line: 1,
            col: 0,
            guard_span_start: Some(10),
            guard_span_end: Some(50),
            sdk_name: None,
        };
        // Leave line_offsets empty (the default in module_with_flags).
        let module = module_with_flags(FileId(0), vec![flag_use]);
        let flags = collect_feature_flags(&[module], &graph);
        assert_eq!(flags.len(), 1);
        assert!(
            flags[0].guard_line_start.is_none(),
            "without line_offsets, guard lines stay None"
        );
        assert!(
            flags[0].guard_line_end.is_none(),
            "without line_offsets, guard lines stay None"
        );
    }

    /// When guard_span_start or guard_span_end is None, guard lines stay None
    /// even when line_offsets are present.
    #[test]
    #[expect(deprecated, reason = "testing the deprecated public API")]
    fn collect_feature_flags_no_guard_lines_when_span_absent() {
        let graph = graph_with_module(FileId(0), PathBuf::from("/project/src/no_span.ts"));
        let flag_use = FlagUse {
            flag_name: "NO_SPAN_FLAG".to_string(),
            kind: FlagUseKind::SdkCall,
            line: 5,
            col: 0,
            guard_span_start: None,
            guard_span_end: None,
            sdk_name: None,
        };
        let mut module = module_with_flags(FileId(0), vec![flag_use]);
        module.line_offsets = compute_line_offsets("some\ncontent\nhere\n");

        let flags = collect_feature_flags(&[module], &graph);
        assert_eq!(flags.len(), 1);
        assert!(flags[0].guard_line_start.is_none());
        assert!(flags[0].guard_line_end.is_none());
    }

    // ---------------------------------------------------------------------------
    // correlate_with_dead_code
    // ---------------------------------------------------------------------------

    /// When both unused_exports and unused_types are empty, the function returns
    /// early without touching any flag.
    #[test]
    #[expect(deprecated, reason = "testing the deprecated public API")]
    fn correlate_with_dead_code_early_return_when_results_empty() {
        let mut flags = vec![FeatureFlag {
            path: PathBuf::from("/project/src/a.ts"),
            flag_name: "EARLY".to_string(),
            kind: FlagKind::EnvironmentVariable,
            confidence: FlagConfidence::High,
            line: 1,
            col: 0,
            guard_span_start: Some(0),
            guard_span_end: Some(100),
            sdk_name: None,
            guard_line_start: Some(1),
            guard_line_end: Some(5),
            guarded_dead_exports: Vec::new(),
        }];
        let results = AnalysisResults::default();
        correlate_with_dead_code(&mut flags, &results);
        assert!(
            flags[0].guarded_dead_exports.is_empty(),
            "no dead exports should be added when results are empty"
        );
    }

    /// A flag with no guard lines (None) is skipped in the correlation loop.
    #[test]
    #[expect(deprecated, reason = "testing the deprecated public API")]
    fn correlate_with_dead_code_flag_without_guard_lines_is_skipped() {
        let path = PathBuf::from("/project/src/b.ts");
        let mut flags = vec![FeatureFlag {
            path: path.clone(),
            flag_name: "NO_GUARD".to_string(),
            kind: FlagKind::EnvironmentVariable,
            confidence: FlagConfidence::High,
            line: 2,
            col: 0,
            guard_span_start: None,
            guard_span_end: None,
            sdk_name: None,
            guard_line_start: None,
            guard_line_end: None,
            guarded_dead_exports: Vec::new(),
        }];
        let mut results = AnalysisResults::default();
        results
            .unused_exports
            .push(make_unused_export(path, "someExport", 5));

        correlate_with_dead_code(&mut flags, &results);
        assert!(
            flags[0].guarded_dead_exports.is_empty(),
            "flag without guard lines should not accumulate dead exports"
        );
    }

    /// An unused export whose path and line fall within the flag guard span is credited.
    #[test]
    #[expect(deprecated, reason = "testing the deprecated public API")]
    fn correlate_with_dead_code_unused_export_within_guard_span_is_credited() {
        let path = PathBuf::from("/project/src/feature.ts");
        let mut flags = vec![FeatureFlag {
            path: path.clone(),
            flag_name: "MY_FEATURE".to_string(),
            kind: FlagKind::EnvironmentVariable,
            confidence: FlagConfidence::High,
            line: 1,
            col: 0,
            guard_span_start: Some(0),
            guard_span_end: Some(200),
            sdk_name: None,
            guard_line_start: Some(10),
            guard_line_end: Some(20),
            guarded_dead_exports: Vec::new(),
        }];
        // Export on line 15 of the same file is within [10, 20].
        let mut results = AnalysisResults::default();
        results
            .unused_exports
            .push(make_unused_export(path, "myExport", 15));

        correlate_with_dead_code(&mut flags, &results);
        assert_eq!(flags[0].guarded_dead_exports, vec!["myExport"]);
    }

    /// An unused export on a different path is not credited.
    #[test]
    #[expect(deprecated, reason = "testing the deprecated public API")]
    fn correlate_with_dead_code_export_on_different_path_not_credited() {
        let other_path = PathBuf::from("/project/src/other.ts");
        let mut flags = vec![FeatureFlag {
            path: PathBuf::from("/project/src/feature.ts"),
            flag_name: "MY_FEATURE".to_string(),
            kind: FlagKind::EnvironmentVariable,
            confidence: FlagConfidence::High,
            line: 1,
            col: 0,
            guard_span_start: Some(0),
            guard_span_end: Some(200),
            sdk_name: None,
            guard_line_start: Some(1),
            guard_line_end: Some(50),
            guarded_dead_exports: Vec::new(),
        }];
        let mut results = AnalysisResults::default();
        results
            .unused_exports
            .push(make_unused_export(other_path, "wrongFile", 10));

        correlate_with_dead_code(&mut flags, &results);
        assert!(
            flags[0].guarded_dead_exports.is_empty(),
            "export from a different path should not be credited"
        );
    }

    /// An unused export outside the line range is not credited.
    #[test]
    #[expect(deprecated, reason = "testing the deprecated public API")]
    fn correlate_with_dead_code_export_outside_line_range_not_credited() {
        let path = PathBuf::from("/project/src/feature.ts");
        let mut flags = vec![FeatureFlag {
            path: path.clone(),
            flag_name: "MY_FEATURE".to_string(),
            kind: FlagKind::EnvironmentVariable,
            confidence: FlagConfidence::High,
            line: 1,
            col: 0,
            guard_span_start: Some(0),
            guard_span_end: Some(200),
            sdk_name: None,
            guard_line_start: Some(10),
            guard_line_end: Some(20),
            guarded_dead_exports: Vec::new(),
        }];
        let mut results = AnalysisResults::default();
        // Line 99 is outside [10, 20].
        results
            .unused_exports
            .push(make_unused_export(path, "outsideExport", 99));

        correlate_with_dead_code(&mut flags, &results);
        assert!(
            flags[0].guarded_dead_exports.is_empty(),
            "export outside guard line range should not be credited"
        );
    }

    /// An unused TYPE within the guard span is credited via the unused_types path.
    #[test]
    #[expect(deprecated, reason = "testing the deprecated public API")]
    fn correlate_with_dead_code_unused_type_within_guard_span_is_credited() {
        use fallow_types::output_dead_code::UnusedTypeFinding;

        let path = PathBuf::from("/project/src/types.ts");
        let mut flags = vec![FeatureFlag {
            path: path.clone(),
            flag_name: "TYPE_FLAG".to_string(),
            kind: FlagKind::SdkCall,
            confidence: FlagConfidence::High,
            line: 1,
            col: 0,
            guard_span_start: Some(0),
            guard_span_end: Some(500),
            sdk_name: None,
            guard_line_start: Some(5),
            guard_line_end: Some(30),
            guarded_dead_exports: Vec::new(),
        }];
        let unused_type = UnusedTypeFinding::with_actions(UnusedExport {
            path,
            export_name: "MyInterface".to_string(),
            is_type_only: true,
            line: 10,
            col: 0,
            span_start: 0,
            is_re_export: false,
        });
        let mut results = AnalysisResults::default();
        results.unused_types.push(unused_type);

        correlate_with_dead_code(&mut flags, &results);
        assert_eq!(flags[0].guarded_dead_exports, vec!["MyInterface"]);
    }

    /// Both unused_exports and unused_types contribute to guarded_dead_exports.
    #[test]
    #[expect(deprecated, reason = "testing the deprecated public API")]
    fn correlate_with_dead_code_combines_exports_and_types() {
        use fallow_types::output_dead_code::UnusedTypeFinding;

        let path = PathBuf::from("/project/src/combined.ts");
        let mut flags = vec![FeatureFlag {
            path: path.clone(),
            flag_name: "COMBO".to_string(),
            kind: FlagKind::EnvironmentVariable,
            confidence: FlagConfidence::High,
            line: 1,
            col: 0,
            guard_span_start: Some(0),
            guard_span_end: Some(1000),
            sdk_name: None,
            guard_line_start: Some(1),
            guard_line_end: Some(100),
            guarded_dead_exports: Vec::new(),
        }];
        let mut results = AnalysisResults::default();
        results
            .unused_exports
            .push(make_unused_export(path.clone(), "valueExport", 10));
        results
            .unused_types
            .push(UnusedTypeFinding::with_actions(UnusedExport {
                path,
                export_name: "TypeExport".to_string(),
                is_type_only: true,
                line: 50,
                col: 0,
                span_start: 0,
                is_re_export: false,
            }));

        correlate_with_dead_code(&mut flags, &results);
        assert!(
            flags[0]
                .guarded_dead_exports
                .contains(&"valueExport".to_string())
        );
        assert!(
            flags[0]
                .guarded_dead_exports
                .contains(&"TypeExport".to_string())
        );
    }

    /// Boundary check: export exactly at guard_line_start is credited (inclusive lower bound).
    #[test]
    #[expect(deprecated, reason = "testing the deprecated public API")]
    fn correlate_with_dead_code_export_at_guard_start_is_credited() {
        let path = PathBuf::from("/project/src/boundary.ts");
        let mut flags = vec![FeatureFlag {
            path: path.clone(),
            flag_name: "BOUNDARY_FLAG".to_string(),
            kind: FlagKind::EnvironmentVariable,
            confidence: FlagConfidence::High,
            line: 1,
            col: 0,
            guard_span_start: Some(0),
            guard_span_end: Some(200),
            sdk_name: None,
            guard_line_start: Some(10),
            guard_line_end: Some(20),
            guarded_dead_exports: Vec::new(),
        }];
        let mut results = AnalysisResults::default();
        results
            .unused_exports
            .push(make_unused_export(path, "atStart", 10));

        correlate_with_dead_code(&mut flags, &results);
        assert!(
            flags[0]
                .guarded_dead_exports
                .contains(&"atStart".to_string()),
            "export exactly at guard_line_start should be credited (inclusive lower bound)"
        );
    }

    /// Boundary check: export exactly at guard_line_end is credited (inclusive upper bound).
    #[test]
    #[expect(deprecated, reason = "testing the deprecated public API")]
    fn correlate_with_dead_code_export_at_guard_end_is_credited() {
        let path = PathBuf::from("/project/src/boundary.ts");
        let mut flags = vec![FeatureFlag {
            path: path.clone(),
            flag_name: "BOUNDARY_FLAG".to_string(),
            kind: FlagKind::EnvironmentVariable,
            confidence: FlagConfidence::High,
            line: 1,
            col: 0,
            guard_span_start: Some(0),
            guard_span_end: Some(200),
            sdk_name: None,
            guard_line_start: Some(10),
            guard_line_end: Some(20),
            guarded_dead_exports: Vec::new(),
        }];
        let mut results = AnalysisResults::default();
        results
            .unused_exports
            .push(make_unused_export(path, "atEnd", 20));

        correlate_with_dead_code(&mut flags, &results);
        assert!(
            flags[0].guarded_dead_exports.contains(&"atEnd".to_string()),
            "export exactly at guard_line_end should be credited (inclusive upper bound)"
        );
    }

    /// Multiple flags each only pick up exports in their own guard span.
    #[test]
    #[expect(deprecated, reason = "testing the deprecated public API")]
    fn correlate_with_dead_code_multiple_flags_independent() {
        let path = PathBuf::from("/project/src/multi_flag.ts");
        let mut flags = vec![
            FeatureFlag {
                path: path.clone(),
                flag_name: "FLAG_1".to_string(),
                kind: FlagKind::EnvironmentVariable,
                confidence: FlagConfidence::High,
                line: 1,
                col: 0,
                guard_span_start: Some(0),
                guard_span_end: Some(200),
                sdk_name: None,
                guard_line_start: Some(1),
                guard_line_end: Some(10),
                guarded_dead_exports: Vec::new(),
            },
            FeatureFlag {
                path: path.clone(),
                flag_name: "FLAG_2".to_string(),
                kind: FlagKind::EnvironmentVariable,
                confidence: FlagConfidence::High,
                line: 15,
                col: 0,
                guard_span_start: Some(200),
                guard_span_end: Some(400),
                sdk_name: None,
                guard_line_start: Some(20),
                guard_line_end: Some(30),
                guarded_dead_exports: Vec::new(),
            },
        ];
        let mut results = AnalysisResults::default();
        // Export at line 5 belongs to FLAG_1 guard [1..10].
        results
            .unused_exports
            .push(make_unused_export(path.clone(), "exportForFlag1", 5));
        // Export at line 25 belongs to FLAG_2 guard [20..30].
        results
            .unused_exports
            .push(make_unused_export(path, "exportForFlag2", 25));

        correlate_with_dead_code(&mut flags, &results);
        assert_eq!(flags[0].guarded_dead_exports, vec!["exportForFlag1"]);
        assert_eq!(flags[1].guarded_dead_exports, vec!["exportForFlag2"]);
    }

    // ---------------------------------------------------------------------------
    // Original private-fn tests (preserved from before this batch)
    // ---------------------------------------------------------------------------

    #[test]
    fn flag_use_to_feature_flag_env_var() {
        let flag_use = FlagUse {
            flag_name: "FEATURE_X".to_string(),
            kind: FlagUseKind::EnvVar,
            line: 10,
            col: 4,
            guard_span_start: Some(100),
            guard_span_end: Some(200),
            sdk_name: None,
        };

        let result = flag_use_to_feature_flag(&flag_use, PathBuf::from("src/config.ts"));
        assert_eq!(result.flag_name, "FEATURE_X");
        assert_eq!(result.kind, FlagKind::EnvironmentVariable);
        assert_eq!(result.confidence, FlagConfidence::High);
        assert_eq!(result.line, 10);
        assert!(result.guard_span_start.is_some());
    }

    #[test]
    fn flag_use_to_feature_flag_sdk_call() {
        let flag_use = FlagUse {
            flag_name: "new-checkout".to_string(),
            kind: FlagUseKind::SdkCall,
            line: 5,
            col: 0,
            guard_span_start: None,
            guard_span_end: None,
            sdk_name: Some("LaunchDarkly".to_string()),
        };

        let result = flag_use_to_feature_flag(&flag_use, PathBuf::from("src/hooks.ts"));
        assert_eq!(result.kind, FlagKind::SdkCall);
        assert_eq!(result.confidence, FlagConfidence::High);
        assert_eq!(result.sdk_name.as_deref(), Some("LaunchDarkly"));
    }

    #[test]
    fn flag_use_to_feature_flag_config_object() {
        let flag_use = FlagUse {
            flag_name: "features.newCheckout".to_string(),
            kind: FlagUseKind::ConfigObject,
            line: 42,
            col: 8,
            guard_span_start: None,
            guard_span_end: None,
            sdk_name: None,
        };

        let result = flag_use_to_feature_flag(&flag_use, PathBuf::from("src/app.ts"));
        assert_eq!(result.kind, FlagKind::ConfigObject);
        assert_eq!(result.confidence, FlagConfidence::Low);
    }
}
