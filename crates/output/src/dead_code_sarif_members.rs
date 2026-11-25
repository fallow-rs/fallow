//! SARIF results for unused member findings.

use super::*;

pub(super) fn push_member_sarif_results(
    sarif_results: &mut Vec<serde_json::Value>,
    ctx: &SarifCtx<'_>,
    snippets: &mut SourceSnippetCache,
) {
    let SarifCtx {
        results,
        root,
        rules,
    } = *ctx;

    push_sarif_results(sarif_results, &results.unused_enum_members, snippets, |m| {
        with_caveats(
            sarif_member_fields(
                &m.member,
                root,
                "fallow/unused-enum-member",
                finding_level(m, rules.unused_enum_members),
                "Enum",
            ),
            &m.reachability_caveats,
        )
    });
    push_sarif_results(
        sarif_results,
        &results.unused_class_members,
        snippets,
        |m| {
            with_caveats(
                sarif_member_fields(
                    &m.member,
                    root,
                    "fallow/unused-class-member",
                    finding_level(m, rules.unused_class_members),
                    "Class",
                ),
                &m.reachability_caveats,
            )
        },
    );
    push_sarif_results(
        sarif_results,
        &results.unused_store_members,
        snippets,
        |m| {
            with_caveats(
                sarif_member_fields(
                    &m.member,
                    root,
                    "fallow/unused-store-member",
                    finding_level(m, rules.unused_store_members),
                    "Store",
                ),
                &m.reachability_caveats,
            )
        },
    );
    push_sarif_results(
        sarif_results,
        &results.unused_namespace_members,
        snippets,
        |m| {
            with_caveats(
                sarif_member_fields(
                    &m.member,
                    root,
                    "fallow/unused-namespace-member",
                    finding_level(m, rules.unused_namespace_members),
                    "Namespace",
                ),
                &m.reachability_caveats,
            )
        },
    );
    push_sarif_results(
        sarif_results,
        &results.unused_protected_class_members,
        snippets,
        |m| {
            with_caveats(
                sarif_member_fields(
                    &m.member,
                    root,
                    "fallow/unused-protected-class-member",
                    finding_level(m, rules.unused_protected_class_members),
                    "Protected class",
                ),
                &m.reachability_caveats,
            )
        },
    );
    push_sarif_results(sarif_results, &results.namespace_escapes, snippets, |e| {
        let escape = &e.escape;
        SarifFields {
            rule_id: "fallow/namespace-escape",
            level: finding_level(e, rules.namespace_escapes),
            message: escape.message(&relative_uri(&escape.module, root)),
            uri: relative_uri(&escape.path, root),
            region: Some((escape.line, escape.col + 1)),
            source_path: Some(escape.path.clone()),
            properties: None,
        }
    });
    push_test_only_export_sarif_results(sarif_results, ctx, snippets);
}

fn push_test_only_export_sarif_results(
    sarif_results: &mut Vec<serde_json::Value>,
    ctx: &SarifCtx<'_>,
    snippets: &mut SourceSnippetCache,
) {
    let SarifCtx {
        results,
        root,
        rules,
    } = *ctx;
    push_sarif_results(sarif_results, &results.test_only_exports, snippets, |t| {
        let export = &t.export;
        SarifFields {
            rule_id: "fallow/test-only-export",
            level: finding_level(t, rules.test_only_exports),
            message: export.message(),
            uri: relative_uri(&export.path, root),
            region: Some((export.line, export.col + 1)),
            source_path: Some(export.path.clone()),
            properties: None,
        }
    });
}
