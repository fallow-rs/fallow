#[allow(
    clippy::wildcard_imports,
    reason = "many taint source helper AST types used"
)]
use oxc_ast::ast::*;
use oxc_span::GetSpan;

use fallow_types::extract::TaintedBinding;

use super::super::{ModuleInfoExtractor, extract_destructured_names};
use super::{
    DIRECT_TAINT_HOP, FRAMEWORK_REQUEST_SOURCE, GRAPHQL_ARGS_SOURCE, MAX_TAINT_BINDING_HOPS,
    MCP_TOOL_INPUT_SOURCE, NEXT_FORM_DATA_SOURCE, NEXT_REQUEST_SOURCE, QUEUE_JOB_SOURCE,
    TRPC_INPUT_SOURCE, apply_source_return_path, binding_source_path_candidates, callback_params,
    callee_method_name, collect_chained_taint_idents, destructure_source_path,
    extract_function_body_final_return_expr, extract_object_pattern_bindings, flatten_callee_path,
    function_body_has_use_server, function_like_params, is_framework_route_receiver_path,
    is_http_route_handler_name, is_route_registration_method, is_trpc_procedure_callee,
    is_trpc_procedure_method, last_callback_params, route_callback_params, source_returning_helper,
    unwrap_parens,
};

impl ModuleInfoExtractor {
    /// Record a tainted binding together with its chain-hop depth, deduped on
    /// `(local, source_path)`. On a duplicate the existing record keeps its
    /// span (first read wins, matching the analyze layer's name-keyed
    /// `or_insert`) and the SHALLOWER hop depth, so a direct source capture
    /// (hop 1) is never demoted past the chain cap by a chained re-record of
    /// the same pair, and a chained record is never inflated by a later one.
    fn push_tainted_binding(&mut self, binding: TaintedBinding, hop: u8) {
        debug_assert_eq!(
            self.tainted_bindings.len(),
            self.tainted_binding_hops.len(),
            "tainted_binding_hops must stay aligned with tainted_bindings"
        );
        if let Some(index) = self
            .tainted_bindings
            .iter()
            .position(|b| b.local == binding.local && b.source_path == binding.source_path)
        {
            self.tainted_binding_hops[index] = self.tainted_binding_hops[index].min(hop);
            return;
        }
        self.tainted_bindings.push(binding);
        self.tainted_binding_hops.push(hop);
    }

    /// Record tainted-source bindings for `const <name> = <object>.<prop>` and
    /// for one-hop local expressions that embed a source member access. The
    /// recorded candidates include the exact member path and the flattened
    /// object path, so `const id = req.query.id` still records `req.query`
    /// while leaf sources such as `const ref = document.referrer` can match
    /// exact source rows.
    /// Captured at any scope (no `is_module_scope` gate): a sink inside a route
    /// handler reading a function-local source is exactly the target case.
    pub(super) fn record_tainted_source_binding(&mut self, name: &str, expr: &Expression<'_>) {
        for source_path in binding_source_path_candidates(expr) {
            self.push_tainted_binding(
                TaintedBinding {
                    local: name.to_string(),
                    source_path,
                    // The initializer member-access read (`req.query.id`); anchors
                    // the taint trace's source node at the real read line.
                    source_span_start: expr.span().start,
                },
                DIRECT_TAINT_HOP,
            );
        }
    }

    fn record_tainted_param_binding(&mut self, name: &str, source_path: &'static str) {
        self.push_tainted_binding(
            TaintedBinding {
                local: name.to_string(),
                source_path: source_path.to_string(),
                // Synthetic framework-handler-param source (e.g. `framework.request`);
                // no concrete member-access read expression, so the analyze layer
                // anchors at the sink rather than a spurious line. `0` signals "no
                // captured read span".
                source_span_start: 0,
            },
            DIRECT_TAINT_HOP,
        );
    }

    /// Chain a declarator's taint from already-tainted local bindings that its
    /// initializer references through the conservative #1095 expression shapes
    /// plus a bare-identifier alias (issue #1146). Each chained binding carries
    /// the ORIGINAL `source_path` and `source_span_start`, so the analyze layer
    /// needs no changes: the chained local matches `source_tainted_locals` and
    /// anchors its trace at the original source read. Chains only form in
    /// lexical declarator order within one extractor walk; use-before-declaration
    /// flows and cross-SFC-block chains are silent false negatives by design
    /// (FN-preferring, matching the #885 doctrine).
    pub(super) fn record_chained_tainted_binding(&mut self, name: &str, init: &Expression<'_>) {
        let mut idents = Vec::new();
        collect_chained_taint_idents(init, &mut idents);
        if idents.is_empty() {
            return;
        }
        let chained = self.chained_bindings_for_idents(name, &idents);
        for (binding, hop) in chained {
            self.push_tainted_binding(binding, hop);
        }
    }

    /// Chained `(binding, hop)` records for a new local named `name` whose
    /// initializer references `idents`. Referenced bindings already at
    /// `MAX_TAINT_BINDING_HOPS` contribute nothing, so an over-cap chain is
    /// simply not recorded and the sink degrades to module-level reachability;
    /// a false arg-level claim past the cap is structurally impossible.
    fn chained_bindings_for_idents(
        &self,
        name: &str,
        idents: &[String],
    ) -> Vec<(TaintedBinding, u8)> {
        let mut out = Vec::new();
        for ident in idents {
            // No `nested_scope_shadows` guard on the referenced ident: tainted
            // bindings are recorded scope-insensitively and name-keyed (the
            // direct recorders do the same), and the chain root is normally a
            // local declared in the SAME function body, which the nested
            // declaration stack already contains. Guarding on it would reject
            // the common `const a = req.query.id; const b = ...a...` case
            // inside a route handler. Cross-scope name collisions are the
            // accepted Risk 1, bounded by the hop cap.
            if ident == name {
                continue;
            }
            for (index, binding) in self.tainted_bindings.iter().enumerate() {
                if binding.local != *ident {
                    continue;
                }
                let hop = self.tainted_binding_hops[index];
                if hop >= MAX_TAINT_BINDING_HOPS {
                    tracing::debug!(
                        local = name,
                        via = ident.as_str(),
                        source_path = binding.source_path.as_str(),
                        "taint binding chain dropped: would exceed MAX_TAINT_BINDING_HOPS"
                    );
                    continue;
                }
                out.push((
                    TaintedBinding {
                        local: name.to_string(),
                        source_path: binding.source_path.clone(),
                        source_span_start: binding.source_span_start,
                    },
                    hop + 1,
                ));
            }
        }
        out
    }

    /// Chain an object destructure from a tainted local (`const { id } = a`
    /// where `a` is source-tainted), mirroring the shipped
    /// destructure-from-source capture (issue #1146). Only a bare-identifier
    /// initializer chains; member-expression initializers stay out for the
    /// same reason member roots are excluded from
    /// `collect_chained_taint_idents`.
    pub(super) fn record_chained_tainted_destructure_bindings(
        &mut self,
        obj_pat: &ObjectPattern<'_>,
        init: &Expression<'_>,
    ) {
        let Expression::Identifier(ident) = unwrap_parens(init) else {
            return;
        };
        let idents = vec![ident.name.to_string()];
        let mut chained = Vec::new();
        for local in extract_destructured_names(obj_pat) {
            chained.extend(self.chained_bindings_for_idents(&local, &idents));
        }
        for (binding, hop) in chained {
            self.push_tainted_binding(binding, hop);
        }
    }

    fn record_first_param_source(
        &mut self,
        params: &FormalParameters<'_>,
        source_path: &'static str,
    ) {
        self.record_param_source_at_index(params, 0, source_path);
    }

    fn record_param_source_at_index(
        &mut self,
        params: &FormalParameters<'_>,
        index: usize,
        source_path: &'static str,
    ) {
        let Some(param) = params.items.get(index) else {
            return;
        };
        match &param.pattern {
            BindingPattern::BindingIdentifier(id) => {
                self.record_tainted_param_binding(id.name.as_str(), source_path);
            }
            BindingPattern::ObjectPattern(obj_pat) => {
                for local in extract_destructured_names(obj_pat) {
                    self.record_tainted_param_binding(&local, source_path);
                }
            }
            _ => {}
        }
    }

    fn record_named_param_source(
        &mut self,
        params: &FormalParameters<'_>,
        names: &[&str],
        source_path: &'static str,
    ) {
        for param in &params.items {
            match &param.pattern {
                BindingPattern::BindingIdentifier(id)
                    if names.iter().any(|name| *name == id.name.as_str()) =>
                {
                    self.record_tainted_param_binding(id.name.as_str(), source_path);
                }
                BindingPattern::ObjectPattern(obj_pat) => {
                    for (local, key) in extract_object_pattern_bindings(obj_pat) {
                        if names
                            .iter()
                            .any(|name| key == *name || key.starts_with(&format!("{name}.")))
                        {
                            self.record_tainted_param_binding(&local, source_path);
                        }
                    }
                }
                _ => {}
            }
        }
    }

    pub(super) fn record_graphql_resolver_args_source(&mut self, expr: &Expression<'_>) {
        let Some(params) = function_like_params(expr) else {
            return;
        };
        let Some(param) = params.items.get(1) else {
            return;
        };
        match &param.pattern {
            BindingPattern::BindingIdentifier(id) if id.name == "args" => {
                self.record_param_source_at_index(params, 1, GRAPHQL_ARGS_SOURCE);
            }
            BindingPattern::ObjectPattern(_) => {
                self.record_param_source_at_index(params, 1, GRAPHQL_ARGS_SOURCE);
            }
            _ => {}
        }
    }

    pub(super) fn record_next_function_param_sources(&mut self, func: &Function<'_>) {
        if func
            .id
            .as_ref()
            .is_some_and(|id| is_http_route_handler_name(id.name.as_str()))
        {
            self.record_first_param_source(&func.params, NEXT_REQUEST_SOURCE);
        }
        if function_body_has_use_server(func.body.as_deref()) {
            self.record_named_param_source(&func.params, &["formData"], NEXT_FORM_DATA_SOURCE);
        }
    }

    pub(super) fn record_next_arrow_param_sources(&mut self, expr: &ArrowFunctionExpression<'_>) {
        if function_body_has_use_server(Some(&expr.body)) {
            self.record_named_param_source(&expr.params, &["formData"], NEXT_FORM_DATA_SOURCE);
        }
    }

    pub(super) fn record_framework_callback_param_sources(&mut self, call: &CallExpression<'_>) {
        let callee_path = flatten_callee_path(&call.callee);
        let Some(method) = callee_method_name(&call.callee, callee_path.as_deref()) else {
            return;
        };
        if is_route_registration_method(method) {
            let Some(callee_path) = callee_path.as_deref() else {
                return;
            };
            if !is_framework_route_receiver_path(callee_path, method) {
                return;
            }
            if let Some(params) = route_callback_params(&call.arguments, method) {
                self.record_first_param_source(params, FRAMEWORK_REQUEST_SOURCE);
            }
            return;
        }
        if method == "process"
            && let Some(params) = last_callback_params(&call.arguments)
        {
            self.record_first_param_source(params, QUEUE_JOB_SOURCE);
            return;
        }
        if method == "tool"
            && let Some(params) = last_callback_params(&call.arguments)
        {
            self.record_first_param_source(params, MCP_TOOL_INPUT_SOURCE);
            return;
        }
        if is_trpc_procedure_method(method)
            && is_trpc_procedure_callee(&call.callee, method)
            && let Some(params) = last_callback_params(&call.arguments)
        {
            self.record_named_param_source(params, &["input"], TRPC_INPUT_SOURCE);
        }
    }

    pub(super) fn record_queue_worker_constructor_param_sources(
        &mut self,
        expr: &NewExpression<'_>,
    ) {
        let Some(callee_path) = flatten_callee_path(&expr.callee) else {
            return;
        };
        if callee_path.rsplit('.').next() != Some("Worker") {
            return;
        }
        if let Some(params) = expr.arguments.iter().skip(1).find_map(callback_params) {
            self.record_first_param_source(params, QUEUE_JOB_SOURCE);
        }
    }

    pub(super) fn record_tainted_helper_call_binding(&mut self, name: &str, expr: &Expression<'_>) {
        let Expression::CallExpression(call) = unwrap_parens(expr) else {
            return;
        };
        let Expression::Identifier(callee) = &call.callee else {
            return;
        };
        if self.nested_scope_shadows(callee.name.as_str()) {
            return;
        }
        let Some(helper) = self
            .source_returning_helpers
            .get(callee.name.as_str())
            .cloned()
        else {
            return;
        };

        let mut source_paths = Vec::new();
        for path in &helper.paths {
            let Some(arg_expr) = call
                .arguments
                .get(path.arg_index)
                .and_then(Argument::as_expression)
            else {
                continue;
            };
            source_paths.extend(apply_source_return_path(arg_expr, &path.suffixes));
        }
        source_paths.sort();
        source_paths.dedup();
        for source_path in source_paths {
            self.push_tainted_binding(
                TaintedBinding {
                    local: name.to_string(),
                    source_path,
                    // One-hop helper-return source: the real read lives inside the
                    // helper body, not at this binding, so no concrete read span is
                    // available here. `0` makes the analyze layer anchor at the sink.
                    source_span_start: 0,
                },
                DIRECT_TAINT_HOP,
            );
        }
    }

    fn record_source_returning_function_helper(
        &mut self,
        name: &str,
        params: &FormalParameters<'_>,
        body: &FunctionBody<'_>,
    ) {
        if !self.is_module_scope() {
            return;
        }
        let Some(expr) = extract_function_body_final_return_expr(body) else {
            self.source_returning_helpers.remove(name);
            return;
        };
        if let Some(helper) = source_returning_helper(params, expr) {
            self.source_returning_helpers
                .insert(name.to_string(), helper);
        } else {
            self.source_returning_helpers.remove(name);
        }
    }

    pub(super) fn record_source_returning_function_declaration(&mut self, function: &Function<'_>) {
        let (Some(id), Some(body)) = (function.id.as_ref(), function.body.as_deref()) else {
            return;
        };
        self.record_source_returning_function_helper(id.name.as_str(), &function.params, body);
    }

    /// Record tainted-source bindings for `const { a, b } = <object>.<prop>`,
    /// where the destructured initializer is a member-access chain (or bare
    /// identifier root). Each bound local maps to the FULL flattened init path:
    /// `const { id } = req.query` records `{ local: "id", source_path:
    /// "req.query" }`. Rest patterns are skipped (whole-object capture is out of
    /// the cheap scope). Nested patterns are not destructured.
    pub(super) fn record_tainted_destructure_bindings(
        &mut self,
        obj_pat: &ObjectPattern<'_>,
        expr: &Expression<'_>,
    ) {
        let Some(source_path) = destructure_source_path(expr) else {
            return;
        };
        for local in extract_destructured_names(obj_pat) {
            self.push_tainted_binding(
                TaintedBinding {
                    local,
                    source_path: source_path.clone(),
                    // The destructured initializer read (`req.query` in
                    // `const { id } = req.query`); anchors the source node at the
                    // real read line.
                    source_span_start: expr.span().start,
                },
                DIRECT_TAINT_HOP,
            );
        }
    }
}
