//! Svelte event-dispatch and load-data whole-use capture.

#[allow(
    clippy::wildcard_imports,
    reason = "many Svelte visitor AST types used"
)]
use oxc_ast::ast::*;

use fallow_types::extract::DispatchedEvent;

use super::{ModuleInfoExtractor, ROUTE_LOADER_DATA_OBJECT};

impl ModuleInfoExtractor {
    /// Track the load-data detector's two call-argument exceptions.
    ///
    /// Generic call arguments are intentionally not recorded as whole-object
    /// uses because that would over-credit arbitrary imported objects. The
    /// SvelteKit `data` prop and a binding proven to come from `useLoaderData()`
    /// are different: `fn(data)` / `fn(...data)` can consume any returned key,
    /// so their detector-specific abstention signals are recorded here.
    pub(super) fn record_load_data_whole_arg_use(&mut self, expr: &CallExpression<'_>) {
        for arg in &expr.arguments {
            let route_data_name = match arg {
                Argument::SpreadElement(spread) => match &spread.argument {
                    Expression::Identifier(id) => Some(id.name.as_str()),
                    _ => None,
                },
                Argument::Identifier(id) => Some(id.name.as_str()),
                _ => None,
            };
            if route_data_name.is_some_and(|name| {
                name == "loaderData" || self.route_loader_data_bindings.contains(name)
            }) {
                self.whole_object_uses
                    .push(ROUTE_LOADER_DATA_OBJECT.to_string());
                return;
            }
            if route_data_name == Some("data") {
                self.has_load_data_whole_use = true;
                return;
            }
        }
    }

    /// Track a `const dispatch = createEventDispatcher()` binding for the
    /// `unused-svelte-event` detector. The callee must resolve to a named
    /// `createEventDispatcher` import from `svelte` (the only shape Svelte
    /// supports), so an unrelated local `createEventDispatcher` is ignored. The
    /// local binding name (often `dispatch`, but any name) is recorded; a
    /// `<binding>('<name>')` call then records a `DispatchedEvent`.
    pub(super) fn record_event_dispatch_binding(&mut self, local: &str, init: &Expression<'_>) {
        let Expression::CallExpression(call) = init else {
            return;
        };
        let Expression::Identifier(callee) = &call.callee else {
            return;
        };
        if callee.name != "createEventDispatcher" {
            return;
        }
        if !self.is_named_import_from(callee.name.as_str(), "svelte", "createEventDispatcher") {
            return;
        }
        self.event_dispatch_bindings.insert(local.to_string());
    }

    /// Record a Svelte custom-event dispatch through a `createEventDispatcher()`
    /// binding. A `dispatch('<name>')` literal-arg call records a
    /// `DispatchedEvent`; a `dispatch(<nonLiteral>)` call sets
    /// `has_dynamic_dispatch` (the event name is unknowable, so the whole
    /// component abstains). Gated on the callee resolving to a tracked dispatch
    /// binding, so an ordinary `foo('bar')` call is inert.
    pub(super) fn record_svelte_dispatch_call(&mut self, expr: &CallExpression<'_>) {
        let Expression::Identifier(callee) = &expr.callee else {
            return;
        };
        if !self.event_dispatch_bindings.contains(callee.name.as_str()) {
            return;
        }
        match expr.arguments.first() {
            Some(Argument::StringLiteral(lit)) => {
                self.svelte_dispatched_events.push(DispatchedEvent {
                    name: lit.value.to_string(),
                    span_start: expr.span.start,
                });
            }
            Some(Argument::TemplateLiteral(t)) if t.expressions.is_empty() => {
                if let Some(quasi) = t.quasis.first() {
                    self.svelte_dispatched_events.push(DispatchedEvent {
                        name: quasi.value.raw.to_string(),
                        span_start: expr.span.start,
                    });
                }
            }
            // No argument or a non-literal first arg: the event name is
            // unknowable, so the whole component abstains.
            _ => {
                self.has_dynamic_dispatch = true;
            }
        }
    }

    /// Abstain when a tracked `dispatch` binding is passed as a whole value to
    /// another call (`forwardEvents(dispatch)` / `wrap(...dispatch)`): the helper
    /// can dispatch any event opaquely, so the whole component must abstain. The
    /// callee position is the dispatch call itself (handled by
    /// `record_svelte_dispatch_call`), so only argument positions are inspected.
    pub(super) fn record_svelte_dispatch_whole_arg_use(&mut self, expr: &CallExpression<'_>) {
        for arg in &expr.arguments {
            let used_whole = match arg {
                Argument::Identifier(id) => self.event_dispatch_bindings.contains(id.name.as_str()),
                Argument::SpreadElement(spread) => matches!(
                    &spread.argument,
                    Expression::Identifier(id)
                        if self.event_dispatch_bindings.contains(id.name.as_str())
                ),
                _ => false,
            };
            if used_whole {
                self.has_dynamic_dispatch = true;
                return;
            }
        }
    }
}
