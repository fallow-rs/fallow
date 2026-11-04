//! SARIF results for component framework findings.

use super::*;

/// Push SARIF results for Svelte events, server actions, and load-data keys.
pub(super) fn push_component_framework_sarif_results(
    sarif_results: &mut Vec<serde_json::Value>,
    ctx: &SarifCtx<'_>,
    snippets: &mut SourceSnippetCache,
) {
    let SarifCtx {
        results,
        root,
        rules,
    } = *ctx;

    push_sarif_results(
        sarif_results,
        &results.unused_svelte_events,
        snippets,
        |e| {
            sarif_unused_svelte_event_fields(
                &e.event,
                root,
                finding_level(e, rules.unused_svelte_events),
            )
        },
    );
    push_sarif_results(
        sarif_results,
        &results.duplicate_export_aliases,
        snippets,
        |e| {
            sarif_duplicate_export_alias_fields(
                &e.alias,
                root,
                finding_level(e, rules.duplicate_export_aliases),
            )
        },
    );
    push_sarif_results(
        sarif_results,
        &results.unused_effect_layers,
        snippets,
        |e| {
            sarif_unused_effect_layer_fields(
                &e.layer,
                root,
                finding_level(e, rules.unused_effect_layers),
            )
        },
    );
    push_sarif_results(
        sarif_results,
        &results.unused_tagged_errors,
        snippets,
        |e| {
            sarif_unused_tagged_error_fields(
                &e.error,
                root,
                finding_level(e, rules.unused_tagged_errors),
            )
        },
    );
    push_sarif_results(
        sarif_results,
        &results.unproduced_catch_tags,
        snippets,
        |e| {
            sarif_unproduced_catch_tag_fields(
                &e.catch_tag,
                root,
                finding_level(e, rules.unproduced_catch_tags),
            )
        },
    );
    push_sarif_results(
        sarif_results,
        &results.unused_server_actions,
        snippets,
        |a| {
            sarif_unused_server_action_fields(
                &a.action,
                root,
                finding_level(a, rules.unused_server_actions),
            )
        },
    );
    push_sarif_results(
        sarif_results,
        &results.unused_load_data_keys,
        snippets,
        |k| {
            sarif_unused_load_data_key_fields(
                &k.key,
                root,
                finding_level(k, rules.unused_load_data_keys),
            )
        },
    );
}
