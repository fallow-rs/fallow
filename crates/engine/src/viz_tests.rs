use std::path::PathBuf;

use fallow_config::{BoundaryConfig, BoundaryZone, FallowConfig};
use fallow_graph::graph::ModuleGraph;
use fallow_graph::resolve::{ResolveResult, ResolvedImport, ResolvedModule};
use fallow_types::duplicates::{CloneGroup, CloneInstance};
use fallow_types::extract::{ImportInfo, ImportedName};
use fallow_types::output_dead_code::{BoundaryViolationFinding, CircularDependencyFinding};
use fallow_types::output_format::OutputFormat;
use fallow_types::results::{BoundaryViolation, CircularDependency};

use super::*;
use crate::discover::{EntryPoint, EntryPointSource, FileId};

/// Owned fixture parts backing one [`VizBuildInput`].
struct Fixture {
    config: ResolvedConfig,
    files: Vec<DiscoveredFile>,
    results: AnalysisResults,
    graph: crate::module_graph::RetainedModuleGraph,
    duplication: DuplicationReport,
    workspaces: Vec<WorkspaceInfo>,
}

impl Fixture {
    fn input(&self) -> VizBuildInput<'_> {
        VizBuildInput {
            results: &self.results,
            graph: &self.graph,
            modules: None,
            files: &self.files,
            duplication: &self.duplication,
            workspaces: &self.workspaces,
            config: &self.config,
            feature_flags: &[],
            include_analysis_details: true,
        }
    }
}

fn project_root() -> PathBuf {
    PathBuf::from("/viz-project")
}

fn discovered(id: u32, path: PathBuf, size_bytes: u64) -> DiscoveredFile {
    DiscoveredFile {
        id: FileId(id),
        path,
        size_bytes,
    }
}

fn import_of(target: FileId, specifier: &str) -> ResolvedImport {
    ResolvedImport {
        info: ImportInfo {
            source: specifier.to_owned(),
            imported_name: ImportedName::Named("value".to_owned()),
            local_name: "value".to_owned(),
            is_type_only: false,
            is_type_only_star: false,
            from_style: false,
            span: oxc_span::Span::new(0, 0),
            source_span: oxc_span::Span::new(0, 0),
        },
        target: ResolveResult::InternalModule(target),
    }
}

fn zone(name: &str, pattern: &str) -> BoundaryZone {
    BoundaryZone {
        name: name.to_owned(),
        patterns: vec![pattern.to_owned()],
        auto_discover: Vec::new(),
        root: None,
    }
}

fn resolved_config(root: &Path) -> ResolvedConfig {
    let config = FallowConfig {
        boundaries: BoundaryConfig {
            zones: vec![zone("app", "src/**"), zone("shared", "lib/**")],
            ..BoundaryConfig::default()
        },
        ..FallowConfig::default()
    };
    config.resolve(root.to_path_buf(), OutputFormat::Json, 1, false, true, None)
}

fn cycle_finding(files: Vec<PathBuf>) -> CircularDependencyFinding {
    let length = files.len();
    CircularDependencyFinding::with_actions(CircularDependency {
        files,
        length,
        line: 1,
        col: 0,
        edges: Vec::new(),
        is_cross_package: false,
    })
}

fn violation_finding(from_path: PathBuf, to_path: PathBuf) -> BoundaryViolationFinding {
    BoundaryViolationFinding::with_actions(BoundaryViolation {
        from_path,
        to_path,
        from_zone: "app".to_owned(),
        to_zone: "shared".to_owned(),
        import_specifier: "../lib/c".to_owned(),
        line: 2,
        col: 0,
        via_path: None,
    })
}

fn clone_instance(file: PathBuf, start_line: usize, end_line: usize) -> CloneInstance {
    CloneInstance {
        is_symlink: false,
        file,
        start_line,
        end_line,
        start_col: 0,
        end_col: 0,
        fragment: "const shared = 1;\nconst repeated = 2;\nconst block = 3;".to_owned(),
    }
}

fn clone_group(instances: Vec<CloneInstance>) -> CloneGroup {
    CloneGroup {
        instances,
        token_count: 12,
        line_count: 3,
        similarity: None,
    }
}

/// Synthetic project: 3 files, one import edge a to b, one resolvable
/// cycle (a, b) plus one unresolvable, one clone group over (a, c) plus a
/// dropped and a same-file group, one resolvable boundary violation a to
/// c plus one unresolvable, two zones, one workspace over `lib/`.
fn fixture_with(extra_graph_file: bool) -> Fixture {
    let root = project_root();
    let a = root.join("src/a.ts");
    let b = root.join("src/b.ts");
    let c = root.join("lib/c.ts");
    let missing = root.join("src/missing.ts");

    let files = vec![
        discovered(0, a.clone(), 100),
        discovered(1, b.clone(), 50),
        discovered(2, c.clone(), 25),
    ];

    let mut graph_files = files.clone();
    let mut imports = vec![import_of(FileId(1), "./b")];
    if extra_graph_file {
        graph_files.push(discovered(3, root.join("src/d.ts"), 10));
        imports.push(import_of(FileId(3), "./d"));
    }
    let resolved = vec![ResolvedModule {
        file_id: FileId(0),
        path: a.clone(),
        resolved_imports: imports,
        ..ResolvedModule::default()
    }];
    let entry_points = vec![EntryPoint {
        path: a.clone(),
        source: EntryPointSource::PackageJsonMain,
    }];
    let graph = crate::module_graph::RetainedModuleGraph::from(ModuleGraph::build(
        &resolved,
        &entry_points,
        &graph_files,
    ));

    let results = AnalysisResults {
        circular_dependencies: vec![
            cycle_finding(vec![a.clone(), b]),
            cycle_finding(vec![a.clone(), missing.clone()]),
        ],
        boundary_violations: vec![
            violation_finding(a.clone(), c.clone()),
            violation_finding(a.clone(), missing),
        ],
        ..AnalysisResults::default()
    };

    let duplication = DuplicationReport {
        clone_groups: vec![
            clone_group(vec![
                clone_instance(a.clone(), 1, 3),
                clone_instance(c, 10, 12),
            ]),
            clone_group(vec![
                clone_instance(a.clone(), 20, 22),
                clone_instance(root.join("outside.ts"), 1, 3),
            ]),
            clone_group(vec![
                clone_instance(a.clone(), 30, 32),
                clone_instance(a, 40, 42),
            ]),
        ],
        ..DuplicationReport::default()
    };

    let workspaces = vec![WorkspaceInfo {
        root: root.join("lib"),
        name: "shared-lib".to_owned(),
        is_internal_dependency: false,
    }];

    Fixture {
        config: resolved_config(&root),
        files,
        results,
        graph,
        duplication,
        workspaces,
    }
}

fn fixture() -> Fixture {
    fixture_with(false)
}

#[test]
fn files_and_edges_use_stable_indices() {
    let fx = fixture();
    let data = build_viz_data(&fx.input());

    let paths: Vec<&str> = data.files.iter().map(|f| f.path.as_str()).collect();
    assert_eq!(paths, ["src/a.ts", "src/b.ts", "lib/c.ts"]);
    assert_eq!(data.edges, vec![[0, 1, 0]]);
    assert!(data.files[0].is_entry);
    assert!(matches!(data.files[0].status, VizFileStatus::EntryPoint));
    assert!(matches!(data.files[1].status, VizFileStatus::Clean));
    assert_eq!(data.files[0].import_count, 1);
    assert_eq!(data.files[1].importer_count, 1);
    assert_eq!(data.files[0].workspace, None);
    assert_eq!(data.files[2].workspace, Some(0));
    assert_eq!(data.workspaces.len(), 1);
    assert_eq!(data.workspaces[0].root, "lib");
}

#[test]
fn a_dynamic_import_edge_carries_the_dynamic_flag() {
    let root = project_root();
    let a = root.join("src/a.ts");
    let files = vec![
        discovered(0, a.clone(), 100),
        discovered(1, root.join("src/b.ts"), 50),
        discovered(2, root.join("src/c.ts"), 25),
    ];
    let resolved = vec![ResolvedModule {
        file_id: FileId(0),
        path: a.clone(),
        resolved_imports: vec![import_of(FileId(1), "./b")],
        resolved_dynamic_imports: vec![import_of(FileId(2), "./c")],
        ..ResolvedModule::default()
    }];
    let entry_points = vec![EntryPoint {
        path: a,
        source: EntryPointSource::PackageJsonMain,
    }];
    let fx = Fixture {
        graph: crate::module_graph::RetainedModuleGraph::from(ModuleGraph::build(
            &resolved,
            &entry_points,
            &files,
        )),
        files,
        ..fixture()
    };
    let data = build_viz_data(&fx.input());

    assert_eq!(data.edges, vec![[0, 1, 0], [0, 2, EDGE_FLAG_DYNAMIC]]);
}

#[test]
fn edges_to_files_missing_from_input_are_dropped() {
    let fx = fixture_with(true);
    let data = build_viz_data(&fx.input());

    // The graph carries a to b AND a to d, but d is not in `input.files`,
    // so build_edges drops the second edge instead of emitting a
    // dangling index.
    assert_eq!(fx.graph.edge_count(), 2);
    assert_eq!(data.edges, vec![[0, 1, 0]]);
}

#[test]
fn clone_groups_drop_unresolvable_and_dedup_per_file() {
    let fx = fixture();
    let data = build_viz_data(&fx.input());

    // The group whose second instance lives outside `input.files` keeps
    // only 1 resolvable instance and is dropped entirely.
    assert_eq!(data.clones.len(), 2);
    assert_eq!(data.clones[0].instances.len(), 2);
    assert_eq!(data.clones[0].instances[0].file, 0);
    assert_eq!(data.clones[0].instances[1].file, 2);
    assert_eq!(data.clones[0].lines, 3);
    assert_eq!(data.clones[0].tokens, 12);
    // Two same-file instances in one group dedup to a single group id.
    assert_eq!(data.files[0].clone_groups, vec![0, 1]);
    assert_eq!(data.files[2].clone_groups, vec![0]);
    // dup_lines sums (end minus start plus 1) per resolvable instance.
    assert_eq!(data.files[0].dup_lines, 9);
    assert_eq!(data.files[2].dup_lines, 3);
    assert_eq!(data.files[1].dup_lines, 0);
}

#[test]
fn truncate_preview_caps_lines_and_bytes() {
    // Line cap: more lines than the cap in, CLONE_PREVIEW_MAX_LINES out
    // plus the ellipsis appended directly after the last kept line.
    let last_kept = CLONE_PREVIEW_MAX_LINES - 1;
    let many_lines = (0..CLONE_PREVIEW_MAX_LINES + 5)
        .map(|i| format!("line {i}"))
        .collect::<Vec<_>>();
    let out = truncate_preview(&many_lines.join("\n"));
    assert_eq!(out.matches('\n').count(), CLONE_PREVIEW_MAX_LINES - 1);
    assert!(out.contains(&format!("line {last_kept}")));
    assert!(!out.contains(&format!("line {CLONE_PREVIEW_MAX_LINES}")));
    assert!(out.ends_with('\u{2026}'));

    // Byte budget: the second big line would exceed CLONE_PREVIEW_MAX_BYTES,
    // so output stops after the first line.
    let big = CLONE_PREVIEW_MAX_BYTES * 3 / 4;
    let two_long_lines = format!("{}\n{}", "a".repeat(big), "b".repeat(big));
    let out = truncate_preview(&two_long_lines);
    assert_eq!(out, format!("{}\u{2026}", "a".repeat(big)));

    // Multi-byte content over budget truncates at a line boundary and
    // never slices inside a character (4 bytes per emoji, well over budget).
    let emoji_line = "\u{1f389}".repeat(CLONE_PREVIEW_MAX_BYTES);
    let out = truncate_preview(&emoji_line);
    assert_eq!(out, "\u{2026}");
}

#[test]
fn clone_preview_windows_context_around_the_block() {
    use std::io::Write as _;

    // 20 numbered source lines; the copied block covers lines 8..=11.
    let mut file = tempfile::NamedTempFile::new().expect("temp file");
    let body = (1..=20)
        .map(|i| format!("line {i}"))
        .collect::<Vec<_>>()
        .join("\n");
    file.write_all(body.as_bytes()).expect("write source");
    let inst = clone_instance(file.path().to_path_buf(), 8, 11);

    let (preview, highlight_start, highlight_lines) = build_clone_preview(&inst);
    let preview_lines: Vec<&str> = preview.lines().collect();

    // Block (4 lines) plus 4 lines of context each side fits the cap, so
    // the full window is kept: 4 dimmed + 4 highlighted + 4 dimmed.
    assert_eq!(preview_lines.len(), 12);
    assert_eq!(highlight_start, 4);
    assert_eq!(highlight_lines, 4);
    assert_eq!(preview_lines.first(), Some(&"line 4"));
    let start = highlight_start as usize;
    let end = start + highlight_lines as usize;
    assert_eq!(
        &preview_lines[start..end],
        ["line 8", "line 9", "line 10", "line 11"],
    );
    // The line directly above the block is dimmed context, not copied.
    assert_eq!(preview_lines[start - 1], "line 7");
}

#[test]
fn clone_preview_keeps_leading_context_when_the_block_fills_the_cap() {
    use std::io::Write as _;

    // A block far larger than the cap. The old logic zeroed the context
    // and highlighted the whole (truncated) window; the fix keeps the
    // leading context dimmed so the highlight still reads against it.
    let mut file = tempfile::NamedTempFile::new().expect("temp file");
    let body = (1..=200)
        .map(|i| format!("line {i}"))
        .collect::<Vec<_>>()
        .join("\n");
    file.write_all(body.as_bytes()).expect("write source");
    let inst = clone_instance(file.path().to_path_buf(), 50, 150);

    let (preview, highlight_start, highlight_lines) = build_clone_preview(&inst);
    let preview_lines: Vec<&str> = preview.lines().collect();

    assert_eq!(highlight_start, CLONE_PREVIEW_CONTEXT as u32);
    assert!(
        highlight_start > 0,
        "leading context must survive a huge block"
    );
    assert_eq!(preview_lines.len(), CLONE_PREVIEW_MAX_LINES);
    assert_eq!(
        highlight_lines as usize,
        CLONE_PREVIEW_MAX_LINES - CLONE_PREVIEW_CONTEXT,
    );
    assert_eq!(preview_lines[highlight_start as usize - 1], "line 49");
    assert_eq!(preview_lines[highlight_start as usize], "line 50");
}

#[test]
fn clone_preview_clamps_context_at_file_start() {
    use std::io::Write as _;

    let mut file = tempfile::NamedTempFile::new().expect("temp file");
    file.write_all(b"line 1\nline 2\nline 3\nline 4\nline 5")
        .expect("write source");
    // Block at the very top: no context fits above it, so the highlight
    // starts at index 0 and the trailing lines are dimmed context.
    let inst = clone_instance(file.path().to_path_buf(), 1, 2);

    let (preview, highlight_start, highlight_lines) = build_clone_preview(&inst);
    assert_eq!(highlight_start, 0);
    assert_eq!(highlight_lines, 2);
    assert_eq!(preview, "line 1\nline 2\nline 3\nline 4\nline 5");
}

#[test]
fn clone_preview_falls_back_when_source_is_unreadable() {
    // A missing file forces the fragment fallback: the whole block is
    // highlighted so nothing is dimmed.
    let inst = clone_instance(project_root().join("does-not-exist.ts"), 1, 3);
    let (preview, highlight_start, highlight_lines) = build_clone_preview(&inst);
    assert_eq!(preview, inst.fragment);
    assert_eq!(highlight_start, 0);
    assert_eq!(highlight_lines as usize, preview.lines().count());
}

#[test]
fn cycles_drop_when_any_member_unresolved() {
    let fx = fixture();
    let data = build_viz_data(&fx.input());

    // The a/b cycle resolves fully; the cycle referencing the missing
    // file yields no entry at all (not a partial one).
    assert_eq!(data.cycles, vec![vec![0, 1]]);
    assert!(data.files[0].in_cycle);
    assert!(data.files[1].in_cycle);
    assert!(!data.files[2].in_cycle);
    // The summary counts the rendered cycles, not the raw results, so
    // the dropped cycle does not inflate the header number.
    assert_eq!(data.summary.circular_deps, data.cycles.len());
}

#[test]
fn violations_resolve_zone_and_file_indices() {
    let fx = fixture();
    let data = build_viz_data(&fx.input());

    assert_eq!(data.zones.len(), 2);
    assert_eq!(data.zones[0].name, "app");
    assert_eq!(data.zones[0].files, 2);
    assert_eq!(data.zones[1].name, "shared");
    assert_eq!(data.zones[1].files, 1);
    assert_eq!(data.files[0].zone, Some(0));
    assert_eq!(data.files[1].zone, Some(0));
    assert_eq!(data.files[2].zone, Some(1));

    // The violation whose to_path is not in `input.files` is dropped.
    assert_eq!(data.violations.len(), 1);
    let v = &data.violations[0];
    assert_eq!((v.from, v.to), (0, 2));
    assert_eq!((v.from_zone, v.to_zone), (0, 1));
    assert_eq!(v.line, 2);
    assert_eq!(v.specifier, "../lib/c");
}

#[test]
fn clone_group_cap_counts_truncated_groups() {
    let fx = fixture();
    let index = FileIndex::new(&fx.files);

    // The fixture report has two keepable groups plus one dropped for
    // unresolvable instances; a cap of 1 keeps the first keepable group
    // and counts only the second as truncated (the unresolvable drop is
    // not a truncation).
    let (clones, groups_by_file, _dup_lines, truncated) = build_clones(&fx.duplication, &index, 1);
    assert_eq!(clones.len(), 1);
    assert_eq!(truncated, 1);
    assert!(
        groups_by_file
            .values()
            .all(|ids| ids.iter().all(|&id| (id as usize) < clones.len()))
    );

    // The default cap leaves a small report untouched and unflagged.
    let data = build_viz_data(&fx.input());
    assert_eq!(data.clones.len(), 2);
    assert_eq!(data.summary.clone_groups_truncated, None);
}

#[test]
fn summary_flags_clone_truncation_only_when_nonzero() {
    let fx = fixture();
    let data = build_viz_data(&fx.input());

    let summary = build_summary(&fx.input(), &data.files, &data.clones, &[], &[], 3);
    assert_eq!(summary.clone_groups_truncated, Some(3));
    let summary = build_summary(&fx.input(), &data.files, &data.clones, &[], &[], 0);
    assert_eq!(summary.clone_groups_truncated, None);
}

#[test]
fn summary_counts_match_rendered_arrays() {
    let fx = fixture();
    let data = build_viz_data(&fx.input());
    let s = &data.summary;

    assert_eq!(s.total_files, data.files.len());
    assert_eq!(s.total_size, 175);
    assert_eq!(s.total_edges, data.edges.len());
    assert_eq!(s.clone_groups, data.clones.len());
    assert_eq!(s.duplicated_lines, 12);
    assert_eq!(s.hotspot_files, 0);
    assert_eq!(s.unused_files, 0);
    assert_eq!(s.unused_exports, 0);
    // The raw results carry one unresolvable cycle and one unresolvable
    // violation; the header counts only what the arrays render.
    assert_eq!(s.circular_deps, data.cycles.len());
    assert_eq!(s.circular_deps, 1);
    assert_eq!(s.boundary_violations, data.violations.len());
    assert_eq!(s.boundary_violations, 1);
}

#[test]
fn payload_keeps_counts_and_availability_explicit() {
    let fx = fixture();
    let data = build_viz_data(&fx.input());
    let value = serde_json::to_value(&data).expect("viz data serializes");

    assert_eq!(value["architecture"]["availability"]["unit"], "violations");
    assert_eq!(value["dependencies"]["availability"]["unit"], "findings");
    assert_eq!(value["security"]["availability"]["unit"], "candidates");
    assert_eq!(value["security"]["availability"]["state"], "complete");
    assert_eq!(
        value["security"]["runtime_availability"]["state"],
        "unavailable"
    );
    assert_eq!(value["health"]["availability"]["state"], "unavailable");
    assert_eq!(
        value["health"]["capabilities"]["coverage"]["state"],
        "unavailable"
    );
    assert!(value["frameworks"]["detectors"].is_array());
    assert_eq!(
        value["frameworks"]["detector_availability"]["state"],
        "unavailable"
    );
    assert!(value["styling"].get("score").is_none());
}

/// A completed Health run still knows nothing about production execution
/// unless a runtime coverage input was supplied. The lens must say that
/// instead of letting a complete static answer imply a complete one.
#[test]
fn health_reports_runtime_evidence_as_unavailable_without_a_runtime_input() {
    let fx = fixture();
    let mut data = build_viz_data(&fx.input());
    apply_health_report(&mut data, &HealthReport::default(), Path::new("/project"));
    let value = serde_json::to_value(&data).expect("viz data serializes");

    assert_eq!(value["health"]["availability"]["state"], "complete");
    let runtime = &value["health"]["capabilities"]["runtime"];
    assert_eq!(runtime["state"], "unavailable");
    assert_eq!(runtime["unit"], "observations");
    assert_eq!(runtime["reason"], NO_RUNTIME_COVERAGE_REASON);
    assert_eq!(runtime["count"], 0);
    assert_eq!(
        value["security"]["runtime_availability"]["reason"],
        NO_RUNTIME_COVERAGE_REASON
    );
}

/// A drive-less rooted path is rooted but NOT absolute on Windows, so a
/// redaction gated on `is_absolute` skipped it there and leaked the full
/// path. Pinned on every platform because the predicate must not regress.
#[test]
fn rooted_paths_without_a_drive_are_redacted() {
    let root = Path::new("/project");
    assert_eq!(
        relative_path(Path::new("/Users/private/secret.ts"), root),
        "<external>/secret.ts"
    );
    assert_eq!(
        relative_path(Path::new("/etc/passwd"), root),
        "<external>/passwd"
    );
    // A genuinely relative path is not redacted; it is project-relative.
    assert_eq!(relative_path(Path::new("src/a.ts"), root), "src/a.ts");

    // The JSON layer gates on the same predicate and must agree.
    let mut detail = serde_json::json!({ "path": "/Users/private/secret.ts" });
    relativize_value_paths(&mut detail, root);
    assert_eq!(detail["path"], "<external>/secret.ts");

    // A value that is not under a path key is left alone regardless, so a
    // route specifier does not get treated as a filesystem path.
    let mut route = serde_json::json!({ "specifier": "/api/v1" });
    relativize_value_paths(&mut route, root);
    assert_eq!(route["specifier"], "/api/v1");
}

#[test]
fn external_absolute_paths_are_redacted() {
    let root = Path::new("/project");
    assert_eq!(
        relative_path(Path::new("/project/src/a.ts"), root),
        "src/a.ts"
    );
    assert_eq!(
        relative_path(Path::new("/Users/private/secret.ts"), root),
        "<external>/secret.ts"
    );

    let mut detail = serde_json::json!({ "path": "/Users/private/secret.ts" });
    relativize_value_paths(&mut detail, root);
    assert_eq!(detail["path"], "<external>/secret.ts");

    let mut route = serde_json::json!({ "specifier": "/api/v1" });
    relativize_value_paths(&mut route, root);
    assert_eq!(route["specifier"], "/api/v1");

    let mut conflicts = serde_json::json!({
        "conflicting_paths": ["/project/app/a.ts", "/project/app/b.ts"]
    });
    relativize_value_paths(&mut conflicts, root);
    assert_eq!(conflicts["conflicting_paths"][0], "app/a.ts");
    assert_eq!(conflicts["conflicting_paths"][1], "app/b.ts");
}

#[test]
fn config_action_values_are_not_rendered_as_commands() {
    let finding = finding_from_value(
        "dependency",
        "Dependency finding",
        serde_json::json!({
            "actions": [{
                "kind": "add-to-config",
                "auto_fixable": false,
                "config_key": "entry",
                "value": "./errors",
                "description": "Add the entry to configuration"
            }]
        }),
        Path::new("/project"),
        &|_| None,
    );
    assert_eq!(finding.actions.len(), 1);
    let action = &finding.actions[0];
    assert_eq!(action.kind.as_deref(), Some("add-to-config"));
    assert!(!action.auto_fixable);
    assert_eq!(action.config_key.as_deref(), Some("entry"));
    assert_eq!(action.value, Some(Value::String("./errors".to_string())));
    assert!(action.command.is_none());
}
