//! Spans of the references to a test mock API, such as `vi` or `jest`.

use oxc_ast::AstKind;
use oxc_span::Span;

use super::MockApiReferenceSpans;
use fallow_types::extract::{ImportInfo, ImportedName};

pub(super) fn compute_mock_api_reference_spans(
    semantic: &oxc_semantic::Semantic<'_>,
    imports: &[ImportInfo],
    root_scope: oxc_semantic::ScopeId,
) -> MockApiReferenceSpans {
    let scoping = semantic.scoping();
    let mut spans = MockApiReferenceSpans::default();

    let collect_binding_spans = |local_name: &str, out: &mut rustc_hash::FxHashSet<Span>| {
        let Some(symbol_id) = scoping.get_binding(root_scope, oxc_str::Ident::from(local_name))
        else {
            return;
        };
        out.extend(
            scoping
                .get_resolved_references(symbol_id)
                .filter_map(|reference| {
                    let AstKind::IdentifierReference(identifier) =
                        semantic.nodes().kind(reference.node_id())
                    else {
                        return None;
                    };
                    Some(identifier.span)
                }),
        );
    };

    for import in imports {
        if import.is_type_only || import.local_name.is_empty() {
            continue;
        }
        let vitest = matches!(import.source.as_str(), "vitest" | "vite-plus/test");
        let is_vi_binding =
            vitest && matches!(&import.imported_name, ImportedName::Named(name) if name == "vi");
        let is_jest_binding = import.source == "@jest/globals"
            && matches!(&import.imported_name, ImportedName::Named(name) if name == "jest");
        let is_vitest_namespace =
            vitest && matches!(&import.imported_name, ImportedName::Namespace);

        if is_vi_binding || is_jest_binding {
            collect_binding_spans(&import.local_name, &mut spans.mock_bindings);
        } else if is_vitest_namespace {
            collect_binding_spans(&import.local_name, &mut spans.vitest_namespaces);
        }
    }

    // The Jest test environment injects `jest` as a global, so unresolved
    // value references named `jest` count as mock-API provenance. Masking only
    // ever applies to files the plugin layer classified as test entry points,
    // which grounds this in the existing Jest test-root detection. Unresolved
    // `vi` stays unproven on purpose (unchanged from #2068): Vitest exposes
    // `vi` as a global only under `globals: true`, and without reading that
    // config the safe direction is to abstain.
    for (name, reference_ids) in scoping.root_unresolved_references() {
        if name.as_str() != "jest" {
            continue;
        }
        spans
            .mock_bindings
            .extend(reference_ids.iter().filter_map(|reference_id| {
                let reference = scoping.get_reference(*reference_id);
                if !reference.is_value() {
                    return None;
                }
                let AstKind::IdentifierReference(identifier) =
                    semantic.nodes().kind(reference.node_id())
                else {
                    return None;
                };
                Some(identifier.span)
            }));
    }

    spans
}
