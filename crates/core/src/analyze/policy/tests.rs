use rustc_hash::{FxHashMap, FxHashSet};
use std::path::PathBuf;

use fallow_config::{
    ConfigOverride, FallowConfig, OutputFormat, PartialRulesConfig, ResolvedConfig, RulePackDef,
    RulePackRule, RulePackRuleKind, RulesConfig, Severity,
};
use fallow_types::extract::{CalleeUse, ImportInfo, ImportedName, ModuleInfo, ReExportInfo};
use fallow_types::results::{PolicyRuleKind, PolicyViolationSeverity, SuppressionOrigin};

use crate::discover::{DiscoveredFile, EntryPoint, EntryPointSource, FileId};
use crate::graph::ModuleGraph;
use crate::resolve::ResolvedModule;
use crate::suppress::SuppressionContext;

use super::find_policy_violations;

fn rule(id: &str, kind: RulePackRuleKind) -> RulePackRule {
    RulePackRule {
        id: id.to_string(),
        kind,
        callees: Vec::new(),
        specifiers: Vec::new(),
        ignore_type_only: false,
        files: Vec::new(),
        exclude: Vec::new(),
        message: None,
        severity: None,
    }
}

fn banned_call(id: &str, callees: &[&str]) -> RulePackRule {
    RulePackRule {
        callees: callees.iter().map(ToString::to_string).collect(),
        ..rule(id, RulePackRuleKind::BannedCall)
    }
}

fn banned_import(id: &str, specifiers: &[&str]) -> RulePackRule {
    RulePackRule {
        specifiers: specifiers.iter().map(ToString::to_string).collect(),
        ..rule(id, RulePackRuleKind::BannedImport)
    }
}

fn pack(rules: Vec<RulePackRule>) -> RulePackDef {
    RulePackDef {
        schema: None,
        version: 1,
        name: "team-policy".to_string(),
        description: None,
        rules,
    }
}

fn make_config(root: PathBuf, packs: Vec<RulePackDef>, master: Severity) -> ResolvedConfig {
    let mut config = FallowConfig {
        rules: RulesConfig {
            policy_violation: master,
            ..RulesConfig::default()
        },
        ..Default::default()
    }
    .resolve(root, OutputFormat::Human, 1, true, true, None);
    // Packs are normally loaded from disk inside `resolve()`; tests inject
    // parsed packs directly.
    config.rule_packs = packs;
    config
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
        security_sinks: Vec::new(),
        security_sinks_skipped: 0,
        security_unresolved_callee_sites: Vec::new(),
        tainted_bindings: Vec::new(),
        sanitized_sink_args: Vec::new(),
        security_control_sites: Vec::new(),
        callee_uses,
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
fn banned_call_fires_on_written_path_with_line_col() {
    let root = PathBuf::from("/tmp/policy-test");
    let config = make_config(
        root.clone(),
        vec![pack(vec![banned_call("no-console", &["console.*"])])],
        Severity::Warn,
    );
    let graph = build_graph(&root, &["src/app.ts"]);
    let modules = vec![module(0, vec![callee("console.log", 25)], Vec::new())];
    let suppressions = SuppressionContext::empty();
    let offsets: Vec<u32> = vec![0, 10, 20, 30];
    let mut line_offsets = FxHashMap::default();
    line_offsets.insert(FileId(0), offsets.as_slice());

    let violations =
        find_policy_violations(&graph, &modules, &config, &suppressions, &line_offsets);

    assert_eq!(violations.len(), 1);
    let v = &violations[0];
    assert!(v.path.ends_with("src/app.ts"));
    assert_eq!(v.line, 3);
    assert_eq!(v.col, 5);
    assert_eq!(v.pack, "team-policy");
    assert_eq!(v.rule_id, "no-console");
    assert_eq!(v.kind, PolicyRuleKind::BannedCall);
    assert_eq!(v.matched, "console.log");
    assert_eq!(v.severity, PolicyViolationSeverity::Warn);
}

#[test]
fn banned_call_matches_import_resolved_canonical_path() {
    let root = PathBuf::from("/tmp/policy-test");
    let config = make_config(
        root.clone(),
        vec![pack(vec![banned_call("no-exec", &["child_process.*"])])],
        Severity::Warn,
    );
    let graph = build_graph(&root, &["src/app.ts"]);
    // import { execSync } from "node:child_process"; execSync(...)
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
        find_policy_violations(&graph, &modules, &config, &suppressions, &line_offsets);
    assert_eq!(violations.len(), 1);
    assert_eq!(violations[0].matched, "execSync");
}

#[test]
fn banned_call_ignores_same_named_local_without_matching_import() {
    let root = PathBuf::from("/tmp/policy-test");
    let config = make_config(
        root.clone(),
        vec![pack(vec![banned_call("no-exec", &["child_process.exec"])])],
        Severity::Warn,
    );
    let graph = build_graph(&root, &["src/app.ts"]);
    // A local function `exec` with no child_process import: written path
    // `exec` does not match `child_process.exec`, and no canonical path
    // exists.
    let modules = vec![module(0, vec![callee("exec", 0)], Vec::new())];
    let suppressions = SuppressionContext::empty();
    let line_offsets = FxHashMap::default();

    let violations =
        find_policy_violations(&graph, &modules, &config, &suppressions, &line_offsets);
    assert!(violations.is_empty());
}

#[test]
fn banned_import_matches_segment_aware_specifiers() {
    let root = PathBuf::from("/tmp/policy-test");
    let config = make_config(
        root.clone(),
        vec![pack(vec![banned_import("no-moment", &["moment"])])],
        Severity::Warn,
    );
    let graph = build_graph(&root, &["src/app.ts"]);
    let modules = vec![module(
        0,
        Vec::new(),
        vec![
            import("moment", ImportedName::Default, "moment", false),
            import("moment/locale/nl", ImportedName::SideEffect, "", false),
            import("moment-timezone", ImportedName::Default, "momentTz", false),
        ],
    )];
    let suppressions = SuppressionContext::empty();
    let line_offsets = FxHashMap::default();

    let violations =
        find_policy_violations(&graph, &modules, &config, &suppressions, &line_offsets);
    let matched: Vec<&str> = violations.iter().map(|v| v.matched.as_str()).collect();
    assert_eq!(matched, vec!["moment", "moment/locale/nl"]);
    assert!(
        violations
            .iter()
            .all(|v| v.kind == PolicyRuleKind::BannedImport)
    );
}

#[test]
fn banned_import_covers_re_exports() {
    let root = PathBuf::from("/tmp/policy-test");
    let config = make_config(
        root.clone(),
        vec![pack(vec![banned_import("no-moment", &["moment"])])],
        Severity::Warn,
    );
    let graph = build_graph(&root, &["src/barrel.ts"]);
    let mut m = module(0, Vec::new(), Vec::new());
    m.re_exports.push(ReExportInfo {
        source: "moment".to_string(),
        imported_name: "*".to_string(),
        exported_name: "*".to_string(),
        is_type_only: false,
        span: oxc_span::Span::new(0, 10),
    });
    let suppressions = SuppressionContext::empty();
    let line_offsets = FxHashMap::default();

    let violations = find_policy_violations(&graph, &[m], &config, &suppressions, &line_offsets);
    assert_eq!(violations.len(), 1);
}

#[test]
fn ignore_type_only_skips_type_imports() {
    let root = PathBuf::from("/tmp/policy-test");
    let mut import_rule = banned_import("no-moment", &["moment"]);
    import_rule.ignore_type_only = true;
    let config = make_config(root.clone(), vec![pack(vec![import_rule])], Severity::Warn);
    let graph = build_graph(&root, &["src/app.ts"]);
    let modules = vec![module(
        0,
        Vec::new(),
        vec![
            import(
                "moment",
                ImportedName::Named("Moment".to_string()),
                "Moment",
                true,
            ),
            import("moment", ImportedName::Default, "moment", false),
        ],
    )];
    let suppressions = SuppressionContext::empty();
    let line_offsets = FxHashMap::default();

    let violations =
        find_policy_violations(&graph, &modules, &config, &suppressions, &line_offsets);
    assert_eq!(violations.len(), 1, "only the value import should fire");
}

#[test]
fn type_only_imports_flagged_by_default() {
    let root = PathBuf::from("/tmp/policy-test");
    let config = make_config(
        root.clone(),
        vec![pack(vec![banned_import("no-moment", &["moment"])])],
        Severity::Warn,
    );
    let graph = build_graph(&root, &["src/app.ts"]);
    let modules = vec![module(
        0,
        Vec::new(),
        vec![import(
            "moment",
            ImportedName::Named("Moment".to_string()),
            "Moment",
            true,
        )],
    )];
    let suppressions = SuppressionContext::empty();
    let line_offsets = FxHashMap::default();

    let violations =
        find_policy_violations(&graph, &modules, &config, &suppressions, &line_offsets);
    assert_eq!(violations.len(), 1);
}

#[test]
fn files_and_exclude_globs_scope_rules() {
    let root = PathBuf::from("/tmp/policy-test");
    let mut call_rule = banned_call("no-console", &["console.*"]);
    call_rule.files = vec!["src/**".to_string()];
    call_rule.exclude = vec!["src/tooling/**".to_string()];
    let config = make_config(root.clone(), vec![pack(vec![call_rule])], Severity::Warn);
    let graph = build_graph(
        &root,
        &["src/app.ts", "src/tooling/dev.ts", "scripts/build.ts"],
    );
    let modules = vec![
        module(0, vec![callee("console.log", 0)], Vec::new()),
        module(1, vec![callee("console.log", 0)], Vec::new()),
        module(2, vec![callee("console.log", 0)], Vec::new()),
    ];
    let suppressions = SuppressionContext::empty();
    let line_offsets = FxHashMap::default();

    let violations =
        find_policy_violations(&graph, &modules, &config, &suppressions, &line_offsets);
    assert_eq!(violations.len(), 1);
    assert!(violations[0].path.ends_with("src/app.ts"));
}

#[test]
fn per_rule_severity_overrides_master() {
    let root = PathBuf::from("/tmp/policy-test");
    let mut error_rule = banned_call("no-console", &["console.*"]);
    error_rule.severity = Some(Severity::Error);
    let warn_rule = banned_import("no-moment", &["moment"]);
    let config = make_config(
        root.clone(),
        vec![pack(vec![error_rule, warn_rule])],
        Severity::Warn,
    );
    let graph = build_graph(&root, &["src/app.ts"]);
    let modules = vec![module(
        0,
        vec![callee("console.log", 0)],
        vec![import("moment", ImportedName::Default, "moment", false)],
    )];
    let suppressions = SuppressionContext::empty();
    let line_offsets = FxHashMap::default();

    let violations =
        find_policy_violations(&graph, &modules, &config, &suppressions, &line_offsets);
    assert_eq!(violations.len(), 2);
    let by_rule: FxHashMap<&str, PolicyViolationSeverity> = violations
        .iter()
        .map(|v| (v.rule_id.as_str(), v.severity))
        .collect();
    assert_eq!(by_rule["no-console"], PolicyViolationSeverity::Error);
    assert_eq!(by_rule["no-moment"], PolicyViolationSeverity::Warn);
}

#[test]
fn rule_severity_off_disables_only_that_rule() {
    let root = PathBuf::from("/tmp/policy-test");
    let mut off_rule = banned_call("no-console", &["console.*"]);
    off_rule.severity = Some(Severity::Off);
    let live_rule = banned_call("no-fetch", &["fetch"]);
    let config = make_config(
        root.clone(),
        vec![pack(vec![off_rule, live_rule])],
        Severity::Error,
    );
    let graph = build_graph(&root, &["src/app.ts"]);
    let modules = vec![module(
        0,
        vec![callee("console.log", 0), callee("fetch", 5)],
        Vec::new(),
    )];
    let suppressions = SuppressionContext::empty();
    let line_offsets = FxHashMap::default();

    let violations =
        find_policy_violations(&graph, &modules, &config, &suppressions, &line_offsets);
    assert_eq!(violations.len(), 1);
    assert_eq!(violations[0].rule_id, "no-fetch");
    assert_eq!(violations[0].severity, PolicyViolationSeverity::Error);
}

#[test]
fn per_file_override_off_is_a_kill_switch() {
    let root = PathBuf::from("/tmp/policy-test");
    let mut error_rule = banned_call("no-console", &["console.*"]);
    error_rule.severity = Some(Severity::Error);
    let mut config = FallowConfig {
        rules: RulesConfig {
            policy_violation: Severity::Warn,
            ..RulesConfig::default()
        },
        overrides: vec![ConfigOverride {
            files: vec!["src/legacy/**".to_string()],
            rules: PartialRulesConfig {
                policy_violation: Some(Severity::Off),
                ..PartialRulesConfig::default()
            },
        }],
        ..Default::default()
    }
    .resolve(root.clone(), OutputFormat::Human, 1, true, true, None);
    config.rule_packs = vec![pack(vec![error_rule])];

    let graph = build_graph(&root, &["src/legacy/old.ts", "src/app.ts"]);
    let modules = vec![
        module(0, vec![callee("console.log", 0)], Vec::new()),
        module(1, vec![callee("console.log", 0)], Vec::new()),
    ];
    let suppressions = SuppressionContext::empty();
    let line_offsets = FxHashMap::default();

    let violations =
        find_policy_violations(&graph, &modules, &config, &suppressions, &line_offsets);
    assert_eq!(violations.len(), 1);
    assert!(violations[0].path.ends_with("src/app.ts"));
}

#[test]
fn line_and_file_suppressions_are_honored_and_consumed() {
    let root = PathBuf::from("/tmp/policy-test");
    let config = make_config(
        root.clone(),
        vec![pack(vec![banned_call("no-console", &["console.*"])])],
        Severity::Warn,
    );
    let graph = build_graph(&root, &["src/line.ts", "src/file.ts"]);

    // line.ts: suppression on line 3 where the call sits.
    let mut line_module = module(0, vec![callee("console.log", 25)], Vec::new());
    line_module
        .suppressions
        .push(fallow_types::suppress::Suppression::issue(
            3,
            2,
            crate::suppress::IssueKind::PolicyViolation,
        ));
    line_module.line_offsets = vec![0, 10, 20, 30];

    // file.ts: file-wide suppression.
    let mut file_module = module(1, vec![callee("console.log", 0)], Vec::new());
    file_module
        .suppressions
        .push(fallow_types::suppress::Suppression::issue(
            0,
            1,
            crate::suppress::IssueKind::PolicyViolation,
        ));

    let modules = vec![line_module, file_module];
    let suppressions = SuppressionContext::new(&modules);
    let offsets: Vec<u32> = vec![0, 10, 20, 30];
    let mut line_offsets = FxHashMap::default();
    line_offsets.insert(FileId(0), offsets.as_slice());

    let violations =
        find_policy_violations(&graph, &modules, &config, &suppressions, &line_offsets);
    assert!(violations.is_empty());

    let stale = suppressions.find_stale(&graph, &config);
    assert!(
        stale.is_empty(),
        "consumed policy suppressions must not be stale: {stale:?}"
    );
}

#[test]
fn scoped_policy_suppression_only_suppresses_matching_rule() {
    let root = PathBuf::from("/tmp/policy-test");
    let config = make_config(
        root.clone(),
        vec![pack(vec![
            banned_import("no-moment", &["moment"]),
            banned_import("no-moment-alt", &["moment"]),
        ])],
        Severity::Warn,
    );
    let graph = build_graph(&root, &["src/app.ts"]);
    let mut module = module(
        0,
        Vec::new(),
        vec![import("moment", ImportedName::Default, "moment", false)],
    );
    module
        .suppressions
        .push(fallow_types::suppress::Suppression::policy_rule(
            1,
            0,
            "team-policy",
            "no-moment",
        ));
    let modules = vec![module];
    let suppressions = SuppressionContext::new(&modules);
    let line_offsets = FxHashMap::default();

    let violations =
        find_policy_violations(&graph, &modules, &config, &suppressions, &line_offsets);
    assert_eq!(violations.len(), 1);
    assert_eq!(violations[0].rule_id, "no-moment-alt");
    assert!(suppressions.find_stale(&graph, &config).is_empty());
}

#[test]
fn file_scoped_policy_suppression_only_suppresses_matching_rule() {
    let root = PathBuf::from("/tmp/policy-test");
    let config = make_config(
        root.clone(),
        vec![pack(vec![
            banned_import("no-moment", &["moment"]),
            banned_import("no-moment-alt", &["moment"]),
        ])],
        Severity::Warn,
    );
    let graph = build_graph(&root, &["src/app.ts"]);
    let mut module = module(
        0,
        Vec::new(),
        vec![import("moment", ImportedName::Default, "moment", false)],
    );
    module
        .suppressions
        .push(fallow_types::suppress::Suppression::policy_rule(
            0,
            1,
            "team-policy",
            "no-moment",
        ));
    let modules = vec![module];
    let suppressions = SuppressionContext::new(&modules);
    let line_offsets = FxHashMap::default();

    let violations =
        find_policy_violations(&graph, &modules, &config, &suppressions, &line_offsets);
    assert_eq!(violations.len(), 1);
    assert_eq!(violations[0].rule_id, "no-moment-alt");
    assert!(suppressions.find_stale(&graph, &config).is_empty());
}

#[test]
fn stale_scoped_policy_suppression_preserves_full_token() {
    let root = PathBuf::from("/tmp/policy-test");
    let config = make_config(
        root.clone(),
        vec![pack(vec![banned_import("no-moment", &["moment"])])],
        Severity::Warn,
    );
    let graph = build_graph(&root, &["src/app.ts"]);
    let mut module = module(0, Vec::new(), Vec::new());
    module
        .suppressions
        .push(fallow_types::suppress::Suppression::policy_rule(
            0,
            1,
            "team-policy",
            "removed-rule",
        ));
    let modules = vec![module];
    let suppressions = SuppressionContext::new(&modules);

    let stale = suppressions.find_stale(&graph, &config);
    assert_eq!(stale.len(), 1);
    assert!(matches!(
        &stale[0].origin,
        SuppressionOrigin::Comment {
            issue_kind: Some(token),
            is_file_level: true,
            kind_known: true,
        } if token == "policy-violation:team-policy/removed-rule"
    ));
}

#[test]
fn scoped_policy_suppression_is_dormant_when_master_is_off() {
    let root = PathBuf::from("/tmp/policy-test");
    let config = make_config(
        root.clone(),
        vec![pack(vec![banned_import("no-moment", &["moment"])])],
        Severity::Off,
    );
    let graph = build_graph(&root, &["src/app.ts"]);
    let mut module = module(0, Vec::new(), Vec::new());
    module
        .suppressions
        .push(fallow_types::suppress::Suppression::policy_rule(
            0,
            1,
            "team-policy",
            "no-moment",
        ));
    let modules = vec![module];
    let suppressions = SuppressionContext::new(&modules);

    assert!(suppressions.find_stale(&graph, &config).is_empty());
}

#[test]
fn scoped_policy_suppression_is_dormant_when_rule_is_off() {
    let root = PathBuf::from("/tmp/policy-test");
    let mut disabled_rule = banned_import("no-moment", &["moment"]);
    disabled_rule.severity = Some(Severity::Off);
    let config = make_config(
        root.clone(),
        vec![pack(vec![disabled_rule])],
        Severity::Warn,
    );
    let graph = build_graph(&root, &["src/app.ts"]);
    let mut module = module(0, Vec::new(), Vec::new());
    module
        .suppressions
        .push(fallow_types::suppress::Suppression::policy_rule(
            0,
            1,
            "team-policy",
            "no-moment",
        ));
    let modules = vec![module];
    let suppressions = SuppressionContext::new(&modules);

    assert!(suppressions.find_stale(&graph, &config).is_empty());
}

#[test]
fn no_packs_means_no_findings() {
    let root = PathBuf::from("/tmp/policy-test");
    let config = make_config(root.clone(), Vec::new(), Severity::Error);
    let graph = build_graph(&root, &["src/app.ts"]);
    let modules = vec![module(0, vec![callee("console.log", 0)], Vec::new())];
    let suppressions = SuppressionContext::empty();
    let line_offsets = FxHashMap::default();

    let violations =
        find_policy_violations(&graph, &modules, &config, &suppressions, &line_offsets);
    assert!(violations.is_empty());
}

#[test]
fn message_is_carried_onto_findings() {
    let root = PathBuf::from("/tmp/policy-test");
    let mut call_rule = banned_call("no-console", &["console.*"]);
    call_rule.message = Some("Use the logger facade.".to_string());
    let config = make_config(root.clone(), vec![pack(vec![call_rule])], Severity::Warn);
    let graph = build_graph(&root, &["src/app.ts"]);
    let modules = vec![module(0, vec![callee("console.log", 0)], Vec::new())];
    let suppressions = SuppressionContext::empty();
    let line_offsets = FxHashMap::default();

    let violations =
        find_policy_violations(&graph, &modules, &config, &suppressions, &line_offsets);
    assert_eq!(
        violations[0].message.as_deref(),
        Some("Use the logger facade.")
    );
}
