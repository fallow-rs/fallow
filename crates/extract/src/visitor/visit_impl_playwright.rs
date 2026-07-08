use oxc_ast::ast::{
    Argument, CallExpression, Expression, TSInterfaceDeclaration, TSType, TSTypeAliasDeclaration,
};

use crate::DynamicImportInfo;

use super::super::{ModuleInfoExtractor, PendingPlaywrightFactory};
use super::visit_helpers::{
    collect_fixture_type_bindings_from_members, collect_fixture_type_bindings_from_type,
    playwright_extend_base_name, vi_mock_has_factory, vitest_auto_mock_source, vitest_mock_source,
};

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
            .filter_map(|argument| match argument {
                Argument::Identifier(ident) => Some(ident.name.to_string()),
                _ => None,
            })
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
        } else if let Expression::Identifier(ident) = &call.callee {
            self.pending_playwright_factory_aliases
                .push((test_name.to_string(), ident.name.to_string()));
        }
    }

    pub(super) fn record_vitest_mock_dynamic_imports(&mut self, expr: &CallExpression<'_>) {
        let Some(target_source) = vitest_mock_source(expr) else {
            return;
        };

        self.dynamic_imports.push(DynamicImportInfo {
            source: target_source.clone(),
            span: expr.span,
            destructured_names: Vec::new(),
            local_name: None,
            is_speculative: false,
        });

        if !vi_mock_has_factory(expr)
            && let Some(mock_source) = vitest_auto_mock_source(&target_source)
        {
            self.dynamic_imports.push(DynamicImportInfo {
                source: mock_source,
                span: expr.span,
                destructured_names: Vec::new(),
                local_name: Some(String::new()),
                is_speculative: true,
            });
        }
    }
}
