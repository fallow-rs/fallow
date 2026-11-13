use super::*;

/// Per-test isolation: a fresh user-config dir (so the store never touches
/// the real dir and parallel tests do not collide) plus a fresh project
/// root. Bind BOTH returned `TempDir`s for the test's lifetime. The store
/// for a non-git tempdir root keys on the canonical root, so each test's
/// root is its own store.
fn test_env() -> (tempfile::TempDir, tempfile::TempDir) {
    let config = tempfile::tempdir().unwrap();
    TEST_CONFIG_DIR.with(|c| *c.borrow_mut() = Some(config.path().to_path_buf()));
    let root = tempfile::tempdir().unwrap();
    (config, root)
}

/// All frontier rel-paths across every worktree sub-map (tests use one
/// root => one worktree key), for the v4 nested-frontier shape.
fn frontier_paths(store: &ImpactStore) -> FxHashSet<String> {
    store
        .frontier
        .values()
        .flat_map(|m| m.keys().cloned())
        .collect()
}

/// All clone fingerprints across every worktree sub-map.
fn clone_fingerprints(store: &ImpactStore) -> FxHashSet<String> {
    store
        .clone_frontier
        .values()
        .flat_map(|m| m.keys().cloned())
        .collect()
}

/// Seed raw bytes at the resolved (user-dir) store path, creating parent
/// dirs, to exercise the load/parse path against hand-authored JSON.
fn seed_store_raw(root: &Path, bytes: &[u8]) {
    let path = store_path(root).expect("test config dir set");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, bytes).unwrap();
}

fn summary(dead: usize, complexity: usize, dupes: usize) -> AuditSummary {
    AuditSummary {
        dead_code_issues: dead,
        dead_code_has_errors: dead > 0,
        dead_code_skipped: false,
        complexity_skipped: false,
        duplication_skipped: false,
        complexity_findings: complexity,
        max_cyclomatic: None,
        duplication_clone_groups: dupes,
    }
}

/// Record a run with no per-finding attribution (v1 surfacing/trend/containment only).
#[expect(
    clippy::too_many_arguments,
    reason = "test scaffold; positional record builder mirrors the AuditRunRecord fields, bundling adds churn with no production value"
)]
fn record_v1(
    root: &Path,
    summary: &AuditSummary,
    verdict: AuditVerdict,
    gate: bool,
    git_sha: Option<&str>,
    version: &str,
    timestamp: &str,
) {
    record_audit_run(
        root,
        summary,
        &AuditRunRecord {
            verdict,
            gate_source: gate.then_some(GateSource::Unknown),
            git_sha,
            version,
            timestamp,
            attribution: None,
            analysis_identity: &fallow_types::semantic::SemanticAnalysisIdentity::default(),
        },
    )
    .unwrap();
}

/// Create a real file under `root` (attribution prunes frontier entries for
/// files that no longer exist, so test files must exist on disk).
fn touch(root: &Path, rel: &str) -> PathBuf {
    let p = root.join(rel);
    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(&p, b"x").unwrap();
    p
}

fn fi(path: &Path, kind: &'static str, symbol: &str) -> FindingInput {
    FindingInput {
        path: path.to_path_buf(),
        kind,
        symbol: Some(symbol.to_owned()),
    }
}

fn supp(path: &Path, kind: &str) -> ActiveSuppression {
    ActiveSuppression {
        path: path.to_path_buf(),
        kind: Some(kind.to_owned()),
        is_file_level: false,
        reason: None,
        comment_line: 3,
    }
}

/// Record one attribution run against the store.
fn run(
    root: &Path,
    changed: &[&Path],
    findings: Vec<FindingInput>,
    clones: Vec<CloneInput>,
    supps: &[ActiveSuppression],
    ts: &str,
) {
    let changed_files: Vec<PathBuf> = changed.iter().map(|p| p.to_path_buf()).collect();
    let input = AttributionInput {
        root,
        scope: Scope::ChangedFiles(&changed_files),
        findings,
        clones,
        suppressions: supps,
    };
    record_audit_run(
        root,
        &summary(0, 0, 0),
        &AuditRunRecord {
            verdict: AuditVerdict::Pass,
            gate_source: Some(GateSource::Unknown),
            git_sha: Some("sha"),
            version: "2.0.0",
            timestamp: ts,
            attribution: Some(&input),
            analysis_identity: &fallow_types::semantic::SemanticAnalysisIdentity::default(),
        },
    )
    .unwrap();
}

#[test]
fn disabled_store_does_not_record() {
    let (_config, dir) = test_env();
    let root = dir.path();
    record_v1(
        root,
        &summary(3, 1, 0),
        AuditVerdict::Fail,
        true,
        Some("abc1234"),
        "2.0.0",
        "2026-05-29T10:00:00Z",
    );
    let store = load(root);
    assert!(store.records.is_empty());
    assert!(!store.enabled);
}

#[test]
fn enable_and_disable_record_the_explicit_decision() {
    let (_config, dir) = test_env();
    let root = dir.path();
    assert!(!load(root).explicit_decision, "fresh store: never asked");

    // Declining on a never-enabled project is an explicit decision too.
    disable(root).unwrap();
    let store = load(root);
    assert!(!store.enabled);
    assert!(store.explicit_decision);
    assert!(build_report(&store).explicit_decision);
}

#[test]
fn due_digest_stamps_and_respects_interval_and_gates() {
    let (_config, dir) = test_env();
    let root = dir.path();

    // Disabled, or enabled with zero value: never due.
    assert!(take_due_digest(root).is_none());
    enable(root).unwrap();
    assert!(take_due_digest(root).is_none(), "zero counters never nag");

    let mut store = load(root);
    store.resolved_total = 3;
    store.containment.push(ContainmentEvent {
        blocked_at: "2026-06-11T00:00:00Z".to_string(),
        cleared_at: "2026-06-11T00:05:00Z".to_string(),
        git_sha: None,
        blocked_counts: ImpactCounts::default(),
    });
    save(&store, root);

    let digest = take_due_digest(root).expect("first digest is due");
    assert_eq!(digest.containment_count, 1);
    assert_eq!(digest.resolved_total, 3);
    assert!(
        take_due_digest(root).is_none(),
        "stamped: not due again within the interval"
    );

    // An expired stamp makes it due again.
    let mut store = load(root);
    store.last_digest_epoch = Some(0);
    save(&store, root);
    assert!(take_due_digest(root).is_some());
}

#[test]
fn decline_onboarding_persists_in_existing_store() {
    let (_config, dir) = test_env();
    let root = dir.path();

    assert!(decline_onboarding(root).unwrap());
    assert!(!decline_onboarding(root).unwrap());

    let store = load(root);
    assert!(store.onboarding_declined);
    assert_eq!(store.schema_version, STORE_SCHEMA_VERSION);
    // Decline persists in the user store and writes nothing into the repo.
    assert!(!root.join(".gitignore").exists());
    let report = build_report(&store);
    assert!(report.onboarding_declined);
}

#[test]
fn enable_then_record_accrues_history() {
    let (_config, dir) = test_env();
    let root = dir.path();
    assert!(enable(root).unwrap());
    assert!(!enable(root).unwrap()); // second enable is a no-op-ish (already on)
    record_v1(
        root,
        &summary(2, 1, 0),
        AuditVerdict::Warn,
        false,
        None,
        "2.0.0",
        "2026-05-29T10:00:00Z",
    );
    let store = load(root);
    assert_eq!(store.records.len(), 1);
    assert_eq!(store.records[0].counts.total_issues, 3);
    assert_eq!(
        store.first_recorded.as_deref(),
        Some("2026-05-29T10:00:00Z")
    );
}

#[test]
fn record_is_a_noop_in_ci() {
    // Impact is local-dev-only: it must never record on CI. The suite itself
    // runs on CI (where `CI` / `GITHUB_ACTIONS` are set), so the gate uses a
    // per-test override instead of the ambient env; here we force it true to
    // prove the production no-op. Without the gate, an enabled project would
    // record on every CI run.
    let (_config, dir) = test_env();
    let root = dir.path();
    assert!(enable(root).unwrap());
    TEST_FORCE_CI.with(|c| c.set(true));
    record_v1(
        root,
        &summary(2, 1, 0),
        AuditVerdict::Warn,
        false,
        None,
        "2.0.0",
        "2026-05-29T10:00:00Z",
    );
    TEST_FORCE_CI.with(|c| c.set(false));
    let store = load(root);
    assert_eq!(store.records.len(), 0, "impact must not record while in CI");
}

#[test]
fn enable_writes_nothing_into_the_repo() {
    let (_config, dir) = test_env();
    let root = dir.path();
    enable(root).unwrap();
    // The user-store relocation means enable never touches the repo: no
    // .gitignore mutation and no in-repo .fallow/ dir.
    assert!(
        !root.join(".gitignore").exists(),
        "enable must not create or modify the repo's .gitignore"
    );
    assert!(
        !root.join(".fallow").exists(),
        "enable must not create an in-repo .fallow/ dir"
    );
    // The decision IS persisted, in the user store.
    let store = load(root);
    assert!(store.enabled);
    assert!(store.explicit_decision);
    assert!(resolve_enabled(&store).0);
}

#[test]
fn single_record_yields_no_trend_no_spike() {
    let mut store = ImpactStore {
        enabled: true,
        ..Default::default()
    };
    store.records.push(ImpactRecord {
        timestamp: "t0".into(),
        version: "2.0.0".into(),
        git_sha: None,
        verdict: "warn".into(),
        gate_source: None,
        counts: ImpactCounts {
            total_issues: 5,
            dead_code: 5,
            complexity: 0,
            duplication: 0,
        },
    });
    let report = build_report(&store);
    assert!(report.trend.is_none());
    assert_eq!(report.surfacing.unwrap().total_issues, 5);
}

#[test]
fn empty_store_report_is_first_run() {
    let store = ImpactStore::default();
    let report = build_report(&store);
    assert_eq!(report.record_count, 0);
    assert!(report.trend.is_none());
    assert!(report.surfacing.is_none());
    let human = render_human(&report);
    assert!(human.contains("off")); // default store is disabled
}

#[test]
fn enabled_empty_store_shows_check_back() {
    let store = ImpactStore {
        enabled: true,
        ..Default::default()
    };
    let report = build_report(&store);
    let human = render_human(&report);
    assert!(human.contains("No history yet"));
    assert!(!human.contains("0 issues"));
}

#[test]
fn statusline_renders_stable_non_success_states() {
    let off = build_report(&ImpactStore::default());
    assert_eq!(render_statusline(&off), "fallow impact  off");

    let tracking = build_report(&ImpactStore {
        enabled: true,
        ..Default::default()
    });
    assert_eq!(
        render_statusline(&tracking),
        "fallow impact  tracking · awaiting first scan"
    );
    assert_eq!(
        render_statusline_unavailable(),
        "fallow impact  data unavailable"
    );
    assert_eq!(
        render_statusline_newer_schema(),
        "fallow impact  data from newer fallow · upgrade this fallow"
    );
}

#[test]
fn statusline_uses_only_comparable_whole_project_trend() {
    let mut report = rreport(
        2,
        Some("2026-06-13T00:00:00Z"),
        Some(rcounts(62, 62, 0, 0)),
        Some(rtrend(9, 62)),
        Some(rcounts(7, 7, 0, 0)),
        Some(rtrend(2, 7)),
        true,
    );
    report.resolved_total = 4_949;

    assert_eq!(
        render_statusline(&report),
        "fallow impact  7 issues in last full scan · 5 more than prior · 4.9k cleared while tracking"
    );
}

#[test]
fn statusline_labels_legacy_changed_file_scope_and_omits_trend() {
    let report = rreport(
        2,
        Some("2026-06-13T00:00:00Z"),
        Some(rcounts(62, 62, 0, 0)),
        Some(rtrend(9, 62)),
        None,
        None,
        false,
    );

    assert_eq!(
        render_statusline(&report),
        "fallow impact  62 issues in last changed-file scan"
    );
}

#[test]
fn statusline_renders_baseline_improvement_stability_and_zero_cleared() {
    let baseline = rreport(0, None, None, None, Some(rcounts(1, 1, 0, 0)), None, false);
    assert_eq!(
        render_statusline(&baseline),
        "fallow impact  1 issue in last full scan · compare after next full scan"
    );

    let improving = rreport(
        0,
        None,
        None,
        None,
        Some(rcounts(3, 3, 0, 0)),
        Some(rtrend(8, 3)),
        false,
    );
    assert_eq!(
        render_statusline(&improving),
        "fallow impact  3 issues in last full scan · 5 fewer than prior"
    );

    let stable = rreport(
        0,
        None,
        None,
        None,
        Some(rcounts(3, 3, 0, 0)),
        Some(rtrend(3, 3)),
        true,
    );
    assert_eq!(
        render_statusline(&stable),
        "fallow impact  3 issues in last full scan · unchanged from prior · 0 cleared while tracking"
    );
}

#[test]
fn compact_statusline_counts_are_readable() {
    for (count, expected) in [
        (0, "0"),
        (999, "999"),
        (1_000, "1k"),
        (1_050, "1.1k"),
        (4_949, "4.9k"),
        (10_000, "10k"),
        (1_250_000, "1.3m"),
    ] {
        assert_eq!(compact_count(count), expected);
    }
}

#[test]
fn trend_improving_when_issues_drop() {
    let mut store = ImpactStore {
        enabled: true,
        ..Default::default()
    };
    for total in [8usize, 3usize] {
        store.records.push(ImpactRecord {
            timestamp: format!("t{total}"),
            version: "2.0.0".into(),
            git_sha: None,
            verdict: "warn".into(),
            gate_source: None,
            counts: ImpactCounts {
                total_issues: total,
                dead_code: total,
                complexity: 0,
                duplication: 0,
            },
        });
    }
    let report = build_report(&store);
    let trend = report.trend.unwrap();
    assert_eq!(trend.direction, ImpactTrendDirection::Improving);
    assert_eq!(trend.total_delta, -5);
}

#[test]
fn containment_blocked_then_cleared_records_one_event() {
    let (_config, dir) = test_env();
    let root = dir.path();
    enable(root).unwrap();
    record_v1(
        root,
        &summary(2, 0, 0),
        AuditVerdict::Fail,
        true,
        Some("sha1"),
        "2.0.0",
        "t0",
    );
    let store = load(root);
    assert!(store.pending_containment.is_some());
    assert!(store.containment.is_empty());

    record_v1(
        root,
        &summary(0, 0, 0),
        AuditVerdict::Pass,
        true,
        Some("sha2"),
        "2.0.0",
        "t1",
    );
    let store = load(root);
    assert!(store.pending_containment.is_none());
    assert_eq!(store.containment.len(), 1);
    assert_eq!(store.containment[0].blocked_at, "t0");
    assert_eq!(store.containment[0].cleared_at, "t1");
}

#[test]
fn non_gate_run_never_creates_containment() {
    let (_config, dir) = test_env();
    let root = dir.path();
    enable(root).unwrap();
    record_v1(
        root,
        &summary(2, 0, 0),
        AuditVerdict::Fail,
        false,
        None,
        "2.0.0",
        "t0",
    );
    let store = load(root);
    assert!(store.pending_containment.is_none());
    assert!(store.containment.is_empty());
}

#[test]
fn corrupt_store_loads_as_default_no_panic() {
    let (_config, dir) = test_env();
    let root = dir.path();
    seed_store_raw(root, b"{ not valid json ][");
    let store = load(root);
    assert!(!store.enabled);
    assert!(store.records.is_empty());
    record_v1(
        root,
        &summary(1, 0, 0),
        AuditVerdict::Fail,
        true,
        None,
        "2.0.0",
        "t0",
    );
}

#[test]
fn records_are_bounded() {
    let mut store = ImpactStore {
        enabled: true,
        ..Default::default()
    };
    for i in 0..(MAX_RECORDS + 50) {
        store.records.push(ImpactRecord {
            timestamp: format!("t{i}"),
            version: "2.0.0".into(),
            git_sha: None,
            verdict: "pass".into(),
            gate_source: None,
            counts: ImpactCounts::default(),
        });
    }
    compact(&mut store);
    assert_eq!(store.records.len(), MAX_RECORDS);
    assert_eq!(store.records[0].timestamp, "t50");
}

#[test]
fn report_always_carries_schema_version() {
    let empty = build_report(&ImpactStore::default());
    assert_eq!(empty.schema_version, ImpactReportSchemaVersion::V2);
    let json = render_json(&empty);
    let value: serde_json::Value = serde_json::from_str(&json).expect("impact JSON must parse");
    assert_eq!(
        value["schema_version"], "2",
        "schema_version must be present even when disabled: {json}"
    );

    let mut store = ImpactStore {
        enabled: true,
        ..Default::default()
    };
    store.records.push(ImpactRecord {
        timestamp: "2026-05-29T10:00:00Z".into(),
        version: "2.0.0".into(),
        git_sha: None,
        verdict: "pass".into(),
        gate_source: None,
        counts: ImpactCounts::default(),
    });
    assert_eq!(
        build_report(&store).schema_version,
        ImpactReportSchemaVersion::V2
    );
}

#[test]
fn date_only_trims_iso_timestamp() {
    assert_eq!(date_only("2026-05-29T18:15:23Z"), "2026-05-29");
    assert_eq!(date_only("2026-05-29"), "2026-05-29");
    assert_eq!(date_only("the first run"), "the first run");
}

#[test]
fn human_footer_shows_date_only() {
    let mut store = ImpactStore {
        enabled: true,
        ..Default::default()
    };
    store.first_recorded = Some("2026-05-29T18:15:23Z".into());
    store.records.push(ImpactRecord {
        timestamp: "2026-05-29T18:15:23Z".into(),
        version: "2.0.0".into(),
        git_sha: None,
        verdict: "pass".into(),
        gate_source: None,
        counts: ImpactCounts::default(),
    });
    let report = build_report(&store);
    let human = render_human(&report);
    assert!(
        human.contains("since 2026-05-29.") && !human.contains("18:15:23"),
        "human footer must show date-only: {human}"
    );
    let md = render_markdown(&report);
    assert!(
        md.contains("since 2026-05-29.") && !md.contains("18:15:23"),
        "markdown footer must show date-only: {md}"
    );
}

#[test]
fn future_schema_version_store_loads_without_panic_or_loss() {
    let (_config, dir) = test_env();
    let root = dir.path();
    let future = format!(
        "{{\"schema_version\":{},\"enabled\":true,\"records\":[],\"containment\":[]}}",
        STORE_SCHEMA_VERSION + 1
    );
    seed_store_raw(root, future.as_bytes());
    let store = load(root);
    assert_eq!(store.schema_version, STORE_SCHEMA_VERSION + 1);
    assert!(
        store.enabled,
        "future-version store must not degrade to default"
    );
}

/// A store written by a newer Fallow, with a field and a record shape that
/// this build does not know.
fn future_store_json() -> String {
    format!(
        r#"{{"schema_version":{},"enabled":true,"explicit_decision":false,"records":[{{"timestamp":"2026-10-01T00:00:00Z","version":"99.1.0","verdict":"pass","counts":{{"total_issues":0,"dead_code":0,"complexity":0,"duplication":0}},"future_record_field":1}}],"project_records":[{{"timestamp":"2026-09-01T00:00:00Z","version":"99.0.0","verdict":"pass","counts":{{"total_issues":0,"dead_code":0,"complexity":0,"duplication":0}}}}],"containment":[],"future_field":{{"sentinel":"preserve"}}}}"#,
        STORE_SCHEMA_VERSION + 1
    )
}

fn raw_store(root: &Path) -> String {
    std::fs::read_to_string(store_path(root).expect("test config dir set")).unwrap()
}

#[test]
fn future_schema_store_survives_every_write_path_unchanged() {
    let (_config, dir) = test_env();
    let root = dir.path();
    let future = future_store_json();
    seed_store_raw(root, future.as_bytes());

    assert!(disable(root).is_err(), "disable must refuse a newer store");
    assert!(enable(root).is_err(), "enable must refuse a newer store");
    assert!(decline_onboarding(root).is_err());
    assert!(take_due_digest(root).is_none());
    let audit = record_audit_run(
        root,
        &summary(1, 0, 0),
        &AuditRunRecord {
            verdict: AuditVerdict::Fail,
            gate_source: Some(GateSource::Unknown),
            git_sha: Some("abc1234"),
            version: "2.0.0",
            timestamp: "2026-10-02T00:00:00Z",
            attribution: None,
            analysis_identity: &fallow_types::semantic::SemanticAnalysisIdentity::default(),
        },
    );
    assert!(
        audit.is_err(),
        "an audit record must not rewrite a newer store"
    );
    let combined = record_combined_run(
        root,
        ImpactCounts::default(),
        Some("abc1234"),
        "2.0.0",
        "2026-10-02T00:00:00Z",
        None,
        &fallow_types::semantic::SemanticAnalysisIdentity::default(),
    );
    assert!(
        combined.is_err(),
        "a combined record must not rewrite a newer store"
    );

    let after = raw_store(root);
    assert_eq!(
        after, future,
        "a newer store must stay byte-for-byte unchanged"
    );
    let value: serde_json::Value = serde_json::from_str(&after).unwrap();
    assert_eq!(value["future_field"]["sentinel"], "preserve");
    assert_eq!(value["records"][0]["future_record_field"], 1);
    assert!(load(root).enabled, "reads stay best-effort");
}

#[test]
fn newer_store_refusal_names_schema_and_newest_writer_version() {
    let (_config, dir) = test_env();
    let root = dir.path();
    seed_store_raw(root, future_store_json().as_bytes());

    let message = disable(root).unwrap_err().message();
    assert!(
        message.contains(&format!("schema_version {}", STORE_SCHEMA_VERSION + 1)),
        "{message}"
    );
    assert!(
        message.contains("fallow 99.1.0"),
        "newest record wins: {message}"
    );
    assert!(message.contains("The store is unchanged"), "{message}");
}

#[test]
fn future_schema_store_with_unknown_shape_is_not_reset() {
    let (_config, dir) = test_env();
    let root = dir.path();
    // A newer schema may change the type of a known field. The typed
    // parse then fails, but the store must not be reset to defaults.
    let future = format!(
        r#"{{"schema_version":{},"enabled":"per-worktree","records":{{"v2":[]}}}}"#,
        STORE_SCHEMA_VERSION + 1
    );
    seed_store_raw(root, future.as_bytes());

    assert!(enable(root).is_err());
    assert!(disable(root).is_err());
    assert_eq!(raw_store(root), future);
}

#[test]
fn corrupt_supported_store_is_still_reset_by_enable() {
    let (_config, dir) = test_env();
    let root = dir.path();
    seed_store_raw(root, b"{not json");
    assert!(enable(root).unwrap());
    assert!(load(root).enabled);
}

#[test]
fn removed_finding_is_credited_as_resolved() {
    let (_config, dir) = test_env();
    let root = dir.path();
    enable(root).unwrap();
    let a = touch(root, "src/a.ts");
    run(
        root,
        &[&a],
        vec![fi(&a, "unused-export", "foo")],
        vec![],
        &[],
        "t0",
    );
    assert_eq!(
        load(root).resolved_total,
        0,
        "first run only establishes a baseline"
    );
    run(root, &[&a], vec![], vec![], &[], "t1");
    let store = load(root);
    assert_eq!(store.resolved_total, 1);
    assert_eq!(store.suppressed_total, 0);
    assert_eq!(store.recent_resolved.len(), 1);
    assert_eq!(store.recent_resolved[0].kind, "unused-export");
    assert_eq!(store.recent_resolved[0].symbol.as_deref(), Some("foo"));
    assert_eq!(store.recent_resolved[0].path, "src/a.ts");
}

#[test]
fn suppressed_finding_is_not_a_win() {
    let (_config, dir) = test_env();
    let root = dir.path();
    enable(root).unwrap();
    let a = touch(root, "src/a.ts");
    run(
        root,
        &[&a],
        vec![fi(&a, "unused-export", "foo")],
        vec![],
        &[],
        "t0",
    );
    run(
        root,
        &[&a],
        vec![],
        vec![],
        &[supp(&a, "unused-export")],
        "t1",
    );
    let store = load(root);
    assert_eq!(
        store.resolved_total, 0,
        "a suppression must never count as a win"
    );
    assert_eq!(store.suppressed_total, 1);
}

#[test]
fn fix_and_suppress_same_kind_credits_zero_resolved() {
    let (_config, dir) = test_env();
    let root = dir.path();
    enable(root).unwrap();
    let a = touch(root, "src/a.ts");
    run(
        root,
        &[&a],
        vec![
            fi(&a, "unused-export", "foo"),
            fi(&a, "unused-export", "bar"),
        ],
        vec![],
        &[],
        "t0",
    );
    run(
        root,
        &[&a],
        vec![],
        vec![],
        &[supp(&a, "unused-export")],
        "t1",
    );
    let store = load(root);
    assert_eq!(store.resolved_total, 0);
    assert_eq!(store.suppressed_total, 2);
}

#[test]
fn within_file_move_is_not_resolved() {
    let (_config, dir) = test_env();
    let root = dir.path();
    enable(root).unwrap();
    let a = touch(root, "src/a.ts");
    run(
        root,
        &[&a],
        vec![fi(&a, "unused-export", "foo")],
        vec![],
        &[],
        "t0",
    );
    run(
        root,
        &[&a],
        vec![fi(&a, "unused-export", "foo")],
        vec![],
        &[],
        "t1",
    );
    let store = load(root);
    assert_eq!(store.resolved_total, 0);
    assert_eq!(store.suppressed_total, 0);
}

#[test]
fn cross_file_move_in_same_run_is_not_resolved() {
    let (_config, dir) = test_env();
    let root = dir.path();
    enable(root).unwrap();
    let a = touch(root, "src/a.ts");
    let b = touch(root, "src/b.ts");
    run(
        root,
        &[&a],
        vec![fi(&a, "unused-export", "foo")],
        vec![],
        &[],
        "t0",
    );
    run(
        root,
        &[&a, &b],
        vec![fi(&b, "unused-export", "foo")],
        vec![],
        &[],
        "t1",
    );
    assert_eq!(
        load(root).resolved_total,
        0,
        "a cross-file move is not a resolution"
    );
}

#[test]
fn cross_run_move_uncredits_the_prior_resolution() {
    let (_config, dir) = test_env();
    let root = dir.path();
    enable(root).unwrap();
    let a = touch(root, "src/a.ts");
    let b = touch(root, "src/b.ts");
    run(
        root,
        &[&a],
        vec![fi(&a, "unused-export", "foo")],
        vec![],
        &[],
        "t0",
    );
    run(root, &[&a], vec![], vec![], &[], "t1");
    assert_eq!(
        load(root).resolved_total,
        1,
        "source disappearance credited in run A"
    );
    run(
        root,
        &[&b],
        vec![fi(&b, "unused-export", "foo")],
        vec![],
        &[],
        "t2",
    );
    let store = load(root);
    assert_eq!(
        store.resolved_total, 0,
        "cross-run move must un-credit the phantom win"
    );
    assert!(
        store.recent_resolved.is_empty(),
        "the stale resolution event is dropped"
    );
}

#[test]
fn resolved_complexity_finding_and_suppressed_complexity() {
    let (_config, dir) = test_env();
    let root = dir.path();
    enable(root).unwrap();
    let a = touch(root, "src/a.ts");
    run(
        root,
        &[&a],
        vec![fi(&a, "complexity", "bigFn")],
        vec![],
        &[],
        "t0",
    );
    run(root, &[&a], vec![], vec![], &[supp(&a, "complexity")], "t1");
    let store = load(root);
    assert_eq!(store.resolved_total, 0);
    assert_eq!(store.suppressed_total, 1);

    let b = touch(root, "src/b.ts");
    run(
        root,
        &[&b],
        vec![fi(&b, "complexity", "huge")],
        vec![],
        &[],
        "t2",
    );
    run(root, &[&b], vec![], vec![], &[], "t3");
    assert_eq!(load(root).resolved_total, 1);
}

#[test]
fn resolved_duplication_clone_group() {
    let (_config, dir) = test_env();
    let root = dir.path();
    enable(root).unwrap();
    let a = touch(root, "src/a.ts");
    let b = touch(root, "src/b.ts");
    let clone = CloneInput {
        fingerprint: "dup:abc12345".to_owned(),
        instance_paths: vec![a.clone(), b],
    };
    run(root, &[&a], vec![], vec![clone], &[], "t0");
    run(root, &[&a], vec![], vec![], &[], "t1");
    let store = load(root);
    assert_eq!(store.resolved_total, 1);
    assert_eq!(store.recent_resolved[0].kind, "code-duplication");
}

#[test]
fn blanket_suppression_covers_any_kind() {
    let (_config, dir) = test_env();
    let root = dir.path();
    enable(root).unwrap();
    let a = touch(root, "src/a.ts");
    run(
        root,
        &[&a],
        vec![fi(&a, "unused-export", "foo")],
        vec![],
        &[],
        "t0",
    );
    let blanket = ActiveSuppression {
        path: a.clone(),
        kind: None,
        is_file_level: true,
        reason: None,
        comment_line: 1,
    };
    run(root, &[&a], vec![], vec![], &[blanket], "t1");
    let store = load(root);
    assert_eq!(store.resolved_total, 0);
    assert_eq!(store.suppressed_total, 1);
}

// ----- gate source: recorded provenance and the schema 6 migration -----

/// Record one gate run with an explicit marker, no attribution.
fn record_gate(root: &Path, marker: Option<&str>, timestamp: &str) {
    record_audit_run(
        root,
        &summary(0, 0, 0),
        &AuditRunRecord {
            verdict: AuditVerdict::Pass,
            gate_source: marker.map(GateSource::from_marker),
            git_sha: None,
            version: "3.0.0",
            timestamp,
            attribution: None,
            analysis_identity: &fallow_types::semantic::SemanticAnalysisIdentity::default(),
        },
    )
    .unwrap();
}

#[test]
fn gate_marker_is_recorded_and_counted_per_source() {
    let (_config, dir) = test_env();
    let root = dir.path();
    enable(root).unwrap();

    record_gate(root, Some("agent"), "t0");
    record_gate(root, Some("agent"), "t1");
    record_gate(root, Some("pre-commit"), "t2");
    record_gate(root, Some("ci"), "t3");
    record_gate(root, Some("nightly-robot"), "t4");
    record_gate(root, None, "t5");

    let counts = build_report(&load(root))
        .gate_runs
        .expect("gate runs must be reported once a gate recorded a run");
    assert_eq!(counts.agent, 2);
    assert_eq!(counts.pre_commit, 1);
    assert_eq!(counts.ci, 1);
    assert_eq!(
        counts.unknown, 1,
        "an unrecognised marker is still a gate run"
    );
}

#[test]
fn report_omits_gate_runs_when_no_gate_recorded() {
    let (_config, dir) = test_env();
    let root = dir.path();
    enable(root).unwrap();
    record_gate(root, None, "t0");

    let report = build_report(&load(root));
    assert!(
        report.gate_runs.is_none(),
        "a store without gate runs must keep the field off the wire"
    );
    let json = render_json(&report);
    assert!(
        !json.contains("gate_runs"),
        "gate_runs must be absent on the unchanged path: {json}"
    );
}

#[test]
fn legacy_gate_bool_migrates_to_unknown_source() {
    let (_config, dir) = test_env();
    let root = dir.path();
    let counts = r#"{"total_issues":0,"dead_code":0,"complexity":0,"duplication":0}"#;
    let v6 = format!(
        r#"{{"schema_version":6,"enabled":true,"first_recorded":"t0","records":[
                {{"timestamp":"t0","version":"3.0.0","verdict":"pass","gate":true,"counts":{counts}}},
                {{"timestamp":"t1","version":"3.0.0","verdict":"pass","gate":false,"counts":{counts}}}
            ],"containment":[]}}"#
    );
    seed_store_raw(root, v6.as_bytes());

    let store = load(root);
    assert_eq!(store.records[0].gate_source, Some(GateSource::Unknown));
    assert_eq!(
        store.records[1].gate_source, None,
        "a legacy non-gate run must not become a gate run"
    );

    let counts = build_report(&store)
        .gate_runs
        .expect("a legacy gate run must survive the migration");
    assert_eq!(counts.unknown, 1);
    assert_eq!(counts.agent + counts.pre_commit + counts.ci, 0);
}

#[test]
fn human_report_names_where_gate_runs_come_from() {
    let (_config, dir) = test_env();
    let root = dir.path();
    enable(root).unwrap();
    record_gate(root, Some("agent"), "t0");
    record_gate(root, Some("pre-commit"), "t1");

    let human = render_human(&build_report(&load(root)));
    assert!(
        human.contains("Where your gate runs come from: agent 1, pre-commit 1."),
        "human report must name the gate sources: {human}"
    );
}

#[test]
fn v1_store_loads_and_upgrades_to_v2() {
    let (_config, dir) = test_env();
    let root = dir.path();
    let v1 = r#"{"schema_version":1,"enabled":true,"first_recorded":"t0","records":[{"timestamp":"t0","version":"2.0.0","verdict":"warn","gate":false,"counts":{"total_issues":1,"dead_code":1,"complexity":0,"duplication":0}}],"containment":[]}"#;
    seed_store_raw(root, v1.as_bytes());
    let store = load(root);
    assert_eq!(store.schema_version, 1);
    assert!(store.frontier.is_empty());
    assert_eq!(store.resolved_total, 0);
    let a = touch(root, "src/a.ts");
    run(
        root,
        &[&a],
        vec![fi(&a, "unused-export", "foo")],
        vec![],
        &[],
        "t1",
    );
    let store = load(root);
    assert_eq!(store.schema_version, STORE_SCHEMA_VERSION);
    assert!(frontier_paths(&store).contains("src/a.ts"));
}

#[test]
fn recent_resolved_is_bounded() {
    let mut store = ImpactStore {
        enabled: true,
        ..Default::default()
    };
    for i in 0..(MAX_RECENT_RESOLVED + 25) {
        store.recent_resolved.push(ResolutionEvent {
            kind: "unused-export".into(),
            path: format!("src/f{i}.ts"),
            symbol: Some(format!("s{i}")),
            git_sha: None,
            timestamp: format!("t{i}"),
        });
    }
    bound_recent_resolved(&mut store);
    assert_eq!(store.recent_resolved.len(), MAX_RECENT_RESOLVED);
    assert_eq!(store.recent_resolved[0].path, "src/f25.ts");
}

#[test]
fn frontier_prunes_deleted_files() {
    let (_config, dir) = test_env();
    let root = dir.path();
    enable(root).unwrap();
    let a = touch(root, "src/a.ts");
    run(
        root,
        &[&a],
        vec![fi(&a, "unused-export", "foo")],
        vec![],
        &[],
        "t0",
    );
    assert!(frontier_paths(&load(root)).contains("src/a.ts"));
    std::fs::remove_file(&a).unwrap();
    let b = touch(root, "src/b.ts");
    run(root, &[&b], vec![], vec![], &[], "t1");
    assert!(!frontier_paths(&load(root)).contains("src/a.ts"));
}

#[test]
fn honest_empty_state_before_attribution_baseline() {
    let store = ImpactStore {
        enabled: true,
        records: vec![ImpactRecord {
            timestamp: "t0".into(),
            version: "2.0.0".into(),
            git_sha: None,
            verdict: "warn".into(),
            gate_source: None,
            counts: ImpactCounts::default(),
        }],
        ..Default::default()
    };
    let report = build_report(&store);
    assert!(!report.attribution_active);
    let human = render_human(&report);
    assert!(human.contains("resolution tracking starts from your next gate run"));
    assert!(!human.contains("0 finding"));
}

#[test]
fn suppression_only_state_renders_under_a_resolved_header() {
    let report = ImpactReport {
        schema_version: ImpactReportSchemaVersion::V1,
        enabled: true,
        enabled_source: EnabledSource::Project,
        record_count: 2,
        meta: None,
        first_recorded: Some("2026-05-29T10:00:00Z".into()),
        latest_git_sha: None,
        surfacing: Some(ImpactCounts::default()),
        trend: None,
        project_surfacing: None,
        project_trend: None,
        gate_runs: None,
        containment_count: 0,
        recent_containment: vec![],
        resolved_total: 0,
        suppressed_total: 2,
        recent_resolved: vec![],
        attribution_active: true,
        onboarding_declined: false,
        explicit_decision: false,
    };
    let human = render_human(&report);
    let resolved_idx = human.find("  RESOLVED").expect("RESOLVED header present");
    let supp_idx = human
        .find("2 findings you marked intentional")
        .expect("suppression line present");
    assert!(
        resolved_idx < supp_idx,
        "suppression must render under RESOLVED"
    );
    assert!(human.contains("none yet"));

    let md = render_markdown(&report);
    assert!(
        md.contains("- **Resolved:**"),
        "markdown always has a Resolved bullet"
    );
    assert!(md.contains("- **Marked intentional:** 2 finding"));
}

/// Build a `CloneInput` over real absolute paths (built from `root`).
fn clone_at(fingerprint: &str, paths: &[&Path]) -> CloneInput {
    CloneInput {
        fingerprint: fingerprint.to_owned(),
        instance_paths: paths.iter().map(|p| p.to_path_buf()).collect(),
    }
}

/// Record a WHOLE-PROJECT run via the real combined-track recorder
/// (`record_combined_run` with `Scope::WholeProject`), exercising the same
/// path `combined.rs` uses on a full `fallow` run.
fn run_wp(
    root: &Path,
    findings: Vec<FindingInput>,
    clones: Vec<CloneInput>,
    supps: &[ActiveSuppression],
    ts: &str,
) {
    let input = AttributionInput {
        root,
        scope: Scope::WholeProject,
        findings,
        clones,
        suppressions: supps,
    };
    record_combined_run(
        root,
        ImpactCounts::default(),
        Some("sha"),
        "2.0.0",
        ts,
        Some(&input),
        &fallow_types::semantic::SemanticAnalysisIdentity::default(),
    )
    .unwrap();
}

#[test]
fn whole_project_run_does_not_double_credit_after_audit() {
    let (_config, dir) = test_env();
    let root = dir.path();
    enable(root).unwrap();
    let a = touch(root, "src/a.ts");
    let b = touch(root, "src/b.ts");
    run(
        root,
        &[&a, &b],
        vec![],
        vec![clone_at("dup:abc", &[&a, &b])],
        &[],
        "t1",
    );
    assert_eq!(clone_fingerprints(&load(root)).len(), 1);

    run(root, &[&a, &b], vec![], vec![], &[], "t2");
    assert_eq!(load(root).resolved_total, 1);
    assert!(load(root).clone_frontier.is_empty());

    run_wp(root, vec![], vec![], &[], "t3");
    assert_eq!(
        load(root).resolved_total,
        1,
        "whole-project run re-credited a resolution"
    );
}

#[test]
fn whole_project_run_credits_suppressed_not_resolved() {
    let (_config, dir) = test_env();
    let root = dir.path();
    enable(root).unwrap();
    let util = touch(root, "src/util.ts");
    run(
        root,
        &[&util],
        vec![fi(&util, "unused-export", "dead")],
        vec![],
        &[],
        "t1",
    );
    assert_eq!(frontier_paths(&load(root)).len(), 1);

    run_wp(root, vec![], vec![], &[supp(&util, "unused-export")], "t2");
    let store = load(root);
    assert_eq!(
        store.suppressed_total, 1,
        "suppressed finding not counted suppressed"
    );
    assert_eq!(
        store.resolved_total, 0,
        "suppressed finding wrongly counted resolved"
    );
}

#[test]
fn clone_reshape_three_to_two_not_credited_as_resolved() {
    let (_config, dir) = test_env();
    let root = dir.path();
    enable(root).unwrap();
    let a = touch(root, "src/a.ts");
    let b = touch(root, "src/b.ts");
    let c = touch(root, "src/c.ts");
    run(
        root,
        &[&a, &b, &c],
        vec![],
        vec![clone_at("dup:aaa", &[&a, &b, &c])],
        &[],
        "t1",
    );
    assert_eq!(clone_fingerprints(&load(root)).len(), 1);

    run_wp(
        root,
        vec![],
        vec![clone_at("dup:bbb", &[&a, &b])],
        &[],
        "t2",
    );
    let store = load(root);
    assert_eq!(
        store.resolved_total, 0,
        "clone reshape miscredited as resolved"
    );
    assert!(clone_fingerprints(&store).contains("dup:bbb"));
    assert!(!clone_fingerprints(&store).contains("dup:aaa"));
}

fn rcounts(total: usize, dead: usize, complexity: usize, dup: usize) -> ImpactCounts {
    ImpactCounts {
        total_issues: total,
        dead_code: dead,
        complexity,
        duplication: dup,
    }
}

fn rtrend(prev: usize, cur: usize) -> TrendSummary {
    TrendSummary {
        direction: direction_for(crate::count_delta::signed_delta(cur, prev)),
        total_delta: crate::count_delta::signed_delta(cur, prev),
        previous_total: prev,
        current_total: cur,
    }
}

/// Build a report literal for render-state tests.
#[expect(
    clippy::too_many_arguments,
    reason = "test scaffold; positional ImpactReport builder, bundling adds churn with no production value"
)]
fn rreport(
    record_count: usize,
    first_recorded: Option<&str>,
    surfacing: Option<ImpactCounts>,
    trend: Option<TrendSummary>,
    project_surfacing: Option<ImpactCounts>,
    project_trend: Option<TrendSummary>,
    attribution_active: bool,
) -> ImpactReport {
    ImpactReport {
        schema_version: ImpactReportSchemaVersion::V1,
        enabled: true,
        enabled_source: EnabledSource::Project,
        record_count,
        meta: None,
        first_recorded: first_recorded.map(ToOwned::to_owned),
        latest_git_sha: None,
        surfacing,
        trend,
        project_surfacing,
        project_trend,
        gate_runs: None,
        containment_count: 0,
        recent_containment: vec![],
        resolved_total: 0,
        suppressed_total: 0,
        recent_resolved: vec![],
        attribution_active,
        onboarding_declined: false,
        explicit_decision: false,
    }
}

#[test]
fn render_human_project_only_store_shows_whole_project_not_empty_state() {
    let r = rreport(
        0,
        Some("2026-05-30T10:00:00Z"),
        None,
        None,
        Some(rcounts(1, 1, 0, 0)),
        None,
        true,
    );
    let human = render_human(&r);
    assert!(
        human.contains("WHOLE PROJECT (whole-repo context, not a to-do)"),
        "project-only must render the labeled section"
    );
    assert!(human.contains("1 issue across the whole project"));
    assert!(
        human.contains("project trend starts after your next full `fallow` run"),
        "single project record => no trend line, shows the next-run hint"
    );
    assert!(human.contains("Tracking since 2026-05-30"));
    assert!(
        !human.contains("No history yet"),
        "must not show the empty-state copy"
    );
    assert!(
        !human.contains("LATEST RUN"),
        "no changed-file track recorded"
    );
    assert!(
        !human.contains("recorded audit run"),
        "no audit runs => no changed-file footer"
    );
}

#[test]
fn render_human_both_tracks_label_actionable_vs_context() {
    let r = rreport(
        3,
        Some("2026-05-29T10:00:00Z"),
        Some(rcounts(4, 4, 0, 0)),
        Some(rtrend(6, 4)),
        Some(rcounts(40, 30, 5, 5)),
        Some(rtrend(45, 40)),
        true,
    );
    let human = render_human(&r);
    let latest = human
        .find("LATEST RUN (changed files, act on these now)")
        .expect("LATEST RUN labeled actionable");
    let whole = human
        .find("WHOLE PROJECT (whole-repo context, not a to-do)")
        .expect("WHOLE PROJECT labeled context");
    assert!(
        latest < whole,
        "changed-file section renders before whole-project"
    );
    assert!(human.contains("45 -> 40 (down) across your last two full runs"));
    assert!(human.contains("advances only on your local full `fallow` runs, not CI"));
}

#[test]
fn render_markdown_project_only_store_shows_whole_project_not_empty_state() {
    let r = rreport(
        0,
        Some("2026-05-30T10:00:00Z"),
        None,
        None,
        Some(rcounts(1, 1, 0, 0)),
        None,
        true,
    );
    let md = render_markdown(&r);
    assert!(
        md.contains("- **Whole project (whole-repo context, last full `fallow` run):** 1 issue"),
        "project-only md must render the labeled whole-project line"
    );
    assert!(
        !md.contains("No history yet"),
        "project-only md must not show empty state"
    );
    assert!(md.contains("Tracking since 2026-05-30"));
}

#[test]
fn resolve_enabled_precedence_table() {
    let (_config, _dir) = test_env();
    // enabled-true is an explicit project opt-in regardless of the flag.
    let on = ImpactStore {
        enabled: true,
        ..Default::default()
    };
    assert_eq!(resolve_enabled(&on), (true, EnabledSource::Project));

    // explicitly disabled here stays off as a Project decision.
    let off_explicit = ImpactStore {
        enabled: false,
        explicit_decision: true,
        ..Default::default()
    };
    assert_eq!(
        resolve_enabled(&off_explicit),
        (false, EnabledSource::Project)
    );

    // never-asked + no global default => off (Default).
    let never = ImpactStore::default();
    assert_eq!(resolve_enabled(&never), (false, EnabledSource::Default));

    // never-asked + global default on => on (User).
    assert!(set_global_default(true));
    assert_eq!(resolve_enabled(&never), (true, EnabledSource::User));
    // a per-repo disable still wins over the global default.
    assert_eq!(
        resolve_enabled(&off_explicit),
        (false, EnabledSource::Project)
    );
}

#[test]
fn human_report_explains_user_global_default() {
    let (_config, _dir) = test_env();
    set_global_default(true);
    // A never-asked store resolved on a project: enabled via the global default.
    let report = build_report(&ImpactStore::default());
    assert_eq!(report.enabled_source, EnabledSource::User);
    let human = render_human(&report);
    assert!(
        human.contains("Enabled by your user-global default"),
        "human report must explain a global-default enable: {human}"
    );
    // A project-enabled report does NOT show the global-default note.
    let project = build_report(&ImpactStore {
        enabled: true,
        explicit_decision: true,
        ..Default::default()
    });
    assert_eq!(project.enabled_source, EnabledSource::Project);
    assert!(!render_human(&project).contains("user-global default"));
}

#[test]
fn global_default_round_trips() {
    let (_config, _dir) = test_env();
    assert!(!load_global_default());
    assert!(set_global_default(true));
    assert!(load_global_default());
    assert!(!set_global_default(true)); // unchanged
    assert!(set_global_default(false));
    assert!(!load_global_default());
}

#[test]
fn global_default_records_without_per_repo_enable() {
    let (_config, dir) = test_env();
    let root = dir.path();
    set_global_default(true);
    // No `enable(root)` call: the global default alone should activate.
    record_v1(
        root,
        &summary(2, 0, 0),
        AuditVerdict::Warn,
        false,
        None,
        "2.0.0",
        "t0",
    );
    let report = build_report(&load(root));
    assert!(report.enabled);
    assert_eq!(report.enabled_source, EnabledSource::User);
    assert_eq!(report.record_count, 1);
}

#[test]
fn legacy_in_repo_store_is_migrated_on_first_load() {
    let (_config, dir) = test_env();
    let root = dir.path();
    // Seed a pre-relocation v3 store with a FLAT frontier in the repo.
    let legacy = r#"{"schema_version":3,"enabled":true,"explicit_decision":true,
            "records":[{"timestamp":"t0","version":"2.0.0","verdict":"warn","gate":false,
            "counts":{"total_issues":1,"dead_code":1,"complexity":0,"duplication":0}}],
            "resolved_total":2,
            "frontier":{"src/a.ts":{"findings":[{"id":"x","kind":"unused-export","symbol":"foo"}],"suppressions":[]}},
            "containment":[]}"#;
    std::fs::create_dir_all(root.join(".fallow")).unwrap();
    std::fs::write(legacy_store_path(root), legacy).unwrap();

    let store = load(root);
    assert!(store.enabled);
    assert_eq!(store.schema_version, STORE_SCHEMA_VERSION);
    assert_eq!(store.records.len(), 1);
    assert_eq!(store.resolved_total, 2);
    // The flat frontier was wrapped under the worktree key (nested v4 shape).
    assert!(frontier_paths(&store).contains("src/a.ts"));
    // The user store now exists, so a second load does NOT re-import (it
    // reads the user store directly).
    assert!(store_path(root).is_some_and(|p| p.exists()));
    let again = load(root);
    assert_eq!(again.records.len(), 1);
}

#[test]
fn reset_removes_only_this_project() {
    let (_config, dir) = test_env();
    let root = dir.path();
    enable(root).unwrap();
    record_v1(
        root,
        &summary(1, 0, 0),
        AuditVerdict::Warn,
        false,
        None,
        "2.0.0",
        "t0",
    );
    assert_eq!(load(root).records.len(), 1);
    assert!(reset(root));
    assert!(load(root).records.is_empty());
    assert!(!reset(root)); // already gone
}

#[test]
fn reset_all_clears_dir_but_keeps_global_default() {
    let (_config, dir) = test_env();
    let root = dir.path();
    set_global_default(true);
    enable(root).unwrap();
    assert!(load(root).enabled);
    assert!(reset_all());
    // The global default toggle survives a data wipe.
    assert!(load_global_default());
}

// ----- cross-repo aggregate (`impact --all`) tests --------------------

/// Set an isolated config dir (no project root needed) and return its guard.
fn aggregate_env() -> tempfile::TempDir {
    let config = tempfile::tempdir().unwrap();
    TEST_CONFIG_DIR.with(|c| *c.borrow_mut() = Some(config.path().to_path_buf()));
    config
}

/// Write a store file directly under `<config>/impact/<key>.json`.
fn seed_store(key: &str, store: &ImpactStore) {
    let dir = impact_config_dir().unwrap().join("impact");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join(format!("{key}.json")),
        serde_json::to_string_pretty(store).unwrap(),
    )
    .unwrap();
}

fn store_with(
    label: &str,
    resolved: usize,
    contained: usize,
    latest_ts: &str,
    latest_issues: usize,
) -> ImpactStore {
    let mut s = ImpactStore {
        enabled: true,
        explicit_decision: true,
        resolved_total: resolved,
        label: Some(label.to_owned()),
        ..Default::default()
    };
    s.records.push(ImpactRecord {
        timestamp: latest_ts.to_owned(),
        version: "2.0.0".to_owned(),
        git_sha: None,
        verdict: "warn".to_owned(),
        gate_source: None,
        counts: ImpactCounts::from_combined(latest_issues, 0, 0),
    });
    for _ in 0..contained {
        s.containment.push(ContainmentEvent {
            blocked_at: "t0".to_owned(),
            cleared_at: "t1".to_owned(),
            git_sha: None,
            blocked_counts: ImpactCounts::default(),
        });
    }
    s
}

#[test]
fn repo_basename_returns_last_component_only() {
    assert_eq!(
        repo_basename(Path::new("/a/b/myrepo/.git")).as_deref(),
        Some("myrepo")
    );
    assert_eq!(
        repo_basename(Path::new("/a/b/proj")).as_deref(),
        Some("proj")
    );
    // Never a separator in the result.
    let name = repo_basename(Path::new("/x/y/z/.git")).unwrap();
    assert!(!name.contains('/') && !name.contains('\\'));
}

#[test]
fn aggregate_rolls_up_totals_and_excludes_empty() {
    let _cfg = aggregate_env();
    seed_store(
        "aaa",
        &store_with("alpha", 10, 2, "2026-06-10T00:00:00Z", 3),
    );
    seed_store("bbb", &store_with("beta", 5, 1, "2026-06-11T00:00:00Z", 0));
    // enabled-but-empty: no records, no resolved, no containment.
    seed_store(
        "ccc",
        &ImpactStore {
            enabled: true,
            explicit_decision: true,
            label: Some("gamma".into()),
            ..Default::default()
        },
    );
    let report = aggregate(CrossRepoSort::Recent);
    assert_eq!(report.project_count, 3, "all three stores enumerated");
    assert_eq!(report.tracked_count, 2, "empty store excluded from rows");
    assert_eq!(report.totals.resolved_total, 15);
    assert_eq!(report.totals.containment_count, 3);
    assert_eq!(report.unreadable_count, 0);
}

#[test]
fn aggregate_sort_recent_orders_by_last_activity() {
    let _cfg = aggregate_env();
    seed_store("old", &store_with("older", 1, 0, "2026-06-01T00:00:00Z", 1));
    seed_store("new", &store_with("newer", 1, 0, "2026-06-12T00:00:00Z", 1));
    let report = aggregate(CrossRepoSort::Recent);
    assert_eq!(report.projects[0].label.as_deref(), Some("newer"));
    assert_eq!(report.projects[1].label.as_deref(), Some("older"));
}

#[test]
fn cross_repo_json_carries_kind_and_leaks_no_path() {
    let _cfg = aggregate_env();
    seed_store("aaa", &store_with("alpha", 4, 1, "2026-06-10T00:00:00Z", 2));
    let report = aggregate(CrossRepoSort::Recent);
    let json = render_cross_repo_json(&report);
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(value["kind"], "impact-cross-repo");
    // No label (or any string) may contain a path separator.
    for entry in value["projects"].as_array().unwrap() {
        if let Some(label) = entry["label"].as_str() {
            assert!(
                !label.contains('/') && !label.contains('\\'),
                "label must be a basename, got {label}"
            );
        }
    }
    assert!(
        !json.contains('/') || !json.contains("Users"),
        "json must not leak an absolute home path"
    );
}

#[test]
fn cross_repo_markdown_pluralizes_single_project() {
    let _cfg = aggregate_env();
    seed_store("solo", &store_with("solo", 3, 1, "2026-06-10T00:00:00Z", 2));
    let report = aggregate(CrossRepoSort::Recent);
    assert_eq!(report.project_count, 1);
    assert_eq!(report.tracked_count, 1);
    let md = render_cross_repo_markdown(&report);
    assert!(
        md.contains("1 project tracked"),
        "single project must read 'project', got:\n{md}"
    );
    assert!(
        !md.contains("1 projects tracked"),
        "must not pluralize a single project, got:\n{md}"
    );
    assert!(
        md.contains("across 1 tracked project"),
        "grand totals must read 'tracked project' (singular), got:\n{md}"
    );
    assert!(
        !md.contains("tracked projects"),
        "must not pluralize a single tracked project, got:\n{md}"
    );
}

#[test]
fn cross_repo_corrupt_file_is_skipped_and_counted() {
    let _cfg = aggregate_env();
    seed_store("good", &store_with("good", 3, 0, "2026-06-10T00:00:00Z", 1));
    let dir = impact_config_dir().unwrap().join("impact");
    std::fs::write(dir.join("bad.json"), b"{ not valid json ][").unwrap();
    let report = aggregate(CrossRepoSort::Recent);
    assert_eq!(report.tracked_count, 1, "good store still aggregated");
    assert_eq!(
        report.unreadable_count, 1,
        "corrupt file counted, not crashed"
    );
}

#[test]
fn cross_repo_empty_dir_is_first_run() {
    let _cfg = aggregate_env();
    let report = aggregate(CrossRepoSort::Recent);
    assert_eq!(report.project_count, 0);
    let human = render_cross_repo_human(&report, None);
    assert!(human.contains("No projects tracked yet"));
}

#[test]
fn cross_repo_all_corrupt_reports_unreadable_not_first_run() {
    let _cfg = aggregate_env();
    let dir = impact_config_dir().unwrap().join("impact");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("bad.json"), b"{ broken ][").unwrap();
    let report = aggregate(CrossRepoSort::Recent);
    assert_eq!(report.project_count, 0);
    assert_eq!(report.unreadable_count, 1);
    let human = render_cross_repo_human(&report, None);
    assert!(
        human.contains("unreadable store") && !human.contains("No projects tracked yet"),
        "all-corrupt must report unreadable, not a misleading first-run hint: {human}"
    );
}

#[test]
fn record_audit_run_captures_basename_label() {
    let (_config, dir) = test_env();
    let root = dir.path();
    enable(root).unwrap();
    record_v1(
        root,
        &summary(1, 0, 0),
        AuditVerdict::Warn,
        false,
        None,
        "2.0.0",
        "t0",
    );
    let label = load(root).label.expect("label captured on record");
    assert!(
        !label.contains('/') && !label.contains('\\'),
        "label must be a basename, got {label}"
    );
}

// ----- store advisory lock + age-based GC --------------------------------

#[test]
fn lock_path_appends_lock_suffix() {
    assert_eq!(
        lock_path_for(Path::new("/c/fallow/impact/abc.json")),
        PathBuf::from("/c/fallow/impact/abc.json.lock")
    );
}

#[test]
fn store_lock_acquire_drop_then_record_roundtrips() {
    let (_config, dir) = test_env();
    let root = dir.path();
    enable(root).unwrap();
    // Acquiring + dropping the lock around a record must not deadlock and
    // the record must persist.
    {
        let _lock = ImpactStoreLock::acquire(root).expect("lock acquires");
    }
    record_v1(
        root,
        &summary(1, 0, 0),
        AuditVerdict::Warn,
        false,
        None,
        "2.0.0",
        "t0",
    );
    assert_eq!(load(root).records.len(), 1, "record persisted under lock");
    // The lock sidecar lives next to the store and is never the store itself.
    let store = store_path(root).unwrap();
    assert!(lock_path_for(&store).exists(), "lock sidecar created");
    assert!(store.exists(), "store file is distinct from its lock");
}

#[test]
fn sweep_keeps_fresh_and_self_deletes_aged_out() {
    let _cfg = aggregate_env();
    seed_store("keepme", &store_with("keep", 1, 0, "t0", 1));
    seed_store("oldone", &store_with("old", 1, 0, "t0", 1));
    // A `.lock` sidecar must survive the sweep (lock-lifecycle invariant).
    let lock = impact_config_dir()
        .unwrap()
        .join("impact")
        .join("oldone.json.lock");
    std::fs::write(&lock, b"").unwrap();

    // max_age = 0 ages out every non-kept store regardless of mtime.
    sweep_old_stores("keepme", std::time::Duration::ZERO);

    let dir = impact_config_dir().unwrap().join("impact");
    assert!(dir.join("keepme.json").exists(), "kept store survives");
    assert!(
        !dir.join("oldone.json").exists(),
        "aged-out store reclaimed"
    );
    assert!(lock.exists(), "lock sidecar never deleted by the sweep");
}

#[test]
fn sweep_keeps_everything_under_a_large_window() {
    let _cfg = aggregate_env();
    seed_store("a", &store_with("a", 1, 0, "t0", 1));
    seed_store("b", &store_with("b", 1, 0, "t0", 1));
    // 10-year window: freshly-written stores are never aged out.
    sweep_old_stores("a", std::time::Duration::from_hours(10 * 365 * 24));
    let dir = impact_config_dir().unwrap().join("impact");
    assert!(dir.join("a.json").exists());
    assert!(dir.join("b.json").exists());
}

// ----- LegacyFlatStore::into_store with empty frontiers -----------------

#[test]
#[cfg_attr(miri, ignore)]
fn legacy_into_store_with_empty_frontiers_does_not_insert_worktree_key() {
    // When a legacy store has no frontier or clone_frontier entries, the
    // resulting v4 store must not insert empty sub-maps for the worktree key.
    let legacy = LegacyFlatStore {
        enabled: true,
        explicit_decision: true,
        first_recorded: Some("t0".to_owned()),
        records: vec![],
        project_records: vec![],
        containment: vec![],
        pending_containment: None,
        frontier: FxHashMap::default(),
        clone_frontier: FxHashMap::default(),
        resolved_total: 0,
        suppressed_total: 0,
        recent_resolved: vec![],
        onboarding_declined: false,
        last_digest_epoch: None,
    };
    let store = legacy.into_store("wt-key");
    assert!(
        store.frontier.is_empty(),
        "empty legacy frontier must not insert a worktree key"
    );
    assert!(
        store.clone_frontier.is_empty(),
        "empty legacy clone_frontier must not insert a worktree key"
    );
    assert_eq!(store.schema_version, STORE_SCHEMA_VERSION);
    assert!(store.label.is_none(), "label is always None on migration");
}

// ----- repo_basename edge case: path with no parent ---------------------

#[test]
fn repo_basename_non_git_path_returns_last_component() {
    // A non-.git directory name returns its own basename.
    assert_eq!(
        repo_basename(Path::new("/a/b/myproject")).as_deref(),
        Some("myproject")
    );
}

#[test]
fn repo_basename_root_path_returns_none() {
    // A root-only path has no file_name; must return None, not panic.
    assert!(repo_basename(Path::new("/")).is_none());
}

// ----- direction_for: stable and declining branches ---------------------

#[test]
fn direction_for_declining_when_delta_positive() {
    assert_eq!(direction_for(1), ImpactTrendDirection::Declining);
    assert_eq!(direction_for(100), ImpactTrendDirection::Declining);
}

#[test]
fn direction_for_stable_when_delta_zero() {
    assert_eq!(direction_for(0), ImpactTrendDirection::Stable);
}

#[test]
fn direction_for_improving_when_delta_negative() {
    assert_eq!(direction_for(-1), ImpactTrendDirection::Improving);
}

// ----- trend_arrow covers all three directions -------------------------

#[test]
fn trend_arrow_all_directions() {
    assert_eq!(trend_arrow(ImpactTrendDirection::Improving), "down");
    assert_eq!(trend_arrow(ImpactTrendDirection::Declining), "up");
    assert_eq!(trend_arrow(ImpactTrendDirection::Stable), "flat");
}

// ----- build_report project_records trend is populated -----------------

#[test]
fn build_report_project_trend_populated_from_project_records() {
    let mut store = ImpactStore {
        enabled: true,
        ..Default::default()
    };
    for total in [20usize, 15usize] {
        store.project_records.push(ImpactRecord {
            timestamp: format!("t{total}"),
            version: "2.0.0".into(),
            git_sha: None,
            verdict: "warn".into(),
            gate_source: None,
            counts: ImpactCounts {
                total_issues: total,
                dead_code: total,
                complexity: 0,
                duplication: 0,
            },
        });
    }
    let report = build_report(&store);
    let pt = report
        .project_trend
        .expect("two project records yield a trend");
    assert_eq!(pt.direction, ImpactTrendDirection::Improving);
    assert_eq!(pt.previous_total, 20);
    assert_eq!(pt.current_total, 15);
    assert_eq!(pt.total_delta, -5);
}

// ----- latest_activity selects the max timestamp across both series -----

#[test]
fn latest_activity_both_series_returns_newer() {
    let mut store = ImpactStore::default();
    store.records.push(ImpactRecord {
        timestamp: "2026-01-01T00:00:00Z".to_owned(),
        version: "2.0.0".to_owned(),
        git_sha: None,
        verdict: "warn".to_owned(),
        gate_source: None,
        counts: ImpactCounts::default(),
    });
    store.project_records.push(ImpactRecord {
        timestamp: "2026-06-15T00:00:00Z".to_owned(),
        version: "2.0.0".to_owned(),
        git_sha: None,
        verdict: "warn".to_owned(),
        gate_source: None,
        counts: ImpactCounts::default(),
    });
    assert_eq!(
        latest_activity(&store).as_deref(),
        Some("2026-06-15T00:00:00Z")
    );
}

#[test]
fn latest_activity_only_project_records() {
    let mut store = ImpactStore::default();
    store.project_records.push(ImpactRecord {
        timestamp: "2026-05-01T00:00:00Z".to_owned(),
        version: "2.0.0".to_owned(),
        git_sha: None,
        verdict: "pass".to_owned(),
        gate_source: None,
        counts: ImpactCounts::default(),
    });
    assert_eq!(
        latest_activity(&store).as_deref(),
        Some("2026-05-01T00:00:00Z")
    );
}

#[test]
fn latest_activity_empty_store_returns_none() {
    assert!(latest_activity(&ImpactStore::default()).is_none());
}

// ----- apply_containment: containment overflow capped at MAX_CONTAINMENT --

#[test]
#[cfg_attr(miri, ignore)]
fn containment_events_are_bounded_at_max_containment() {
    let (_config, dir) = test_env();
    let root = dir.path();
    enable(root).unwrap();
    // Directly pre-fill the store with MAX_CONTAINMENT events.
    let mut store = load(root);
    for i in 0..MAX_CONTAINMENT {
        store.containment.push(ContainmentEvent {
            blocked_at: format!("block{i}"),
            cleared_at: format!("clear{i}"),
            git_sha: None,
            blocked_counts: ImpactCounts::default(),
        });
    }
    save(&store, root);

    // Trigger one more containment cycle (fail then pass).
    record_v1(
        root,
        &summary(1, 0, 0),
        AuditVerdict::Fail,
        true,
        None,
        "2.0.0",
        "overflow_block",
    );
    record_v1(
        root,
        &summary(0, 0, 0),
        AuditVerdict::Pass,
        true,
        None,
        "2.0.0",
        "overflow_clear",
    );
    let store = load(root);
    assert_eq!(
        store.containment.len(),
        MAX_CONTAINMENT,
        "containment must be capped at MAX_CONTAINMENT"
    );
    // The most recent event is at the end.
    assert_eq!(
        store.containment.last().unwrap().blocked_at,
        "overflow_block"
    );
}

// ----- disable from enabled state sets schema_version ------------------

#[test]
#[cfg_attr(miri, ignore)]
fn disable_from_enabled_state_sets_schema_version() {
    let (_config, dir) = test_env();
    let root = dir.path();
    enable(root).unwrap();
    let was_newly_disabled = disable(root).unwrap();
    assert!(
        was_newly_disabled,
        "disable returns true when previously enabled"
    );
    let store = load(root);
    assert!(!store.enabled);
    assert!(store.explicit_decision);
    assert_eq!(store.schema_version, STORE_SCHEMA_VERSION);

    // A second disable is a no-op (already off).
    let again = disable(root).unwrap();
    assert!(!again, "disable returns false when already off");
}

// ----- record_combined_run: CI gate and project_records bounded ---------

#[test]
#[cfg_attr(miri, ignore)]
fn record_combined_run_is_noop_in_ci() {
    let (_config, dir) = test_env();
    let root = dir.path();
    enable(root).unwrap();
    TEST_FORCE_CI.with(|c| c.set(true));
    record_combined_run(
        root,
        ImpactCounts::from_combined(5, 0, 0),
        None,
        "2.0.0",
        "t0",
        None,
        &fallow_types::semantic::SemanticAnalysisIdentity::default(),
    )
    .unwrap();
    TEST_FORCE_CI.with(|c| c.set(false));
    assert!(
        load(root).project_records.is_empty(),
        "combined run must not record on CI"
    );
}

#[test]
#[cfg_attr(miri, ignore)]
fn record_combined_run_is_noop_when_disabled() {
    let (_config, dir) = test_env();
    let root = dir.path();
    // store is disabled by default; no enable() call.
    record_combined_run(
        root,
        ImpactCounts::from_combined(3, 0, 0),
        None,
        "2.0.0",
        "t0",
        None,
        &fallow_types::semantic::SemanticAnalysisIdentity::default(),
    )
    .unwrap();
    assert!(
        load(root).project_records.is_empty(),
        "combined run must not record when disabled"
    );
}

#[test]
#[cfg_attr(miri, ignore)]
fn record_combined_run_rejects_incompatible_analysis_history() {
    let (_config, dir) = test_env();
    let root = dir.path();
    enable(root).unwrap();
    record_combined_run(
        root,
        ImpactCounts::from_combined(1, 0, 0),
        None,
        "2.0.0",
        "t0",
        None,
        &fallow_types::semantic::SemanticAnalysisIdentity::default(),
    )
    .unwrap();

    let type_aware = fallow_types::semantic::SemanticAnalysisIdentity {
        mode: fallow_types::semantic::SemanticAnalysisMode::TypeAware,
        semantic_schema_version: 1,
        capabilities: vec![fallow_types::semantic::SemanticCapability::TypeCoupling],
        project_config_hash: "sha256:test".to_string(),
        backend_family: "typescript-go".to_string(),
        completeness: fallow_types::semantic::SemanticCompleteness::Complete,
    };
    let error = record_combined_run(
        root,
        ImpactCounts::from_combined(1, 0, 0),
        None,
        "2.0.0",
        "t1",
        None,
        &type_aware,
    )
    .expect_err("mixed semantic history must be rejected");

    assert!(error.contains("mode"), "{error}");
    assert!(error.contains("fallow impact reset"), "{error}");
}

#[test]
#[cfg_attr(miri, ignore)]
fn record_combined_run_project_records_bounded_at_max() {
    let (_config, dir) = test_env();
    let root = dir.path();
    enable(root).unwrap();
    // Pre-fill to MAX_RECORDS.
    let mut store = load(root);
    for i in 0..MAX_RECORDS {
        store.project_records.push(ImpactRecord {
            timestamp: format!("t{i}"),
            version: "2.0.0".to_owned(),
            git_sha: None,
            verdict: "warn".to_owned(),
            gate_source: None,
            counts: ImpactCounts::default(),
        });
    }
    save(&store, root);

    record_combined_run(
        root,
        ImpactCounts::from_combined(1, 0, 0),
        None,
        "2.0.0",
        "overflow",
        None,
        &fallow_types::semantic::SemanticAnalysisIdentity::default(),
    )
    .unwrap();
    let store = load(root);
    assert_eq!(
        store.project_records.len(),
        MAX_RECORDS,
        "project_records must be capped at MAX_RECORDS"
    );
    assert_eq!(
        store.project_records.last().unwrap().timestamp,
        "overflow",
        "newest record must be at the tail"
    );
}

// ----- clone_dup_suppressed: clone disappearance with suppression -------

#[test]
#[cfg_attr(miri, ignore)]
fn suppressed_clone_disappearance_credits_suppressed_not_resolved() {
    let (_config, dir) = test_env();
    let root = dir.path();
    enable(root).unwrap();
    let a = touch(root, "src/a.ts");
    let b = touch(root, "src/b.ts");
    let clone = CloneInput {
        fingerprint: "dup:suppressed".to_owned(),
        instance_paths: vec![a.clone(), b.clone()],
    };
    run(root, &[&a, &b], vec![], vec![clone], &[], "t0");
    // Second run: clone disappears AND a blanket suppression appears on `a`.
    let blanket = ActiveSuppression {
        path: a.clone(),
        kind: None,
        is_file_level: true,
        reason: None,
        comment_line: 1,
    };
    run(root, &[&a, &b], vec![], vec![], &[blanket], "t1");
    let store = load(root);
    assert_eq!(
        store.suppressed_total, 1,
        "suppressed clone disappearance must count as suppressed"
    );
    assert_eq!(
        store.resolved_total, 0,
        "suppressed clone must not count as resolved"
    );
}

// ----- load_all skips .lock and non-.json files -------------------------

#[test]
#[cfg_attr(miri, ignore)]
fn load_all_skips_non_json_and_lock_files() {
    let _cfg = aggregate_env();
    seed_store("real", &store_with("real", 2, 0, "2026-06-10T00:00:00Z", 1));
    let dir = impact_config_dir().unwrap().join("impact");
    // Write a .lock sidecar and a non-json file that should both be ignored.
    std::fs::write(dir.join("real.json.lock"), b"").unwrap();
    std::fs::write(dir.join("notes.txt"), b"ignored").unwrap();
    let (stores, unreadable) = load_all();
    assert_eq!(stores.len(), 1, "only the .json store is loaded");
    assert_eq!(
        unreadable, 0,
        "lock and txt files are not counted unreadable"
    );
}

// ----- build_aggregate_report: project_wide_issues totalling ------------

#[test]
fn aggregate_totals_project_wide_issues_from_project_surfacing() {
    // Build stores directly (no fs involvement for this pure test).
    let mut s1 = ImpactStore {
        enabled: true,
        explicit_decision: true,
        resolved_total: 3,
        ..Default::default()
    };
    s1.records.push(ImpactRecord {
        timestamp: "t1".to_owned(),
        version: "2.0.0".to_owned(),
        git_sha: None,
        verdict: "warn".to_owned(),
        gate_source: None,
        counts: ImpactCounts::from_combined(5, 0, 0),
    });
    // Add a project_records entry so project_surfacing is non-None.
    s1.project_records.push(ImpactRecord {
        timestamp: "pt1".to_owned(),
        version: "2.0.0".to_owned(),
        git_sha: None,
        verdict: "warn".to_owned(),
        gate_source: None,
        counts: ImpactCounts::from_combined(20, 0, 0),
    });

    let mut s2 = ImpactStore {
        enabled: true,
        explicit_decision: true,
        resolved_total: 7,
        ..Default::default()
    };
    s2.records.push(ImpactRecord {
        timestamp: "t2".to_owned(),
        version: "2.0.0".to_owned(),
        git_sha: None,
        verdict: "warn".to_owned(),
        gate_source: None,
        counts: ImpactCounts::from_combined(2, 0, 0),
    });
    s2.project_records.push(ImpactRecord {
        timestamp: "pt2".to_owned(),
        version: "2.0.0".to_owned(),
        git_sha: None,
        verdict: "warn".to_owned(),
        gate_source: None,
        counts: ImpactCounts::from_combined(30, 0, 0),
    });

    let report = build_aggregate_report(
        vec![("k1".to_owned(), s1), ("k2".to_owned(), s2)],
        0,
        CrossRepoSort::Recent,
    );
    assert_eq!(report.totals.resolved_total, 10);
    assert_eq!(report.totals.project_wide_issues, 50);
    assert_eq!(report.totals.projects_with_baseline, 2);
}

// ----- sort_cross_repo: all sort variants --------------------------------

#[test]
fn aggregate_sort_resolved_orders_by_resolved_total() {
    let _cfg = aggregate_env();
    seed_store("low", &store_with("low", 1, 0, "2026-06-10T00:00:00Z", 1));
    seed_store("high", &store_with("high", 9, 0, "2026-06-09T00:00:00Z", 1));
    let report = aggregate(CrossRepoSort::Resolved);
    assert_eq!(report.projects[0].label.as_deref(), Some("high"));
    assert_eq!(report.projects[1].label.as_deref(), Some("low"));
}

#[test]
fn aggregate_sort_contained_orders_by_containment_count() {
    let _cfg = aggregate_env();
    seed_store("none", &store_with("none", 1, 0, "2026-06-10T00:00:00Z", 1));
    seed_store("many", &store_with("many", 1, 3, "2026-06-09T00:00:00Z", 1));
    let report = aggregate(CrossRepoSort::Contained);
    assert_eq!(report.projects[0].label.as_deref(), Some("many"));
    assert_eq!(report.projects[1].label.as_deref(), Some("none"));
}

#[test]
fn aggregate_sort_name_orders_alphabetically_by_label() {
    let _cfg = aggregate_env();
    seed_store("zzz", &store_with("zulu", 1, 0, "2026-06-10T00:00:00Z", 1));
    seed_store("aaa", &store_with("alpha", 1, 0, "2026-06-09T00:00:00Z", 1));
    let report = aggregate(CrossRepoSort::Name);
    assert_eq!(report.projects[0].label.as_deref(), Some("alpha"));
    assert_eq!(report.projects[1].label.as_deref(), Some("zulu"));
}

// ----- render_cross_repo_human: limit and overflow line -----------------

#[test]
fn render_cross_repo_human_limit_shows_overflow_line() {
    let _cfg = aggregate_env();
    seed_store("a", &store_with("alpha", 1, 0, "2026-06-10T00:00:00Z", 1));
    seed_store("b", &store_with("beta", 2, 0, "2026-06-09T00:00:00Z", 1));
    seed_store("c", &store_with("gamma", 3, 0, "2026-06-08T00:00:00Z", 1));
    let report = aggregate(CrossRepoSort::Recent);
    let human = render_cross_repo_human(&report, Some(2));
    assert!(
        human.contains("and 1 more"),
        "overflow line missing when limit < tracked_count: {human}"
    );
}

#[test]
fn render_cross_repo_human_no_limit_shows_all_rows() {
    let _cfg = aggregate_env();
    seed_store("a", &store_with("alpha", 1, 0, "2026-06-10T00:00:00Z", 1));
    seed_store("b", &store_with("beta", 2, 0, "2026-06-09T00:00:00Z", 1));
    let report = aggregate(CrossRepoSort::Recent);
    let human = render_cross_repo_human(&report, None);
    assert!(
        !human.contains("more (raise --limit"),
        "no limit => no overflow line: {human}"
    );
    assert!(human.contains("alpha"));
    assert!(human.contains("beta"));
}

// ----- render_cross_repo_human: skipped (no-history) and unreadable ----

#[test]
fn render_cross_repo_human_shows_no_history_count() {
    let _cfg = aggregate_env();
    seed_store(
        "empty",
        &ImpactStore {
            enabled: true,
            explicit_decision: true,
            ..Default::default()
        },
    );
    seed_store("full", &store_with("full", 1, 0, "t0", 1));
    let report = aggregate(CrossRepoSort::Recent);
    let human = render_cross_repo_human(&report, None);
    assert!(
        human.contains("tracked project") && human.contains("no history yet"),
        "must report the no-history count: {human}"
    );
}

#[test]
fn render_cross_repo_human_shows_skipped_unreadable() {
    let _cfg = aggregate_env();
    seed_store("good", &store_with("good", 1, 0, "t0", 1));
    let dir = impact_config_dir().unwrap().join("impact");
    std::fs::write(dir.join("corrupt.json"), b"}{broken").unwrap();
    let report = aggregate(CrossRepoSort::Recent);
    let human = render_cross_repo_human(&report, None);
    assert!(
        human.contains("skipped") && human.contains("unreadable store"),
        "must show the skipped unreadable count: {human}"
    );
}

// ----- render_cross_repo_totals: project_wide line ----------------------

#[test]
fn render_cross_repo_human_grand_totals_shows_project_wide_when_present() {
    let _cfg = aggregate_env();
    let mut s = store_with("proj", 5, 1, "2026-06-10T00:00:00Z", 4);
    // Add a project_records entry so project_surfacing is populated.
    s.project_records.push(ImpactRecord {
        timestamp: "pt".to_owned(),
        version: "2.0.0".to_owned(),
        git_sha: None,
        verdict: "warn".to_owned(),
        gate_source: None,
        counts: ImpactCounts::from_combined(42, 0, 0),
    });
    seed_store("proj", &s);
    let report = aggregate(CrossRepoSort::Recent);
    let human = render_cross_repo_human(&report, None);
    assert!(
        human.contains("42 issue") && human.contains("project-wide"),
        "grand totals must include the project-wide line: {human}"
    );
}

// ----- render_human_resolved_section: resolved events without symbol ----

#[test]
fn render_human_resolved_event_without_symbol_omits_symbol() {
    let report = ImpactReport {
        schema_version: ImpactReportSchemaVersion::V1,
        enabled: true,
        enabled_source: EnabledSource::Project,
        record_count: 2,
        meta: None,
        first_recorded: Some("2026-05-01T00:00:00Z".into()),
        latest_git_sha: None,
        surfacing: Some(ImpactCounts::default()),
        trend: None,
        project_surfacing: None,
        project_trend: None,
        gate_runs: None,
        containment_count: 0,
        recent_containment: vec![],
        resolved_total: 1,
        suppressed_total: 0,
        recent_resolved: vec![ResolutionEvent {
            kind: "unused-file".to_owned(),
            path: "src/dead.ts".to_owned(),
            symbol: None,
            git_sha: None,
            timestamp: "t1".to_owned(),
        }],
        attribution_active: true,
        onboarding_declined: false,
        explicit_decision: true,
    };
    let human = render_human(&report);
    // Resolved event without a symbol prints "kind in path", never "None".
    assert!(
        human.contains("unused-file in src/dead.ts"),
        "no-symbol event must show 'kind in path': {human}"
    );
    assert!(!human.contains("None"), "must not stringify None: {human}");
}

// ----- render_markdown: disabled state and enabled with trend -----------

#[test]
fn render_markdown_disabled_shows_enable_hint() {
    let report = ImpactReport {
        schema_version: ImpactReportSchemaVersion::V1,
        enabled: false,
        enabled_source: EnabledSource::Default,
        record_count: 0,
        meta: None,
        first_recorded: None,
        latest_git_sha: None,
        surfacing: None,
        trend: None,
        project_surfacing: None,
        project_trend: None,
        gate_runs: None,
        containment_count: 0,
        recent_containment: vec![],
        resolved_total: 0,
        suppressed_total: 0,
        recent_resolved: vec![],
        attribution_active: false,
        onboarding_declined: false,
        explicit_decision: false,
    };
    let md = render_markdown(&report);
    assert!(
        md.contains("Impact tracking is off"),
        "disabled markdown must show enable hint: {md}"
    );
    assert!(md.contains("fallow impact enable"));
}

#[test]
fn render_markdown_with_trend_shows_trend_line() {
    let report = ImpactReport {
        schema_version: ImpactReportSchemaVersion::V1,
        enabled: true,
        enabled_source: EnabledSource::Project,
        record_count: 2,
        meta: None,
        first_recorded: Some("2026-05-01T00:00:00Z".into()),
        latest_git_sha: None,
        surfacing: Some(ImpactCounts::from_combined(3, 3, 0)),
        trend: Some(TrendSummary {
            direction: ImpactTrendDirection::Declining,
            total_delta: 2,
            previous_total: 1,
            current_total: 3,
        }),
        project_surfacing: None,
        project_trend: None,
        gate_runs: None,
        containment_count: 0,
        recent_containment: vec![],
        resolved_total: 0,
        suppressed_total: 0,
        recent_resolved: vec![],
        attribution_active: false,
        onboarding_declined: false,
        explicit_decision: true,
    };
    let md = render_markdown(&report);
    assert!(
        md.contains("Trend (changed-file scope"),
        "markdown with trend must show trend line: {md}"
    );
    assert!(md.contains("1 -> 3 (up)"), "trend values present: {md}");
}

#[test]
fn render_markdown_with_project_trend_shows_project_trend_line() {
    let report = ImpactReport {
        schema_version: ImpactReportSchemaVersion::V1,
        enabled: true,
        enabled_source: EnabledSource::Project,
        record_count: 1,
        meta: None,
        first_recorded: Some("2026-05-01T00:00:00Z".into()),
        latest_git_sha: None,
        surfacing: None,
        trend: None,
        project_surfacing: Some(ImpactCounts::from_combined(10, 5, 3)),
        project_trend: Some(TrendSummary {
            direction: ImpactTrendDirection::Stable,
            total_delta: 0,
            previous_total: 10,
            current_total: 10,
        }),
        gate_runs: None,
        containment_count: 0,
        recent_containment: vec![],
        resolved_total: 0,
        suppressed_total: 0,
        recent_resolved: vec![],
        attribution_active: false,
        onboarding_declined: false,
        explicit_decision: true,
    };
    let md = render_markdown(&report);
    assert!(
        md.contains("Project trend (whole project"),
        "project trend line must appear in markdown: {md}"
    );
    assert!(
        md.contains("10 -> 10 (flat)"),
        "stable trend must read 'flat': {md}"
    );
}

#[test]
fn render_markdown_enabled_no_history_shows_check_back() {
    let report = ImpactReport {
        schema_version: ImpactReportSchemaVersion::V1,
        enabled: true,
        enabled_source: EnabledSource::Project,
        record_count: 0,
        meta: None,
        first_recorded: None,
        latest_git_sha: None,
        surfacing: None,
        trend: None,
        project_surfacing: None,
        project_trend: None,
        gate_runs: None,
        containment_count: 0,
        recent_containment: vec![],
        resolved_total: 0,
        suppressed_total: 0,
        recent_resolved: vec![],
        attribution_active: false,
        onboarding_declined: false,
        explicit_decision: true,
    };
    let md = render_markdown(&report);
    assert!(
        md.contains("No history yet"),
        "enabled but empty markdown must show 'No history yet': {md}"
    );
}

#[test]
fn render_markdown_resolved_shows_count_when_positive() {
    let report = ImpactReport {
        schema_version: ImpactReportSchemaVersion::V1,
        enabled: true,
        enabled_source: EnabledSource::Project,
        record_count: 3,
        meta: None,
        first_recorded: Some("2026-05-01T00:00:00Z".into()),
        latest_git_sha: None,
        surfacing: Some(ImpactCounts::default()),
        trend: None,
        project_surfacing: None,
        project_trend: None,
        gate_runs: None,
        containment_count: 0,
        recent_containment: vec![],
        resolved_total: 4,
        suppressed_total: 1,
        recent_resolved: vec![],
        attribution_active: true,
        onboarding_declined: false,
        explicit_decision: true,
    };
    let md = render_markdown(&report);
    assert!(
        md.contains("**Resolved:** 4 finding"),
        "markdown must show resolved count: {md}"
    );
    assert!(
        md.contains("**Marked intentional:** 1 finding"),
        "markdown must show suppressed count: {md}"
    );
}

// ----- render_markdown_footer: project-only (no audit records) ----------

#[test]
fn render_markdown_footer_project_only_no_audit_records() {
    // When record_count is 0 but project_surfacing exists, the footer
    // must show the "Tracking since" form, not "recorded audit runs".
    let report = ImpactReport {
        schema_version: ImpactReportSchemaVersion::V1,
        enabled: true,
        enabled_source: EnabledSource::Project,
        record_count: 0,
        meta: None,
        first_recorded: Some("2026-05-10T00:00:00Z".into()),
        latest_git_sha: None,
        surfacing: None,
        trend: None,
        project_surfacing: Some(ImpactCounts::from_combined(3, 2, 1)),
        project_trend: None,
        gate_runs: None,
        containment_count: 0,
        recent_containment: vec![],
        resolved_total: 0,
        suppressed_total: 0,
        recent_resolved: vec![],
        attribution_active: false,
        onboarding_declined: false,
        explicit_decision: true,
    };
    let md = render_markdown(&report);
    assert!(
        md.contains("Tracking since 2026-05-10"),
        "project-only footer must say 'Tracking since': {md}"
    );
    assert!(
        !md.contains("recorded audit run"),
        "project-only footer must not mention 'recorded audit run': {md}"
    );
}

// ----- cross_repo_markdown: project_wide totals line --------------------

#[test]
fn render_cross_repo_markdown_includes_project_wide_totals_when_present() {
    let _cfg = aggregate_env();
    let mut s = store_with("proj", 2, 0, "t0", 1);
    s.project_records.push(ImpactRecord {
        timestamp: "pt".to_owned(),
        version: "2.0.0".to_owned(),
        git_sha: None,
        verdict: "warn".to_owned(),
        gate_source: None,
        counts: ImpactCounts::from_combined(99, 0, 0),
    });
    seed_store("proj", &s);
    let report = aggregate(CrossRepoSort::Recent);
    let md = render_cross_repo_markdown(&report);
    assert!(
        md.contains("99 issue") && md.contains("project-wide"),
        "cross-repo markdown must show project-wide totals: {md}"
    );
}

// ----- migrate_legacy_store: corrupt legacy file falls back to default ---

#[test]
#[cfg_attr(miri, ignore)]
fn migrate_legacy_store_corrupt_file_returns_default() {
    let (_config, dir) = test_env();
    let root = dir.path();
    // Write a corrupt legacy in-repo store.
    std::fs::create_dir_all(root.join(".fallow")).unwrap();
    std::fs::write(legacy_store_path(root), b"{ corrupted json ][").unwrap();
    // load() tries the user store (missing) then migrate_legacy_store.
    let store = load(root);
    // A corrupt legacy file must return a default store without panicking.
    assert!(!store.enabled);
    assert!(store.records.is_empty());
}

#[test]
#[cfg_attr(miri, ignore)]
fn statusline_loader_reads_legacy_store_without_migrating_it() {
    let (_config, dir) = test_env();
    let root = dir.path();
    std::fs::create_dir_all(root.join(".fallow")).unwrap();
    std::fs::write(
        legacy_store_path(root),
        br#"{
                "enabled": true,
                "records": [{
                    "timestamp": "t0",
                    "version": "2.0.0",
                    "verdict": "warn",
                    "counts": {
                        "total_issues": 2,
                        "dead_code": 2,
                        "complexity": 0,
                        "duplication": 0
                    }
                }]
            }"#,
    )
    .unwrap();

    let user_store = store_path(root).unwrap();
    assert!(!user_store.exists());
    let store = load_statusline(root).expect("legacy statusline store reads");
    assert!(store.enabled);
    assert_eq!(store.records.len(), 1);
    assert!(
        !user_store.exists(),
        "statusline load must not persist a migrated user store"
    );
}

#[test]
#[cfg_attr(miri, ignore)]
fn statusline_loader_reports_corrupt_and_future_user_stores() {
    let (_config, dir) = test_env();
    let root = dir.path();

    seed_store_raw(root, b"{ not valid json ][");
    assert!(matches!(
        load_statusline(root),
        Err(StatuslineLoadError::DataUnavailable)
    ));

    seed_store_raw(
        root,
        format!(
            r#"{{"schema_version":{},"enabled":true}}"#,
            STORE_SCHEMA_VERSION + 1
        )
        .as_bytes(),
    );
    assert!(matches!(
        load_statusline(root),
        Err(StatuslineLoadError::NewerSchema)
    ));
}

#[test]
#[cfg_attr(miri, ignore)]
fn statusline_loader_reports_corrupt_legacy_store() {
    let (_config, dir) = test_env();
    let root = dir.path();
    std::fs::create_dir_all(root.join(".fallow")).unwrap();
    std::fs::write(legacy_store_path(root), b"{ corrupted json ][").unwrap();

    assert!(matches!(
        load_statusline(root),
        Err(StatuslineLoadError::DataUnavailable)
    ));
    assert!(!store_path(root).unwrap().exists());
}

// ----- resolve_enabled: user source propagates to report ----------------

#[test]
fn resolve_enabled_user_source_appears_in_report() {
    let (_config, _dir) = test_env();
    set_global_default(true);
    let never_asked = ImpactStore::default();
    let (enabled, source) = resolve_enabled(&never_asked);
    assert!(enabled);
    assert_eq!(source, EnabledSource::User);
    let report = build_report(&never_asked);
    assert_eq!(report.enabled_source, EnabledSource::User);
}

// ----- render_cross_repo_human: long label truncated --------------------

#[test]
fn render_cross_repo_human_long_label_is_truncated_in_table() {
    let _cfg = aggregate_env();
    let very_long_name = "this_is_a_very_long_project_name_that_exceeds_the_column_width";
    let s = store_with(very_long_name, 1, 0, "2026-06-10T00:00:00Z", 1);
    seed_store("longkey", &s);
    let report = aggregate(CrossRepoSort::Recent);
    let human = render_cross_repo_human(&report, None);
    // Must contain the truncation marker and never the full long name inline.
    assert!(
        human.contains("..."),
        "long label must be truncated with '...': {human}"
    );
}

// ----- aggregate_sort_name when label is absent: falls back to short key -

#[test]
fn aggregate_sort_name_falls_back_to_short_key_when_no_label() {
    let _cfg = aggregate_env();
    // Store with no label; cross_repo_label falls back to the key prefix.
    let mut s = ImpactStore {
        enabled: true,
        explicit_decision: true,
        resolved_total: 1,
        label: None,
        ..Default::default()
    };
    s.records.push(ImpactRecord {
        timestamp: "t0".to_owned(),
        version: "2.0.0".to_owned(),
        git_sha: None,
        verdict: "warn".to_owned(),
        gate_source: None,
        counts: ImpactCounts::from_combined(1, 0, 0),
    });
    seed_store("abcdefghijklmnop", &s);
    let report = aggregate(CrossRepoSort::Name);
    // The row is present even without a label.
    assert_eq!(report.tracked_count, 1);
    // The short_key used is the first 12 chars of the key.
    assert_eq!(report.projects[0].project_key, "abcdefghijklmnop");
}

// ----- row_trend fallback to changed-file trend when no project trend ---

#[test]
fn row_trend_falls_back_to_changed_file_trend_when_no_project_trend() {
    let report = ImpactReport {
        schema_version: ImpactReportSchemaVersion::V1,
        enabled: true,
        enabled_source: EnabledSource::Project,
        record_count: 2,
        meta: None,
        first_recorded: None,
        latest_git_sha: None,
        surfacing: Some(ImpactCounts::default()),
        trend: Some(TrendSummary {
            direction: ImpactTrendDirection::Improving,
            total_delta: -3,
            previous_total: 8,
            current_total: 5,
        }),
        project_surfacing: None,
        project_trend: None,
        gate_runs: None,
        containment_count: 0,
        recent_containment: vec![],
        resolved_total: 0,
        suppressed_total: 0,
        recent_resolved: vec![],
        attribution_active: false,
        onboarding_declined: false,
        explicit_decision: true,
    };
    assert_eq!(row_trend(&report), "down");
}

#[test]
fn row_trend_returns_dash_when_no_trend_at_all() {
    let report = ImpactReport {
        schema_version: ImpactReportSchemaVersion::V1,
        enabled: true,
        enabled_source: EnabledSource::Project,
        record_count: 1,
        meta: None,
        first_recorded: None,
        latest_git_sha: None,
        surfacing: Some(ImpactCounts::default()),
        trend: None,
        project_surfacing: None,
        project_trend: None,
        gate_runs: None,
        containment_count: 0,
        recent_containment: vec![],
        resolved_total: 0,
        suppressed_total: 0,
        recent_resolved: vec![],
        attribution_active: false,
        onboarding_declined: false,
        explicit_decision: true,
    };
    assert_eq!(row_trend(&report), "-");
}

// ----- opt_count returns "-" for None and the total for Some ------------

#[test]
fn opt_count_returns_dash_for_none_and_total_for_some() {
    assert_eq!(opt_count(None), "-");
    // from_combined(dead=4, complexity=2, dup=1) => total=7
    assert_eq!(opt_count(Some(&ImpactCounts::from_combined(4, 2, 1))), "7");
}

// ----- project_key is stable (no separator) for non-git dirs -----------

#[test]
#[cfg_attr(miri, ignore)]
fn impact_project_key_is_a_hex_string_with_no_separator() {
    let (_config, dir) = test_env();
    let root = dir.path();
    let key = project_identity(root).0;
    assert!(
        !key.contains('/') && !key.contains('\\'),
        "project key must not contain a path separator: {key}"
    );
    assert!(!key.is_empty(), "project key must not be empty");
}

// ----- ImpactCounts::from_combined wires through correctly --------------

#[test]
fn impact_counts_from_combined_sums_to_total() {
    let c = ImpactCounts::from_combined(3, 2, 1);
    assert_eq!(c.total_issues, 6);
    assert_eq!(c.dead_code, 3);
    assert_eq!(c.complexity, 2);
    assert_eq!(c.duplication, 1);
}

// ----- cross_repo_markdown: no history case (project_count > 0, tracked_count 0) --

#[test]
fn render_cross_repo_markdown_all_empty_projects_tracked_count_zero() {
    // All stores are enabled-but-empty => tracked_count 0, project_count > 0.
    let _cfg = aggregate_env();
    seed_store(
        "empty1",
        &ImpactStore {
            enabled: true,
            explicit_decision: true,
            ..Default::default()
        },
    );
    let report = aggregate(CrossRepoSort::Recent);
    assert_eq!(report.project_count, 1);
    assert_eq!(report.tracked_count, 0);
    let md = render_cross_repo_markdown(&report);
    // Must show project_count but NOT an empty table (projects vec is empty).
    assert!(
        md.contains("1 project tracked, 0 with history"),
        "must show counts: {md}"
    );
    assert!(
        !md.contains("| Project |"),
        "no table when tracked_count is 0: {md}"
    );
}

// ----- render_cross_repo_markdown: project_count == 0, unreadable > 0 --

#[test]
fn render_cross_repo_markdown_zero_projects_with_unreadable() {
    let _cfg = aggregate_env();
    let dir = impact_config_dir().unwrap().join("impact");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("bad.json"), b"}{bad").unwrap();
    let report = aggregate(CrossRepoSort::Recent);
    assert_eq!(report.project_count, 0);
    assert_eq!(report.unreadable_count, 1);
    let md = render_cross_repo_markdown(&report);
    assert!(
        md.contains("skipped") && md.contains("unreadable store"),
        "must report corrupt stores: {md}"
    );
}

/// Outside a repository, and without git, each single probe fails in the
/// same way as the combined probe. The identity then uses the root and
/// starts no more git processes.
#[test]
fn identity_paths_skip_the_single_probes_outside_a_repository() {
    let root = tempfile::tempdir().unwrap();
    let canonical = dunce::canonicalize(root.path()).unwrap();
    for error in [
        ChangedFilesError::NotARepository,
        ChangedFilesError::GitMissing("not found".to_owned()),
    ] {
        let probes = std::cell::Cell::new(0_u32);
        let paths = identity_paths_from(
            root.path(),
            Err(error),
            || {
                probes.set(probes.get() + 1);
                None
            },
            || {
                probes.set(probes.get() + 1);
                None
            },
        );
        assert_eq!(probes.get(), 0, "no single probe runs");
        assert_eq!(paths, (canonical.clone(), canonical.clone()));
    }
}

/// A bare repository, or an old git that does not know a flag, fails the
/// combined probe with a git error. Each single probe then decides.
#[test]
fn identity_paths_use_the_single_probes_after_a_git_error() {
    let root = tempfile::tempdir().unwrap();
    let common = root.path().join("common.git");
    let paths = identity_paths_from(
        root.path(),
        Err(ChangedFilesError::GitFailed("fatal".to_owned())),
        || Some(common.clone()),
        || None,
    );
    let canonical = dunce::canonicalize(root.path()).unwrap();
    assert_eq!(paths, (common, canonical));
}

#[test]
fn is_inside_any_matches_a_temp_base_and_its_children() {
    let temp = tempfile::tempdir().unwrap();
    let child = temp.path().join("repo");
    std::fs::create_dir_all(&child).unwrap();
    let bases = vec![temp.path().to_path_buf()];

    assert!(record_gate::is_inside_any(temp.path(), &bases));
    assert!(record_gate::is_inside_any(&child, &bases));
    assert!(record_gate::is_inside_any(
        &child.join("not-created"),
        &bases
    ));
}

#[test]
fn is_inside_any_rejects_a_sibling_with_a_shared_prefix() {
    let parent = tempfile::tempdir().unwrap();
    let base = parent.path().join("tmp");
    let sibling = parent.path().join("tmp-project");
    std::fs::create_dir_all(&base).unwrap();
    std::fs::create_dir_all(&sibling).unwrap();

    assert!(!record_gate::is_inside_any(&sibling, &[base]));
    assert!(!record_gate::is_inside_any(parent.path(), &[]));
}

#[cfg(unix)]
#[test]
fn is_inside_any_follows_a_symlinked_base() {
    let parent = tempfile::tempdir().unwrap();
    let real = parent.path().join("real");
    let link = parent.path().join("link");
    std::fs::create_dir_all(real.join("repo")).unwrap();
    std::os::unix::fs::symlink(&real, &link).unwrap();

    assert!(record_gate::is_inside_any(
        &real.join("repo"),
        std::slice::from_ref(&link)
    ));
    assert!(record_gate::is_inside_any(&link.join("repo"), &[real]));
}

#[test]
fn usable_temp_bases_drops_a_root_and_the_home_folder() {
    let home = tempfile::tempdir().unwrap();
    let home_parent = home.path().parent().unwrap().to_path_buf();
    let other = tempfile::tempdir().unwrap();
    let candidates = vec![
        PathBuf::from(std::path::MAIN_SEPARATOR_STR),
        home.path().to_path_buf(),
        home_parent.clone(),
        other.path().to_path_buf(),
    ];

    let bases = record_gate::usable_temp_bases(candidates, Some(home.path()));

    assert_eq!(bases, vec![home_parent, other.path().to_path_buf()]);
}
