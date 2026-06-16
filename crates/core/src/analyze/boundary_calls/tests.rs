use rustc_hash::{FxHashMap, FxHashSet};
use std::path::PathBuf;

use fallow_config::{
    BoundaryCallsConfig, BoundaryConfig, BoundaryCoverageConfig, BoundaryZone, FallowConfig,
    ForbiddenCallRule, ForbiddenCallee, OutputFormat, ResolvedConfig, RulesConfig, Severity,
};
use fallow_types::extract::{CalleeUse, ImportInfo, ImportedName, ModuleInfo};

use crate::discover::{DiscoveredFile, EntryPoint, EntryPointSource, FileId};
use crate::graph::ModuleGraph;
use crate::resolve::ResolvedModule;
use crate::suppress::{Suppression, SuppressionContext};

use super::find_boundary_call_violations;

fn make_config(root: PathBuf, forbidden: Vec<ForbiddenCallRule>) -> ResolvedConfig {
    FallowConfig {
        rules: RulesConfig {
            boundary_violation: Severity::Error,
            ..RulesConfig::default()
        },
        boundaries: BoundaryConfig {
            coverage: BoundaryCoverageConfig::default(),
            calls: BoundaryCallsConfig { forbidden },
            preset: None,
            zones: vec![
                BoundaryZone {
                    name: "domain".to_string(),
                    patterns: vec!["src/domain/**".to_string()],
                    auto_discover: vec![],
                    root: None,
                },
                BoundaryZone {
                    name: "ui".to_string(),
                    patterns: vec!["src/ui/**".to_string()],
                    auto_discover: vec![],
                    root: None,
                },
            ],
            rules: vec![],
        },
        ..Default::default()
    }
    .resolve(root, OutputFormat::Human, 1, true, true, None)
}

fn forbid(from: &str, callee: &str) -> ForbiddenCallRule {
    ForbiddenCallRule {
        from: from.to_string(),
        callee: ForbiddenCallee::Single(callee.to_string()),
    }
}

fn build_graph(root: &std::path::Path, file_names: &[&str]) -> ModuleGraph {
    let files: Vec<DiscoveredFile> = file_names
        .iter()
        .enumerate()
        .map(|(i, name)| DiscoveredFile {
            id: FileId(u32::try_from(i).expect("test file count fits u32")),
            path: root.join(name),
            size_bytes: 100,
        })
        .collect();

    // Every file is an entry point so reachability never hides a test file.
    let entry_points: Vec<EntryPoint> = files
        .iter()
        .map(|f| EntryPoint {
            path: f.path.clone(),
            source: EntryPointSource::ManualEntry,
        })
        .collect();

    let resolved: Vec<ResolvedModule> = files
        .iter()
        .map(|f| ResolvedModule {
            file_id: f.id,
            path: f.path.clone(),
            exports: vec![],
            re_exports: vec![],
            resolved_imports: vec![],
            resolved_dynamic_imports: vec![],
            resolved_dynamic_patterns: vec![],
            member_accesses: vec![],
            whole_object_uses: vec![],
            has_cjs_exports: false,
            has_angular_component_template_url: false,
            unused_import_bindings: FxHashSet::default(),
            type_referenced_import_bindings: vec![],
            value_referenced_import_bindings: vec![],
            namespace_object_aliases: vec![],
        })
        .collect();

    ModuleGraph::build(&resolved, &entry_points, &files)
}

fn module(file_id: u32, callee_uses: Vec<CalleeUse>, imports: Vec<ImportInfo>) -> ModuleInfo {
    ModuleInfo {
        file_id: FileId(file_id),
        exports: Vec::new(),
        imports,
        re_exports: Vec::new(),
        dynamic_imports: Vec::new(),
        dynamic_import_patterns: Vec::new(),
        require_calls: Vec::new(),
        package_path_references: Vec::new(),
        member_accesses: Vec::new(),
        whole_object_uses: Vec::new(),
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
        flag_uses: Vec::new(),
        class_heritage: Vec::new(),
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
        callee_uses,
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
    }
}

fn callee(path: &str, span_start: u32) -> CalleeUse {
    CalleeUse {
        callee_path: path.to_string(),
        span_start,
    }
}

fn import(source: &str, imported: ImportedName, local: &str, is_type_only: bool) -> ImportInfo {
    ImportInfo {
        source: source.to_string(),
        imported_name: imported,
        local_name: local.to_string(),
        is_type_only,
        from_style: false,
        span: oxc_span::Span::new(0, 10),
        source_span: oxc_span::Span::new(0, 10),
    }
}

#[test]
fn zoned_file_with_matching_written_path_fires_with_line_col() {
    let root = PathBuf::from("/tmp/boundary-calls-test");
    let config = make_config(root.clone(), vec![forbid("domain", "console.*")]);
    let graph = build_graph(&root, &["src/domain/rules.ts", "src/ui/App.tsx"]);
    let modules = vec![module(0, vec![callee("console.log", 25)], Vec::new())];
    let suppressions = SuppressionContext::empty();
    let offsets: Vec<u32> = vec![0, 10, 20, 30];
    let mut line_offsets = FxHashMap::default();
    line_offsets.insert(FileId(0), offsets.as_slice());

    let violations =
        find_boundary_call_violations(&graph, &modules, &config, &suppressions, &line_offsets);

    assert_eq!(violations.len(), 1);
    let v = &violations[0];
    assert!(v.path.ends_with("src/domain/rules.ts"));
    assert_eq!(v.line, 3);
    assert_eq!(v.col, 5);
    assert_eq!(v.zone, "domain");
    assert_eq!(v.callee, "console.log");
    assert_eq!(v.pattern, "console.*");
}

#[test]
fn unzoned_file_is_quiet() {
    let root = PathBuf::from("/tmp/boundary-calls-test");
    let config = make_config(root.clone(), vec![forbid("domain", "console.*")]);
    let graph = build_graph(&root, &["src/other/util.ts"]);
    let modules = vec![module(0, vec![callee("console.log", 0)], Vec::new())];
    let suppressions = SuppressionContext::empty();
    let line_offsets = FxHashMap::default();

    let violations =
        find_boundary_call_violations(&graph, &modules, &config, &suppressions, &line_offsets);
    assert!(violations.is_empty());
}

#[test]
fn zone_without_matching_rule_is_quiet() {
    let root = PathBuf::from("/tmp/boundary-calls-test");
    let config = make_config(root.clone(), vec![forbid("domain", "console.*")]);
    let graph = build_graph(&root, &["src/ui/App.tsx"]);
    let modules = vec![module(0, vec![callee("console.log", 0)], Vec::new())];
    let suppressions = SuppressionContext::empty();
    let line_offsets = FxHashMap::default();

    let violations =
        find_boundary_call_violations(&graph, &modules, &config, &suppressions, &line_offsets);
    assert!(violations.is_empty());
}

#[test]
fn exact_pattern_does_not_substring_match() {
    let root = PathBuf::from("/tmp/boundary-calls-test");
    let config = make_config(root.clone(), vec![forbid("domain", "fetch")]);
    let graph = build_graph(&root, &["src/domain/api.ts"]);
    let modules = vec![module(
        0,
        vec![
            callee("myfetch", 0),
            callee("fetcher", 5),
            callee("fetch", 10),
        ],
        Vec::new(),
    )];
    let suppressions = SuppressionContext::empty();
    let line_offsets = FxHashMap::default();

    let violations =
        find_boundary_call_violations(&graph, &modules, &config, &suppressions, &line_offsets);
    assert_eq!(violations.len(), 1);
    assert_eq!(violations[0].callee, "fetch");
}

#[test]
fn leading_wildcard_suffix_matches() {
    let root = PathBuf::from("/tmp/boundary-calls-test");
    let config = make_config(root.clone(), vec![forbid("domain", "*.innerHTML")]);
    let graph = build_graph(&root, &["src/domain/render.ts"]);
    let modules = vec![module(
        0,
        vec![callee("el.innerHTML", 0), callee("innerHTML", 5)],
        Vec::new(),
    )];
    let suppressions = SuppressionContext::empty();
    let line_offsets = FxHashMap::default();

    let violations =
        find_boundary_call_violations(&graph, &modules, &config, &suppressions, &line_offsets);
    assert_eq!(violations.len(), 1);
    assert_eq!(violations[0].callee, "el.innerHTML");
}

#[test]
fn canonical_match_for_named_import_with_node_prefix() {
    let root = PathBuf::from("/tmp/boundary-calls-test");
    let config = make_config(root.clone(), vec![forbid("domain", "child_process.*")]);
    let graph = build_graph(&root, &["src/domain/runner.ts"]);
    let modules = vec![module(
        0,
        vec![callee("execSync", 0)],
        vec![import(
            "node:child_process",
            ImportedName::Named("execSync".to_string()),
            "execSync",
            false,
        )],
    )];
    let suppressions = SuppressionContext::empty();
    let line_offsets = FxHashMap::default();

    let violations =
        find_boundary_call_violations(&graph, &modules, &config, &suppressions, &line_offsets);
    assert_eq!(violations.len(), 1);
    assert_eq!(violations[0].callee, "execSync");
    assert_eq!(violations[0].pattern, "child_process.*");
}

#[test]
fn canonical_match_for_aliased_named_import_bare_specifier() {
    let root = PathBuf::from("/tmp/boundary-calls-test");
    let config = make_config(root.clone(), vec![forbid("domain", "child_process.exec")]);
    let graph = build_graph(&root, &["src/domain/runner.ts"]);
    let modules = vec![module(
        0,
        vec![callee("run", 0)],
        vec![import(
            "child_process",
            ImportedName::Named("exec".to_string()),
            "run",
            false,
        )],
    )];
    let suppressions = SuppressionContext::empty();
    let line_offsets = FxHashMap::default();

    let violations =
        find_boundary_call_violations(&graph, &modules, &config, &suppressions, &line_offsets);
    assert_eq!(violations.len(), 1);
    assert_eq!(violations[0].callee, "run");
}

#[test]
fn canonical_match_for_namespace_import() {
    let root = PathBuf::from("/tmp/boundary-calls-test");
    let config = make_config(root.clone(), vec![forbid("domain", "child_process.*")]);
    let graph = build_graph(&root, &["src/domain/runner.ts"]);
    let modules = vec![module(
        0,
        vec![callee("cp.exec", 0)],
        vec![import(
            "node:child_process",
            ImportedName::Namespace,
            "cp",
            false,
        )],
    )];
    let suppressions = SuppressionContext::empty();
    let line_offsets = FxHashMap::default();

    let violations =
        find_boundary_call_violations(&graph, &modules, &config, &suppressions, &line_offsets);
    assert_eq!(violations.len(), 1);
    assert_eq!(violations[0].callee, "cp.exec");
}

#[test]
fn relative_import_source_is_not_canonicalized() {
    let root = PathBuf::from("/tmp/boundary-calls-test");
    let config = make_config(root.clone(), vec![forbid("domain", "child_process.*")]);
    let graph = build_graph(&root, &["src/domain/runner.ts"]);
    // A local helper module that happens to bind the same local name; zone-to-
    // zone calls are the import rules' job, so no canonicalization happens.
    let modules = vec![module(
        0,
        vec![callee("execSync", 0)],
        vec![import(
            "./child_process",
            ImportedName::Named("execSync".to_string()),
            "execSync",
            false,
        )],
    )];
    let suppressions = SuppressionContext::empty();
    let line_offsets = FxHashMap::default();

    let violations =
        find_boundary_call_violations(&graph, &modules, &config, &suppressions, &line_offsets);
    assert!(violations.is_empty());
}

#[test]
fn type_only_import_is_not_canonicalized() {
    let root = PathBuf::from("/tmp/boundary-calls-test");
    let config = make_config(root.clone(), vec![forbid("domain", "child_process.*")]);
    let graph = build_graph(&root, &["src/domain/runner.ts"]);
    let modules = vec![module(
        0,
        vec![callee("execSync", 0)],
        vec![import(
            "node:child_process",
            ImportedName::Named("execSync".to_string()),
            "execSync",
            true,
        )],
    )];
    let suppressions = SuppressionContext::empty();
    let line_offsets = FxHashMap::default();

    let violations =
        find_boundary_call_violations(&graph, &modules, &config, &suppressions, &line_offsets);
    assert!(violations.is_empty());
}

#[test]
fn file_level_suppression_is_consumed() {
    let root = PathBuf::from("/tmp/boundary-calls-test");
    let config = make_config(root.clone(), vec![forbid("domain", "console.*")]);
    let graph = build_graph(&root, &["src/domain/rules.ts"]);
    let mut m = module(0, vec![callee("console.log", 0)], Vec::new());
    m.suppressions.push(Suppression::issue(
        0,
        1,
        crate::suppress::IssueKind::BoundaryViolation,
    ));
    let modules = vec![m];
    let suppressions = SuppressionContext::new(&modules);
    let line_offsets = FxHashMap::default();

    let violations =
        find_boundary_call_violations(&graph, &modules, &config, &suppressions, &line_offsets);
    assert!(violations.is_empty());
}

#[test]
fn line_level_suppression_is_consumed() {
    let root = PathBuf::from("/tmp/boundary-calls-test");
    let config = make_config(root.clone(), vec![forbid("domain", "console.*")]);
    let graph = build_graph(&root, &["src/domain/rules.ts"]);
    // Offsets put span 25 on line 3; the suppression targets line 3.
    let mut m = module(0, vec![callee("console.log", 25)], Vec::new());
    m.suppressions.push(Suppression::issue(
        3,
        2,
        crate::suppress::IssueKind::BoundaryViolation,
    ));
    let modules = vec![m];
    let suppressions = SuppressionContext::new(&modules);
    let offsets: Vec<u32> = vec![0, 10, 20, 30];
    let mut line_offsets = FxHashMap::default();
    line_offsets.insert(FileId(0), offsets.as_slice());

    let violations =
        find_boundary_call_violations(&graph, &modules, &config, &suppressions, &line_offsets);
    assert!(violations.is_empty());
}

#[test]
fn no_calls_config_returns_empty() {
    let root = PathBuf::from("/tmp/boundary-calls-test");
    let config = make_config(root.clone(), vec![]);
    let graph = build_graph(&root, &["src/domain/rules.ts"]);
    let modules = vec![module(0, vec![callee("console.log", 0)], Vec::new())];
    let suppressions = SuppressionContext::empty();
    let line_offsets = FxHashMap::default();

    let violations =
        find_boundary_call_violations(&graph, &modules, &config, &suppressions, &line_offsets);
    assert!(violations.is_empty());
}

#[test]
fn multiple_patterns_per_rule_all_apply() {
    let root = PathBuf::from("/tmp/boundary-calls-test");
    let config = make_config(
        root.clone(),
        vec![ForbiddenCallRule {
            from: "domain".to_string(),
            callee: ForbiddenCallee::Many(vec![
                "console.*".to_string(),
                "process.exit".to_string(),
            ]),
        }],
    );
    let graph = build_graph(&root, &["src/domain/rules.ts"]);
    let modules = vec![module(
        0,
        vec![callee("console.warn", 0), callee("process.exit", 5)],
        Vec::new(),
    )];
    let suppressions = SuppressionContext::empty();
    let line_offsets = FxHashMap::default();

    let violations =
        find_boundary_call_violations(&graph, &modules, &config, &suppressions, &line_offsets);
    assert_eq!(violations.len(), 2);
}

#[test]
fn rebound_callee_is_a_documented_false_negative() {
    // `import * as cp from "child_process"; const run = cp.exec; run()` only
    // captures the written callee `run`. The local binding is not an import,
    // so canonicalization finds no provenance and the call stays quiet. This
    // pins the documented direct-callee-only posture: laundered, injected,
    // and re-bound callees are out of scope by design.
    let root = PathBuf::from("/tmp/boundary-calls-test");
    let config = make_config(root.clone(), vec![forbid("domain", "child_process.*")]);
    let graph = build_graph(&root, &["src/domain/rules.ts"]);
    let modules = vec![module(
        0,
        vec![callee("run", 0)],
        vec![import(
            "child_process",
            ImportedName::Namespace,
            "cp",
            false,
        )],
    )];
    let suppressions = SuppressionContext::empty();
    let line_offsets = FxHashMap::default();

    let violations =
        find_boundary_call_violations(&graph, &modules, &config, &suppressions, &line_offsets);
    assert!(
        violations.is_empty(),
        "a re-bound callee has no import provenance and must not match: {violations:?}"
    );
}

#[test]
fn distinct_written_paths_for_same_canonical_callee_both_report() {
    // Extraction dedupes on the WRITTEN path, before canonicalization. Two
    // different written forms of the same canonical callee (`cp.exec` via the
    // namespace import and `execSync` via the named import) are distinct
    // callee uses and must each produce a finding.
    let root = PathBuf::from("/tmp/boundary-calls-test");
    let config = make_config(root.clone(), vec![forbid("domain", "child_process.*")]);
    let graph = build_graph(&root, &["src/domain/rules.ts"]);
    let modules = vec![module(
        0,
        vec![callee("cp.exec", 0), callee("execSync", 5)],
        vec![
            import("child_process", ImportedName::Namespace, "cp", false),
            import(
                "node:child_process",
                ImportedName::Named("execSync".to_string()),
                "execSync",
                false,
            ),
        ],
    )];
    let suppressions = SuppressionContext::empty();
    let line_offsets = FxHashMap::default();

    let violations =
        find_boundary_call_violations(&graph, &modules, &config, &suppressions, &line_offsets);
    assert_eq!(
        violations.len(),
        2,
        "dedup is per written path, not per canonical path: {violations:?}"
    );
    let callees: Vec<&str> = violations.iter().map(|v| v.callee.as_str()).collect();
    assert!(callees.contains(&"cp.exec"));
    assert!(callees.contains(&"execSync"));
}
