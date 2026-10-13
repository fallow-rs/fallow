//! Mutant site collection.
//!
//! A site is one expression that the instrumentation wraps in a switch. Each
//! site holds the candidate mutants for that expression. The walk skips the
//! positions where a mutant is invalid or useless: type positions, enum
//! members, decorators, module syntax, directives, assignment targets,
//! property keys and the arguments of logging calls.
//!
//! The logging rule is broad: a `console.*` or `debug(...)` call gives no
//! mutant at all. This covers each expression in its arguments (strings,
//! arithmetic, updates, callbacks), and the call statement itself.

use std::collections::{BTreeMap, BTreeSet};

use oxc_ast::ast::{
    AccessorProperty, ArrowFunctionExpression, AssignmentExpression, AssignmentTarget,
    BinaryExpression, BooleanLiteral, CallExpression, ChainElement, ChainExpression, Class,
    ConditionalExpression, Decorator, Directive, DoWhileStatement, ExportAllDeclaration,
    ExportDefaultDeclaration, ExportFromDeclaration, ExportNamedDeclaration, Expression,
    ExpressionStatement, ForStatement, ForStatementLeft, Function, IfStatement, ImportDeclaration,
    ImportExpression, JSXAttributeValue, LogicalExpression, MemberExpression, MethodDefinition,
    MethodDefinitionKind, ObjectProperty, Program, PropertyDefinition, PropertyKey,
    ReturnStatement, SimpleAssignmentTarget, StringLiteral, TSEnumDeclaration, TSExportAssignment,
    TSExternalModuleDeclaration, TSGlobalDeclaration, TSImportEqualsDeclaration,
    TSInterfaceDeclaration, TSNamespaceDeclaration, TSNamespaceExportDeclaration, TSType,
    TSTypeAliasDeclaration, TSTypeAnnotation, TSTypeParameterDeclaration,
    TSTypeParameterInstantiation, TaggedTemplateExpression, UnaryExpression, UpdateExpression,
    VariableDeclaration, VariableDeclarator, WhileStatement,
};
use oxc_ast_visit::{Visit, walk};
use oxc_span::{GetSpan, Span};
use oxc_syntax::operator::{
    AssignmentOperator, BinaryOperator, LogicalOperator, UnaryOperator, UpdateOperator,
};
use oxc_syntax::scope::ScopeFlags;

use crate::{FunctionRef, Operator, is_identifier_byte};

/// One candidate mutant: replace `region` with `replacement`.
pub struct Candidate {
    pub operator: Operator,
    pub region: (u32, u32),
    pub replacement: String,
}

/// One expression that the instrumentation wraps.
pub struct Site {
    pub span: (u32, u32),
    pub candidates: Vec<Candidate>,
    /// The span of the node that anchors the mutant ids: the innermost
    /// function, or the top-level statement.
    pub anchor: (u32, u32),
    pub function: Option<FunctionRef>,
}

/// The collector output.
pub struct Collected {
    /// The sites, ordered by start, then by end descending (outer first).
    pub sites: Vec<Site>,
    /// The start offsets of expression statements. A wrapper that starts a
    /// statement needs a guard against automatic semicolon insertion.
    pub statement_starts: BTreeSet<u32>,
}

struct Frame {
    reference: FunctionRef,
    constructor: bool,
    generator: bool,
}

struct PendingFunction {
    name: Option<String>,
    constructor: bool,
}

pub fn collect(program: &Program<'_>, source: &str) -> Collected {
    let mut collector = Collector {
        source,
        sites: BTreeMap::new(),
        statement_starts: BTreeSet::new(),
        frames: Vec::new(),
        top_statement: (0, 0),
        pending: None,
        no_wrap: BTreeSet::new(),
        logical_operands: BTreeSet::new(),
        relational_operands: BTreeSet::new(),
        expression_starts: BTreeSet::new(),
    };
    collector.visit_program(program);
    let mut sites: Vec<Site> = collector.sites.into_values().collect();
    sites.sort_by_key(|site| (site.span.0, std::cmp::Reverse(site.span.1)));
    Collected {
        sites,
        statement_starts: collector.statement_starts,
    }
}

struct Collector<'s> {
    source: &'s str,
    sites: BTreeMap<(u32, u32), Site>,
    statement_starts: BTreeSet<u32>,
    frames: Vec<Frame>,
    top_statement: (u32, u32),
    pending: Option<PendingFunction>,
    /// Chain expressions that must keep their reference semantics, for
    /// example `(a?.b)()`, where a wrapper would lose the `this` value.
    no_wrap: BTreeSet<(u32, u32)>,
    /// Logical expressions that are a direct operand of another logical
    /// expression. Their replacement needs parentheses: `a ?? b ?? c` with
    /// the inner `??` replaced must become `(a && b) ?? c`.
    logical_operands: BTreeSet<(u32, u32)>,
    /// Binary expressions that are a direct operand of a relational or
    /// equality expression. A swap from `<` to `!=` lowers the precedence:
    /// `c == a < b` with the `<` replaced must become `c == (a != b)`.
    relational_operands: BTreeSet<(u32, u32)>,
    /// The starts of `export default` expressions and concise arrow bodies.
    /// At these positions a leading `function`, `class` or `{` changes the
    /// parse, as at the start of a statement.
    expression_starts: BTreeSet<u32>,
}

const fn pair(span: Span) -> (u32, u32) {
    (span.start, span.end)
}

fn is_function_like(expression: &Expression<'_>) -> bool {
    matches!(
        expression.without_parentheses(),
        Expression::FunctionExpression(_) | Expression::ArrowFunctionExpression(_)
    )
}

fn key_name(key: &PropertyKey<'_>) -> Option<String> {
    key.static_name().map(|name| name.into_owned())
}

/// `console.*` and `debug(...)` calls. The collector skips the full call:
/// it gives no mutant for any expression in the arguments, not only for the
/// strings. Logic in a logging argument is therefore not tested.
///
/// The check reads the callee through parentheses and TypeScript wrappers,
/// so `(console.log as any)(x)` is a logging call too.
fn is_logging_callee(callee: &Expression<'_>) -> bool {
    let callee = callee_without_wrappers(callee);
    if callee.is_specific_id("debug") {
        return true;
    }
    callee
        .as_member_expression()
        .is_some_and(|member| callee_without_wrappers(member.object()).is_specific_id("console"))
}

/// A `require(...)` call, also behind parentheses and TypeScript wrappers.
fn is_require_callee(callee: &Expression<'_>) -> bool {
    callee_without_wrappers(callee).is_specific_id("require")
}

/// The callee without parentheses and TypeScript expression wrappers, at any
/// depth. Type stripping keeps the reference, so `(o?.m as any)()` calls `m`
/// with `this` set to `o`, the same as `(o?.m)()`.
fn callee_without_wrappers<'e, 'a>(callee: &'e Expression<'a>) -> &'e Expression<'a> {
    match callee {
        Expression::ParenthesizedExpression(inner) => callee_without_wrappers(&inner.expression),
        Expression::TSAsExpression(inner) => callee_without_wrappers(&inner.expression),
        Expression::TSSatisfiesExpression(inner) => callee_without_wrappers(&inner.expression),
        Expression::TSNonNullExpression(inner) => callee_without_wrappers(&inner.expression),
        Expression::TSTypeAssertion(inner) => callee_without_wrappers(&inner.expression),
        Expression::TSInstantiationExpression(inner) => callee_without_wrappers(&inner.expression),
        _ => callee,
    }
}

fn is_skipped_call(call: &CallExpression<'_>) -> bool {
    matches!(call.callee, Expression::Super(_))
        || is_logging_callee(&call.callee)
        || is_require_callee(&call.callee)
}

const fn is_relational(operator: BinaryOperator) -> bool {
    matches!(
        operator,
        BinaryOperator::LessThan
            | BinaryOperator::LessEqualThan
            | BinaryOperator::GreaterThan
            | BinaryOperator::GreaterEqualThan
            | BinaryOperator::Equality
            | BinaryOperator::Inequality
            | BinaryOperator::StrictEquality
            | BinaryOperator::StrictInequality
    )
}

/// The operators at the relational or equality precedence level.
const fn is_comparison_level(operator: BinaryOperator) -> bool {
    is_relational(operator) || matches!(operator, BinaryOperator::In | BinaryOperator::Instanceof)
}

/// True when the replacement operator binds weaker than `operator`, so that
/// a token swap inside a comparison regroups the operands.
const fn lowers_precedence(operator: BinaryOperator, replacement: &str) -> bool {
    matches!(
        operator,
        BinaryOperator::LessThan
            | BinaryOperator::LessEqualThan
            | BinaryOperator::GreaterThan
            | BinaryOperator::GreaterEqualThan
    ) && matches!(replacement.as_bytes(), b"==" | b"!=")
}

/// The non-redundant relational replacements: operator texts, then the
/// constant that replaces the whole expression.
const fn relational_replacements(
    operator: BinaryOperator,
) -> Option<(&'static [&'static str], &'static str)> {
    let replacements: (&'static [&'static str], &'static str) = match operator {
        BinaryOperator::LessThan => (&["<=", "!="], "false"),
        BinaryOperator::LessEqualThan => (&["<", "=="], "true"),
        BinaryOperator::GreaterThan => (&[">=", "!="], "false"),
        BinaryOperator::GreaterEqualThan => (&[">", "=="], "true"),
        BinaryOperator::StrictEquality => (&["!=="], "false"),
        BinaryOperator::StrictInequality => (&["==="], "true"),
        BinaryOperator::Equality => (&["!="], "false"),
        BinaryOperator::Inequality => (&["=="], "true"),
        _ => return None,
    };
    Some(replacements)
}

const fn arithmetic_replacement(operator: BinaryOperator) -> Option<&'static str> {
    match operator {
        BinaryOperator::Addition => Some("-"),
        BinaryOperator::Subtraction => Some("+"),
        BinaryOperator::Multiplication => Some("/"),
        BinaryOperator::Division | BinaryOperator::Remainder => Some("*"),
        _ => None,
    }
}

fn is_text_operand(expression: &Expression<'_>) -> bool {
    matches!(
        expression.without_parentheses(),
        Expression::StringLiteral(_) | Expression::TemplateLiteral(_)
    )
}

/// The byte length of the `//` comment at the start of `rest`. The comment
/// ends before the first line terminator: `\n`, `\r`, U+2028 or U+2029.
fn line_comment_len(rest: &[u8]) -> usize {
    const LINE_SEPARATOR: &[u8] = "\u{2028}".as_bytes();
    const PARAGRAPH_SEPARATOR: &[u8] = "\u{2029}".as_bytes();
    (0..rest.len())
        .find(|&index| {
            let tail = &rest[index..];
            matches!(tail, [b'\n' | b'\r', ..])
                || tail.starts_with(LINE_SEPARATOR)
                || tail.starts_with(PARAGRAPH_SEPARATOR)
        })
        .unwrap_or(rest.len())
}

impl Collector<'_> {
    fn text(&self, start: u32, end: u32) -> &str {
        self.source
            .get(start as usize..end as usize)
            .unwrap_or_default()
    }

    fn byte(&self, offset: u32) -> Option<u8> {
        self.source.as_bytes().get(offset as usize).copied()
    }

    /// Finds `token` in `from..to`, after whitespace and comments only.
    ///
    /// The comment check comes first: a comment starts with `/`, the same
    /// byte as the division token.
    fn find_token(&self, from: u32, to: u32, token: &str) -> Option<u32> {
        let bytes = self.source.as_bytes();
        let end = (to as usize).min(bytes.len());
        let mut index = from as usize;
        while index < end {
            let rest = bytes.get(index..end)?;
            let comment = matches!(rest, [b'/', b'/' | b'*', ..]);
            if !comment && rest.starts_with(token.as_bytes()) {
                return u32::try_from(index).ok();
            }
            match rest {
                [b'/', b'/', ..] => {
                    index += line_comment_len(rest);
                }
                [b'/', b'*', ..] => {
                    let close = rest.windows(2).position(|w| w == b"*/")?;
                    index += close + 2;
                }
                [byte, ..] if byte.is_ascii_whitespace() => index += 1,
                [first, ..] if *first >= 0x80 => {
                    // Unicode whitespace such as NBSP or line separators.
                    let ch = self.source.get(index..)?.chars().next()?;
                    if !ch.is_whitespace() {
                        return None;
                    }
                    index += ch.len_utf8();
                }
                _ => return None,
            }
        }
        None
    }

    /// Adds spaces where the replacement would join an identifier next to
    /// the region, for example `void 0` after the `do` keyword in `do(f)()`.
    fn separate(&self, region: (u32, u32), replacement: String) -> String {
        let bytes = replacement.as_bytes();
        let before = region.0.checked_sub(1).and_then(|offset| self.byte(offset));
        let joins_before = bytes.first().copied().is_some_and(is_identifier_byte)
            && before.is_some_and(is_identifier_byte);
        let joins_after = bytes.last().copied().is_some_and(is_identifier_byte)
            && self.byte(region.1).is_some_and(is_identifier_byte);
        match (joins_before, joins_after) {
            (false, false) => replacement,
            (true, false) => format!(" {replacement}"),
            (false, true) => format!("{replacement} "),
            (true, true) => format!(" {replacement} "),
        }
    }

    fn add(&mut self, site: Span, operator: Operator, region: (u32, u32), replacement: String) {
        let replacement = self.separate(region, replacement);
        let anchor = self
            .frames
            .last()
            .map_or(self.top_statement, |frame| frame.reference.span);
        let function = self.frames.last().map(|frame| frame.reference.clone());
        self.sites
            .entry(pair(site))
            .or_insert_with(|| Site {
                span: pair(site),
                candidates: Vec::new(),
                anchor,
                function,
            })
            .candidates
            .push(Candidate {
                operator,
                region,
                replacement,
            });
    }

    /// Adds a space after `operator` when the next byte would merge with it
    /// into another token, for example `a+-b` to `a- -b`.
    fn pad_operator(&self, operator: &str, end: u32) -> String {
        match self.byte(end) {
            Some(b'+' | b'-' | b'/' | b'*') => format!("{operator} "),
            _ => operator.to_owned(),
        }
    }

    /// The replacement for a removed prefix token: a space when the removal
    /// would join two identifier characters, for example `return!a`.
    fn removal_text(&self, start: u32, end: u32) -> String {
        let before = start.checked_sub(1).and_then(|offset| self.byte(offset));
        let after = self.byte(end);
        match (before, after) {
            (Some(left), Some(right)) if is_identifier_byte(left) && is_identifier_byte(right) => {
                " ".to_owned()
            }
            _ => String::new(),
        }
    }

    fn negate(&mut self, test: &Expression<'_>) {
        let skip = match test.without_parentheses() {
            Expression::BinaryExpression(binary) => is_relational(binary.operator),
            Expression::LogicalExpression(_)
            | Expression::ImportMeta(_)
            | Expression::NewTarget(_) => true,
            _ => false,
        };
        if skip {
            return;
        }
        let region = pair(test.span());
        for value in ["true", "false"] {
            self.add(
                test.span(),
                Operator::NegateCondition,
                region,
                value.to_owned(),
            );
        }
    }

    fn removable_call(expression: &Expression<'_>) -> bool {
        match expression.without_parentheses() {
            Expression::CallExpression(call) => !is_skipped_call(call),
            Expression::AwaitExpression(await_expression) => {
                Self::removable_call(&await_expression.argument)
            }
            Expression::ChainExpression(chain) => match &chain.expression {
                ChainElement::CallExpression(call) => !is_skipped_call(call),
                _ => false,
            },
            _ => false,
        }
    }

    fn returns_value(&self, argument: &Expression<'_>) -> bool {
        if self
            .frames
            .last()
            .is_some_and(|frame| frame.constructor || frame.generator)
        {
            return false;
        }
        match argument.without_parentheses() {
            Expression::Identifier(identifier) => identifier.name != "undefined",
            Expression::UnaryExpression(unary) => unary.operator != UnaryOperator::Void,
            Expression::BooleanLiteral(_)
            | Expression::ImportMeta(_)
            | Expression::NewTarget(_) => false,
            _ => true,
        }
    }

    fn push_frame(&mut self, span: Span, name: Option<String>, generator: bool) {
        let pending = self.pending.take();
        let constructor = pending.as_ref().is_some_and(|pending| pending.constructor);
        let name = name.or_else(|| pending.and_then(|pending| pending.name));
        self.frames.push(Frame {
            reference: FunctionRef {
                name,
                span: pair(span),
            },
            constructor,
            generator,
        });
    }

    fn set_pending(&mut self, value: &Expression<'_>, name: Option<String>, constructor: bool) {
        self.pending = is_function_like(value).then_some(PendingFunction { name, constructor });
    }

    fn chain_links(&self, element: &ChainElement<'_>, links: &mut Vec<(u32, &'static str)>) {
        match element {
            ChainElement::CallExpression(call) => {
                self.call_link(call.optional, &call.callee, links);
            }
            ChainElement::TSNonNullExpression(non_null) => {
                self.expression_links(&non_null.expression, links);
            }
            _ => {
                if let Some(member) = element.as_member_expression() {
                    self.member_link(member, links);
                }
            }
        }
    }

    fn expression_links(&self, expression: &Expression<'_>, links: &mut Vec<(u32, &'static str)>) {
        match expression {
            Expression::CallExpression(call) => self.call_link(call.optional, &call.callee, links),
            Expression::TSNonNullExpression(non_null) => {
                self.expression_links(&non_null.expression, links);
            }
            _ => {
                if let Some(member) = expression.as_member_expression() {
                    self.member_link(member, links);
                }
            }
        }
    }

    fn call_link(
        &self,
        optional: bool,
        callee: &Expression<'_>,
        links: &mut Vec<(u32, &'static str)>,
    ) {
        if optional && let Some(offset) = self.find_token(callee.span().end, u32::MAX, "?.") {
            links.push((offset, ""));
        }
        self.expression_links(callee, links);
    }

    fn member_link(&self, member: &MemberExpression<'_>, links: &mut Vec<(u32, &'static str)>) {
        let object = member.object();
        if member.optional()
            && let Some(offset) = self.find_token(object.span().end, u32::MAX, "?.")
        {
            // `1?.x` to `1.x` would lex the dot as a decimal point.
            let number_before =
                matches!(object, Expression::NumericLiteral(_)) && offset == object.span().end;
            let replacement = match member {
                MemberExpression::ComputedMemberExpression(_) => "",
                _ if number_before => " .",
                _ => ".",
            };
            links.push((offset, replacement));
        }
        self.expression_links(object, links);
    }

    fn logical_operand(&self, operand: &Expression<'_>) -> String {
        let text = self.text(operand.span().start, operand.span().end);
        if matches!(operand, Expression::LogicalExpression(_)) {
            format!("({text})")
        } else {
            text.to_owned()
        }
    }
}

impl<'a> Visit<'a> for Collector<'_> {
    fn visit_program(&mut self, it: &Program<'a>) {
        for statement in &it.body {
            self.top_statement = pair(statement.span());
            self.visit_statement(statement);
        }
    }

    fn visit_function(&mut self, it: &Function<'a>, flags: ScopeFlags) {
        if it.declare || it.body.is_none() {
            self.pending = None;
            return;
        }
        let name = it.id.as_ref().map(|id| id.name.as_str().to_owned());
        self.push_frame(it.span, name, it.generator);
        walk::walk_function(self, it, flags);
        self.frames.pop();
    }

    fn visit_arrow_function_expression(&mut self, it: &ArrowFunctionExpression<'a>) {
        if let Some(body) = it.get_expression() {
            self.expression_starts.insert(body.span().start);
        }
        self.push_frame(it.span, None, false);
        walk::walk_arrow_function_expression(self, it);
        self.frames.pop();
    }

    fn visit_method_definition(&mut self, it: &MethodDefinition<'a>) {
        if it.value.body.is_none() {
            return;
        }
        let constructor = it.kind == MethodDefinitionKind::Constructor;
        self.pending = Some(PendingFunction {
            name: key_name(&it.key),
            constructor,
        });
        self.visit_function(&it.value, ScopeFlags::Function);
    }

    fn visit_property_definition(&mut self, it: &PropertyDefinition<'a>) {
        if it.declare {
            return;
        }
        if let Some(value) = &it.value {
            self.set_pending(value, key_name(&it.key), false);
            self.visit_expression(value);
        }
    }

    fn visit_accessor_property(&mut self, it: &AccessorProperty<'a>) {
        if let Some(value) = &it.value {
            self.set_pending(value, key_name(&it.key), false);
            self.visit_expression(value);
        }
    }

    fn visit_object_property(&mut self, it: &ObjectProperty<'a>) {
        self.set_pending(&it.value, key_name(&it.key), false);
        self.visit_expression(&it.value);
    }

    fn visit_variable_declaration(&mut self, it: &VariableDeclaration<'a>) {
        if !it.declare {
            walk::walk_variable_declaration(self, it);
        }
    }

    fn visit_variable_declarator(&mut self, it: &VariableDeclarator<'a>) {
        self.visit_binding_pattern(&it.id);
        if let Some(init) = &it.init {
            let name = it
                .id
                .get_binding_identifier()
                .map(|id| id.name.as_str().to_owned());
            self.set_pending(init, name, false);
            self.visit_expression(init);
        }
    }

    fn visit_class(&mut self, it: &Class<'a>) {
        if !it.declare {
            walk::walk_class(self, it);
        }
    }

    fn visit_expression_statement(&mut self, it: &ExpressionStatement<'a>) {
        let expression = &it.expression;
        self.statement_starts.insert(expression.span().start);
        if Self::removable_call(expression) {
            let span = expression.span();
            self.add(span, Operator::RemovedCall, pair(span), "void 0".to_owned());
        }
        self.visit_expression(expression);
    }

    fn visit_export_default_declaration(&mut self, it: &ExportDefaultDeclaration<'a>) {
        if let Some(expression) = it.declaration.as_expression() {
            self.expression_starts.insert(expression.span().start);
        }
        walk::walk_export_default_declaration(self, it);
    }

    fn visit_return_statement(&mut self, it: &ReturnStatement<'a>) {
        if let Some(argument) = &it.argument {
            if self.returns_value(argument) {
                let span = argument.span();
                self.add(
                    span,
                    Operator::ReturnValue,
                    pair(span),
                    "undefined".to_owned(),
                );
            }
            self.visit_expression(argument);
        }
    }

    fn visit_if_statement(&mut self, it: &IfStatement<'a>) {
        walk::walk_if_statement(self, it);
        self.negate(&it.test);
    }

    fn visit_while_statement(&mut self, it: &WhileStatement<'a>) {
        walk::walk_while_statement(self, it);
        self.negate(&it.test);
    }

    fn visit_do_while_statement(&mut self, it: &DoWhileStatement<'a>) {
        walk::walk_do_while_statement(self, it);
        self.negate(&it.test);
    }

    fn visit_for_statement(&mut self, it: &ForStatement<'a>) {
        walk::walk_for_statement(self, it);
        if let Some(test) = &it.test {
            self.negate(test);
        }
    }

    fn visit_conditional_expression(&mut self, it: &ConditionalExpression<'a>) {
        walk::walk_conditional_expression(self, it);
        self.negate(&it.test);
    }

    fn visit_binary_expression(&mut self, it: &BinaryExpression<'a>) {
        let operator = it.operator;
        let found = self.find_token(it.left.span().end, it.right.span().start, operator.as_str());
        if let Some(start) = found {
            let end = start + operator.as_str().len() as u32;
            if let Some(replacement) = arithmetic_replacement(operator) {
                let concatenation =
                    operator == BinaryOperator::Addition || operator == BinaryOperator::Subtraction;
                let text =
                    concatenation && (is_text_operand(&it.left) || is_text_operand(&it.right));
                if !text {
                    let padded = self.pad_operator(replacement, end);
                    self.add(it.span, Operator::ArithmeticOperator, (start, end), padded);
                }
            }
            if let Some((operators, constant)) = relational_replacements(operator) {
                self.add(
                    it.span,
                    Operator::RelationalOperator,
                    pair(it.span),
                    constant.to_owned(),
                );
                let operand = self.relational_operands.contains(&pair(it.span));
                for replacement in operators {
                    if operand && lowers_precedence(operator, replacement) {
                        let text = format!(
                            "({}{replacement}{})",
                            self.text(it.span.start, start),
                            self.text(end, it.span.end),
                        );
                        self.add(it.span, Operator::RelationalOperator, pair(it.span), text);
                    } else {
                        self.add(
                            it.span,
                            Operator::RelationalOperator,
                            (start, end),
                            (*replacement).to_owned(),
                        );
                    }
                }
            }
        }
        if is_comparison_level(operator) {
            for operand in [&it.left, &it.right] {
                if matches!(operand, Expression::BinaryExpression(_)) {
                    self.relational_operands.insert(pair(operand.span()));
                }
            }
        }
        walk::walk_binary_expression(self, it);
    }

    fn visit_logical_expression(&mut self, it: &LogicalExpression<'a>) {
        let operator = it.operator;
        let found = self.find_token(it.left.span().end, it.right.span().start, operator.as_str());
        if let Some(start) = found {
            let end = start + operator.as_str().len() as u32;
            let replacement = match operator {
                LogicalOperator::And => "||",
                LogicalOperator::Or | LogicalOperator::Coalesce => "&&",
            };
            let left_nested = matches!(it.left, Expression::LogicalExpression(_));
            let right_nested = matches!(it.right, Expression::LogicalExpression(_));
            if left_nested {
                self.logical_operands.insert(pair(it.left.span()));
            }
            if right_nested {
                self.logical_operands.insert(pair(it.right.span()));
            }
            let operand = self.logical_operands.contains(&pair(it.span));
            if left_nested || right_nested || operand {
                let text = format!(
                    "{}{}{replacement}{}{}",
                    self.logical_operand(&it.left),
                    self.text(it.left.span().end, start),
                    self.text(end, it.right.span().start),
                    self.logical_operand(&it.right),
                );
                let text = if operand { format!("({text})") } else { text };
                self.add(it.span, Operator::LogicalOperator, pair(it.span), text);
            } else {
                let replacement = replacement.to_owned();
                self.add(
                    it.span,
                    Operator::LogicalOperator,
                    (start, end),
                    replacement,
                );
            }
        }
        walk::walk_logical_expression(self, it);
    }

    fn visit_unary_expression(&mut self, it: &UnaryExpression<'a>) {
        let mutable = match it.operator {
            UnaryOperator::Delete => return,
            UnaryOperator::LogicalNot => !matches!(
                it.argument.without_parentheses(),
                Expression::UnaryExpression(inner) if inner.operator == UnaryOperator::LogicalNot
            ),
            UnaryOperator::UnaryNegation => true,
            _ => false,
        };
        if mutable && self.text(it.span.start, it.span.start + 1) == it.operator.as_str() {
            if self.statement_starts.contains(&it.span.start)
                || self.expression_starts.contains(&it.span.start)
            {
                // Without the operator, `!function () {}()` would start with
                // `function` and parse as a declaration.
                let argument = it.argument.span();
                let replacement = format!("({})", self.text(argument.start, argument.end));
                self.add(it.span, Operator::UnaryOperator, pair(it.span), replacement);
            } else {
                let region = (it.span.start, it.span.start + 1);
                let replacement = self.removal_text(region.0, region.1);
                self.add(it.span, Operator::UnaryOperator, region, replacement);
            }
        }
        walk::walk_unary_expression(self, it);
    }

    fn visit_update_expression(&mut self, it: &UpdateExpression<'a>) {
        let (text, replacement) = match it.operator {
            UpdateOperator::Increment => ("++", "--"),
            UpdateOperator::Decrement => ("--", "++"),
        };
        let region = if it.prefix {
            (it.span.start, it.span.start + 2)
        } else {
            (it.span.end.saturating_sub(2), it.span.end)
        };
        if self.text(region.0, region.1) != text {
            return;
        }
        let before = region.0.checked_sub(1).and_then(|offset| self.byte(offset));
        let replacement = if it.prefix && matches!(before, Some(b'+' | b'-')) {
            format!(" {replacement}")
        } else {
            replacement.to_owned()
        };
        self.add(it.span, Operator::UpdateOperator, region, replacement);
    }

    fn visit_assignment_expression(&mut self, it: &AssignmentExpression<'a>) {
        let replacement = match it.operator {
            AssignmentOperator::Addition => Some("-="),
            AssignmentOperator::Subtraction => Some("+="),
            _ => None,
        };
        let token = it.operator.as_str();
        if let Some(replacement) = replacement
            && let Some(start) = self.find_token(it.left.span().end, it.right.span().start, token)
        {
            let region = (start, start + token.len() as u32);
            self.add(
                it.span,
                Operator::ArithmeticOperator,
                region,
                replacement.to_owned(),
            );
        }
        self.visit_expression(&it.right);
    }

    fn visit_chain_expression(&mut self, it: &ChainExpression<'a>) {
        if let ChainElement::CallExpression(call) = &it.expression
            && is_logging_callee(&call.callee)
        {
            return;
        }
        if !self.no_wrap.contains(&pair(it.span)) {
            let mut links = Vec::new();
            self.chain_links(&it.expression, &mut links);
            links.sort_unstable();
            for (offset, replacement) in links {
                let region = (offset, offset + 2);
                self.add(
                    it.span,
                    Operator::OptionalChaining,
                    region,
                    replacement.to_owned(),
                );
            }
        }
        walk::walk_chain_expression(self, it);
    }

    fn visit_call_expression(&mut self, it: &CallExpression<'a>) {
        if is_logging_callee(&it.callee) || is_require_callee(&it.callee) {
            return;
        }
        if let Expression::ChainExpression(chain) = callee_without_wrappers(&it.callee) {
            self.no_wrap.insert(pair(chain.span));
        }
        walk::walk_call_expression(self, it);
    }

    fn visit_boolean_literal(&mut self, it: &BooleanLiteral) {
        let replacement = if it.value { "false" } else { "true" };
        self.add(
            it.span,
            Operator::BooleanLiteral,
            pair(it.span),
            replacement.to_owned(),
        );
    }

    fn visit_string_literal(&mut self, it: &StringLiteral<'a>) {
        let quote = self.text(it.span.start, it.span.start + 1).to_owned();
        if quote != "\"" && quote != "'" {
            return;
        }
        let replacement = if it.value.is_empty() {
            format!("{quote}fallow{quote}")
        } else {
            format!("{quote}{quote}")
        };
        self.add(it.span, Operator::StringLiteral, pair(it.span), replacement);
    }

    fn visit_tagged_template_expression(&mut self, it: &TaggedTemplateExpression<'a>) {
        self.visit_template_literal(&it.quasi);
    }

    fn visit_jsx_attribute_value(&mut self, it: &JSXAttributeValue<'a>) {
        if !matches!(it, JSXAttributeValue::StringLiteral(_)) {
            walk::walk_jsx_attribute_value(self, it);
        }
    }

    fn visit_ts_namespace_declaration(&mut self, it: &TSNamespaceDeclaration<'a>) {
        if !it.declare {
            walk::walk_ts_namespace_declaration(self, it);
        }
    }

    // Positions that never produce a mutant.
    fn visit_property_key(&mut self, _it: &PropertyKey<'a>) {}
    fn visit_directive(&mut self, _it: &Directive<'a>) {}
    fn visit_decorator(&mut self, _it: &Decorator<'a>) {}
    fn visit_assignment_target(&mut self, _it: &AssignmentTarget<'a>) {}
    fn visit_simple_assignment_target(&mut self, _it: &SimpleAssignmentTarget<'a>) {}
    fn visit_for_statement_left(&mut self, _it: &ForStatementLeft<'a>) {}
    fn visit_import_expression(&mut self, _it: &ImportExpression<'a>) {}
    fn visit_import_declaration(&mut self, _it: &ImportDeclaration<'a>) {}
    fn visit_export_named_declaration(&mut self, _it: &ExportNamedDeclaration<'a>) {}
    fn visit_export_from_declaration(&mut self, _it: &ExportFromDeclaration<'a>) {}
    fn visit_export_all_declaration(&mut self, _it: &ExportAllDeclaration<'a>) {}
    fn visit_ts_export_assignment(&mut self, _it: &TSExportAssignment<'a>) {}
    fn visit_ts_namespace_export_declaration(&mut self, _it: &TSNamespaceExportDeclaration<'a>) {}
    fn visit_ts_import_equals_declaration(&mut self, _it: &TSImportEqualsDeclaration<'a>) {}
    fn visit_ts_external_module_declaration(&mut self, _it: &TSExternalModuleDeclaration<'a>) {}
    fn visit_ts_global_declaration(&mut self, _it: &TSGlobalDeclaration<'a>) {}
    fn visit_ts_enum_declaration(&mut self, _it: &TSEnumDeclaration<'a>) {}
    fn visit_ts_type_alias_declaration(&mut self, _it: &TSTypeAliasDeclaration<'a>) {}
    fn visit_ts_interface_declaration(&mut self, _it: &TSInterfaceDeclaration<'a>) {}
    fn visit_ts_type(&mut self, _it: &TSType<'a>) {}
    fn visit_ts_type_annotation(&mut self, _it: &TSTypeAnnotation<'a>) {}
    fn visit_ts_type_parameter_declaration(&mut self, _it: &TSTypeParameterDeclaration<'a>) {}
    fn visit_ts_type_parameter_instantiation(&mut self, _it: &TSTypeParameterInstantiation<'a>) {}
}
