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

#[allow(clippy::wildcard_imports, reason = "many AST types used")]
use oxc_ast::ast::*;
use oxc_ast_visit::Visit;
use oxc_ast_visit::walk;
use oxc_semantic::ScopeFlags;
use oxc_span::GetSpan;
use oxc_span::Span;

use fallow_types::extract::{
    ComplexityContribution, ComplexityContributionKind, ComplexityMetric, FunctionComplexity,
};

/// Per-function state on the scope stack.
struct FunctionFrame {
    name: String,
    span: Span,
    cyclomatic: u16,
    cognitive: u16,
    nesting_level: u16,
    /// Track the last logical operator for cognitive boolean sequence detection.
    last_logical_operator: Option<LogicalOperator>,
    /// Number of parameters (excluding TypeScript's `this` parameter).
    param_count: u8,
    /// Per-increment breakdown accumulated as the function body is walked.
    contributions: Vec<ComplexityContribution>,
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
    /// Name override from a parent node (e.g., method name from `MethodDefinition`,
    /// variable name from `const foo = function() {}`).
    pending_name: Option<String>,
}

impl<'a> ComplexityVisitor<'a> {
    pub const fn new(source: &'a str, line_offsets: &'a [u32]) -> Self {
        Self {
            stack: Vec::new(),
            results: Vec::new(),
            line_offsets,
            source,
            pending_name: None,
        }
    }

    /// Compute the content digest for a function's full-span byte slice.
    ///
    /// The slice (`&source[span.start..span.end]`) is the canonical body bytes
    /// (signature line + body + closing brace, no whitespace normalization).
    /// Returns `None` if the span falls outside the source or on a non-char
    /// boundary; valid AST spans never do, but we clamp defensively rather than
    /// panic, mirroring `line_col_utf16` in the inventory walker.
    fn source_hash_for_span(&self, span: Span) -> Option<String> {
        let start = span.start as usize;
        let end = span.end as usize;
        let slice = self.source.get(start..end)?;
        Some(fallow_cov_protocol::source_hash_for(slice.as_bytes()))
    }

    fn push_function(&mut self, name: String, span: Span, param_count: u8) {
        self.stack.push(FunctionFrame {
            name,
            span,
            cyclomatic: 1,
            cognitive: 0,
            nesting_level: 0,
            last_logical_operator: None,
            param_count,
            contributions: Vec::new(),
        });
    }

    fn pop_function(&mut self) {
        if let Some(frame) = self.stack.pop() {
            let (line, col) =
                fallow_types::extract::byte_offset_to_line_col(self.line_offsets, frame.span.start);
            let end_line =
                fallow_types::extract::byte_offset_to_line_col(self.line_offsets, frame.span.end).0;
            let source_hash = self.source_hash_for_span(frame.span);
            self.results.push(FunctionComplexity {
                name: frame.name,
                line,
                col,
                cyclomatic: frame.cyclomatic,
                cognitive: frame.cognitive,
                line_count: end_line.saturating_sub(line) + 1,
                param_count: frame.param_count,
                source_hash,
                contributions: frame.contributions,
            });
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
    fn visit_function(&mut self, func: &Function<'ast>, flags: ScopeFlags) {
        let name = func
            .id
            .as_ref()
            .map(|id| {
                self.pending_name.take();
                id.name.to_string()
            })
            .or_else(|| self.pending_name.take())
            .unwrap_or_else(|| "<anonymous>".to_string());

        let is_nested = !self.stack.is_empty();
        if is_nested {
            self.inc_nesting();
        }

        let param_count = Self::count_params(&func.params);
        self.push_function(name, func.span, param_count);
        walk::walk_function(self, func, flags);
        self.pop_function();

        if is_nested {
            self.dec_nesting();
        }
    }

    fn visit_arrow_function_expression(&mut self, arrow: &ArrowFunctionExpression<'ast>) {
        let name = self
            .pending_name
            .take()
            .unwrap_or_else(|| "<arrow>".to_string());

        let is_nested = !self.stack.is_empty();
        if is_nested {
            self.inc_nesting();
        }

        let param_count = Self::count_params(&arrow.params);
        self.push_function(name, arrow.span, param_count);
        walk::walk_arrow_function_expression(self, arrow);
        self.pop_function();

        if is_nested {
            self.dec_nesting();
        }
    }

    fn visit_method_definition(&mut self, method: &MethodDefinition<'ast>) {
        if let Some(name) = method.key.static_name() {
            self.pending_name = Some(name.to_string());
        }
        walk::walk_method_definition(self, method);
        self.pending_name = None;
    }

    fn visit_variable_declarator(&mut self, decl: &VariableDeclarator<'ast>) {
        if let Some(id) = decl.id.get_binding_identifier() {
            self.pending_name = Some(id.name.to_string());
        }
        walk::walk_variable_declarator(self, decl);
        self.pending_name = None;
    }

    fn visit_property_definition(&mut self, prop: &PropertyDefinition<'ast>) {
        if let Some(name) = prop.key.static_name() {
            self.pending_name = Some(name.to_string());
        }
        walk::walk_property_definition(self, prop);
        self.pending_name = None;
    }

    fn visit_object_property(&mut self, prop: &ObjectProperty<'ast>) {
        if let Some(name) = prop.key.static_name() {
            self.pending_name = Some(name.to_string());
        }
        walk::walk_object_property(self, prop);
        self.pending_name = None;
    }

    fn visit_export_default_declaration(&mut self, decl: &ExportDefaultDeclaration<'ast>) {
        self.pending_name = Some("default".to_string());
        walk::walk_export_default_declaration(self, decl);
        self.pending_name = None;
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
        self.inc_cyclomatic(expr.span, logical_kind(expr.operator));

        self.handle_logical_operator(expr.operator, expr.span);

        self.visit_expression(&expr.left);

        self.visit_expression(&expr.right);

        if !Self::is_nested_logical(&expr.right) && !Self::is_nested_logical(&expr.left) {
            self.reset_logical_operator();
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
mod tests {
    use super::*;
    use fallow_types::extract::compute_line_offsets;
    use oxc_allocator::Allocator;
    use oxc_parser::Parser;
    use oxc_span::SourceType;

    fn analyze(source: &str) -> Vec<FunctionComplexity> {
        let allocator = Allocator::default();
        let source_type = SourceType::tsx();
        let parser_return = Parser::new(&allocator, source, source_type).parse();
        let line_offsets = compute_line_offsets(source);
        compute_complexity(&parser_return.program, source, &line_offsets)
    }

    fn find_fn<'a>(results: &'a [FunctionComplexity], name: &str) -> &'a FunctionComplexity {
        results
            .iter()
            .find(|r| r.name == name)
            .unwrap_or_else(|| panic!("function '{name}' not found in results: {results:?}"))
    }

    #[test]
    fn empty_function_has_cyclomatic_1() {
        let results = analyze("function foo() {}");
        let f = find_fn(&results, "foo");
        assert_eq!(f.cyclomatic, 1);
    }

    #[test]
    fn if_statement_adds_1() {
        let results = analyze("function foo(x) { if (x) { return 1; } return 0; }");
        let f = find_fn(&results, "foo");
        assert_eq!(f.cyclomatic, 2);
    }

    #[test]
    fn if_else_if_else_adds_2() {
        let results = analyze(
            "function foo(x) { if (x > 0) { return 1; } else if (x < 0) { return -1; } else { return 0; } }",
        );
        let f = find_fn(&results, "foo");
        assert_eq!(f.cyclomatic, 3);
    }

    #[test]
    fn for_loop_adds_1() {
        let results = analyze("function foo() { for (let i = 0; i < 10; i++) {} }");
        let f = find_fn(&results, "foo");
        assert_eq!(f.cyclomatic, 2);
    }

    #[test]
    fn while_loop_adds_1() {
        let results = analyze("function foo() { while (true) { break; } }");
        let f = find_fn(&results, "foo");
        assert_eq!(f.cyclomatic, 2);
    }

    #[test]
    fn switch_case_adds_per_case() {
        let results = analyze(
            "function foo(x) { switch (x) { case 1: break; case 2: break; default: break; } }",
        );
        let f = find_fn(&results, "foo");
        assert_eq!(f.cyclomatic, 3);
    }

    #[test]
    fn catch_adds_1() {
        let results = analyze("function foo() { try { } catch (e) { } }");
        let f = find_fn(&results, "foo");
        assert_eq!(f.cyclomatic, 2);
    }

    #[test]
    fn ternary_adds_1() {
        let results = analyze("function foo(x) { return x ? 1 : 0; }");
        let f = find_fn(&results, "foo");
        assert_eq!(f.cyclomatic, 2);
    }

    #[test]
    fn logical_and_adds_1() {
        let results = analyze("function foo(a, b) { return a && b; }");
        let f = find_fn(&results, "foo");
        assert_eq!(f.cyclomatic, 2);
    }

    #[test]
    fn logical_or_adds_1() {
        let results = analyze("function foo(a, b) { return a || b; }");
        let f = find_fn(&results, "foo");
        assert_eq!(f.cyclomatic, 2);
    }

    #[test]
    fn nullish_coalescing_adds_1() {
        let results = analyze("function foo(a) { return a ?? 'default'; }");
        let f = find_fn(&results, "foo");
        assert_eq!(f.cyclomatic, 2);
    }

    #[test]
    fn logical_assignment_adds_1() {
        let results = analyze("function foo(a) { a &&= true; a ||= false; a ??= null; }");
        let f = find_fn(&results, "foo");
        assert_eq!(f.cyclomatic, 4);
    }

    #[test]
    fn do_while_adds_1() {
        let results = analyze("function foo() { do { } while (true); }");
        let f = find_fn(&results, "foo");
        assert_eq!(f.cyclomatic, 2);
    }

    #[test]
    fn for_of_adds_1() {
        let results = analyze("function foo(arr) { for (const x of arr) { } }");
        let f = find_fn(&results, "foo");
        assert_eq!(f.cyclomatic, 2);
    }

    #[test]
    fn for_in_adds_1() {
        let results = analyze("function foo(obj) { for (const k in obj) { } }");
        let f = find_fn(&results, "foo");
        assert_eq!(f.cyclomatic, 2);
    }

    #[test]
    fn optional_chaining_adds_1() {
        let results = analyze("function foo(obj) { return obj?.value; }");
        let f = find_fn(&results, "foo");
        assert_eq!(f.cyclomatic, 2);
    }

    #[test]
    fn optional_chaining_computed_member_adds_1() {
        let results = analyze("function foo(obj) { return obj?.[0]; }");
        let f = find_fn(&results, "foo");
        assert_eq!(f.cyclomatic, 2);
    }

    #[test]
    fn optional_chaining_not_cognitive() {
        let results = analyze("function foo(obj) { return obj?.a?.b?.c; }");
        let f = find_fn(&results, "foo");
        assert!(
            f.cyclomatic > 1,
            "optional chaining should increment cyclomatic"
        );
        assert_eq!(
            f.cognitive, 0,
            "optional chaining should NOT increment cognitive"
        );
    }

    #[test]
    fn complex_function_cyclomatic() {
        let results = analyze(
            r"function complex(x, y) {
                if (x > 0) {
                    for (let i = 0; i < x; i++) {
                        if (y && i > 5) {
                            return true;
                        }
                    }
                } else if (x < 0) {
                    while (y) {
                        y--;
                    }
                }
                return x ? true : false;
            }",
        );
        let f = find_fn(&results, "complex");
        assert_eq!(f.cyclomatic, 8);
    }

    #[test]
    fn empty_function_has_cognitive_0() {
        let results = analyze("function foo() {}");
        let f = find_fn(&results, "foo");
        assert_eq!(f.cognitive, 0);
    }

    #[test]
    fn simple_if_cognitive_1() {
        let results = analyze("function foo(x) { if (x) { return 1; } }");
        let f = find_fn(&results, "foo");
        assert_eq!(f.cognitive, 1);
    }

    #[test]
    fn nested_if_cognitive_with_nesting() {
        let results = analyze("function foo(x, y) { if (x) { if (y) { return 1; } } }");
        let f = find_fn(&results, "foo");
        assert_eq!(f.cognitive, 3);
    }

    #[test]
    fn if_else_cognitive() {
        let results = analyze("function foo(x) { if (x) { return 1; } else { return 0; } }");
        let f = find_fn(&results, "foo");
        assert_eq!(f.cognitive, 2);
    }

    #[test]
    fn if_else_if_else_cognitive() {
        let results = analyze(
            "function foo(x) { if (x > 0) { return 1; } else if (x < 0) { return -1; } else { return 0; } }",
        );
        let f = find_fn(&results, "foo");
        assert_eq!(f.cognitive, 3);
    }

    #[test]
    fn boolean_sequence_same_operator() {
        let results = analyze("function foo(a, b, c) { return a && b && c; }");
        let f = find_fn(&results, "foo");
        assert_eq!(f.cognitive, 1);
    }

    #[test]
    fn boolean_sequence_mixed_operators() {
        let results = analyze("function foo(a, b, c) { return a && b || c; }");
        let f = find_fn(&results, "foo");
        assert_eq!(f.cognitive, 2);
    }

    #[test]
    fn for_loop_increases_nesting() {
        let results =
            analyze("function foo(arr) { for (const x of arr) { if (x) { return x; } } }");
        let f = find_fn(&results, "foo");
        assert_eq!(f.cognitive, 3);
    }

    #[test]
    fn switch_cognitive_1() {
        let results = analyze("function foo(x) { switch (x) { case 1: break; case 2: break; } }");
        let f = find_fn(&results, "foo");
        assert_eq!(f.cognitive, 1);
    }

    #[test]
    fn nested_function_resets_nesting() {
        let results = analyze(
            r"function outer(x) {
                if (x) {
                    const inner = () => {
                        if (x) { return 1; }
                    };
                }
            }",
        );
        let outer = find_fn(&results, "outer");
        let inner = find_fn(&results, "inner");
        assert_eq!(outer.cognitive, 1);
        assert_eq!(inner.cognitive, 1);
    }

    #[test]
    fn break_with_label_adds_1() {
        let results = analyze("function foo() { outer: for (;;) { break outer; } }");
        let f = find_fn(&results, "foo");
        assert!(f.cognitive >= 2);
    }

    #[test]
    fn arrow_function_tracked() {
        let results = analyze("const foo = (x) => x > 0 ? 1 : 0;");
        assert!(!results.is_empty());
        let f = &results[0];
        assert_eq!(f.name, "foo");
        assert_eq!(f.cyclomatic, 2);
    }

    #[test]
    fn line_count_computed() {
        let results =
            analyze("function foo() {\n  const a = 1;\n  const b = 2;\n  return a + b;\n}");
        let f = find_fn(&results, "foo");
        assert_eq!(f.line_count, 5);
    }

    #[test]
    fn deeply_nested_cognitive() {
        let results = analyze(
            r"function deep(a, b, c, d) {
                if (a) {
                    for (;;) {
                        if (b) {
                            while (c) {
                                if (d) {}
                            }
                        }
                    }
                }
            }",
        );
        let f = find_fn(&results, "deep");
        assert_eq!(f.cognitive, 15);
    }

    #[test]
    fn object_method_shorthand_named() {
        let results = analyze("const obj = { foo(x) { if (x) {} } };");
        let f = find_fn(&results, "foo");
        assert_eq!(f.cyclomatic, 2);
    }

    #[test]
    fn object_arrow_property_named() {
        let results = analyze("const obj = { bar: (x) => x ? 1 : 0 };");
        let f = find_fn(&results, "bar");
        assert_eq!(f.cyclomatic, 2);
    }

    #[test]
    fn class_method_named() {
        let results = analyze("class Foo { parse(x) { if (x) {} } }");
        let f = find_fn(&results, "parse");
        assert_eq!(f.cyclomatic, 2);
    }

    #[test]
    fn export_default_function_named() {
        let results = analyze("export default function() { if (true) {} }");
        let f = find_fn(&results, "default");
        assert_eq!(f.cyclomatic, 2);
    }

    #[test]
    fn export_default_named_function_keeps_name() {
        let results = analyze("export default function myFn() { if (true) {} }");
        let f = find_fn(&results, "myFn");
        assert_eq!(f.cyclomatic, 2);
    }

    #[test]
    fn catch_cognitive_with_nesting() {
        let results = analyze("function foo() { if (true) { try { } catch (e) { } } }");
        let f = find_fn(&results, "foo");
        assert_eq!(f.cognitive, 3);
    }

    #[test]
    fn do_while_cognitive_with_nesting() {
        let results = analyze("function foo() { if (true) { do { } while (true); } }");
        let f = find_fn(&results, "foo");
        assert_eq!(f.cognitive, 3);
    }

    #[test]
    fn while_cognitive_with_nesting() {
        let results = analyze("function foo() { if (true) { while (true) { break; } } }");
        let f = find_fn(&results, "foo");
        assert_eq!(f.cognitive, 3);
    }

    #[test]
    fn ternary_cognitive_with_nesting() {
        let results = analyze("function foo(x) { if (x) { return x ? 1 : 0; } }");
        let f = find_fn(&results, "foo");
        assert_eq!(f.cognitive, 3);
    }

    #[test]
    fn continue_with_label_cognitive() {
        let results =
            analyze("function foo() { outer: for (let i = 0; i < 10; i++) { continue outer; } }");
        let f = find_fn(&results, "foo");
        assert!(f.cognitive >= 2);
    }

    #[test]
    fn class_property_arrow_named() {
        let results = analyze("class Foo { bar = (x: number) => x > 0 ? 1 : 0; }");
        let f = find_fn(&results, "bar");
        assert_eq!(f.cyclomatic, 2);
    }

    #[test]
    fn nested_arrow_functions_independent_complexity() {
        let results = analyze(
            r"const outer = (x) => {
                if (x) {
                    const inner = (y) => {
                        if (y) { return 1; }
                        return 0;
                    };
                    return inner(x);
                }
                return 0;
            };",
        );
        let outer = find_fn(&results, "outer");
        let inner = find_fn(&results, "inner");
        assert_eq!(outer.cyclomatic, 2);
        assert_eq!(inner.cyclomatic, 2);
    }

    #[test]
    fn method_definition_named() {
        let results = analyze("class Foo { doWork(x) { if (x) { return 1; } return 0; } }");
        let f = find_fn(&results, "doWork");
        assert_eq!(f.cyclomatic, 2);
    }

    #[test]
    fn logical_nullish_cognitive() {
        let results = analyze("function foo(a, b) { return a ?? b; }");
        let f = find_fn(&results, "foo");
        assert_eq!(f.cognitive, 1);
    }

    #[test]
    fn mixed_logical_operators_cognitive() {
        let results = analyze("function foo(a, b, c, d) { return a && b || c ?? d; }");
        let f = find_fn(&results, "foo");
        assert_eq!(f.cognitive, 3);
    }

    #[test]
    fn saturating_add_prevents_overflow() {
        let mut source = "function foo() {".to_string();
        for _ in 0..20 {
            source.push_str("if (true) {");
        }
        for _ in 0..20 {
            source.push('}');
        }
        source.push('}');
        let results = analyze(&source);
        assert!(!results.is_empty());
    }

    #[test]
    fn empty_source_no_functions() {
        let results = analyze("");
        assert!(results.is_empty());
    }

    #[test]
    fn top_level_code_not_reported() {
        let results = analyze("if (true) { console.log('hello'); }");
        assert!(results.is_empty());
    }

    #[test]
    fn for_in_cognitive_with_nesting() {
        let results = analyze("function foo(obj) { for (const k in obj) { if (k) {} } }");
        let f = find_fn(&results, "foo");
        assert_eq!(f.cognitive, 3);
    }

    #[test]
    fn for_of_cognitive_with_nesting() {
        let results = analyze("function foo(arr) { for (const x of arr) { if (x) {} } }");
        let f = find_fn(&results, "foo");
        assert_eq!(f.cognitive, 3);
    }

    #[test]
    fn optional_call_expression_cyclomatic() {
        let results = analyze("function foo(obj) { return obj?.method(); }");
        let f = find_fn(&results, "foo");
        assert!(f.cyclomatic >= 1);
        assert_eq!(f.cognitive, 0);
    }

    #[test]
    fn logical_assignment_not_cognitive() {
        let results = analyze("function foo(a) { a &&= true; }");
        let f = find_fn(&results, "foo");
        assert_eq!(f.cyclomatic, 2);
    }

    #[test]
    fn multiple_switch_cases_cyclomatic() {
        let results = analyze(
            "function foo(x) { switch (x) { case 1: break; case 2: break; case 3: break; default: break; } }",
        );
        let f = find_fn(&results, "foo");
        assert_eq!(f.cyclomatic, 4);
    }

    #[test]
    fn switch_nested_in_if_cognitive() {
        let results = analyze("function foo(x, y) { if (x) { switch (y) { case 1: break; } } }");
        let f = find_fn(&results, "foo");
        assert_eq!(f.cognitive, 3);
    }

    #[test]
    fn line_and_col_computed_correctly() {
        let results = analyze("\n\nfunction foo() {\n  if (true) {}\n}\n");
        let f = find_fn(&results, "foo");
        assert_eq!(f.line, 3);
    }

    #[test]
    fn param_count_zero_for_no_params() {
        let results = analyze("function foo() {}");
        assert_eq!(find_fn(&results, "foo").param_count, 0);
    }

    #[test]
    fn param_count_simple_params() {
        let results = analyze("function foo(a, b, c) {}");
        assert_eq!(find_fn(&results, "foo").param_count, 3);
    }

    #[test]
    fn param_count_arrow_function() {
        let results = analyze("const bar = (a, b, c, d, e) => {}");
        assert_eq!(find_fn(&results, "bar").param_count, 5);
    }

    #[test]
    fn param_count_excludes_ts_this_parameter() {
        let results = analyze("function greet(this: Context, name: string) {}");
        assert_eq!(find_fn(&results, "greet").param_count, 1);
    }

    #[test]
    fn param_count_destructured_counts_as_one() {
        let results = analyze("function foo({ a, b, c }: Options) {}");
        assert_eq!(find_fn(&results, "foo").param_count, 1);
    }

    #[test]
    fn param_count_rest_parameter() {
        let results = analyze("function foo(a: number, ...rest: string[]) {}");
        assert_eq!(find_fn(&results, "foo").param_count, 2);
    }

    #[test]
    fn param_count_method_definition() {
        let results = analyze("class Foo { bar(a: number, b: string) {} }");
        assert_eq!(find_fn(&results, "bar").param_count, 2);
    }

    // --- Per-decision-point contribution breakdown ---

    fn sum_weights(f: &FunctionComplexity, metric: ComplexityMetric) -> u16 {
        f.contributions
            .iter()
            .filter(|c| c.metric == metric)
            .map(|c| c.weight)
            .sum()
    }

    /// The load-bearing invariant: the recorded breakdown must reconstruct the
    /// aggregate metric exactly. `cyclomatic = 1 + sum(cyclomatic weights)`,
    /// `cognitive = sum(cognitive weights)`. Asserted over an adversarial corpus
    /// that stresses every kind, including nested `else if` inside loops and
    /// `try` (the exact path whose arithmetic the else-if refactor rewrote).
    #[test]
    fn contributions_reconstruct_aggregate_metrics() {
        let corpus = [
            "function f(a,b){ if(a){} else if(b){} else {} }",
            "function f(a){ for(let i=0;i<10;i++){ if(a){ while(a){} } } }",
            "function f(a){ try { if(a){} } catch(e) { if(a){} else if(a){} } }",
            "function f(a,b,c){ return a && b || c ?? a; }",
            "function f(a){ switch(a){ case 1: break; case 2: return; default: return; } }",
            "function f(a){ for(const x of a){ if(x){ if(x){ if(x){} } } } }",
            "function f(a){ return a?.b?.c?.d; }",
            "function f(a){ a ||= 1; a &&= 2; a ??= 3; }",
            "function f(a){ outer: for(;;){ if(a){ break outer; } continue outer; } }",
            "function f(a,b){ if(a){ if(b){} else if(a){} else {} } else if(b){ for(;;){} } }",
            "const f = (a) => a ? (a ? 1 : 2) : 3;",
            "function f(a){ do { if(a){} } while(a); }",
        ];
        for src in corpus {
            for func in analyze(src) {
                assert_eq!(
                    func.cyclomatic,
                    1 + sum_weights(&func, ComplexityMetric::Cyclomatic),
                    "cyclomatic mismatch for `{src}`: {:?}",
                    func.contributions
                );
                assert_eq!(
                    func.cognitive,
                    sum_weights(&func, ComplexityMetric::Cognitive),
                    "cognitive mismatch for `{src}`: {:?}",
                    func.contributions
                );
            }
        }
    }

    #[test]
    fn else_if_contribution_is_flat_one() {
        // `else if` adds a flat +1 cognitive (no nesting penalty) and is recorded
        // as the ElseIf kind on the else-if line, not the leading `if` line.
        let results = analyze("function f(a, b) {\n  if (a) {\n  } else if (b) {\n  }\n}");
        let f = find_fn(&results, "f");
        let else_if = f
            .contributions
            .iter()
            .find(|c| {
                c.kind == ComplexityContributionKind::ElseIf
                    && c.metric == ComplexityMetric::Cognitive
            })
            .expect("else-if cognitive contribution");
        assert_eq!(else_if.weight, 1, "else-if cognitive is flat +1");
        assert_eq!(else_if.nesting, 0);
        assert_eq!(else_if.line, 3, "anchored on the `else if` line");
    }

    #[test]
    fn nested_if_carries_nesting_weight() {
        // An `if` nested one level deep inside another `if` gets cognitive +2
        // (+1 base, +1 nesting), recorded with nesting == 1.
        let results = analyze("function f(a, b) { if (a) { if (b) {} } }");
        let f = find_fn(&results, "f");
        let nested = f
            .contributions
            .iter()
            .filter(|c| {
                c.kind == ComplexityContributionKind::If && c.metric == ComplexityMetric::Cognitive
            })
            .max_by_key(|c| c.nesting)
            .expect("a nested if cognitive contribution");
        assert_eq!(nested.nesting, 1);
        assert_eq!(nested.weight, 2, "+1 base, +1 nesting");
    }

    #[test]
    fn cyclomatic_and_cognitive_logical_share_a_line() {
        // The cyclomatic and cognitive contributions for a `&&` are anchored to
        // the same node span, so a consumer grouping by line sees both together.
        let results = analyze("function f(a, b) { return a && b; }");
        let f = find_fn(&results, "f");
        let cyc = f
            .contributions
            .iter()
            .find(|c| {
                c.kind == ComplexityContributionKind::LogicalAnd
                    && c.metric == ComplexityMetric::Cyclomatic
            })
            .expect("cyclomatic &&");
        let cog = f
            .contributions
            .iter()
            .find(|c| {
                c.kind == ComplexityContributionKind::LogicalAnd
                    && c.metric == ComplexityMetric::Cognitive
            })
            .expect("cognitive &&");
        assert_eq!(cyc.line, cog.line);
    }

    #[test]
    fn simple_function_has_no_contributions() {
        let results = analyze("function f(a) { return a; }");
        assert!(find_fn(&results, "f").contributions.is_empty());
    }

    #[test]
    fn default_case_emits_no_contribution() {
        // A bare `default:` carries no test, so it adds nothing to cyclomatic and
        // produces no Case contribution; only the two real cases do.
        let results = analyze("function f(a) { switch(a) { case 1: break; default: break; } }");
        let f = find_fn(&results, "f");
        let cases = f
            .contributions
            .iter()
            .filter(|c| c.kind == ComplexityContributionKind::Case)
            .count();
        assert_eq!(cases, 1, "only the `case 1` test, not `default`");
    }
}
