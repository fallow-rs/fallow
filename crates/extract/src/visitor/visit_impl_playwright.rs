use oxc_ast::ast::{
    Argument, CallExpression, Expression, TSInterfaceDeclaration, TSType, TSTypeAliasDeclaration,
};

use crate::{
    DynamicImportInfo, SemanticFact, VitestModuleMockAction, VitestModuleMockOperationFact,
};

use super::super::{
    MockObjectProvenance, ModuleInfoExtractor, PendingPlaywrightFactory,
    PendingVitestMockOperation, PendingVitestMockProof,
};
use super::visit_helpers::{
    collect_fixture_type_bindings_from_members, collect_fixture_type_bindings_from_type,
    mock_method_object_span, mock_object_is_literal_global, mock_replacement_candidate,
    mock_static_target_source, playwright_extend_base_name, root_manual_mock_source,
    vi_mock_has_factory, vitest_auto_mock_source,
};
use crate::parse::MockApiReferenceSpans;

impl ModuleInfoExtractor {
    fn collect_playwright_fixture_type_bindings(&self, ty: &TSType<'_>) -> Vec<(String, String)> {
        let mut bindings = Vec::new();
        collect_fixture_type_bindings_from_type(
            ty,
            "",
            &self.playwright_fixture_types,
            &mut bindings,
        );
        bindings.sort_unstable();
        bindings.dedup();
        bindings
    }

    pub(super) fn record_playwright_fixture_type_alias(
        &mut self,
        alias: &TSTypeAliasDeclaration<'_>,
    ) {
        let bindings = self.collect_playwright_fixture_type_bindings(&alias.type_annotation);
        self.record_playwright_fixture_type_bindings(alias.id.name.as_str(), bindings);
    }

    /// Record an INTERFACE-declared fixture map (`interface MyFixtures {
    /// loginPage: LoginPage }` consumed by `base.extend<MyFixtures>`) into the
    /// same `playwright_fixture_types` table as the type-alias form, so both
    /// declaration styles resolve identically. `extends` heritage members are
    /// not expanded (the body's own members still resolve). See issue #1785.
    pub(super) fn record_playwright_fixture_interface(
        &mut self,
        iface: &TSInterfaceDeclaration<'_>,
    ) {
        let mut bindings = Vec::new();
        collect_fixture_type_bindings_from_members(
            &iface.body.body,
            "",
            &self.playwright_fixture_types,
            &mut bindings,
        );
        self.record_playwright_fixture_type_bindings(iface.id.name.as_str(), bindings);
    }

    /// Shared sink for alias- and interface-declared fixture maps: normalizes
    /// (sort + dedup), records the binding table, and emits the fixture-type
    /// facts.
    fn record_playwright_fixture_type_bindings(
        &mut self,
        type_name: &str,
        mut bindings: Vec<(String, String)>,
    ) {
        bindings.sort_unstable();
        bindings.dedup();
        if bindings.is_empty() {
            return;
        }
        self.playwright_fixture_types
            .insert(type_name.to_string(), bindings.clone());
        for (fixture_name, fixture_type) in bindings {
            self.record_playwright_fixture_type_fact(
                type_name.to_string(),
                fixture_name.clone(),
                fixture_type,
            );
        }
    }

    pub(super) fn record_playwright_fixture_definitions(
        &mut self,
        test_name: &str,
        call: &CallExpression<'_>,
    ) {
        let Some(base_name) = playwright_extend_base_name(call) else {
            return;
        };
        if !self.is_named_import_from(base_name.as_str(), "@playwright/test", "test") {
            return;
        }
        let Some(type_arguments) = call.type_arguments.as_deref() else {
            return;
        };
        let mut bindings = Vec::new();
        for type_arg in &type_arguments.params {
            bindings.extend(self.collect_playwright_fixture_type_bindings(type_arg));
        }
        bindings.sort_unstable();
        bindings.dedup();
        // Remember this local `const X = base.extend<T>({...})` fixture definition
        // so a helper wrapping `<X>.extend(...)` can inherit the bindings under its
        // own name (issue #1791). Stored whether or not the const is exported.
        if !bindings.is_empty() {
            self.playwright_local_fixture_defs
                .insert(test_name.to_string(), bindings.clone());
        }
        for (fixture_name, type_name) in bindings {
            self.record_playwright_fixture_definition_fact(
                test_name.to_string(),
                fixture_name.clone(),
                type_name,
            );
        }
    }

    fn record_playwright_fixture_alias(&mut self, test_name: &str, base_name: &str) {
        self.record_playwright_fixture_alias_fact(test_name.to_string(), base_name.to_string());
    }

    pub(super) fn record_playwright_wrapper_aliases(
        &mut self,
        test_name: &str,
        call: &CallExpression<'_>,
    ) {
        if let Some(base_name) = playwright_extend_base_name(call) {
            if !self.is_named_import_from(base_name.as_str(), "@playwright/test", "test") {
                self.record_playwright_fixture_alias(test_name, &base_name);
            }
            return;
        }

        let Expression::Identifier(callee) = &call.callee else {
            return;
        };
        if !self.is_named_import_from(callee.name.as_str(), "@playwright/test", "mergeTests") {
            return;
        }

        let mut base_names: Vec<String> = call
            .arguments
            .iter()
            .filter_map(playwright_merge_argument_base_name)
            .collect();
        base_names.sort();
        base_names.dedup();
        for base_name in base_names {
            self.record_playwright_fixture_alias(test_name, &base_name);
        }
    }

    /// Capture helper-function Playwright fixtures or aliases from returns.
    pub(super) fn try_capture_playwright_factory_helper(
        &mut self,
        test_name: &str,
        call: &CallExpression<'_>,
    ) {
        if let Some(base_name) = playwright_extend_base_name(call) {
            // A helper returning `<base>.extend(...)`. `<base>` is either the
            // `@playwright/test` `test` import (its own fixtures come from the
            // type argument via the PendingPlaywrightFactory below) OR a local
            // fixture const / another helper. Record an alias unconditionally so
            // the helper inherits the base's fixture bindings even when the
            // wrapping `.extend({})` carries no type argument of its own (issue
            // #1791). The alias no-ops when `base` is the raw `test` import, since
            // no captured factory is keyed on it. The alias is name-based, so a
            // non-Playwright `<x>.extend(...)` helper whose `<x>` collides with a
            // same-file genuine fixture const could over-credit that fixture's
            // class; this only ever over-credits (false-negative direction),
            // matching the documented `mergeTests` name-collision tolerance.
            self.pending_playwright_factory_aliases
                .push((test_name.to_string(), base_name.clone()));

            // An IMPORTED base const (`billingBaseFixture.extend({})` where
            // `billingBaseFixture` comes from a sibling file) cannot resolve
            // through the same-module `resolve_playwright_factory_call_definitions`
            // pass, so ALSO emit an analyze-time alias fact for any base that is
            // not the raw `@playwright/test` `test` import (mirror the gate in
            // `record_playwright_wrapper_aliases`); the #1210 cross-file
            // expansion then resolves the imported base to its fixture
            // definitions. The raw `test` base no-ops. Issue #1795.
            if !self.is_named_import_from(base_name.as_str(), "@playwright/test", "test") {
                self.record_playwright_fixture_alias(test_name, &base_name);
            }

            let Some(type_arguments) = call.type_arguments.as_deref() else {
                return;
            };
            let mut bindings = Vec::new();
            for type_arg in &type_arguments.params {
                bindings.extend(self.collect_playwright_fixture_type_bindings(type_arg));
            }
            bindings.sort_unstable();
            bindings.dedup();
            if bindings.is_empty() {
                return;
            }
            self.pending_playwright_factory_calls
                .push(PendingPlaywrightFactory {
                    test_name: test_name.to_string(),
                    base_name,
                    type_bindings: bindings,
                });
        } else if let Expression::Identifier(callee) = &call.callee
            && self.is_named_import_from(callee.name.as_str(), "@playwright/test", "mergeTests")
        {
            // A helper returning `mergeTests(billingTest(), ordersUiTest())`
            // (issue #1795): emit an analyze-time alias fact per argument base so
            // each wrapped fixture's definitions are inherited cross-file via the
            // #1210 expansion, and push the same pairs for same-file helper
            // inheritance. The import gate (handles `mergeTests as merge`) keeps a
            // user-local `mergeTests` function inert.
            let mut base_names: Vec<String> = call
                .arguments
                .iter()
                .filter_map(playwright_merge_argument_base_name)
                .collect();
            base_names.sort();
            base_names.dedup();
            for base_name in base_names {
                self.record_playwright_fixture_alias(test_name, &base_name);
                self.pending_playwright_factory_aliases
                    .push((test_name.to_string(), base_name));
            }
        } else if let Expression::Identifier(ident) = &call.callee {
            self.pending_playwright_factory_aliases
                .push((test_name.to_string(), ident.name.to_string()));
        }
    }

    pub(super) fn record_vitest_mock_imports(&mut self, expr: &CallExpression<'_>) {
        let Some((object_span, provenance)) = mock_method_object_span(expr, "mock") else {
            return;
        };
        let Some(target_source) = mock_static_target_source(expr) else {
            return;
        };

        let has_factory = vi_mock_has_factory(expr);
        // Literal `vi`/`jest` objects may be injected globals with no import to
        // prove provenance against, so their credit edges are pushed eagerly.
        // Aliased and namespace forms always come from an import; their edges
        // wait for the span-provenance check in
        // `resolve_vitest_mock_operations` so an unrelated `x.mock("./y")`
        // never credits a file.
        let literal_global = mock_object_is_literal_global(expr);
        if literal_global {
            self.push_mock_credit_edges(&target_source, expr.span, has_factory);
        }

        let proof = mock_replacement_candidate(expr).map_or(
            PendingVitestMockProof::UnprovenMock,
            |candidate| PendingVitestMockProof::ClosedFactory {
                binding_requirement_spans: candidate.binding_requirement_spans,
                namespace_requirement_spans: candidate.namespace_requirement_spans,
            },
        );
        self.pending_vitest_mock_operations
            .push(PendingVitestMockOperation {
                source: target_source,
                object_span,
                provenance,
                call_span: expr.span,
                has_factory,
                needs_deferred_edges: !literal_global,
                proof,
            });
    }

    /// Record `X.doMock(...)` / `ns.vi.doMock(...)` for coverage credit only.
    ///
    /// Decision for issue #2082: `doMock` never masks. Unlike `vi.mock` it is
    /// not hoisted, affects only module requests evaluated after the call, and
    /// usually sits inside a test callback whose execution (and order relative
    /// to the file's dynamic imports) is a runtime scheduling question, so a
    /// mask derived from it could produce false uncovered findings. The safe,
    /// useful part is the credit side: the target and its speculative
    /// `__mocks__` sibling become dynamic-import edges, so a manual mock
    /// registered only through `doMock` stops looking like an unused file.
    /// `doUnmock` is ignored entirely: it only affects later dynamic imports,
    /// so letting it clear a sound hoisted `vi.mock` mask would lose
    /// precision for nothing.
    pub(super) fn record_vitest_do_mock(&mut self, expr: &CallExpression<'_>) {
        let Some((object_span, provenance)) = mock_method_object_span(expr, "doMock") else {
            return;
        };
        let Some(target_source) = mock_static_target_source(expr) else {
            return;
        };

        let has_factory = vi_mock_has_factory(expr);
        if mock_object_is_literal_global(expr) {
            self.push_mock_credit_edges(&target_source, expr.span, has_factory);
            return;
        }
        self.pending_vitest_mock_operations
            .push(PendingVitestMockOperation {
                source: target_source,
                object_span,
                provenance,
                call_span: expr.span,
                has_factory,
                needs_deferred_edges: true,
                proof: PendingVitestMockProof::CreditOnly,
            });
    }

    fn push_mock_credit_edges(
        &mut self,
        target_source: &str,
        span: oxc_span::Span,
        has_factory: bool,
    ) {
        self.dynamic_imports.push(DynamicImportInfo {
            source: target_source.to_string(),
            span,
            destructured_names: Vec::new(),
            local_name: None,
            is_speculative: false,
        });

        if has_factory {
            return;
        }
        // Two candidate conventions: the `__mocks__` sibling next to the
        // mocked module (relative and alias-shaped specifiers, issue #251) and
        // the root-level `__mocks__/<specifier>` manual mock for bare package
        // specifiers (issue #2225). A slash-bearing bare source gets both: for
        // an aliased user module the sibling resolves internally and the root
        // candidate misses, while for a real package the sibling is dropped in
        // package space and the root candidate probes the runner convention.
        for mock_source in [
            vitest_auto_mock_source(target_source),
            root_manual_mock_source(target_source),
        ]
        .into_iter()
        .flatten()
        {
            self.dynamic_imports.push(DynamicImportInfo {
                source: mock_source,
                span,
                destructured_names: Vec::new(),
                local_name: Some(String::new()),
                is_speculative: true,
            });
        }
    }

    pub(super) fn record_vitest_unmock(&mut self, expr: &CallExpression<'_>) {
        if let Some((object_span, provenance)) = mock_method_object_span(expr, "unmock")
            && let Some(source) = mock_static_target_source(expr)
        {
            self.pending_vitest_mock_operations
                .push(PendingVitestMockOperation {
                    source,
                    object_span,
                    provenance,
                    call_span: expr.span,
                    has_factory: false,
                    needs_deferred_edges: false,
                    proof: PendingVitestMockProof::Unmock,
                });
        }
    }

    pub(crate) fn resolve_vitest_mock_operations(&mut self, spans: &MockApiReferenceSpans) {
        let mut operations: Vec<_> = self
            .pending_vitest_mock_operations
            .drain(..)
            .filter(|operation| match operation.provenance {
                MockObjectProvenance::Binding => {
                    spans.mock_bindings.contains(&operation.object_span)
                }
                MockObjectProvenance::VitestNamespace => {
                    spans.vitest_namespaces.contains(&operation.object_span)
                }
            })
            .collect();
        operations.sort_unstable_by_key(|operation| operation.call_span.start);

        // Provenance is proven now, so push the deferred credit edges for
        // aliased and namespace mock registrations (the literal `vi`/`jest`
        // forms already pushed theirs at record time).
        let deferred_edges: Vec<(String, oxc_span::Span, bool)> = operations
            .iter()
            .filter(|operation| {
                operation.needs_deferred_edges
                    && !matches!(operation.proof, PendingVitestMockProof::Unmock)
            })
            .map(|operation| {
                (
                    operation.source.clone(),
                    operation.call_span,
                    operation.has_factory,
                )
            })
            .collect();
        for (source, span, has_factory) in deferred_edges {
            self.push_mock_credit_edges(&source, span, has_factory);
        }

        self.semantic_facts
            .extend(operations.into_iter().filter_map(|operation| {
                let action = match operation.proof {
                    PendingVitestMockProof::ClosedFactory {
                        binding_requirement_spans,
                        namespace_requirement_spans,
                    } => VitestModuleMockAction::Mock {
                        factory_replaces_original: binding_requirement_spans
                            .iter()
                            .all(|span| spans.mock_bindings.contains(span))
                            && namespace_requirement_spans
                                .iter()
                                .all(|span| spans.vitest_namespaces.contains(span)),
                    },
                    PendingVitestMockProof::UnprovenMock => VitestModuleMockAction::Mock {
                        factory_replaces_original: false,
                    },
                    PendingVitestMockProof::Unmock => VitestModuleMockAction::Unmock,
                    // `doMock` contributes credit edges only; it must never
                    // enter the ordered mask fact stream (issue #2082).
                    PendingVitestMockProof::CreditOnly => return None,
                };
                Some(SemanticFact::VitestModuleMockOperation(
                    VitestModuleMockOperationFact {
                        source: operation.source,
                        call_start: operation.call_span.start,
                        action,
                    },
                ))
            }));
    }
}

/// The base test name a `mergeTests(...)` argument contributes: a bare fixture
/// identifier (`mergeTests(testA, testB)`, issue #1210) or a factory call with a
/// bare identifier callee (`mergeTests(billingTest(), ordersUiTest())`, issue
/// #1795). Other argument shapes (spreads, member-expression callees) abstain.
fn playwright_merge_argument_base_name(argument: &Argument<'_>) -> Option<String> {
    match argument {
        Argument::Identifier(ident) => Some(ident.name.to_string()),
        Argument::CallExpression(call) => match &call.callee {
            Expression::Identifier(callee) => Some(callee.name.to_string()),
            _ => None,
        },
        _ => None,
    }
}
