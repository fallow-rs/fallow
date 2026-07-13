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

use std::collections::BTreeSet;

use fallow_cli::report::github::{PackageManager, PathRebase, RenderOptions};
use fallow_cli::report::github_annotations::{EnvelopeKind, render_annotations};
use fallow_cli::report::github_summary::{LinkContext, render_summary};
use fallow_types::issue_meta::{ISSUE_RESULT_META, IssueResultMeta};
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
        (EnvelopeKind::Fix, fix_envelope()),
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

/// `report --from <fix-results.json> --format github-summary` routes the
/// kind-less fix envelope through `EnvelopeKind::Fix`, which must reach the
/// same renderer as the live `fallow fix` command (`render_fix_summary`) and
/// carry `summary-fix.jq`'s sections.
#[test]
fn render_summary_fix_mirrors_render_fix_summary() {
    let env = fix_envelope();
    let via_dispatch = render_summary(EnvelopeKind::Fix, &env, &LinkContext::default());
    let direct = fallow_cli::report::github_summary::render_fix_summary(&env);
    assert_eq!(
        via_dispatch, direct,
        "EnvelopeKind::Fix must reach render_fix_summary"
    );
    // The summary-fix.jq sections: heading, headline, count table, details.
    assert!(
        via_dispatch.starts_with("## Fallow - Auto-fix"),
        "{via_dispatch}"
    );
    assert!(
        via_dispatch.contains("**Dry run**: would apply **3 fixes**"),
        "{via_dispatch}"
    );
    assert!(
        via_dispatch.contains("skipped 1 file(s) that changed since analysis"),
        "{via_dispatch}"
    );
    assert!(
        via_dispatch.contains(
            "kept exports in 2 file(s) where consumers may be hidden from static analysis"
        ),
        "{via_dispatch}"
    );
    assert!(
        via_dispatch.contains("| Export removals | 2 |"),
        "{via_dispatch}"
    );
    assert!(
        via_dispatch.contains("| Dependency removals | 1 |"),
        "{via_dispatch}"
    );
    assert!(
        via_dispatch.contains("<summary>View details</summary>"),
        "{via_dispatch}"
    );
    assert!(
        via_dispatch.contains("- `src/api/client.ts:42` - `unusedFn`"),
        "{via_dispatch}"
    );
    assert!(
        via_dispatch.contains("- `left-pad` from dependencies in `package.json`"),
        "{via_dispatch}"
    );
}

/// The bundled action no-ops annotations for the fix command, so the native
/// `EnvelopeKind::Fix` annotation renderer must emit nothing.
#[test]
fn render_annotations_fix_is_empty() {
    let rendered = render_annotations(EnvelopeKind::Fix, &fix_envelope(), &plain_options());
    assert_eq!(rendered, "", "fix annotations must be empty: {rendered}");
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
        (EnvelopeKind::Fix, fix_envelope()),
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

// ---------------------------------------------------------------------------
// Per-IssueKind drift guard (advisor plan 027 Phase A)
//
// The bundled action's shell drift guard (`action/tests/issuekind-drift-guard.sh`,
// `assert_issuekind_summary_coverage`) asserts that every counted dead-code
// IssueKind's serialized `result_key` reaches the GitHub summary + annotation
// surfaces. Those surfaces render natively since v3.4.2, but no Rust test held
// the same line: a new `counts_in_total` IssueKind could land in
// `ISSUE_RESULT_META` and silently miss `render_summary` / `render_annotations`.
// These tests iterate the registry (the `counts_in_total == true` rows the shell
// guard gates) and assert each kind renders in BOTH github-summary and
// github-annotations.
// ---------------------------------------------------------------------------

/// The counted dead-code result rows: exactly the set the shell guard gates
/// (`counts_in_total == true`). The three `counts_in_total == false` rows
/// (prop-drilling, thin-wrapper, duplicate-prop-shape) are CLI/JSON-only
/// advisory signals the PR surfaces deliberately do not carry, and the shell
/// guard skips them the same way.
fn counted_dead_code_metas() -> impl Iterator<Item = &'static IssueResultMeta> {
    ISSUE_RESULT_META.iter().filter(|meta| meta.counts_in_total)
}

/// Number of counted dead-code IssueKinds the GitHub surfaces must carry. This
/// mirrors the shell drift guard's gated set size (verified equal by running
/// `action/tests/issuekind-drift-guard.sh`); bump it in lockstep when a counted
/// IssueKind lands so the Rust guard and the shell guard keep agreeing.
const COUNTED_DEAD_CODE_KINDS: usize = 42;

/// Sentinel path embedded per kind so an annotation for that kind is uniquely
/// identifiable in the rendered stream. `snt/` + the unique `result_key` +
/// `.ts` carries no workflow-command reserved character, so it renders verbatim
/// in the annotation `file=` property.
fn kind_sentinel(result_key: &str) -> String {
    format!("snt/{result_key}.ts")
}

/// One finding for `result_key`, shaped so the annotation renderer emits the
/// kind's sentinel path in `file=`. Most kinds read a top-level `path`; the
/// handful that anchor on a nested location carry the sentinel there instead.
fn dead_code_finding(result_key: &str) -> Value {
    let path = kind_sentinel(result_key);
    match result_key {
        "unlisted_dependencies" => json!({
            "package_name": "pkg",
            "imported_from": [{ "path": path, "line": 1, "col": 0 }],
        }),
        "duplicate_exports" => json!({
            "export_name": "dup",
            "locations": [{ "path": path, "line": 1, "col": 0 }],
        }),
        "circular_dependencies" => json!({ "files": [path], "line": 0, "col": 0, "length": 1 }),
        "re_export_cycles" => json!({ "files": [path], "kind": "cycle" }),
        "boundary_violations" => json!({
            "from_path": path, "to_path": "src/to.ts",
            "from_zone": "ui", "to_zone": "db", "line": 1, "col": 0,
        }),
        _ => json!({ "path": path, "line": 1, "col": 0 }),
    }
}

/// A dead-code envelope carrying exactly one finding of every counted kind.
fn every_dead_code_kind_envelope() -> Value {
    let mut env = serde_json::Map::new();
    env.insert("kind".to_owned(), json!("dead-code"));
    let mut total = 0u64;
    for meta in counted_dead_code_metas() {
        env.insert(
            meta.result_key.to_owned(),
            json!([dead_code_finding(meta.result_key)]),
        );
        total += 1;
    }
    env.insert("total_issues".to_owned(), json!(total));
    env.insert("elapsed_ms".to_owned(), json!(1));
    Value::Object(env)
}

/// Trip-wire that keeps the two coverage tests honest: the fixture must carry
/// one finding array per counted `result_key`, no more, no fewer. A new counted
/// IssueKind that is not added here fails this before the coverage tests even
/// run, and the count pins agreement with the shell guard's gated set.
#[test]
fn fixture_tracks_the_counted_registry_exactly() {
    let env = every_dead_code_kind_envelope();
    let obj = env.as_object().expect("object envelope");
    let registry: BTreeSet<&str> = counted_dead_code_metas()
        .map(|meta| meta.result_key)
        .collect();
    let fixture: BTreeSet<&str> = obj
        .iter()
        .filter(|(_, value)| value.is_array())
        .map(|(key, _)| key.as_str())
        .collect();
    assert_eq!(
        fixture, registry,
        "fixture drifted from the counted ISSUE_RESULT_META set"
    );
    assert_eq!(
        registry.len(),
        COUNTED_DEAD_CODE_KINDS,
        "counted dead-code kind count changed; update COUNTED_DEAD_CODE_KINDS and re-run action/tests/issuekind-drift-guard.sh to keep the Rust and shell guards in sync"
    );
}

/// Every counted dead-code IssueKind must surface a category row in
/// github-summary. Mirrors the shell guard over `summary-check.jq`.
#[test]
fn summary_covers_every_counted_dead_code_kind() {
    let env = every_dead_code_kind_envelope();
    let rendered = render_summary(EnvelopeKind::DeadCode, &env, &LinkContext::default());
    for meta in counted_dead_code_metas() {
        let marker = format!("[{}](", meta.summary_label);
        assert!(
            rendered.contains(&marker),
            "github-summary omits the dead-code category row for `{}` (label {:?}): a counted IssueKind is not wired into render_summary",
            meta.result_key,
            meta.summary_label,
        );
    }
}

/// Every counted dead-code IssueKind must surface an annotation in
/// github-annotations. Mirrors the shell guard over `annotations-check.jq`.
#[test]
fn annotations_cover_every_counted_dead_code_kind() {
    let env = every_dead_code_kind_envelope();
    let rendered = render_annotations(EnvelopeKind::DeadCode, &env, &plain_options());
    for meta in counted_dead_code_metas() {
        let marker = kind_sentinel(meta.result_key);
        assert!(
            rendered.contains(&marker),
            "github-annotations omits an annotation for `{}`: a counted IssueKind is not wired into render_annotations",
            meta.result_key,
        );
    }
}
