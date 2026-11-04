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
}
