use super::super::HealthSort;
use super::super::component_rollup::append_component_rollup_findings;
use super::super::filters::{
    filter_complexity_findings_by_diff, filter_hotspots_by_diff, filter_large_functions_by_diff,
};
use super::super::findings::{
    CollectFindingsInput, CrapFindingMergeInput, collect_findings, collect_findings_with_resolver,
    merge_crap_findings,
};
use super::super::ignore::build_ignore_set;
use super::super::large_functions::{LargeFunctionInput, collect_large_functions};
use super::super::runtime_filter::{RuntimeCoverageFilterContext, apply_runtime_coverage_filters};
use super::super::scoring;
use super::super::sort_findings;
use super::super::threshold_overrides::{
    GlobalHealthThresholds, ThresholdOverrideResolver, ThresholdOverrideStateTracker,
};
use crate::baseline::HealthBaselineData;
use crate::source::ModuleInfo;
use fallow_output::{ComplexityViolation, ExceededThreshold, FindingSeverity};
use fallow_types::discover::FileId;
use fallow_types::extract::FunctionComplexity;
use rustc_hash::{FxHashMap, FxHashSet};
use std::path::{Path, PathBuf};

/// Build a minimal `ModuleInfo` with only the fields `collect_findings` needs.
fn make_module(file_id: FileId, complexity: Vec<FunctionComplexity>) -> ModuleInfo {
    ModuleInfo {
        file_id,
        exports: vec![],
        imports: vec![],
        re_exports: vec![],
        dynamic_imports: vec![],
        dynamic_import_patterns: vec![],
        require_calls: vec![],
        package_path_references: Box::default(),
        member_accesses: vec![],
        semantic_facts: Box::default(),
        whole_object_uses: Box::default(),
        has_cjs_exports: false,
        has_angular_component_template_url: false,
        content_hash: 0,
        suppressions: vec![],
        unknown_suppression_kinds: vec![],
        unused_import_bindings: vec![],
        type_referenced_import_bindings: vec![],
        value_referenced_import_bindings: vec![],
        line_offsets: vec![0],
        complexity,
        flag_uses: vec![],
        class_heritage: vec![],
        exported_factory_returns: Box::default(),
        type_member_types: Box::default(),
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

fn make_fc(name: &str, cyclomatic: u16, cognitive: u16, line_count: u32) -> FunctionComplexity {
    FunctionComplexity {
        name: name.to_string(),
        line: 1,
        col: 0,
        cyclomatic,
        cognitive,
        line_count,
        param_count: 0,
        react_hook_count: 0,
        react_jsx_max_depth: 0,
        react_prop_count: 0,
        source_hash: None,
        contributions: Vec::new(),
    }
}

fn make_fc_with_contributions(name: &str, cyclomatic: u16, cognitive: u16) -> FunctionComplexity {
    use fallow_types::extract::{
        ComplexityContribution, ComplexityContributionKind, ComplexityMetric,
    };
    let mut fc = make_fc(name, cyclomatic, cognitive, 50);
    fc.contributions = vec![ComplexityContribution {
        line: 2,
        col: 4,
        metric: ComplexityMetric::Cyclomatic,
        kind: ComplexityContributionKind::If,
        weight: 1,
        nesting: 0,
    }];
    fc
}

#[test]
fn collect_findings_omits_contributions_without_breakdown_flag() {
    let path = PathBuf::from("/project/src/a.ts");
    let modules = vec![make_module(
        FileId(0),
        vec![make_fc_with_contributions("complexFn", 25, 5)],
    )];
    let mut file_paths = FxHashMap::default();
    file_paths.insert(FileId(0), &path);
    let (findings, _, _) = collect_findings(
        &modules,
        &file_paths,
        Path::new("/project"),
        &globset::GlobSet::empty(),
        None,
        None,
        20,
        15,
        false,
    );
    assert_eq!(findings.len(), 1);
    assert!(
        findings[0].contributions.is_empty(),
        "contributions must be omitted without the breakdown flag"
    );
}

#[test]
fn collect_findings_includes_contributions_with_breakdown_flag() {
    let path = PathBuf::from("/project/src/a.ts");
    let modules = vec![make_module(
        FileId(0),
        vec![make_fc_with_contributions("complexFn", 25, 5)],
    )];
    let mut file_paths = FxHashMap::default();
    file_paths.insert(FileId(0), &path);
    let (findings, _, _) = collect_findings(
        &modules,
        &file_paths,
        Path::new("/project"),
        &globset::GlobSet::empty(),
        None,
        None,
        20,
        15,
        true,
    );
    assert_eq!(findings.len(), 1);
    assert_eq!(
        findings[0].contributions.len(),
        1,
        "contributions must flow through when the breakdown flag is set"
    );
}

fn threshold_resolver(
    overrides: &[fallow_config::HealthThresholdOverride],
) -> ThresholdOverrideResolver {
    ThresholdOverrideResolver::new(
        overrides,
        GlobalHealthThresholds {
            cyclomatic: 20,
            cognitive: 15,
            crap: 30.0,
            unit_size: 60,
        },
    )
}

#[test]
fn collect_findings_uses_threshold_override_as_local_ceiling() {
    let path = PathBuf::from("/project/src/a.ts");
    let modules = vec![make_module(
        FileId(0),
        vec![make_fc("complexFn", 25, 20, 50)],
    )];
    let mut file_paths = FxHashMap::default();
    file_paths.insert(FileId(0), &path);
    let resolver = threshold_resolver(&[fallow_config::HealthThresholdOverride {
        files: vec!["src/a.ts".to_string()],
        functions: vec!["complexFn".to_string()],
        max_cyclomatic: Some(30),
        max_cognitive: Some(25),
        max_crap: None,
        max_unit_size: None,
        reason: Some("approved assembly".to_string()),
    }]);
    let mut tracker = ThresholdOverrideStateTracker::default();

    let mut input = CollectFindingsInput {
        modules: &modules,
        file_paths: &file_paths,
        config_root: Path::new("/project"),
        ignore_set: &globset::GlobSet::empty(),
        changed_files: None,
        ws_roots: None,
        threshold_resolver: &resolver,
        threshold_state_tracker: &mut tracker,
        complexity_breakdown: false,
    };
    let (findings, _, _) = collect_findings_with_resolver(&mut input);

    assert!(findings.is_empty());
    let states = tracker.into_states();
    assert_eq!(states.len(), 1);
    assert!(matches!(
        states[0].status,
        fallow_output::ThresholdOverrideStatus::Active
    ));
}

#[test]
fn collect_findings_reports_when_local_ceiling_is_exceeded() {
    let path = PathBuf::from("/project/src/a.ts");
    let modules = vec![make_module(
        FileId(0),
        vec![make_fc("complexFn", 31, 20, 50)],
    )];
    let mut file_paths = FxHashMap::default();
    file_paths.insert(FileId(0), &path);
    let resolver = threshold_resolver(&[fallow_config::HealthThresholdOverride {
        files: vec!["src/a.ts".to_string()],
        functions: vec!["complexFn".to_string()],
        max_cyclomatic: Some(30),
        max_cognitive: Some(25),
        max_crap: None,
        max_unit_size: None,
        reason: None,
    }]);
    let mut tracker = ThresholdOverrideStateTracker::default();

    let mut input = CollectFindingsInput {
        modules: &modules,
        file_paths: &file_paths,
        config_root: Path::new("/project"),
        ignore_set: &globset::GlobSet::empty(),
        changed_files: None,
        ws_roots: None,
        threshold_resolver: &resolver,
        threshold_state_tracker: &mut tracker,
        complexity_breakdown: false,
    };
    let (findings, _, _) = collect_findings_with_resolver(&mut input);

    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].effective_thresholds.unwrap().max_cyclomatic, 30);
    assert!(matches!(
        findings[0].threshold_source,
        Some(fallow_output::ThresholdSource::Override)
    ));
}

#[test]
fn collect_findings_reports_stale_override_when_under_global_thresholds() {
    let path = PathBuf::from("/project/src/a.ts");
    let modules = vec![make_module(
        FileId(0),
        vec![make_fc("complexFn", 10, 8, 20)],
    )];
    let mut file_paths = FxHashMap::default();
    file_paths.insert(FileId(0), &path);
    let resolver = threshold_resolver(&[fallow_config::HealthThresholdOverride {
        files: vec!["src/a.ts".to_string()],
        functions: vec!["complexFn".to_string()],
        max_cyclomatic: Some(30),
        max_cognitive: None,
        max_crap: None,
        max_unit_size: None,
        reason: None,
    }]);
    let mut tracker = ThresholdOverrideStateTracker::default();

    let mut input = CollectFindingsInput {
        modules: &modules,
        file_paths: &file_paths,
        config_root: Path::new("/project"),
        ignore_set: &globset::GlobSet::empty(),
        changed_files: None,
        ws_roots: None,
        threshold_resolver: &resolver,
        threshold_state_tracker: &mut tracker,
        complexity_breakdown: false,
    };
    let (findings, _, _) = collect_findings_with_resolver(&mut input);

    assert!(findings.is_empty());
    let states = tracker.into_states();
    assert_eq!(states.len(), 1);
    assert!(matches!(
        states[0].status,
        fallow_output::ThresholdOverrideStatus::Stale
    ));
}

#[test]
fn threshold_override_tracker_reports_no_match_only_when_requested() {
    let resolver = threshold_resolver(&[fallow_config::HealthThresholdOverride {
        files: vec!["src/missing.ts".to_string()],
        functions: vec!["missingFn".to_string()],
        max_cyclomatic: Some(30),
        max_cognitive: None,
        max_crap: None,
        max_unit_size: None,
        reason: None,
    }]);
    let mut tracker = ThresholdOverrideStateTracker::default();
    tracker.record_no_match_entries(&resolver, false);
    assert!(tracker.into_states().is_empty());

    let mut tracker = ThresholdOverrideStateTracker::default();
    tracker.record_no_match_entries(&resolver, true);
    let states = tracker.into_states();
    assert_eq!(states.len(), 1);
    assert!(matches!(
        states[0].status,
        fallow_output::ThresholdOverrideStatus::NoMatch
    ));
}

#[test]
fn build_ignore_set_empty_patterns() {
    let set = build_ignore_set(&[]);
    assert!(set.is_empty());
}

#[test]
fn build_ignore_set_matches_glob() {
    let patterns = vec!["src/generated/**".to_string()];
    let set = build_ignore_set(&patterns);
    assert!(set.is_match(Path::new("src/generated/types.ts")));
    assert!(!set.is_match(Path::new("src/utils.ts")));
}

#[test]
fn build_ignore_set_multiple_patterns() {
    let patterns = vec!["*.test.ts".to_string(), "dist/**".to_string()];
    let set = build_ignore_set(&patterns);
    assert!(set.is_match(Path::new("foo.test.ts")));
    assert!(set.is_match(Path::new("dist/index.js")));
    assert!(!set.is_match(Path::new("src/index.ts")));
}

#[test]
#[should_panic(expected = "validated at config load time")]
fn build_ignore_set_panics_on_unvalidated_invalid_pattern() {
    let patterns = vec!["[invalid".to_string(), "*.js".to_string()];
    let _ = build_ignore_set(&patterns);
}

fn make_finding(name: &str, exceeded: ExceededThreshold) -> ComplexityViolation {
    ComplexityViolation {
        path: PathBuf::from("/project/src/a.ts"),
        name: name.to_string(),
        line: 1,
        col: 0,
        cyclomatic: match exceeded {
            ExceededThreshold::Cyclomatic
            | ExceededThreshold::Both
            | ExceededThreshold::CyclomaticCrap
            | ExceededThreshold::All => 25,
            _ => 8,
        },
        cognitive: match exceeded {
            ExceededThreshold::Cognitive
            | ExceededThreshold::Both
            | ExceededThreshold::CognitiveCrap
            | ExceededThreshold::All => 20,
            _ => 5,
        },
        line_count: 10,
        param_count: 0,
        react_hook_count: 0,
        react_jsx_max_depth: 0,
        react_prop_count: 0,
        react_hook_profile: None,
        exceeded,
        severity: FindingSeverity::Moderate,
        crap: exceeded.includes_crap().then_some(30.0),
        coverage_pct: None,
        coverage_tier: None,
        coverage_source: None,
        inherited_from: None,
        component_rollup: None,
        contributions: Vec::new(),
        effective_thresholds: None,
        threshold_source: None,
    }
}

#[test]
fn sort_findings_by_severity_surfaces_crap_before_single_metric_findings() {
    let mut findings = vec![
        make_finding("cyclomatic", ExceededThreshold::Cyclomatic),
        make_finding("cognitive", ExceededThreshold::Cognitive),
        make_finding("both", ExceededThreshold::Both),
        make_finding("crap", ExceededThreshold::Crap),
        make_finding("cyclomatic_crap", ExceededThreshold::CyclomaticCrap),
        make_finding("all", ExceededThreshold::All),
    ];

    sort_findings(&mut findings, HealthSort::Severity);

    let names = findings
        .iter()
        .map(|finding| finding.name.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        names,
        [
            "all",
            "cyclomatic_crap",
            "crap",
            "both",
            "cyclomatic",
            "cognitive",
        ]
    );
}

#[test]
fn collect_findings_empty_modules() {
    let (findings, files, functions) = collect_findings(
        &[],
        &FxHashMap::default(),
        Path::new("/project"),
        &globset::GlobSet::empty(),
        None,
        None,
        20,
        15,
        false,
    );
    assert!(findings.is_empty());
    assert_eq!(files, 0);
    assert_eq!(functions, 0);
}

#[test]
fn collect_findings_below_threshold() {
    let path = PathBuf::from("/project/src/a.ts");
    let modules = vec![make_module(FileId(0), vec![make_fc("doStuff", 5, 3, 10)])];
    let mut file_paths = FxHashMap::default();
    file_paths.insert(FileId(0), &path);

    let (findings, files, functions) = collect_findings(
        &modules,
        &file_paths,
        Path::new("/project"),
        &globset::GlobSet::empty(),
        None,
        None,
        20,
        15,
        false,
    );
    assert!(findings.is_empty());
    assert_eq!(files, 1);
    assert_eq!(functions, 1);
}

#[test]
fn collect_findings_exceeds_cyclomatic_only() {
    let path = PathBuf::from("/project/src/a.ts");
    let modules = vec![make_module(
        FileId(0),
        vec![make_fc("complexFn", 25, 5, 50)],
    )];
    let mut file_paths = FxHashMap::default();
    file_paths.insert(FileId(0), &path);

    let (findings, _, _) = collect_findings(
        &modules,
        &file_paths,
        Path::new("/project"),
        &globset::GlobSet::empty(),
        None,
        None,
        20,
        15,
        false,
    );
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].cyclomatic, 25);
    assert!(matches!(
        findings[0].exceeded,
        ExceededThreshold::Cyclomatic
    ));
}

#[test]
fn collect_findings_exceeds_cognitive_only() {
    let path = PathBuf::from("/project/src/a.ts");
    let modules = vec![make_module(FileId(0), vec![make_fc("nestedFn", 5, 20, 30)])];
    let mut file_paths = FxHashMap::default();
    file_paths.insert(FileId(0), &path);

    let (findings, _, _) = collect_findings(
        &modules,
        &file_paths,
        Path::new("/project"),
        &globset::GlobSet::empty(),
        None,
        None,
        20,
        15,
        false,
    );
    assert_eq!(findings.len(), 1);
    assert!(matches!(findings[0].exceeded, ExceededThreshold::Cognitive));
}

#[test]
fn collect_findings_exceeds_both() {
    let path = PathBuf::from("/project/src/a.ts");
    let modules = vec![make_module(
        FileId(0),
        vec![make_fc("terribleFn", 25, 20, 100)],
    )];
    let mut file_paths = FxHashMap::default();
    file_paths.insert(FileId(0), &path);

    let (findings, _, _) = collect_findings(
        &modules,
        &file_paths,
        Path::new("/project"),
        &globset::GlobSet::empty(),
        None,
        None,
        20,
        15,
        false,
    );
    assert_eq!(findings.len(), 1);
    assert!(matches!(findings[0].exceeded, ExceededThreshold::Both));
}

#[test]
fn collect_findings_multiple_functions_per_file() {
    let path = PathBuf::from("/project/src/a.ts");
    let modules = vec![make_module(
        FileId(0),
        vec![
            make_fc("ok", 5, 3, 10),
            make_fc("bad", 25, 20, 50),
            make_fc("also_bad", 21, 5, 30),
        ],
    )];
    let mut file_paths = FxHashMap::default();
    file_paths.insert(FileId(0), &path);

    let (findings, files, functions) = collect_findings(
        &modules,
        &file_paths,
        Path::new("/project"),
        &globset::GlobSet::empty(),
        None,
        None,
        20,
        15,
        false,
    );
    assert_eq!(findings.len(), 2);
    assert_eq!(files, 1);
    assert_eq!(functions, 3);
}

#[test]
fn collect_findings_ignores_matching_files() {
    let path = PathBuf::from("/project/src/generated/types.ts");
    let modules = vec![make_module(FileId(0), vec![make_fc("genFn", 25, 20, 50)])];
    let mut file_paths = FxHashMap::default();
    file_paths.insert(FileId(0), &path);

    let ignore_set = build_ignore_set(&["src/generated/**".to_string()]);
    let (findings, files, _) = collect_findings(
        &modules,
        &file_paths,
        Path::new("/project"),
        &ignore_set,
        None,
        None,
        20,
        15,
        false,
    );
    assert!(findings.is_empty());
    assert_eq!(files, 0);
}

#[test]
fn collect_findings_filters_by_changed_files() {
    let path_a = PathBuf::from("/project/src/a.ts");
    let path_b = PathBuf::from("/project/src/b.ts");
    let modules = vec![
        make_module(FileId(0), vec![make_fc("fnA", 25, 20, 50)]),
        make_module(FileId(1), vec![make_fc("fnB", 25, 20, 50)]),
    ];
    let mut file_paths = FxHashMap::default();
    file_paths.insert(FileId(0), &path_a);
    file_paths.insert(FileId(1), &path_b);

    let mut changed = FxHashSet::default();
    changed.insert(PathBuf::from("/project/src/a.ts"));

    let (findings, files, _) = collect_findings(
        &modules,
        &file_paths,
        Path::new("/project"),
        &globset::GlobSet::empty(),
        Some(&changed),
        None,
        20,
        15,
        false,
    );
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].name, "fnA");
    assert_eq!(files, 1);
}

fn build_diff(text: &str) -> fallow_output::DiffIndex {
    fallow_output::DiffIndex::from_unified_diff(text)
}

#[test]
fn filter_complexity_findings_by_diff_keeps_hotspot_overlapping_diff_line() {
    let mut findings = vec![ComplexityViolation {
        path: PathBuf::from("/project/src/big.ts"),
        name: "wide_fn".into(),
        line: 10,
        col: 0,
        cyclomatic: 30,
        cognitive: 30,
        line_count: 110,
        param_count: 0,
        react_hook_count: 0,
        react_jsx_max_depth: 0,
        react_prop_count: 0,
        react_hook_profile: None,
        exceeded: ExceededThreshold::Both,
        severity: FindingSeverity::High,
        crap: None,
        coverage_pct: None,
        coverage_tier: None,
        coverage_source: None,
        inherited_from: None,
        component_rollup: None,
        contributions: Vec::new(),
        effective_thresholds: None,
        threshold_source: None,
    }];
    let diff = build_diff(
        "diff --git a/src/big.ts b/src/big.ts\n\
             --- a/src/big.ts\n\
             +++ b/src/big.ts\n\
             @@ -114,1 +114,2 @@\n\
              ctx\n\
             +touched\n",
    );
    filter_complexity_findings_by_diff(&mut findings, &diff, Path::new("/project"));
    assert_eq!(findings.len(), 1);
}

#[test]
fn filter_complexity_findings_by_diff_drops_finding_outside_diff() {
    let mut findings = vec![ComplexityViolation {
        path: PathBuf::from("/project/src/elsewhere.ts"),
        name: "outside".into(),
        line: 10,
        col: 0,
        cyclomatic: 30,
        cognitive: 30,
        line_count: 5,
        param_count: 0,
        react_hook_count: 0,
        react_jsx_max_depth: 0,
        react_prop_count: 0,
        react_hook_profile: None,
        exceeded: ExceededThreshold::Both,
        severity: FindingSeverity::High,
        crap: None,
        coverage_pct: None,
        coverage_tier: None,
        coverage_source: None,
        inherited_from: None,
        component_rollup: None,
        contributions: Vec::new(),
        effective_thresholds: None,
        threshold_source: None,
    }];
    let diff = build_diff(
        "diff --git a/src/big.ts b/src/big.ts\n\
             --- a/src/big.ts\n\
             +++ b/src/big.ts\n\
             @@ -114,1 +114,2 @@\n\
              ctx\n\
             +touched\n",
    );
    filter_complexity_findings_by_diff(&mut findings, &diff, Path::new("/project"));
    assert!(findings.is_empty());
}

#[test]
fn filter_complexity_findings_by_diff_handles_zero_line_count() {
    let mut findings = vec![ComplexityViolation {
        path: PathBuf::from("/project/src/a.ts"),
        name: "zero_extent".into(),
        line: 5,
        col: 0,
        cyclomatic: 30,
        cognitive: 30,
        line_count: 0,
        param_count: 0,
        react_hook_count: 0,
        react_jsx_max_depth: 0,
        react_prop_count: 0,
        react_hook_profile: None,
        exceeded: ExceededThreshold::Both,
        severity: FindingSeverity::High,
        crap: None,
        coverage_pct: None,
        coverage_tier: None,
        coverage_source: None,
        inherited_from: None,
        component_rollup: None,
        contributions: Vec::new(),
        effective_thresholds: None,
        threshold_source: None,
    }];
    let diff = build_diff(
        "diff --git a/src/a.ts b/src/a.ts\n\
             --- a/src/a.ts\n\
             +++ b/src/a.ts\n\
             @@ -4,1 +4,2 @@\n\
              ctx\n\
             +touched\n",
    );
    filter_complexity_findings_by_diff(&mut findings, &diff, Path::new("/project"));
    assert_eq!(findings.len(), 1);
}

#[test]
fn filter_hotspots_by_diff_uses_file_level_membership() {
    use fallow_output::HotspotEntry;
    let mut hotspots = vec![
        HotspotEntry {
            path: PathBuf::from("/project/src/touched.ts"),
            score: 90.0,
            commits: 50,
            weighted_commits: 25.0,
            lines_added: 1000,
            lines_deleted: 500,
            complexity_density: 0.4,
            fan_in: 5,
            trend: crate::churn::ChurnTrend::Stable,
            ownership: None,
            is_test_path: false,
        },
        HotspotEntry {
            path: PathBuf::from("/project/src/untouched.ts"),
            score: 90.0,
            commits: 50,
            weighted_commits: 25.0,
            lines_added: 1000,
            lines_deleted: 500,
            complexity_density: 0.4,
            fan_in: 5,
            trend: crate::churn::ChurnTrend::Stable,
            ownership: None,
            is_test_path: false,
        },
    ];
    let diff = build_diff(
        "diff --git a/src/touched.ts b/src/touched.ts\n\
             --- a/src/touched.ts\n\
             +++ b/src/touched.ts\n\
             @@ -0,0 +1,1 @@\n\
             +new\n",
    );
    filter_hotspots_by_diff(&mut hotspots, &diff, Path::new("/project"));
    assert_eq!(hotspots.len(), 1);
    assert_eq!(hotspots[0].path, PathBuf::from("/project/src/touched.ts"));
}

#[test]
fn filter_large_functions_by_diff_uses_range_overlap() {
    use fallow_output::LargeFunctionEntry;
    let mut entries = vec![
        LargeFunctionEntry {
            path: PathBuf::from("/project/src/a.ts"),
            name: "kept".into(),
            line: 10,
            line_count: 100,
        },
        LargeFunctionEntry {
            path: PathBuf::from("/project/src/a.ts"),
            name: "dropped".into(),
            line: 500,
            line_count: 100,
        },
    ];
    let diff = build_diff(
        "diff --git a/src/a.ts b/src/a.ts\n\
             --- a/src/a.ts\n\
             +++ b/src/a.ts\n\
             @@ -49,1 +49,2 @@\n\
              ctx\n\
             +touched\n",
    );
    filter_large_functions_by_diff(&mut entries, &diff, Path::new("/project"));
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].name, "kept");
}

fn dominant_unit_size_vital_signs() -> fallow_output::VitalSigns {
    fallow_output::VitalSigns {
        unit_size_profile: Some(fallow_output::RiskProfile {
            low_risk: 0.0,
            medium_risk: 0.0,
            high_risk: 0.0,
            very_high_risk: 100.0,
        }),
        ..Default::default()
    }
}

#[test]
fn collect_large_functions_respects_max_unit_size_override() {
    // Two identical 218-line functions: one in a test file covered by a
    // `maxUnitSize: 500` override, one in a plain src file on the default 60.
    let test_path = PathBuf::from("/project/src/math.test.ts");
    let src_path = PathBuf::from("/project/src/math.ts");
    let modules = vec![
        make_module(FileId(0), vec![make_fc("<arrow>", 1, 1, 218)]),
        make_module(FileId(1), vec![make_fc("bigHelper", 1, 1, 218)]),
    ];
    let mut file_paths = FxHashMap::default();
    file_paths.insert(FileId(0), &test_path);
    file_paths.insert(FileId(1), &src_path);

    let resolver = threshold_resolver(&[fallow_config::HealthThresholdOverride {
        files: vec!["**/*.test.*".to_string()],
        functions: Vec::new(),
        max_cyclomatic: None,
        max_cognitive: None,
        max_crap: None,
        max_unit_size: Some(500),
        reason: None,
    }]);
    let vital_signs = dominant_unit_size_vital_signs();
    let input = LargeFunctionInput {
        vital_signs: &vital_signs,
        modules: &modules,
        file_paths: &file_paths,
        config_root: Path::new("/project"),
        ignore_set: &globset::GlobSet::empty(),
        changed_files: None,
        ws_roots: None,
        thresholds: &resolver,
    };

    let entries = collect_large_functions(&input);
    // The overridden test-file function drops out (218 <= 500); the src-file
    // function stays listed (218 > 60). Neuter check: reverting the effective
    // gate to `> 60` re-lists the test-file `<arrow>` and fails this assertion.
    let listed: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(listed, vec!["bigHelper"]);
    assert!(entries.iter().all(|entry| entry.path == src_path));
}

#[test]
fn collect_large_functions_default_lists_every_oversized_function() {
    // Control: with no override, the default 60-LOC ceiling lists both.
    let test_path = PathBuf::from("/project/src/math.test.ts");
    let src_path = PathBuf::from("/project/src/math.ts");
    let modules = vec![
        make_module(FileId(0), vec![make_fc("<arrow>", 1, 1, 218)]),
        make_module(FileId(1), vec![make_fc("bigHelper", 1, 1, 218)]),
    ];
    let mut file_paths = FxHashMap::default();
    file_paths.insert(FileId(0), &test_path);
    file_paths.insert(FileId(1), &src_path);

    let resolver = threshold_resolver(&[]);
    let vital_signs = dominant_unit_size_vital_signs();
    let input = LargeFunctionInput {
        vital_signs: &vital_signs,
        modules: &modules,
        file_paths: &file_paths,
        config_root: Path::new("/project"),
        ignore_set: &globset::GlobSet::empty(),
        changed_files: None,
        ws_roots: None,
        thresholds: &resolver,
    };

    let entries = collect_large_functions(&input);
    assert_eq!(entries.len(), 2);
}

#[test]
fn collect_findings_skips_module_without_path() {
    let modules = vec![make_module(FileId(99), vec![make_fc("orphan", 25, 20, 50)])];
    let file_paths = FxHashMap::default();

    let (findings, files, _) = collect_findings(
        &modules,
        &file_paths,
        Path::new("/project"),
        &globset::GlobSet::empty(),
        None,
        None,
        20,
        15,
        false,
    );
    assert!(findings.is_empty());
    assert_eq!(files, 0);
}

#[test]
fn collect_findings_at_exact_threshold_not_reported() {
    let path = PathBuf::from("/project/src/a.ts");
    let modules = vec![make_module(
        FileId(0),
        vec![make_fc("borderline", 20, 15, 20)],
    )];
    let mut file_paths = FxHashMap::default();
    file_paths.insert(FileId(0), &path);

    let (findings, _, _) = collect_findings(
        &modules,
        &file_paths,
        Path::new("/project"),
        &globset::GlobSet::empty(),
        None,
        None,
        20,
        15,
        false,
    );
    assert!(findings.is_empty());
}

#[test]
fn collect_findings_preserves_function_metadata() {
    let path = PathBuf::from("/project/src/a.ts");
    let modules = vec![make_module(
        FileId(0),
        vec![FunctionComplexity {
            name: "processData".to_string(),
            line: 42,
            col: 8,
            cyclomatic: 25,
            cognitive: 18,
            line_count: 75,
            param_count: 2,
            react_hook_count: 0,
            react_jsx_max_depth: 0,
            react_prop_count: 0,
            source_hash: None,
            contributions: Vec::new(),
        }],
    )];
    let mut file_paths = FxHashMap::default();
    file_paths.insert(FileId(0), &path);

    let (findings, _, _) = collect_findings(
        &modules,
        &file_paths,
        Path::new("/project"),
        &globset::GlobSet::empty(),
        None,
        None,
        20,
        15,
        false,
    );
    assert_eq!(findings.len(), 1);
    let f = &findings[0];
    assert_eq!(f.name, "processData");
    assert_eq!(f.line, 42);
    assert_eq!(f.col, 8);
    assert_eq!(f.cyclomatic, 25);
    assert_eq!(f.cognitive, 18);
    assert_eq!(f.line_count, 75);
    assert_eq!(f.path, PathBuf::from("/project/src/a.ts"));
}

#[test]
fn merge_crap_findings_disambiguates_same_line_functions() {
    let path = PathBuf::from("/project/src/curried.ts");
    let outer = FunctionComplexity {
        name: "handler".to_string(),
        line: 1,
        col: 23,
        cyclomatic: 1,
        cognitive: 0,
        line_count: 11,
        param_count: 1,
        react_hook_count: 0,
        react_jsx_max_depth: 0,
        react_prop_count: 0,
        source_hash: None,
        contributions: Vec::new(),
    };
    let inner = FunctionComplexity {
        name: "<arrow>".to_string(),
        line: 1,
        col: 43,
        cyclomatic: 7,
        cognitive: 0,
        line_count: 10,
        param_count: 1,
        react_hook_count: 0,
        react_jsx_max_depth: 0,
        react_prop_count: 0,
        source_hash: None,
        contributions: Vec::new(),
    };
    let modules = vec![make_module(FileId(0), vec![inner.clone(), outer.clone()])];
    let mut file_paths: FxHashMap<FileId, &PathBuf> = FxHashMap::default();
    file_paths.insert(FileId(0), &path);

    let mut findings: Vec<ComplexityViolation> = Vec::new();

    let mut per_function_crap: FxHashMap<PathBuf, Vec<scoring::PerFunctionCrap>> =
        FxHashMap::default();
    per_function_crap.insert(
        path.clone(),
        vec![
            scoring::PerFunctionCrap {
                line: inner.line,
                col: inner.col,
                crap: 56.0,
                coverage_pct: None,
                coverage_tier: fallow_output::CoverageTier::None,
                coverage_source: fallow_output::CoverageSource::Estimated,
            },
            scoring::PerFunctionCrap {
                line: outer.line,
                col: outer.col,
                crap: 2.0,
                coverage_pct: None,
                coverage_tier: fallow_output::CoverageTier::None,
                coverage_source: fallow_output::CoverageSource::Estimated,
            },
        ],
    );

    let resolver = threshold_resolver(&[]);
    let mut tracker = ThresholdOverrideStateTracker::default();
    let mut input = CrapFindingMergeInput {
        modules: &modules,
        file_paths: &file_paths,
        config_root: Path::new("/project"),
        ignore_set: &globset::GlobSet::empty(),
        changed_files: None,
        ws_roots: None,
        per_function_crap: &per_function_crap,
        template_inherit_provenance: &FxHashMap::default(),
        complexity_breakdown: false,
        threshold_resolver: &resolver,
        threshold_state_tracker: &mut tracker,
    };
    merge_crap_findings(&mut findings, &mut input);

    assert_eq!(
        findings.len(),
        1,
        "expected one CRAP finding for inner arrow"
    );
    let f = &findings[0];
    assert_eq!(f.name, "<arrow>", "name must come from inner arrow");
    assert_eq!(f.line, 1);
    assert_eq!(f.col, 43, "col must disambiguate same-line arrows");
    assert_eq!(f.cyclomatic, 7, "cyclomatic must come from inner arrow");
    assert_eq!(f.cognitive, 0);
    assert_eq!(
        f.crap,
        Some(56.0),
        "CRAP must match the function it's reported against"
    );
    let cc = f64::from(f.cyclomatic);
    #[expect(
        clippy::suboptimal_flops,
        reason = "cc * cc + cc matches the CRAP formula specification"
    )]
    let expected_crap = cc * cc + cc;
    assert!(
        (f.crap.unwrap() - expected_crap).abs() < 0.01,
        "CRAP must be consistent with reported CC: cc={cc}, crap={:?}, expected={expected_crap}",
        f.crap,
    );
}

#[test]
fn merge_crap_findings_picks_outer_when_outer_exceeds() {
    let path = PathBuf::from("/project/src/curried_outer.ts");
    let outer = FunctionComplexity {
        name: "complex".to_string(),
        line: 5,
        col: 10,
        cyclomatic: 8,
        cognitive: 0,
        line_count: 20,
        param_count: 1,
        react_hook_count: 0,
        react_jsx_max_depth: 0,
        react_prop_count: 0,
        source_hash: None,
        contributions: Vec::new(),
    };
    let inner = FunctionComplexity {
        name: "<arrow>".to_string(),
        line: 5,
        col: 30,
        cyclomatic: 1,
        cognitive: 0,
        line_count: 1,
        param_count: 1,
        react_hook_count: 0,
        react_jsx_max_depth: 0,
        react_prop_count: 0,
        source_hash: None,
        contributions: Vec::new(),
    };
    let modules = vec![make_module(FileId(0), vec![inner.clone(), outer.clone()])];
    let mut file_paths: FxHashMap<FileId, &PathBuf> = FxHashMap::default();
    file_paths.insert(FileId(0), &path);

    let mut findings: Vec<ComplexityViolation> = Vec::new();
    let mut per_function_crap: FxHashMap<PathBuf, Vec<scoring::PerFunctionCrap>> =
        FxHashMap::default();
    per_function_crap.insert(
        path.clone(),
        vec![
            scoring::PerFunctionCrap {
                line: inner.line,
                col: inner.col,
                crap: 2.0,
                coverage_pct: None,
                coverage_tier: fallow_output::CoverageTier::None,
                coverage_source: fallow_output::CoverageSource::Estimated,
            },
            scoring::PerFunctionCrap {
                line: outer.line,
                col: outer.col,
                crap: 72.0,
                coverage_pct: None,
                coverage_tier: fallow_output::CoverageTier::None,
                coverage_source: fallow_output::CoverageSource::Estimated,
            },
        ],
    );

    let resolver = threshold_resolver(&[]);
    let mut tracker = ThresholdOverrideStateTracker::default();
    let mut input = CrapFindingMergeInput {
        modules: &modules,
        file_paths: &file_paths,
        config_root: Path::new("/project"),
        ignore_set: &globset::GlobSet::empty(),
        changed_files: None,
        ws_roots: None,
        per_function_crap: &per_function_crap,
        template_inherit_provenance: &FxHashMap::default(),
        complexity_breakdown: false,
        threshold_resolver: &resolver,
        threshold_state_tracker: &mut tracker,
    };
    merge_crap_findings(&mut findings, &mut input);

    assert_eq!(findings.len(), 1);
    let f = &findings[0];
    assert_eq!(f.name, "complex");
    assert_eq!(f.col, 10);
    assert_eq!(f.cyclomatic, 8);
    assert_eq!(f.crap, Some(72.0));
}

fn fx_summary(
    tracked: usize,
    hit: usize,
    unhit: usize,
    untracked: usize,
) -> fallow_output::RuntimeCoverageSummary {
    #[expect(
        clippy::cast_precision_loss,
        reason = "test fixture totals are tiny, f64 precision is fine"
    )]
    let coverage_percent = if tracked == 0 {
        0.0
    } else {
        (hit as f64 / tracked as f64) * 100.0
    };
    fallow_output::RuntimeCoverageSummary {
        data_source: fallow_output::RuntimeCoverageDataSource::Local,
        last_received_at: None,
        functions_tracked: tracked,
        functions_hit: hit,
        functions_unhit: unhit,
        functions_untracked: untracked,
        coverage_percent,
        trace_count: 512,
        period_days: 7,
        deployments_seen: 2,
        capture_quality: None,
    }
}

fn fx_evidence(
    static_status: &str,
    test_coverage: &str,
    v8_tracking: &str,
) -> fallow_output::RuntimeCoverageEvidence {
    fallow_output::RuntimeCoverageEvidence {
        static_status: static_status.to_owned(),
        test_coverage: test_coverage.to_owned(),
        v8_tracking: v8_tracking.to_owned(),
        untracked_reason: None,
        observation_days: 7,
        deployments_observed: 2,
    }
}

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "test fixture; linear setup/assert, length is not a maintainability concern"
)]
fn runtime_coverage_top_applies_after_baseline_filtering() {
    let root = Path::new("/project");
    let baseline = HealthBaselineData {
        findings: vec![],
        finding_counts: std::collections::BTreeMap::new(),
        runtime_coverage_findings: vec![
            "fallow:prod:aaaaaaaa".to_owned(),
            "fallow:prod:bbbbbbbb".to_owned(),
        ],
        runtime_coverage_source_hashes: vec![],
        target_keys: vec![],
    };
    let mut report = fallow_output::RuntimeCoverageReport {
        schema_version: fallow_output::RuntimeCoverageSchemaVersion::V1,
        verdict: fallow_output::RuntimeCoverageReportVerdict::ColdCodeDetected,
        signals: Vec::new(),
        summary: fx_summary(3, 0, 2, 1),
        findings: vec![
            fallow_output::RuntimeCoverageFinding {
                id: "fallow:prod:aaaaaaaa".to_owned(),
                stable_id: None,
                path: PathBuf::from("/project/src/a.ts"),
                function: "alpha".to_owned(),
                line: 10,
                verdict: fallow_output::RuntimeCoverageVerdict::ReviewRequired,
                invocations: Some(0),
                confidence: fallow_output::RuntimeCoverageConfidence::Medium,
                evidence: fx_evidence("used", "not_covered", "tracked"),
                actions: vec![],
                source_hash: None,
                discriminators: None,
            },
            fallow_output::RuntimeCoverageFinding {
                id: "fallow:prod:bbbbbbbb".to_owned(),
                stable_id: None,
                path: PathBuf::from("/project/src/b.ts"),
                function: "beta".to_owned(),
                line: 20,
                verdict: fallow_output::RuntimeCoverageVerdict::CoverageUnavailable,
                invocations: None,
                confidence: fallow_output::RuntimeCoverageConfidence::None,
                evidence: fx_evidence("used", "not_covered", "untracked"),
                actions: vec![],
                source_hash: None,
                discriminators: None,
            },
            fallow_output::RuntimeCoverageFinding {
                id: "fallow:prod:cccccccc".to_owned(),
                stable_id: None,
                path: PathBuf::from("/project/src/c.ts"),
                function: "gamma".to_owned(),
                line: 30,
                verdict: fallow_output::RuntimeCoverageVerdict::ReviewRequired,
                invocations: Some(0),
                confidence: fallow_output::RuntimeCoverageConfidence::Medium,
                evidence: fx_evidence("used", "not_covered", "tracked"),
                actions: vec![],
                source_hash: None,
                discriminators: None,
            },
        ],
        hot_paths: vec![
            fallow_output::RuntimeCoverageHotPath {
                id: "fallow:hot:11111111".to_owned(),
                stable_id: None,
                path: PathBuf::from("/project/src/hot-a.ts"),
                function: "hotAlpha".to_owned(),
                line: 1,
                end_line: 5,
                invocations: 500,
                percentile: 99,
                actions: vec![],
            },
            fallow_output::RuntimeCoverageHotPath {
                id: "fallow:hot:22222222".to_owned(),
                stable_id: None,
                path: PathBuf::from("/project/src/hot-b.ts"),
                function: "hotBeta".to_owned(),
                line: 2,
                end_line: 8,
                invocations: 250,
                percentile: 50,
                actions: vec![],
            },
        ],
        blast_radius: vec![],
        importance: vec![],
        watermark: None,
        warnings: vec![],
        actionable: true,
        actionability_reason: None,
        actionability_verdict: None,
        provenance: fallow_output::RuntimeCoverageProvenance::default(),
    };

    apply_runtime_coverage_filters(
        &mut report,
        &RuntimeCoverageFilterContext::new(root)
            .with_baseline(Some(&baseline))
            .with_top(Some(1)),
    );

    assert_eq!(report.findings.len(), 1);
    assert_eq!(report.findings[0].function, "gamma");
    assert_eq!(
        report.verdict,
        fallow_output::RuntimeCoverageReportVerdict::ColdCodeDetected
    );
    assert_eq!(report.summary.functions_tracked, 3);
    assert_eq!(report.summary.functions_hit, 0);
    assert_eq!(report.summary.functions_unhit, 2);
    assert_eq!(report.summary.functions_untracked, 1);
    assert!((report.summary.coverage_percent - 0.0).abs() < 0.05);
    assert_eq!(report.hot_paths.len(), 1);
    assert_eq!(report.hot_paths[0].function, "hotAlpha");
}

#[test]
fn runtime_coverage_baseline_refreshes_to_clean_when_only_baselined_findings_remain() {
    let root = Path::new("/project");
    let baseline = HealthBaselineData {
        findings: vec![],
        finding_counts: std::collections::BTreeMap::new(),
        runtime_coverage_findings: vec!["fallow:prod:aaaaaaaa".to_owned()],
        runtime_coverage_source_hashes: vec![],
        target_keys: vec![],
    };
    let mut report = fallow_output::RuntimeCoverageReport {
        schema_version: fallow_output::RuntimeCoverageSchemaVersion::V1,
        verdict: fallow_output::RuntimeCoverageReportVerdict::ColdCodeDetected,
        signals: Vec::new(),
        summary: fx_summary(2, 1, 1, 0),
        findings: vec![fallow_output::RuntimeCoverageFinding {
            id: "fallow:prod:aaaaaaaa".to_owned(),
            stable_id: None,
            path: PathBuf::from("/project/src/a.ts"),
            function: "alpha".to_owned(),
            line: 10,
            verdict: fallow_output::RuntimeCoverageVerdict::ReviewRequired,
            invocations: Some(0),
            confidence: fallow_output::RuntimeCoverageConfidence::Medium,
            evidence: fx_evidence("used", "not_covered", "tracked"),
            actions: vec![],
            source_hash: None,
            discriminators: None,
        }],
        hot_paths: vec![],
        blast_radius: vec![],
        importance: vec![],
        watermark: None,
        warnings: vec![],
        actionable: true,
        actionability_reason: None,
        actionability_verdict: None,
        provenance: fallow_output::RuntimeCoverageProvenance::default(),
    };

    apply_runtime_coverage_filters(
        &mut report,
        &RuntimeCoverageFilterContext::new(root).with_baseline(Some(&baseline)),
    );

    assert!(report.findings.is_empty());
    assert_eq!(
        report.verdict,
        fallow_output::RuntimeCoverageReportVerdict::Clean
    );
    assert_eq!(report.summary.functions_tracked, 2);
    assert_eq!(report.summary.functions_hit, 1);
    assert_eq!(report.summary.functions_unhit, 1);
    assert_eq!(report.summary.functions_untracked, 0);
    assert!((report.summary.coverage_percent - 50.0).abs() < 0.05);
}

#[test]
fn runtime_coverage_changed_review_uses_hot_path_verdict() {
    let root = Path::new("/project");
    let mut changed_files = FxHashSet::default();
    changed_files.insert(PathBuf::from("/project/src/hot.ts"));
    let mut report = fallow_output::RuntimeCoverageReport {
        schema_version: fallow_output::RuntimeCoverageSchemaVersion::V1,
        verdict: fallow_output::RuntimeCoverageReportVerdict::Clean,
        signals: Vec::new(),
        summary: fx_summary(2, 2, 0, 0),
        findings: vec![],
        hot_paths: vec![fallow_output::RuntimeCoverageHotPath {
            id: "fallow:hot:33333333".to_owned(),
            stable_id: None,
            path: PathBuf::from("/project/src/hot.ts"),
            function: "renderHotPath".to_owned(),
            line: 7,
            end_line: 24,
            invocations: 9_500,
            percentile: 99,
            actions: vec![],
        }],
        blast_radius: vec![],
        importance: vec![],
        watermark: None,
        warnings: vec![],
        actionable: true,
        actionability_reason: None,
        actionability_verdict: None,
        provenance: fallow_output::RuntimeCoverageProvenance::default(),
    };

    apply_runtime_coverage_filters(
        &mut report,
        &RuntimeCoverageFilterContext::new(root).with_changed_files(Some(&changed_files)),
    );

    assert_eq!(
        report.verdict,
        fallow_output::RuntimeCoverageReportVerdict::HotPathTouched
    );
}

#[test]
fn runtime_coverage_changed_review_ignores_unmodified_hot_paths() {
    let root = Path::new("/project");
    let mut changed_files = FxHashSet::default();
    changed_files.insert(PathBuf::from("/project/src/other.ts"));
    let mut report = fallow_output::RuntimeCoverageReport {
        schema_version: fallow_output::RuntimeCoverageSchemaVersion::V1,
        verdict: fallow_output::RuntimeCoverageReportVerdict::Clean,
        signals: Vec::new(),
        summary: fx_summary(2, 2, 0, 0),
        findings: vec![],
        hot_paths: vec![fallow_output::RuntimeCoverageHotPath {
            id: "fallow:hot:44444444".to_owned(),
            stable_id: None,
            path: PathBuf::from("/project/src/hot.ts"),
            function: "renderHotPath".to_owned(),
            line: 7,
            end_line: 24,
            invocations: 9_500,
            percentile: 90,
            actions: vec![],
        }],
        blast_radius: vec![],
        importance: vec![],
        watermark: None,
        warnings: vec![],
        actionable: true,
        actionability_reason: None,
        actionability_verdict: None,
        provenance: fallow_output::RuntimeCoverageProvenance::default(),
    };

    apply_runtime_coverage_filters(
        &mut report,
        &RuntimeCoverageFilterContext::new(root).with_changed_files(Some(&changed_files)),
    );

    assert!(report.hot_paths.is_empty());
    assert_eq!(
        report.verdict,
        fallow_output::RuntimeCoverageReportVerdict::Clean
    );
}

fn fx_runtime_coverage_report_with_hot_paths(
    hot_paths: Vec<fallow_output::RuntimeCoverageHotPath>,
) -> fallow_output::RuntimeCoverageReport {
    fallow_output::RuntimeCoverageReport {
        schema_version: fallow_output::RuntimeCoverageSchemaVersion::V1,
        verdict: fallow_output::RuntimeCoverageReportVerdict::Clean,
        signals: Vec::new(),
        summary: fx_summary(2, 2, 0, 0),
        findings: vec![],
        hot_paths,
        blast_radius: vec![],
        importance: vec![],
        watermark: None,
        warnings: vec![],
        actionable: true,
        actionability_reason: None,
        actionability_verdict: None,
        provenance: fallow_output::RuntimeCoverageProvenance::default(),
    }
}

fn fx_hot_path(
    id: &str,
    path: &str,
    line: u32,
    end_line: u32,
) -> fallow_output::RuntimeCoverageHotPath {
    fallow_output::RuntimeCoverageHotPath {
        id: id.to_owned(),
        stable_id: None,
        path: PathBuf::from(path),
        function: "renderHotPath".to_owned(),
        line,
        end_line,
        invocations: 9_500,
        percentile: 99,
        actions: vec![],
    }
}

#[test]
fn runtime_coverage_diff_index_keeps_hot_paths_with_added_line_in_range() {
    let root = Path::new("/project");
    let diff = "diff --git a/src/hot.ts b/src/hot.ts\n\
                    --- a/src/hot.ts\n\
                    +++ b/src/hot.ts\n\
                    @@ -10,1 +10,2 @@\n\
                    +  // touch the body\n\
                    line 11\n";
    let diff_index = fallow_output::DiffIndex::from_unified_diff(diff);
    let mut report = fx_runtime_coverage_report_with_hot_paths(vec![fx_hot_path(
        "fallow:hot:01010101",
        "src/hot.ts",
        7,
        24,
    )]);

    apply_runtime_coverage_filters(
        &mut report,
        &RuntimeCoverageFilterContext::new(root).with_diff_index(Some(&diff_index)),
    );

    assert_eq!(report.hot_paths.len(), 1);
    assert_eq!(
        report.verdict,
        fallow_output::RuntimeCoverageReportVerdict::HotPathTouched
    );
}

#[test]
fn runtime_coverage_diff_index_drops_hot_paths_when_added_line_outside_range() {
    let root = Path::new("/project");
    let diff = "diff --git a/src/hot.ts b/src/hot.ts\n\
                    --- a/src/hot.ts\n\
                    +++ b/src/hot.ts\n\
                    @@ -50,1 +50,2 @@\n\
                    +  // unrelated change far below the hot function\n\
                    line 51\n";
    let diff_index = fallow_output::DiffIndex::from_unified_diff(diff);
    let mut report = fx_runtime_coverage_report_with_hot_paths(vec![fx_hot_path(
        "fallow:hot:02020202",
        "src/hot.ts",
        7,
        24,
    )]);

    apply_runtime_coverage_filters(
        &mut report,
        &RuntimeCoverageFilterContext::new(root).with_diff_index(Some(&diff_index)),
    );

    assert!(report.hot_paths.is_empty());
    assert_eq!(
        report.verdict,
        fallow_output::RuntimeCoverageReportVerdict::Clean
    );
}

#[test]
fn runtime_coverage_diff_index_falls_back_to_single_line_when_end_line_zero() {
    let root = Path::new("/project");
    let diff = "diff --git a/src/hot.ts b/src/hot.ts\n\
                    --- a/src/hot.ts\n\
                    +++ b/src/hot.ts\n\
                    @@ -7,1 +7,2 @@\n\
                    +  // exactly the function's start line\n\
                    line 8\n";
    let diff_index = fallow_output::DiffIndex::from_unified_diff(diff);
    let mut report = fx_runtime_coverage_report_with_hot_paths(vec![fx_hot_path(
        "fallow:hot:03030303",
        "src/hot.ts",
        7,
        0,
    )]);

    apply_runtime_coverage_filters(
        &mut report,
        &RuntimeCoverageFilterContext::new(root).with_diff_index(Some(&diff_index)),
    );

    assert_eq!(report.hot_paths.len(), 1);
    assert_eq!(
        report.verdict,
        fallow_output::RuntimeCoverageReportVerdict::HotPathTouched
    );
}

#[test]
fn runtime_coverage_diff_index_resolves_absolute_hot_path_against_root() {
    let root = Path::new("/project");
    let diff = "diff --git a/src/hot.ts b/src/hot.ts\n\
                    --- a/src/hot.ts\n\
                    +++ b/src/hot.ts\n\
                    @@ -10,1 +10,2 @@\n\
                    +  // touched\n\
                    line 11\n";
    let diff_index = fallow_output::DiffIndex::from_unified_diff(diff);
    let mut report = fx_runtime_coverage_report_with_hot_paths(vec![fx_hot_path(
        "fallow:hot:04040404",
        "/project/src/hot.ts",
        7,
        24,
    )]);

    apply_runtime_coverage_filters(
        &mut report,
        &RuntimeCoverageFilterContext::new(root).with_diff_index(Some(&diff_index)),
    );

    assert_eq!(report.hot_paths.len(), 1);
}

#[test]
fn runtime_coverage_diff_index_authoritative_for_files_in_diff() {
    let root = Path::new("/project");
    let diff = "diff --git a/src/hot.ts b/src/hot.ts\n\
                    --- a/src/hot.ts\n\
                    +++ b/src/hot.ts\n\
                    @@ -50,1 +50,2 @@\n\
                    +  // outside the hot function\n\
                    line 51\n";
    let diff_index = fallow_output::DiffIndex::from_unified_diff(diff);
    let mut changed_files = FxHashSet::default();
    changed_files.insert(PathBuf::from("/project/src/hot.ts"));
    let mut report = fx_runtime_coverage_report_with_hot_paths(vec![fx_hot_path(
        "fallow:hot:05050505",
        "src/hot.ts",
        7,
        24,
    )]);

    apply_runtime_coverage_filters(
        &mut report,
        &RuntimeCoverageFilterContext::new(root)
            .with_changed_files(Some(&changed_files))
            .with_diff_index(Some(&diff_index)),
    );

    assert!(report.hot_paths.is_empty());
    assert_eq!(
        report.verdict,
        fallow_output::RuntimeCoverageReportVerdict::Clean
    );
}

#[test]
fn runtime_coverage_per_file_fallback_to_changed_files_when_diff_omits_file() {
    let root = Path::new("/project");
    let diff = "diff --git a/src/other.ts b/src/other.ts\n\
                    --- a/src/other.ts\n\
                    +++ b/src/other.ts\n\
                    @@ -1,1 +1,2 @@\n\
                    +  // unrelated\n\
                    line 2\n";
    let diff_index = fallow_output::DiffIndex::from_unified_diff(diff);
    let mut changed_files = FxHashSet::default();
    changed_files.insert(PathBuf::from("/project/src/hot.ts"));
    let mut report = fx_runtime_coverage_report_with_hot_paths(vec![fx_hot_path(
        "fallow:hot:0a0a0a0a",
        "src/hot.ts",
        7,
        24,
    )]);

    apply_runtime_coverage_filters(
        &mut report,
        &RuntimeCoverageFilterContext::new(root)
            .with_changed_files(Some(&changed_files))
            .with_diff_index(Some(&diff_index)),
    );

    assert_eq!(report.hot_paths.len(), 1);
    assert_eq!(
        report.verdict,
        fallow_output::RuntimeCoverageReportVerdict::HotPathTouched
    );
}

#[test]
fn runtime_coverage_pr_context_promotes_hot_path_touched_above_cold_code() {
    let root = Path::new("/project");
    let mut changed_files = FxHashSet::default();
    changed_files.insert(PathBuf::from("/project/src/hot.ts"));
    let mut report = fallow_output::RuntimeCoverageReport {
        schema_version: fallow_output::RuntimeCoverageSchemaVersion::V1,
        verdict: fallow_output::RuntimeCoverageReportVerdict::ColdCodeDetected,
        signals: Vec::new(),
        summary: fx_summary(2, 1, 1, 0),
        findings: vec![fallow_output::RuntimeCoverageFinding {
            id: "fallow:prod:cold0001".to_owned(),
            stable_id: None,
            path: PathBuf::from("/project/src/cold.ts"),
            function: "coldFn".to_owned(),
            line: 4,
            verdict: fallow_output::RuntimeCoverageVerdict::SafeToDelete,
            invocations: Some(0),
            confidence: fallow_output::RuntimeCoverageConfidence::High,
            evidence: fx_evidence("unused", "not_covered", "tracked"),
            actions: vec![],
            source_hash: None,
            discriminators: None,
        }],
        hot_paths: vec![fx_hot_path("fallow:hot:0b0b0b0b", "src/hot.ts", 7, 24)],
        blast_radius: vec![],
        importance: vec![],
        watermark: None,
        warnings: vec![],
        actionable: true,
        actionability_reason: None,
        actionability_verdict: None,
        provenance: fallow_output::RuntimeCoverageProvenance::default(),
    };

    apply_runtime_coverage_filters(
        &mut report,
        &RuntimeCoverageFilterContext::new(root).with_changed_files(Some(&changed_files)),
    );

    assert_eq!(
        report.verdict,
        fallow_output::RuntimeCoverageReportVerdict::HotPathTouched
    );
    assert_eq!(
        report.signals,
        vec![
            fallow_output::RuntimeCoverageSignal::ColdCodeDetected,
            fallow_output::RuntimeCoverageSignal::HotPathTouched,
        ]
    );
}

#[test]
fn runtime_coverage_standalone_keeps_cold_code_primary_above_unchanged_hot_paths() {
    let root = Path::new("/project");
    let mut report = fallow_output::RuntimeCoverageReport {
        schema_version: fallow_output::RuntimeCoverageSchemaVersion::V1,
        verdict: fallow_output::RuntimeCoverageReportVerdict::Clean,
        signals: Vec::new(),
        summary: fx_summary(2, 1, 1, 0),
        findings: vec![fallow_output::RuntimeCoverageFinding {
            id: "fallow:prod:cold0002".to_owned(),
            stable_id: None,
            path: PathBuf::from("/project/src/cold.ts"),
            function: "coldFn".to_owned(),
            line: 4,
            verdict: fallow_output::RuntimeCoverageVerdict::SafeToDelete,
            invocations: Some(0),
            confidence: fallow_output::RuntimeCoverageConfidence::High,
            evidence: fx_evidence("unused", "not_covered", "tracked"),
            actions: vec![],
            source_hash: None,
            discriminators: None,
        }],
        hot_paths: vec![fx_hot_path("fallow:hot:0c0c0c0c", "src/hot.ts", 7, 24)],
        blast_radius: vec![],
        importance: vec![],
        watermark: None,
        warnings: vec![],
        actionable: true,
        actionability_reason: None,
        actionability_verdict: None,
        provenance: fallow_output::RuntimeCoverageProvenance::default(),
    };

    apply_runtime_coverage_filters(&mut report, &RuntimeCoverageFilterContext::new(root));

    assert_eq!(
        report.verdict,
        fallow_output::RuntimeCoverageReportVerdict::ColdCodeDetected
    );
    assert_eq!(
        report.signals,
        vec![fallow_output::RuntimeCoverageSignal::ColdCodeDetected]
    );
    assert_eq!(report.hot_paths.len(), 1);
}

#[test]
fn runtime_coverage_license_grace_outranks_pr_context_signals() {
    let root = Path::new("/project");
    let mut changed_files = FxHashSet::default();
    changed_files.insert(PathBuf::from("/project/src/hot.ts"));
    let mut report = fallow_output::RuntimeCoverageReport {
        schema_version: fallow_output::RuntimeCoverageSchemaVersion::V1,
        verdict: fallow_output::RuntimeCoverageReportVerdict::LicenseExpiredGrace,
        signals: Vec::new(),
        summary: fx_summary(2, 1, 1, 0),
        findings: vec![],
        hot_paths: vec![fx_hot_path("fallow:hot:0d0d0d0d", "src/hot.ts", 7, 24)],
        blast_radius: vec![],
        importance: vec![],
        watermark: Some(fallow_output::RuntimeCoverageWatermark::LicenseExpiredGrace),
        warnings: vec![],
        actionable: true,
        actionability_reason: None,
        actionability_verdict: None,
        provenance: fallow_output::RuntimeCoverageProvenance::default(),
    };

    apply_runtime_coverage_filters(
        &mut report,
        &RuntimeCoverageFilterContext::new(root).with_changed_files(Some(&changed_files)),
    );

    assert_eq!(
        report.verdict,
        fallow_output::RuntimeCoverageReportVerdict::LicenseExpiredGrace
    );
    assert!(
        report
            .signals
            .contains(&fallow_output::RuntimeCoverageSignal::LicenseExpiredGrace)
    );
    assert!(
        report
            .signals
            .contains(&fallow_output::RuntimeCoverageSignal::HotPathTouched)
    );
}

#[test]
fn retain_hot_paths_drops_when_diff_touches_file_but_no_added_lines() {
    let root = Path::new("/project");
    let diff = fallow_output::DiffIndex::from_unified_diff(
        "diff --git a/src/hot.ts b/src/hot.ts\n\
             --- a/src/hot.ts\n\
             +++ b/src/hot.ts\n\
             @@ -10,3 +10,1 @@\n\
             -one\n\
             -two\n\
             -three\n\
             ctx\n",
    );
    let mut changed_files = FxHashSet::default();
    changed_files.insert(PathBuf::from("/project/src/hot.ts"));
    let mut report = fx_runtime_coverage_report_with_hot_paths(vec![fx_hot_path(
        "fallow:hot:deletiononly",
        "src/hot.ts",
        10,
        12,
    )]);

    apply_runtime_coverage_filters(
        &mut report,
        &RuntimeCoverageFilterContext::new(root)
            .with_diff_index(Some(&diff))
            .with_changed_files(Some(&changed_files)),
    );

    assert!(
        report.hot_paths.is_empty(),
        "diff touched the file with no added lines: must drop, not fall through to changed_files"
    );
}

#[test]
fn runtime_coverage_changed_files_matches_relative_hot_path_against_absolute_set() {
    let root = Path::new("/project");
    let mut changed_files = FxHashSet::default();
    changed_files.insert(PathBuf::from("/project/src/hot.ts"));
    let mut report = fx_runtime_coverage_report_with_hot_paths(vec![fx_hot_path(
        "fallow:hot:06060606",
        "src/hot.ts",
        7,
        24,
    )]);

    apply_runtime_coverage_filters(
        &mut report,
        &RuntimeCoverageFilterContext::new(root).with_changed_files(Some(&changed_files)),
    );

    assert_eq!(report.hot_paths.len(), 1);
}

fn make_class_finding(
    path: &str,
    name: &str,
    line: u32,
    cyclomatic: u16,
    cognitive: u16,
) -> ComplexityViolation {
    ComplexityViolation {
        path: PathBuf::from(path),
        name: name.to_string(),
        line,
        col: 0,
        cyclomatic,
        cognitive,
        line_count: 20,
        param_count: 0,
        react_hook_count: 0,
        react_jsx_max_depth: 0,
        react_prop_count: 0,
        react_hook_profile: None,
        exceeded: ExceededThreshold::Both,
        severity: FindingSeverity::Moderate,
        crap: None,
        coverage_pct: None,
        coverage_tier: None,
        coverage_source: None,
        inherited_from: None,
        component_rollup: None,
        contributions: Vec::new(),
        effective_thresholds: None,
        threshold_source: None,
    }
}

fn make_template_finding(
    path: &str,
    line: u32,
    cyclomatic: u16,
    cognitive: u16,
) -> ComplexityViolation {
    ComplexityViolation {
        path: PathBuf::from(path),
        name: "<template>".to_string(),
        line,
        col: 0,
        cyclomatic,
        cognitive,
        line_count: 30,
        param_count: 0,
        react_hook_count: 0,
        react_jsx_max_depth: 0,
        react_prop_count: 0,
        react_hook_profile: None,
        exceeded: ExceededThreshold::Both,
        severity: FindingSeverity::Moderate,
        crap: None,
        coverage_pct: None,
        coverage_tier: None,
        coverage_source: None,
        inherited_from: None,
        component_rollup: None,
        contributions: Vec::new(),
        effective_thresholds: None,
        threshold_source: None,
    }
}

#[test]
fn rollup_external_template_via_provenance_lookup() {
    let component_ts = PathBuf::from("/proj/src/host-game.component.ts");
    let template_html = PathBuf::from("/proj/src/host-game.component.html");
    let mut findings = vec![
        make_class_finding(component_ts.to_str().unwrap(), "handleClick", 42, 3, 4),
        make_template_finding(template_html.to_str().unwrap(), 1, 6, 10),
    ];
    let mut lookup = rustc_hash::FxHashMap::default();
    lookup.insert(template_html.clone(), component_ts.clone());
    append_component_rollup_findings(&mut findings, Some(&lookup), 8, 8);

    assert_eq!(findings.len(), 3, "rollup is strictly additive");
    let rollup = findings
        .iter()
        .find(|f| f.name == "<component>")
        .expect("rollup must be present");
    assert_eq!(rollup.path, component_ts);
    assert_eq!(rollup.cyclomatic, 9, "9 = worst class 3 + template 6");
    assert_eq!(rollup.cognitive, 14, "14 = worst class 4 + template 10");
    assert_eq!(rollup.line, 42, "anchored at worst class function line");
    let breakdown = rollup.component_rollup.as_ref().expect("breakdown present");
    assert_eq!(
        breakdown.component, "host-game.component",
        "component identifier is the .ts owner's file stem"
    );
    assert_eq!(breakdown.class_worst_function, "handleClick");
    assert_eq!(breakdown.class_cyclomatic, 3);
    assert_eq!(breakdown.template_cyclomatic, 6);
    assert_eq!(breakdown.template_path, template_html);
}

#[test]
fn rollup_inline_template_owner_is_same_ts_file() {
    let component_ts = PathBuf::from("/proj/src/inline.component.ts");
    let mut findings = vec![
        make_class_finding(component_ts.to_str().unwrap(), "ngOnInit", 25, 5, 8),
        make_template_finding(component_ts.to_str().unwrap(), 10, 4, 6),
    ];
    append_component_rollup_findings(&mut findings, None, 8, 8);

    let rollup = findings
        .iter()
        .find(|f| f.name == "<component>")
        .expect("rollup must be present for inline-template case without provenance lookup");
    assert_eq!(rollup.cyclomatic, 9);
    assert_eq!(rollup.cognitive, 14);
    let breakdown = rollup.component_rollup.as_ref().unwrap();
    assert_eq!(breakdown.template_path, component_ts);
    assert_eq!(breakdown.component, "inline.component");
}

#[test]
fn rollup_picks_worst_class_function_by_cyclomatic() {
    let component_ts = PathBuf::from("/proj/src/multi.component.ts");
    let template = PathBuf::from("/proj/src/multi.component.html");
    let mut findings = vec![
        make_class_finding(component_ts.to_str().unwrap(), "first", 10, 3, 4),
        make_class_finding(component_ts.to_str().unwrap(), "worst", 20, 8, 9),
        make_class_finding(component_ts.to_str().unwrap(), "middle", 30, 5, 6),
        make_template_finding(template.to_str().unwrap(), 1, 4, 6),
    ];
    let mut lookup = rustc_hash::FxHashMap::default();
    lookup.insert(template, component_ts);
    append_component_rollup_findings(&mut findings, Some(&lookup), 8, 8);

    let rollup = findings.iter().find(|f| f.name == "<component>").unwrap();
    assert_eq!(rollup.cyclomatic, 12, "8 (worst.cyc) + 4 (template.cyc)");
    let breakdown = rollup.component_rollup.as_ref().unwrap();
    assert_eq!(breakdown.class_worst_function, "worst");
    assert_eq!(breakdown.class_cyclomatic, 8);
}

#[test]
fn rollup_skipped_when_no_template_finding() {
    let component_ts = "/proj/src/only-class.component.ts";
    let mut findings = vec![make_class_finding(component_ts, "Foo.method", 10, 5, 7)];
    let before = findings.len();
    append_component_rollup_findings(&mut findings, None, 30, 25);
    assert_eq!(findings.len(), before, "no template means no rollup");
}

#[test]
fn rollup_skipped_when_no_class_findings() {
    let template_html = PathBuf::from("/proj/src/orphan.component.html");
    let component_ts = PathBuf::from("/proj/src/orphan.component.ts");
    let mut findings = vec![make_template_finding(
        template_html.to_str().unwrap(),
        1,
        6,
        10,
    )];
    let mut lookup = rustc_hash::FxHashMap::default();
    lookup.insert(template_html, component_ts);
    let before = findings.len();
    append_component_rollup_findings(&mut findings, Some(&lookup), 8, 8);
    assert_eq!(
        findings.len(),
        before,
        "no class methods above threshold means no rollup"
    );
}

#[test]
fn rollup_skipped_when_multiple_templates_on_one_owner() {
    let component_ts = PathBuf::from("/proj/src/twin.component.ts");
    let mut findings = vec![
        make_class_finding(component_ts.to_str().unwrap(), "TwinA.fn", 10, 5, 7),
        make_template_finding(component_ts.to_str().unwrap(), 5, 3, 4),
        make_template_finding(component_ts.to_str().unwrap(), 50, 4, 5),
    ];
    let before = findings.len();
    append_component_rollup_findings(&mut findings, None, 30, 25);
    assert_eq!(
        findings.len(),
        before,
        "two templates on one owner is defensively skipped"
    );
}

#[test]
fn rollup_external_template_skipped_when_lookup_missing() {
    let template_html = PathBuf::from("/proj/src/no-owner.component.html");
    let component_ts = "/proj/src/no-owner.component.ts";
    let mut findings = vec![
        make_class_finding(component_ts, "NoOwner.fn", 10, 5, 7),
        make_template_finding(template_html.to_str().unwrap(), 1, 6, 10),
    ];
    let before = findings.len();
    append_component_rollup_findings(&mut findings, None, 30, 25);
    assert_eq!(findings.len(), before);
}
