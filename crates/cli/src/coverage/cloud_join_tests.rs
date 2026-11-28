//! Tests of the cloud join gate and the local structure fill of
//! `coverage analyze --cloud` (fallow-rs/fallow-cloud#328).

use fallow_output::{RuntimeCoverageConfidence, RuntimeCoverageRiskBand, RuntimeCoverageVerdict};
use sha2::Digest as _;

use crate::coverage::analyze::tests::{
    cloud_context, cloud_function, static_index_with, static_info,
};
use crate::coverage::analyze::{
    StaticFunctionInfo, StaticIndex, index_static_function, merge_cloud_snapshot,
};
use crate::coverage::cloud_client::{
    CloudNeverCalledSource, CloudRuntimeContext, CloudTrackingState,
};

/// A never-called function in a local graph that misses half of what runs
/// in production. The local "statically unused" answer is then not
/// trusted on its own, so the join caps the verdict (fallow-cloud#328).
#[test]
fn cloud_join_below_match_ratio_caps_safe_to_delete() {
    let mut static_index = StaticIndex::default();
    index_static_function(
        &mut static_index,
        "src/a.ts",
        static_info("src/a.ts", "oldFlow", 10, 20),
    );
    let mut snapshot = cloud_context(2, 0);
    snapshot.summary.trace_count = 100;
    let mut matched = cloud_function("src/a.ts", "oldFlow", Some(10), Some(10), Some(20));
    matched.never_called_source = CloudNeverCalledSource::RuntimeObserved;
    let unmatched = cloud_function("src/missing.ts", "missingInAst", Some(1), Some(1), Some(3));
    snapshot.functions = vec![matched, unmatched];

    let report = merge_cloud_snapshot(&snapshot, &static_index, 100).report;

    assert_eq!(report.findings.len(), 1);
    assert_eq!(
        report.findings[0].verdict,
        RuntimeCoverageVerdict::ReviewRequired
    );
    assert_eq!(
        report.findings[0].confidence,
        RuntimeCoverageConfidence::Medium
    );
    assert_eq!(
        report.findings[0].actions.first().map(|a| a.kind.as_str()),
        Some("review-runtime")
    );
    let codes: Vec<&str> = report
        .warnings
        .iter()
        .map(|warning| warning.code.as_str())
        .collect();
    assert!(codes.contains(&"cloud_functions_unmatched"));
    assert!(codes.contains(&"cloud_local_join_incomplete"));
}

/// The cloud sends its own blast-radius and importance rows, with null
/// structure fields when it has no caller graph, complexity or owners.
/// The join fills them from the local AST (fallow-cloud#328) and keeps
/// the cloud score and reason (F4: context only).
#[test]
fn cloud_rows_take_local_structure_when_the_cloud_has_none() {
    let info = StaticFunctionInfo {
        caller_count: 8,
        cyclomatic: 12,
        owner_count: Some(2),
        ..static_info("src/api.ts", "handler", 10, 22)
    };
    let stable_id = info.stable_id.clone();
    let (static_index, mut snapshot) = with_importer_traffic(info, 700);
    let mut function = cloud_function("src/api.ts", "handler", Some(10), Some(10), Some(22));
    function.tracking_state = CloudTrackingState::Called;
    function.hit_count = Some(20_000);
    function.stable_id = Some(stable_id.clone());
    snapshot.functions.push(function);
    snapshot.blast_radius = vec![
        crate::coverage::cloud_client::CloudRuntimeBlastRadiusEntry {
            id: "fallow:blast:cloud0001".to_owned(),
            legacy_id: None,
            stable_id: Some(stable_id.clone()),
            file: "src/api.ts".to_owned(),
            function: "handler".to_owned(),
            line: 10,
            caller_count: None,
            caller_count_weighted_by_traffic: None,
            deploys_touched: Some(3),
            risk_band: crate::coverage::cloud_client::CloudRuntimeRiskBand::Unknown,
            extra: serde_json::Map::new(),
        },
    ];
    snapshot.importance = vec![crate::coverage::cloud_client::CloudRuntimeImportanceEntry {
        id: "fallow:importance:cloud0001".to_owned(),
        legacy_id: None,
        stable_id: Some(stable_id),
        file: "src/api.ts".to_owned(),
        function: "handler".to_owned(),
        line: 10,
        invocations: 20_000,
        cyclomatic: None,
        owner_count: None,
        importance_score: 42.0,
        reason: "Moderate traffic; scored from invocations only".to_owned(),
    }];

    let report = merge_cloud_snapshot(&snapshot, &static_index, 100).report;

    let blast = &report.blast_radius[0];
    assert_eq!(blast.id, "fallow:blast:cloud0001");
    assert_eq!(blast.caller_count, 8);
    // The traffic in the importer file, as the cloud measures it, not the
    // own invocations times the callers (fallow-cloud#1642).
    assert_eq!(blast.caller_count_weighted_by_traffic, Some(700));
    assert_eq!(blast.risk_band, RuntimeCoverageRiskBand::Medium);
    let importance = &report.importance[0];
    assert_eq!(importance.cyclomatic, 12);
    assert_eq!(importance.owner_count, 2);
    assert!((importance.importance_score - 42.0).abs() < f64::EPSILON);
    assert_eq!(
        importance.reason,
        "Moderate traffic; scored from invocations only"
    );
}

/// Four of five cloud functions match: exactly the limit, so the join is
/// complete and the verdict stays.
#[test]
fn cloud_join_at_the_match_limit_keeps_safe_to_delete() {
    let mut static_index = StaticIndex::default();
    let mut functions = Vec::new();
    for (index, name) in ["oldFlow", "b", "c", "d"].into_iter().enumerate() {
        let line = 10 * (u32::try_from(index).expect("small") + 1);
        index_static_function(
            &mut static_index,
            "src/a.ts",
            static_info("src/a.ts", name, line, line + 5),
        );
        let mut function = cloud_function("src/a.ts", name, Some(line), Some(line), Some(line + 5));
        if name == "oldFlow" {
            function.never_called_source = CloudNeverCalledSource::RuntimeObserved;
        } else {
            function.tracking_state = CloudTrackingState::Called;
            function.hit_count = Some(1);
        }
        functions.push(function);
    }
    functions.push(cloud_function(
        "src/missing.ts",
        "gone",
        Some(1),
        Some(1),
        Some(3),
    ));
    let mut snapshot = cloud_context(5, 0);
    snapshot.functions = functions;

    let report = merge_cloud_snapshot(&snapshot, &static_index, 100).report;

    assert_eq!(
        report.findings[0].verdict,
        RuntimeCoverageVerdict::SafeToDelete
    );
    assert!(
        !report
            .warnings
            .iter()
            .any(|warning| warning.code == "cloud_local_join_incomplete")
    );
}

/// A join below the limit that caps nothing (only called functions) adds
/// no warning, because no verdict changed.
#[test]
fn cloud_join_below_limit_without_a_capped_finding_adds_no_warning() {
    let static_index = static_index_with(vec![static_info("src/a.ts", "hot", 10, 20)]);
    let mut hot = cloud_function("src/a.ts", "hot", Some(10), Some(10), Some(20));
    hot.tracking_state = CloudTrackingState::Called;
    hot.hit_count = Some(5);
    let mut snapshot = cloud_context(2, 0);
    snapshot.functions = vec![
        hot,
        cloud_function("src/missing.ts", "gone", Some(1), Some(1), Some(3)),
    ];

    let report = merge_cloud_snapshot(&snapshot, &static_index, 100).report;

    assert!(
        !report
            .warnings
            .iter()
            .any(|warning| warning.code == "cloud_local_join_incomplete")
    );
}

fn cloud_blast_row(
    stable_id: Option<String>,
    caller_count: Option<u32>,
    risk_band: crate::coverage::cloud_client::CloudRuntimeRiskBand,
) -> crate::coverage::cloud_client::CloudRuntimeBlastRadiusEntry {
    crate::coverage::cloud_client::CloudRuntimeBlastRadiusEntry {
        id: "fallow:blast:cloud0001".to_owned(),
        legacy_id: None,
        stable_id,
        file: "src/api.ts".to_owned(),
        function: "handler".to_owned(),
        line: 10,
        caller_count,
        caller_count_weighted_by_traffic: None,
        deploys_touched: None,
        risk_band,
        extra: serde_json::Map::new(),
    }
}

fn called_handler_snapshot(
    row: crate::coverage::cloud_client::CloudRuntimeBlastRadiusEntry,
) -> (StaticIndex, CloudRuntimeContext) {
    let info = StaticFunctionInfo {
        caller_count: 8,
        ..static_info("src/api.ts", "handler", 10, 22)
    };
    let stable_id = info.stable_id.clone();
    let static_index = static_index_with(vec![info]);
    let mut function = cloud_function("src/api.ts", "handler", Some(10), Some(10), Some(22));
    function.tracking_state = CloudTrackingState::Called;
    function.hit_count = Some(20_000);
    function.stable_id = Some(stable_id);
    let mut snapshot = cloud_context(1, 0);
    snapshot.functions = vec![function];
    snapshot.blast_radius = vec![row];
    (static_index, snapshot)
}

/// The cloud has a caller graph (a caller count) and saw no importer run
/// (null weighted traffic). Its counts stay: local callers never mix in.
#[test]
fn cloud_rows_keep_a_cloud_caller_graph() {
    let (static_index, snapshot) = called_handler_snapshot(cloud_blast_row(
        None,
        Some(3),
        crate::coverage::cloud_client::CloudRuntimeRiskBand::Low,
    ));

    let report = merge_cloud_snapshot(&snapshot, &static_index, 100).report;

    assert_eq!(report.blast_radius[0].caller_count, 3);
    // The cloud sent null: no importer file ran. Null stays null.
    assert_eq!(
        report.blast_radius[0].caller_count_weighted_by_traffic,
        None
    );
    assert_eq!(
        report.blast_radius[0].risk_band,
        RuntimeCoverageRiskBand::Low
    );
}

/// An older cloud sends no `stable_id` on the row. The fill then joins on
/// file, name and line.
#[test]
fn cloud_rows_without_stable_id_take_local_structure_by_position() {
    let (static_index, snapshot) = called_handler_snapshot(cloud_blast_row(
        None,
        None,
        crate::coverage::cloud_client::CloudRuntimeRiskBand::Unknown,
    ));

    let report = merge_cloud_snapshot(&snapshot, &static_index, 100).report;

    assert_eq!(report.blast_radius[0].caller_count, 8);
    assert_eq!(
        report.blast_radius[0].risk_band,
        RuntimeCoverageRiskBand::Medium
    );
}

/// The ids of one called function, as `coverage analyze --cloud` builds them
/// without cloud blast-radius and importance rows.
fn built_ids(start_line: u32) -> (String, Option<String>, String, String) {
    let info = static_info("src/api.ts", "handler", start_line, start_line + 12);
    let stable_id = info.stable_id.clone();
    let static_index = static_index_with(vec![info]);
    let mut function = cloud_function(
        "src/api.ts",
        "handler",
        Some(start_line),
        Some(start_line),
        Some(start_line + 12),
    );
    function.tracking_state = CloudTrackingState::Called;
    function.hit_count = Some(500);
    function.stable_id = Some(stable_id);
    let mut snapshot = cloud_context(1, 0);
    snapshot.functions = vec![function];

    let report = merge_cloud_snapshot(&snapshot, &static_index, 1).report;

    (
        report.hot_paths[0].id.clone(),
        report.hot_paths[0].legacy_id.clone(),
        report.blast_radius[0].id.clone(),
        report.importance[0].id.clone(),
    )
}

/// An edit above a function moves it to another line. Its hot-path,
/// blast-radius and importance ids stay, and `legacy_id` keeps the
/// file+line id of the protocol for the transition window
/// (fallow-cloud#1597).
#[test]
fn cloud_join_ids_survive_a_line_move() {
    let (hot_before, legacy_before, blast_before, importance_before) = built_ids(10);
    let (hot_after, legacy_after, blast_after, importance_after) = built_ids(14);

    assert_eq!(hot_after, hot_before);
    assert_eq!(blast_after, blast_before);
    assert_eq!(importance_after, importance_before);
    assert_eq!(
        legacy_before.as_deref(),
        Some(fallow_cov_protocol::hot_path_id("src/api.ts", "handler", 10).as_str())
    );
    assert_ne!(legacy_after, legacy_before);
}

/// The cloud sends `legacy_id` on its rows (fallow-cloud#322). The report
/// keeps it, so a reference on the old file+line id still resolves.
#[test]
fn cloud_rows_keep_the_cloud_legacy_id() {
    let row: crate::coverage::cloud_client::CloudRuntimeBlastRadiusEntry = serde_json::from_str(
        r#"{"id":"fallow:blast:new00001","legacy_id":"fallow:blast:old00001","file":"src/api.ts","function":"handler","line":10,"risk_band":"low"}"#,
    )
    .expect("cloud row should parse");
    let (static_index, snapshot) = called_handler_snapshot(row);

    let report = merge_cloud_snapshot(&snapshot, &static_index, 100).report;

    assert_eq!(report.blast_radius[0].id, "fallow:blast:new00001");
    assert_eq!(
        report.blast_radius[0].legacy_id.as_deref(),
        Some("fallow:blast:old00001")
    );
}

/// The file has two `map` functions and only one of them ran. The id still
/// comes from the `stable_id`, because the local AST has both, so the other
/// `map` cannot take over the id in a later period.
#[test]
fn cloud_join_ids_count_same_name_functions_that_did_not_run() {
    let ran = static_info("src/list.ts", "map", 3, 5);
    let stable_id = ran.stable_id.clone();
    let static_index = static_index_with(vec![ran, static_info("src/list.ts", "map", 9, 11)]);
    let mut function = cloud_function("src/list.ts", "map", Some(3), Some(3), Some(5));
    function.tracking_state = CloudTrackingState::Called;
    function.hit_count = Some(500);
    function.stable_id = Some(stable_id.clone());
    let mut snapshot = cloud_context(1, 0);
    snapshot.functions = vec![function];

    let report = merge_cloud_snapshot(&snapshot, &static_index, 1).report;

    let digest = sha2::Sha256::digest(format!("hot\0stable\0{stable_id}").as_bytes());
    let expected: String = digest.iter().take(4).fold(String::new(), |mut hex, byte| {
        use std::fmt::Write as _;
        let _ = write!(hex, "{byte:02x}");
        hex
    });
    assert_eq!(report.hot_paths[0].id, format!("fallow:hot:{expected}"));
}

/// A local index where `src/routes.ts` imports `src/api.ts`, and a snapshot
/// whose `src/routes.ts` function ran `routes_hits` times.
fn with_importer_traffic(
    info: StaticFunctionInfo,
    routes_hits: u64,
) -> (StaticIndex, CloudRuntimeContext) {
    let mut static_index =
        static_index_with(vec![info, static_info("src/routes.ts", "route", 1, 9)]);
    static_index
        .importers
        .insert("src/api.ts".to_owned(), vec!["src/routes.ts".to_owned()]);
    let mut route = cloud_function("src/routes.ts", "route", Some(1), Some(1), Some(9));
    route.tracking_state = CloudTrackingState::Called;
    route.hit_count = Some(routes_hits);
    let mut snapshot = cloud_context(2, 0);
    snapshot.functions = vec![route];
    (static_index, snapshot)
}

/// A function with few importers stays `low`, also when the importer files
/// run five million times. Traffic does not move the band (fallow-cloud#1642).
#[test]
fn cloud_join_band_ignores_traffic() {
    let info = StaticFunctionInfo {
        caller_count: 2,
        ..static_info("src/api.ts", "handler", 10, 22)
    };
    let (static_index, mut snapshot) = with_importer_traffic(info, 5_000_000);
    let mut function = cloud_function("src/api.ts", "handler", Some(10), Some(10), Some(22));
    function.tracking_state = CloudTrackingState::Called;
    function.hit_count = Some(900_000);
    snapshot.functions.push(function);

    let report = merge_cloud_snapshot(&snapshot, &static_index, 1).report;

    let blast = report
        .blast_radius
        .iter()
        .find(|row| row.function == "handler")
        .expect("handler row");
    assert_eq!(blast.caller_count_weighted_by_traffic, Some(5_000_000));
    assert_eq!(blast.risk_band, RuntimeCoverageRiskBand::Low);
}

/// No importer file of the function has runtime data, so the weighted
/// value is not measured and stays null, not 0.
#[test]
fn cloud_join_weighted_is_null_without_observed_importers() {
    let info = StaticFunctionInfo {
        caller_count: 1,
        ..static_info("src/api.ts", "handler", 10, 22)
    };
    let static_index = static_index_with(vec![info]);
    let mut function = cloud_function("src/api.ts", "handler", Some(10), Some(10), Some(22));
    function.tracking_state = CloudTrackingState::Called;
    function.hit_count = Some(50);
    let mut snapshot = cloud_context(1, 0);
    snapshot.functions = vec![function];

    let report = merge_cloud_snapshot(&snapshot, &static_index, 1).report;

    assert_eq!(
        report.blast_radius[0].caller_count_weighted_by_traffic,
        None
    );
    let json = serde_json::to_value(&report.blast_radius[0]).expect("serialize");
    assert!(json["caller_count_weighted_by_traffic"].is_null());
}

/// The cloud row is `unknown` and the local AST has no match for it, so the
/// band stays `unknown` instead of a guessed `low`.
#[test]
fn cloud_rows_keep_unknown_without_local_structure() {
    let (static_index, snapshot) = called_handler_snapshot(cloud_blast_row(
        None,
        None,
        crate::coverage::cloud_client::CloudRuntimeRiskBand::Unknown,
    ));
    let mut snapshot = snapshot;
    snapshot.blast_radius[0].file = "src/elsewhere.ts".to_owned();
    snapshot.blast_radius[0].function = "other".to_owned();

    let report = merge_cloud_snapshot(&snapshot, &static_index, 100).report;

    assert_eq!(
        report.blast_radius[0].risk_band,
        RuntimeCoverageRiskBand::Unknown
    );
    assert_eq!(
        report.blast_radius[0].caller_count_weighted_by_traffic,
        None
    );
    // Not known, documented as 0 next to the `unknown` band.
    assert_eq!(report.blast_radius[0].caller_count, 0);
}

/// The only function of the importer file has no local match (an anonymous
/// callback that the AST names otherwise). Its hits still count for that
/// file, as in the cloud sum, instead of a null traffic value.
#[test]
fn cloud_join_counts_unmatched_hits_in_a_resolved_importer_file() {
    let info = StaticFunctionInfo {
        caller_count: 1,
        ..static_info("src/api.ts", "handler", 10, 22)
    };
    let mut static_index =
        static_index_with(vec![info, static_info("src/routes.ts", "route", 1, 9)]);
    static_index
        .importers
        .insert("src/api.ts".to_owned(), vec!["src/routes.ts".to_owned()]);
    let mut callback = cloud_function(
        "/app/src/routes.ts",
        "(anonymous_3)",
        Some(40),
        Some(40),
        Some(44),
    );
    callback.tracking_state = CloudTrackingState::Called;
    callback.hit_count = Some(10_000);
    let mut function = cloud_function("src/api.ts", "handler", Some(10), Some(10), Some(22));
    function.tracking_state = CloudTrackingState::Called;
    function.hit_count = Some(50);
    let mut snapshot = cloud_context(2, 0);
    snapshot.functions = vec![callback, function];

    let report = merge_cloud_snapshot(&snapshot, &static_index, 1).report;

    let blast = report
        .blast_radius
        .iter()
        .find(|row| row.function == "handler")
        .expect("handler row");
    assert_eq!(blast.caller_count_weighted_by_traffic, Some(10_000));
}
