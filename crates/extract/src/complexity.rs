//! Cyclomatic and cognitive complexity computation via Oxc AST visitor.
//!
//! Computes both metrics in a single AST traversal using a function scope stack.
//! Each function/method/arrow gets its own independent complexity frame.
//!
//! **Cyclomatic complexity** (McCabe): 1 + number of decision points per function.
//! Counts `if`, `for`, `while`, `do`, `case`, `catch`, `?:`, `&&`, `||`, `??`,
//! `&&=`/`||=`/`??=`, and `?.`.
//!
//! **Cognitive complexity** (SonarSource): structural increments with nesting penalty.
//! Counts control flow breaks weighted by nesting depth. Boolean operator sequences
//! add +1 per operator kind change. Optional chaining (`?.`) is NOT counted (Principle 3).
//!
//! **React context**: JSX max depth is tracked as descriptive render-layout
//! context and does not alter cognitive complexity. React hook density and wide
//! prop interfaces still fold into the EXISTING cognitive metric rather than
//! minting new tunable rules. These fold as `HookDensity` / `PropCount`
//! contributions and the raw counts surface as descriptive hotspot context. No
//! new threshold knob: a god component surfaces through `max_cognitive` /
//! `max_crap` like any other complex function.

/// Prop count at or below which no cognitive penalty accrues. A handful of props
/// is normal; a wide prop interface (the god-component shape) folds its excess
/// into cognitive. Only props beyond this floor are counted.
const PROP_COUNT_FLOOR: u16 = 4;

/// Arguments that take the span name of `Effect.fn("span")(...)`: only the
/// body. The later arguments are pipeline transforms.
const SPAN_NAMED_ARGUMENTS: std::ops::Range<usize> = 0..1;

/// Arguments that take the name of a call with a string-literal first
/// argument: every argument after that literal.
const STRING_KEYED_ARGUMENTS: std::ops::RangeFrom<usize> = 1..;

#[allow(clippy::wildcard_imports, reason = "many AST types used")]
use oxc_ast::ast::*;
use oxc_ast_visit::Visit;
use oxc_ast_visit::walk;
use oxc_semantic::ScopeFlags;
use oxc_span::GetSpan;
use oxc_span::Span;

use fallow_types::extract::{
    ANONYMOUS_ARROW_NAME, ANONYMOUS_FUNCTION_NAME, AnonymousFunctionKind, ComplexityContribution,
    ComplexityContributionKind, ComplexityMetric, FunctionComplexity,
};

use crate::{complexity_names, js_text::JsText};

/// Per-function state on the scope stack.
struct FunctionFrame {
    identity: FunctionIdentity,
    span: Span,
    cyclomatic: u16,
    cognitive: u16,
    nesting_level: u16,
    /// Track the last logical operator for cognitive boolean sequence detection.
    last_logical_operator: Option<LogicalOperator>,
    /// Number of parameters (excluding TypeScript's `this` parameter).
    param_count: u8,
    /// Current JSX element nesting depth: how many JSX elements/fragments are
    /// open above the cursor in this frame's own body. Reset per frame so a
    /// nested render-prop arrow starts its own depth.
    jsx_depth: u16,
    /// Deepest JSX nesting reached in this frame (descriptive context).
    jsx_max_depth: u16,
    /// Count of React hook calls made directly in this frame's body.
    hook_count: u16,
    /// Number of props destructured from this function's first parameter, if it
    /// is a flat object pattern (`{ a, b, c }`). `0` for a bare `props`
    /// identifier or no parameter. Folded into cognitive only once the frame is
    /// known to render JSX (a component), at pop time.
    prop_count: u16,
    /// Per-increment breakdown accumulated as the function body is walked.
    contributions: Vec<ComplexityContribution>,
    /// Levels of nested functions below this one (`function_hash`).
    height: u32,
}

#[derive(Clone)]
struct FunctionIdentity {
    name: String,
    is_private_member: bool,
    /// The name comes from the call or the function that holds this function
    /// (see `FunctionComplexity::context_named`). The kind starts as
    /// `Function` and an arrow sets `Arrow` when it takes the identity.
    context_named: Option<AnonymousFunctionKind>,
}

impl FunctionIdentity {
    fn public(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            is_private_member: false,
            context_named: None,
        }
    }

    fn private(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            is_private_member: true,
            context_named: None,
        }
    }

    fn from_context(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            is_private_member: false,
            context_named: Some(AnonymousFunctionKind::Function),
        }
    }
}

/// AST visitor that computes per-function complexity metrics.
pub struct ComplexityVisitor<'a> {
    stack: Vec<FunctionFrame>,
    pub results: Vec<FunctionComplexity>,
    /// Line offsets for byte-offset to line/col conversion.
    line_offsets: &'a [u32],
    /// Source text the AST was parsed from. Used to compute each function's
    /// content digest (`source_hash`) from its full-span byte slice.
    source: &'a str,
    /// Identity override from a parent node (e.g., method identity from
    /// `MethodDefinition`, variable name from `const foo = function() {}`).
    pending_identity: Option<FunctionIdentity>,
    /// A call whose anonymous generator arguments take a context name, keyed
    /// by the call span: the `Effect.gen(...)` call that a named function
    /// returns, or one passed to a string-keyed call.
    named_generator_call: Option<(Span, FunctionIdentity)>,
    /// Span of the outermost `||` chain under the cursor that is a literal
    /// membership chain. Its cyclomatic `||` increments carry the
    /// `literal_membership` flag.
    literal_membership_chain: Option<Span>,
    /// Spans of `||` expressions that are direct operands of an enclosing
    /// `||` expression. Such an expression is part of a longer chain, so it
    /// cannot open a literal membership chain of its own.
    nested_or_operands: Vec<Span>,
}

impl<'a> ComplexityVisitor<'a> {
    pub const fn new(source: &'a str, line_offsets: &'a [u32]) -> Self {
        Self {
            stack: Vec::new(),
            results: Vec::new(),
            line_offsets,
            source,
            pending_identity: None,
            named_generator_call: None,
            literal_membership_chain: None,
            nested_or_operands: Vec::new(),
        }
    }

    fn push_function(
        &mut self,
        identity: FunctionIdentity,
        span: Span,
        param_count: u8,
        prop_count: u16,
    ) {
        self.stack.push(FunctionFrame {
            identity,
            span,
            cyclomatic: 1,
            cognitive: 0,
            nesting_level: 0,
            last_logical_operator: None,
            param_count,
            jsx_depth: 0,
            jsx_max_depth: 0,
            hook_count: 0,
            prop_count,
            contributions: Vec::new(),
            height: 0,
        });
    }

    fn pop_function(&mut self) {
        // Fold the component prop count into cognitive before popping, so the
        // contribution lands on the still-current frame (a prop fold only fires
        // for a JSX-rendering frame, i.e. a real component).
        self.fold_component_prop_count();
        if let Some(frame) = self.stack.pop() {
            let (line, col) =
                fallow_types::extract::byte_offset_to_line_col(self.line_offsets, frame.span.start);
            let end_line =
                fallow_types::extract::byte_offset_to_line_col(self.line_offsets, frame.span.end).0;
            let source_hash =
                crate::function_hash::function_source_hash(self.source, frame.span, frame.height);
            if let Some(parent) = self.stack.last_mut() {
                parent.height = parent.height.max(frame.height + 1);
            }
            self.results.push(FunctionComplexity {
                name: frame.identity.name,
                is_private_member: frame.identity.is_private_member,
                context_named: frame.identity.context_named,
                line,
                col,
                cyclomatic: frame.cyclomatic,
                cognitive: frame.cognitive,
                line_count: end_line.saturating_sub(line) + 1,
                param_count: frame.param_count,
                react_hook_count: frame.hook_count,
                react_jsx_max_depth: frame.jsx_max_depth,
                react_prop_count: frame.prop_count,
                source_hash,
                contributions: frame.contributions,
            });
        }
    }

    /// Pop the root module frame and emit it as a synthetic `<module>` unit.
    ///
    /// Separate from [`Self::pop_function`] on three points:
    ///
    /// 1. No `fold_component_prop_count`. That fold is a component signal and
    ///    the root frame is not a function, let alone a component.
    /// 2. The unit is dropped when it never branched (`cyclomatic == 1 &&
    ///    cognitive == 0`), mirroring the template unit's emission gate. A file
    ///    with no module-scope decision point gets no new unit, so its existing
    ///    numbers are untouched.
    /// 3. It is anchored and sized by its own contributions rather than by the
    ///    program span, matching the template unit's anchor convention. The
    ///    anchor points at real branching instead of at line 1, and the size is
    ///    the distance between the first and last decision point rather than
    ///    the file's line count, which would otherwise push nearly every file
    ///    into the very-high-risk unit-size bin.
    ///
    /// `source_hash` is `None` like the template unit's: a whole-file slice
    /// joins nothing in the Istanbul or runtime-coverage function maps.
    fn pop_module_frame(&mut self) {
        let Some(frame) = self.stack.pop() else {
            return;
        };
        if frame.cyclomatic == 1 && frame.cognitive == 0 {
            return;
        }
        let Some((line, col)) = frame
            .contributions
            .iter()
            .map(|contribution| (contribution.line, contribution.col))
            .min()
        else {
            return;
        };
        let end_line = frame
            .contributions
            .iter()
            .map(|contribution| contribution.line)
            .max()
            .unwrap_or(line);
        self.results.push(FunctionComplexity {
            name: fallow_types::extract::MODULE_UNIT_NAME.to_string(),
            is_private_member: false,
            context_named: None,
            line,
            col,
            cyclomatic: frame.cyclomatic,
            cognitive: frame.cognitive,
            line_count: end_line.saturating_sub(line) + 1,
            param_count: 0,
            react_hook_count: 0,
            react_jsx_max_depth: 0,
            react_prop_count: 0,
            source_hash: None,
            contributions: frame.contributions,
        });
    }

    /// Fold a component's prop count past [`PROP_COUNT_FLOOR`] into cognitive,
    /// recorded as a single `PropCount` contribution anchored at the function
    /// span. Fires only when the frame rendered JSX (`jsx_max_depth > 0`), so a
    /// plain function destructuring an options bag is never penalized. The fold
    /// is one increment carrying the excess as its weight (a 14-prop component
    /// with floor 4 adds `+10`), keeping the breakdown reconstructable.
    fn fold_component_prop_count(&mut self) {
        let Some(frame) = self.stack.last() else {
            return;
        };
        if frame.jsx_max_depth == 0 || frame.prop_count <= PROP_COUNT_FLOOR {
            return;
        }
        let excess = frame.prop_count - PROP_COUNT_FLOOR;
        let span = frame.span;
        self.push_contribution(
            span,
            ComplexityMetric::Cognitive,
            ComplexityContributionKind::PropCount,
            excess,
            0,
        );
        if let Some(frame) = self.stack.last_mut() {
            frame.cognitive = frame.cognitive.saturating_add(excess);
        }
    }

    /// Record one increment event at `span` and return nothing; the caller is
    /// responsible for applying the matching `+weight` to the counter so the
    /// recorded breakdown can never drift from the aggregate metric.
    fn push_contribution(
        &mut self,
        span: Span,
        metric: ComplexityMetric,
        kind: ComplexityContributionKind,
        weight: u16,
        nesting: u16,
    ) {
        let (line, col) =
            fallow_types::extract::byte_offset_to_line_col(self.line_offsets, span.start);
        if let Some(frame) = self.stack.last_mut() {
            frame.contributions.push(ComplexityContribution {
                line,
                col,
                metric,
                kind,
                weight,
                nesting,
                literal_membership: false,
            });
        }
    }

    /// Increment cyclomatic complexity for the current function and record the
    /// contributing construct. Cyclomatic increments are flat `+1`.
    fn inc_cyclomatic(&mut self, span: Span, kind: ComplexityContributionKind) {
        self.push_contribution(span, ComplexityMetric::Cyclomatic, kind, 1, 0);
        if let Some(frame) = self.stack.last_mut() {
            frame.cyclomatic = frame.cyclomatic.saturating_add(1);
        }
    }

    /// Increment cyclomatic complexity for one `||` operator of a literal
    /// membership chain. The count is the same as [`Self::inc_cyclomatic`];
    /// only the recorded contribution carries the `literal_membership` flag.
    fn inc_cyclomatic_literal_membership(&mut self, span: Span) {
        self.inc_cyclomatic(span, ComplexityContributionKind::LogicalOr);
        if let Some(contribution) = self
            .stack
            .last_mut()
            .and_then(|frame| frame.contributions.last_mut())
        {
            contribution.literal_membership = true;
        }
    }

    /// Increment cognitive complexity: +1 structural + nesting penalty, and
    /// record the contribution with `weight == 1 + nesting`.
    fn inc_cognitive_with_nesting(&mut self, span: Span, kind: ComplexityContributionKind) {
        let nesting = self.stack.last().map_or(0, |frame| frame.nesting_level);
        let weight = 1 + nesting;
        self.push_contribution(span, ComplexityMetric::Cognitive, kind, weight, nesting);
        if let Some(frame) = self.stack.last_mut() {
            frame.cognitive = frame.cognitive.saturating_add(weight);
        }
    }

    /// Increment cognitive complexity: flat +1 (no nesting penalty), and record
    /// the contribution.
    fn inc_cognitive_flat(&mut self, span: Span, kind: ComplexityContributionKind) {
        self.push_contribution(span, ComplexityMetric::Cognitive, kind, 1, 0);
        if let Some(frame) = self.stack.last_mut() {
            frame.cognitive = frame.cognitive.saturating_add(1);
        }
    }

    /// Open a JSX element/fragment: bump the frame's JSX depth and track the max
    /// for descriptive output only. Markup nesting is layout shape, not control
    /// flow, so it does not add cognitive weight or deepen structural nesting.
    fn open_jsx(&mut self) {
        let new_depth = self.stack.last().map_or(0, |frame| frame.jsx_depth) + 1;
        if let Some(frame) = self.stack.last_mut() {
            frame.jsx_depth = new_depth;
            frame.jsx_max_depth = frame.jsx_max_depth.max(new_depth);
        }
    }

    /// Close a JSX element/fragment opened by [`Self::open_jsx`].
    fn close_jsx(&mut self) {
        if let Some(frame) = self.stack.last_mut() {
            frame.jsx_depth = frame.jsx_depth.saturating_sub(1);
        }
    }

    /// Record one React hook call in the current frame: bump the descriptive hook
    /// count and, when the frame is React-shaped (a component or custom hook by
    /// naming convention), fold a flat cognitive increment (recorded as
    /// `HookDensity`). A hook-heavy component accrues cognitive load the same way
    /// branching does. Gating on the name convention keeps a `use*` call inside an
    /// ordinary non-React function from accruing the penalty (zero-FP posture).
    fn record_hook(&mut self, span: Span) {
        let react_shaped = self.stack.last().is_some_and(|frame| {
            frame.identity.context_named.is_none()
                && frame_name_is_react_shaped(&frame.identity.name)
        });
        if let Some(frame) = self.stack.last_mut() {
            frame.hook_count = frame.hook_count.saturating_add(1);
        }
        if react_shaped {
            self.inc_cognitive_flat(span, ComplexityContributionKind::HookDensity);
        }
    }

    /// Count function parameters, excluding TypeScript's `this` parameter.
    #[expect(
        clippy::cast_possible_truncation,
        reason = "functions with >255 params are unrealistic"
    )]
    fn count_params(params: &FormalParameters<'_>) -> u8 {
        let mut count = params
            .items
            .iter()
            .filter(|p| {
                !matches!(&p.pattern, BindingPattern::BindingIdentifier(id) if id.name == "this")
            })
            .count();
        if params.rest.is_some() {
            count += 1;
        }
        count as u8
    }

    /// Count the props destructured from a function's first parameter, when it
    /// is a flat object pattern (`{ a, b, c }`, with or without a type
    /// annotation), the React component-props shape. A bare `props` identifier,
    /// a rest element, or no first parameter yields `0` (not statically
    /// countable, or not a props-destructuring component). Mirrors the
    /// statically-harvestable prop shape the React extractor uses, but counts
    /// only (it does not need to resolve names).
    #[expect(
        clippy::cast_possible_truncation,
        reason = "components with >65535 destructured props are impossible"
    )]
    fn count_props(params: &FormalParameters<'_>) -> u16 {
        let Some(first) = params.items.first() else {
            return 0;
        };
        let BindingPattern::ObjectPattern(obj) = &first.pattern else {
            return 0;
        };
        obj.properties.len().min(u16::MAX as usize) as u16
    }

    /// Increase nesting level for the current function.
    fn inc_nesting(&mut self) {
        if let Some(frame) = self.stack.last_mut() {
            frame.nesting_level = frame.nesting_level.saturating_add(1);
        }
    }

    /// Decrease nesting level for the current function.
    fn dec_nesting(&mut self) {
        if let Some(frame) = self.stack.last_mut() {
            frame.nesting_level = frame.nesting_level.saturating_sub(1);
        }
    }

    /// Handle a logical expression for cognitive complexity.
    /// Sequences of the same operator get +1 total; each operator change adds +1.
    /// `span` anchors the recorded contribution to the same node as the
    /// cyclomatic sibling so consumers can group both by line.
    fn handle_logical_operator(&mut self, op: LogicalOperator, span: Span) {
        let changed = match self
            .stack
            .last()
            .and_then(|frame| frame.last_logical_operator)
        {
            Some(prev) => prev != op,
            None => true,
        };
        if changed {
            self.push_contribution(span, ComplexityMetric::Cognitive, logical_kind(op), 1, 0);
            if let Some(frame) = self.stack.last_mut() {
                frame.cognitive = frame.cognitive.saturating_add(1);
                frame.last_logical_operator = Some(op);
            }
        }
    }

    /// Reset the logical operator tracking (end of a logical expression chain).
    fn reset_logical_operator(&mut self) {
        if let Some(frame) = self.stack.last_mut() {
            frame.last_logical_operator = None;
        }
    }

    /// Check if a node is the direct child of a `LogicalExpression`.
    /// Used to avoid resetting the logical operator tracker in the middle of a chain.
    const fn is_nested_logical(expr: &Expression<'_>) -> bool {
        matches!(expr, Expression::LogicalExpression(_))
    }

    /// Mark the generator call that the current function returns, so that the
    /// generator takes the name of that function. In
    /// `const load = () => Effect.gen(function* () {...})` the generator is the
    /// real body of `load`. A function with a placeholder name passes nothing.
    fn name_returned_generator(&mut self, returned: &Expression<'_>) {
        let Some(frame) = self.stack.last() else {
            return;
        };
        if complexity_names::is_placeholder_name(&frame.identity.name) {
            return;
        }
        if let Some(call) = complexity_names::returned_generator_call(returned) {
            let identity = FunctionIdentity::from_context(frame.identity.name.clone());
            self.named_generator_call = Some((call.span, identity));
        }
    }

    fn take_named_generator_call(&mut self, span: Span) -> Option<FunctionIdentity> {
        if self
            .named_generator_call
            .as_ref()
            .is_some_and(|(call_span, _)| *call_span == span)
        {
            return self
                .named_generator_call
                .take()
                .map(|(_, identity)| identity);
        }
        None
    }

    /// Walk a call and give `identity` to each anonymous generator argument.
    fn visit_call_with_named_generators(
        &mut self,
        call: &CallExpression<'_>,
        identity: &FunctionIdentity,
    ) {
        self.visit_expression(&call.callee);
        for argument in &call.arguments {
            let offered = self.pending_identity.is_none()
                && complexity_names::is_anonymous_generator(argument);
            if offered {
                self.pending_identity = Some(identity.clone());
            }
            self.visit_argument(argument);
            if offered {
                self.pending_identity = None;
            }
        }
    }

    /// Walk a call and offer `identity` to the arguments in `named`. A
    /// function argument takes it directly. A call argument passes it to its
    /// anonymous generator (`.handle("get", Effect.fn(function* ...))`), and
    /// to nothing else, so `it("x", wrap(() => ...))` keeps `<arrow>`.
    ///
    /// A binding or key name that waits for the first function keeps
    /// priority: the offer happens only when no such name is pending, so only
    /// a function that would get a placeholder takes the context name. In
    /// `const setup = Command.make("setup", {...}, cb)` the object argument
    /// consumes the binding name, so `cb` takes `Command.make("setup")`.
    fn visit_call_with_context_name(
        &mut self,
        call: &CallExpression<'_>,
        named: &impl std::ops::RangeBounds<usize>,
        identity: &FunctionIdentity,
    ) {
        self.visit_expression(&call.callee);
        for (index, argument) in call.arguments.iter().enumerate() {
            let offered = named.contains(&index)
                && self.pending_identity.is_none()
                && self.offer_context_name(argument, identity);
            self.visit_argument(argument);
            if offered {
                self.pending_identity = None;
                self.named_generator_call = None;
            }
        }
    }

    /// Offer `identity` to one argument. Returns whether the argument can take
    /// it.
    fn offer_context_name(&mut self, argument: &Argument<'_>, identity: &FunctionIdentity) -> bool {
        if matches!(
            argument,
            Argument::FunctionExpression(_) | Argument::ArrowFunctionExpression(_)
        ) {
            self.pending_identity = Some(identity.clone());
            return true;
        }
        let Some(call) = argument
            .as_expression()
            .and_then(complexity_names::returned_generator_call)
        else {
            return false;
        };
        self.named_generator_call = Some((call.span, identity.clone()));
        true
    }

    /// Walk an `if` (or an `else if` continuation) computing both metrics.
    ///
    /// `is_else_if` is `true` when the statement is the `alternate` of a parent
    /// `if`. An `else if` adds a flat `+1` to cognitive complexity (SonarSource
    /// treats it as a same-level continuation, no nesting penalty), replacing
    /// the previous "+1+nesting then subtract nesting" arithmetic. This keeps
    /// the recorded contribution honest (the else-if is genuinely `+1`) while
    /// producing byte-identical cyclomatic and cognitive totals.
    fn visit_if_chain(&mut self, stmt: &IfStatement<'_>, is_else_if: bool) {
        let condition_kind = if is_else_if {
            ComplexityContributionKind::ElseIf
        } else {
            ComplexityContributionKind::If
        };
        self.inc_cyclomatic(stmt.span, condition_kind);
        if is_else_if {
            self.inc_cognitive_flat(stmt.span, ComplexityContributionKind::ElseIf);
        } else {
            self.inc_cognitive_with_nesting(stmt.span, ComplexityContributionKind::If);
        }

        self.visit_expression(&stmt.test);

        self.inc_nesting();
        self.visit_statement(&stmt.consequent);
        self.dec_nesting();

        if let Some(alternate) = &stmt.alternate {
            match alternate {
                Statement::IfStatement(else_if) => {
                    self.visit_if_chain(else_if, true);
                }
                _ => {
                    self.inc_cognitive_flat(alternate.span(), ComplexityContributionKind::Else);
                    self.inc_nesting();
                    self.visit_statement(alternate);
                    self.dec_nesting();
                }
            }
        }
    }
}

/// Map a logical operator to its [`ComplexityContributionKind`].
const fn logical_kind(op: LogicalOperator) -> ComplexityContributionKind {
    match op {
        LogicalOperator::And => ComplexityContributionKind::LogicalAnd,
        LogicalOperator::Or => ComplexityContributionKind::LogicalOr,
        LogicalOperator::Coalesce => ComplexityContributionKind::NullishCoalescing,
    }
}

impl<'ast> Visit<'ast> for ComplexityVisitor<'_> {
    /// Open a root frame for module scope before walking the program.
    ///
    /// Every counter writer targets `self.stack.last_mut()` and silently drops
    /// its increment when the stack is empty, so without this frame a top-level
    /// `if` ladder, `??` default, or `?.` access contributed to no metric at
    /// all. The frame is closed by [`ComplexityVisitor::pop_module_frame`],
    /// which drops it again when nothing branched.
    fn visit_program(&mut self, program: &Program<'ast>) {
        self.push_function(
            FunctionIdentity::public(fallow_types::extract::MODULE_UNIT_NAME),
            program.span,
            0,
            0,
        );
        walk::walk_program(self, program);
        self.pop_module_frame();
    }

    fn visit_function(&mut self, func: &Function<'ast>, flags: ScopeFlags) {
        if func.body.is_none() {
            walk::walk_function(self, func, flags);
            return;
        }
        let identity = func
            .id
            .as_ref()
            .map(|id| {
                self.pending_identity.take();
                FunctionIdentity::public(id.name.to_string())
            })
            .or_else(|| self.pending_identity.take())
            .unwrap_or_else(|| FunctionIdentity::public(ANONYMOUS_FUNCTION_NAME));

        let is_nested = !self.stack.is_empty();
        if is_nested {
            self.inc_nesting();
        }

        let param_count = Self::count_params(&func.params);
        let prop_count = Self::count_props(&func.params);
        self.push_function(identity, func.span, param_count, prop_count);
        walk::walk_function(self, func, flags);
        self.pop_function();

        if is_nested {
            self.dec_nesting();
        }
    }

    fn visit_arrow_function_expression(&mut self, arrow: &ArrowFunctionExpression<'ast>) {
        let identity = self.pending_identity.take().map_or_else(
            || FunctionIdentity::public(ANONYMOUS_ARROW_NAME),
            |mut identity| {
                if identity.context_named.is_some() {
                    identity.context_named = Some(AnonymousFunctionKind::Arrow);
                }
                identity
            },
        );

        let is_nested = !self.stack.is_empty();
        if is_nested {
            self.inc_nesting();
        }

        let param_count = Self::count_params(&arrow.params);
        let prop_count = Self::count_props(&arrow.params);
        self.push_function(identity, arrow.span, param_count, prop_count);
        if let Some(body) = arrow.get_expression() {
            self.name_returned_generator(body);
        }
        walk::walk_arrow_function_expression(self, arrow);
        self.named_generator_call = None;
        self.pop_function();

        if is_nested {
            self.dec_nesting();
        }
    }

    fn visit_method_definition(&mut self, method: &MethodDefinition<'ast>) {
        // `static_name` has no answer for a private key, so `#work` would fall
        // through to `<anonymous>` and read as an unnamed unit in every report.
        if let Some(name) = method.key.static_name() {
            self.pending_identity = Some(FunctionIdentity::public(name.text().into_owned()));
        } else if let PropertyKey::PrivateIdentifier(private) = &method.key {
            self.pending_identity = Some(FunctionIdentity::private(format!("#{}", private.name)));
        }
        walk::walk_method_definition(self, method);
        self.pending_identity = None;
    }

    fn visit_variable_declarator(&mut self, decl: &VariableDeclarator<'ast>) {
        if let Some(id) = decl.id.get_binding_identifier() {
            self.pending_identity = Some(FunctionIdentity::public(id.name.to_string()));
        }
        walk::walk_variable_declarator(self, decl);
        self.pending_identity = None;
    }

    fn visit_property_definition(&mut self, prop: &PropertyDefinition<'ast>) {
        if let Some(name) = prop.key.static_name() {
            self.pending_identity = Some(FunctionIdentity::public(name.text().into_owned()));
        } else if let PropertyKey::PrivateIdentifier(private) = &prop.key {
            self.pending_identity = Some(FunctionIdentity::private(format!("#{}", private.name)));
        }
        walk::walk_property_definition(self, prop);
        self.pending_identity = None;
    }

    fn visit_object_property(&mut self, prop: &ObjectProperty<'ast>) {
        if let Some(name) = prop.key.static_name() {
            self.pending_identity = Some(FunctionIdentity::public(name.text().into_owned()));
        }
        walk::walk_object_property(self, prop);
        self.pending_identity = None;
    }

    fn visit_export_default_declaration(&mut self, decl: &ExportDefaultDeclaration<'ast>) {
        self.pending_identity = Some(FunctionIdentity::public("default"));
        walk::walk_export_default_declaration(self, decl);
        self.pending_identity = None;
    }

    fn visit_if_statement(&mut self, stmt: &IfStatement<'ast>) {
        self.visit_if_chain(stmt, false);
    }

    fn visit_for_statement(&mut self, stmt: &ForStatement<'ast>) {
        self.inc_cyclomatic(stmt.span, ComplexityContributionKind::For);
        self.inc_cognitive_with_nesting(stmt.span, ComplexityContributionKind::For);
        if let Some(init) = &stmt.init {
            self.visit_for_statement_init(init);
        }
        if let Some(test) = &stmt.test {
            self.visit_expression(test);
        }
        if let Some(update) = &stmt.update {
            self.visit_expression(update);
        }
        self.inc_nesting();
        self.visit_statement(&stmt.body);
        self.dec_nesting();
    }

    fn visit_for_in_statement(&mut self, stmt: &ForInStatement<'ast>) {
        self.inc_cyclomatic(stmt.span, ComplexityContributionKind::ForIn);
        self.inc_cognitive_with_nesting(stmt.span, ComplexityContributionKind::ForIn);
        self.visit_for_statement_left(&stmt.left);
        self.visit_expression(&stmt.right);
        self.inc_nesting();
        self.visit_statement(&stmt.body);
        self.dec_nesting();
    }

    fn visit_for_of_statement(&mut self, stmt: &ForOfStatement<'ast>) {
        self.inc_cyclomatic(stmt.span, ComplexityContributionKind::ForOf);
        self.inc_cognitive_with_nesting(stmt.span, ComplexityContributionKind::ForOf);
        self.visit_for_statement_left(&stmt.left);
        self.visit_expression(&stmt.right);
        self.inc_nesting();
        self.visit_statement(&stmt.body);
        self.dec_nesting();
    }

    fn visit_while_statement(&mut self, stmt: &WhileStatement<'ast>) {
        self.inc_cyclomatic(stmt.span, ComplexityContributionKind::While);
        self.inc_cognitive_with_nesting(stmt.span, ComplexityContributionKind::While);
        self.visit_expression(&stmt.test);
        self.inc_nesting();
        self.visit_statement(&stmt.body);
        self.dec_nesting();
    }

    fn visit_do_while_statement(&mut self, stmt: &DoWhileStatement<'ast>) {
        self.inc_cyclomatic(stmt.span, ComplexityContributionKind::DoWhile);
        self.inc_cognitive_with_nesting(stmt.span, ComplexityContributionKind::DoWhile);
        self.inc_nesting();
        self.visit_statement(&stmt.body);
        self.dec_nesting();
        self.visit_expression(&stmt.test);
    }

    fn visit_switch_statement(&mut self, stmt: &SwitchStatement<'ast>) {
        self.inc_cognitive_with_nesting(stmt.span, ComplexityContributionKind::Switch);
        self.visit_expression(&stmt.discriminant);
        self.inc_nesting();
        for case in &stmt.cases {
            self.visit_switch_case(case);
        }
        self.dec_nesting();
    }

    fn visit_switch_case(&mut self, case: &SwitchCase<'ast>) {
        if case.test.is_some() {
            self.inc_cyclomatic(case.span, ComplexityContributionKind::Case);
        }
        walk::walk_switch_case(self, case);
    }

    fn visit_catch_clause(&mut self, clause: &CatchClause<'ast>) {
        self.inc_cyclomatic(clause.span, ComplexityContributionKind::Catch);
        self.inc_cognitive_with_nesting(clause.span, ComplexityContributionKind::Catch);
        self.inc_nesting();
        walk::walk_catch_clause(self, clause);
        self.dec_nesting();
    }

    fn visit_conditional_expression(&mut self, expr: &ConditionalExpression<'ast>) {
        self.inc_cyclomatic(expr.span, ComplexityContributionKind::Ternary);
        self.inc_cognitive_with_nesting(expr.span, ComplexityContributionKind::Ternary);
        self.visit_expression(&expr.test);
        self.inc_nesting();
        self.visit_expression(&expr.consequent);
        self.visit_expression(&expr.alternate);
        self.dec_nesting();
    }

    fn visit_logical_expression(&mut self, expr: &LogicalExpression<'ast>) {
        let chain_root = !self.nested_or_operands.contains(&expr.span);
        let opens_chain = chain_root
            && self.literal_membership_chain.is_none()
            && crate::literal_membership::is_literal_membership_chain(expr);
        if opens_chain {
            self.literal_membership_chain = Some(expr.span);
        }
        let nested_operands_before = self.nested_or_operands.len();
        if expr.operator == LogicalOperator::Or {
            for operand in [&expr.left, &expr.right] {
                if let Expression::LogicalExpression(inner) = operand.without_parentheses()
                    && inner.operator == LogicalOperator::Or
                {
                    self.nested_or_operands.push(inner.span);
                }
            }
        }
        if self.literal_membership_chain.is_some() && expr.operator == LogicalOperator::Or {
            self.inc_cyclomatic_literal_membership(expr.span);
        } else {
            self.inc_cyclomatic(expr.span, logical_kind(expr.operator));
        }

        self.handle_logical_operator(expr.operator, expr.span);

        self.visit_expression(&expr.left);

        self.visit_expression(&expr.right);

        if !Self::is_nested_logical(&expr.right) && !Self::is_nested_logical(&expr.left) {
            self.reset_logical_operator();
        }

        self.nested_or_operands.truncate(nested_operands_before);
        if opens_chain {
            self.literal_membership_chain = None;
        }
    }

    fn visit_assignment_expression(&mut self, expr: &AssignmentExpression<'ast>) {
        if matches!(
            expr.operator,
            AssignmentOperator::LogicalAnd
                | AssignmentOperator::LogicalOr
                | AssignmentOperator::LogicalNullish
        ) {
            self.inc_cyclomatic(expr.span, ComplexityContributionKind::LogicalAssignment);
        }
        walk::walk_assignment_expression(self, expr);
    }

    fn visit_chain_expression(&mut self, expr: &ChainExpression<'ast>) {
        match &expr.expression {
            ChainElement::CallExpression(call) => {
                if call.optional {
                    self.inc_cyclomatic(call.span, ComplexityContributionKind::OptionalChain);
                }
            }
            ChainElement::StaticMemberExpression(member) => {
                if member.optional {
                    self.inc_cyclomatic(member.span, ComplexityContributionKind::OptionalChain);
                }
            }
            ChainElement::ComputedMemberExpression(member) => {
                if member.optional {
                    self.inc_cyclomatic(member.span, ComplexityContributionKind::OptionalChain);
                }
            }
            ChainElement::PrivateFieldExpression(field) => {
                if field.optional {
                    self.inc_cyclomatic(field.span, ComplexityContributionKind::OptionalChain);
                }
            }
            ChainElement::TSNonNullExpression(_) => {}
        }
        walk::walk_chain_expression(self, expr);
    }

    fn visit_break_statement(&mut self, stmt: &BreakStatement<'ast>) {
        if stmt.label.is_some() {
            self.inc_cognitive_flat(stmt.span, ComplexityContributionKind::LabeledBreak);
        }
        walk::walk_break_statement(self, stmt);
    }

    fn visit_continue_statement(&mut self, stmt: &ContinueStatement<'ast>) {
        if stmt.label.is_some() {
            self.inc_cognitive_flat(stmt.span, ComplexityContributionKind::LabeledContinue);
        }
        walk::walk_continue_statement(self, stmt);
    }

    fn visit_jsx_element(&mut self, element: &JSXElement<'ast>) {
        // JSX depth is descriptive context for render layout.
        self.open_jsx();
        walk::walk_jsx_element(self, element);
        self.close_jsx();
    }

    fn visit_jsx_fragment(&mut self, fragment: &JSXFragment<'ast>) {
        // A `<>...</>` fragment also contributes to descriptive JSX depth.
        self.open_jsx();
        walk::walk_jsx_fragment(self, fragment);
        self.close_jsx();
    }

    fn visit_return_statement(&mut self, stmt: &ReturnStatement<'ast>) {
        if let Some(argument) = &stmt.argument {
            self.name_returned_generator(argument);
        }
        walk::walk_return_statement(self, stmt);
        self.named_generator_call = None;
    }

    fn visit_call_expression(&mut self, call: &CallExpression<'ast>) {
        if let Expression::Identifier(callee) = &call.callee
            && is_hook_callee(callee.name.as_str())
        {
            self.record_hook(call.span);
        }
        if let Some(identity) = self.take_named_generator_call(call.span) {
            let identity =
                complexity_names::span_name(call).map_or(identity, FunctionIdentity::from_context);
            self.visit_call_with_named_generators(call, &identity);
            return;
        }
        if let Some(span) = complexity_names::span_name(call) {
            let identity = FunctionIdentity::from_context(span);
            self.visit_call_with_context_name(call, &SPAN_NAMED_ARGUMENTS, &identity);
            return;
        }
        if let Some(name) = complexity_names::string_keyed_call_name(call) {
            let identity = FunctionIdentity::from_context(name);
            self.visit_call_with_context_name(call, &STRING_KEYED_ARGUMENTS, &identity);
            return;
        }
        walk::walk_call_expression(self, call);
    }
}

/// Whether a function-frame name marks a React component (capitalized) or a
/// custom hook (`use<Uppercase>`). Used to gate the hook-density cognitive fold
/// so only React-shaped frames accrue it.
fn frame_name_is_react_shaped(name: &str) -> bool {
    let first = name.chars().next();
    if first.is_some_and(char::is_uppercase) {
        return true;
    }
    name.strip_prefix("use")
        .and_then(|rest| rest.chars().next())
        .is_some_and(char::is_uppercase)
}

/// Whether a callee identifier names a React hook: a built-in
/// (`useState` / `useEffect` / `useMemo` / `useCallback`) or any custom `use*`
/// hook (`use` followed by an uppercase letter, so a bare `use` / `used` is not
/// a hook). Mirrors the React extractor's `hook_kind` / `is_custom_hook_name`.
fn is_hook_callee(name: &str) -> bool {
    name.strip_prefix("use")
        .and_then(|rest| rest.chars().next())
        .is_some_and(char::is_uppercase)
}

/// Compute per-function complexity metrics from a parsed Oxc program.
pub fn compute_complexity(
    program: &Program<'_>,
    source: &str,
    line_offsets: &[u32],
) -> Vec<FunctionComplexity> {
    let mut visitor = ComplexityVisitor::new(source, line_offsets);

    visitor.visit_program(program);

    visitor.results
}

#[cfg(all(test, not(miri)))]
#[path = "complexity_tests.rs"]
mod tests;
