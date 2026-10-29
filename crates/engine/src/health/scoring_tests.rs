use super::super::threshold_overrides::GlobalHealthThresholds;
use super::*;

/// Resolver with no override entries and the given global CRAP ceiling,
/// the default-configuration shape for scoring tests.
fn test_crap_resolver(crap: f64) -> ThresholdOverrideResolver {
    ThresholdOverrideResolver::new(
        &[],
        GlobalHealthThresholds {
            cyclomatic: 20,
            cognitive: 15,
            crap,
            unit_size: 120,
        },
    )
}

/// Resolver with the given override entries over the default global 30.0.
fn test_override_resolver(
    overrides: &[fallow_config::HealthThresholdOverride],
) -> ThresholdOverrideResolver {
    ThresholdOverrideResolver::new(
        overrides,
        GlobalHealthThresholds {
            cyclomatic: 20,
            cognitive: 15,
            crap: CRAP_THRESHOLD,
            unit_size: 120,
        },
    )
}

/// `compute_crap_scores_istanbul` with default-configuration ceilings.
pub(super) fn istanbul_crap_default(
    complexity: &[fallow_types::extract::FunctionComplexity],
    file_coverage: Option<&IstanbulFileCoverage>,
    is_test_reachable: bool,
) -> IstanbulCrapResult {
    let resolver = test_crap_resolver(CRAP_THRESHOLD);
    let ceilings = CrapCeilingLookup::new(
        CrapScoreThresholds {
            resolver: &resolver,
            enforce_crap: true,
        },
        std::path::Path::new("src/test.ts"),
        &[],
    );
    compute_crap_scores_istanbul(complexity, file_coverage, is_test_reachable, &ceilings)
}

/// A coverage map that says nothing about a function is not evidence that
/// the function ran, so passing one must not score it lower than the run
/// without a map would have. Both paths use the same static estimate for a
/// function whose file tests reach.
#[test]
fn an_unmatched_function_scores_the_same_with_and_without_a_coverage_map() {
    let temp = tempfile::TempDir::new().unwrap();
    let source_path = temp.path().join("src/grade.ts");
    std::fs::create_dir_all(source_path.parent().unwrap()).unwrap();
    std::fs::write(&source_path, "// geometry fixture\n").unwrap();

    // A map that carries the file but records a function fallow never
    // extracted, which is what a stale map or an unresolved producer
    // anchor produces.
    let coverage_path = temp.path().join("coverage-final.json");
    write_single_file_istanbul_fixture(
        &coverage_path,
        &source_path,
        &serde_json::json!({
            "0": {
                "name": "unrelated",
                "line": 40,
                "decl": { "start": { "line": 40, "column": 0 }, "end": { "line": 40, "column": 9 } },
                "loc": { "start": { "line": 40, "column": 20 }, "end": { "line": 44, "column": 1 } }
            }
        }),
        &serde_json::json!({ "0": 1 }),
    );
    let coverage = load_istanbul_coverage(&coverage_path, None, None, false).unwrap();
    let canonical_source = dunce::canonicalize(&source_path).unwrap();
    let file_coverage = coverage.get(&canonical_source).unwrap();

    let function = make_fn_complexity(10);
    let with_map =
        istanbul_crap_default(std::slice::from_ref(&function), Some(file_coverage), true);
    let estimated = compute_crap_scores_estimated(
        std::slice::from_ref(&function),
        &rustc_hash::FxHashSet::default(),
        true,
        fallow_output::CoverageSource::Estimated,
        &CrapCeilingLookup::new(
            CrapScoreThresholds {
                resolver: &test_crap_resolver(CRAP_THRESHOLD),
                enforce_crap: true,
            },
            std::path::Path::new("src/test.ts"),
            &[],
        ),
    );

    assert_eq!(with_map.matched, 0);
    assert!(
        (with_map.per_function[0].crap - estimated.per_function[0].crap).abs() < f64::EPSILON,
        "a map that attributes nothing must not change the score"
    );
    assert_eq!(with_map.per_function[0].coverage_pct, None);
}

fn test_istanbul_file_coverage(
    functions: rustc_hash::FxHashMap<(String, u32, u32), f64>,
    relocated: bool,
) -> IstanbulFileCoverage {
    let functions = functions
        .into_iter()
        .map(
            |((name, line, col), coverage_pct)| IstanbulFunctionCoverage {
                name,
                coverage_pct,
                aliases: vec![primary_alias(line, col)],
                decl_start: IstanbulPosition::new(line, col),
                header_holds_other_fn: false,
                header_span: None,
                body_span: None,
            },
        )
        .collect();
    IstanbulFileCoverage::new(functions, relocated)
}

fn primary_alias(line: u32, col: u32) -> IstanbulAlias {
    IstanbulAlias {
        position: IstanbulPosition::new(line, col),
        primary: true,
    }
}

fn secondary_alias(line: u32, col: u32) -> IstanbulAlias {
    IstanbulAlias {
        position: IstanbulPosition::new(line, col),
        primary: false,
    }
}

fn body_span(start: (u32, u32), end: (u32, u32)) -> IstanbulSpan {
    IstanbulSpan {
        start: IstanbulPosition::new(start.0, start.1),
        end: IstanbulPosition::new(end.0, end.1),
    }
}

/// `compute_crap_scores_estimated` with default-configuration ceilings.
fn estimated_crap_default(
    complexity: &[fallow_types::extract::FunctionComplexity],
    test_referenced_exports: &rustc_hash::FxHashSet<String>,
    is_test_reachable: bool,
    coverage_source: fallow_output::CoverageSource,
) -> EstimatedCrapResult {
    let resolver = test_crap_resolver(CRAP_THRESHOLD);
    let ceilings = CrapCeilingLookup::new(
        CrapScoreThresholds {
            resolver: &resolver,
            enforce_crap: true,
        },
        std::path::Path::new("src/test.ts"),
        &[],
    );
    compute_crap_scores_estimated(
        complexity,
        test_referenced_exports,
        is_test_reachable,
        coverage_source,
        &ceilings,
    )
}

/// `compute_file_scores` with default-configuration CRAP thresholds.
fn compute_file_scores_default(
    modules: &[crate::source::ModuleInfo],
    file_paths: &rustc_hash::FxHashMap<crate::discover::FileId, &std::path::PathBuf>,
    changed_files: Option<&rustc_hash::FxHashSet<std::path::PathBuf>>,
    analysis_output: crate::results::DeadCodeAnalysisArtifacts,
    istanbul_coverage: Option<&IstanbulCoverage>,
    root: &std::path::Path,
) -> Result<FileScoreOutput, String> {
    let resolver = test_crap_resolver(CRAP_THRESHOLD);
    compute_file_scores(
        FileScoreComputeInput {
            modules,
            file_paths,
            changed_files,
            istanbul_coverage,
            root,
            crap_thresholds: CrapScoreThresholds {
                resolver: &resolver,
                enforce_crap: true,
            },
            with_static_links: false,
            test_reach: None,
        },
        analysis_output,
    )
}

#[test]
fn maintainability_perfect_score() {
    assert!((compute_maintainability_index(0.0, 0.0, 0, 100) - 100.0).abs() < f64::EPSILON);
}

#[test]
fn crap_resolution_prefers_template_inheritance_over_istanbul() {
    let inherit_ctx = TemplateInheritContext {
        is_test_reachable: true,
        test_referenced_exports: rustc_hash::FxHashSet::default(),
        provenance_owner: std::path::PathBuf::from("/project/src/app.component.ts"),
    };
    let istanbul = IstanbulCoverage {
        files: rustc_hash::FxHashMap::default(),
        format: fallow_output::CoverageInputFormat::Istanbul,
    };

    let resolution = resolve_crap_coverage(
        Some(&inherit_ctx),
        Some(&istanbul),
        std::path::Path::new("/project/src/app.component.html"),
    );

    assert!(matches!(
        resolution,
        CrapCoverageResolution::TemplateInherited(_)
    ));
}

#[test]
fn crap_resolution_keeps_istanbul_when_file_is_missing() {
    let istanbul = IstanbulCoverage {
        files: rustc_hash::FxHashMap::default(),
        format: fallow_output::CoverageInputFormat::Istanbul,
    };

    let resolution = resolve_crap_coverage(
        None,
        Some(&istanbul),
        std::path::Path::new("/project/src/missing.ts"),
    );

    assert!(matches!(
        resolution,
        CrapCoverageResolution::Istanbul {
            file_coverage: None
        }
    ));
}

#[test]
fn maintainability_clamped_at_zero() {
    assert!((compute_maintainability_index(10.0, 1.0, 100, 200) - 0.0).abs() < f64::EPSILON);
}

#[test]
fn maintainability_formula_correct() {
    let result = compute_maintainability_index(0.5, 0.3, 10, 100);
    let expected = 11.0_f64.ln().mul_add(-4.0, 100.0 - 15.0 - 6.0);
    assert!((result - expected).abs() < 0.01);
}

#[test]
fn maintainability_dead_file_penalty() {
    let result = compute_maintainability_index(0.0, 1.0, 0, 100);
    assert!((result - 80.0).abs() < f64::EPSILON);
}

#[test]
fn maintainability_fan_out_is_logarithmic() {
    let result_10 = compute_maintainability_index(0.0, 0.0, 10, 100);
    let result_100 = compute_maintainability_index(0.0, 0.0, 100, 100);
    let result_200 = compute_maintainability_index(0.0, 0.0, 200, 100);

    assert!(result_10 > 90.0); // ~90.4
    assert!(result_100 > 84.0); // 85.0 (capped)
    assert!((result_100 - result_200).abs() < f64::EPSILON);
}

#[test]
fn maintainability_fan_out_capped_at_15() {
    let result = compute_maintainability_index(0.0, 1.0, 1000, 100);
    assert!((result - 65.0).abs() < f64::EPSILON);
}

#[test]
fn maintainability_small_file_dampened() {
    let small = compute_maintainability_index(0.40, 0.0, 0, 5);
    assert!((small - 98.8).abs() < 0.01);
}

#[test]
fn maintainability_large_file_undampened() {
    let large = compute_maintainability_index(0.30, 0.0, 0, 192);
    assert!((large - 91.0).abs() < 0.01);
}

#[test]
fn maintainability_small_file_ranks_better_than_complex_large_file() {
    let trivial = compute_maintainability_index(0.40, 0.0, 0, 5);
    let nightmare = compute_maintainability_index(0.30, 0.0, 0, 192);
    assert!(
        trivial > nightmare,
        "trivial file ({trivial}) should rank better than nightmare ({nightmare})"
    );
}

#[test]
fn maintainability_at_dampening_boundary() {
    let at_boundary = compute_maintainability_index(0.5, 0.0, 0, 50);
    let above_boundary = compute_maintainability_index(0.5, 0.0, 0, 51);
    assert!((at_boundary - above_boundary).abs() < 0.01);
}

#[test]
fn maintainability_zero_lines_zero_density_penalty() {
    let result = compute_maintainability_index(5.0, 0.0, 0, 0);
    assert!((result - 100.0).abs() < f64::EPSILON);
}

#[test]
fn complexity_density_zero_lines() {
    assert!((compute_complexity_density(10, 0)).abs() < f64::EPSILON);
}

#[test]
fn complexity_density_normal() {
    let result = compute_complexity_density(10, 100);
    assert!((result - 0.1).abs() < f64::EPSILON);
}

#[test]
fn complexity_density_high() {
    let result = compute_complexity_density(50, 10);
    assert!((result - 5.0).abs() < f64::EPSILON);
}

#[test]
fn dead_code_ratio_no_exports() {
    let unused_files = rustc_hash::FxHashSet::default();
    let unused_map = rustc_hash::FxHashMap::default();
    let path = std::path::Path::new("/src/foo.ts");
    let exports: Vec<fallow_graph::graph::ExportSymbol> = vec![];

    let ratio = compute_dead_code_ratio(path, &exports, &unused_files, &unused_map);
    assert!((ratio).abs() < f64::EPSILON);
}

#[test]
fn dead_code_ratio_all_unused_file() {
    let mut unused_files: rustc_hash::FxHashSet<&std::path::Path> =
        rustc_hash::FxHashSet::default();
    let path = std::path::Path::new("/src/foo.ts");
    unused_files.insert(path);
    let unused_map = rustc_hash::FxHashMap::default();
    let exports: Vec<fallow_graph::graph::ExportSymbol> = vec![];

    let ratio = compute_dead_code_ratio(path, &exports, &unused_files, &unused_map);
    assert!((ratio - 1.0).abs() < f64::EPSILON);
}

#[test]
fn dead_code_ratio_mix() {
    let unused_files = rustc_hash::FxHashSet::default();
    let path = std::path::Path::new("/src/foo.ts");

    let exports = vec![
        fallow_graph::graph::ExportSymbol {
            name: crate::source::ExportName::Named("a".into()),
            is_type_only: false,
            is_side_effect_used: false,
            visibility: crate::source::VisibilityTag::None,
            expected_unused_reason: None,
            span: oxc_span::Span::empty(0),
            references: vec![],
            reference_paths: Vec::new(),
            members: vec![],
            deprecated: false,
            deprecated_reason: None,
        },
        fallow_graph::graph::ExportSymbol {
            name: crate::source::ExportName::Named("b".into()),
            is_type_only: false,
            is_side_effect_used: false,
            visibility: crate::source::VisibilityTag::None,
            expected_unused_reason: None,
            span: oxc_span::Span::empty(0),
            references: vec![],
            reference_paths: Vec::new(),
            members: vec![],
            deprecated: false,
            deprecated_reason: None,
        },
        fallow_graph::graph::ExportSymbol {
            name: crate::source::ExportName::Named("c".into()),
            is_type_only: false,
            is_side_effect_used: false,
            visibility: crate::source::VisibilityTag::None,
            expected_unused_reason: None,
            span: oxc_span::Span::empty(0),
            references: vec![],
            reference_paths: Vec::new(),
            members: vec![],
            deprecated: false,
            deprecated_reason: None,
        },
        fallow_graph::graph::ExportSymbol {
            name: crate::source::ExportName::Named("MyType".into()),
            is_type_only: true,
            is_side_effect_used: false,
            visibility: crate::source::VisibilityTag::None,
            expected_unused_reason: None,
            span: oxc_span::Span::empty(0),
            references: vec![],
            reference_paths: Vec::new(),
            members: vec![],
            deprecated: false,
            deprecated_reason: None,
        },
    ];

    let mut unused_map: rustc_hash::FxHashMap<&std::path::Path, usize> =
        rustc_hash::FxHashMap::default();
    unused_map.insert(path, 2);

    let ratio = compute_dead_code_ratio(path, &exports, &unused_files, &unused_map);
    assert!((ratio - 2.0 / 3.0).abs() < 1e-10);
}

#[test]
fn dead_code_ratio_all_type_only_exports() {
    let unused_files = rustc_hash::FxHashSet::default();
    let path = std::path::Path::new("/src/types.ts");

    let exports = vec![fallow_graph::graph::ExportSymbol {
        name: crate::source::ExportName::Named("Foo".into()),
        is_type_only: true,
        is_side_effect_used: false,
        visibility: crate::source::VisibilityTag::None,
        expected_unused_reason: None,
        span: oxc_span::Span::empty(0),
        references: vec![],
        reference_paths: Vec::new(),
        members: vec![],
        deprecated: false,
        deprecated_reason: None,
    }];
    let unused_map = rustc_hash::FxHashMap::default();

    let ratio = compute_dead_code_ratio(path, &exports, &unused_files, &unused_map);
    assert!((ratio).abs() < f64::EPSILON);
}

#[test]
fn aggregate_complexity_empty_module() {
    let module = crate::source::ModuleInfo::empty(crate::discover::FileId(0));

    let (cyc, cog, funcs, lines) = aggregate_complexity(&module);
    assert_eq!(cyc, 0);
    assert_eq!(cog, 0);
    assert_eq!(funcs, 0);
    assert_eq!(lines, 0);
}

#[test]
fn aggregate_complexity_single_function() {
    let module = crate::source::ModuleInfo {
        line_offsets: vec![0, 10, 20, 30, 40], // 5 lines
        complexity: vec![fallow_types::extract::FunctionComplexity {
            name: "doStuff".into(),
            is_private_member: false,
            context_named: None,
            line: 1,
            col: 0,
            cyclomatic: 7,
            cognitive: 4,
            line_count: 5,
            param_count: 0,
            react_hook_count: 0,
            react_jsx_max_depth: 0,
            react_prop_count: 0,
            source_hash: None,
            contributions: Vec::new(),
        }],
        ..crate::source::ModuleInfo::empty(crate::discover::FileId(0))
    };

    let (cyc, cog, funcs, lines) = aggregate_complexity(&module);
    assert_eq!(cyc, 7);
    assert_eq!(cog, 4);
    assert_eq!(funcs, 1);
    assert_eq!(lines, 5);
}

#[test]
fn aggregate_complexity_multiple_functions() {
    let module = crate::source::ModuleInfo {
        line_offsets: vec![0, 10, 20], // 3 lines
        complexity: vec![
            fallow_types::extract::FunctionComplexity {
                name: "a".into(),
                is_private_member: false,
                context_named: None,
                line: 1,
                col: 0,
                cyclomatic: 3,
                cognitive: 2,
                line_count: 1,
                param_count: 0,
                react_hook_count: 0,
                react_jsx_max_depth: 0,
                react_prop_count: 0,
                source_hash: None,
                contributions: Vec::new(),
            },
            fallow_types::extract::FunctionComplexity {
                name: "b".into(),
                is_private_member: false,
                context_named: None,
                line: 2,
                col: 0,
                cyclomatic: 5,
                cognitive: 8,
                line_count: 2,
                param_count: 0,
                react_hook_count: 0,
                react_jsx_max_depth: 0,
                react_prop_count: 0,
                source_hash: None,
                contributions: Vec::new(),
            },
        ],
        ..crate::source::ModuleInfo::empty(crate::discover::FileId(0))
    };

    let (cyc, cog, funcs, lines) = aggregate_complexity(&module);
    assert_eq!(cyc, 8);
    assert_eq!(cog, 10);
    assert_eq!(funcs, 2);
    assert_eq!(lines, 3);
}

#[test]
fn count_unused_exports_empty() {
    let exports: Vec<crate::results::UnusedExportFinding> = vec![];
    let map = count_unused_exports_by_path(&exports);
    assert!(map.is_empty());
}

#[test]
fn count_unused_exports_groups_by_path() {
    let exports = vec![
        crate::results::UnusedExportFinding::with_actions(crate::results::UnusedExport {
            path: std::path::PathBuf::from("/src/a.ts"),
            export_name: "foo".into(),
            is_type_only: false,
            line: 1,
            col: 0,
            span_start: 0,
            is_re_export: false,
            deprecated: false,
            deprecated_reason: None,
        }),
        crate::results::UnusedExportFinding::with_actions(crate::results::UnusedExport {
            path: std::path::PathBuf::from("/src/a.ts"),
            export_name: "bar".into(),
            is_type_only: false,
            line: 5,
            col: 0,
            span_start: 40,
            is_re_export: false,
            deprecated: false,
            deprecated_reason: None,
        }),
        crate::results::UnusedExportFinding::with_actions(crate::results::UnusedExport {
            path: std::path::PathBuf::from("/src/b.ts"),
            export_name: "baz".into(),
            is_type_only: false,
            line: 1,
            col: 0,
            span_start: 0,
            is_re_export: false,
            deprecated: false,
            deprecated_reason: None,
        }),
    ];
    let map = count_unused_exports_by_path(&exports);
    assert_eq!(map.get(std::path::Path::new("/src/a.ts")).copied(), Some(2));
    assert_eq!(map.get(std::path::Path::new("/src/b.ts")).copied(), Some(1));
}

#[test]
fn dead_code_ratio_all_value_exports_unused() {
    let unused_files = rustc_hash::FxHashSet::default();
    let path = std::path::Path::new("/src/foo.ts");

    let exports = vec![
        fallow_graph::graph::ExportSymbol {
            name: crate::source::ExportName::Named("a".into()),
            is_type_only: false,
            is_side_effect_used: false,
            visibility: crate::source::VisibilityTag::None,
            expected_unused_reason: None,
            span: oxc_span::Span::empty(0),
            references: vec![],
            reference_paths: Vec::new(),
            members: vec![],
            deprecated: false,
            deprecated_reason: None,
        },
        fallow_graph::graph::ExportSymbol {
            name: crate::source::ExportName::Named("b".into()),
            is_type_only: false,
            is_side_effect_used: false,
            visibility: crate::source::VisibilityTag::None,
            expected_unused_reason: None,
            span: oxc_span::Span::empty(0),
            references: vec![],
            reference_paths: Vec::new(),
            members: vec![],
            deprecated: false,
            deprecated_reason: None,
        },
    ];

    let mut unused_map: rustc_hash::FxHashMap<&std::path::Path, usize> =
        rustc_hash::FxHashMap::default();
    unused_map.insert(path, 2);

    let ratio = compute_dead_code_ratio(path, &exports, &unused_files, &unused_map);
    assert!((ratio - 1.0).abs() < f64::EPSILON);
}

#[test]
fn dead_code_ratio_clamped_when_unused_exceeds_value_exports() {
    let unused_files = rustc_hash::FxHashSet::default();
    let path = std::path::Path::new("/src/foo.ts");

    let exports = vec![fallow_graph::graph::ExportSymbol {
        name: crate::source::ExportName::Named("a".into()),
        is_type_only: false,
        is_side_effect_used: false,
        visibility: crate::source::VisibilityTag::None,
        expected_unused_reason: None,
        span: oxc_span::Span::empty(0),
        references: vec![],
        reference_paths: Vec::new(),
        members: vec![],
        deprecated: false,
        deprecated_reason: None,
    }];

    let mut unused_map: rustc_hash::FxHashMap<&std::path::Path, usize> =
        rustc_hash::FxHashMap::default();
    unused_map.insert(path, 5);

    let ratio = compute_dead_code_ratio(path, &exports, &unused_files, &unused_map);
    assert!((ratio - 1.0).abs() < f64::EPSILON);
}

#[test]
fn dead_code_ratio_no_unused_exports_for_path() {
    let unused_files = rustc_hash::FxHashSet::default();
    let path = std::path::Path::new("/src/clean.ts");

    let exports = vec![fallow_graph::graph::ExportSymbol {
        name: crate::source::ExportName::Named("used".into()),
        is_type_only: false,
        is_side_effect_used: false,
        visibility: crate::source::VisibilityTag::None,
        expected_unused_reason: None,
        span: oxc_span::Span::empty(0),
        references: vec![],
        reference_paths: Vec::new(),
        members: vec![],
        deprecated: false,
        deprecated_reason: None,
    }];

    let unused_map = rustc_hash::FxHashMap::default();
    let ratio = compute_dead_code_ratio(path, &exports, &unused_files, &unused_map);
    assert!(ratio.abs() < f64::EPSILON);
}

#[test]
fn complexity_density_zero_cyclomatic_with_lines() {
    let result = compute_complexity_density(0, 100);
    assert!(result.abs() < f64::EPSILON);
}

#[test]
fn complexity_density_single_line() {
    let result = compute_complexity_density(1, 1);
    assert!((result - 1.0).abs() < f64::EPSILON);
}

#[test]
fn maintainability_only_complexity_penalty() {
    let result = compute_maintainability_index(3.0, 0.0, 0, 100);
    assert!((result - 10.0).abs() < f64::EPSILON);
}

#[test]
fn maintainability_only_dead_code_penalty() {
    let result = compute_maintainability_index(0.0, 0.5, 0, 100);
    assert!((result - 90.0).abs() < f64::EPSILON);
}

#[test]
fn maintainability_fan_out_one() {
    let result = compute_maintainability_index(0.0, 0.0, 1, 100);
    let expected = 2.0_f64.ln().mul_add(-4.0, 100.0);
    assert!((result - expected).abs() < 0.01);
}

#[test]
fn maintainability_all_penalties_maxed() {
    let result = compute_maintainability_index(10.0, 1.0, 1000, 200);
    assert!(result.abs() < f64::EPSILON);
}

#[test]
fn count_unused_exports_single_file_single_export() {
    let exports = vec![crate::results::UnusedExportFinding::with_actions(
        crate::results::UnusedExport {
            path: std::path::PathBuf::from("/src/only.ts"),
            export_name: "lonely".into(),
            is_type_only: false,
            line: 1,
            col: 0,
            span_start: 0,
            is_re_export: false,
            deprecated: false,
            deprecated_reason: None,
        },
    )];
    let map = count_unused_exports_by_path(&exports);
    assert_eq!(map.len(), 1);
    assert_eq!(
        map.get(std::path::Path::new("/src/only.ts")).copied(),
        Some(1)
    );
}

/// Helper to build a minimal `ModuleGraph` from scratch.
fn build_test_graph(
    files: &[crate::discover::DiscoveredFile],
    entry_point_paths: &[std::path::PathBuf],
    resolved_modules: &[fallow_graph::resolve::ResolvedModule],
) -> fallow_graph::graph::ModuleGraph {
    let entry_points: Vec<crate::discover::EntryPoint> = entry_point_paths
        .iter()
        .map(|p| crate::discover::EntryPoint {
            path: p.clone(),
            source: crate::discover::EntryPointSource::PackageJsonMain,
        })
        .collect();
    fallow_graph::graph::ModuleGraph::build(resolved_modules, &entry_points, files)
}

/// Helper to create a `ModuleInfo` with given complexity and line count.
fn make_module_info(
    file_id: u32,
    line_count: usize,
    functions: Vec<fallow_types::extract::FunctionComplexity>,
) -> crate::source::ModuleInfo {
    crate::source::ModuleInfo {
        line_offsets: (0..line_count).map(|i| (i * 10) as u32).collect(),
        complexity: functions,
        ..crate::source::ModuleInfo::empty(crate::discover::FileId(file_id))
    }
}

fn make_file_score(path: &str, maintainability_index: f64, crap_max: f64) -> FileHealthScore {
    FileHealthScore {
        path: std::path::PathBuf::from(path),
        fan_in: 0,
        fan_out: 0,
        dead_code_ratio: 0.0,
        complexity_density: 0.0,
        maintainability_index,
        total_cyclomatic: 0,
        total_cognitive: 0,
        function_count: 1,
        lines: 1,
        crap_max,
        crap_above_threshold: usize::from(crap_max >= CRAP_THRESHOLD),
        crap_exempted: 0,
        crap_suppressed: 0,
        crap_unsuppressed_max: None,
        crap_unsuppressed_effective_threshold: None,
        crap_effective_threshold: None,
    }
}

#[test]
fn suppressed_crap_keeps_unsuppressed_bands_and_legacy_fallback() {
    for (remaining, threshold, above, exempted, global, expected) in [
        (20.0, 30.0, 0, 1, 30.0, 55.0),
        (20.0, 30.0, 0, 0, 30.0, 55.0),
        (0.0, 30.0, 0, 0, 30.0, 0.0),
        (110.0, 500.0, 0, 1, 30.0, 0.0),
        (42.0, 0.0, 0, 2, 0.0, 0.0),
        (20.0, 5.0, 1, 0, 5.0, 100.0),
    ] {
        let mut score = make_file_score("src/legacy.ts", 100.0, 210.0);
        score.crap_suppressed = 1;
        score.crap_above_threshold = above;
        score.crap_exempted = exempted;
        score.crap_effective_threshold = Some(5.0);
        score.crap_unsuppressed_max = Some(remaining);
        score.crap_unsuppressed_effective_threshold = Some(threshold);
        let restored: FileHealthScore =
            serde_json::from_value(serde_json::to_value(&score).unwrap()).unwrap();
        assert!((file_score_crap_concern(&restored, global) - expected).abs() < f64::EPSILON);
        assert_eq!(restored.crap_max, 210.0);
        assert_eq!(restored.crap_effective_threshold, Some(5.0));
        assert_eq!(restored.crap_exempted, exempted);
    }
    let mut legacy = make_file_score("src/legacy.ts", 100.0, 20.0);
    legacy.crap_effective_threshold = Some(10.0);
    for (maximum, threshold) in [(None, None), (Some(0.0), None), (None, Some(500.0))] {
        legacy.crap_unsuppressed_max = maximum;
        legacy.crap_unsuppressed_effective_threshold = threshold;
        let restored: FileHealthScore =
            serde_json::from_value(serde_json::to_value(&legacy).unwrap()).unwrap();
        let signal = file_score_crap_signal(&restored, 30.0);
        assert_eq!(signal.maximum, 20.0);
        assert_eq!(signal.threshold, 10.0);
        assert_eq!(signal.metric(), "crap_max");
        assert!((file_score_crap_concern(&restored, 30.0) - 85.714_285_714_285_71).abs() < 1e-10);
    }
}

#[test]
fn suppressed_crap_maximum_does_not_win_the_triage_tie_break() {
    let mut suppressed = make_file_score("src/a.ts", 0.0, 210.0);
    suppressed.crap_suppressed = 1;
    suppressed.crap_unsuppressed_max = Some(42.0);
    suppressed.crap_unsuppressed_effective_threshold = Some(30.0);
    let other = make_file_score("src/b.ts", 0.0, 50.0);
    assert_eq!(
        compare_file_score_triage(&suppressed, &other, 30.0),
        std::cmp::Ordering::Greater
    );
    suppressed.crap_unsuppressed_max = Some(60.0);
    assert_eq!(
        compare_file_score_triage(&suppressed, &other, 30.0),
        std::cmp::Ordering::Less
    );
}

#[test]
fn inherited_crap_suppression_keeps_raw_function_measurements() {
    let mut module = make_module_info(
        0,
        30,
        vec![
            make_named_fn_complexity("suppressed", 1, 14),
            make_named_fn_complexity("remaining", 20, 6),
        ],
    );
    let resolver = test_crap_resolver(30.0);
    let context = TemplateInheritContext {
        is_test_reachable: false,
        test_referenced_exports: rustc_hash::FxHashSet::default(),
        provenance_owner: std::path::PathBuf::from("src/owner.ts"),
    };
    for (suppressed, expected_above) in [(false, 2), (true, 1)] {
        module.suppressions = if suppressed {
            vec![crate::suppress::Suppression::issue(
                1,
                1,
                crate::suppress::IssueKind::Complexity,
            )]
        } else {
            Vec::new()
        };
        let lookup = CrapCeilingLookup::new(
            CrapScoreThresholds {
                resolver: &resolver,
                enforce_crap: true,
            },
            std::path::Path::new("src/template.html"),
            &module.suppressions,
        );
        let result = compute_template_inherited_crap(&module, &context, &lookup);
        assert_eq!(result.max, 210.0);
        assert_eq!(result.signals.above, expected_above);
        assert_eq!(result.signals.suppressed, usize::from(suppressed));
        assert_eq!(
            result.signals.unsuppressed_max,
            if suppressed { 42.0 } else { 210.0 }
        );
        let raw = result
            .per_function
            .iter()
            .map(|function| (function.line, function.crap))
            .collect::<Vec<_>>();
        assert_eq!(raw, [(1, 210.0), (20, 42.0)]);
        assert!(
            result
                .per_function
                .iter()
                .all(|function| function.coverage_source
                    == fallow_output::CoverageSource::EstimatedComponentInherited)
        );
    }
}

fn crap_concern_at_default(crap_max: f64) -> f64 {
    file_score_crap_concern(
        &make_file_score("/src/concern.ts", 100.0, crap_max),
        CRAP_THRESHOLD,
    )
}

#[test]
fn file_score_crap_concern_tracks_crap_risk_bands() {
    assert!((crap_concern_at_default(0.0) - 0.0).abs() < f64::EPSILON);
    assert!((crap_concern_at_default(15.0) - 45.0).abs() < f64::EPSILON);
    assert!((crap_concern_at_default(CRAP_THRESHOLD) - 75.0).abs() < f64::EPSILON);
    assert!((crap_concern_at_default(100.0) - 100.0).abs() < f64::EPSILON);
    assert!((crap_concern_at_default(552.0) - 100.0).abs() < f64::EPSILON);
}

#[test]
fn file_score_crap_concern_generalizes_bands_over_effective_ceiling() {
    // At t = 500 the band edges scale to (250, 500, 1666.7): a breaching
    // 250 sits at the moderate/high edge and a breaching 500 at high.
    let mut at_edge = make_file_score("/src/edge.ts", 100.0, 250.0);
    at_edge.crap_above_threshold = 1;
    at_edge.crap_effective_threshold = Some(500.0);
    assert!((file_score_crap_concern(&at_edge, CRAP_THRESHOLD) - 45.0).abs() < f64::EPSILON);

    let mut at_ceiling = make_file_score("/src/ceiling.ts", 100.0, 500.0);
    at_ceiling.crap_above_threshold = 1;
    at_ceiling.crap_effective_threshold = Some(500.0);
    assert!((file_score_crap_concern(&at_ceiling, CRAP_THRESHOLD) - 75.0).abs() < f64::EPSILON);
}

#[test]
fn file_score_crap_concern_zeroes_fully_exempt_file() {
    // The issue's own numbers: crap_max 110 under ceiling 500 with both
    // breaches exempted. Band generalization alone would yield 19.8 and
    // keep the row above a structural concern of 12; the fully-exempt rule
    // must zero it (issue #2228).
    let mut exempt = make_file_score("/src/legacy.ts", 88.0, 110.0);
    exempt.crap_above_threshold = 0;
    exempt.crap_exempted = 2;
    exempt.crap_effective_threshold = Some(500.0);
    assert!((file_score_crap_concern(&exempt, CRAP_THRESHOLD) - 0.0).abs() < f64::EPSILON);
    assert!(file_score_fully_crap_exempt(&exempt, CRAP_THRESHOLD));
    assert_eq!(
        file_score_concern_axis(&exempt, CRAP_THRESHOLD),
        FileScoreConcern::Structural
    );
}

#[test]
fn file_score_crap_concern_zeroes_when_enforcement_disabled() {
    let mut score = make_file_score("/src/any.ts", 88.0, 110.0);
    score.crap_above_threshold = 0;
    score.crap_exempted = 2;
    assert!((file_score_crap_concern(&score, 0.0) - 0.0).abs() < f64::EPSILON);
    assert!(file_score_fully_crap_exempt(&score, 0.0));
    assert_eq!(
        file_score_concern_axis(&score, 0.0),
        FileScoreConcern::Structural
    );
}

#[test]
fn file_score_partial_exemption_keeps_risk_axis() {
    // One breaching function survives its ceiling: the row is NOT fully
    // exempt, so the risk story stays.
    let mut mixed = make_file_score("/src/mixed.ts", 88.0, 110.0);
    mixed.crap_above_threshold = 1;
    mixed.crap_exempted = 1;
    mixed.crap_effective_threshold = Some(30.0);
    assert!(!file_score_fully_crap_exempt(&mixed, CRAP_THRESHOLD));
    assert_eq!(
        file_score_concern_axis(&mixed, CRAP_THRESHOLD),
        FileScoreConcern::Risk
    );
}

#[test]
fn file_score_concern_axis_labels_dominant_signal() {
    let risk_driven = make_file_score("/src/risk.ts", 84.8, 552.0);
    assert_eq!(
        file_score_concern_axis(&risk_driven, CRAP_THRESHOLD),
        FileScoreConcern::Risk
    );
    assert_eq!(
        file_score_concern_axis(&risk_driven, CRAP_THRESHOLD).label(),
        "risk"
    );

    let structure_driven = make_file_score("/src/structure.ts", 30.0, 8.0);
    assert_eq!(
        file_score_concern_axis(&structure_driven, CRAP_THRESHOLD),
        FileScoreConcern::Structural
    );
    assert_eq!(
        file_score_concern_axis(&structure_driven, CRAP_THRESHOLD).label(),
        "structure"
    );

    let no_risk = make_file_score("/src/clean.ts", 100.0, 0.0);
    assert_eq!(
        file_score_concern_axis(&no_risk, CRAP_THRESHOLD),
        FileScoreConcern::Structural
    );
}

#[test]
fn file_score_triage_sort_prioritizes_high_crap_over_slightly_lower_mi() {
    let low_mi_low_risk = make_file_score("/src/low-mi-low-risk.ts", 81.7, 2.0);
    let higher_mi_high_risk = make_file_score("/src/higher-mi-high-risk.ts", 84.8, 552.0);

    let mut scores = [low_mi_low_risk, higher_mi_high_risk];
    scores.sort_by(|a, b| compare_file_score_triage(a, b, CRAP_THRESHOLD));

    assert_eq!(
        scores[0].path,
        std::path::Path::new("/src/higher-mi-high-risk.ts")
    );
    assert_eq!(
        scores[1].path,
        std::path::Path::new("/src/low-mi-low-risk.ts")
    );
}

#[test]
fn file_score_triage_sort_orders_saturated_crap_by_raw_crap_descending() {
    let lower_crap_worse_mi = make_file_score("/src/a.ts", 84.8, 106.0);
    let higher_crap_better_mi = make_file_score("/src/b.ts", 96.7, 552.0);

    let mut scores = [lower_crap_worse_mi, higher_crap_better_mi];
    scores.sort_by(|a, b| compare_file_score_triage(a, b, CRAP_THRESHOLD));

    assert_eq!(scores[0].path, std::path::Path::new("/src/b.ts"));
    assert_eq!(scores[1].path, std::path::Path::new("/src/a.ts"));
}

#[test]
fn file_score_triage_sort_uses_mi_crap_and_path_tie_breakers() {
    let mut scores = [
        make_file_score("/src/b.ts", 70.0, 1.0),
        make_file_score("/src/a.ts", 70.0, 1.0),
        make_file_score("/src/higher-crap.ts", 70.0, 2.0),
        make_file_score("/src/lower-concern.ts", 80.0, 1.0),
    ];

    scores.sort_by(|a, b| compare_file_score_triage(a, b, CRAP_THRESHOLD));

    let paths: Vec<_> = scores.iter().map(|score| score.path.as_path()).collect();
    assert_eq!(
        paths,
        vec![
            std::path::Path::new("/src/higher-crap.ts"),
            std::path::Path::new("/src/a.ts"),
            std::path::Path::new("/src/b.ts"),
            std::path::Path::new("/src/lower-concern.ts"),
        ]
    );
}

#[test]
fn compute_file_scores_empty_graph() {
    let files: Vec<crate::discover::DiscoveredFile> = vec![];
    let graph = build_test_graph(&files, &[], &[]);
    let modules: Vec<crate::source::ModuleInfo> = vec![];
    let file_paths = rustc_hash::FxHashMap::default();

    let output = crate::results::DeadCodeAnalysisArtifacts {
        results: fallow_types::results::AnalysisResults::default(),
        timings: None,
        graph: Some(crate::module_graph::RetainedModuleGraph::from(graph)),
        modules: None,
        files: None,
        script_used_packages: rustc_hash::FxHashSet::default(),
        trace_provenance: crate::trace::TraceProvenance::default(),
        file_hashes: rustc_hash::FxHashMap::default(),
    };

    let result = compute_file_scores_default(
        &modules,
        &file_paths,
        None,
        output,
        None,
        std::path::Path::new("/project"),
    )
    .unwrap();
    assert!(result.scores.is_empty());
    assert!(result.circular_files.is_empty());
    assert!(result.top_complex_fns.is_empty());
    assert!(result.entry_points.is_empty());
    assert_eq!(result.analysis_counts.total_exports, 0);
    assert_eq!(result.analysis_counts.dead_files, 0);
}

#[test]
fn compute_file_scores_no_graph_returns_error() {
    let modules: Vec<crate::source::ModuleInfo> = vec![];
    let file_paths = rustc_hash::FxHashMap::default();

    let output = crate::results::DeadCodeAnalysisArtifacts {
        results: fallow_types::results::AnalysisResults::default(),
        timings: None,
        graph: None,
        modules: None,
        files: None,
        script_used_packages: rustc_hash::FxHashSet::default(),
        trace_provenance: crate::trace::TraceProvenance::default(),
        file_hashes: rustc_hash::FxHashMap::default(),
    };

    let result = compute_file_scores_default(
        &modules,
        &file_paths,
        None,
        output,
        None,
        std::path::Path::new("/project"),
    );
    assert!(result.is_err());
    match result {
        Err(msg) => assert_eq!(msg, "graph not available"),
        Ok(_) => panic!("expected error"),
    }
}

#[test]
fn compute_file_scores_single_file_with_function() {
    let path_a = std::path::PathBuf::from("/src/a.ts");
    let files = vec![crate::discover::DiscoveredFile {
        id: crate::discover::FileId(0),
        path: path_a.clone(),
        size_bytes: 100,
    }];

    let resolved_modules = vec![fallow_graph::resolve::ResolvedModule {
        file_id: crate::discover::FileId(0),
        path: path_a.clone(),
        exports: vec![fallow_types::extract::ExportInfo {
            name: crate::source::ExportName::Named("foo".into()),
            local_name: None,
            is_type_only: false,
            visibility: crate::source::VisibilityTag::None,
            expected_unused_reason: None,
            span: oxc_span::Span::empty(0),
            members: vec![],
            is_side_effect_used: false,
            super_class: None,
            deprecated: false,
            deprecated_reason: None,
        }]
        .into(),
        ..Default::default()
    }];

    let graph = build_test_graph(&files, std::slice::from_ref(&path_a), &resolved_modules);

    let modules = vec![make_module_info(
        0,
        10,
        vec![fallow_types::extract::FunctionComplexity {
            name: "foo".into(),
            is_private_member: false,
            context_named: None,
            line: 1,
            col: 0,
            cyclomatic: 5,
            cognitive: 3,
            line_count: 10,
            param_count: 0,
            react_hook_count: 0,
            react_jsx_max_depth: 0,
            react_prop_count: 0,
            source_hash: None,
            contributions: Vec::new(),
        }],
    )];

    let mut file_paths: rustc_hash::FxHashMap<crate::discover::FileId, &std::path::PathBuf> =
        rustc_hash::FxHashMap::default();
    file_paths.insert(crate::discover::FileId(0), &files[0].path);

    let output = crate::results::DeadCodeAnalysisArtifacts {
        results: fallow_types::results::AnalysisResults::default(),
        timings: None,
        graph: Some(crate::module_graph::RetainedModuleGraph::from(graph)),
        modules: None,
        files: None,
        script_used_packages: rustc_hash::FxHashSet::default(),
        trace_provenance: crate::trace::TraceProvenance::default(),
        file_hashes: rustc_hash::FxHashMap::default(),
    };

    let result = compute_file_scores_default(
        &modules,
        &file_paths,
        None,
        output,
        None,
        std::path::Path::new("/project"),
    )
    .unwrap();
    assert_eq!(result.scores.len(), 1);

    let score = &result.scores[0];
    assert_eq!(score.path, path_a);
    assert_eq!(score.total_cyclomatic, 5);
    assert_eq!(score.total_cognitive, 3);
    assert_eq!(score.function_count, 1);
    assert_eq!(score.lines, 10);
    assert!((score.complexity_density - 0.5).abs() < f64::EPSILON);
    assert!(score.dead_code_ratio.abs() < f64::EPSILON);
    assert!(result.entry_points.contains(&path_a));
}

#[test]
fn compute_file_scores_excludes_barrel_files() {
    let path_a = std::path::PathBuf::from("/src/index.ts");
    let files = vec![crate::discover::DiscoveredFile {
        id: crate::discover::FileId(0),
        path: path_a.clone(),
        size_bytes: 50,
    }];

    let resolved_modules = vec![fallow_graph::resolve::ResolvedModule {
        file_id: crate::discover::FileId(0),
        path: path_a.clone(),
        ..Default::default()
    }];

    let graph = build_test_graph(&files, std::slice::from_ref(&path_a), &resolved_modules);

    let modules = vec![make_module_info(0, 5, vec![])];

    let mut file_paths: rustc_hash::FxHashMap<crate::discover::FileId, &std::path::PathBuf> =
        rustc_hash::FxHashMap::default();
    file_paths.insert(crate::discover::FileId(0), &files[0].path);

    let output = crate::results::DeadCodeAnalysisArtifacts {
        results: fallow_types::results::AnalysisResults::default(),
        timings: None,
        graph: Some(crate::module_graph::RetainedModuleGraph::from(graph)),
        modules: None,
        files: None,
        script_used_packages: rustc_hash::FxHashSet::default(),
        trace_provenance: crate::trace::TraceProvenance::default(),
        file_hashes: rustc_hash::FxHashMap::default(),
    };

    let result = compute_file_scores_default(
        &modules,
        &file_paths,
        None,
        output,
        None,
        std::path::Path::new("/project"),
    )
    .unwrap();
    assert!(result.scores.is_empty());
}

#[test]
fn compute_file_scores_changed_since_filter() {
    let path_a = std::path::PathBuf::from("/src/a.ts");
    let path_b = std::path::PathBuf::from("/src/b.ts");
    let files = vec![
        crate::discover::DiscoveredFile {
            id: crate::discover::FileId(0),
            path: path_a.clone(),
            size_bytes: 100,
        },
        crate::discover::DiscoveredFile {
            id: crate::discover::FileId(1),
            path: path_b.clone(),
            size_bytes: 100,
        },
    ];

    let resolved_modules = vec![
        fallow_graph::resolve::ResolvedModule {
            file_id: crate::discover::FileId(0),
            path: path_a,
            ..Default::default()
        },
        fallow_graph::resolve::ResolvedModule {
            file_id: crate::discover::FileId(1),
            path: path_b.clone(),
            ..Default::default()
        },
    ];

    let graph = build_test_graph(&files, &[], &resolved_modules);

    let modules = vec![
        make_module_info(
            0,
            10,
            vec![fallow_types::extract::FunctionComplexity {
                name: "fn_a".into(),
                is_private_member: false,
                context_named: None,
                line: 1,
                col: 0,
                cyclomatic: 2,
                cognitive: 1,
                line_count: 10,
                param_count: 0,
                react_hook_count: 0,
                react_jsx_max_depth: 0,
                react_prop_count: 0,
                source_hash: None,
                contributions: Vec::new(),
            }],
        ),
        make_module_info(
            1,
            10,
            vec![fallow_types::extract::FunctionComplexity {
                name: "fn_b".into(),
                is_private_member: false,
                context_named: None,
                line: 1,
                col: 0,
                cyclomatic: 3,
                cognitive: 2,
                line_count: 10,
                param_count: 0,
                react_hook_count: 0,
                react_jsx_max_depth: 0,
                react_prop_count: 0,
                source_hash: None,
                contributions: Vec::new(),
            }],
        ),
    ];

    let mut file_paths: rustc_hash::FxHashMap<crate::discover::FileId, &std::path::PathBuf> =
        rustc_hash::FxHashMap::default();
    file_paths.insert(crate::discover::FileId(0), &files[0].path);
    file_paths.insert(crate::discover::FileId(1), &files[1].path);

    let path_b_check = std::path::PathBuf::from("/src/b.ts");
    let mut changed = rustc_hash::FxHashSet::default();
    changed.insert(path_b);

    let output = crate::results::DeadCodeAnalysisArtifacts {
        results: fallow_types::results::AnalysisResults::default(),
        timings: None,
        graph: Some(crate::module_graph::RetainedModuleGraph::from(graph)),
        modules: None,
        files: None,
        script_used_packages: rustc_hash::FxHashSet::default(),
        trace_provenance: crate::trace::TraceProvenance::default(),
        file_hashes: rustc_hash::FxHashMap::default(),
    };

    let result = compute_file_scores_default(
        &modules,
        &file_paths,
        Some(&changed),
        output,
        None,
        std::path::Path::new("/project"),
    )
    .unwrap();
    assert_eq!(result.scores.len(), 1);
    assert_eq!(result.scores[0].path, path_b_check);
}

#[test]
fn compute_file_scores_sorted_by_triage_concern() {
    let path_a = std::path::PathBuf::from("/src/a.ts");
    let path_b = std::path::PathBuf::from("/src/b.ts");
    let files = vec![
        crate::discover::DiscoveredFile {
            id: crate::discover::FileId(0),
            path: path_a.clone(),
            size_bytes: 100,
        },
        crate::discover::DiscoveredFile {
            id: crate::discover::FileId(1),
            path: path_b.clone(),
            size_bytes: 100,
        },
    ];

    let resolved_modules = vec![
        fallow_graph::resolve::ResolvedModule {
            file_id: crate::discover::FileId(0),
            path: path_a.clone(),
            ..Default::default()
        },
        fallow_graph::resolve::ResolvedModule {
            file_id: crate::discover::FileId(1),
            path: path_b,
            ..Default::default()
        },
    ];

    let graph = build_test_graph(&files, &[], &resolved_modules);

    let modules = vec![
        make_module_info(
            0,
            10,
            vec![fallow_types::extract::FunctionComplexity {
                name: "complex_fn".into(),
                is_private_member: false,
                context_named: None,
                line: 1,
                col: 0,
                cyclomatic: 30,
                cognitive: 20,
                line_count: 10,
                param_count: 0,
                react_hook_count: 0,
                react_jsx_max_depth: 0,
                react_prop_count: 0,
                source_hash: None,
                contributions: Vec::new(),
            }],
        ),
        make_module_info(
            1,
            100,
            vec![fallow_types::extract::FunctionComplexity {
                name: "simple_fn".into(),
                is_private_member: false,
                context_named: None,
                line: 1,
                col: 0,
                cyclomatic: 1,
                cognitive: 0,
                line_count: 100,
                param_count: 0,
                react_hook_count: 0,
                react_jsx_max_depth: 0,
                react_prop_count: 0,
                source_hash: None,
                contributions: Vec::new(),
            }],
        ),
    ];

    let mut file_paths: rustc_hash::FxHashMap<crate::discover::FileId, &std::path::PathBuf> =
        rustc_hash::FxHashMap::default();
    file_paths.insert(crate::discover::FileId(0), &files[0].path);
    file_paths.insert(crate::discover::FileId(1), &files[1].path);

    let output = crate::results::DeadCodeAnalysisArtifacts {
        results: fallow_types::results::AnalysisResults::default(),
        timings: None,
        graph: Some(crate::module_graph::RetainedModuleGraph::from(graph)),
        modules: None,
        files: None,
        script_used_packages: rustc_hash::FxHashSet::default(),
        trace_provenance: crate::trace::TraceProvenance::default(),
        file_hashes: rustc_hash::FxHashMap::default(),
    };

    let result = compute_file_scores_default(
        &modules,
        &file_paths,
        None,
        output,
        None,
        std::path::Path::new("/project"),
    )
    .unwrap();
    assert_eq!(result.scores.len(), 2);
    assert!(result.scores[0].maintainability_index <= result.scores[1].maintainability_index);
    assert_eq!(result.scores[0].path, path_a);
}

#[test]
fn compute_file_scores_with_unused_file_populates_evidence() {
    let path_a = std::path::PathBuf::from("/src/unused.ts");
    let files = vec![crate::discover::DiscoveredFile {
        id: crate::discover::FileId(0),
        path: path_a.clone(),
        size_bytes: 100,
    }];

    let resolved_modules = vec![fallow_graph::resolve::ResolvedModule {
        file_id: crate::discover::FileId(0),
        path: path_a.clone(),
        exports: vec![fallow_types::extract::ExportInfo {
            name: crate::source::ExportName::Named("orphan".into()),
            local_name: None,
            is_type_only: false,
            visibility: crate::source::VisibilityTag::None,
            expected_unused_reason: None,
            span: oxc_span::Span::empty(0),
            members: vec![],
            is_side_effect_used: false,
            super_class: None,
            deprecated: false,
            deprecated_reason: None,
        }]
        .into(),
        ..Default::default()
    }];

    let graph = build_test_graph(&files, &[], &resolved_modules);

    let modules = vec![make_module_info(
        0,
        10,
        vec![fallow_types::extract::FunctionComplexity {
            name: "orphan".into(),
            is_private_member: false,
            context_named: None,
            line: 1,
            col: 0,
            cyclomatic: 1,
            cognitive: 0,
            line_count: 10,
            param_count: 0,
            react_hook_count: 0,
            react_jsx_max_depth: 0,
            react_prop_count: 0,
            source_hash: None,
            contributions: Vec::new(),
        }],
    )];

    let mut file_paths: rustc_hash::FxHashMap<crate::discover::FileId, &std::path::PathBuf> =
        rustc_hash::FxHashMap::default();
    file_paths.insert(crate::discover::FileId(0), &files[0].path);

    let mut results = fallow_types::results::AnalysisResults::default();
    results.unused_files.push(
        fallow_types::output_dead_code::UnusedFileFinding::with_actions(
            fallow_types::results::UnusedFile {
                path: path_a.clone(),
            },
        ),
    );

    let output = crate::results::DeadCodeAnalysisArtifacts {
        results,
        timings: None,
        graph: Some(crate::module_graph::RetainedModuleGraph::from(graph)),
        modules: None,
        files: None,
        script_used_packages: rustc_hash::FxHashSet::default(),
        trace_provenance: crate::trace::TraceProvenance::default(),
        file_hashes: rustc_hash::FxHashMap::default(),
    };

    let result = compute_file_scores_default(
        &modules,
        &file_paths,
        None,
        output,
        None,
        std::path::Path::new("/project"),
    )
    .unwrap();
    assert_eq!(result.scores.len(), 1);
    assert!((result.scores[0].dead_code_ratio - 1.0).abs() < f64::EPSILON);
    assert!(result.unused_export_names.contains_key(&path_a));
    let names = &result.unused_export_names[&path_a];
    assert_eq!(names, &["orphan"]);
    assert_eq!(result.analysis_counts.dead_files, 1);
}

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "test fixture; linear setup/assert, length is not a maintainability concern"
)]
fn compute_file_scores_tracks_top_complex_functions() {
    let path_a = std::path::PathBuf::from("/src/complex.ts");
    let files = vec![crate::discover::DiscoveredFile {
        id: crate::discover::FileId(0),
        path: path_a.clone(),
        size_bytes: 500,
    }];

    let resolved_modules = vec![fallow_graph::resolve::ResolvedModule {
        file_id: crate::discover::FileId(0),
        path: path_a.clone(),
        ..Default::default()
    }];

    let graph = build_test_graph(&files, &[], &resolved_modules);

    let modules = vec![make_module_info(
        0,
        50,
        vec![
            fallow_types::extract::FunctionComplexity {
                name: "high".into(),
                is_private_member: false,
                context_named: None,
                line: 1,
                col: 0,
                cyclomatic: 10,
                cognitive: 20,
                line_count: 10,
                param_count: 0,
                react_hook_count: 0,
                react_jsx_max_depth: 0,
                react_prop_count: 0,
                source_hash: None,
                contributions: Vec::new(),
            },
            fallow_types::extract::FunctionComplexity {
                name: "medium".into(),
                is_private_member: false,
                context_named: None,
                line: 11,
                col: 0,
                cyclomatic: 5,
                cognitive: 10,
                line_count: 10,
                param_count: 0,
                react_hook_count: 0,
                react_jsx_max_depth: 0,
                react_prop_count: 0,
                source_hash: None,
                contributions: Vec::new(),
            },
            fallow_types::extract::FunctionComplexity {
                name: "low".into(),
                is_private_member: false,
                context_named: None,
                line: 21,
                col: 0,
                cyclomatic: 2,
                cognitive: 5,
                line_count: 10,
                param_count: 0,
                react_hook_count: 0,
                react_jsx_max_depth: 0,
                react_prop_count: 0,
                source_hash: None,
                contributions: Vec::new(),
            },
            fallow_types::extract::FunctionComplexity {
                name: "trivial".into(),
                is_private_member: false,
                context_named: None,
                line: 31,
                col: 0,
                cyclomatic: 1,
                cognitive: 1,
                line_count: 10,
                param_count: 0,
                react_hook_count: 0,
                react_jsx_max_depth: 0,
                react_prop_count: 0,
                source_hash: None,
                contributions: Vec::new(),
            },
        ],
    )];

    let mut file_paths: rustc_hash::FxHashMap<crate::discover::FileId, &std::path::PathBuf> =
        rustc_hash::FxHashMap::default();
    file_paths.insert(crate::discover::FileId(0), &files[0].path);

    let output = crate::results::DeadCodeAnalysisArtifacts {
        results: fallow_types::results::AnalysisResults::default(),
        timings: None,
        graph: Some(crate::module_graph::RetainedModuleGraph::from(graph)),
        modules: None,
        files: None,
        script_used_packages: rustc_hash::FxHashSet::default(),
        trace_provenance: crate::trace::TraceProvenance::default(),
        file_hashes: rustc_hash::FxHashMap::default(),
    };

    let result = compute_file_scores_default(
        &modules,
        &file_paths,
        None,
        output,
        None,
        std::path::Path::new("/project"),
    )
    .unwrap();
    assert!(result.top_complex_fns.contains_key(&path_a));
    let top = &result.top_complex_fns[&path_a];
    assert_eq!(top.len(), 3);
    assert_eq!(top[0].0, "high");
    assert_eq!(top[0].2, 20);
    assert_eq!(top[1].0, "medium");
    assert_eq!(top[1].2, 10);
    assert_eq!(top[2].0, "low");
    assert_eq!(top[2].2, 5);
}

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "test fixture; linear setup/assert, length is not a maintainability concern"
)]
fn compute_file_scores_with_circular_deps() {
    let path_a = std::path::PathBuf::from("/src/a.ts");
    let path_b = std::path::PathBuf::from("/src/b.ts");
    let files = vec![
        crate::discover::DiscoveredFile {
            id: crate::discover::FileId(0),
            path: path_a.clone(),
            size_bytes: 100,
        },
        crate::discover::DiscoveredFile {
            id: crate::discover::FileId(1),
            path: path_b.clone(),
            size_bytes: 100,
        },
    ];

    let resolved_modules = vec![
        fallow_graph::resolve::ResolvedModule {
            file_id: crate::discover::FileId(0),
            path: path_a.clone(),
            ..Default::default()
        },
        fallow_graph::resolve::ResolvedModule {
            file_id: crate::discover::FileId(1),
            path: path_b.clone(),
            ..Default::default()
        },
    ];

    let graph = build_test_graph(&files, &[], &resolved_modules);

    let modules = vec![
        make_module_info(
            0,
            10,
            vec![fallow_types::extract::FunctionComplexity {
                name: "fn_a".into(),
                is_private_member: false,
                context_named: None,
                line: 1,
                col: 0,
                cyclomatic: 2,
                cognitive: 1,
                line_count: 10,
                param_count: 0,
                react_hook_count: 0,
                react_jsx_max_depth: 0,
                react_prop_count: 0,
                source_hash: None,
                contributions: Vec::new(),
            }],
        ),
        make_module_info(
            1,
            10,
            vec![fallow_types::extract::FunctionComplexity {
                name: "fn_b".into(),
                is_private_member: false,
                context_named: None,
                line: 1,
                col: 0,
                cyclomatic: 3,
                cognitive: 2,
                line_count: 10,
                param_count: 0,
                react_hook_count: 0,
                react_jsx_max_depth: 0,
                react_prop_count: 0,
                source_hash: None,
                contributions: Vec::new(),
            }],
        ),
    ];

    let mut file_paths: rustc_hash::FxHashMap<crate::discover::FileId, &std::path::PathBuf> =
        rustc_hash::FxHashMap::default();
    file_paths.insert(crate::discover::FileId(0), &files[0].path);
    file_paths.insert(crate::discover::FileId(1), &files[1].path);

    let mut results = fallow_types::results::AnalysisResults::default();
    results.circular_dependencies.push(
        fallow_types::output_dead_code::CircularDependencyFinding::with_actions(
            fallow_types::results::CircularDependency {
                files: vec![path_a.clone(), path_b.clone()],
                length: 2,
                line: 1,
                col: 0,
                edges: Vec::new(),
                is_cross_package: false,
            },
        ),
    );

    let output = crate::results::DeadCodeAnalysisArtifacts {
        results,
        timings: None,
        graph: Some(crate::module_graph::RetainedModuleGraph::from(graph)),
        modules: None,
        files: None,
        script_used_packages: rustc_hash::FxHashSet::default(),
        trace_provenance: crate::trace::TraceProvenance::default(),
        file_hashes: rustc_hash::FxHashMap::default(),
    };

    let result = compute_file_scores_default(
        &modules,
        &file_paths,
        None,
        output,
        None,
        std::path::Path::new("/project"),
    )
    .unwrap();
    assert!(result.circular_files.contains(&path_a));
    assert!(result.circular_files.contains(&path_b));
    assert!(result.cycle_members.contains_key(&path_a));
    assert_eq!(result.cycle_members[&path_a], vec![path_b.clone()]);
    assert!(result.cycle_members.contains_key(&path_b));
    assert_eq!(result.cycle_members[&path_b], vec![path_a]);
    assert_eq!(result.analysis_counts.circular_deps, 1);
}

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "test fixture; linear setup/assert, length is not a maintainability concern"
)]
fn compute_file_scores_analysis_counts_unused_exports_and_types() {
    let path_a = std::path::PathBuf::from("/src/a.ts");
    let files = vec![crate::discover::DiscoveredFile {
        id: crate::discover::FileId(0),
        path: path_a.clone(),
        size_bytes: 100,
    }];

    let resolved_modules = vec![fallow_graph::resolve::ResolvedModule {
        file_id: crate::discover::FileId(0),
        path: path_a.clone(),
        exports: vec![
            fallow_types::extract::ExportInfo {
                name: crate::source::ExportName::Named("foo".into()),
                local_name: None,
                is_type_only: false,
                visibility: crate::source::VisibilityTag::None,
                expected_unused_reason: None,
                span: oxc_span::Span::empty(0),
                members: vec![],
                is_side_effect_used: false,
                super_class: None,
                deprecated: false,
                deprecated_reason: None,
            },
            fallow_types::extract::ExportInfo {
                name: crate::source::ExportName::Named("bar".into()),
                local_name: None,
                is_type_only: false,
                visibility: crate::source::VisibilityTag::None,
                expected_unused_reason: None,
                span: oxc_span::Span::empty(0),
                members: vec![],
                is_side_effect_used: false,
                super_class: None,
                deprecated: false,
                deprecated_reason: None,
            },
        ]
        .into(),
        ..Default::default()
    }];

    let graph = build_test_graph(&files, &[], &resolved_modules);

    let mut module = make_module_info(
        0,
        10,
        vec![fallow_types::extract::FunctionComplexity {
            name: "fn_a".into(),
            is_private_member: false,
            context_named: None,
            line: 1,
            col: 0,
            cyclomatic: 1,
            cognitive: 0,
            line_count: 10,
            param_count: 0,
            react_hook_count: 0,
            react_jsx_max_depth: 0,
            react_prop_count: 0,
            source_hash: None,
            contributions: Vec::new(),
        }],
    );
    module.exports = vec![
        fallow_types::extract::ExportInfo {
            name: crate::source::ExportName::Named("foo".into()),
            local_name: None,
            is_type_only: false,
            visibility: crate::source::VisibilityTag::None,
            expected_unused_reason: None,
            span: oxc_span::Span::empty(0),
            members: vec![],
            is_side_effect_used: false,
            super_class: None,
            deprecated: false,
            deprecated_reason: None,
        },
        fallow_types::extract::ExportInfo {
            name: crate::source::ExportName::Named("bar".into()),
            local_name: None,
            is_type_only: false,
            visibility: crate::source::VisibilityTag::None,
            expected_unused_reason: None,
            span: oxc_span::Span::empty(0),
            members: vec![],
            is_side_effect_used: false,
            super_class: None,
            deprecated: false,
            deprecated_reason: None,
        },
    ]
    .into();
    let modules = vec![module];

    let mut file_paths: rustc_hash::FxHashMap<crate::discover::FileId, &std::path::PathBuf> =
        rustc_hash::FxHashMap::default();
    file_paths.insert(crate::discover::FileId(0), &files[0].path);

    let mut results = fallow_types::results::AnalysisResults::default();
    results.unused_exports.push(
        fallow_types::output_dead_code::UnusedExportFinding::with_actions(
            fallow_types::results::UnusedExport {
                path: path_a.clone(),
                export_name: "foo".into(),
                is_type_only: false,
                line: 1,
                col: 0,
                span_start: 0,
                is_re_export: false,
                deprecated: false,
                deprecated_reason: None,
            },
        ),
    );
    results.unused_types.push(
        fallow_types::output_dead_code::UnusedTypeFinding::with_actions(
            fallow_types::results::UnusedExport {
                path: path_a,
                export_name: "MyType".into(),
                is_type_only: true,
                line: 5,
                col: 0,
                span_start: 40,
                is_re_export: false,
                deprecated: false,
                deprecated_reason: None,
            },
        ),
    );
    results.unused_dependencies.push(
        fallow_types::output_dead_code::UnusedDependencyFinding::with_actions(
            fallow_types::results::UnusedDependency {
                package_name: "lodash".into(),
                location: fallow_types::results::DependencyLocation::Dependencies,
                path: std::path::PathBuf::from("/package.json"),
                line: 1,
                used_in_workspaces: Vec::new(),
                declared_and_imported_in: Vec::new(),
            },
        ),
    );

    let output = crate::results::DeadCodeAnalysisArtifacts {
        results,
        timings: None,
        graph: Some(crate::module_graph::RetainedModuleGraph::from(graph)),
        modules: None,
        files: None,
        script_used_packages: rustc_hash::FxHashSet::default(),
        trace_provenance: crate::trace::TraceProvenance::default(),
        file_hashes: rustc_hash::FxHashMap::default(),
    };

    let result = compute_file_scores_default(
        &modules,
        &file_paths,
        None,
        output,
        None,
        std::path::Path::new("/project"),
    )
    .unwrap();
    assert_eq!(result.analysis_counts.total_exports, 2);
    assert_eq!(result.analysis_counts.dead_exports, 2);
    assert_eq!(result.analysis_counts.unused_deps, 1);
}

/// Regression: total_exports must count graph modules, not extraction modules.
#[test]
#[expect(
    clippy::too_many_lines,
    reason = "test fixture; linear setup/assert, length is not a maintainability concern"
)]
fn total_exports_counts_graph_modules_not_extraction_modules() {
    let path_a = std::path::PathBuf::from("/src/a.ts");
    let files = vec![crate::discover::DiscoveredFile {
        id: crate::discover::FileId(0),
        path: path_a.clone(),
        size_bytes: 100,
    }];

    let resolved_modules = vec![fallow_graph::resolve::ResolvedModule {
        file_id: crate::discover::FileId(0),
        path: path_a.clone(),
        exports: vec![
            fallow_types::extract::ExportInfo {
                name: crate::source::ExportName::Named("foo".into()),
                local_name: None,
                is_type_only: false,
                visibility: crate::source::VisibilityTag::None,
                expected_unused_reason: None,
                span: oxc_span::Span::empty(0),
                members: vec![],
                is_side_effect_used: false,
                super_class: None,
                deprecated: false,
                deprecated_reason: None,
            },
            fallow_types::extract::ExportInfo {
                name: crate::source::ExportName::Named("bar".into()),
                local_name: None,
                is_type_only: false,
                visibility: crate::source::VisibilityTag::None,
                expected_unused_reason: None,
                span: oxc_span::Span::empty(0),
                members: vec![],
                is_side_effect_used: false,
                super_class: None,
                deprecated: false,
                deprecated_reason: None,
            },
            fallow_types::extract::ExportInfo {
                name: crate::source::ExportName::Named("baz".into()),
                local_name: None,
                is_type_only: false,
                visibility: crate::source::VisibilityTag::None,
                expected_unused_reason: None,
                span: oxc_span::Span::new(0, 0),
                members: vec![],
                is_side_effect_used: false,
                super_class: None,
                deprecated: false,
                deprecated_reason: None,
            },
        ]
        .into(),
        ..Default::default()
    }];

    let graph = build_test_graph(&files, &[], &resolved_modules);

    let mut module = make_module_info(
        0,
        10,
        vec![fallow_types::extract::FunctionComplexity {
            name: "fn_a".into(),
            is_private_member: false,
            context_named: None,
            line: 1,
            col: 0,
            cyclomatic: 1,
            cognitive: 0,
            line_count: 10,
            param_count: 0,
            react_hook_count: 0,
            react_jsx_max_depth: 0,
            react_prop_count: 0,
            source_hash: None,
            contributions: Vec::new(),
        }],
    );
    module.exports = vec![
        fallow_types::extract::ExportInfo {
            name: crate::source::ExportName::Named("foo".into()),
            local_name: None,
            is_type_only: false,
            visibility: crate::source::VisibilityTag::None,
            expected_unused_reason: None,
            span: oxc_span::Span::empty(0),
            members: vec![],
            is_side_effect_used: false,
            super_class: None,
            deprecated: false,
            deprecated_reason: None,
        },
        fallow_types::extract::ExportInfo {
            name: crate::source::ExportName::Named("bar".into()),
            local_name: None,
            is_type_only: false,
            visibility: crate::source::VisibilityTag::None,
            expected_unused_reason: None,
            span: oxc_span::Span::empty(0),
            members: vec![],
            is_side_effect_used: false,
            super_class: None,
            deprecated: false,
            deprecated_reason: None,
        },
    ]
    .into();
    let modules = vec![module];

    let mut file_paths: rustc_hash::FxHashMap<crate::discover::FileId, &std::path::PathBuf> =
        rustc_hash::FxHashMap::default();
    file_paths.insert(crate::discover::FileId(0), &files[0].path);

    let mut results = fallow_types::results::AnalysisResults::default();
    for name in ["foo", "bar", "baz"] {
        results.unused_exports.push(
            fallow_types::output_dead_code::UnusedExportFinding::with_actions(
                fallow_types::results::UnusedExport {
                    path: path_a.clone(),
                    export_name: name.into(),
                    is_type_only: false,
                    line: 1,
                    col: 0,
                    span_start: 0,
                    is_re_export: name == "baz",
                    deprecated: false,
                    deprecated_reason: None,
                },
            ),
        );
    }

    let output = crate::results::DeadCodeAnalysisArtifacts {
        results,
        timings: None,
        graph: Some(crate::module_graph::RetainedModuleGraph::from(graph)),
        modules: None,
        files: None,
        script_used_packages: rustc_hash::FxHashSet::default(),
        trace_provenance: crate::trace::TraceProvenance::default(),
        file_hashes: rustc_hash::FxHashMap::default(),
    };

    let result = compute_file_scores_default(
        &modules,
        &file_paths,
        None,
        output,
        None,
        std::path::Path::new("/project"),
    )
    .unwrap();
    assert_eq!(result.analysis_counts.total_exports, 3);
    assert_eq!(result.analysis_counts.dead_exports, 3);
}

#[test]
fn compute_file_scores_module_not_in_file_paths_skipped() {
    let path_a = std::path::PathBuf::from("/src/a.ts");
    let files = vec![crate::discover::DiscoveredFile {
        id: crate::discover::FileId(0),
        path: path_a.clone(),
        size_bytes: 100,
    }];

    let resolved_modules = vec![fallow_graph::resolve::ResolvedModule {
        file_id: crate::discover::FileId(0),
        path: path_a,
        ..Default::default()
    }];

    let graph = build_test_graph(&files, &[], &resolved_modules);

    let modules = vec![make_module_info(
        0,
        10,
        vec![fallow_types::extract::FunctionComplexity {
            name: "fn_a".into(),
            is_private_member: false,
            context_named: None,
            line: 1,
            col: 0,
            cyclomatic: 2,
            cognitive: 1,
            line_count: 10,
            param_count: 0,
            react_hook_count: 0,
            react_jsx_max_depth: 0,
            react_prop_count: 0,
            source_hash: None,
            contributions: Vec::new(),
        }],
    )];

    let file_paths: rustc_hash::FxHashMap<crate::discover::FileId, &std::path::PathBuf> =
        rustc_hash::FxHashMap::default();

    let output = crate::results::DeadCodeAnalysisArtifacts {
        results: fallow_types::results::AnalysisResults::default(),
        timings: None,
        graph: Some(crate::module_graph::RetainedModuleGraph::from(graph)),
        modules: None,
        files: None,
        script_used_packages: rustc_hash::FxHashSet::default(),
        trace_provenance: crate::trace::TraceProvenance::default(),
        file_hashes: rustc_hash::FxHashMap::default(),
    };

    let result = compute_file_scores_default(
        &modules,
        &file_paths,
        None,
        output,
        None,
        std::path::Path::new("/project"),
    )
    .unwrap();
    assert!(result.scores.is_empty());
}

#[test]
fn compute_file_scores_mi_rounded_to_one_decimal() {
    let path_a = std::path::PathBuf::from("/src/a.ts");
    let files = vec![crate::discover::DiscoveredFile {
        id: crate::discover::FileId(0),
        path: path_a.clone(),
        size_bytes: 100,
    }];

    let resolved_modules = vec![fallow_graph::resolve::ResolvedModule {
        file_id: crate::discover::FileId(0),
        path: path_a.clone(),
        ..Default::default()
    }];

    let graph = build_test_graph(&files, std::slice::from_ref(&path_a), &resolved_modules);

    let modules = vec![make_module_info(
        0,
        100,
        vec![fallow_types::extract::FunctionComplexity {
            name: "fn".into(),
            is_private_member: false,
            context_named: None,
            line: 1,
            col: 0,
            cyclomatic: 7,
            cognitive: 3,
            line_count: 100,
            param_count: 0,
            react_hook_count: 0,
            react_jsx_max_depth: 0,
            react_prop_count: 0,
            source_hash: None,
            contributions: Vec::new(),
        }],
    )];

    let mut file_paths: rustc_hash::FxHashMap<crate::discover::FileId, &std::path::PathBuf> =
        rustc_hash::FxHashMap::default();
    file_paths.insert(crate::discover::FileId(0), &files[0].path);

    let output = crate::results::DeadCodeAnalysisArtifacts {
        results: fallow_types::results::AnalysisResults::default(),
        timings: None,
        graph: Some(crate::module_graph::RetainedModuleGraph::from(graph)),
        modules: None,
        files: None,
        script_used_packages: rustc_hash::FxHashSet::default(),
        trace_provenance: crate::trace::TraceProvenance::default(),
        file_hashes: rustc_hash::FxHashMap::default(),
    };

    let result = compute_file_scores_default(
        &modules,
        &file_paths,
        None,
        output,
        None,
        std::path::Path::new("/project"),
    )
    .unwrap();
    let mi = result.scores[0].maintainability_index;
    let rounded = (mi * 10.0).round() / 10.0;
    assert!((mi - rounded).abs() < f64::EPSILON);
}

#[test]
fn compute_file_scores_value_export_counts_tracked() {
    let path_a = std::path::PathBuf::from("/src/a.ts");
    let files = vec![crate::discover::DiscoveredFile {
        id: crate::discover::FileId(0),
        path: path_a.clone(),
        size_bytes: 100,
    }];

    let resolved_modules = vec![fallow_graph::resolve::ResolvedModule {
        file_id: crate::discover::FileId(0),
        path: path_a.clone(),
        exports: vec![
            fallow_types::extract::ExportInfo {
                name: crate::source::ExportName::Named("a".into()),
                local_name: None,
                is_type_only: false,
                visibility: crate::source::VisibilityTag::None,
                expected_unused_reason: None,
                span: oxc_span::Span::empty(0),
                members: vec![],
                is_side_effect_used: false,
                super_class: None,
                deprecated: false,
                deprecated_reason: None,
            },
            fallow_types::extract::ExportInfo {
                name: crate::source::ExportName::Named("b".into()),
                local_name: None,
                is_type_only: false,
                visibility: crate::source::VisibilityTag::None,
                expected_unused_reason: None,
                span: oxc_span::Span::empty(0),
                members: vec![],
                is_side_effect_used: false,
                super_class: None,
                deprecated: false,
                deprecated_reason: None,
            },
            fallow_types::extract::ExportInfo {
                name: crate::source::ExportName::Named("T".into()),
                local_name: None,
                is_type_only: true,
                visibility: crate::source::VisibilityTag::None,
                expected_unused_reason: None,
                span: oxc_span::Span::empty(0),
                members: vec![],
                is_side_effect_used: false,
                super_class: None,
                deprecated: false,
                deprecated_reason: None,
            },
        ]
        .into(),
        ..Default::default()
    }];

    let graph = build_test_graph(&files, &[], &resolved_modules);

    let modules = vec![make_module_info(
        0,
        10,
        vec![fallow_types::extract::FunctionComplexity {
            name: "fn_a".into(),
            is_private_member: false,
            context_named: None,
            line: 1,
            col: 0,
            cyclomatic: 2,
            cognitive: 1,
            line_count: 10,
            param_count: 0,
            react_hook_count: 0,
            react_jsx_max_depth: 0,
            react_prop_count: 0,
            source_hash: None,
            contributions: Vec::new(),
        }],
    )];

    let mut file_paths: rustc_hash::FxHashMap<crate::discover::FileId, &std::path::PathBuf> =
        rustc_hash::FxHashMap::default();
    file_paths.insert(crate::discover::FileId(0), &files[0].path);

    let output = crate::results::DeadCodeAnalysisArtifacts {
        results: fallow_types::results::AnalysisResults::default(),
        timings: None,
        graph: Some(crate::module_graph::RetainedModuleGraph::from(graph)),
        modules: None,
        files: None,
        script_used_packages: rustc_hash::FxHashSet::default(),
        trace_provenance: crate::trace::TraceProvenance::default(),
        file_hashes: rustc_hash::FxHashMap::default(),
    };

    let result = compute_file_scores_default(
        &modules,
        &file_paths,
        None,
        output,
        None,
        std::path::Path::new("/project"),
    )
    .unwrap();
    assert_eq!(result.value_export_counts[&path_a], 2);
}

#[test]
fn compute_file_scores_top_complex_fns_zero_cognitive_excluded() {
    let path_a = std::path::PathBuf::from("/src/simple.ts");
    let files = vec![crate::discover::DiscoveredFile {
        id: crate::discover::FileId(0),
        path: path_a.clone(),
        size_bytes: 100,
    }];

    let resolved_modules = vec![fallow_graph::resolve::ResolvedModule {
        file_id: crate::discover::FileId(0),
        path: path_a.clone(),
        ..Default::default()
    }];

    let graph = build_test_graph(&files, &[], &resolved_modules);

    let modules = vec![make_module_info(
        0,
        10,
        vec![fallow_types::extract::FunctionComplexity {
            name: "trivial".into(),
            is_private_member: false,
            context_named: None,
            line: 1,
            col: 0,
            cyclomatic: 1,
            cognitive: 0,
            line_count: 10,
            param_count: 0,
            react_hook_count: 0,
            react_jsx_max_depth: 0,
            react_prop_count: 0,
            source_hash: None,
            contributions: Vec::new(),
        }],
    )];

    let mut file_paths: rustc_hash::FxHashMap<crate::discover::FileId, &std::path::PathBuf> =
        rustc_hash::FxHashMap::default();
    file_paths.insert(crate::discover::FileId(0), &files[0].path);

    let output = crate::results::DeadCodeAnalysisArtifacts {
        results: fallow_types::results::AnalysisResults::default(),
        timings: None,
        graph: Some(crate::module_graph::RetainedModuleGraph::from(graph)),
        modules: None,
        files: None,
        script_used_packages: rustc_hash::FxHashSet::default(),
        trace_provenance: crate::trace::TraceProvenance::default(),
        file_hashes: rustc_hash::FxHashMap::default(),
    };

    let result = compute_file_scores_default(
        &modules,
        &file_paths,
        None,
        output,
        None,
        std::path::Path::new("/project"),
    )
    .unwrap();
    assert!(!result.top_complex_fns.contains_key(&path_a));
}

pub(super) fn make_fn_complexity(cyclomatic: u16) -> fallow_types::extract::FunctionComplexity {
    fallow_types::extract::FunctionComplexity {
        name: "test_fn".into(),
        is_private_member: false,
        context_named: None,
        line: 1,
        col: 0,
        cyclomatic,
        cognitive: 0,
        line_count: 10,
        param_count: 0,
        react_hook_count: 0,
        react_jsx_max_depth: 0,
        react_prop_count: 0,
        source_hash: None,
        contributions: Vec::new(),
    }
}

fn make_named_fn_complexity(
    name: &str,
    line: u32,
    cyclomatic: u16,
) -> fallow_types::extract::FunctionComplexity {
    fallow_types::extract::FunctionComplexity {
        name: name.into(),
        is_private_member: false,
        context_named: None,
        line,
        col: 0,
        cyclomatic,
        cognitive: 0,
        line_count: 10,
        param_count: 0,
        react_hook_count: 0,
        react_jsx_max_depth: 0,
        react_prop_count: 0,
        source_hash: None,
        contributions: Vec::new(),
    }
}

fn crap_override_entry(
    files: &[&str],
    functions: &[&str],
    max_crap: Option<f64>,
) -> fallow_config::HealthThresholdOverride {
    fallow_config::HealthThresholdOverride {
        files: files.iter().map(ToString::to_string).collect(),
        functions: functions.iter().map(ToString::to_string).collect(),
        max_cyclomatic: None,
        max_cognitive: None,
        max_crap,
        max_unit_size: None,
        max_file_lines: None,
        reason: Some("test override".into()),
    }
}

fn estimated_signals_with(
    resolver: &ThresholdOverrideResolver,
    relative: &str,
    enforce_crap: bool,
    complexity: &[fallow_types::extract::FunctionComplexity],
) -> CrapThresholdSignals {
    let ceilings = CrapCeilingLookup::new(
        CrapScoreThresholds {
            resolver,
            enforce_crap,
        },
        std::path::Path::new(relative),
        &[],
    );
    compute_crap_scores_estimated(
        complexity,
        &rustc_hash::FxHashSet::default(),
        false,
        fallow_output::CoverageSource::Estimated,
        &ceilings,
    )
    .signals
}

#[test]
fn crap_counting_exempts_functions_under_override_ceiling() {
    // The issue's repro: two untested cyclomatic-10 functions (CRAP 110)
    // under an override raising maxCrap to 500 on the file.
    let resolver =
        test_override_resolver(&[crap_override_entry(&["src/legacy.ts"], &[], Some(500.0))]);
    let fns = vec![
        make_named_fn_complexity("a", 1, 10),
        make_named_fn_complexity("b", 12, 10),
    ];

    let covered = estimated_signals_with(&resolver, "src/legacy.ts", true, &fns);
    assert_eq!(covered.above, 0);
    assert_eq!(covered.exempted, 2);
    assert_eq!(covered.min_ceiling, Some(500.0));

    let elsewhere = estimated_signals_with(&resolver, "src/other.ts", true, &fns);
    assert_eq!(elsewhere.above, 2);
    assert_eq!(elsewhere.exempted, 0);
    assert_eq!(elsewhere.min_ceiling, Some(CRAP_THRESHOLD));
}

#[test]
fn crap_counting_insufficient_override_keeps_count() {
    let resolver =
        test_override_resolver(&[crap_override_entry(&["src/legacy.ts"], &[], Some(50.0))]);
    let fns = vec![
        make_named_fn_complexity("a", 1, 10),
        make_named_fn_complexity("b", 12, 10),
    ];

    let signals = estimated_signals_with(&resolver, "src/legacy.ts", true, &fns);
    assert_eq!(signals.above, 2);
    assert_eq!(signals.exempted, 0);
    assert_eq!(signals.min_ceiling, Some(50.0));
}

#[test]
fn crap_counting_partial_function_override() {
    // Only `a` is exempted; `b` keeps the global ceiling, which is also
    // the file's lowest effective ceiling.
    let resolver =
        test_override_resolver(&[crap_override_entry(&["src/legacy.ts"], &["a"], Some(500.0))]);
    let fns = vec![
        make_named_fn_complexity("a", 1, 10),
        make_named_fn_complexity("b", 12, 10),
    ];

    let signals = estimated_signals_with(&resolver, "src/legacy.ts", true, &fns);
    assert_eq!(signals.above, 1);
    assert_eq!(signals.exempted, 1);
    assert_eq!(signals.min_ceiling, Some(CRAP_THRESHOLD));
}

#[test]
fn crap_counting_disabled_enforcement_counts_baseline_exemptions() {
    // Global maxCrap 0 disables enforcement: nothing is above threshold
    // and every canonical-baseline breach is disclosed as exempt.
    let resolver = test_crap_resolver(0.0);
    let fns = vec![
        make_named_fn_complexity("a", 1, 10),
        make_named_fn_complexity("b", 12, 10),
        make_named_fn_complexity("tiny", 24, 1),
    ];

    let signals = estimated_signals_with(&resolver, "src/any.ts", false, &fns);
    assert_eq!(signals.above, 0);
    assert_eq!(signals.exempted, 2);
}

#[test]
fn crap_counting_stricter_ceiling_never_counts_exempt() {
    // A ceiling below the canonical baseline flags the band between it and
    // 30 as above-threshold, never as exempt.
    let resolver = test_crap_resolver(10.0);
    let fns = vec![make_named_fn_complexity("a", 1, 4)]; // untested CRAP 20

    let signals = estimated_signals_with(&resolver, "src/any.ts", true, &fns);
    assert_eq!(signals.above, 1);
    assert_eq!(signals.exempted, 0);
}

#[test]
fn crap_counting_uses_rounded_value_at_boundary() {
    // Istanbul coverage tuned so unrounded CRAP is 29.96, which rounds to
    // the stored per-function 30.0. The findings pipeline compares the
    // ROUNDED value against the ceiling and emits a finding; the count
    // must agree at the same boundary (issue #2228).
    let funcs = vec![make_fn_complexity(10)];
    let mut functions = rustc_hash::FxHashMap::default();
    functions.insert(("test_fn".to_string(), 1, 0), 41.56);
    let file_cov = test_istanbul_file_coverage(functions, false);

    let result = istanbul_crap_default(&funcs, Some(&file_cov), false);
    assert!((result.per_function[0].crap - 30.0).abs() < f64::EPSILON);
    assert_eq!(result.signals.above, 1);
    assert_eq!(result.signals.exempted, 0);
}

#[test]
fn compute_file_scores_discloses_override_exemption_on_row() {
    let path_a = std::path::PathBuf::from("/project/src/legacy.ts");
    let files = vec![crate::discover::DiscoveredFile {
        id: crate::discover::FileId(0),
        path: path_a.clone(),
        size_bytes: 100,
    }];
    let resolved_modules = vec![fallow_graph::resolve::ResolvedModule {
        file_id: crate::discover::FileId(0),
        path: path_a.clone(),
        ..Default::default()
    }];
    let graph = build_test_graph(&files, std::slice::from_ref(&path_a), &resolved_modules);
    let modules = vec![make_module_info(
        0,
        26,
        vec![
            make_named_fn_complexity("a", 1, 10),
            make_named_fn_complexity("b", 12, 10),
        ],
    )];
    let mut file_paths: rustc_hash::FxHashMap<crate::discover::FileId, &std::path::PathBuf> =
        rustc_hash::FxHashMap::default();
    file_paths.insert(crate::discover::FileId(0), &files[0].path);
    let output = crate::results::DeadCodeAnalysisArtifacts {
        results: fallow_types::results::AnalysisResults::default(),
        timings: None,
        graph: Some(crate::module_graph::RetainedModuleGraph::from(graph)),
        modules: None,
        files: None,
        script_used_packages: rustc_hash::FxHashSet::default(),
        trace_provenance: crate::trace::TraceProvenance::default(),
        file_hashes: rustc_hash::FxHashMap::default(),
    };

    let resolver =
        test_override_resolver(&[crap_override_entry(&["src/legacy.ts"], &[], Some(500.0))]);
    let result = compute_file_scores(
        FileScoreComputeInput {
            modules: &modules,
            file_paths: &file_paths,
            changed_files: None,
            istanbul_coverage: None,
            root: std::path::Path::new("/project"),
            crap_thresholds: CrapScoreThresholds {
                resolver: &resolver,
                enforce_crap: true,
            },
            with_static_links: false,
            test_reach: None,
        },
        output,
    )
    .unwrap();

    assert_eq!(result.scores.len(), 1);
    let score = &result.scores[0];
    assert!((score.crap_max - 110.0).abs() < f64::EPSILON);
    assert_eq!(score.crap_above_threshold, 0);
    assert_eq!(score.crap_exempted, 2);
    assert_eq!(score.crap_effective_threshold, Some(500.0));
    assert!(file_score_fully_crap_exempt(score, CRAP_THRESHOLD));
    assert_eq!(
        file_score_concern_axis(score, CRAP_THRESHOLD),
        FileScoreConcern::Structural
    );
}

#[test]
fn compute_file_scores_raised_global_omits_row_threshold() {
    let path_a = std::path::PathBuf::from("/project/src/legacy.ts");
    let files = vec![crate::discover::DiscoveredFile {
        id: crate::discover::FileId(0),
        path: path_a.clone(),
        size_bytes: 100,
    }];
    let resolved_modules = vec![fallow_graph::resolve::ResolvedModule {
        file_id: crate::discover::FileId(0),
        path: path_a.clone(),
        ..Default::default()
    }];
    let graph = build_test_graph(&files, std::slice::from_ref(&path_a), &resolved_modules);
    let modules = vec![make_module_info(
        0,
        26,
        vec![
            make_named_fn_complexity("a", 1, 10),
            make_named_fn_complexity("b", 12, 10),
        ],
    )];
    let mut file_paths: rustc_hash::FxHashMap<crate::discover::FileId, &std::path::PathBuf> =
        rustc_hash::FxHashMap::default();
    file_paths.insert(crate::discover::FileId(0), &files[0].path);
    let output = crate::results::DeadCodeAnalysisArtifacts {
        results: fallow_types::results::AnalysisResults::default(),
        timings: None,
        graph: Some(crate::module_graph::RetainedModuleGraph::from(graph)),
        modules: None,
        files: None,
        script_used_packages: rustc_hash::FxHashSet::default(),
        trace_provenance: crate::trace::TraceProvenance::default(),
        file_hashes: rustc_hash::FxHashMap::default(),
    };

    // Raised via the global (`--max-crap 5000`), not an override: the row
    // must not repeat the run global as its own effective threshold.
    let resolver = test_crap_resolver(5000.0);
    let result = compute_file_scores(
        FileScoreComputeInput {
            modules: &modules,
            file_paths: &file_paths,
            changed_files: None,
            istanbul_coverage: None,
            root: std::path::Path::new("/project"),
            crap_thresholds: CrapScoreThresholds {
                resolver: &resolver,
                enforce_crap: true,
            },
            with_static_links: false,
            test_reach: None,
        },
        output,
    )
    .unwrap();

    assert_eq!(result.scores.len(), 1);
    let score = &result.scores[0];
    assert_eq!(score.crap_above_threshold, 0);
    assert_eq!(score.crap_exempted, 2);
    assert_eq!(score.crap_effective_threshold, None);
    assert!(file_score_fully_crap_exempt(score, 5000.0));
    assert_eq!(
        file_score_concern_axis(score, 5000.0),
        FileScoreConcern::Structural
    );
}

/// Untested-file aggregation on the shipped estimated path: the reported
/// max is the highest rounded score, `above` counts only the units at or
/// over their effective ceiling, and the synthetic template and module
/// units leave the CRAP dimension entirely.
#[test]
fn estimated_crap_untested_aggregates_over_real_units_only() {
    let funcs = vec![
        make_named_fn_complexity("below", 1, 4),
        make_named_fn_complexity("at_threshold", 2, 5),
        make_named_fn_complexity("above_threshold", 3, 8),
        make_named_fn_complexity("<template>", 4, 21),
        make_named_fn_complexity("<module>", 5, 21),
    ];
    let result = estimated_crap_default(
        &funcs,
        &rustc_hash::FxHashSet::default(),
        false,
        fallow_output::CoverageSource::Estimated,
    );
    assert!((result.max_crap - 72.0).abs() < f64::EPSILON, "{result:#?}");
    assert_eq!(result.signals.above, 2);
    assert_eq!(result.per_function.len(), 3);
}

#[test]
fn crap_formula_full_coverage() {
    let result = crap_formula(10.0, 100.0);
    assert!((result - 10.0).abs() < f64::EPSILON);
}

#[test]
fn crap_formula_zero_coverage() {
    let result = crap_formula(5.0, 0.0);
    assert!((result - 30.0).abs() < f64::EPSILON);
}

#[test]
fn crap_formula_partial_coverage() {
    let result = crap_formula(10.0, 50.0);
    assert!((result - 22.5).abs() < f64::EPSILON);
}

#[test]
fn crap_formula_high_coverage_low_complexity() {
    let result = crap_formula(2.0, 90.0);
    assert!((result - 2.004).abs() < 0.001);
}

/// Pin the exact cyclomatic value at which the default CRAP gate (30.0)
/// trips for each estimated-coverage tier. These numbers back the
/// changelog and docs wording: 5 at the 0% tier, 10 at the 40% indirect
/// tier, 28 at the 85% direct tier.
#[test]
fn crap_default_gate_cyclomatic_boundaries_per_estimate_tier() {
    for (coverage_pct, gate_cc) in [(0.0, 5.0), (40.0, 10.0), (85.0, 28.0)] {
        assert!(
            crap_formula(gate_cc, coverage_pct) >= CRAP_THRESHOLD,
            "cyclomatic {gate_cc} at {coverage_pct}% must reach the gate"
        );
        assert!(
            crap_formula(gate_cc - 1.0, coverage_pct) < CRAP_THRESHOLD,
            "cyclomatic {} at {coverage_pct}% must stay under the gate",
            gate_cc - 1.0
        );
    }
}

#[test]
fn istanbul_crap_excludes_synthetic_template_units() {
    let funcs = vec![
        make_named_fn_complexity("<template>", 1, 21),
        make_named_fn_complexity("<snippet:rowBody>", 1, 16),
        make_fn_complexity(6),
    ];
    let result = istanbul_crap_default(&funcs, None, false);
    assert!((result.max_crap - 42.0).abs() < f64::EPSILON, "{result:#?}");
    assert_eq!(result.signals.above, 1);
    assert_eq!(
        result.total, 1,
        "template units must not count as unmatched"
    );
    assert_eq!(result.per_function.len(), 1);
}

#[test]
fn estimated_crap_excludes_synthetic_template_units() {
    let funcs = vec![
        make_named_fn_complexity("<template>", 1, 21),
        make_named_fn_complexity("<snippet:rowBody>", 1, 16),
    ];
    let result = estimated_crap_default(
        &funcs,
        &rustc_hash::FxHashSet::default(),
        false,
        fallow_output::CoverageSource::Estimated,
    );
    assert!(result.max_crap.abs() < f64::EPSILON, "{result:#?}");
    assert_eq!(result.signals.above, 0);
    assert!(result.per_function.is_empty());
}

#[test]
fn istanbul_crap_with_coverage_data() {
    let funcs = vec![make_fn_complexity(10)];
    let mut functions = rustc_hash::FxHashMap::default();
    functions.insert(("test_fn".to_string(), 1, 0), 80.0);
    let file_cov = test_istanbul_file_coverage(functions, false);
    let result = istanbul_crap_default(&funcs, Some(&file_cov), false);
    assert!((result.max_crap - 10.8).abs() < 0.1);
    assert_eq!(result.signals.above, 0);
}

#[test]
fn istanbul_crap_falls_back_to_binary_when_no_match() {
    let funcs = vec![make_fn_complexity(6)];
    let file_cov = test_istanbul_file_coverage(rustc_hash::FxHashMap::default(), false);
    let result = istanbul_crap_default(&funcs, Some(&file_cov), false);
    assert!((result.max_crap - 42.0).abs() < f64::EPSILON);
    assert_eq!(result.signals.above, 1);
}

/// A file tests reach, with no coverage data for it at all, keeps the
/// static estimate rather than being scored as fully covered.
#[test]
fn istanbul_crap_uses_the_static_estimate_when_no_file_coverage() {
    let funcs = vec![make_fn_complexity(5)];
    let result = istanbul_crap_default(&funcs, None, true);
    // The reported score is rounded to one decimal, so compare against
    // the estimate rather than the formula's last bit.
    assert!((result.max_crap - 10.4).abs() < 1e-9);
    assert_eq!(result.signals.above, 0);
}

#[test]
fn istanbul_crap_zero_coverage_matches_binary_untested() {
    let funcs = vec![make_fn_complexity(5)];
    let mut functions = rustc_hash::FxHashMap::default();
    functions.insert(("test_fn".to_string(), 1, 0), 0.0);
    let file_cov = test_istanbul_file_coverage(functions, false);
    let result = istanbul_crap_default(&funcs, Some(&file_cov), false);
    assert!((result.max_crap - 30.0).abs() < f64::EPSILON);
    assert_eq!(result.signals.above, 1);
}

#[test]
fn estimated_crap_direct_test_reference() {
    let funcs = vec![make_fn_complexity(10)];
    let mut refs = rustc_hash::FxHashSet::default();
    refs.insert("test_fn".to_string());
    let result = estimated_crap_default(
        &funcs,
        &refs,
        true,
        fallow_output::CoverageSource::Estimated,
    );
    let (max, above) = (result.max_crap, result.signals.above);
    assert!((max - 10.3).abs() < 0.1);
    assert_eq!(above, 0);
}

#[test]
fn estimated_crap_context_name_is_not_a_direct_test_reference() {
    let mut generator = make_fn_complexity(10);
    generator.context_named = Some(fallow_types::extract::AnonymousFunctionKind::Function);
    let mut refs = rustc_hash::FxHashSet::default();
    refs.insert("test_fn".to_string());
    let with_context_name = estimated_crap_default(
        &[generator],
        &refs,
        true,
        fallow_output::CoverageSource::Estimated,
    );
    let without_reference = estimated_crap_default(
        &[make_fn_complexity(10)],
        &rustc_hash::FxHashSet::default(),
        true,
        fallow_output::CoverageSource::Estimated,
    );
    assert!((with_context_name.max_crap - without_reference.max_crap).abs() < f64::EPSILON);
}

#[test]
fn estimated_crap_indirect_test_reachable() {
    let funcs = vec![make_fn_complexity(10)];
    let refs = rustc_hash::FxHashSet::default();
    let result = estimated_crap_default(
        &funcs,
        &refs,
        true,
        fallow_output::CoverageSource::Estimated,
    );
    let (max, above) = (result.max_crap, result.signals.above);
    assert!((max - 31.6).abs() < 0.1);
    assert_eq!(above, 1);
}

#[test]
fn estimated_crap_untested_file() {
    let funcs = vec![make_fn_complexity(5)];
    let refs = rustc_hash::FxHashSet::default();
    let result = estimated_crap_default(
        &funcs,
        &refs,
        false,
        fallow_output::CoverageSource::Estimated,
    );
    let (max, above) = (result.max_crap, result.signals.above);
    assert!((max - 30.0).abs() < f64::EPSILON);
    assert_eq!(above, 1);
}

#[test]
fn estimated_crap_low_complexity_direct_ref() {
    let funcs = vec![make_fn_complexity(2)];
    let mut refs = rustc_hash::FxHashSet::default();
    refs.insert("test_fn".to_string());
    let result = estimated_crap_default(
        &funcs,
        &refs,
        true,
        fallow_output::CoverageSource::Estimated,
    );
    let (max, above) = (result.max_crap, result.signals.above);
    assert!(max < 3.0);
    assert_eq!(above, 0);
}

#[test]
fn estimated_crap_empty() {
    let refs = rustc_hash::FxHashSet::default();
    let result = estimated_crap_default(&[], &refs, true, fallow_output::CoverageSource::Estimated);
    let (max, above) = (result.max_crap, result.signals.above);
    assert!((max).abs() < f64::EPSILON);
    assert_eq!(above, 0);
}

fn make_export(name: &str, is_type_only: bool) -> fallow_graph::graph::ExportSymbol {
    fallow_graph::graph::ExportSymbol {
        name: fallow_types::extract::ExportName::Named(name.into()),
        is_type_only,
        is_side_effect_used: false,
        visibility: crate::source::VisibilityTag::None,
        expected_unused_reason: None,
        span: oxc_span::Span::default(),
        references: vec![],
        reference_paths: Vec::new(),
        members: vec![],
        deprecated: false,
        deprecated_reason: None,
    }
}

#[test]
fn dead_code_ratio_type_only_exports_excluded_from_denominator() {
    let path = std::path::Path::new("src/types.ts");
    let exports = vec![
        make_export("MyInterface", true),
        make_export("MyType", true),
        make_export("myFunction", false),
    ];
    let unused_files = rustc_hash::FxHashSet::default();
    let mut unused_by_path = rustc_hash::FxHashMap::default();
    unused_by_path.insert(path, 1_usize); // 1 unused value export

    let ratio = compute_dead_code_ratio(path, &exports, &unused_files, &unused_by_path);
    assert!((ratio - 1.0).abs() < f64::EPSILON);
}

#[test]
fn dead_code_ratio_only_type_exports_returns_zero() {
    let path = std::path::Path::new("src/types.ts");
    let exports = vec![
        make_export("MyInterface", true),
        make_export("MyType", true),
    ];
    let unused_files = rustc_hash::FxHashSet::default();
    let unused_by_path = rustc_hash::FxHashMap::default();

    let ratio = compute_dead_code_ratio(path, &exports, &unused_files, &unused_by_path);
    assert!(ratio.abs() < f64::EPSILON);
}

#[test]
fn dead_code_ratio_mixed_exports_counts_only_values() {
    let path = std::path::Path::new("src/component.ts");
    let exports = vec![
        make_export("Props", true),
        make_export("State", true),
        make_export("Component", false),
        make_export("helper", false),
    ];
    let unused_files = rustc_hash::FxHashSet::default();
    let mut unused_by_path = rustc_hash::FxHashMap::default();
    unused_by_path.insert(path, 1_usize);

    let ratio = compute_dead_code_ratio(path, &exports, &unused_files, &unused_by_path);
    assert!((ratio - 0.5).abs() < f64::EPSILON);
}

pub(super) fn write_single_file_istanbul_fixture(
    coverage_path: &std::path::Path,
    source_path: &std::path::Path,
    fn_map: &serde_json::Value,
    function_hits: &serde_json::Value,
) {
    write_single_file_istanbul_fixture_with_statements(
        coverage_path,
        source_path,
        fn_map,
        function_hits,
        &serde_json::json!({}),
        &serde_json::json!({}),
    );
}

fn write_single_file_istanbul_fixture_with_statements(
    coverage_path: &std::path::Path,
    source_path: &std::path::Path,
    fn_map: &serde_json::Value,
    function_hits: &serde_json::Value,
    statement_map: &serde_json::Value,
    statement_hits: &serde_json::Value,
) {
    let mut root = serde_json::Map::new();
    root.insert(
        source_path.to_string_lossy().into_owned(),
        serde_json::json!({
            "path": source_path.to_string_lossy().into_owned(),
            "statementMap": statement_map,
            "fnMap": fn_map,
            "branchMap": {},
            "s": statement_hits,
            "f": function_hits,
            "b": {}
        }),
    );

    std::fs::write(coverage_path, serde_json::to_string(&root).unwrap()).unwrap();
}

/// `v8-to-istanbul`, which is what c8 and nyc write, records `column: -1`
/// for the implicit else of a bare `if`. Positions are unsigned, so the
/// strict parse rejected the whole map over a coordinate in a section
/// nothing here reads. Geometry from a map generated by running real V8
/// coverage through v8-to-istanbul 9.2.0.
#[test]
fn a_negative_branch_coordinate_does_not_cost_the_whole_map() {
    let temp = tempfile::TempDir::new().unwrap();
    let source_path = temp.path().join("src/pick.ts");
    std::fs::create_dir_all(source_path.parent().unwrap()).unwrap();
    std::fs::write(&source_path, "// geometry fixture\n").unwrap();

    let coverage_path = temp.path().join("coverage-final.json");
    let source = source_path.to_string_lossy().into_owned();
    std::fs::write(
            &coverage_path,
            serde_json::to_string(&serde_json::json!({
                source.clone(): {
                    "path": source,
                    "statementMap": {},
                    "fnMap": {
                        "0": {
                            "name": "pick",
                            "line": 1,
                            "decl": { "start": { "line": 1, "column": 16 }, "end": { "line": 1, "column": 20 } },
                            "loc": { "start": { "line": 1, "column": 41 }, "end": { "line": 6, "column": 1 } }
                        }
                    },
                    "branchMap": {
                        "0": {
                            "type": "branch",
                            "line": 5,
                            "loc": {
                                "start": { "line": 5, "column": -1 },
                                "end": { "line": 6, "column": 0 }
                            },
                            "locations": [
                                {
                                    "start": { "line": 5, "column": -1 },
                                    "end": { "line": 6, "column": 0 }
                                }
                            ]
                        }
                    },
                    "s": {},
                    "f": { "0": 2 },
                    "b": { "0": [1, 0] }
                }
            }))
            .unwrap(),
        )
        .unwrap();

    let coverage = load_istanbul_coverage(&coverage_path, None, None, false).unwrap();
    let canonical_source = dunce::canonicalize(&source_path).unwrap();
    let file_coverage = coverage.get(&canonical_source).unwrap();

    assert_eq!(file_coverage.lookup("pick", 1, 16), Some(100.0));
}

/// Vitest `coverage-v8` (through ast-v8-to-istanbul) records a negative
/// hit count for the implicit else of an `if`, for example `[5, -1]`.
/// Counts are unsigned, so the strict parse rejected the whole map over a
/// value in `b`, which nothing here reads (issue #3564).
#[test]
fn a_negative_branch_count_does_not_cost_the_whole_map() {
    let temp = tempfile::TempDir::new().unwrap();
    let source_path = temp.path().join("src/a.js");
    std::fs::create_dir_all(source_path.parent().unwrap()).unwrap();
    std::fs::write(
        &source_path,
        "export function f(a) {\n  if (a) {\n    return 1;\n  }\n  return 2;\n}\n",
    )
    .unwrap();

    let coverage_path = temp.path().join("coverage-final.json");
    let source = source_path.to_string_lossy().into_owned();
    let branch_loc = serde_json::json!({
        "start": { "line": 2, "column": 2 },
        "end": { "line": 4, "column": 3 }
    });
    std::fs::write(
            &coverage_path,
            serde_json::to_string(&serde_json::json!({
                source.clone(): {
                    "path": source,
                    "statementMap": {
                        "0": branch_loc,
                        "1": { "start": { "line": 3, "column": 4 }, "end": { "line": 3, "column": 13 } },
                        "2": { "start": { "line": 5, "column": 2 }, "end": { "line": 5, "column": 11 } }
                    },
                    "fnMap": {
                        "0": {
                            "name": "f",
                            "line": 1,
                            "decl": { "start": { "line": 1, "column": 16 }, "end": { "line": 1, "column": 17 } },
                            "loc": { "start": { "line": 1, "column": 20 }, "end": { "line": 6, "column": 1 } }
                        }
                    },
                    "branchMap": {
                        "0": {
                            "type": "if",
                            "line": 2,
                            "loc": branch_loc,
                            "locations": [branch_loc, branch_loc]
                        }
                    },
                    "s": { "0": 5, "1": 5, "2": 0 },
                    "f": { "0": 5 },
                    "b": { "0": [5, -1] },
                    "b_T": { "0": [5, -5] }
                }
            }))
            .unwrap(),
        )
        .unwrap();

    let coverage = load_istanbul_coverage(&coverage_path, None, None, false).unwrap();
    let canonical_source = dunce::canonicalize(&source_path).unwrap();
    let file_coverage = coverage.get(&canonical_source).unwrap();

    let pct = file_coverage.lookup("f", 1, 16).unwrap();
    assert!((pct - 200.0 / 3.0).abs() < 1e-9, "got {pct}");
}

/// A negative statement or function count is a miss, the same answer the
/// `> 0` coverage test gives for it, rather than a reason to reject the map.
#[test]
fn a_negative_statement_count_reads_as_a_miss() {
    let temp = tempfile::TempDir::new().unwrap();
    let source_path = temp.path().join("src/a.js");
    std::fs::create_dir_all(source_path.parent().unwrap()).unwrap();
    std::fs::write(&source_path, "// geometry fixture\n").unwrap();

    let coverage_path = temp.path().join("coverage-final.json");
    write_single_file_istanbul_fixture_with_statements(
        &coverage_path,
        &source_path,
        &serde_json::json!({
            "0": {
                "name": "f",
                "line": 1,
                "decl": { "start": { "line": 1, "column": 16 }, "end": { "line": 1, "column": 17 } },
                "loc": { "start": { "line": 1, "column": 20 }, "end": { "line": 6, "column": 1 } }
            }
        }),
        &serde_json::json!({ "0": -2 }),
        &serde_json::json!({
            "0": { "start": { "line": 2, "column": 2 }, "end": { "line": 2, "column": 11 } },
            "1": { "start": { "line": 3, "column": 2 }, "end": { "line": 3, "column": 11 } }
        }),
        &serde_json::json!({ "0": 3, "1": -1 }),
    );

    let coverage = load_istanbul_coverage(&coverage_path, None, None, false).unwrap();
    let canonical_source = dunce::canonicalize(&source_path).unwrap();
    let file_coverage = coverage.get(&canonical_source).unwrap();

    assert_eq!(file_coverage.lookup("f", 1, 16), Some(50.0));
}

/// Raw V8 coverage and `oxc-coverage-instrument` record an accessor as
/// `get area`, istanbul-lib-instrument leaves it anonymous, and fallow
/// extracts the unit as `area`. Both spellings must reach the record.
#[test]
fn an_accessor_answers_to_its_property_name() {
    let temp = tempfile::TempDir::new().unwrap();
    let source_path = temp.path().join("src/box.ts");
    std::fs::create_dir_all(source_path.parent().unwrap()).unwrap();
    std::fs::write(&source_path, "// geometry fixture\n").unwrap();

    let coverage_path = temp.path().join("coverage-final.json");
    write_single_file_istanbul_fixture(
        &coverage_path,
        &source_path,
        &serde_json::json!({
            "0": {
                "name": "get area",
                "line": 4,
                "decl": { "start": { "line": 4, "column": 6 }, "end": { "line": 4, "column": 10 } },
                "loc": { "start": { "line": 4, "column": 13 }, "end": { "line": 6, "column": 3 } }
            }
        }),
        &serde_json::json!({ "0": 0 }),
    );

    let coverage = load_istanbul_coverage(&coverage_path, None, None, false).unwrap();
    let canonical_source = dunce::canonicalize(&source_path).unwrap();
    let file_coverage = coverage.get(&canonical_source).unwrap();

    // Fallow extracts the accessor under the property name alone.
    assert_eq!(file_coverage.lookup("area", 4, 6), Some(0.0));
    // The producer's own spelling still resolves.
    assert_eq!(file_coverage.lookup("get area", 4, 6), Some(0.0));
}

#[test]
fn resolve_relative_to_root_joins_relative_with_project_root() {
    let resolved = resolve_relative_to_root(
        std::path::Path::new("coverage/coverage-final.json"),
        Some(std::path::Path::new("/work/my-app")),
    );
    assert_eq!(
        resolved,
        std::path::PathBuf::from("/work/my-app/coverage/coverage-final.json")
    );
}

#[test]
fn resolve_relative_to_root_returns_absolute_unchanged() {
    let resolved = resolve_relative_to_root(
        std::path::Path::new("/tmp/coverage-final.json"),
        Some(std::path::Path::new("/work/my-app")),
    );
    assert_eq!(
        resolved,
        std::path::PathBuf::from("/tmp/coverage-final.json")
    );
}

#[test]
fn resolve_relative_to_root_returns_windows_absolute_unchanged_on_any_host() {
    let path = std::path::Path::new(r"C:\coverage\coverage-final.json");
    let resolved = resolve_relative_to_root(path, Some(std::path::Path::new("/work/my-app")));
    assert_eq!(resolved, path);
}

#[cfg(windows)]
#[test]
fn resolve_relative_to_root_returns_posix_rooted_path_unchanged_on_windows() {
    let path = std::path::Path::new(r"/ci/workspace/coverage-final.json");
    let resolved = resolve_relative_to_root(path, Some(std::path::Path::new(r"C:\work\my-app")));
    assert_eq!(resolved, path);
}

#[test]
fn resolve_relative_to_root_without_project_root_returns_relative_unchanged() {
    let resolved =
        resolve_relative_to_root(std::path::Path::new("coverage/coverage-final.json"), None);
    assert_eq!(
        resolved,
        std::path::PathBuf::from("coverage/coverage-final.json")
    );
}

/// nyc and some Jest setups record project-relative keys. Resolving one
/// against the process directory misses the file, and a run from anywhere
/// but the project root loses the whole map at once.
#[test]
fn load_istanbul_coverage_resolves_relative_map_keys_against_project_root() {
    let temp = tempfile::TempDir::new().unwrap();
    let source_path = temp.path().join("src/index.ts");
    std::fs::create_dir_all(source_path.parent().unwrap()).unwrap();
    std::fs::write(&source_path, "export function f(){}").unwrap();

    let coverage_path = temp.path().join("coverage-final.json");
    std::fs::write(
            &coverage_path,
            serde_json::to_string(&serde_json::json!({
                "src/index.ts": {
                    "path": "src/index.ts",
                    "statementMap": {},
                    "fnMap": {
                        "0": {
                            "name": "f",
                            "decl": { "start": { "line": 1, "column": 0 }, "end": { "line": 1, "column": 21 } },
                            "loc": { "start": { "line": 1, "column": 0 }, "end": { "line": 1, "column": 21 } }
                        }
                    },
                    "branchMap": {},
                    "s": {},
                    "f": { "0": 2 },
                    "b": {}
                }
            }))
            .unwrap(),
        )
        .unwrap();

    let coverage = load_istanbul_coverage(&coverage_path, None, Some(temp.path()), false).unwrap();
    let canonical_source = dunce::canonicalize(&source_path).unwrap();
    let file_coverage = coverage.get(&canonical_source).unwrap();

    assert_eq!(file_coverage.lookup("f", 1, 0), Some(100.0));
}

#[test]
fn load_istanbul_coverage_resolves_relative_path_against_project_root() {
    let temp = tempfile::TempDir::new().unwrap();
    let source_path = temp.path().join("src/index.ts");
    std::fs::create_dir_all(source_path.parent().unwrap()).unwrap();
    std::fs::write(&source_path, "export function f(){}").unwrap();

    let coverage_path = temp.path().join("coverage/coverage-final.json");
    std::fs::create_dir_all(coverage_path.parent().unwrap()).unwrap();
    write_single_file_istanbul_fixture(
        &coverage_path,
        &source_path,
        &serde_json::json!({
            "0": {
                "name": "f",
                "decl": { "start": { "line": 1, "column": 0 }, "end": { "line": 1, "column": 21 } },
                "loc":  { "start": { "line": 1, "column": 0 }, "end": { "line": 1, "column": 21 } }
            }
        }),
        &serde_json::json!({ "0": 1 }),
    );

    let coverage = load_istanbul_coverage(
        std::path::Path::new("coverage/coverage-final.json"),
        None,
        Some(temp.path()),
        false,
    )
    .expect("relative path must resolve against project_root");
    assert!(
        !coverage.files.is_empty(),
        "expected coverage to load via project_root resolution, got {} files",
        coverage.files.len()
    );
}

#[test]
fn load_istanbul_coverage_falls_back_to_decl_line_for_missing_fn_line() {
    let temp = tempfile::TempDir::new().unwrap();
    let source_path = temp.path().join("src/service.ts");
    std::fs::create_dir_all(source_path.parent().unwrap()).unwrap();
    std::fs::write(&source_path, "export class DataService {}\n").unwrap();

    let coverage_path = temp.path().join("coverage-final.json");
    write_single_file_istanbul_fixture(
        &coverage_path,
        &source_path,
        &serde_json::json!({
            "0": {
                "name": "(anonymous_0)",
                "decl": {
                    "start": { "line": 5, "column": 2 },
                    "end": { "line": 5, "column": 13 }
                },
                "loc": {
                    "start": { "line": 5, "column": 14 },
                    "end": { "line": 11, "column": 3 }
                }
            },
            "1": {
                "name": "(anonymous_1)",
                "decl": {
                    "start": { "line": 20, "column": 14 },
                    "end": { "line": 20, "column": 25 }
                },
                "loc": {
                    "start": { "line": 20, "column": 28 },
                    "end": { "line": 22, "column": 2 }
                }
            }
        }),
        &serde_json::json!({
            "0": 1,
            "1": 0
        }),
    );

    let coverage = load_istanbul_coverage(&coverage_path, None, None, false).unwrap();
    let canonical_source = dunce::canonicalize(&source_path).unwrap();
    let file_coverage = coverage.get(&canonical_source).unwrap();

    assert_eq!(file_coverage.lookup("processData", 5, 0), Some(100.0));
    assert_eq!(file_coverage.lookup("handleSpecial", 20, 0), Some(0.0));
}

#[test]
fn load_istanbul_coverage_indexes_explicit_and_decl_lines() {
    let temp = tempfile::TempDir::new().unwrap();
    let source_path = temp.path().join("src/handler.ts");
    std::fs::create_dir_all(source_path.parent().unwrap()).unwrap();
    std::fs::write(&source_path, "export const handleClick = () => {}\n").unwrap();

    let coverage_path = temp.path().join("coverage-final.json");
    write_single_file_istanbul_fixture(
        &coverage_path,
        &source_path,
        &serde_json::json!({
            "0": {
                "name": "handleClick",
                "line": 40,
                "decl": {
                    "start": { "line": 22, "column": 13 },
                    "end": { "line": 22, "column": 24 }
                },
                "loc": {
                    "start": { "line": 40, "column": 27 },
                    "end": { "line": 42, "column": 1 }
                }
            }
        }),
        &serde_json::json!({
            "0": 1
        }),
    );

    let coverage = load_istanbul_coverage(&coverage_path, None, None, false).unwrap();
    let canonical_source = dunce::canonicalize(&source_path).unwrap();
    let file_coverage = coverage.get(&canonical_source).unwrap();

    assert_eq!(file_coverage.lookup("handleClick", 40, 0), Some(100.0));
    assert_eq!(file_coverage.lookup("handleClick", 22, 13), Some(100.0));
}

#[test]
fn load_istanbul_coverage_indexes_valid_body_start_alias() {
    let temp = tempfile::TempDir::new().unwrap();
    let source_path = temp.path().join("src/handler.ts");
    std::fs::create_dir_all(source_path.parent().unwrap()).unwrap();
    std::fs::write(&source_path, "export const handler = () => true;\n").unwrap();

    let coverage_path = temp.path().join("coverage-final.json");
    write_single_file_istanbul_fixture(
        &coverage_path,
        &source_path,
        &serde_json::json!({
            "0": {
                "name": "(anonymous_0)",
                "line": 8,
                "decl": {
                    "start": { "line": 8, "column": 14 },
                    "end": { "line": 8, "column": 25 }
                },
                "loc": {
                    "start": { "line": 20, "column": 6 },
                    "end": { "line": 22, "column": 1 }
                }
            }
        }),
        &serde_json::json!({ "0": 1 }),
    );

    let coverage = load_istanbul_coverage(&coverage_path, None, None, false).unwrap();
    let canonical_source = dunce::canonicalize(&source_path).unwrap();
    let file_coverage = coverage.get(&canonical_source).unwrap();

    assert_eq!(file_coverage.lookup("handler", 20, 6), Some(100.0));

    let mut function = make_fn_complexity(4);
    function.name = "handler".to_string();
    function.line = 20;
    function.col = 6;
    let result = istanbul_crap_default(&[function], Some(file_coverage), false);
    assert_eq!(result.matched, 1);
    assert_eq!(result.total, 1);
    assert_eq!(
        result.per_function[0].coverage_source,
        fallow_output::CoverageSource::Istanbul
    );
    assert_eq!(result.per_function[0].coverage_pct, Some(100.0));
}

/// A generator that a named arrow returns takes the arrow's name as a
/// context name, but the coverage producer records it without a name. The
/// name must not match the wrapper's record two columns away, so the
/// lookup treats the generator as anonymous.
#[test]
fn a_context_named_generator_takes_its_own_anonymous_record() {
    let temp = tempfile::TempDir::new().unwrap();
    let source_path = temp.path().join("src/load.ts");
    std::fs::create_dir_all(source_path.parent().unwrap()).unwrap();
    std::fs::write(
        &source_path,
        "export const load = (id: string) =>\n  Effect.gen(function* () {\n    if (id) {}\n  });\n",
    )
    .unwrap();

    let coverage_path = temp.path().join("coverage-final.json");
    write_single_file_istanbul_fixture(
        &coverage_path,
        &source_path,
        &serde_json::json!({
            "0": {
                "name": "load",
                "line": 1,
                "decl": { "start": { "line": 1, "column": 20 }, "end": { "line": 1, "column": 21 } },
                "loc": { "start": { "line": 1, "column": 20 }, "end": { "line": 4, "column": 4 } }
            },
            "1": {
                "name": "(anonymous_1)",
                "line": 2,
                "decl": { "start": { "line": 2, "column": 13 }, "end": { "line": 2, "column": 14 } },
                "loc": { "start": { "line": 2, "column": 13 }, "end": { "line": 4, "column": 3 } }
            }
        }),
        &serde_json::json!({ "0": 1, "1": 0 }),
    );

    let coverage = load_istanbul_coverage(&coverage_path, None, None, false).unwrap();
    let canonical_source = dunce::canonicalize(&source_path).unwrap();
    let file_coverage = coverage.get(&canonical_source).unwrap();

    let mut generator = make_fn_complexity(2);
    generator.name = "load".to_string();
    generator.line = 2;
    generator.col = 13;
    assert_eq!(
        file_coverage.lookup_function(&generator),
        Some(100.0),
        "a declared name matches the wrapper record"
    );
    generator.context_named = Some(fallow_types::extract::AnonymousFunctionKind::Function);
    assert_eq!(file_coverage.lookup_function(&generator), Some(0.0));
}

/// Curried arrows written one per line put each record's body start on
/// the next record's declaration. The declaration is primary and wins the
/// position, so each arrow keeps a record of its own and the innermost one
/// resolves through the header span that opens at the arrow above it.
/// Geometry from istanbul-lib-instrument 6 for the source below, which is
/// what Prettier produces for a curried arrow.
#[test]
fn curried_arrows_one_per_line_each_take_their_own_record() {
    let temp = tempfile::TempDir::new().unwrap();
    let source_path = temp.path().join("src/adjust.ts");
    std::fs::create_dir_all(source_path.parent().unwrap()).unwrap();
    std::fs::write(
            &source_path,
            "export const adjust = (base: number) =>\n  (factor: number) =>\n  (offset: number) =>\n    base * factor + offset;\n",
        )
        .unwrap();

    let coverage_path = temp.path().join("coverage-final.json");
    write_single_file_istanbul_fixture(
        &coverage_path,
        &source_path,
        &serde_json::json!({
            "0": {
                "name": "(anonymous_0)",
                "line": 2,
                "decl": { "start": { "line": 1, "column": 22 }, "end": { "line": 1, "column": 23 } },
                "loc": { "start": { "line": 2, "column": 2 }, "end": { "line": 4, "column": 26 } }
            },
            "1": {
                "name": "(anonymous_1)",
                "line": 3,
                "decl": { "start": { "line": 2, "column": 2 }, "end": { "line": 2, "column": 3 } },
                "loc": { "start": { "line": 3, "column": 2 }, "end": { "line": 4, "column": 26 } }
            },
            "2": {
                "name": "(anonymous_2)",
                "line": 4,
                "decl": { "start": { "line": 3, "column": 2 }, "end": { "line": 3, "column": 3 } },
                "loc": { "start": { "line": 4, "column": 4 }, "end": { "line": 4, "column": 26 } }
            }
        }),
        &serde_json::json!({ "0": 2, "1": 1, "2": 0 }),
    );

    let coverage = load_istanbul_coverage(&coverage_path, None, None, false).unwrap();
    let canonical_source = dunce::canonicalize(&source_path).unwrap();
    let file_coverage = coverage.get(&canonical_source).unwrap();

    assert_eq!(file_coverage.lookup("adjust", 1, 22), Some(100.0));
    assert_eq!(file_coverage.lookup("<arrow>", 2, 2), Some(100.0));
    // The innermost arrow never ran, and takes neither neighbour's value.
    assert_eq!(file_coverage.lookup("<arrow>", 3, 2), Some(0.0));
}

/// A default value in a parameter list is inside the member's signature,
/// and its own record can be anchored at the parameter rather than at the
/// function, putting the extracted position tens of columns from the
/// declaration. The record still owns that position, because its own
/// signature span covers it. Geometry from an @vitest/coverage-istanbul
/// map for a constructor whose parameter carries a default arrow.
#[test]
fn a_default_value_keeps_its_own_record_inside_the_enclosing_signature() {
    let temp = tempfile::TempDir::new().unwrap();
    let source_path = temp.path().join("src/filter.ts");
    std::fs::create_dir_all(source_path.parent().unwrap()).unwrap();
    std::fs::write(&source_path, "// geometry fixture\n").unwrap();

    let coverage_path = temp.path().join("coverage-final.json");
    write_single_file_istanbul_fixture(
        &coverage_path,
        &source_path,
        &serde_json::json!({
            "0": {
                "name": "(anonymous_0)",
                "line": 173,
                "decl": { "start": { "line": 169, "column": 2 }, "end": { "line": 170, "column": 3 } },
                "loc": { "start": { "line": 173, "column": 4 }, "end": { "line": 182, "column": 3 } }
            },
            "1": {
                "name": "(anonymous_1)",
                "line": 171,
                "decl": { "start": { "line": 171, "column": 21 }, "end": { "line": 171, "column": 67 } },
                "loc": { "start": { "line": 171, "column": 67 }, "end": { "line": 171, "column": 76 } }
            }
        }),
        &serde_json::json!({ "0": 46, "1": 0 }),
    );

    let coverage = load_istanbul_coverage(&coverage_path, None, None, false).unwrap();
    let canonical_source = dunce::canonicalize(&source_path).unwrap();
    let file_coverage = coverage.get(&canonical_source).unwrap();

    // Fallow extracts the arrow at its parameter paren, 40 columns right
    // of the record's declaration but inside the record's own span.
    assert_eq!(file_coverage.lookup("<arrow>", 171, 61), Some(0.0));
}

/// A named function in a signature is a second function inside the
/// member's header span, so the span identifies nothing on its own. A unit
/// with no record of its own, such as a private member or a unit the
/// producer names differently, must take the estimate rather than the
/// coverage of whichever record happens to be near. Geometry from
/// istanbul-lib-instrument 6 for the source below.
#[test]
fn header_span_abstains_when_another_function_is_declared_inside_it() {
    let temp = tempfile::TempDir::new().unwrap();
    let source_path = temp.path().join("src/chart.ts");
    std::fs::create_dir_all(source_path.parent().unwrap()).unwrap();
    std::fs::write(
            &source_path,
            "export class Chart {\n  @Watch(\"data\")\n  render(\n    rows: number[],\n    project = function scale(\n      row: number\n    ) {\n      return row * 2;\n    }\n  ) {\n    return rows.map(project);\n  }\n}\n",
        )
        .unwrap();

    let coverage_path = temp.path().join("coverage-final.json");
    write_single_file_istanbul_fixture(
        &coverage_path,
        &source_path,
        &serde_json::json!({
            "0": {
                "name": "(anonymous_0)",
                "line": 10,
                "decl": { "start": { "line": 2, "column": 2 }, "end": { "line": 2, "column": 3 } },
                "loc": { "start": { "line": 10, "column": 4 }, "end": { "line": 12, "column": 3 } }
            },
            "1": {
                "name": "scale",
                "line": 7,
                "decl": { "start": { "line": 5, "column": 23 }, "end": { "line": 5, "column": 28 } },
                "loc": { "start": { "line": 7, "column": 6 }, "end": { "line": 9, "column": 5 } }
            }
        }),
        &serde_json::json!({ "0": 3, "1": 0 }),
    );

    let canonical_source = dunce::canonicalize(&source_path).unwrap();
    let discovered_sources = rustc_hash::FxHashSet::from_iter([canonical_source.clone()]);
    let coverage = load_istanbul_coverage_for_sources(
        &coverage_path,
        None,
        Some(temp.path()),
        Some(&discovered_sources),
        false,
    )
    .unwrap();
    let file_coverage = coverage.get(&canonical_source).unwrap();

    // Inside both the member's header span and `scale`'s, so neither says
    // anything about this position.
    assert_eq!(file_coverage.lookup("<anonymous>", 6, 6), None);
    // Both records still resolve.
    assert_eq!(file_coverage.lookup("scale", 5, 14), Some(0.0));
    assert_eq!(file_coverage.lookup("render", 3, 8), Some(100.0));
}

/// A named function in a member signature can legally reuse the member's
/// name. Name fuzz must not return that inner function before established
/// anonymous resolution preserves the member's valid attribution.
#[test]
fn same_named_function_in_signature_does_not_supply_member_coverage() {
    let temp = tempfile::TempDir::new().unwrap();
    let source_path = temp.path().join("src/chart.ts");
    std::fs::create_dir_all(source_path.parent().unwrap()).unwrap();
    std::fs::write(
            &source_path,
            "export class Chart {\n  @Watch(\"data\")\n  render(\n    rows: number[],\n    project = function render(\n      row: number\n    ) {\n      return row * 2;\n    }\n  ) {\n    return rows.map(project);\n  }\n}\n",
        )
        .unwrap();

    let coverage_path = temp.path().join("coverage-final.json");
    write_single_file_istanbul_fixture(
        &coverage_path,
        &source_path,
        &serde_json::json!({
            "0": {
                "name": "(anonymous_0)",
                "line": 10,
                "decl": { "start": { "line": 2, "column": 2 }, "end": { "line": 2, "column": 3 } },
                "loc": { "start": { "line": 10, "column": 4 }, "end": { "line": 12, "column": 3 } }
            },
            "1": {
                "name": "render",
                "line": 7,
                "decl": { "start": { "line": 5, "column": 23 }, "end": { "line": 5, "column": 29 } },
                "loc": { "start": { "line": 7, "column": 6 }, "end": { "line": 9, "column": 5 } }
            }
        }),
        &serde_json::json!({ "0": 3, "1": 0 }),
    );

    let coverage = load_istanbul_coverage(&coverage_path, None, None, false).unwrap();
    let canonical_source = dunce::canonicalize(&source_path).unwrap();
    let file_coverage = coverage.get(&canonical_source).unwrap();

    assert_eq!(file_coverage.lookup("render", 3, 8), Some(100.0));
    assert_eq!(file_coverage.lookup("render", 5, 23), Some(0.0));
}

/// A same-line signature can place an unrelated identifier exactly one
/// function-expression prefix away from the member start. Column distance
/// alone must not make that nested function the member's coverage source.
#[test]
fn same_line_same_named_function_in_signature_does_not_supply_member_coverage() {
    let temp = tempfile::TempDir::new().unwrap();
    let source_path = temp.path().join("src/chart.ts");
    std::fs::create_dir_all(source_path.parent().unwrap()).unwrap();
    std::fs::write(
        &source_path,
        "export class Chart {\n  @Watch(\"data\") render(x  = function  render() {}) {}\n}\n",
    )
    .unwrap();

    let coverage_path = temp.path().join("coverage-final.json");
    write_single_file_istanbul_fixture(
        &coverage_path,
        &source_path,
        &serde_json::json!({
            "0": {
                "name": "(anonymous_0)",
                "line": 2,
                "decl": { "start": { "line": 2, "column": 2 }, "end": { "line": 2, "column": 3 } },
                "loc": { "start": { "line": 2, "column": 52 }, "end": { "line": 2, "column": 54 } }
            },
            "1": {
                "name": "render",
                "line": 2,
                "decl": { "start": { "line": 2, "column": 39 }, "end": { "line": 2, "column": 45 } },
                "loc": { "start": { "line": 2, "column": 48 }, "end": { "line": 2, "column": 50 } }
            }
        }),
        &serde_json::json!({ "0": 3, "1": 0 }),
    );

    let canonical_source = dunce::canonicalize(&source_path).unwrap();
    let discovered_sources = rustc_hash::FxHashSet::from_iter([canonical_source.clone()]);
    let coverage = load_istanbul_coverage_for_sources(
        &coverage_path,
        None,
        Some(temp.path()),
        Some(&discovered_sources),
        false,
    )
    .unwrap();
    let file_coverage = coverage.get(&canonical_source).unwrap();

    assert_eq!(file_coverage.lookup("render", 2, 23), Some(100.0));
    assert_eq!(file_coverage.lookup("render", 2, 29), Some(0.0));
}

#[test]
fn named_generator_alias_handles_trivia_and_utf16_columns() {
    let temp = tempfile::TempDir::new().unwrap();
    let source_path = temp.path().join("src/chart.ts");
    std::fs::create_dir_all(source_path.parent().unwrap()).unwrap();
    let source_line =
        "  render(label = \"pi: π, mushroom: 🍄\", x = function/* gap */*render() {}) {}";
    std::fs::write(
        &source_path,
        format!("export class Chart {{\n{source_line}\n}}\n"),
    )
    .unwrap();

    let utf16_column = |byte_column: usize| {
        u32::try_from(source_line[..byte_column].encode_utf16().count()).unwrap()
    };
    let outer_target_column = source_line.find("render(").unwrap() + "render".len();
    let syntax_column = source_line.find("function").unwrap();
    let name_column = source_line.find("*render").unwrap() + 1;
    let inner_body_column = source_line.find("{}").unwrap();
    let outer_body_column = source_line.rfind("{}").unwrap();

    let coverage_path = temp.path().join("coverage-final.json");
    write_single_file_istanbul_fixture(
        &coverage_path,
        &source_path,
        &serde_json::json!({
            "0": {
                "name": "(anonymous_0)",
                "line": 2,
                "decl": { "start": { "line": 2, "column": 2 }, "end": { "line": 2, "column": 3 } },
                "loc": {
                    "start": { "line": 2, "column": utf16_column(outer_body_column) },
                    "end": { "line": 2, "column": utf16_column(outer_body_column + 2) }
                }
            },
            "1": {
                "name": "render",
                "line": 2,
                "decl": {
                    "start": { "line": 2, "column": utf16_column(name_column) },
                    "end": { "line": 2, "column": utf16_column(name_column + "render".len()) }
                },
                "loc": {
                    "start": { "line": 2, "column": utf16_column(inner_body_column) },
                    "end": { "line": 2, "column": utf16_column(inner_body_column + 2) }
                }
            }
        }),
        &serde_json::json!({ "0": 3, "1": 0 }),
    );

    let canonical_source = dunce::canonicalize(&source_path).unwrap();
    let discovered_sources = rustc_hash::FxHashSet::from_iter([canonical_source.clone()]);
    let coverage = load_istanbul_coverage_for_sources(
        &coverage_path,
        None,
        Some(temp.path()),
        Some(&discovered_sources),
        false,
    )
    .unwrap();
    let file_coverage = coverage.get(&canonical_source).unwrap();

    assert_eq!(
        file_coverage.lookup("render", 2, u32::try_from(outer_target_column).unwrap()),
        Some(100.0)
    );
    assert_eq!(
        file_coverage.lookup("render", 2, u32::try_from(syntax_column).unwrap()),
        Some(0.0)
    );
}

#[test]
fn utf16_index_is_sparse_and_rejects_surrogate_boundaries() {
    let ascii_prefix = "a".repeat(4_096);
    let source = format!("{ascii_prefix}🍄{}", "b".repeat(4_096));
    let index = IstanbulSourceIndex::new(&source, std::path::Path::new("minified.js"));
    let line_index = &index.non_ascii_lines[&0];

    assert_eq!(line_index.checkpoints.len(), 1);
    assert_eq!(
        index.byte_position(1, 4_096),
        Some(IstanbulPosition::new(1, 4_096))
    );
    assert_eq!(index.byte_position(1, 4_097), None);
    assert_eq!(
        index.byte_position(1, 4_098),
        Some(IstanbulPosition::new(1, 4_100))
    );
    assert_eq!(
        index.byte_position(1, 8_194),
        Some(IstanbulPosition::new(1, 8_196))
    );
}

#[test]
fn effective_alias_normalizes_against_its_own_unicode_line() {
    let temp = tempfile::TempDir::new().unwrap();
    let source_path = temp.path().join("src/render.ts");
    std::fs::create_dir_all(source_path.parent().unwrap()).unwrap();
    std::fs::write(
        &source_path,
        "/*🍄*/ const placeholder = 0;\n/*π*/ function render() {}\n",
    )
    .unwrap();

    let coverage_path = temp.path().join("coverage-final.json");
    write_single_file_istanbul_fixture(
        &coverage_path,
        &source_path,
        &serde_json::json!({
            "0": {
                "name": "render",
                "line": 1,
                "decl": { "start": { "line": 2, "column": 15 }, "end": { "line": 2, "column": 21 } },
                "loc": { "start": { "line": 2, "column": 24 }, "end": { "line": 2, "column": 26 } }
            }
        }),
        &serde_json::json!({ "0": 1 }),
    );

    let canonical_source = dunce::canonicalize(&source_path).unwrap();
    let discovered_sources = rustc_hash::FxHashSet::from_iter([canonical_source.clone()]);
    let coverage = load_istanbul_coverage_for_sources(
        &coverage_path,
        None,
        Some(temp.path()),
        Some(&discovered_sources),
        false,
    )
    .unwrap();
    let function = &coverage.get(&canonical_source).unwrap().functions[0];

    assert!(
        function
            .aliases
            .iter()
            .any(|alias| { alias.position == IstanbulPosition::new(1, 17) && alias.primary })
    );
    assert!(
        !function
            .aliases
            .iter()
            .any(|alias| alias.position == IstanbulPosition::new(1, 16))
    );
}

#[test]
fn stale_utf16_coordinates_are_rejected_with_trusted_source() {
    let temp = tempfile::TempDir::new().unwrap();
    let source_path = temp.path().join("src/render.ts");
    std::fs::create_dir_all(source_path.parent().unwrap()).unwrap();
    std::fs::write(&source_path, "export function render() {}\n").unwrap();

    let coverage_path = temp.path().join("coverage-final.json");
    write_single_file_istanbul_fixture(
        &coverage_path,
        &source_path,
        &serde_json::json!({
            "0": {
                "name": "render",
                "line": 1,
                "decl": { "start": { "line": 1, "column": 999 }, "end": { "line": 1, "column": 22 } },
                "loc": { "start": { "line": 1, "column": 25 }, "end": { "line": 1, "column": 27 } }
            }
        }),
        &serde_json::json!({ "0": 1 }),
    );

    let canonical_source = dunce::canonicalize(&source_path).unwrap();
    let discovered_sources = rustc_hash::FxHashSet::from_iter([canonical_source.clone()]);
    let coverage = load_istanbul_coverage_for_sources(
        &coverage_path,
        None,
        Some(temp.path()),
        Some(&discovered_sources),
        false,
    )
    .unwrap();

    assert_eq!(
        coverage
            .get(&canonical_source)
            .unwrap()
            .lookup("render", 1, 7),
        None
    );
}

#[test]
fn invalid_optional_coordinates_preserve_valid_declaration_attribution() {
    let temp = tempfile::TempDir::new().unwrap();
    let source_path = temp.path().join("src/render.ts");
    std::fs::create_dir_all(source_path.parent().unwrap()).unwrap();
    std::fs::write(&source_path, "export function render() {}\n").unwrap();

    let coverage_path = temp.path().join("coverage-final.json");
    write_single_file_istanbul_fixture(
        &coverage_path,
        &source_path,
        &serde_json::json!({
            "0": {
                "name": "render",
                "line": 99,
                "decl": { "start": { "line": 1, "column": 16 }, "end": { "line": 99, "column": 999 } },
                "loc": { "start": { "line": 1, "column": 999 }, "end": { "line": 99, "column": 999 } }
            }
        }),
        &serde_json::json!({ "0": 1 }),
    );

    let canonical_source = dunce::canonicalize(&source_path).unwrap();
    let discovered_sources = rustc_hash::FxHashSet::from_iter([canonical_source.clone()]);
    let coverage = load_istanbul_coverage_for_sources(
        &coverage_path,
        None,
        Some(temp.path()),
        Some(&discovered_sources),
        false,
    )
    .unwrap();
    let file_coverage = coverage.get(&canonical_source).unwrap();
    let function = &file_coverage.functions[0];

    assert_eq!(file_coverage.lookup("render", 1, 16), Some(100.0));
    assert!(function.body_span.is_none());
    assert!(function.header_span.is_none());
    assert!(
        !function
            .aliases
            .iter()
            .any(|alias| { alias.position.line == 99 || alias.position.col == 999 })
    );
}

#[test]
fn malformed_source_does_not_supply_named_function_provenance() {
    assert!(
        !IstanbulSourceIndex::new(
            "export function render() {}",
            std::path::Path::new("valid.ts"),
        )
        .named_function_starts
        .is_empty()
    );
    let index = IstanbulSourceIndex::new(
        "export function render() {} const broken = ;",
        std::path::Path::new("broken.ts"),
    );

    assert!(index.named_function_starts.is_empty());
}

#[test]
fn javascript_with_jsx_uses_clean_jsx_provenance_parse() {
    let index = IstanbulSourceIndex::new(
        "export function render() { return <div />; }",
        std::path::Path::new("component.js"),
    );

    assert!(!index.named_function_starts.is_empty());
}

#[test]
fn undiscovered_coverage_path_is_not_loaded_for_source_provenance() {
    let temp = tempfile::TempDir::new().unwrap();
    let source_path = temp.path().join("src/excluded.ts");
    std::fs::create_dir_all(source_path.parent().unwrap()).unwrap();
    std::fs::write(&source_path, "export function render() {}\n").unwrap();

    let coverage_path = temp.path().join("coverage-final.json");
    write_single_file_istanbul_fixture(
        &coverage_path,
        &source_path,
        &serde_json::json!({
            "0": {
                "name": "render",
                "line": 1,
                "decl": { "start": { "line": 1, "column": 16 }, "end": { "line": 1, "column": 22 } },
                "loc": { "start": { "line": 1, "column": 25 }, "end": { "line": 1, "column": 27 } }
            }
        }),
        &serde_json::json!({ "0": 1 }),
    );

    let coverage = load_istanbul_coverage_for_sources(
        &coverage_path,
        None,
        Some(temp.path()),
        Some(&rustc_hash::FxHashSet::default()),
        false,
    )
    .unwrap();
    let canonical_source = dunce::canonicalize(&source_path).unwrap();
    let function = &coverage.get(&canonical_source).unwrap().functions[0];

    assert!(
        function
            .aliases
            .iter()
            .all(|alias| alias.position != IstanbulPosition::new(1, 7))
    );
}

/// A decorated member's `decl` opens at the decorator and its `loc` opens
/// at the body brace, so the extracted position sits between them with no
/// alias in reach: the decorator is more than
/// `ANONYMOUS_FALLBACK_MAX_COLUMN_DRIFT` columns to the left and the body
/// is more than `ALIAS_FUZZ_MAX_LINE_DRIFT` lines below. The header span
/// is what identifies the member. Geometry from istanbul-lib-instrument 6
/// for the source below, which is ordinary NestJS, Angular, and TypeORM
/// shape rather than a corner case.
#[test]
fn decorated_member_matches_its_istanbul_header_span() {
    let temp = tempfile::TempDir::new().unwrap();
    let source_path = temp.path().join("src/users.controller.ts");
    std::fs::create_dir_all(source_path.parent().unwrap()).unwrap();
    std::fs::write(
            &source_path,
            "export class UserController {\n  @Get(\":id\")\n  async findOneWithProfile(\n    id: string,\n    include: string[]\n  ) {\n    return this.service.find(id, include);\n  }\n}\n",
        )
        .unwrap();

    let coverage_path = temp.path().join("coverage-final.json");
    write_single_file_istanbul_fixture(
        &coverage_path,
        &source_path,
        &serde_json::json!({
            "0": {
                "name": "(anonymous_0)",
                "line": 6,
                "decl": { "start": { "line": 2, "column": 2 }, "end": { "line": 2, "column": 3 } },
                "loc": { "start": { "line": 6, "column": 4 }, "end": { "line": 8, "column": 3 } }
            }
        }),
        &serde_json::json!({ "0": 3 }),
    );

    let coverage = load_istanbul_coverage(&coverage_path, None, None, false).unwrap();
    let canonical_source = dunce::canonicalize(&source_path).unwrap();
    let file_coverage = coverage.get(&canonical_source).unwrap();

    // Fallow extracts the member at the parameter paren, 3:26.
    assert_eq!(
        file_coverage.lookup("findOneWithProfile", 3, 26),
        Some(100.0)
    );
}

/// A function written in a signature has a record of its own, and the
/// signature it sits in belongs to a different function. The record wins:
/// crediting the enclosing member would report the member's coverage for
/// a default value that never ran. Geometry from istanbul-lib-instrument 6
/// for the source below.
#[test]
fn established_alias_wins_over_the_signature_that_contains_it() {
    let temp = tempfile::TempDir::new().unwrap();
    let source_path = temp.path().join("src/chart.ts");
    std::fs::create_dir_all(source_path.parent().unwrap()).unwrap();
    std::fs::write(
            &source_path,
            "export class Chart {\n  @Watch(\"data\")\n  render(\n    rows: number[],\n    project = (row: number) => row * 2\n  ) {\n    return rows.map(project);\n  }\n}\n",
        )
        .unwrap();

    let coverage_path = temp.path().join("coverage-final.json");
    write_single_file_istanbul_fixture(
        &coverage_path,
        &source_path,
        &serde_json::json!({
            "0": {
                "name": "(anonymous_0)",
                "line": 6,
                "decl": { "start": { "line": 2, "column": 2 }, "end": { "line": 2, "column": 3 } },
                "loc": { "start": { "line": 6, "column": 4 }, "end": { "line": 8, "column": 3 } }
            },
            "1": {
                "name": "(anonymous_1)",
                "line": 5,
                "decl": { "start": { "line": 5, "column": 14 }, "end": { "line": 5, "column": 15 } },
                "loc": { "start": { "line": 5, "column": 31 }, "end": { "line": 5, "column": 38 } }
            }
        }),
        &serde_json::json!({ "0": 3, "1": 0 }),
    );

    let coverage = load_istanbul_coverage(&coverage_path, None, None, false).unwrap();
    let canonical_source = dunce::canonicalize(&source_path).unwrap();
    let file_coverage = coverage.get(&canonical_source).unwrap();

    // The default value takes its own record, not the method's 100.
    assert_eq!(file_coverage.lookup("<arrow>", 5, 14), Some(0.0));
    // The method still resolves, and not to its default value.
    assert_eq!(file_coverage.lookup("render", 3, 8), Some(100.0));
}

/// A member whose signature holds a function of its own has two records a
/// line or two apart, and the member's extracted position is closer to
/// the inner one. Proximity would report the default value's coverage for
/// the member, and the header span cannot break the tie because it
/// contains both. Abstaining leaves the static estimate, which is wrong by
/// a known amount rather than wrong while claiming to be measured.
/// Geometry from istanbul-lib-instrument 6 for the source below.
#[test]
fn signature_holding_a_function_abstains_instead_of_crediting_it() {
    let temp = tempfile::TempDir::new().unwrap();
    let source_path = temp.path().join("src/users.controller.ts");
    std::fs::create_dir_all(source_path.parent().unwrap()).unwrap();
    std::fs::write(
            &source_path,
            "export class UserController {\n  @Get(\":id\")\n  async findOneWithProfile(\n    id: string,\n    transform = (row: string) => row.trim()\n  ) {\n    return transform(id);\n  }\n}\n",
        )
        .unwrap();

    let coverage_path = temp.path().join("coverage-final.json");
    write_single_file_istanbul_fixture(
        &coverage_path,
        &source_path,
        &serde_json::json!({
            "0": {
                "name": "(anonymous_0)",
                "line": 6,
                "decl": { "start": { "line": 2, "column": 2 }, "end": { "line": 2, "column": 3 } },
                "loc": { "start": { "line": 6, "column": 4 }, "end": { "line": 8, "column": 3 } }
            },
            "1": {
                "name": "(anonymous_1)",
                "line": 5,
                "decl": { "start": { "line": 5, "column": 16 }, "end": { "line": 5, "column": 17 } },
                "loc": { "start": { "line": 5, "column": 33 }, "end": { "line": 5, "column": 43 } }
            }
        }),
        &serde_json::json!({ "0": 3, "1": 0 }),
    );

    let coverage = load_istanbul_coverage(&coverage_path, None, None, false).unwrap();
    let canonical_source = dunce::canonicalize(&source_path).unwrap();
    let file_coverage = coverage.get(&canonical_source).unwrap();

    // The member is two lines from the default value's declaration and
    // ten columns off it, well inside the fallback's reach.
    assert_eq!(file_coverage.lookup("findOneWithProfile", 3, 26), None);
    // The default value itself still resolves.
    assert_eq!(file_coverage.lookup("<arrow>", 5, 16), Some(0.0));
}

#[test]
fn anonymous_record_aliases_do_not_tie_with_their_own_identity() {
    let temp = tempfile::TempDir::new().unwrap();
    let source_path = temp.path().join("src/aliases.ts");
    std::fs::create_dir_all(source_path.parent().unwrap()).unwrap();
    std::fs::write(&source_path, "export const handler = () => true;\n").unwrap();

    let coverage_path = temp.path().join("coverage-final.json");
    write_single_file_istanbul_fixture(
        &coverage_path,
        &source_path,
        &serde_json::json!({
            "0": {
                "name": "(anonymous_0)",
                "line": 10,
                "decl": {
                    "start": { "line": 10, "column": 8 },
                    "end": { "line": 10, "column": 9 }
                },
                "loc": {
                    "start": { "line": 12, "column": 8 },
                    "end": { "line": 13, "column": 1 }
                }
            }
        }),
        &serde_json::json!({ "0": 1 }),
    );

    let coverage = load_istanbul_coverage(&coverage_path, None, None, false).unwrap();
    let canonical_source = dunce::canonicalize(&source_path).unwrap();
    let file_coverage = coverage.get(&canonical_source).unwrap();

    assert_eq!(file_coverage.lookup("handler", 11, 8), Some(100.0));
}

/// istanbul-lib-instrument geometry for
/// `export const nested = () => () => true;`: the outer arrow's `loc` is
/// its expression body, which starts exactly where the inner arrow is
/// declared.
#[test]
fn curried_arrow_one_liner_resolves_both_arrows() {
    let temp = tempfile::TempDir::new().unwrap();
    let source_path = temp.path().join("src/nested.ts");
    std::fs::create_dir_all(source_path.parent().unwrap()).unwrap();
    std::fs::write(&source_path, "export const nested = () => () => true;\n").unwrap();

    let coverage_path = temp.path().join("coverage-final.json");
    write_single_file_istanbul_fixture(
        &coverage_path,
        &source_path,
        &serde_json::json!({
            "0": {
                "name": "(anonymous_0)",
                "line": 1,
                "decl": {
                    "start": { "line": 1, "column": 22 },
                    "end": { "line": 1, "column": 23 }
                },
                "loc": {
                    "start": { "line": 1, "column": 28 },
                    "end": { "line": 1, "column": 38 }
                }
            },
            "1": {
                "name": "(anonymous_1)",
                "line": 1,
                "decl": {
                    "start": { "line": 1, "column": 28 },
                    "end": { "line": 1, "column": 29 }
                },
                "loc": {
                    "start": { "line": 1, "column": 34 },
                    "end": { "line": 1, "column": 38 }
                }
            }
        }),
        &serde_json::json!({ "0": 1, "1": 0 }),
    );

    let coverage = load_istanbul_coverage(&coverage_path, None, None, false).unwrap();
    let canonical_source = dunce::canonicalize(&source_path).unwrap();
    let file_coverage = coverage.get(&canonical_source).unwrap();

    assert_eq!(file_coverage.lookup("nested", 1, 22), Some(100.0));
    assert_eq!(file_coverage.lookup("<arrow>", 1, 28), Some(0.0));
}

/// A nested function body sits inside its enclosing function's `loc`, so
/// counting every contained statement charged the inner body's misses to
/// the outer record as well.
///
/// Geometry recorded verbatim from istanbul-lib-instrument 6.0.3 for:
///
/// ```js
/// function outer() {
///   return function inner(flag) {
///     if (flag) return "yes";
///     return "no";
///   };
/// }
///
/// module.exports = { outer };
/// ```
///
/// with `outer()` called once and `inner` never called. Statement 0 is
/// the outer return and is hit, statements 1 to 3 are the inner body and
/// are not, statement 4 is the module-scope export.
#[test]
fn nested_function_statements_do_not_lower_the_outer_function() {
    let temp = tempfile::TempDir::new().unwrap();
    let source_path = temp.path().join("src/nested.js");
    std::fs::create_dir_all(source_path.parent().unwrap()).unwrap();
    std::fs::write(&source_path, "// geometry fixture\n").unwrap();

    let coverage_path = temp.path().join("coverage-final.json");
    write_single_file_istanbul_fixture_with_statements(
        &coverage_path,
        &source_path,
        &nested_return_fn_map(),
        &serde_json::json!({ "0": 1, "1": 0 }),
        &nested_return_statement_map(),
        &serde_json::json!({ "0": 1, "1": 0, "2": 0, "3": 0, "4": 1 }),
    );

    let coverage = load_istanbul_coverage(&coverage_path, None, None, false).unwrap();
    let canonical_source = dunce::canonicalize(&source_path).unwrap();
    let file_coverage = coverage.get(&canonical_source).unwrap();

    assert_eq!(file_coverage.lookup("outer", 1, 9), Some(100.0));
    assert_eq!(file_coverage.lookup("inner", 2, 18), Some(0.0));
}

/// A statement outside every `fnMap` record belongs to no function. The
/// module-scope export in the fixture above is hit, and flipping it to
/// unhit must not move either function.
#[test]
fn module_scope_statements_belong_to_no_function() {
    let temp = tempfile::TempDir::new().unwrap();
    let source_path = temp.path().join("src/nested.js");
    std::fs::create_dir_all(source_path.parent().unwrap()).unwrap();
    std::fs::write(&source_path, "// geometry fixture\n").unwrap();

    let coverage_path = temp.path().join("coverage-final.json");
    write_single_file_istanbul_fixture_with_statements(
        &coverage_path,
        &source_path,
        &nested_return_fn_map(),
        &serde_json::json!({ "0": 1, "1": 0 }),
        &nested_return_statement_map(),
        &serde_json::json!({ "0": 1, "1": 0, "2": 0, "3": 0, "4": 0 }),
    );

    let coverage = load_istanbul_coverage(&coverage_path, None, None, false).unwrap();
    let canonical_source = dunce::canonicalize(&source_path).unwrap();
    let file_coverage = coverage.get(&canonical_source).unwrap();

    assert_eq!(file_coverage.lookup("outer", 1, 9), Some(100.0));
    assert_eq!(file_coverage.lookup("inner", 2, 18), Some(0.0));
}

/// Ownership can leave a record with no statements of its own, and that
/// record still has to fall back to the function hit count. Geometry
/// recorded verbatim from istanbul-lib-instrument 6.0.3 for:
///
/// ```js
/// function outer() {
///   function inner(flag) {
///     if (flag) return "yes";
///     return "no";
///   }
/// }
///
/// module.exports = { outer };
/// ```
///
/// with `outer()` called once. A hoisted declaration is not a statement,
/// so every statement inside `outer` belongs to `inner`.
#[test]
fn an_outer_function_with_only_a_nested_body_falls_back_to_the_hit_count() {
    let temp = tempfile::TempDir::new().unwrap();
    let source_path = temp.path().join("src/decl-nested.js");
    std::fs::create_dir_all(source_path.parent().unwrap()).unwrap();
    std::fs::write(&source_path, "// geometry fixture\n").unwrap();

    let coverage_path = temp.path().join("coverage-final.json");
    write_single_file_istanbul_fixture_with_statements(
        &coverage_path,
        &source_path,
        &serde_json::json!({
            "0": {
                "name": "outer",
                "line": 1,
                "decl": {
                    "start": { "line": 1, "column": 9 },
                    "end": { "line": 1, "column": 14 }
                },
                "loc": {
                    "start": { "line": 1, "column": 17 },
                    "end": { "line": 6, "column": 1 }
                }
            },
            "1": {
                "name": "inner",
                "line": 2,
                "decl": {
                    "start": { "line": 2, "column": 11 },
                    "end": { "line": 2, "column": 16 }
                },
                "loc": {
                    "start": { "line": 2, "column": 23 },
                    "end": { "line": 5, "column": 3 }
                }
            }
        }),
        &serde_json::json!({ "0": 1, "1": 0 }),
        &serde_json::json!({
            "0": {
                "start": { "line": 3, "column": 4 },
                "end": { "line": 3, "column": 27 }
            },
            "1": {
                "start": { "line": 3, "column": 14 },
                "end": { "line": 3, "column": 27 }
            },
            "2": {
                "start": { "line": 4, "column": 4 },
                "end": { "line": 4, "column": 16 }
            },
            "3": {
                "start": { "line": 8, "column": 0 },
                "end": { "line": 8, "column": 27 }
            }
        }),
        &serde_json::json!({ "0": 0, "1": 0, "2": 0, "3": 1 }),
    );

    let coverage = load_istanbul_coverage(&coverage_path, None, None, false).unwrap();
    let canonical_source = dunce::canonicalize(&source_path).unwrap();
    let file_coverage = coverage.get(&canonical_source).unwrap();

    assert_eq!(file_coverage.lookup("outer", 1, 9), Some(100.0));
    assert_eq!(file_coverage.lookup("inner", 2, 11), Some(0.0));
}

/// Two functions at module scope never contain each other, so ownership
/// must leave them exactly where they were. Geometry recorded verbatim
/// from istanbul-lib-instrument 6.0.3 for the same source with `inner`
/// hoisted out of `outer`.
#[test]
fn sibling_functions_keep_independent_statement_coverage() {
    let temp = tempfile::TempDir::new().unwrap();
    let source_path = temp.path().join("src/sibling.js");
    std::fs::create_dir_all(source_path.parent().unwrap()).unwrap();
    std::fs::write(&source_path, "// geometry fixture\n").unwrap();

    let coverage_path = temp.path().join("coverage-final.json");
    write_single_file_istanbul_fixture_with_statements(
        &coverage_path,
        &source_path,
        &serde_json::json!({
            "0": {
                "name": "inner",
                "line": 1,
                "decl": {
                    "start": { "line": 1, "column": 9 },
                    "end": { "line": 1, "column": 14 }
                },
                "loc": {
                    "start": { "line": 1, "column": 21 },
                    "end": { "line": 4, "column": 1 }
                }
            },
            "1": {
                "name": "outer",
                "line": 6,
                "decl": {
                    "start": { "line": 6, "column": 9 },
                    "end": { "line": 6, "column": 14 }
                },
                "loc": {
                    "start": { "line": 6, "column": 17 },
                    "end": { "line": 8, "column": 1 }
                }
            }
        }),
        &serde_json::json!({ "0": 0, "1": 1 }),
        &serde_json::json!({
            "0": {
                "start": { "line": 2, "column": 2 },
                "end": { "line": 2, "column": 25 }
            },
            "1": {
                "start": { "line": 2, "column": 12 },
                "end": { "line": 2, "column": 25 }
            },
            "2": {
                "start": { "line": 3, "column": 2 },
                "end": { "line": 3, "column": 14 }
            },
            "3": {
                "start": { "line": 7, "column": 2 },
                "end": { "line": 7, "column": 15 }
            },
            "4": {
                "start": { "line": 10, "column": 0 },
                "end": { "line": 10, "column": 27 }
            }
        }),
        &serde_json::json!({ "0": 0, "1": 0, "2": 0, "3": 1, "4": 1 }),
    );

    let coverage = load_istanbul_coverage(&coverage_path, None, None, false).unwrap();
    let canonical_source = dunce::canonicalize(&source_path).unwrap();
    let file_coverage = coverage.get(&canonical_source).unwrap();

    assert_eq!(file_coverage.lookup("outer", 6, 9), Some(100.0));
    assert_eq!(file_coverage.lookup("inner", 1, 9), Some(0.0));
}

/// Curried arrows share their closing position, so the inner body is
/// distinguished only by its later start. Geometry recorded verbatim from
/// istanbul-lib-instrument 6.0.3 for `const add = (a) => (b) => a + b;`
/// with `add(1)` called once, so the outer arrow ran and the inner did
/// not. Statement 1 is the outer expression body, statement 2 the inner.
#[test]
fn curried_arrow_statements_belong_to_the_innermost_arrow() {
    let temp = tempfile::TempDir::new().unwrap();
    let source_path = temp.path().join("src/curried.js");
    std::fs::create_dir_all(source_path.parent().unwrap()).unwrap();
    std::fs::write(&source_path, "// geometry fixture\n").unwrap();

    let coverage_path = temp.path().join("coverage-final.json");
    write_single_file_istanbul_fixture_with_statements(
        &coverage_path,
        &source_path,
        &serde_json::json!({
            "0": {
                "name": "(anonymous_0)",
                "line": 1,
                "decl": {
                    "start": { "line": 1, "column": 12 },
                    "end": { "line": 1, "column": 13 }
                },
                "loc": {
                    "start": { "line": 1, "column": 19 },
                    "end": { "line": 1, "column": 31 }
                }
            },
            "1": {
                "name": "(anonymous_1)",
                "line": 1,
                "decl": {
                    "start": { "line": 1, "column": 19 },
                    "end": { "line": 1, "column": 20 }
                },
                "loc": {
                    "start": { "line": 1, "column": 26 },
                    "end": { "line": 1, "column": 31 }
                }
            }
        }),
        &serde_json::json!({ "0": 1, "1": 0 }),
        &serde_json::json!({
            "0": {
                "start": { "line": 1, "column": 12 },
                "end": { "line": 1, "column": 31 }
            },
            "1": {
                "start": { "line": 1, "column": 19 },
                "end": { "line": 1, "column": 31 }
            },
            "2": {
                "start": { "line": 1, "column": 26 },
                "end": { "line": 1, "column": 31 }
            },
            "3": {
                "start": { "line": 3, "column": 0 },
                "end": { "line": 3, "column": 25 }
            }
        }),
        &serde_json::json!({ "0": 1, "1": 1, "2": 0, "3": 1 }),
    );

    let coverage = load_istanbul_coverage(&coverage_path, None, None, false).unwrap();
    let canonical_source = dunce::canonicalize(&source_path).unwrap();
    let file_coverage = coverage.get(&canonical_source).unwrap();

    assert_eq!(file_coverage.lookup("add", 1, 12), Some(100.0));
    assert_eq!(file_coverage.lookup("<arrow>", 1, 19), Some(0.0));
}

/// The `fnMap` and `statementMap` istanbul-lib-instrument 6.0.3 records
/// for the nested-return source, shared by the two tests that pin
/// statement ownership against it.
fn nested_return_fn_map() -> serde_json::Value {
    serde_json::json!({
        "0": {
            "name": "outer",
            "line": 1,
            "decl": {
                "start": { "line": 1, "column": 9 },
                "end": { "line": 1, "column": 14 }
            },
            "loc": {
                "start": { "line": 1, "column": 17 },
                "end": { "line": 6, "column": 1 }
            }
        },
        "1": {
            "name": "inner",
            "line": 2,
            "decl": {
                "start": { "line": 2, "column": 18 },
                "end": { "line": 2, "column": 23 }
            },
            "loc": {
                "start": { "line": 2, "column": 30 },
                "end": { "line": 5, "column": 3 }
            }
        }
    })
}

fn nested_return_statement_map() -> serde_json::Value {
    serde_json::json!({
        "0": {
            "start": { "line": 2, "column": 2 },
            "end": { "line": 5, "column": 4 }
        },
        "1": {
            "start": { "line": 3, "column": 4 },
            "end": { "line": 3, "column": 27 }
        },
        "2": {
            "start": { "line": 3, "column": 14 },
            "end": { "line": 3, "column": 27 }
        },
        "3": {
            "start": { "line": 4, "column": 4 },
            "end": { "line": 4, "column": 16 }
        },
        "4": {
            "start": { "line": 8, "column": 0 },
            "end": { "line": 8, "column": 27 }
        }
    })
}

/// istanbul-lib-instrument geometry for a multi-line higher-order
/// component. The producer's `line` is the body start line, so the outer
/// record also carries an effective alias on line 2.
#[test]
fn curried_arrow_multiline_hoc_resolves_both_arrows() {
    let temp = tempfile::TempDir::new().unwrap();
    let source_path = temp.path().join("src/with-auth.tsx");
    std::fs::create_dir_all(source_path.parent().unwrap()).unwrap();
    std::fs::write(
            &source_path,
            "export const withAuth = (Component) =>\n  (props) => {\n    return Component(props);\n  };\n",
        )
        .unwrap();

    let coverage_path = temp.path().join("coverage-final.json");
    write_single_file_istanbul_fixture(
        &coverage_path,
        &source_path,
        &serde_json::json!({
            "0": {
                "name": "(anonymous_0)",
                "line": 2,
                "decl": {
                    "start": { "line": 1, "column": 24 },
                    "end": { "line": 1, "column": 25 }
                },
                "loc": {
                    "start": { "line": 2, "column": 2 },
                    "end": { "line": 4, "column": 3 }
                }
            },
            "1": {
                "name": "(anonymous_1)",
                "line": 2,
                "decl": {
                    "start": { "line": 2, "column": 2 },
                    "end": { "line": 2, "column": 3 }
                },
                "loc": {
                    "start": { "line": 2, "column": 13 },
                    "end": { "line": 4, "column": 3 }
                }
            }
        }),
        &serde_json::json!({ "0": 1, "1": 0 }),
    );

    let coverage = load_istanbul_coverage(&coverage_path, None, None, false).unwrap();
    let canonical_source = dunce::canonicalize(&source_path).unwrap();
    let file_coverage = coverage.get(&canonical_source).unwrap();

    assert_eq!(file_coverage.lookup("withAuth", 1, 24), Some(100.0));
    assert_eq!(file_coverage.lookup("<arrow>", 2, 2), Some(0.0));
}

/// istanbul-lib-instrument geometry for a depth-3 redux middleware chain.
/// Every non-first arrow is declared where the previous body starts.
#[test]
fn curried_arrow_depth_three_chain_resolves_every_arrow() {
    let temp = tempfile::TempDir::new().unwrap();
    let source_path = temp.path().join("src/logger.ts");
    std::fs::create_dir_all(source_path.parent().unwrap()).unwrap();
    std::fs::write(
        &source_path,
        "export const logger = (store) => (next) => (action) => {\n  return next(action);\n};\n",
    )
    .unwrap();

    let coverage_path = temp.path().join("coverage-final.json");
    write_single_file_istanbul_fixture(
        &coverage_path,
        &source_path,
        &serde_json::json!({
            "0": {
                "name": "(anonymous_0)",
                "line": 1,
                "decl": {
                    "start": { "line": 1, "column": 22 },
                    "end": { "line": 1, "column": 23 }
                },
                "loc": {
                    "start": { "line": 1, "column": 33 },
                    "end": { "line": 3, "column": 1 }
                }
            },
            "1": {
                "name": "(anonymous_1)",
                "line": 1,
                "decl": {
                    "start": { "line": 1, "column": 33 },
                    "end": { "line": 1, "column": 34 }
                },
                "loc": {
                    "start": { "line": 1, "column": 43 },
                    "end": { "line": 3, "column": 1 }
                }
            },
            "2": {
                "name": "(anonymous_2)",
                "line": 1,
                "decl": {
                    "start": { "line": 1, "column": 43 },
                    "end": { "line": 1, "column": 44 }
                },
                "loc": {
                    "start": { "line": 1, "column": 55 },
                    "end": { "line": 3, "column": 1 }
                }
            }
        }),
        &serde_json::json!({ "0": 0, "1": 1, "2": 0 }),
    );

    let coverage = load_istanbul_coverage(&coverage_path, None, None, false).unwrap();
    let canonical_source = dunce::canonicalize(&source_path).unwrap();
    let file_coverage = coverage.get(&canonical_source).unwrap();

    assert_eq!(file_coverage.lookup("logger", 1, 22), Some(0.0));
    assert_eq!(file_coverage.lookup("<arrow>", 1, 33), Some(100.0));
    assert_eq!(file_coverage.lookup("<arrow>", 1, 43), Some(0.0));
}

/// istanbul-lib-instrument geometry for a curried class-property arrow.
#[test]
fn curried_class_property_arrow_resolves_both_arrows() {
    let temp = tempfile::TempDir::new().unwrap();
    let source_path = temp.path().join("src/store.ts");
    std::fs::create_dir_all(source_path.parent().unwrap()).unwrap();
    std::fs::write(
            &source_path,
            "export class Store {\n  handle = (event) => (payload) => {\n    return payload;\n  };\n}\n",
        )
        .unwrap();

    let coverage_path = temp.path().join("coverage-final.json");
    write_single_file_istanbul_fixture(
        &coverage_path,
        &source_path,
        &serde_json::json!({
            "0": {
                "name": "(anonymous_0)",
                "line": 2,
                "decl": {
                    "start": { "line": 2, "column": 11 },
                    "end": { "line": 2, "column": 12 }
                },
                "loc": {
                    "start": { "line": 2, "column": 22 },
                    "end": { "line": 4, "column": 3 }
                }
            },
            "1": {
                "name": "(anonymous_1)",
                "line": 2,
                "decl": {
                    "start": { "line": 2, "column": 22 },
                    "end": { "line": 2, "column": 23 }
                },
                "loc": {
                    "start": { "line": 2, "column": 35 },
                    "end": { "line": 4, "column": 3 }
                }
            }
        }),
        &serde_json::json!({ "0": 1, "1": 0 }),
    );

    let coverage = load_istanbul_coverage(&coverage_path, None, None, false).unwrap();
    let canonical_source = dunce::canonicalize(&source_path).unwrap();
    let file_coverage = coverage.get(&canonical_source).unwrap();

    assert_eq!(file_coverage.lookup("handle", 2, 11), Some(100.0));
    assert_eq!(file_coverage.lookup("<arrow>", 2, 22), Some(0.0));
}

/// istanbul-lib-instrument geometry for two sibling arrows in an object
/// literal. A target one line away from both, at the shared column, ties
/// on distance and lies in neither body, so the lookup abstains.
#[test]
fn anonymous_sibling_tie_outside_every_body_abstains() {
    let temp = tempfile::TempDir::new().unwrap();
    let source_path = temp.path().join("src/handlers.ts");
    std::fs::create_dir_all(source_path.parent().unwrap()).unwrap();
    std::fs::write(
        &source_path,
        "export const handlers = {\n  a: () => true,\n\n  b: () => false,\n};\n",
    )
    .unwrap();

    let coverage_path = temp.path().join("coverage-final.json");
    write_single_file_istanbul_fixture(
        &coverage_path,
        &source_path,
        &serde_json::json!({
            "0": {
                "name": "(anonymous_0)",
                "line": 2,
                "decl": {
                    "start": { "line": 2, "column": 5 },
                    "end": { "line": 2, "column": 6 }
                },
                "loc": {
                    "start": { "line": 2, "column": 11 },
                    "end": { "line": 2, "column": 15 }
                }
            },
            "1": {
                "name": "(anonymous_1)",
                "line": 4,
                "decl": {
                    "start": { "line": 4, "column": 5 },
                    "end": { "line": 4, "column": 6 }
                },
                "loc": {
                    "start": { "line": 4, "column": 11 },
                    "end": { "line": 4, "column": 16 }
                }
            }
        }),
        &serde_json::json!({ "0": 1, "1": 0 }),
    );

    let coverage = load_istanbul_coverage(&coverage_path, None, None, false).unwrap();
    let canonical_source = dunce::canonicalize(&source_path).unwrap();
    let file_coverage = coverage.get(&canonical_source).unwrap();

    assert_eq!(file_coverage.lookup("a", 2, 5), Some(100.0));
    assert_eq!(file_coverage.lookup("b", 4, 5), Some(0.0));
    assert!(file_coverage.lookup("<arrow>", 3, 5).is_none());
}

/// istanbul-lib-instrument geometry for a function expression whose block
/// body wraps an arrow, with the closing braces on a second line. A target
/// on that line, equidistant from the outer `{` and the inner declaration,
/// ties on distance and lies inside both bodies; the strictly innermost
/// body wins.
#[test]
fn anonymous_tie_selects_unique_strictly_innermost_containing_span() {
    let temp = tempfile::TempDir::new().unwrap();
    let source_path = temp.path().join("src/nested.ts");
    std::fs::create_dir_all(source_path.parent().unwrap()).unwrap();
    std::fs::write(
            &source_path,
            "export const o = function () { const f = () => { return 1;\n                                       }; return f; };\n",
        )
        .unwrap();

    let coverage_path = temp.path().join("coverage-final.json");
    write_single_file_istanbul_fixture(
        &coverage_path,
        &source_path,
        &serde_json::json!({
            "0": {
                "name": "(anonymous_0)",
                "line": 1,
                "decl": {
                    "start": { "line": 1, "column": 17 },
                    "end": { "line": 1, "column": 18 }
                },
                "loc": {
                    "start": { "line": 1, "column": 29 },
                    "end": { "line": 2, "column": 53 }
                }
            },
            "1": {
                "name": "(anonymous_1)",
                "line": 1,
                "decl": {
                    "start": { "line": 1, "column": 41 },
                    "end": { "line": 1, "column": 42 }
                },
                "loc": {
                    "start": { "line": 1, "column": 47 },
                    "end": { "line": 2, "column": 40 }
                }
            }
        }),
        &serde_json::json!({ "0": 1, "1": 0 }),
    );

    let coverage = load_istanbul_coverage(&coverage_path, None, None, false).unwrap();
    let canonical_source = dunce::canonicalize(&source_path).unwrap();
    let file_coverage = coverage.get(&canonical_source).unwrap();

    assert_eq!(file_coverage.lookup("<arrow>", 2, 35), Some(0.0));
}

/// Defensive: no producer emits partially overlapping bodies, but a tie
/// between two containing spans that are not nested must not pick either.
#[test]
fn anonymous_tie_rejects_incomparable_containing_spans() {
    let file_coverage = IstanbulFileCoverage::new(
        vec![
            IstanbulFunctionCoverage {
                name: "(anonymous_0)".to_string(),
                coverage_pct: 100.0,
                aliases: vec![primary_alias(10, 8), secondary_alias(10, 14)],
                decl_start: IstanbulPosition::new(10, 8),
                header_holds_other_fn: false,
                header_span: None,
                body_span: Some(body_span((10, 14), (12, 30))),
            },
            IstanbulFunctionCoverage {
                name: "(anonymous_1)".to_string(),
                coverage_pct: 0.0,
                aliases: vec![primary_alias(10, 20), secondary_alias(11, 0)],
                decl_start: IstanbulPosition::new(10, 20),
                header_holds_other_fn: false,
                header_span: None,
                body_span: Some(body_span((11, 0), (14, 0))),
            },
        ],
        false,
    );

    assert!(file_coverage.lookup("<arrow>", 12, 17).is_none());
}

/// Two records whose primary aliases coincide (a multi-line parameter
/// list whose body-start line and declaration column produce the same
/// effective position as the inner arrow's declaration) stay ambiguous
/// even though their bodies nest.
#[test]
fn anonymous_shared_primary_alias_rejects_even_nested_spans() {
    let file_coverage = IstanbulFileCoverage::new(
        vec![
            IstanbulFunctionCoverage {
                name: "(anonymous_0)".to_string(),
                coverage_pct: 100.0,
                aliases: vec![primary_alias(4, 11), primary_alias(1, 11)],
                decl_start: IstanbulPosition::new(4, 11),
                header_holds_other_fn: false,
                header_span: None,
                body_span: Some(body_span((4, 11), (4, 23))),
            },
            IstanbulFunctionCoverage {
                name: "(anonymous_1)".to_string(),
                coverage_pct: 0.0,
                aliases: vec![primary_alias(4, 11), secondary_alias(4, 18)],
                decl_start: IstanbulPosition::new(4, 11),
                header_holds_other_fn: false,
                header_span: None,
                body_span: Some(body_span((4, 18), (4, 23))),
            },
        ],
        false,
    );

    assert!(file_coverage.lookup("<arrow>", 4, 11).is_none());
    assert_eq!(file_coverage.lookup("aa", 1, 11), Some(100.0));
}

#[test]
fn colliding_anonymous_alias_uses_unique_safe_header_span() {
    let file_coverage = IstanbulFileCoverage::new(
        vec![
            IstanbulFunctionCoverage {
                name: "(anonymous_0)".to_string(),
                coverage_pct: 100.0,
                aliases: vec![primary_alias(1, 0), secondary_alias(3, 4)],
                decl_start: IstanbulPosition::new(1, 0),
                header_holds_other_fn: false,
                header_span: Some(body_span((1, 0), (5, 0))),
                body_span: Some(body_span((5, 0), (8, 0))),
            },
            IstanbulFunctionCoverage {
                name: "(anonymous_1)".to_string(),
                coverage_pct: 0.0,
                aliases: vec![primary_alias(10, 0), secondary_alias(3, 4)],
                decl_start: IstanbulPosition::new(10, 0),
                header_holds_other_fn: false,
                header_span: None,
                body_span: Some(body_span((10, 0), (12, 0))),
            },
        ],
        false,
    );

    assert_eq!(file_coverage.lookup("<arrow>", 3, 4), Some(100.0));
}

/// A secondary alias that collides with another record's secondary alias
/// is dropped from both, and the shared position remains ambiguous.
#[test]
fn colliding_secondary_aliases_abstain_at_shared_position() {
    let file_coverage = IstanbulFileCoverage::new(
        vec![
            IstanbulFunctionCoverage {
                name: "(anonymous_0)".to_string(),
                coverage_pct: 100.0,
                aliases: vec![primary_alias(10, 0), secondary_alias(12, 4)],
                decl_start: IstanbulPosition::new(10, 0),
                header_holds_other_fn: false,
                header_span: None,
                body_span: Some(body_span((12, 4), (20, 0))),
            },
            IstanbulFunctionCoverage {
                name: "(anonymous_1)".to_string(),
                coverage_pct: 0.0,
                aliases: vec![primary_alias(11, 0), secondary_alias(12, 4)],
                decl_start: IstanbulPosition::new(11, 0),
                header_holds_other_fn: false,
                header_span: None,
                body_span: Some(body_span((12, 4), (18, 0))),
            },
        ],
        false,
    );

    assert_eq!(file_coverage.lookup("first", 10, 0), Some(100.0));
    assert_eq!(file_coverage.lookup("second", 11, 0), Some(0.0));
    assert!(file_coverage.lookup("<arrow>", 12, 4).is_none());
}

#[test]
fn colliding_named_secondary_aliases_abstain_at_shared_position() {
    let file_coverage = IstanbulFileCoverage::new(
        vec![
            IstanbulFunctionCoverage {
                name: "handler".to_string(),
                coverage_pct: 100.0,
                aliases: vec![primary_alias(10, 0), secondary_alias(12, 4)],
                decl_start: IstanbulPosition::new(10, 0),
                header_holds_other_fn: false,
                header_span: None,
                body_span: Some(body_span((12, 4), (20, 0))),
            },
            IstanbulFunctionCoverage {
                name: "handler".to_string(),
                coverage_pct: 0.0,
                aliases: vec![primary_alias(11, 0), secondary_alias(12, 4)],
                decl_start: IstanbulPosition::new(11, 0),
                header_holds_other_fn: false,
                header_span: None,
                body_span: Some(body_span((12, 4), (18, 0))),
            },
        ],
        false,
    );

    assert_eq!(file_coverage.lookup("handler", 10, 0), Some(100.0));
    assert_eq!(file_coverage.lookup("handler", 11, 0), Some(0.0));
    assert!(file_coverage.lookup("handler", 12, 4).is_none());
}

#[test]
fn invalid_body_location_does_not_create_an_alias() {
    let temp = tempfile::TempDir::new().unwrap();
    let source_path = temp.path().join("src/invalid-location.ts");
    std::fs::create_dir_all(source_path.parent().unwrap()).unwrap();
    std::fs::write(&source_path, "export const handler = () => true;\n").unwrap();

    let coverage_path = temp.path().join("coverage-final.json");
    write_single_file_istanbul_fixture(
        &coverage_path,
        &source_path,
        &serde_json::json!({
            "0": {
                "name": "(anonymous_0)",
                "line": 8,
                "decl": {
                    "start": { "line": 8, "column": 14 },
                    "end": { "line": 8, "column": 25 }
                },
                "loc": {
                    "start": { "line": 22, "column": 1 },
                    "end": { "line": 20, "column": 6 }
                }
            }
        }),
        &serde_json::json!({ "0": 1 }),
    );

    let coverage = load_istanbul_coverage(&coverage_path, None, None, false).unwrap();
    let canonical_source = dunce::canonicalize(&source_path).unwrap();
    let file_coverage = coverage.get(&canonical_source).unwrap();

    assert!(file_coverage.lookup("handler", 22, 1).is_none());
}

#[test]
fn load_istanbul_coverage_matches_multiline_async_arrow_decl_alias() {
    let temp = tempfile::TempDir::new().unwrap();
    let source_path = temp.path().join("src/actor.ts");
    std::fs::create_dir_all(source_path.parent().unwrap()).unwrap();
    std::fs::write(
            &source_path,
            "export const elementsFrom = async (\n  locator: AnyLocator,\n  options?: { missingAsEmpty?: boolean },\n): Promise<HTMLElement[]> => {\n  return [];\n};\n",
        )
        .unwrap();

    let coverage_path = temp.path().join("coverage-final.json");
    write_single_file_istanbul_fixture(
        &coverage_path,
        &source_path,
        &serde_json::json!({
            "0": {
                "name": "(anonymous_0)",
                "line": 4,
                "decl": {
                    "start": { "line": 1, "column": 28 },
                    "end": { "line": 4, "column": 26 }
                },
                "loc": {
                    "start": { "line": 4, "column": 27 },
                    "end": { "line": 6, "column": 1 }
                }
            }
        }),
        &serde_json::json!({
            "0": 642
        }),
    );

    let coverage = load_istanbul_coverage(&coverage_path, None, None, false).unwrap();
    let canonical_source = dunce::canonicalize(&source_path).unwrap();
    let file_coverage = coverage.get(&canonical_source).unwrap();

    assert_eq!(file_coverage.lookup("elementsFrom", 1, 28), Some(100.0));
}

#[test]
fn istanbul_lookup_exact_match() {
    let mut functions = rustc_hash::FxHashMap::default();
    functions.insert(("handleClick".to_string(), 10, 0), 85.0);
    let fc = test_istanbul_file_coverage(functions, false);
    assert!((fc.lookup("handleClick", 10, 0).unwrap() - 85.0).abs() < f64::EPSILON);
}

#[test]
fn istanbul_lookup_fuzzy_match_within_offset() {
    let mut functions = rustc_hash::FxHashMap::default();
    functions.insert(("handleClick".to_string(), 10, 0), 72.0);
    let fc = test_istanbul_file_coverage(functions, false);
    assert!((fc.lookup("handleClick", 11, 0).unwrap() - 72.0).abs() < f64::EPSILON);
    assert!((fc.lookup("handleClick", 12, 0).unwrap() - 72.0).abs() < f64::EPSILON);
}

#[test]
fn istanbul_lookup_fuzzy_match_outside_offset() {
    let mut functions = rustc_hash::FxHashMap::default();
    functions.insert(("handleClick".to_string(), 10, 0), 72.0);
    let fc = test_istanbul_file_coverage(functions, false);
    assert!(fc.lookup("handleClick", 13, 0).is_none());
}

#[test]
fn istanbul_lookup_relocated_matches_unique_name_at_any_distance() {
    let mut functions = rustc_hash::FxHashMap::default();
    functions.insert(("handleClick".to_string(), 29, 0), 72.0);
    let fc = test_istanbul_file_coverage(functions, true);
    assert!((fc.lookup("handleClick", 10, 0).unwrap() - 72.0).abs() < f64::EPSILON);
}

#[test]
fn istanbul_lookup_relocated_accepts_declaration_alias_pair() {
    let mut functions = rustc_hash::FxHashMap::default();
    functions.insert(("handleClick".to_string(), 29, 16), 72.0);
    functions.insert(("handleClick".to_string(), 29, 0), 72.0);
    let fc = test_istanbul_file_coverage(functions, true);
    assert!((fc.lookup("handleClick", 10, 0).unwrap() - 72.0).abs() < f64::EPSILON);
}

#[test]
fn istanbul_lookup_relocated_bails_on_disagreeing_same_name_entries() {
    let mut functions = rustc_hash::FxHashMap::default();
    functions.insert(("render".to_string(), 29, 0), 72.0);
    functions.insert(("render".to_string(), 80, 0), 10.0);
    let fc = test_istanbul_file_coverage(functions, true);
    assert!(fc.lookup("render", 10, 0).is_none());
}

#[test]
fn istanbul_lookup_relocated_prefers_bounded_fuzzy_match() {
    let mut functions = rustc_hash::FxHashMap::default();
    functions.insert(("render".to_string(), 11, 0), 72.0);
    functions.insert(("render".to_string(), 80, 0), 10.0);
    let fc = test_istanbul_file_coverage(functions, true);
    assert!((fc.lookup("render", 10, 0).unwrap() - 72.0).abs() < f64::EPSILON);
}

#[test]
fn istanbul_lookup_name_mismatch() {
    let mut functions = rustc_hash::FxHashMap::default();
    functions.insert(("handleClick".to_string(), 10, 0), 85.0);
    let fc = test_istanbul_file_coverage(functions, false);
    assert!(fc.lookup("handleSubmit", 10, 0).is_none());
}

#[test]
fn istanbul_lookup_empty() {
    let fc = test_istanbul_file_coverage(rustc_hash::FxHashMap::default(), false);
    assert!(fc.lookup("anything", 1, 0).is_none());
}

#[test]
fn istanbul_lookup_fuzzy_picks_closest() {
    let mut functions = rustc_hash::FxHashMap::default();
    functions.insert(("render".to_string(), 8, 0), 60.0);
    functions.insert(("render".to_string(), 12, 0), 90.0);
    let fc = test_istanbul_file_coverage(functions, false);
    let result = fc.lookup("render", 10, 0);
    assert!(result.is_some());
    let pct = result.unwrap();
    assert!((pct - 60.0).abs() < f64::EPSILON || (pct - 90.0).abs() < f64::EPSILON);
}

#[test]
fn istanbul_lookup_anonymous_fallback_single_candidate() {
    let mut functions = rustc_hash::FxHashMap::default();
    functions.insert(("(anonymous_0)".to_string(), 28, 0), 75.0);
    let fc = test_istanbul_file_coverage(functions, false);
    assert!((fc.lookup("myHandler", 28, 0).unwrap() - 75.0).abs() < f64::EPSILON);
    assert!((fc.lookup("myHandler", 30, 0).unwrap() - 75.0).abs() < f64::EPSILON);
}

#[test]
fn istanbul_lookup_anonymous_fallback_rejects_nearby_far_column() {
    let mut functions = rustc_hash::FxHashMap::default();
    functions.insert(("(anonymous_0)".to_string(), 4, 28), 75.0);
    let fc = test_istanbul_file_coverage(functions, false);

    assert!(fc.lookup("declaredHelper", 3, 0).is_none());
}

#[test]
fn istanbul_lookup_anonymous_fallback_picks_closest_when_lines_differ() {
    let mut functions = rustc_hash::FxHashMap::default();
    functions.insert(("(anonymous_0)".to_string(), 28, 0), 75.0);
    functions.insert(("(anonymous_1)".to_string(), 29, 0), 50.0);
    let fc = test_istanbul_file_coverage(functions, false);
    assert!((fc.lookup("myHandler", 28, 0).unwrap() - 75.0).abs() < f64::EPSILON);
}

#[test]
fn istanbul_lookup_anonymous_fallback_picks_closest_by_col_on_same_line() {
    let mut functions = rustc_hash::FxHashMap::default();
    functions.insert(("(anonymous_0)".to_string(), 1, 23), 90.0); // outer
    functions.insert(("(anonymous_1)".to_string(), 1, 43), 10.0); // inner
    let fc = test_istanbul_file_coverage(functions, false);
    assert!((fc.lookup("<arrow>", 1, 43).unwrap() - 10.0).abs() < f64::EPSILON);
    assert!((fc.lookup("<arrow>", 1, 23).unwrap() - 90.0).abs() < f64::EPSILON);
}

#[test]
fn istanbul_lookup_anonymous_fallback_bails_only_on_true_tie() {
    let mut functions = rustc_hash::FxHashMap::default();
    functions.insert(("(anonymous_0)".to_string(), 27, 0), 75.0);
    functions.insert(("(anonymous_1)".to_string(), 29, 0), 50.0);
    let fc = test_istanbul_file_coverage(functions, false);
    assert!(fc.lookup("myHandler", 28, 0).is_none());
}

#[test]
fn istanbul_lookup_anonymous_fallback_outside_offset() {
    let mut functions = rustc_hash::FxHashMap::default();
    functions.insert(("(anonymous_0)".to_string(), 28, 0), 75.0);
    let fc = test_istanbul_file_coverage(functions, false);
    assert!(fc.lookup("myHandler", 31, 0).is_none());
}

#[test]
fn istanbul_lookup_named_match_beats_nearby_anonymous() {
    let mut functions = rustc_hash::FxHashMap::default();
    functions.insert(("handleClick".to_string(), 10, 0), 90.0);
    functions.insert(("(anonymous_7)".to_string(), 11, 0), 10.0);
    let fc = test_istanbul_file_coverage(functions, false);
    assert!((fc.lookup("handleClick", 10, 0).unwrap() - 90.0).abs() < f64::EPSILON);
}

#[test]
fn build_test_refs_empty() {
    let exports: Vec<fallow_graph::graph::ExportSymbol> = vec![];
    let graph = fallow_graph::graph::ModuleGraph::build(&[], &[], &[]);
    let refs = build_test_referenced_exports(
        crate::discover::FileId(0),
        &exports,
        StaticTestCoverage::new(&graph),
    );
    assert!(refs.is_empty());
}

#[test]
fn istanbul_crap_empty_complexity() {
    let result = istanbul_crap_default(&[], None, false);
    assert!((result.max_crap).abs() < f64::EPSILON);
    assert_eq!(result.signals.above, 0);
    assert_eq!(result.matched, 0);
    assert_eq!(result.total, 0);
}

#[test]
fn istanbul_crap_match_statistics() {
    let funcs = vec![make_fn_complexity(5), {
        let mut f = make_fn_complexity(3);
        f.name = "other_fn".into();
        f.line = 10;
        f
    }];
    let mut functions = rustc_hash::FxHashMap::default();
    functions.insert(("test_fn".to_string(), 1, 0), 80.0);
    let file_cov = test_istanbul_file_coverage(functions, false);
    let result = istanbul_crap_default(&funcs, Some(&file_cov), true);
    assert_eq!(result.matched, 1);
    assert_eq!(result.total, 2);
}

#[test]
fn estimated_crap_multiple_functions_mixed_coverage() {
    let funcs = vec![
        make_fn_complexity(10), // name "test_fn" line 1
        {
            let mut f = make_fn_complexity(3);
            f.name = "helper".into();
            f.line = 20;
            f
        },
    ];
    let mut refs = rustc_hash::FxHashSet::default();
    refs.insert("test_fn".to_string());
    let result = estimated_crap_default(
        &funcs,
        &refs,
        true,
        fallow_output::CoverageSource::Estimated,
    );
    let (max, above) = (result.max_crap, result.signals.above);
    assert!(max > 10.0);
    assert_eq!(above, 0);
}
