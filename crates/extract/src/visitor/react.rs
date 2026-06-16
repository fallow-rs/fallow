//! React / JSX structural extraction (Phase 0 foundation).
//!
//! Captures the IR every later React-health phase consumes: identified
//! component functions, their props (used-in-body), hook call sites, and render
//! edges (one component rendering another). Pure syntactic analysis (ADR-001):
//! nothing here resolves a type, and anything that would require cross-file type
//! info is recorded as an abstain flag rather than guessed.
//!
//! The whole walk is gated on `jsx_capable` (set by `parse.rs` only for
//! `.jsx`/`.tsx` parses), so it is a no-op on non-JSX files and never regresses
//! the `audit` hot path on non-React repos.

#[allow(clippy::wildcard_imports, reason = "many AST types used")]
use oxc_ast::ast::*;
use rustc_hash::FxHashSet;

use fallow_types::extract::{
    ComponentFunction, ComponentFunctionKind, ComponentProp, ForwardAttr, HookUse, HookUseKind,
    RenderEdge,
};

use super::{ModuleInfoExtractor, PendingComponentArrow};

impl ModuleInfoExtractor {
    /// Pre-scan a variable declaration for named arrow / function-expression
    /// bindings that may be React components, recording per-arrow metadata keyed
    /// by the function span so the function-body visit can push the component
    /// stack with the binding name. No-op unless `jsx_capable`.
    ///
    /// Handles three shapes:
    /// - `const Foo = () => <.../>` / `const Foo = function () { ... }` (Arrow)
    /// - `const Foo = forwardRef((props, ref) => <.../>)` (ForwardRefWrapper)
    /// - `const Foo = memo((props) => <.../>)` (MemoWrapper)
    pub(crate) fn react_prescan_variable_declaration(&mut self, decl: &VariableDeclaration<'_>) {
        if !self.jsx_capable {
            return;
        }
        for declarator in &decl.declarations {
            let BindingPattern::BindingIdentifier(id) = &declarator.id else {
                continue;
            };
            let name = id.name.as_str();
            if !is_component_name(name) {
                continue;
            }
            let Some(init) = &declarator.init else {
                continue;
            };
            let is_exported = self.is_exported_binding(name);
            if let Some((func_span, kind)) = classify_component_init(init) {
                self.pending_component_arrows.insert(
                    func_span,
                    PendingComponentArrow {
                        name: name.to_string(),
                        kind,
                        is_exported,
                    },
                );
            }
        }
    }

    /// Enter a `function Foo() { ... }` declaration as a potential React
    /// component. Returns `true` if a component scope was pushed (the caller must
    /// call [`Self::react_exit_component`] with the same bool after the walk).
    pub(crate) fn react_enter_function(&mut self, func: &Function<'_>) -> bool {
        if !self.jsx_capable {
            return false;
        }
        // A named arrow / function-expression binding (`const Foo = forwardRef(...)`)
        // was pre-registered by `react_prescan_variable_declaration`; consume it
        // by span so the stack carries the binding name rather than the (absent)
        // function id.
        if let Some(pending) = self.pending_component_arrows.remove(&func.span) {
            let component = pending.name.clone();
            self.begin_component(
                pending.name,
                func.span.start,
                pending.kind,
                pending.is_exported,
            );
            self.harvest_function_props(&component, &func.params, func.body.as_deref());
            return true;
        }
        let Some(id) = func.id.as_ref() else {
            return false;
        };
        let name = id.name.as_str();
        if !is_component_name(name) || !function_body_returns_jsx(func.body.as_deref()) {
            return false;
        }
        let is_exported = self.is_exported_binding(name);
        self.begin_component(
            name.to_string(),
            func.span.start,
            ComponentFunctionKind::FnDecl,
            is_exported,
        );
        self.harvest_function_props(name, &func.params, func.body.as_deref());
        true
    }

    /// Enter an arrow function as a potential React component. Only fires for a
    /// named binding pre-registered by `react_prescan_variable_declaration`
    /// (an anonymous inline arrow is never a component definition we name).
    /// Returns `true` if a component scope was pushed.
    pub(crate) fn react_enter_arrow(&mut self, expr: &ArrowFunctionExpression<'_>) -> bool {
        if !self.jsx_capable {
            return false;
        }
        let Some(pending) = self.pending_component_arrows.remove(&expr.span) else {
            return false;
        };
        let component = pending.name.clone();
        self.begin_component(
            pending.name,
            expr.span.start,
            pending.kind,
            pending.is_exported,
        );
        self.harvest_arrow_props(&component, &expr.params, &expr.body);
        true
    }

    /// Pop the component stack if [`Self::react_enter_function`] /
    /// [`Self::react_enter_arrow`] pushed one.
    pub(crate) fn react_exit_component(&mut self, pushed: bool) {
        if pushed {
            self.component_stack.pop();
        }
    }

    /// Record a JSX element: a render edge for a component tag (capitalized or a
    /// member-expression `Foo.Bar`), plus the passed attribute names and whether
    /// a spread is present. Lowercase host tags are skipped for render purposes.
    /// No-op unless `jsx_capable`.
    pub(crate) fn react_record_jsx_element(&mut self, element: &JSXElement<'_>) {
        if !self.jsx_capable {
            return;
        }
        let opening = &element.opening_element;
        // A `*.Provider` member-expression tag is a context provider in the
        // subtree: the prop-drilling phase downgrades/abstains on chains through
        // the enclosing component (the drilling may be deliberate, or the value is
        // about to be provided). Detected on EVERY render (component or host
        // child), so it fires even when the provider wraps host markup.
        if jsx_is_provider_tag(&opening.name) {
            self.react_mark_renders_provider();
        }
        // A render-prop / children-as-function child marks the enclosing
        // component: `<Foo render={() => ...}/>` or `<Foo>{() => ...}</Foo>`.
        if jsx_has_function_render_prop(opening) || jsx_children_has_function(&element.children) {
            self.react_mark_children_as_function();
        }
        let Some(child_name) = jsx_component_tag_name(&opening.name) else {
            // Lowercase host element (`<div>`): not a render edge. Nesting depth
            // is measured by the surrounding cognitive-complexity visitor; here
            // we only record component renders.
            return;
        };
        let (attr_names, has_spread, forward_attrs, has_complex_forward) =
            collect_jsx_attributes(&opening.attributes);
        let parent_component = self.component_stack.last().cloned().unwrap_or_default();
        self.render_edges.push(RenderEdge {
            parent_component,
            child_component_name: child_name,
            attr_names,
            has_spread,
            forward_attrs,
            has_complex_forward,
        });
    }

    /// Mark the enclosing component as rendering a context provider (abstain
    /// signal for prop-drilling). No-op outside a component scope.
    fn react_mark_renders_provider(&mut self) {
        if let Some(component) = self.component_functions.last_mut() {
            component.renders_provider = true;
        }
    }

    /// Mark the enclosing component as passing a function as a child / render
    /// prop (abstain signal for prop-drilling). No-op outside a component scope.
    fn react_mark_children_as_function(&mut self) {
        if let Some(component) = self.component_functions.last_mut() {
            component.has_children_as_function = true;
        }
    }

    /// Record a React hook call (`useState` / `useEffect` / `useMemo` /
    /// `useCallback` / custom `use*`). Only fires inside an identified component
    /// (the stack is non-empty), so a `use*`-named call outside a component (a
    /// custom-hook definition's own body still counts because that body is itself
    /// a component-shaped scope only when it renders JSX) is not recorded as a
    /// component hook. No-op unless `jsx_capable`.
    pub(crate) fn react_record_hook_call(&mut self, call: &CallExpression<'_>) {
        if !self.jsx_capable || self.component_stack.is_empty() {
            return;
        }
        // `cloneElement` / `React.cloneElement` injects props by reflection, so
        // the static forward-set is incomplete: the prop-drilling phase abstains
        // on any chain through this component. Checked before the hook gate
        // because `cloneElement` is not a `use*` call.
        if is_clone_element_callee(&call.callee)
            && let Some(component) = self.component_functions.last_mut()
        {
            component.uses_clone_element = true;
        }
        let Expression::Identifier(callee) = &call.callee else {
            return;
        };
        let Some(kind) = hook_kind(callee.name.as_str()) else {
            return;
        };
        let dep_array_arity = hook_dep_array_arity(kind, &call.arguments);
        self.hook_uses.push(HookUse {
            kind,
            dep_array_arity,
            span_start: call.span.start,
        });
    }

    /// Push a component scope and record its `ComponentFunction`.
    fn begin_component(
        &mut self,
        name: String,
        span_start: u32,
        kind: ComponentFunctionKind,
        is_exported: bool,
    ) {
        self.component_functions.push(ComponentFunction {
            name: name.clone(),
            span_start,
            kind,
            is_exported,
            // Populated by `harvest_*_props` if the signature is unharvestable.
            has_unharvestable_props: false,
            // Populated by `react_record_jsx_element` / `react_record_call` as the
            // body is walked (prop-drilling abstain signals).
            uses_clone_element: false,
            renders_provider: false,
            has_children_as_function: false,
            // Populated by `harvest_props_from_params` once the props binding
            // name and the body are in hand (thin-wrapper extraction signal).
            is_pure_passthrough: false,
        });
        self.component_stack.push(name);
    }

    /// Harvest props from a `function` component's parameter list, computing
    /// each prop's used-in-body flag against `body`.
    fn harvest_function_props(
        &mut self,
        component: &str,
        params: &FormalParameters<'_>,
        body: Option<&FunctionBody<'_>>,
    ) {
        self.harvest_props_from_params(
            component,
            params.items.first().map(|p| &p.pattern),
            params.rest.is_some(),
            body,
        );
    }

    /// Harvest props from an arrow component's parameter list, computing each
    /// prop's used-in-body flag against `body` (oxc wraps an expression-body
    /// arrow's returned expression in a single statement, so one body type
    /// covers both forms).
    fn harvest_arrow_props(
        &mut self,
        component: &str,
        params: &FormalParameters<'_>,
        body: &FunctionBody<'_>,
    ) {
        self.harvest_props_from_params(
            component,
            params.items.first().map(|p| &p.pattern),
            params.rest.is_some(),
            Some(body),
        );
    }

    /// Harvest props from the first (props) parameter. v1 covers the
    /// inline-destructured literal form (`{ a, b }`); a bare identifier,
    /// rest/spread, or absent destructure marks the just-pushed component's props
    /// unharvestable (abstain, ADR-001). Each harvested prop's `used_in_script`
    /// is set from a focused resolved-reference pass over the component `body`,
    /// so the detector flags only props read NOWHERE in their component.
    fn harvest_props_from_params(
        &mut self,
        component: &str,
        first: Option<&BindingPattern<'_>>,
        has_rest_param: bool,
        body: Option<&FunctionBody<'_>>,
    ) {
        let mark_unharvestable = |this: &mut Self| {
            if let Some(component) = this.component_functions.last_mut() {
                component.has_unharvestable_props = true;
            }
        };

        // Thin-wrapper extraction (independent of the harvestable path below): a
        // pure passthrough forwards a spread of its own props binding. The spread
        // root can only be a bare-identifier props param (`(props) =>`) or an
        // object-rest local (`({ a, ...rest }) =>`); a flat object destructure
        // (`({ a, b }) =>`) has no single root to spread, so it never qualifies.
        // Computed here because this is the one site with both the props binding
        // shape and the component body in hand; the just-pushed component is
        // `component_functions.last_mut()` (`begin_component` ran immediately
        // before harvest). The bare-`props` signature also sets
        // `has_unharvestable_props` below, but `<Child {...props}/>` is THE
        // canonical thin wrapper, so the two flags are orthogonal.
        if let Some(props_root) = passthrough_spread_root(first)
            && body_is_pure_passthrough(body, &props_root)
            && let Some(component) = self.component_functions.last_mut()
        {
            component.is_pure_passthrough = true;
        }

        if has_rest_param {
            mark_unharvestable(self);
            return;
        }
        let Some(pattern) = first else {
            // Zero-parameter component: no props to harvest, nothing to abstain.
            return;
        };
        let BindingPattern::ObjectPattern(obj) = pattern else {
            // `props` bare identifier or an array pattern: not statically
            // harvestable as named props (v1). A bare `props` identifier also
            // covers the `forwardRef<T, Props>((props, ref) => ...)` /
            // `memo((props) => ...)` imported-interface case the abstain ladder
            // requires (ADR-001): the names live in an unresolvable type, so the
            // signature carries a bare identifier, not an inline destructure.
            mark_unharvestable(self);
            return;
        };
        if obj.rest.is_some() {
            // `{ a, ...rest }`: rest can carry any prop; abstain.
            mark_unharvestable(self);
            return;
        }

        // Collect (declared-name, local-binding, span) for each statically
        // harvestable prop. A computed / non-flat key abstains on the whole
        // component (zero-FP doctrine: never guess a name we cannot read).
        let mut harvested: Vec<(String, String, u32)> = Vec::new();
        for prop in &obj.properties {
            let key_name = match &prop.key {
                PropertyKey::StaticIdentifier(id) => id.name.to_string(),
                PropertyKey::StringLiteral(s) => s.value.to_string(),
                _ => {
                    // Computed key: cannot harvest the name; abstain on the
                    // whole component to stay zero-FP.
                    mark_unharvestable(self);
                    return;
                }
            };
            let Some(local) = binding_pattern_local_name(&prop.value) else {
                // Nested destructure (`{ user: { name } }`): cannot flatten the
                // local with confidence; abstain on the whole component.
                mark_unharvestable(self);
                return;
            };
            harvested.push((key_name, local, prop.span.start));
        }

        // Used-in-body: a destructured local with at least one resolved
        // reference inside the component body (mirrors the Vue script-usage
        // check). `used_outside_forward` additionally tracks whether the local is
        // referenced OUTSIDE a child-JSX attribute value expression (a
        // substantive consumption vs a pure forward), the prop-drilling signal.
        // Both are computed in one body pass for all locals.
        let local_refs = harvested
            .iter()
            .map(|(_, local, _)| local.as_str())
            .collect::<Vec<_>>();
        let usage = resolve_body_local_usage(body, &local_refs);

        for (name, local, span_start) in harvested {
            let used_in_script = usage.used.contains(local.as_str());
            let used_outside_forward = usage.used_outside_forward.contains(local.as_str());
            self.react_props.push(ComponentProp {
                name,
                local,
                span_start,
                used_in_script,
                // React has no template; always false (the struct is shared with
                // Vue where this is the template-usage bit).
                used_in_template: false,
                component: component.to_string(),
                used_outside_forward,
            });
        }
    }

    /// Whether a top-level binding name is exported from this module (a named
    /// export or a local export specifier). Used to set `is_exported` on a
    /// `ComponentFunction` so the prop phase can abstain on public-API
    /// components.
    fn is_exported_binding(&self, name: &str) -> bool {
        self.exports.iter().any(|export| {
            export
                .local_name
                .as_deref()
                .is_some_and(|local| local == name)
                || export.name.matches_str(name)
        })
    }
}

/// Per-prop-local body usage: which locals are referenced at all (`used`) and
/// which are referenced OUTSIDE a child-JSX attribute value expression
/// (`used_outside_forward`, the prop-drilling consumer signal).
struct BodyLocalUsage {
    used: FxHashSet<String>,
    used_outside_forward: FxHashSet<String>,
}

/// Compute, for each of `locals`, whether it is referenced anywhere in the
/// component `body` and whether it is referenced outside a child-JSX attribute
/// value expression.
///
/// Pure syntactic, no `oxc_semantic`: a body fragment cannot be fed to
/// `SemanticBuilder` in isolation (it is not a `Program`). A focused `Visit`
/// collects every identifier reference and tracks whether the cursor is inside a
/// JSX ATTRIBUTE value container (a forward site). The check is CONSERVATIVE in
/// the over-credit direction for `used` (a name appearing anywhere counts as
/// used, so a finding can only be suppressed, never created) and in the
/// over-credit direction for `used_outside_forward` (only references provably
/// inside an attribute value container are excluded; anything else, including JSX
/// CHILDREN expressions, counts as substantive consumption). Both directions
/// favour false-negatives over false-positives, the zero-FP house rule.
fn resolve_body_local_usage(body: Option<&FunctionBody<'_>>, locals: &[&str]) -> BodyLocalUsage {
    let mut usage = BodyLocalUsage {
        used: FxHashSet::default(),
        used_outside_forward: FxHashSet::default(),
    };
    let Some(body) = body else {
        return usage;
    };
    if locals.is_empty() {
        return usage;
    }
    let wanted: FxHashSet<&str> = locals.iter().copied().collect();
    let mut visitor = BodyIdentVisitor {
        wanted: &wanted,
        used: &mut usage.used,
        used_outside_forward: &mut usage.used_outside_forward,
        attr_value_depth: 0,
    };
    for stmt in &body.statements {
        oxc_ast_visit::Visit::visit_statement(&mut visitor, stmt);
    }
    usage
}

/// Collects every identifier reference in a component body whose name is one of
/// the wanted prop locals. Captures `IdentifierReference` (a bare read, the
/// object root of a member access, a call argument, a JSX expression value),
/// which covers every shape a destructured prop is read through. Tracks
/// `attr_value_depth` so a reference inside a child-JSX attribute value (a
/// forward site) is excluded from `used_outside_forward`.
struct BodyIdentVisitor<'a, 'b> {
    wanted: &'a FxHashSet<&'a str>,
    used: &'b mut FxHashSet<String>,
    used_outside_forward: &'b mut FxHashSet<String>,
    attr_value_depth: u32,
}

impl<'a> oxc_ast_visit::Visit<'a> for BodyIdentVisitor<'_, '_> {
    fn visit_identifier_reference(&mut self, ident: &IdentifierReference<'a>) {
        let name = ident.name.as_str();
        if self.wanted.contains(name) {
            self.used.insert(name.to_string());
            if self.attr_value_depth == 0 {
                self.used_outside_forward.insert(name.to_string());
            }
        }
    }

    fn visit_jsx_attribute(&mut self, attr: &JSXAttribute<'a>) {
        // A reference inside a child attribute VALUE is a forward, not a
        // consumption. Mark the depth only while descending into the value
        // (the attribute NAME carries no identifier reference).
        if let Some(value) = &attr.value {
            self.attr_value_depth += 1;
            oxc_ast_visit::walk::walk_jsx_attribute_value(self, value);
            self.attr_value_depth -= 1;
        }
    }
}

/// React convention: a component is named with a capital first letter. A
/// lowercase `use*` is a hook, not a component.
fn is_component_name(name: &str) -> bool {
    name.chars().next().is_some_and(char::is_uppercase)
}

/// Classify a variable initializer as a React component definition, returning
/// the inner function/arrow span (the key the body visit looks up) and the
/// component kind. Returns `None` when the init is not a component shape.
fn classify_component_init(
    init: &Expression<'_>,
) -> Option<(oxc_span::Span, ComponentFunctionKind)> {
    match init {
        Expression::ArrowFunctionExpression(arrow) if arrow_returns_jsx(arrow) => {
            Some((arrow.span, ComponentFunctionKind::Arrow))
        }
        Expression::FunctionExpression(func) if function_body_returns_jsx(func.body.as_deref()) => {
            Some((func.span, ComponentFunctionKind::Arrow))
        }
        Expression::CallExpression(call) => classify_wrapper_call(call),
        Expression::ParenthesizedExpression(paren) => classify_component_init(&paren.expression),
        Expression::TSAsExpression(ts_as) => classify_component_init(&ts_as.expression),
        Expression::TSSatisfiesExpression(ts_sat) => classify_component_init(&ts_sat.expression),
        _ => None,
    }
}

/// Classify a `forwardRef(...)` / `memo(...)` / `React.forwardRef(...)` /
/// `React.memo(...)` wrapper whose first argument is an arrow / function
/// expression. The inner function's span is the stack-push key.
fn classify_wrapper_call(
    call: &CallExpression<'_>,
) -> Option<(oxc_span::Span, ComponentFunctionKind)> {
    let wrapper = wrapper_callee_name(&call.callee)?;
    let kind = match wrapper {
        "forwardRef" => ComponentFunctionKind::ForwardRefWrapper,
        "memo" => ComponentFunctionKind::MemoWrapper,
        _ => return None,
    };
    let first = call.arguments.first()?.as_expression()?;
    match first {
        Expression::ArrowFunctionExpression(arrow) => Some((arrow.span, kind)),
        Expression::FunctionExpression(func) => Some((func.span, kind)),
        Expression::ParenthesizedExpression(paren) => match &paren.expression {
            Expression::ArrowFunctionExpression(arrow) => Some((arrow.span, kind)),
            Expression::FunctionExpression(func) => Some((func.span, kind)),
            _ => None,
        },
        _ => None,
    }
}

/// Extract the trailing identifier of a wrapper callee: `forwardRef` from a bare
/// call, or `forwardRef` / `memo` from `React.forwardRef` / `React.memo`.
fn wrapper_callee_name<'a>(callee: &'a Expression<'_>) -> Option<&'a str> {
    match callee {
        Expression::Identifier(ident) => Some(ident.name.as_str()),
        Expression::StaticMemberExpression(member) => Some(member.property.name.as_str()),
        _ => None,
    }
}

/// Whether an arrow function's body returns JSX: either an expression-body arrow
/// whose expression is JSX (`() => <.../>`) or a block-body arrow with a
/// `return <.../>` statement.
fn arrow_returns_jsx(arrow: &ArrowFunctionExpression<'_>) -> bool {
    if arrow.expression {
        // Expression-body arrow: the body is a single ExpressionStatement
        // wrapping the returned expression.
        return arrow
            .body
            .statements
            .first()
            .is_some_and(|stmt| match stmt {
                Statement::ExpressionStatement(expr_stmt) => {
                    is_jsx_expression(&expr_stmt.expression)
                }
                _ => false,
            });
    }
    function_body_returns_jsx(Some(&arrow.body))
}

/// Whether a function body contains a `return <jsx/>` statement at any depth
/// (covering early returns and conditional branches that return JSX).
fn function_body_returns_jsx(body: Option<&FunctionBody<'_>>) -> bool {
    let Some(body) = body else {
        return false;
    };
    body.statements.iter().any(statement_returns_jsx)
}

/// Whether a statement (recursively, through control flow) returns JSX.
fn statement_returns_jsx(stmt: &Statement<'_>) -> bool {
    match stmt {
        Statement::ReturnStatement(ret) => ret.argument.as_ref().is_some_and(is_jsx_expression),
        Statement::IfStatement(if_stmt) => {
            statement_returns_jsx(&if_stmt.consequent)
                || if_stmt
                    .alternate
                    .as_ref()
                    .is_some_and(statement_returns_jsx)
        }
        Statement::BlockStatement(block) => block.body.iter().any(statement_returns_jsx),
        Statement::SwitchStatement(switch) => switch
            .cases
            .iter()
            .any(|case| case.consequent.iter().any(statement_returns_jsx)),
        Statement::TryStatement(try_stmt) => {
            try_stmt.block.body.iter().any(statement_returns_jsx)
                || try_stmt
                    .handler
                    .as_ref()
                    .is_some_and(|h| h.body.body.iter().any(statement_returns_jsx))
                || try_stmt
                    .finalizer
                    .as_ref()
                    .is_some_and(|f| f.body.iter().any(statement_returns_jsx))
        }
        _ => false,
    }
}

/// Whether an expression is JSX, unwrapping parentheses, logical/conditional
/// shortcuts, and `as`/`satisfies` casts that commonly wrap a returned element.
fn is_jsx_expression(expr: &Expression<'_>) -> bool {
    match expr {
        Expression::JSXElement(_) | Expression::JSXFragment(_) => true,
        Expression::ParenthesizedExpression(paren) => is_jsx_expression(&paren.expression),
        Expression::ConditionalExpression(cond) => {
            is_jsx_expression(&cond.consequent) || is_jsx_expression(&cond.alternate)
        }
        Expression::LogicalExpression(logical) => is_jsx_expression(&logical.right),
        Expression::TSAsExpression(ts_as) => is_jsx_expression(&ts_as.expression),
        Expression::TSSatisfiesExpression(ts_sat) => is_jsx_expression(&ts_sat.expression),
        Expression::TSNonNullExpression(ts_non_null) => is_jsx_expression(&ts_non_null.expression),
        _ => false,
    }
}

/// Resolve a JSX opening-element name to a rendered component name. Returns the
/// capitalized identifier (`Foo`), the full member-expression path (`Foo.Bar`),
/// or `None` for a lowercase host element (`div`).
fn jsx_component_tag_name(name: &JSXElementName<'_>) -> Option<String> {
    match name {
        JSXElementName::Identifier(ident) => {
            let n = ident.name.as_str();
            is_component_name(n).then(|| n.to_string())
        }
        JSXElementName::IdentifierReference(ident) => {
            let n = ident.name.as_str();
            is_component_name(n).then(|| n.to_string())
        }
        // A member-expression tag (`Foo.Bar`) is always a component render
        // (host elements are never member expressions).
        JSXElementName::MemberExpression(member) => Some(jsx_member_path(member)),
        // `this.X` and namespaced `<ns:tag>` are out of scope (rare; abstain).
        JSXElementName::ThisExpression(_) | JSXElementName::NamespacedName(_) => None,
    }
}

/// Flatten a JSX member-expression tag (`Foo.Bar.Baz`) into a dotted path.
fn jsx_member_path(member: &JSXMemberExpression<'_>) -> String {
    let object = match &member.object {
        JSXMemberExpressionObject::IdentifierReference(ident) => ident.name.to_string(),
        JSXMemberExpressionObject::MemberExpression(inner) => jsx_member_path(inner),
        JSXMemberExpressionObject::ThisExpression(_) => "this".to_string(),
    };
    format!("{object}.{}", member.property.name)
}

/// Collect, for a JSX opening element:
/// - the attribute (prop) names, in source order;
/// - whether a spread (`{...x}`) is present;
/// - the forwarded attributes (child attr NAME paired with the identifier ROOT of
///   its value expression), recorded ONLY for plain identifier / member-root
///   values (the prop-drilling forward signal);
/// - whether any attribute value is a COMPLEX expression (a call, arrow/function,
///   conditional, JSX element, template literal, etc.) whose root was not
///   recorded (so a forwarded prop flowing through it cannot be a pure forward).
fn collect_jsx_attributes(
    attrs: &[JSXAttributeItem<'_>],
) -> (Vec<String>, bool, Vec<ForwardAttr>, bool) {
    let mut names = Vec::new();
    let mut has_spread = false;
    let mut forward_attrs: Vec<ForwardAttr> = Vec::new();
    let mut has_complex_forward = false;
    for item in attrs {
        match item {
            JSXAttributeItem::Attribute(attr) => {
                let attr_name = match &attr.name {
                    JSXAttributeName::Identifier(ident) => Some(ident.name.to_string()),
                    JSXAttributeName::NamespacedName(ns) => {
                        Some(format!("{}:{}", ns.namespace.name, ns.name.name))
                    }
                };
                if let Some(name) = &attr_name {
                    names.push(name.clone());
                }
                classify_attr_value(
                    attr_name.as_deref(),
                    attr.value.as_ref(),
                    &mut forward_attrs,
                    &mut has_complex_forward,
                );
            }
            JSXAttributeItem::SpreadAttribute(_) => has_spread = true,
        }
    }
    (names, has_spread, forward_attrs, has_complex_forward)
}

/// Classify one JSX attribute value into the forward signal. A plain identifier
/// (`x`) or member-root access (`x.y.z`) value records `{ attr, root }` into
/// `forward_attrs`. Any other expression shape (call, arrow/function,
/// conditional, JSX, template, logical, etc.) sets `has_complex_forward` so a
/// forwarded prop flowing only through it is not treated as a pure forward. A
/// string-literal / boolean (no-value) / empty-container attribute carries no
/// identifier, so it contributes nothing.
fn classify_attr_value(
    attr_name: Option<&str>,
    value: Option<&JSXAttributeValue<'_>>,
    forward_attrs: &mut Vec<ForwardAttr>,
    has_complex_forward: &mut bool,
) {
    let Some(JSXAttributeValue::ExpressionContainer(container)) = value else {
        // `foo="bar"` (StringLiteral), `disabled` (None), or `foo=<El/>`
        // (Element/Fragment): the last is element-as-prop indirection, a complex
        // forward.
        if matches!(
            value,
            Some(JSXAttributeValue::Element(_) | JSXAttributeValue::Fragment(_))
        ) {
            *has_complex_forward = true;
        }
        return;
    };
    let root = match &container.expression {
        // `{x}` and `{x.y.z}`: a pure forward whose root identifier is `x`.
        JSXExpression::Identifier(ident) => Some(ident.name.to_string()),
        JSXExpression::StaticMemberExpression(_) | JSXExpression::ComputedMemberExpression(_) => {
            member_expression_root(&container.expression).map(ToString::to_string)
        }
        // `{}` empty container: nothing.
        JSXExpression::EmptyExpression(_) => return,
        // Everything else (call, arrow/function render-prop, conditional, logical,
        // template literal, JSX, object/array literal, etc.) is a complex forward.
        _ => {
            *has_complex_forward = true;
            return;
        }
    };
    match (attr_name, root) {
        (Some(attr), Some(root)) => forward_attrs.push(ForwardAttr {
            attr: attr.to_string(),
            root,
        }),
        // A member chain whose root is not a plain identifier (`this.x`, a
        // parenthesized expr) is a complex forward we cannot map to a prop.
        _ => *has_complex_forward = true,
    }
}

/// The leftmost identifier root of a member-expression chain
/// (`a.b.c` -> `a`, `a[i].b` -> `a`). `None` when the root is not a plain
/// identifier (a call, `this`, a parenthesized expression, etc.).
fn member_expression_root<'a>(expr: &'a JSXExpression<'a>) -> Option<&'a str> {
    fn walk<'a>(expr: &'a Expression<'_>) -> Option<&'a str> {
        match expr {
            Expression::Identifier(ident) => Some(ident.name.as_str()),
            Expression::StaticMemberExpression(member) => walk(&member.object),
            Expression::ComputedMemberExpression(member) => walk(&member.object),
            _ => None,
        }
    }
    match expr {
        JSXExpression::StaticMemberExpression(member) => walk(&member.object),
        JSXExpression::ComputedMemberExpression(member) => walk(&member.object),
        _ => None,
    }
}

/// Whether a JSX tag is a `*.Provider` member-expression tag
/// (`<FooContext.Provider>`). Plain identifiers and deeper member paths whose
/// trailing segment is not `Provider` do not match.
fn jsx_is_provider_tag(name: &JSXElementName<'_>) -> bool {
    matches!(name, JSXElementName::MemberExpression(member) if member.property.name == "Provider")
}

/// Whether an opening element carries a function-valued render-prop attribute
/// (`render={() => ...}` / `children={() => ...}`): a render-prop signal.
fn jsx_has_function_render_prop(opening: &JSXOpeningElement<'_>) -> bool {
    opening.attributes.iter().any(|item| {
        let JSXAttributeItem::Attribute(attr) = item else {
            return false;
        };
        let Some(JSXAttributeValue::ExpressionContainer(container)) = &attr.value else {
            return false;
        };
        matches!(
            &container.expression,
            JSXExpression::ArrowFunctionExpression(_) | JSXExpression::FunctionExpression(_)
        )
    })
}

/// Whether a JSX element's children include a function expression
/// (`<Foo>{() => ...}</Foo>`): a children-as-function / render-prop signal.
fn jsx_children_has_function(children: &[JSXChild<'_>]) -> bool {
    children.iter().any(|child| {
        let JSXChild::ExpressionContainer(container) = child else {
            return false;
        };
        matches!(
            &container.expression,
            JSXExpression::ArrowFunctionExpression(_) | JSXExpression::FunctionExpression(_)
        )
    })
}

/// Whether a call callee is `cloneElement` or `React.cloneElement` (the trailing
/// identifier match, mirroring the `forwardRef` / `memo` wrapper detection).
fn is_clone_element_callee(callee: &Expression<'_>) -> bool {
    match callee {
        Expression::Identifier(ident) => ident.name == "cloneElement",
        Expression::StaticMemberExpression(member) => member.property.name == "cloneElement",
        _ => false,
    }
}

/// The identifier root a `{...props}` spread could forward, derived from the
/// first (props) parameter pattern. Returns the bare-identifier param name
/// (`(props) =>` -> `props`) or an object-rest local (`({ a, ...rest }) =>` ->
/// `rest`). A flat object destructure has no single spreadable root (`None`), so
/// it never qualifies as a thin wrapper. An array pattern / nested shape is also
/// `None`.
fn passthrough_spread_root(first: Option<&BindingPattern<'_>>) -> Option<String> {
    match first? {
        BindingPattern::BindingIdentifier(id) => Some(id.name.to_string()),
        BindingPattern::ObjectPattern(obj) => match &obj.rest {
            Some(rest) => binding_pattern_local_name(&rest.argument),
            None => None,
        },
        _ => None,
    }
}

/// Whether a component body is a pure structural passthrough: exactly ONE
/// statement that returns a single capitalized/member-expression JSX element
/// which forwards `{...<props_root>}` with NO named attributes alongside the
/// spread, no host wrapper, no extra children, and no self-render. A fragment
/// wrapping a single element child qualifies. Conditional / logical returns
/// abstain (they add a host-less branch and are not pure indirection). Pure
/// syntactic on the component's own AST (ADR-001); the cross-component
/// `thin-wrapper` phase adds hook-density / cyclomatic / resolution joins.
fn body_is_pure_passthrough(body: Option<&FunctionBody<'_>>, props_root: &str) -> bool {
    let Some(body) = body else {
        return false;
    };
    // Exactly one statement (an expression-body arrow is wrapped in a single
    // `ExpressionStatement`; a block body must be a lone `return`). Any extra
    // statement (a local declaration, a log, a guard) disqualifies.
    let [stmt] = body.statements.as_slice() else {
        return false;
    };
    let returned = match stmt {
        Statement::ReturnStatement(ret) => ret.argument.as_ref(),
        Statement::ExpressionStatement(expr_stmt) => Some(&expr_stmt.expression),
        _ => None,
    };
    let Some(returned) = returned else {
        return false;
    };
    let Some(element) = unwrap_single_passthrough_element(returned) else {
        return false;
    };
    jsx_element_is_bare_props_spread(element, props_root)
}

/// Unwrap parens / `as` / `satisfies` / non-null exactly as
/// [`is_jsx_expression`] does, then return the single JSX element if the value is
/// either a bare element or a fragment wrapping exactly one element child.
/// Conditional / logical / any other shape returns `None` (abstain).
fn unwrap_single_passthrough_element<'a>(expr: &'a Expression<'a>) -> Option<&'a JSXElement<'a>> {
    match expr {
        Expression::JSXElement(element) => Some(element),
        Expression::JSXFragment(fragment) => single_element_child(&fragment.children),
        Expression::ParenthesizedExpression(paren) => {
            unwrap_single_passthrough_element(&paren.expression)
        }
        Expression::TSAsExpression(ts_as) => unwrap_single_passthrough_element(&ts_as.expression),
        Expression::TSSatisfiesExpression(ts_sat) => {
            unwrap_single_passthrough_element(&ts_sat.expression)
        }
        Expression::TSNonNullExpression(ts_non_null) => {
            unwrap_single_passthrough_element(&ts_non_null.expression)
        }
        _ => None,
    }
}

/// The sole JSX element child of a fragment, ignoring pure-whitespace text.
/// `None` when there is more than one element child or any non-whitespace,
/// non-element child (text, expression container, nested fragment).
fn single_element_child<'a>(children: &'a [JSXChild<'a>]) -> Option<&'a JSXElement<'a>> {
    let mut found: Option<&'a JSXElement<'a>> = None;
    for child in children {
        match child {
            JSXChild::Text(text) if text.value.trim().is_empty() => {}
            JSXChild::Element(element) => {
                if found.is_some() {
                    return None;
                }
                found = Some(element);
            }
            _ => return None,
        }
    }
    found
}

/// Whether a JSX element is `<Child {...props_root}/>`: a capitalized/member
/// component tag (not a host element), with at least one spread whose root is
/// `props_root`, NO named attributes alongside the spread, and NO non-whitespace
/// children. The forwarded element must NOT be the wrapper's own tag (a
/// self-render guard handled by the analyzer too, but a `<Self {...props}/>`
/// element here would be infinite recursion, never a thin wrapper).
fn jsx_element_is_bare_props_spread(element: &JSXElement<'_>, props_root: &str) -> bool {
    let opening = &element.opening_element;
    // Must render a component, not a host element (`<div>`).
    if jsx_component_tag_name(&opening.name).is_none() {
        return false;
    }
    // No non-whitespace children: a single forwarded element has none.
    if !element.children.iter().all(jsx_child_is_whitespace) {
        return false;
    }
    let mut has_props_spread = false;
    for item in &opening.attributes {
        match item {
            JSXAttributeItem::SpreadAttribute(spread) => {
                match spread_root_identifier(&spread.argument) {
                    // A spread of the props binding/rest local: the forward.
                    Some(root) if root == props_root => has_props_spread = true,
                    // A spread of a DIFFERENT object (`{...someOther}`) forwards a
                    // different object; not a pure props passthrough. Abstain.
                    _ => return false,
                }
            }
            // ANY named attribute alongside the spread is a fixed configuration
            // (`variant="primary"`, a forwarded `ref={...}`): intentional, not a
            // pure passthrough.
            JSXAttributeItem::Attribute(_) => return false,
        }
    }
    has_props_spread
}

/// Whether a JSX child is pure whitespace text (the only child a bare
/// `<Child {...props}/>` element may carry, e.g. source-formatting newlines on a
/// non-self-closing `<Child {...props}></Child>`).
fn jsx_child_is_whitespace(child: &JSXChild<'_>) -> bool {
    matches!(child, JSXChild::Text(text) if text.value.trim().is_empty())
}

/// The root identifier of a JSX spread argument (`{...props}` -> `props`,
/// `{...props.rest}` -> `props`). `None` for a non-identifier-rooted spread
/// (`{...getProps()}`, `{...{a:1}}`).
fn spread_root_identifier<'a>(expr: &'a Expression<'a>) -> Option<&'a str> {
    match expr {
        Expression::Identifier(ident) => Some(ident.name.as_str()),
        Expression::StaticMemberExpression(member) => spread_root_identifier(&member.object),
        Expression::ComputedMemberExpression(member) => spread_root_identifier(&member.object),
        Expression::ParenthesizedExpression(paren) => spread_root_identifier(&paren.expression),
        _ => None,
    }
}

/// The local binding name a destructured prop value binds to (the alias for
/// `{ name: alias }`, or the name itself). `None` for shapes we do not flatten.
fn binding_pattern_local_name(pattern: &BindingPattern<'_>) -> Option<String> {
    match pattern {
        BindingPattern::BindingIdentifier(id) => Some(id.name.to_string()),
        BindingPattern::AssignmentPattern(assign) => binding_pattern_local_name(&assign.left),
        // Nested destructure (`{ user: { name } }`) is not flattened in v1.
        _ => None,
    }
}

/// Classify a hook callee name into a [`HookUseKind`]. Returns `None` for a
/// non-hook (anything not matching the `use*` convention).
fn hook_kind(name: &str) -> Option<HookUseKind> {
    match name {
        "useState" => Some(HookUseKind::UseState),
        "useEffect" => Some(HookUseKind::UseEffect),
        "useMemo" => Some(HookUseKind::UseMemo),
        "useCallback" => Some(HookUseKind::UseCallback),
        _ if is_custom_hook_name(name) => Some(HookUseKind::Custom),
        _ => None,
    }
}

/// React convention: a hook is named `use` followed by an uppercase letter
/// (`useFoo`), so a plain `use` or `used` is not a hook.
fn is_custom_hook_name(name: &str) -> bool {
    name.strip_prefix("use")
        .and_then(|rest| rest.chars().next())
        .is_some_and(char::is_uppercase)
}

/// The dependency-array arity for a hook, recorded ONLY when a literal array is
/// present at the dependency-array position. `useEffect` / `useCallback` /
/// `useMemo` take the deps array as the SECOND argument; other hooks have none.
/// Returns `None` when the position has no literal array (ADR-001: do not guess).
fn hook_dep_array_arity(kind: HookUseKind, args: &[Argument<'_>]) -> Option<u32> {
    let dep_index = match kind {
        HookUseKind::UseEffect | HookUseKind::UseMemo | HookUseKind::UseCallback => 1,
        // useState has no dependency array; a custom hook's arg shape is unknown.
        HookUseKind::UseState | HookUseKind::Custom => return None,
    };
    let arg = args.get(dep_index)?.as_expression()?;
    let Expression::ArrayExpression(array) = arg.get_inner_expression() else {
        return None;
    };
    u32::try_from(array.elements.len()).ok()
}
