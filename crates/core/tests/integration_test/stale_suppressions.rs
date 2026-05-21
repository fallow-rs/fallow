use fallow_types::results::SuppressionOrigin;

use super::common::{create_config, fixture_path};

#[test]
fn stale_next_line_suppression_on_used_export() {
    let root = fixture_path("stale-suppressions");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let stale_comments: Vec<_> = results
        .stale_suppressions
        .iter()
        .filter(|s| matches!(&s.origin, SuppressionOrigin::Comment { .. }))
        .collect();

    // usedHelper has `// fallow-ignore-next-line unused-export` but IS used
    assert!(
        stale_comments
            .iter()
            .any(|s| s.path.ends_with("utils.ts")
                && matches!(&s.origin, SuppressionOrigin::Comment { issue_kind: Some(k), .. } if k == "unused-export")
                && s.line == 2),
        "Expected stale suppression for usedHelper at utils.ts:2, found: {stale_comments:?}"
    );
}

#[test]
fn active_suppression_not_reported_stale() {
    let root = fixture_path("stale-suppressions");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    // unusedHelper has `// fallow-ignore-next-line unused-export` and IS unused
    // Its suppression should NOT be stale
    let stale_for_unused_helper = results.stale_suppressions.iter().any(|s| {
        s.path.ends_with("utils.ts") && s.line == 6 // comment_line of the suppression for unusedHelper
    });

    assert!(
        !stale_for_unused_helper,
        "Suppression for unusedHelper should NOT be stale (export is genuinely unused)"
    );
}

#[test]
fn stale_blanket_suppression() {
    let root = fixture_path("stale-suppressions");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    // anotherUsedExport has a blanket `// fallow-ignore-next-line` but no issues on next line
    let stale_blanket = results.stale_suppressions.iter().any(|s| {
        s.path.ends_with("utils.ts")
            && matches!(
                &s.origin,
                SuppressionOrigin::Comment {
                    issue_kind: None,
                    ..
                }
            )
    });

    assert!(
        stale_blanket,
        "Blanket suppression on anotherUsedExport should be stale (no issues on next line)"
    );
}

#[test]
fn stale_file_level_suppression() {
    let root = fixture_path("stale-suppressions");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    // file-level.ts has `// fallow-ignore-file unused-file` but the file IS reachable
    let stale_file_level = results.stale_suppressions.iter().any(|s| {
        s.path.ends_with("file-level.ts")
            && matches!(
                &s.origin,
                SuppressionOrigin::Comment {
                    is_file_level: true,
                    issue_kind: Some(k),
                    ..
                } if k == "unused-file"
            )
    });

    assert!(
        stale_file_level,
        "File-level unused-file suppression should be stale (file is reachable)"
    );
}

#[test]
fn expected_unused_tag_stale_when_used() {
    let root = fixture_path("stale-suppressions");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    // usedExport has @expected-unused but IS used by index.ts
    let stale_tag = results.stale_suppressions.iter().any(|s| {
        s.path.ends_with("expected-unused.ts")
            && matches!(
                &s.origin,
                SuppressionOrigin::JsdocTag { export_name } if export_name == "usedExport"
            )
    });

    assert!(
        stale_tag,
        "usedExport with @expected-unused should be stale (it IS used)"
    );
}

#[test]
fn expected_unused_tag_not_stale_when_unused() {
    let root = fixture_path("stale-suppressions");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    // genuinelyUnused has @expected-unused and IS unused (tag is working)
    let stale_for_genuinely_unused = results.stale_suppressions.iter().any(|s| {
        s.path.ends_with("expected-unused.ts")
            && matches!(
                &s.origin,
                SuppressionOrigin::JsdocTag { export_name } if export_name == "genuinelyUnused"
            )
    });

    assert!(
        !stale_for_genuinely_unused,
        "genuinelyUnused with @expected-unused should NOT be stale (export is genuinely unused)"
    );
}

#[test]
fn expected_unused_not_in_unused_exports() {
    let root = fixture_path("stale-suppressions");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    // Neither @expected-unused export should appear in unused_exports
    let expected_unused_in_results: Vec<_> = results
        .unused_exports
        .iter()
        .filter(|e| e.export.path.ends_with("expected-unused.ts"))
        .collect();

    assert!(
        expected_unused_in_results.is_empty(),
        "@expected-unused exports should never appear in unused_exports: {expected_unused_in_results:?}"
    );
}

#[test]
fn total_stale_suppressions_count() {
    let root = fixture_path("stale-suppressions");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    assert_eq!(
        results.stale_suppressions.len(),
        4,
        "Expected 4 stale suppressions: 2 comment (next-line on usedHelper, blanket on anotherUsedExport), 1 file-level (unused-file on file-level.ts), 1 jsdoc tag (@expected-unused on usedExport). Found: {:?}",
        results
            .stale_suppressions
            .iter()
            .map(|s| format!("{}:{}", s.path.display(), s.line))
            .collect::<Vec<_>>()
    );
}

// ── Issue #449: partial-accept for unknown kinds ────────────────

#[test]
fn issue_449_known_kind_suppresses_alongside_unknown_token() {
    let root = fixture_path("issue-449-unknown-kind");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let secret_flagged = results
        .unused_exports
        .iter()
        .any(|e| e.export.path.ends_with("utils.ts") && e.export.export_name == "secret");
    assert!(
        !secret_flagged,
        "`unused-export` token in `// fallow-ignore-next-line unused-export, complexity-typo` must still suppress `secret`. \
         unused_exports: {:?}",
        results
            .unused_exports
            .iter()
            .map(|e| format!("{}:{}", e.export.path.display(), e.export.export_name))
            .collect::<Vec<_>>()
    );
}

#[test]
fn issue_449_unknown_token_surfaces_as_stale_with_kind_known_false() {
    let root = fixture_path("issue-449-unknown-kind");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unknown_findings: Vec<_> = results
        .stale_suppressions
        .iter()
        .filter(|s| {
            matches!(
                &s.origin,
                SuppressionOrigin::Comment {
                    kind_known: false,
                    ..
                }
            )
        })
        .collect();

    // Expect two unknown-kind diagnostics: `complexity-typo` and `typo-only`.
    let tokens: Vec<String> = unknown_findings
        .iter()
        .filter_map(|s| match &s.origin {
            SuppressionOrigin::Comment {
                issue_kind: Some(k),
                ..
            } => Some(k.clone()),
            _ => None,
        })
        .collect();
    assert!(
        tokens.iter().any(|t| t == "complexity-typo"),
        "expected `complexity-typo` to surface as an unknown suppression token. tokens: {tokens:?}"
    );
    assert!(
        tokens.iter().any(|t| t == "typo-only"),
        "expected `typo-only` to surface as an unknown suppression token. tokens: {tokens:?}"
    );
}

#[test]
fn issue_449_unknown_token_explanation_carries_next_step_copy() {
    let root = fixture_path("issue-449-unknown-kind");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let complexity_typo = results
        .stale_suppressions
        .iter()
        .find(|s| {
            matches!(
                &s.origin,
                SuppressionOrigin::Comment {
                    issue_kind: Some(k),
                    kind_known: false,
                    ..
                } if k == "complexity-typo"
            )
        })
        .expect("expected complexity-typo unknown suppression to be present");

    let explanation = complexity_typo.explanation();
    assert!(
        explanation.contains("not a recognized fallow issue kind"),
        "unknown-kind explanation must say so explicitly. Got: {explanation}"
    );
    assert!(
        explanation.contains("Other tokens on this line still apply"),
        "explanation must reassure the user that sibling tokens still work. Got: {explanation}"
    );
}

#[test]
fn issue_449_close_typo_explanation_includes_levenshtein_hint() {
    let root = fixture_path("issue-449-unknown-kind");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unsed_export = results
        .stale_suppressions
        .iter()
        .find(|s| {
            matches!(
                &s.origin,
                SuppressionOrigin::Comment {
                    issue_kind: Some(k),
                    kind_known: false,
                    ..
                } if k == "unsed-export"
            )
        })
        .expect("expected unsed-export unknown suppression to be present");

    let explanation = unsed_export.explanation();
    assert!(
        explanation.contains("Did you mean 'unused-export'?"),
        "explanation should surface the Levenshtein hint for a close typo. Got: {explanation}"
    );
}
