//! Component event calls (Svelte dispatch, Vue emit in markup) and
//! load-data whole-use capture.

use oxc_ast::ast::*;

use std::sync::Arc;

use fallow_types::extract::{DispatchedEvent, SinkLiteralValue};

use super::{ModuleInfoExtractor, ROUTE_LOADER_DATA_OBJECT};
use crate::js_text::JsText;
use crate::template_usage::ScriptBindingContext;

/// The markup state of a snippet extractor. A script extractor has no
/// `script` context, because the `defineEmits` harvest marks the emits of a
/// script and its own declarations give the other bindings.
#[derive(Debug, Default)]
pub struct MarkupContext {
    /// The script binding context of the component. All snippets of the
    /// component share it, so a snippet copies none of its bindings.
    pub(in crate::visitor) script: Option<Arc<ScriptBindingContext>>,
    /// The template locals of the snippet. Each one shadows the script
    /// binding of the same name (`{#each items as dispatch}`).
    pub(in crate::visitor) locals: Vec<String>,
    /// The event names of the emit calls with a literal event name.
    pub(in crate::visitor) emitted: Vec<String>,
    /// An emit call has no literal event name. The component then abstains.
    pub(in crate::visitor) dynamic: bool,
}

impl MarkupContext {
    /// The script context for `name`, when no template local shadows it.
    fn script_for(&self, name: &str) -> Option<&ScriptBindingContext> {
        let script = self.script.as_deref()?;
        (!self.locals.iter().any(|local| local == name)).then_some(script)
    }

    /// Whether `name` is a `createEventDispatcher()` binding of the script.
    pub(in crate::visitor) fn is_dispatch_binding(&self, name: &str) -> bool {
        self.script_for(name)
            .is_some_and(|script| script.event_dispatch_bindings.contains(name))
    }

    /// Whether `name` is a Vue emit binding of the script, or `$emit`.
    fn is_emit_binding(&self, name: &str) -> bool {
        self.script_for(name)
            .is_some_and(|script| script.emit_bindings.contains(name))
    }

    /// The script literal of `key` (`name`, or `Enum.MEMBER` for the enum
    /// `root`), when no template local shadows `root`.
    pub(in crate::visitor) fn sink_literal(
        &self,
        key: &str,
        root: &str,
    ) -> Option<&SinkLiteralValue> {
        self.script_for(root)?.static_sink_literals.get(key)
    }
}

impl ModuleInfoExtractor {
    /// Whether `name` is a `createEventDispatcher()` binding of this script, or
    /// of the script of the component when this extractor reads a snippet.
    fn is_event_dispatch_binding(&self, name: &str) -> bool {
        self.event_dispatch_bindings.contains(name) || self.markup.is_dispatch_binding(name)
    }

    /// Record the component event calls of one call expression.
    pub(super) fn record_component_event_calls(&mut self, expr: &CallExpression<'_>) {
        self.record_svelte_dispatch_call(expr);
        self.record_svelte_dispatch_whole_arg_use(expr);
        self.record_template_emit_call(expr);
    }

    /// Record a Vue `emit("close")` or `$emit("close")` call in a markup
    /// snippet. A call with no literal event name sets the abstain flag.
    fn record_template_emit_call(&mut self, expr: &CallExpression<'_>) {
        let Expression::Identifier(callee) = &expr.callee else {
            return;
        };
        if !self.markup.is_emit_binding(callee.name.as_str()) {
            return;
        }
        match expr.arguments.first() {
            Some(Argument::StringLiteral(lit)) => {
                self.markup.emitted.push(lit.value.text().into_owned());
            }
            Some(Argument::TemplateLiteral(tpl)) if tpl.expressions.is_empty() => {
                if let Some(quasi) = tpl.quasis.first() {
                    self.markup.emitted.push(quasi.value.raw.to_string());
                }
            }
            _ => self.markup.dynamic = true,
        }
    }

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
    fn record_svelte_dispatch_call(&mut self, expr: &CallExpression<'_>) {
        let Expression::Identifier(callee) = &expr.callee else {
            return;
        };
        if !self.is_event_dispatch_binding(callee.name.as_str()) {
            return;
        }
        match expr.arguments.first() {
            Some(Argument::StringLiteral(lit)) => {
                self.svelte_dispatched_events.push(DispatchedEvent {
                    name: lit.value.text().into_owned(),
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
    fn record_svelte_dispatch_whole_arg_use(&mut self, expr: &CallExpression<'_>) {
        for arg in &expr.arguments {
            let used_whole = match arg {
                Argument::Identifier(id) => self.is_event_dispatch_binding(id.name.as_str()),
                Argument::SpreadElement(spread) => matches!(
                    &spread.argument,
                    Expression::Identifier(id)
                        if self.is_event_dispatch_binding(id.name.as_str())
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
