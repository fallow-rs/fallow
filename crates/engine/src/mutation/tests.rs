use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use super::*;
use crate::session::AnalysisSession;

const PLANNED: MutantDisposition = MutantDisposition::Planned;
const NO_COVERAGE: MutantDisposition = MutantDisposition::NoCoverage;

fn fixture(name: &str) -> PathBuf {
    let root = PathBuf::from(
        std::env::var_os("CARGO_MANIFEST_DIR").expect("cargo sets CARGO_MANIFEST_DIR"),
    )
    .join("../../tests/fixtures")
    .join(name);
    dunce::canonicalize(root).expect("fixture exists")
}

fn session(root: &Path) -> AnalysisSession {
    AnalysisSession::load_with_config(root, None, |config| config.no_cache = true)
        .expect("fixture loads")
}

/// Runs the analysis one time and plans each request against it.
fn plans(
    root: &Path,
    exports: Option<&FrameworkConfigExports>,
    requests: &[MutationPlanRequest],
) -> Vec<MutationPlan> {
    let session = session(root);
    let artifacts = session
        .analyze_dead_code_with_artifacts(true, true)
        .expect("analysis succeeds");
    let inputs = MutationPlanInputs {
        root: session.root(),
        graph: artifacts.graph.as_ref().expect("graph"),
        modules: artifacts.modules.as_deref().expect("modules"),
        results: &artifacts.results,
        framework_config_exports: exports,
        setup_files: &[],
    };
    requests
        .iter()
        .map(|request| plan_mutations(&inputs, request))
        .collect()
}

fn plan_all(root: &Path) -> MutationPlan {
    plans(root, None, &[MutationPlanRequest::new(MutationScope::All)]).remove(0)
}

fn dispositions(plan: &MutationPlan) -> BTreeMap<String, BTreeSet<MutantDisposition>> {
    let mut map: BTreeMap<String, BTreeSet<MutantDisposition>> = BTreeMap::new();
    for entry in &plan.mutants {
        map.entry(entry.file.clone())
            .or_default()
            .insert(entry.disposition);
    }
    map
}

fn function_dispositions(plan: &MutationPlan, file: &str) -> BTreeMap<String, MutantDisposition> {
    let mut map = BTreeMap::new();
    for entry in plan.mutants.iter().filter(|entry| entry.file == file) {
        let name = entry
            .function
            .as_ref()
            .and_then(|function| function.name.clone())
            .unwrap_or_else(|| "<top-level>".to_owned());
        let previous = map.insert(name.clone(), entry.disposition);
        assert!(
            previous.is_none_or(|previous| previous == entry.disposition),
            "{file} {name} has two dispositions"
        );
    }
    map
}

fn set(items: &[MutantDisposition]) -> BTreeSet<MutantDisposition> {
    items.iter().copied().collect()
}

#[test]
fn each_file_gets_the_disposition_of_its_static_facts() {
    let plan = plan_all(&fixture("mutation-plan"));
    let pruned = MutantDisposition::Pruned;
    let expected: BTreeMap<String, BTreeSet<MutantDisposition>> = [
        ("src/dead.ts", set(&[pruned(PruneReason::DeadCode)])),
        ("src/dependency.ts", set(&[NO_COVERAGE])),
        ("src/index.ts", set(&[NO_COVERAGE])),
        ("src/math.ts", set(&[PLANNED])),
        ("src/page.ts", set(&[PLANNED])),
        (
            "src/schema.generated.ts",
            set(&[pruned(PruneReason::Generated)]),
        ),
        ("src/untested.ts", set(&[NO_COVERAGE])),
        ("src/wrapper.ts", set(&[PLANNED])),
        (
            "src/writer.ts",
            set(&[PLANNED, pruned(PruneReason::SideEffectSink)]),
        ),
    ]
    .into_iter()
    .map(|(file, dispositions)| (file.to_owned(), dispositions))
    .collect();
    assert_eq!(dispositions(&plan), expected);
    assert!(!plan.no_test_files);
}

#[test]
fn a_function_that_calls_a_side_effect_sink_is_pruned_unless_allowed() {
    let root = fixture("mutation-plan");
    let mut allowed = MutationPlanRequest::new(MutationScope::All);
    allowed.allow_side_effects = true;
    let [default, allowed] = plans(
        &root,
        None,
        &[MutationPlanRequest::new(MutationScope::All), allowed],
    )
    .try_into()
    .expect("two plans");
    let expected_default = BTreeMap::from([
        (
            "save".to_owned(),
            MutantDisposition::Pruned(PruneReason::SideEffectSink),
        ),
        ("size".to_owned(), PLANNED),
        ("target".to_owned(), PLANNED),
    ]);
    assert_eq!(
        function_dispositions(&default, "src/writer.ts"),
        expected_default
    );
    let expected_allowed = BTreeMap::from([
        ("save".to_owned(), PLANNED),
        ("size".to_owned(), PLANNED),
        ("target".to_owned(), PLANNED),
    ]);
    assert_eq!(
        function_dispositions(&allowed, "src/writer.ts"),
        expected_allowed
    );
}

#[test]
fn a_framework_config_export_is_pruned_but_functions_stay() {
    let root = fixture("mutation-plan");
    let mut exports = FrameworkConfigExports::default();
    exports.insert("src/page.ts", "revalidate");
    exports.insert("src/page.ts", "config");
    let with_exports = plans(
        &root,
        Some(&exports),
        &[MutationPlanRequest::new(MutationScope::All)],
    )
    .remove(0);
    let expected = BTreeMap::from([
        (
            "<top-level>".to_owned(),
            MutantDisposition::Pruned(PruneReason::FrameworkConfig),
        ),
        ("compute".to_owned(), PLANNED),
        ("render".to_owned(), PLANNED),
    ]);
    assert_eq!(
        function_dispositions(&with_exports, "src/page.ts"),
        expected
    );

    let without = plan_all(&root);
    let control = BTreeMap::from([
        ("<top-level>".to_owned(), PLANNED),
        ("compute".to_owned(), PLANNED),
        ("render".to_owned(), PLANNED),
    ]);
    assert_eq!(function_dispositions(&without, "src/page.ts"), control);
}

#[test]
fn the_plan_says_whether_the_framework_config_rule_ran() {
    let root = fixture("mutation-plan");
    let checked = plans(
        &root,
        Some(&FrameworkConfigExports::default()),
        &[MutationPlanRequest::new(MutationScope::All)],
    )
    .remove(0);
    assert!(checked.framework_config_checked);

    let unchecked = plan_all(&root);
    assert!(!unchecked.framework_config_checked);
}

#[test]
fn the_session_entry_point_prunes_plugin_known_config_exports() {
    let root = fixture("mutation-plan-framework");
    let from_session = plan_mutations_for_session(
        &session(&root),
        &MutationPlanRequest::new(MutationScope::All),
    )
    .expect("plan");
    assert!(from_session.framework_config_checked);
    let config = MutantDisposition::Pruned(PruneReason::FrameworkConfig);
    assert_eq!(
        function_dispositions(&from_session, "app/blog/page.tsx"),
        BTreeMap::from([
            ("<top-level>".to_owned(), config),
            ("Page".to_owned(), PLANNED),
            ("heading".to_owned(), PLANNED),
        ])
    );
    let route = function_dispositions(&from_session, "src/routes/index.tsx");
    assert_eq!(route.get("<top-level>"), Some(&config));
    assert_eq!(
        route
            .values()
            .filter(|&&disposition| disposition == PLANNED)
            .count(),
        1,
        "the loader function stays planned: {route:?}"
    );

    let without = plan_all(&root);
    assert_eq!(
        function_dispositions(&without, "app/blog/page.tsx").get("<top-level>"),
        Some(&PLANNED)
    );
}

#[test]
fn a_file_reached_only_by_a_test_that_mocks_it_has_no_coverage() {
    let plan = plan_all(&fixture("mutation-plan"));
    let map = dispositions(&plan);
    assert_eq!(map.get("src/dependency.ts"), Some(&set(&[NO_COVERAGE])));
    assert_eq!(map.get("src/wrapper.ts"), Some(&set(&[PLANNED])));
    let wrapper = plan
        .files
        .iter()
        .find(|file| file.path == "src/wrapper.ts")
        .expect("wrapper is planned");
    assert_eq!(wrapper.static_test_files, vec!["src/wrapper.test.ts"]);
    assert!(
        plan.files
            .iter()
            .all(|file| file.path != "src/dependency.ts")
    );
}

#[test]
fn a_test_that_mocks_the_file_is_not_a_static_test_of_it() {
    let plan = plan_all(&fixture("mutation-plan-mixed-mock"));
    let static_tests = |path: &str| {
        plan.files
            .iter()
            .find(|file| file.path == path)
            .unwrap_or_else(|| panic!("{path} is planned"))
            .static_test_files
            .clone()
    };
    assert_eq!(
        static_tests("src/dependency.ts"),
        vec!["src/dependency.test.ts"]
    );
    assert_eq!(static_tests("src/wrapper.ts"), vec!["src/wrapper.test.ts"]);
}

#[test]
fn test_code_and_test_support_are_never_mutated() {
    let plan = plan_all(&fixture("mutation-plan"));
    let files: BTreeSet<&str> = plan
        .mutants
        .iter()
        .map(|entry| entry.file.as_str())
        .collect();
    for test_file in [
        "src/math.test.ts",
        "src/wrapper.test.ts",
        "src/fixtures/data.ts",
    ] {
        assert!(!files.contains(test_file), "{test_file} was mutated");
    }
    assert!(files.contains("src/math.ts"));
}

#[test]
fn planned_files_carry_static_tests_digest_and_instrumented_code() {
    let root = fixture("mutation-plan");
    let plan = plan_all(&root);
    let paths: Vec<&str> = plan.files.iter().map(|file| file.path.as_str()).collect();
    assert_eq!(
        paths,
        vec![
            "src/math.ts",
            "src/page.ts",
            "src/wrapper.ts",
            "src/writer.ts"
        ]
    );
    let math = &plan.files[0];
    assert_eq!(math.static_test_files, vec!["src/math.test.ts"]);
    let bytes = std::fs::read(root.join("src/math.ts")).expect("read math.ts");
    let digest = Sha256::digest(&bytes);
    assert_eq!(math.source_sha256.len(), 64);
    let parsed: Vec<u8> = (0..32)
        .map(|index| {
            u8::from_str_radix(&math.source_sha256[index * 2..index * 2 + 2], 16).expect("hex")
        })
        .collect();
    assert_eq!(parsed, digest.as_slice());
    assert_eq!(math.source_sha256, math.source_sha256.to_ascii_lowercase());
    assert!(math.instrumented_code.contains("function __fallow_m_a(id)"));
    assert!(
        math.instrumented_code.contains("return a + b") || math.instrumented_code.contains("a + b")
    );
    assert_eq!(
        plan.test_files,
        vec!["src/math.test.ts", "src/wrapper.test.ts"]
    );
}

#[test]
fn planned_mutants_rank_by_complexity_then_file_and_span() {
    let plan = plan_all(&fixture("mutation-plan"));
    let ranked: Vec<&PlannedMutant> = plan
        .mutants
        .iter()
        .take_while(|entry| entry.rank.is_some())
        .collect();
    assert_eq!(ranked.len(), plan.counts.planned);
    let ranks: Vec<u32> = ranked.iter().filter_map(|entry| entry.rank).collect();
    let expected_ranks: Vec<u32> = (1..=u32::try_from(ranked.len()).expect("fits")).collect();
    assert_eq!(ranks, expected_ranks);
    let mut order: Vec<(String, String)> = Vec::new();
    for entry in &ranked {
        let name = entry
            .function
            .as_ref()
            .and_then(|function| function.name.clone())
            .unwrap_or_else(|| "<top-level>".to_owned());
        let key = (entry.file.clone(), name);
        if order.last() != Some(&key) {
            order.push(key);
        }
    }
    let expected: Vec<(String, String)> = [
        ("src/math.ts", "clamp"),
        ("src/page.ts", "render"),
        ("src/math.ts", "add"),
        ("src/page.ts", "compute"),
        ("src/wrapper.ts", "renderWrapper"),
        ("src/writer.ts", "size"),
        ("src/writer.ts", "target"),
        ("src/page.ts", "<top-level>"),
    ]
    .into_iter()
    .map(|(file, name)| (file.to_owned(), name.to_owned()))
    .collect();
    assert_eq!(order, expected);
    let clamp = ranked[0].function.as_ref().expect("clamp facts");
    assert_eq!(clamp.cyclomatic, Some(3));
    assert_eq!((clamp.line, clamp.end_line), (5, 13));
}

#[test]
fn max_mutants_applies_after_ranking_and_marks_the_rest_pending() {
    let root = fixture("mutation-plan");
    let mut limited = MutationPlanRequest::new(MutationScope::All);
    limited.max_mutants = Some(2);
    let [full, limited] = plans(
        &root,
        None,
        &[MutationPlanRequest::new(MutationScope::All), limited],
    )
    .try_into()
    .expect("two plans");
    assert_eq!(limited.counts.planned, 2);
    assert_eq!(limited.counts.pending, full.counts.planned - 2);
    assert_eq!(limited.counts.total, full.counts.total);
    let first_two: Vec<&MutantId> = full.mutants[..2]
        .iter()
        .map(|entry| &entry.mutant.id)
        .collect();
    let planned: Vec<&MutantId> = limited
        .mutants
        .iter()
        .filter(|entry| entry.disposition == PLANNED)
        .map(|entry| &entry.mutant.id)
        .collect();
    assert_eq!(planned, first_two);
    let pending_ranks: Vec<u32> = limited
        .mutants
        .iter()
        .filter(|entry| entry.disposition == MutantDisposition::Pending)
        .filter_map(|entry| entry.rank)
        .collect();
    assert_eq!(pending_ranks.first(), Some(&3));
    assert_eq!(pending_ranks.len(), limited.counts.pending);
}

#[test]
fn changed_scope_mutates_only_the_added_lines() {
    let root = fixture("mutation-plan");
    let diff = "\
diff --git a/src/math.ts b/src/math.ts
--- a/src/math.ts
+++ b/src/math.ts
@@ -8,0 +9,3 @@
+  if (value > high) {
+    return high;
+  }
";
    let index = DiffIndex::from_unified_diff(diff);
    let plan = plans(
        &root,
        None,
        &[MutationPlanRequest::new(MutationScope::ChangedSinceBase(
            index,
        ))],
    )
    .remove(0);
    assert!(!plan.mutants.is_empty());
    for entry in &plan.mutants {
        assert_eq!(entry.file, "src/math.ts");
        assert!((9..=11).contains(&entry.mutant.line), "{:?}", entry.mutant);
        let function = entry.function.as_ref().expect("in clamp");
        assert_eq!(function.name.as_deref(), Some("clamp"));
        assert!(function.in_diff);
    }
    let originals: BTreeSet<&str> = plan
        .mutants
        .iter()
        .map(|entry| entry.mutant.original.as_str())
        .collect();
    assert!(originals.contains(">"), "{originals:?}");
    assert!(!originals.contains("<"), "{originals:?}");
}

#[test]
fn paths_scope_selects_only_the_named_files() {
    let root = fixture("mutation-plan");
    let plan = plans(
        &root,
        None,
        &[MutationPlanRequest::new(MutationScope::Paths(vec![
            PathBuf::from("src/math.ts"),
        ]))],
    )
    .remove(0);
    let files: BTreeSet<&str> = plan
        .mutants
        .iter()
        .map(|entry| entry.file.as_str())
        .collect();
    assert_eq!(files, BTreeSet::from(["src/math.ts"]));
}

#[test]
fn the_operator_filter_limits_the_mutants() {
    let root = fixture("mutation-plan");
    let mut request = MutationPlanRequest::new(MutationScope::All);
    request.operators = Some(vec![Operator::ArithmeticOperator]);
    let plan = plans(&root, None, &[request]).remove(0);
    assert!(!plan.mutants.is_empty());
    assert!(
        plan.mutants
            .iter()
            .all(|entry| entry.mutant.operator == Operator::ArithmeticOperator)
    );
}

#[test]
fn the_plan_is_deterministic() {
    let root = fixture("mutation-plan");
    let first = plan_all(&root);
    let second = plan_all(&root);
    assert!(first.counts.total > 0);
    assert_eq!(first, second);
}

#[test]
fn counts_match_the_dispositions() {
    let plan = plan_all(&fixture("mutation-plan"));
    let count = |wanted: MutantDisposition| {
        plan.mutants
            .iter()
            .filter(|entry| entry.disposition == wanted)
            .count()
    };
    let counts = plan.counts;
    assert_eq!(counts.total, plan.mutants.len());
    assert_eq!(counts.planned, count(PLANNED));
    assert_eq!(counts.no_coverage, count(NO_COVERAGE));
    assert_eq!(
        counts.pruned.dead_code,
        count(MutantDisposition::Pruned(PruneReason::DeadCode))
    );
    assert_eq!(
        counts.pruned.generated,
        count(MutantDisposition::Pruned(PruneReason::Generated))
    );
    assert_eq!(
        counts.pruned.side_effect_sink,
        count(MutantDisposition::Pruned(PruneReason::SideEffectSink))
    );
    assert!(counts.pruned.dead_code > 0 && counts.pruned.generated > 0);
}

#[test]
fn a_project_without_tests_says_so_once_and_keeps_no_coverage_data() {
    let plan = plan_all(&fixture("mutation-plan-no-tests"));
    assert!(plan.no_test_files);
    assert!(plan.files.is_empty());
    assert!(plan.test_files.is_empty());
    assert!(plan.counts.total > 0);
    assert_eq!(plan.counts.no_coverage, plan.counts.total);

    let control = plan_all(&fixture("mutation-plan"));
    assert!(!control.no_test_files);
}

#[test]
fn the_session_entry_point_plans_the_same_as_the_inputs_entry_point() {
    let root = fixture("mutation-plan");
    let from_session = plan_mutations_for_session(
        &session(&root),
        &MutationPlanRequest::new(MutationScope::All),
    )
    .expect("plan");
    let from_inputs = plans(
        &root,
        Some(&FrameworkConfigExports::default()),
        &[MutationPlanRequest::new(MutationScope::All)],
    )
    .remove(0);
    assert_eq!(from_session, from_inputs);
}

#[test]
fn a_file_that_declares_a_reserved_name_is_skipped_with_a_reason() {
    let plan = plan_all(&fixture("mutation-plan"));
    assert_eq!(
        plan.skipped_files,
        vec![SkippedFile {
            path: "src/reserved.ts".to_owned(),
            reason: SkipReason::ReservedName,
        }]
    );
    assert!(
        plan.mutants
            .iter()
            .all(|entry| entry.file != "src/reserved.ts")
    );
    assert!(plan.files.iter().any(|file| file.path == "src/math.ts"));
}

#[test]
fn a_top_level_side_effect_sink_prunes_the_top_level_mutants() {
    let root = fixture("mutation-plan-top-level-sink");
    let mut allowed = MutationPlanRequest::new(MutationScope::All);
    allowed.allow_side_effects = true;
    let [default, allowed] = plans(
        &root,
        None,
        &[MutationPlanRequest::new(MutationScope::All), allowed],
    )
    .try_into()
    .expect("two plans");
    assert_eq!(
        function_dispositions(&default, "src/startup.ts"),
        BTreeMap::from([
            (
                "<top-level>".to_owned(),
                MutantDisposition::Pruned(PruneReason::SideEffectSink),
            ),
            ("double".to_owned(), PLANNED),
        ])
    );
    assert_eq!(
        function_dispositions(&allowed, "src/startup.ts"),
        BTreeMap::from([
            ("<top-level>".to_owned(), PLANNED),
            ("double".to_owned(), PLANNED),
        ])
    );
}

#[test]
fn a_function_is_in_the_diff_when_an_added_line_lies_in_its_lines() {
    let lines = [2, 7, 9];
    assert!(plan::has_line_between(&lines, 1, 2));
    assert!(plan::has_line_between(&lines, 3, 7));
    assert!(plan::has_line_between(&lines, 9, 9));
    assert!(!plan::has_line_between(&lines, 3, 6));
    assert!(!plan::has_line_between(&lines, 10, 20));
    assert!(!plan::has_line_between(&[], 1, 5));
}

#[test]
fn a_graph_file_without_module_facts_is_skipped_with_a_reason() {
    let session = session(&fixture("mutation-plan"));
    let artifacts = session
        .analyze_dead_code_with_artifacts(true, true)
        .expect("analysis succeeds");
    let graph = artifacts.graph.as_ref().expect("graph");
    let math = graph
        .as_graph()
        .modules
        .iter()
        .find(|node| node.path.ends_with("src/math.ts"))
        .expect("math.ts is in the graph")
        .file_id;
    let modules: Vec<_> = artifacts
        .modules
        .as_deref()
        .expect("modules")
        .iter()
        .filter(|module| module.file_id != math)
        .cloned()
        .collect();
    let inputs = MutationPlanInputs {
        root: session.root(),
        graph,
        modules: &modules,
        results: &artifacts.results,
        framework_config_exports: None,
        setup_files: &[],
    };
    let plan = plan_mutations(&inputs, &MutationPlanRequest::new(MutationScope::All));
    assert!(plan.skipped_files.contains(&SkippedFile {
        path: "src/math.ts".to_owned(),
        reason: SkipReason::NotAnalyzed,
    }));
    assert!(plan.mutants.iter().all(|entry| entry.file != "src/math.ts"));
}

#[test]
fn a_paths_scope_reports_the_paths_that_select_no_file() {
    let root = fixture("mutation-plan");
    let plan = plans(
        &root,
        None,
        &[MutationPlanRequest::new(MutationScope::Paths(vec![
            PathBuf::from("src/math.ts"),
            PathBuf::from("src/missing.ts"),
            PathBuf::from("src/math.test.ts"),
        ]))],
    )
    .remove(0);
    assert_eq!(
        plan.unmatched_paths,
        vec!["src/math.test.ts".to_owned(), "src/missing.ts".to_owned()]
    );
}

#[test]
fn a_test_file_that_the_runner_config_includes_is_never_mutated() {
    let plan = plan_all(&fixture("mutation-plan-custom-tests"));
    let files: BTreeSet<&str> = plan
        .mutants
        .iter()
        .map(|entry| entry.file.as_str())
        .collect();
    assert!(
        !files.contains("checks/sign.ts"),
        "the test file was mutated"
    );
    assert_eq!(
        dispositions(&plan).get("src/sign.ts"),
        Some(&set(&[PLANNED]))
    );
    assert_eq!(plan.test_files, vec!["checks/sign.ts"]);
}

#[test]
fn a_mutant_on_an_added_line_ranks_first_without_an_enclosing_function() {
    let plan = plan_all(&fixture("mutation-plan"));
    let pick = |file: &str, top_level: bool| {
        plan.mutants
            .iter()
            .find(|entry| {
                entry.file == file
                    && entry.function.is_none() == top_level
                    && entry.disposition == PLANNED
            })
            .cloned()
            .unwrap_or_else(|| panic!("{file} has a planned mutant"))
    };
    let in_function = pick("src/math.ts", false);
    let top_level = pick("src/page.ts", true);
    let mut entries = vec![
        plan::RankEntry {
            in_diff: false,
            planned: in_function.clone(),
        },
        plan::RankEntry {
            in_diff: true,
            planned: top_level.clone(),
        },
    ];
    plan::rank(&mut entries, Some(1));
    let order: Vec<(&MutantId, Option<u32>, MutantDisposition)> = entries
        .iter()
        .map(|entry| {
            (
                &entry.planned.mutant.id,
                entry.planned.rank,
                entry.planned.disposition,
            )
        })
        .collect();
    assert_eq!(
        order,
        vec![
            (&top_level.mutant.id, Some(1), PLANNED),
            (&in_function.mutant.id, Some(2), MutantDisposition::Pending),
        ]
    );
}

#[test]
fn response_shaping_sinks_do_not_prune_but_network_calls_do() {
    let plan = plan_all(&fixture("mutation-plan-response"));
    assert_eq!(
        function_dispositions(&plan, "src/response.ts"),
        BTreeMap::from([
            ("back".to_owned(), PLANNED),
            ("cached".to_owned(), PLANNED),
            ("remember".to_owned(), PLANNED),
            (
                "notify".to_owned(),
                MutantDisposition::Pruned(PruneReason::SideEffectSink),
            ),
        ])
    );
}

#[test]
fn each_response_shaping_row_names_a_catalogue_row_with_an_io_effect() {
    for id in plan::RESPONSE_SHAPING_ROWS {
        assert!(
            fallow_security::catalogue()
                .matchers()
                .iter()
                .any(|matcher| matcher.id == id && plan::SIDE_EFFECTS.contains(&matcher.effect)),
            "{id} is not a catalogue row with an IO effect"
        );
    }
}

#[test]
fn a_provenance_row_prunes_only_the_calls_of_its_import_source() {
    let plan = plan_all(&fixture("mutation-plan-provenance"));
    let pruned = MutantDisposition::Pruned(PruneReason::SideEffectSink);
    let all = plan
        .mutants
        .iter()
        .filter(|entry| entry.file.starts_with("src/") && !entry.file.ends_with(".test.ts"))
        .fold(BTreeMap::new(), |mut map, entry| {
            let name = entry
                .function
                .as_ref()
                .and_then(|function| function.name.clone())
                .unwrap_or_else(|| "<top-level>".to_owned());
            map.insert(format!("{}::{name}", entry.file), entry.disposition);
            map
        });
    assert_eq!(
        all,
        BTreeMap::from([
            ("src/download.ts::download".to_owned(), pruned),
            ("src/download.ts::fetchNamed".to_owned(), pruned),
            ("src/get.ts::get".to_owned(), PLANNED),
            ("src/get.ts::getOr".to_owned(), PLANNED),
            ("src/legacy.ts::legacyDownload".to_owned(), pruned),
            ("src/pick.ts::<arrow>".to_owned(), PLANNED),
            ("src/pick.ts::pick".to_owned(), PLANNED),
        ])
    );
}

/// `(file, line, reach, export, mocked targets)` of a mutant.
type ReachRow = (String, u32, String, Option<String>, Vec<String>);

/// The test reach of each mutant that no rule prunes.
fn reach_rows(plan: &MutationPlan) -> BTreeSet<ReachRow> {
    plan.mutants
        .iter()
        .filter(|entry| !matches!(entry.disposition, MutantDisposition::Pruned(_)))
        .map(|entry| {
            let reach = entry
                .test_reach
                .as_ref()
                .unwrap_or_else(|| panic!("{} has a test reach", entry.mutant.id.as_str()));
            (
                entry.file.clone(),
                entry.mutant.line,
                format!("{:?}", reach.reach),
                reach.export_name.clone(),
                reach.mocked_targets.clone(),
            )
        })
        .collect()
}

#[test]
fn each_mutant_gets_the_test_reach_of_its_export_or_its_file() {
    let plan = plan_all(&fixture("mutation-test-reach"));
    let mocked = vec!["src/service.ts".to_owned()];
    let row = |file: &str,
               line: u32,
               reach: ExportTestReach,
               export: Option<&str>,
               targets: &[String]| {
        (
            file.to_owned(),
            line,
            format!("{reach:?}"),
            export.map(str::to_owned),
            targets.to_vec(),
        )
    };
    let expected: BTreeSet<ReachRow> = [
        row(
            "src/consumer.ts",
            4,
            ExportTestReach::Direct,
            Some("total"),
            &[],
        ),
        row(
            "src/index.ts",
            5,
            ExportTestReach::NoneFile,
            Some("main"),
            &[],
        ),
        row("src/math.ts", 1, ExportTestReach::None, None, &[]),
        row("src/math.ts", 4, ExportTestReach::Direct, Some("add"), &[]),
        row(
            "src/service.ts",
            2,
            ExportTestReach::MockedOnly,
            None,
            &mocked,
        ),
        row(
            "src/service.ts",
            6,
            ExportTestReach::MockedOnly,
            Some("price"),
            &mocked,
        ),
    ]
    .into_iter()
    .collect();
    assert_eq!(reach_rows(&plan), expected);
    assert!(
        plan.mutants
            .iter()
            .filter(|entry| entry.file == "src/service.ts")
            .all(|entry| entry.disposition == NO_COVERAGE)
    );
}

#[test]
fn a_project_without_tests_labels_each_mutant_none_file() {
    let plan = plan_all(&fixture("mutation-plan-no-tests"));
    assert!(!plan.mutants.is_empty());
    assert!(plan.mutants.iter().all(|entry| {
        matches!(entry.disposition, MutantDisposition::Pruned(_))
            || entry
                .test_reach
                .as_ref()
                .is_some_and(|reach| reach.reach == ExportTestReach::NoneFile)
    }));
}

#[test]
fn each_test_file_lists_the_files_that_it_reaches_through_imports() {
    let plan = plan_all(&fixture("mutation-plan"));
    let math: Vec<&str> = plan.test_file_reach_paths("src/math.test.ts").collect();
    for file in [
        "src/fixtures/data.ts",
        "src/math.ts",
        "src/page.ts",
        "src/schema.generated.ts",
        "src/writer.ts",
    ] {
        assert!(math.contains(&file), "{file}: {math:?}");
    }
    assert!(
        !math.contains(&"src/math.test.ts"),
        "the test file is not part of its own reach"
    );
    assert!(math.is_sorted(), "{math:?}");
    let wrapper: Vec<&str> = plan.test_file_reach_paths("src/wrapper.test.ts").collect();
    assert!(
        wrapper.contains(&"src/dependency.ts"),
        "a file that `vi.mock` replaces stays in the reach, so a change to it still changes the cache key: {wrapper:?}"
    );
    assert_eq!(
        plan.test_file_reach.keys().cloned().collect::<Vec<_>>(),
        plan.test_files,
        "each plan test file has a reach entry"
    );
    assert!(plan.support_files.is_empty(), "no runner setup file");
}

#[test]
fn a_runner_setup_file_and_its_imports_are_support_files() {
    let root = fixture("mutation-plan");
    let session = session(&root);
    let artifacts = session
        .analyze_dead_code_with_artifacts(true, true)
        .expect("analysis succeeds");
    let setup_files = [
        PathBuf::from("src/wrapper"),
        root.join("src/missing-setup.ts"),
    ];
    let inputs = MutationPlanInputs {
        root: session.root(),
        graph: artifacts.graph.as_ref().expect("graph"),
        modules: artifacts.modules.as_deref().expect("modules"),
        results: &artifacts.results,
        framework_config_exports: None,
        setup_files: &setup_files,
    };
    let plan = plan_mutations(&inputs, &MutationPlanRequest::new(MutationScope::All));
    assert_eq!(
        plan.support_files,
        vec!["src/dependency.ts".to_owned(), "src/wrapper.ts".to_owned()],
        "a setup path without an extension resolves to the graph file, its imports follow, and a missing setup file adds nothing"
    );
}
