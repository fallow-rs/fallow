#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests use unwrap and expect to keep fixture setup concise"
)]

//! Golden tests for the GitHub-native text formats (`github-annotations`,
//! `github-summary`), pinning the per-envelope-kind renderers against
//! hand-built `--format json` envelopes. The regression traps the design
//! spike and panel called out each get a named test: budget notice,
//! severity-sort over path order, path-prefix rebasing, escaping of
//! comma/colon/percent/CRLF, `kind_known == false` stale suppressions,
//! notice-level security candidates, and report-from round-trip parity.

use fallow_cli::report::github::{PackageManager, PathRebase, RenderOptions};
use fallow_cli::report::github_annotations::{EnvelopeKind, render_annotations};
use fallow_cli::report::github_summary::{LinkContext, render_summary};
use serde_json::{Value, json};

fn plain_options() -> RenderOptions {
    RenderOptions {
        rebase: PathRebase::None,
        pm: PackageManager::Npm,
    }
}

fn check_envelope() -> Value {
    json!({
        "kind": "dead-code",
        "schema_version": 7,
        "total_issues": 12,
        "elapsed_ms": 321,
        "unused_files": [
            { "path": "src/a,b.ts" },
            { "path": "src/München.ts" }
        ],
        "unused_exports": [
            { "path": "src/api/client.ts", "line": 42, "col": 9, "export_name": "unusedFn", "is_re_export": false, "is_type_only": false },
            { "path": "src/api/types.ts", "line": 7, "col": 0, "export_name": "LegacyShape", "is_re_export": true, "is_type_only": true }
        ],
        "unused_dependencies": [
            { "path": "package.json", "line": 12, "package_name": "left-pad", "used_in_workspaces": [] },
            { "path": "packages/app/package.json", "line": 0, "package_name": "lodash", "used_in_workspaces": ["web", "docs"] }
        ],
        "unlisted_dependencies": [
            {
                "package_name": "chalk",
                "imported_from": [
                    { "path": "src/cli.ts", "line": 3, "col": 21 },
                    { "path": "src/log.ts", "line": 1, "col": 19 }
                ]
            }
        ],
        "duplicate_exports": [
            {
                "export_name": "format",
                "locations": [
                    { "path": "src/format.ts", "line": 1, "col": 0 },
                    { "path": "src/utils/format.ts", "line": 8, "col": 0 }
                ]
            }
        ],
        "circular_dependencies": [
            { "files": ["src/a.ts", "src/b.ts"], "line": 0, "col": 0, "length": 2 }
        ],
        "re_export_cycles": [
            { "files": ["src/index.ts"], "kind": "self-loop" }
        ],
        "boundary_violations": [
            { "from_path": "src/ui/button.ts", "to_path": "src/db/pool.ts", "from_zone": "ui", "to_zone": "db", "line": 4, "col": 0 }
        ],
        "policy_violations": [
            { "path": "src/legacy/eval.ts", "line": 9, "col": 2, "matched": "eval", "pack": "security", "rule_id": "no-eval", "severity": "error", "message": "eval is 100% banned\r\nsee: policy" },
            { "path": "src/telemetry.ts", "line": 2, "col": 0, "matched": "track", "pack": "hygiene", "rule_id": "no-track", "severity": "warn" }
        ],
        "stale_suppressions": [
            { "path": "src/kept.ts", "line": 11, "col": 0, "origin": { "type": "jsdoc_tag", "export_name": "keptExport" } },
            { "path": "src/typo.ts", "line": 5, "col": 0, "origin": { "type": "comment", "kind_known": false, "issue_kind": "not-a-kind", "is_file_level": false } },
            { "path": "src/old.ts", "line": 1, "col": 0, "origin": { "type": "comment", "kind_known": true, "issue_kind": "unused-export", "is_file_level": true } }
        ],
        "unresolved_catalog_references": [
            { "path": "packages/web/package.json", "line": 14, "entry_name": "react", "catalog_name": "default", "available_in_catalogs": [] }
        ]
    })
}

fn dupes_envelope() -> Value {
    json!({
        "kind": "dupes",
        "elapsed_ms": 87,
        "stats": {
            "total_files": 40,
            "files_with_clones": 3,
            "clone_groups": 2,
            "clone_instances": 5,
            "duplicated_lines": 62,
            "total_lines": 4100,
            "duplication_percentage": 1.512
        },
        "clone_groups": [
            {
                "line_count": 18,
                "token_count": 140,
                "instances": [
                    { "file": "packages/app/src/services/api/client.ts", "start_line": 10, "end_line": 27, "start_col": 0 },
                    { "file": "packages/app/src/services/api/admin.ts", "start_line": 44, "end_line": 61, "start_col": 2 },
                    { "file": "shared.ts", "start_line": 5, "end_line": 22, "start_col": 0 }
                ]
            },
            {
                "line_count": 6,
                "token_count": 55,
                "instances": [
                    { "file": "src/one.ts", "start_line": 1, "end_line": 6, "start_col": 0 },
                    { "file": "src/two.ts", "start_line": 9, "end_line": 14, "start_col": 4 }
                ]
            }
        ]
    })
}

fn health_envelope() -> Value {
    json!({
        "kind": "health",
        "elapsed_ms": 954,
        "health_score": { "grade": "C", "score": 71.25 },
        "health_trend": {
            "compared_to": { "grade": "B", "score": 80.0 },
            "metrics": [
                { "name": "score", "delta": -8.75, "current": 71.25, "label": "Score" },
                { "name": "dead_export_pct", "delta": 2.5, "current": 6.0, "label": "Dead exports" },
                { "name": "avg_cyclomatic", "delta": -0.4, "current": 3.6, "label": "Avg cyclomatic" }
            ]
        },
        "summary": {
            "files_analyzed": 210,
            "functions_analyzed": 1400,
            "functions_above_threshold": 3,
            "max_cyclomatic_threshold": 20,
            "max_cognitive_threshold": 15,
            "max_crap_threshold": 30
        },
        "findings": [
            { "path": "src/z-last.ts", "line": 3, "col": 0, "name": "criticalMess", "severity": "critical", "exceeded": "all", "cyclomatic": 33, "cognitive": 41, "crap": 156.5, "line_count": 120 },
            { "path": "src/a-first.ts", "line": 9, "col": 4, "name": "moderateBranch", "severity": "moderate", "exceeded": "cyclomatic", "cyclomatic": 22, "cognitive": 9, "crap": null, "line_count": 40 },
            { "path": "src/mid.ts", "line": 5, "col": 2, "name": "highCog", "severity": "high", "exceeded": "cognitive", "cyclomatic": 8, "cognitive": 19, "crap": null, "line_count": 33 }
        ],
        "runtime_coverage": {
            "findings": [
                {
                    "path": "src/cold.ts", "line": 12, "function": "neverCalled", "verdict": "safe_to_delete",
                    "invocations": 0, "confidence": "high",
                    "evidence": { "static_status": "unused", "test_coverage": "none", "v8_tracking": "tracked" },
                    "actions": [ { "description": "Delete this function; production never called it in 90 days." } ]
                },
                {
                    "path": "src/unknown.ts", "line": 30, "function": "maybeUsed", "verdict": "coverage_unavailable",
                    "invocations": null, "confidence": "low",
                    "evidence": { "static_status": "used", "test_coverage": "partial", "v8_tracking": "untracked", "untracked_reason": "bundled" },
                    "actions": []
                }
            ],
            "hot_paths": [
                { "path": "src/hot.ts", "line": 2, "function": "handleRequest", "invocations": 91234, "percentile": 99 }
            ],
            "summary": { "functions_tracked": 900, "functions_hit": 700, "functions_unhit": 150, "functions_untracked": 50 }
        },
        "targets": [
            {
                "path": "src/refactor-me.ts", "effort": "medium", "priority": "P1", "confidence": "high",
                "recommendation": "Split the request handler into parsing and dispatch.",
                "factors": [
                    { "metric": "churn", "detail": "12 commits in 30 days" },
                    { "metric": "complexity", "value": 27 }
                ]
            }
        ]
    })
}

fn audit_envelope() -> Value {
    json!({
        "kind": "audit",
        "verdict": "warn",
        "elapsed_ms": 640,
        "changed_files_count": 4,
        "summary": { "dead_code_issues": 3, "complexity_findings": 1, "duplication_clone_groups": 1 },
        "attribution": {
            "gate": "new-only",
            "dead_code_introduced": 2, "dead_code_inherited": 1,
            "complexity_introduced": 1, "complexity_inherited": 0,
            "duplication_introduced": 0, "duplication_inherited": 1
        },
        "dead_code": {
            "unused_exports": [
                { "path": "src/api/client.ts", "line": 42, "export_name": "unusedFn", "introduced": true },
                { "path": "/abs/deep/nested/src/api/types.ts", "line": 7, "export_name": "Old", "introduced": false }
            ],
            "unused_server_actions": [
                { "path": "app/actions.ts", "line": 3, "action_name": "submitForm", "introduced": true }
            ]
        },
        "complexity": {
            "findings": [
                { "path": "src/gnarly.ts", "line": 88, "name": "bigSwitch", "severity": "high", "cyclomatic": 24, "cognitive": 18, "crap": 42.5, "coverage_tier": "partial", "introduced": true }
            ],
            "summary": { "coverage_model": "istanbul", "istanbul_matched": 12, "istanbul_total": 40 }
        },
        "duplication": {
            "clone_groups": [
                {
                    "line_count": 9, "token_count": 80, "introduced": false,
                    "instances": [
                        { "file": "src/one.ts", "start_line": 4, "end_line": 12 },
                        { "file": "src/two.ts", "start_line": 20, "end_line": 28 }
                    ]
                }
            ]
        }
    })
}

fn security_envelope() -> Value {
    json!({
        "kind": "security",
        "elapsed_ms": 233,
        "summary": { "security_findings": 2 },
        "security_findings": [
            {
                "kind": "tainted-sink", "severity": "high",
                "path": "src/routes/exec.ts", "line": 17, "col": 8,
                "evidence": "Non-literal command passed to child_process.exec().",
                "candidate": { "sink": { "callee": "child_process.exec" } }
            },
            {
                "kind": "client-server-leak", "severity": "medium",
                "path": "src/pages/settings,page.tsx", "line": 4, "col": 0,
                "evidence": "\"use client\" cone reaches process.env.SECRET_KEY: 100% certain.",
                "candidate": { "sink": {} }
            }
        ],
        "gate": { "mode": "new", "verdict": "fail", "new_count": 2 }
    })
}

fn fix_envelope() -> Value {
    json!({
        "dry_run": true,
        "total_fixed": 0,
        "skipped": 0,
        "skipped_content_changed": 1,
        "skipped_mixed_line_endings": 0,
        "skipped_low_confidence_exports": 2,
        "fixes": [
            { "type": "remove_export", "path": "src/api/client.ts", "line": 42, "name": "unusedFn", "applied": false },
            { "type": "remove_export", "path": "src/api/types.ts", "line": 7, "name": "LegacyShape", "applied": false },
            { "type": "remove_dependency", "package": "left-pad", "location": "dependencies", "file": "package.json", "applied": false },
            { "type": "skipped", "path": "src/raced.ts", "skipped": true, "skip_reason": "content_changed" }
        ]
    })
}

fn combined_envelope() -> Value {
    json!({
        "kind": "combined",
        "elapsed_ms": 1500,
        "check": {
            "total_issues": 3,
            "unused_files": [ { "path": "src/dead.ts" } ],
            "unused_exports": [
                { "path": "src/api/client.ts", "line": 42, "col": 9, "export_name": "unusedFn", "is_re_export": false, "is_type_only": false }
            ],
            "unused_dependencies": [
                { "path": "package.json", "line": 12, "package_name": "left-pad", "used_in_workspaces": [] }
            ]
        },
        "dupes": {
            "stats": {
                "total_files": 40, "files_with_clones": 2, "clone_groups": 1,
                "clone_instances": 2, "duplicated_lines": 18, "total_lines": 2000,
                "duplication_percentage": 0.9
            },
            "clone_groups": [
                {
                    "line_count": 9, "token_count": 80,
                    "instances": [
                        { "file": "packages/app/src/services/api/client.ts", "start_line": 10, "end_line": 18, "start_col": 0 },
                        { "file": "src/two.ts", "start_line": 20, "end_line": 28, "start_col": 0 }
                    ]
                }
            ]
        },
        "health": {
            "health_score": { "grade": "B", "score": 82.0 },
            "summary": {
                "files_analyzed": 210, "functions_analyzed": 1400, "functions_above_threshold": 2,
                "max_cyclomatic_threshold": 20, "max_cognitive_threshold": 15, "max_crap_threshold": 30
            },
            "findings": [
                { "path": "deep/nested/dir/src/a.ts", "line": 9, "col": 4, "name": "tieBreakFirst", "severity": "moderate", "exceeded": "cyclomatic", "cyclomatic": 22, "cognitive": 9, "crap": null, "line_count": 40 },
                { "path": "src/z.ts", "line": 3, "col": 0, "name": "worstCrap", "severity": "critical", "exceeded": "all", "cyclomatic": 33, "cognitive": 41, "crap": 156.5, "line_count": 120 }
            ],
            "runtime_coverage": {
                "verdict": "hot-path-touched",
                "findings": [
                    {
                        "path": "src/cold.ts", "line": 12, "function": "neverCalled", "verdict": "safe_to_delete",
                        "invocations": 0, "confidence": "high",
                        "evidence": { "static_status": "unused", "test_coverage": "none", "v8_tracking": "tracked" },
                        "actions": []
                    }
                ],
                "hot_paths": [
                    { "path": "src/hot.ts", "line": 2, "function": "handleRequest", "invocations": 91234, "percentile": 99 }
                ]
            },
            "vital_signs": { "maintainability_avg": 74.36, "avg_cyclomatic": 3.4 },
            "file_scores": [
                { "maintainability_index": 61.0 },
                { "maintainability_index": 70.0 }
            ]
        }
    })
}

// ---------------------------------------------------------------------------
// github-annotations
// ---------------------------------------------------------------------------

#[test]
fn github_annotations_check_snapshot() {
    let rendered = render_annotations(EnvelopeKind::DeadCode, &check_envelope(), &plain_options());
    insta::assert_snapshot!("github_annotations_check", rendered);
}

#[test]
fn github_annotations_dupes_snapshot() {
    let rendered = render_annotations(EnvelopeKind::Dupes, &dupes_envelope(), &plain_options());
    insta::assert_snapshot!("github_annotations_dupes", rendered);
}

#[test]
fn github_annotations_health_snapshot() {
    let rendered = render_annotations(EnvelopeKind::Health, &health_envelope(), &plain_options());
    insta::assert_snapshot!("github_annotations_health", rendered);
}

#[test]
fn github_annotations_audit_snapshot() {
    let rendered = render_annotations(EnvelopeKind::Audit, &audit_envelope(), &plain_options());
    insta::assert_snapshot!("github_annotations_audit", rendered);
}

#[test]
fn github_annotations_security_snapshot() {
    let rendered = render_annotations(
        EnvelopeKind::Security,
        &security_envelope(),
        &plain_options(),
    );
    insta::assert_snapshot!("github_annotations_security", rendered);
}

#[test]
fn github_annotations_combined_snapshot() {
    let rendered = render_annotations(
        EnvelopeKind::Combined,
        &combined_envelope(),
        &plain_options(),
    );
    insta::assert_snapshot!("github_annotations_combined", rendered);
}

#[test]
fn annotations_severity_sort_beats_path_order() {
    let rendered = render_annotations(EnvelopeKind::Health, &health_envelope(), &plain_options());
    let lines: Vec<&str> = rendered.lines().collect();
    // Errors first (critical/high complexity), even though their paths sort
    // after the moderate finding's `src/a-first.ts`.
    assert!(
        lines[0].starts_with("::error file=src/mid.ts"),
        "{}",
        lines[0]
    );
    assert!(
        lines[1].starts_with("::error file=src/z-last.ts"),
        "{}",
        lines[1]
    );
    assert!(
        lines[2].starts_with("::warning file=src/a-first.ts"),
        "{}",
        lines[2]
    );
    // Notices (refactoring targets, coverage_unavailable) after warnings.
    let first_notice = lines
        .iter()
        .position(|line| line.starts_with("::notice"))
        .expect("notices present");
    assert!(
        lines[..first_notice]
            .iter()
            .all(|line| !line.starts_with("::notice")),
        "no notice may precede a warning"
    );
}

#[test]
fn annotations_end_with_budget_notice() {
    let rendered = render_annotations(EnvelopeKind::DeadCode, &check_envelope(), &plain_options());
    let last = rendered.lines().last().expect("non-empty");
    let total = rendered.lines().count() - 1;
    assert_eq!(
        last,
        format!(
            "::notice::fallow emitted {total} annotations; GitHub shows at most 10 per type per step"
        )
    );
}

#[test]
fn annotations_single_finding_notice_uses_singular_noun() {
    let rendered = render_annotations(
        EnvelopeKind::DeadCode,
        &json!({
            "kind": "dead-code",
            "total_issues": 1,
            "unused_files": [{ "path": "src/orphan.ts" }]
        }),
        &plain_options(),
    );
    let last = rendered.lines().last().expect("non-empty");
    assert_eq!(
        last,
        "::notice::fallow emitted 1 annotation; GitHub shows at most 10 per type per step"
    );
}

#[test]
fn annotations_empty_run_renders_nothing() {
    let rendered = render_annotations(
        EnvelopeKind::DeadCode,
        &json!({ "kind": "dead-code", "total_issues": 0 }),
        &plain_options(),
    );
    assert_eq!(rendered, "");
}

#[test]
fn annotations_rebase_prefixes_every_file_property() {
    let options = RenderOptions {
        rebase: PathRebase::Prefix("packages/app".to_owned()),
        pm: PackageManager::Npm,
    };
    let rendered = render_annotations(EnvelopeKind::DeadCode, &check_envelope(), &options);
    for line in rendered.lines() {
        if let Some(rest) = line.split_once(" file=").map(|(_, rest)| rest) {
            assert!(
                rest.starts_with("packages/app/"),
                "unrebased file property: {line}"
            );
        }
    }
}

#[test]
fn annotations_escape_comma_colon_percent_crlf() {
    let rendered = render_annotations(EnvelopeKind::DeadCode, &check_envelope(), &plain_options());
    // Comma in a path escapes in the file= property.
    assert!(rendered.contains("file=src/a%2Cb.ts"), "{rendered}");
    // Non-ASCII passes through as UTF-8.
    assert!(rendered.contains("file=src/München.ts"), "{rendered}");
    // Percent and CRLF in a policy message body escape strictly.
    assert!(
        rendered.contains("eval is 100%25 banned%0D%0Asee: policy"),
        "{rendered}"
    );
}

#[test]
fn annotations_package_manager_drives_fix_commands() {
    let options = RenderOptions {
        rebase: PathRebase::None,
        pm: PackageManager::Bun,
    };
    let rendered = render_annotations(EnvelopeKind::DeadCode, &check_envelope(), &options);
    assert!(rendered.contains("Run: bun remove left-pad"), "{rendered}");
    assert!(rendered.contains("Run: bun add chalk"), "{rendered}");
    // Workspace-used dependency keeps the move advice instead of a command.
    assert!(
        rendered.contains("Move this dependency to the consuming workspace package.json."),
        "{rendered}"
    );
}

#[test]
fn annotations_security_candidates_render_at_notice_level() {
    let rendered = render_annotations(
        EnvelopeKind::Security,
        &security_envelope(),
        &plain_options(),
    );
    let lines: Vec<&str> = rendered.lines().collect();
    assert!(
        lines[..lines.len() - 1]
            .iter()
            .all(|line| line.starts_with("::notice ")),
        "security candidates must all be notices: {rendered}"
    );
}

#[test]
fn annotations_stale_suppression_unknown_kind_branch() {
    let rendered = render_annotations(EnvelopeKind::DeadCode, &check_envelope(), &plain_options());
    assert!(
        rendered.contains(
            "title=Unknown suppression kind::'not-a-kind' is not a recognized fallow issue kind."
        ),
        "{rendered}"
    );
    assert!(
        rendered.contains("title=Stale @expected-unused::"),
        "{rendered}"
    );
    assert!(
        rendered.contains(
            "title=Stale suppression::This 'fallow-ignore-file' comment for 'unused-export'"
        ),
        "{rendered}"
    );
}

/// The analyze-once contract: rendering an envelope that went through a JSON
/// serialize/parse round trip (what `--format json -o file` + `fallow report
/// --from file` does) is byte-identical to rendering the in-memory envelope.
#[test]
fn report_from_round_trip_parity() {
    for (kind, envelope) in [
        (EnvelopeKind::DeadCode, check_envelope()),
        (EnvelopeKind::Dupes, dupes_envelope()),
        (EnvelopeKind::Health, health_envelope()),
        (EnvelopeKind::Audit, audit_envelope()),
        (EnvelopeKind::Security, security_envelope()),
        (EnvelopeKind::Combined, combined_envelope()),
    ] {
        let direct = render_annotations(kind, &envelope, &plain_options());
        let serialized = serde_json::to_string_pretty(&envelope).expect("serialize");
        let reparsed: Value = serde_json::from_str(&serialized).expect("parse");
        let from_file = render_annotations(kind, &reparsed, &plain_options());
        assert_eq!(direct, from_file, "round-trip drift for {kind:?}");

        let links = LinkContext::default();
        let direct_summary = render_summary(kind, &envelope, &links);
        let from_file_summary = render_summary(kind, &reparsed, &links);
        assert_eq!(
            direct_summary, from_file_summary,
            "summary round-trip drift for {kind:?}"
        );
    }
}

// ---------------------------------------------------------------------------
// github-summary
// ---------------------------------------------------------------------------

#[test]
fn github_summary_check_snapshot() {
    let rendered = render_summary(
        EnvelopeKind::DeadCode,
        &check_envelope(),
        &LinkContext::default(),
    );
    insta::assert_snapshot!("github_summary_check", rendered);
}

#[test]
fn github_summary_check_clean_snapshot() {
    let rendered = render_summary(
        EnvelopeKind::DeadCode,
        &json!({ "kind": "dead-code", "total_issues": 0, "elapsed_ms": 45 }),
        &LinkContext::default(),
    );
    insta::assert_snapshot!("github_summary_check_clean", rendered);
}

#[test]
fn github_summary_single_issue_uses_singular_noun() {
    let rendered = render_summary(
        EnvelopeKind::DeadCode,
        &json!({
            "kind": "dead-code",
            "total_issues": 1,
            "elapsed_ms": 45,
            "unused_files": [{ "path": "src/orphan.ts" }]
        }),
        &LinkContext::default(),
    );
    assert!(
        rendered.contains("> **1 issue** found"),
        "headline should use the singular noun: {rendered}"
    );
}

#[test]
fn github_summary_dupes_snapshot() {
    let rendered = render_summary(
        EnvelopeKind::Dupes,
        &dupes_envelope(),
        &LinkContext::default(),
    );
    insta::assert_snapshot!("github_summary_dupes", rendered);
}

#[test]
fn github_summary_health_snapshot() {
    let rendered = render_summary(
        EnvelopeKind::Health,
        &health_envelope(),
        &LinkContext::default(),
    );
    insta::assert_snapshot!("github_summary_health", rendered);
}

#[test]
fn github_summary_audit_snapshot() {
    let rendered = render_summary(
        EnvelopeKind::Audit,
        &audit_envelope(),
        &LinkContext::default(),
    );
    insta::assert_snapshot!("github_summary_audit", rendered);
}

#[test]
fn github_summary_security_snapshot() {
    let rendered = render_summary(
        EnvelopeKind::Security,
        &security_envelope(),
        &LinkContext::default(),
    );
    insta::assert_snapshot!("github_summary_security", rendered);
}

#[test]
fn github_summary_fix_snapshot() {
    let rendered = fallow_cli::report::github_summary::render_fix_summary(&fix_envelope());
    insta::assert_snapshot!("github_summary_fix", rendered);
}

#[test]
fn github_summary_combined_snapshot() {
    // Populated link context exercises the blob-URL branch of the dupes file
    // links, including the repo-root prefix slot.
    let links = LinkContext {
        prefix: "packages/app/".to_owned(),
        repo: "acme/monorepo".to_owned(),
        sha: "0123abc".to_owned(),
    };
    let rendered = render_summary(EnvelopeKind::Combined, &combined_envelope(), &links);
    insta::assert_snapshot!("github_summary_combined", rendered);
}

#[test]
fn summary_has_no_em_dashes() {
    // Repo style rule: the renderer templates must never emit em dashes,
    // including where the ported jq layer used them.
    for (kind, envelope) in [
        (EnvelopeKind::DeadCode, check_envelope()),
        (EnvelopeKind::Dupes, dupes_envelope()),
        (EnvelopeKind::Health, health_envelope()),
        (EnvelopeKind::Audit, audit_envelope()),
        (EnvelopeKind::Security, security_envelope()),
        (EnvelopeKind::Combined, combined_envelope()),
    ] {
        let summary = render_summary(kind, &envelope, &LinkContext::default());
        assert!(!summary.contains('\u{2014}'), "em dash in {kind:?} summary");
        let annotations = render_annotations(kind, &envelope, &plain_options());
        assert!(
            !annotations.contains('\u{2014}'),
            "em dash in {kind:?} annotations"
        );
    }
}
