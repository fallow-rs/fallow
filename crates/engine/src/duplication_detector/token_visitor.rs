//! AST visitor that extracts a flat sequence of normalized tokens.
//!
//! The `TokenExtractor` walks an Oxc AST and emits a `Vec<SourceToken>` suitable
//! for suffix-array based clone detection.

use fallow_extract::JsText;
use oxc_ast::ast::*;
use oxc_ast::match_expression;
use oxc_ast_visit::Visit;
use oxc_ast_visit::walk;
use oxc_span::{GetSpan, Span};
use oxc_syntax::scope::ScopeFlags;

use super::token_types::{
    KeywordType, OperatorType, PunctuationType, SourceToken, TokenKind, point_span,
};

/// AST visitor that extracts a flat sequence of normalized tokens.
pub(super) struct TokenExtractor<'s> {
    /// The text that the parser read. Token spans index into it.
    source: &'s str,
    pub(super) tokens: Vec<SourceToken>,
    pub(super) function_spans: Vec<Span>,
    pub(super) atomic_invocation_spans: Vec<Span>,
    pub(super) literal_argument_spans: Vec<Span>,
    /// When true, skip TypeScript type annotations, interfaces, and type aliases
    /// to enable cross-language clone detection between .ts and .js files.
    strip_types: bool,
    /// When true, skip all `import` declarations from the token stream to reduce
    /// noise from sorted import blocks that naturally look similar across files.
    skip_imports: bool,
}

impl<'s> TokenExtractor<'s> {
    pub(super) const fn new(source: &'s str, strip_types: bool, skip_imports: bool) -> Self {
        Self {
            source,
            tokens: Vec::new(),
            function_spans: Vec::new(),
            atomic_invocation_spans: Vec::new(),
            literal_argument_spans: Vec::new(),
            strip_types,
            skip_imports,
        }
    }

    /// Span of the word (a keyword or a name) that starts at `start`, or a
    /// one-byte span when no word starts there.
    fn word(&self, start: u32) -> Span {
        let bytes = self.source.as_bytes();
        let from = start as usize;
        let len = bytes.get(from..).map_or(0, |rest| {
            rest.iter().take_while(|&&b| is_word_byte(b)).count()
        });
        if len == 0 {
            return point_span(start);
        }
        Span::new(start, start.saturating_add(len_u32_from(len)))
    }

    /// Span of `text` at `start`, or a one-byte span when the source has other
    /// text there.
    fn text_at(&self, start: u32, text: &str) -> Span {
        if self
            .source
            .get(start as usize..)
            .is_some_and(|rest| rest.starts_with(text))
        {
            Span::new(start, start.saturating_add(len_u32(text)))
        } else {
            point_span(start)
        }
    }

    /// Span of the keyword `text` at `start`, or an empty span at `start` when
    /// the source has no such keyword there.
    fn keyword_text(&self, start: u32, text: &str) -> Span {
        let word = self.word(start);
        if self.source.get(word.start as usize..word.end as usize) == Some(text) {
            word
        } else {
            Span::new(start, start)
        }
    }

    /// Span of `text` as the next token after byte `from`, past whitespace and
    /// comments. An empty span at `from` when the next token is other text.
    fn next_text(&self, from: u32, text: &str) -> Span {
        let pos = skip_trivia(self.source, from as usize);
        match self.source.get(pos..) {
            Some(rest) if !text.is_empty() && rest.starts_with(text) => {
                let start = len_u32_from(pos);
                Span::new(start, start.saturating_add(len_u32(text)))
            }
            _ => Span::new(from, from),
        }
    }

    /// Span of `text` as the token directly before byte `before`, past
    /// whitespace. An empty span at `before` when the source has other text
    /// there.
    fn prev_text(&self, before: u32, text: &str) -> Span {
        let head = self.source.get(..before as usize).unwrap_or_default();
        let trimmed = head.trim_end();
        if !text.is_empty() && trimmed.ends_with(text) {
            let end = len_u32_from(trimmed.len());
            Span::new(end.saturating_sub(len_u32(text)), end)
        } else {
            Span::new(before, before)
        }
    }

    fn push(&mut self, kind: TokenKind, span: Span) {
        self.tokens.push(SourceToken { kind, span });
    }

    fn push_keyword(&mut self, kw: KeywordType, span: Span) {
        self.push(TokenKind::Keyword(kw), span);
    }

    fn push_op(&mut self, op: OperatorType, span: Span) {
        self.push(TokenKind::Operator(op), span);
    }

    fn push_punc(&mut self, p: PunctuationType, span: Span) {
        self.push(TokenKind::Punctuation(p), span);
    }

    fn push_atomic_invocation_span(&mut self, span: Span) {
        self.atomic_invocation_spans.push(span);
    }

    /// Tokens of a meta property: the meta name at the start of `span` and the
    /// property name at its end.
    fn push_meta_property_tokens(&mut self, span: Span, meta: &str, property: &str) {
        let meta_end = span.start.saturating_add(len_u32(meta)).min(span.end);
        let property_start = span.end.saturating_sub(len_u32(property)).max(span.start);
        self.push(
            TokenKind::Identifier(meta.to_string()),
            Span::new(span.start, meta_end),
        );
        self.push(
            TokenKind::Identifier(property.to_string()),
            Span::new(property_start, span.end),
        );
    }

    /// Tokens of an interface heritage name, spelled as the member expression
    /// it was before Oxc 0.151: `A.B` gives `A`, a dot and `B`.
    fn visit_heritage_type_name(&mut self, name: &TSTypeName<'_>) {
        match name {
            TSTypeName::QualifiedName(qualified) => {
                self.visit_heritage_type_name(&qualified.left);
                self.push_punc(PunctuationType::Dot, point_span(qualified.left.span().end));
                self.push(
                    TokenKind::Identifier(qualified.right.name.to_string()),
                    qualified.right.span,
                );
            }
            _ => self.visit_ts_type_name(name),
        }
    }

    /// Tokens of an expression in statement position: the expression, then a
    /// semicolon.
    fn visit_statement_expression(&mut self, expr: &Expression<'_>, span: Span) {
        let functions_before = self.function_spans.len();
        self.visit_expression(expr);
        if is_invocation_expr(expr) {
            self.push_atomic_invocation_span_if_no_function(span, functions_before);
        }
        self.push_punc(PunctuationType::Semicolon, tail(span));
    }

    /// Record `span` as an atomic invocation when the visitor saw no function
    /// since `functions_before`. A function anywhere inside the invocation (a
    /// callback, an IIFE callee, or a method in an object literal argument such
    /// as the options object of `defineComponent({ ... })`) holds real code, so
    /// a clone inside it must stay reportable.
    fn push_atomic_invocation_span_if_no_function(
        &mut self,
        span: Span,
        functions_before: usize,
    ) -> bool {
        let atomic = self.function_spans.len() == functions_before;
        if atomic {
            self.push_atomic_invocation_span(span);
        }
        atomic
    }

    /// Record the atomic invocation `span` of a call or `new` expression, plus
    /// the spans of its object and array literal arguments. The literal spans
    /// let the filter keep a clone that starts inside one literal argument,
    /// such as a copied `props` block in `defineComponent({ ... })`.
    fn push_atomic_call_spans(
        &mut self,
        span: Span,
        functions_before: usize,
        arguments: &[Argument<'_>],
    ) {
        if self.push_atomic_invocation_span_if_no_function(span, functions_before) {
            self.literal_argument_spans
                .extend(arguments.iter().filter_map(literal_argument_span));
        }
    }
}

// A token gets the span of its own text, not the span of the AST node that
// holds it. A clone instance runs from the start of its first token to the end
// of its last token, so a token with the span of a whole function or block
// stretches the reported clone over code that does not match.

/// Span of a token at the first byte of `span`, such as a leading keyword or an
/// opening brace.
const fn head(span: Span) -> Span {
    point_span(span.start)
}

/// Span of a token at the last byte of `span`, such as a closing brace.
const fn tail(span: Span) -> Span {
    point_span(span.end.saturating_sub(1))
}

fn len_u32_from(len: usize) -> u32 {
    u32::try_from(len).unwrap_or(u32::MAX)
}

const fn is_word_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'$'
}

/// The first byte at or after `from` that is not whitespace or a comment.
fn skip_trivia(source: &str, from: usize) -> usize {
    let bytes = source.as_bytes();
    let mut pos = from;
    while pos < bytes.len() {
        if bytes[pos].is_ascii_whitespace() {
            pos += 1;
        } else if bytes[pos..].starts_with(b"//") {
            pos = bytes[pos..]
                .iter()
                .position(|&b| b == b'\n')
                .map_or(bytes.len(), |nl| pos + nl);
        } else if bytes[pos..].starts_with(b"/*") {
            pos = source[pos + 2..]
                .find("*/")
                .map_or(bytes.len(), |close| pos + 2 + close + 2);
        } else {
            break;
        }
    }
    pos
}

fn len_u32(text: &str) -> u32 {
    u32::try_from(text.len()).unwrap_or(u32::MAX)
}

/// The span of an object or array literal argument, optionally wrapped by
/// parentheses or a type assertion.
fn literal_argument_span(arg: &Argument<'_>) -> Option<Span> {
    arg.as_expression().and_then(literal_expression_span)
}

fn literal_expression_span(expr: &Expression<'_>) -> Option<Span> {
    match expr {
        Expression::ObjectExpression(expr) => Some(expr.span),
        Expression::ArrayExpression(expr) => Some(expr.span),
        Expression::ParenthesizedExpression(expr) => literal_expression_span(&expr.expression),
        Expression::TSAsExpression(expr) => literal_expression_span(&expr.expression),
        Expression::TSSatisfiesExpression(expr) => literal_expression_span(&expr.expression),
        _ => None,
    }
}

/// True when `expr` is a call or `new` invocation, optionally wrapped by
/// `await`, parentheses, a type assertion, or an optional chain.
fn is_invocation_expr(expr: &Expression<'_>) -> bool {
    match expr {
        Expression::CallExpression(_) | Expression::NewExpression(_) => true,
        Expression::AwaitExpression(expr) => is_invocation_expr(&expr.argument),
        Expression::ParenthesizedExpression(expr) => is_invocation_expr(&expr.expression),
        Expression::TSAsExpression(expr) => is_invocation_expr(&expr.expression),
        Expression::TSSatisfiesExpression(expr) => is_invocation_expr(&expr.expression),
        Expression::TSNonNullExpression(expr) => is_invocation_expr(&expr.expression),
        Expression::ChainExpression(expr) => {
            matches!(&expr.expression, ChainElement::CallExpression(_))
        }
        _ => false,
    }
}

fn is_static_require_binding_declaration(decl: &VariableDeclaration<'_>) -> bool {
    matches!(
        decl.kind,
        VariableDeclarationKind::Var
            | VariableDeclarationKind::Let
            | VariableDeclarationKind::Const
    ) && !decl.declarations.is_empty()
        && decl
            .declarations
            .iter()
            .all(is_static_require_binding_declarator)
}

fn is_static_require_binding_declarator(decl: &VariableDeclarator<'_>) -> bool {
    decl.init
        .as_ref()
        .is_some_and(is_static_require_call_expression)
}

fn is_static_require_call_expression(expr: &Expression<'_>) -> bool {
    let Expression::CallExpression(call) = expr else {
        return false;
    };
    call.arguments.len() == 1
        && matches!(&call.callee, Expression::Identifier(id) if id.name == "require")
        && matches!(call.arguments.first(), Some(Argument::StringLiteral(_)))
}

impl<'a> Visit<'a> for TokenExtractor<'_> {
    fn visit_program(&mut self, program: &Program<'a>) {
        if !self.skip_imports {
            walk::walk_program(self, program);
            return;
        }

        if let Some(hashbang) = &program.hashbang {
            self.visit_hashbang(hashbang);
        }
        self.visit_directives(&program.directives);
        for stmt in &program.body {
            if let Statement::VariableDeclaration(decl) = stmt
                && is_static_require_binding_declaration(decl)
            {
                continue;
            }
            self.visit_statement(stmt);
        }
    }

    fn visit_variable_declaration(&mut self, decl: &VariableDeclaration<'a>) {
        let kw = match decl.kind {
            VariableDeclarationKind::Var => KeywordType::Var,
            VariableDeclarationKind::Let => KeywordType::Let,
            VariableDeclarationKind::Const => KeywordType::Const,
            VariableDeclarationKind::Using | VariableDeclarationKind::AwaitUsing => {
                KeywordType::Const
            }
        };
        self.push_keyword(kw, self.word(decl.span.start));
        walk::walk_variable_declaration(self, decl);
    }

    fn visit_return_statement(&mut self, stmt: &ReturnStatement<'a>) {
        let functions_before = self.function_spans.len();
        self.push_keyword(KeywordType::Return, self.word(stmt.span.start));
        walk::walk_return_statement(self, stmt);
        if stmt.argument.as_ref().is_some_and(is_invocation_expr) {
            self.push_atomic_invocation_span_if_no_function(stmt.span, functions_before);
        }
    }

    fn visit_if_statement(&mut self, stmt: &IfStatement<'a>) {
        let keyword = self.word(stmt.span.start);
        self.push_keyword(KeywordType::If, keyword);
        self.push_punc(PunctuationType::OpenParen, self.next_text(keyword.end, "("));
        self.visit_expression(&stmt.test);
        self.push_punc(
            PunctuationType::CloseParen,
            self.next_text(stmt.test.span().end, ")"),
        );
        self.visit_statement(&stmt.consequent);
        if let Some(alt) = &stmt.alternate {
            self.push_keyword(
                KeywordType::Else,
                self.next_text(stmt.consequent.span().end, "else"),
            );
            self.visit_statement(alt);
        }
    }

    fn visit_for_statement(&mut self, stmt: &ForStatement<'a>) {
        let keyword = self.word(stmt.span.start);
        self.push_keyword(KeywordType::For, keyword);
        self.push_punc(PunctuationType::OpenParen, self.next_text(keyword.end, "("));
        walk::walk_for_statement(self, stmt);
        self.push_punc(PunctuationType::CloseParen, tail(stmt.span));
    }

    fn visit_for_in_statement(&mut self, stmt: &ForInStatement<'a>) {
        let keyword = self.word(stmt.span.start);
        self.push_keyword(KeywordType::For, keyword);
        self.push_punc(PunctuationType::OpenParen, self.next_text(keyword.end, "("));
        self.visit_for_statement_left(&stmt.left);
        self.push_keyword(KeywordType::In, self.next_text(stmt.left.span().end, "in"));
        self.visit_expression(&stmt.right);
        self.push_punc(
            PunctuationType::CloseParen,
            self.next_text(stmt.right.span().end, ")"),
        );
        self.visit_statement(&stmt.body);
    }

    fn visit_for_of_statement(&mut self, stmt: &ForOfStatement<'a>) {
        let keyword = self.word(stmt.span.start);
        self.push_keyword(KeywordType::For, keyword);
        self.push_punc(PunctuationType::OpenParen, self.next_text(keyword.end, "("));
        self.visit_for_statement_left(&stmt.left);
        self.push_keyword(KeywordType::Of, self.next_text(stmt.left.span().end, "of"));
        self.visit_expression(&stmt.right);
        self.push_punc(
            PunctuationType::CloseParen,
            self.next_text(stmt.right.span().end, ")"),
        );
        self.visit_statement(&stmt.body);
    }

    fn visit_while_statement(&mut self, stmt: &WhileStatement<'a>) {
        let keyword = self.word(stmt.span.start);
        self.push_keyword(KeywordType::While, keyword);
        self.push_punc(PunctuationType::OpenParen, self.next_text(keyword.end, "("));
        walk::walk_while_statement(self, stmt);
        self.push_punc(PunctuationType::CloseParen, tail(stmt.span));
    }

    fn visit_do_while_statement(&mut self, stmt: &DoWhileStatement<'a>) {
        self.push_keyword(KeywordType::Do, self.word(stmt.span.start));
        walk::walk_do_while_statement(self, stmt);
    }

    fn visit_switch_statement(&mut self, stmt: &SwitchStatement<'a>) {
        let keyword = self.word(stmt.span.start);
        self.push_keyword(KeywordType::Switch, keyword);
        self.push_punc(PunctuationType::OpenParen, self.next_text(keyword.end, "("));
        walk::walk_switch_statement(self, stmt);
        self.push_punc(PunctuationType::CloseParen, tail(stmt.span));
    }

    fn visit_switch_case(&mut self, case: &SwitchCase<'a>) {
        let keyword = self.word(case.span.start);
        let colon_after = if let Some(test) = &case.test {
            self.push_keyword(KeywordType::Case, keyword);
            self.visit_expression(test);
            test.span().end
        } else {
            self.push_keyword(KeywordType::Default, keyword);
            keyword.end
        };
        let colon = self.next_text(colon_after, ":");
        self.push_punc(PunctuationType::Colon, colon);
        self.visit_statements(&case.consequent);
    }

    fn visit_break_statement(&mut self, stmt: &BreakStatement<'a>) {
        self.push_keyword(KeywordType::Break, self.word(stmt.span.start));
    }

    fn visit_continue_statement(&mut self, stmt: &ContinueStatement<'a>) {
        self.push_keyword(KeywordType::Continue, self.word(stmt.span.start));
    }

    fn visit_throw_statement(&mut self, stmt: &ThrowStatement<'a>) {
        self.push_keyword(KeywordType::Throw, self.word(stmt.span.start));
        walk::walk_throw_statement(self, stmt);
    }

    fn visit_try_statement(&mut self, stmt: &TryStatement<'a>) {
        self.push_keyword(KeywordType::Try, self.word(stmt.span.start));
        walk::walk_try_statement(self, stmt);
    }

    fn visit_catch_clause(&mut self, clause: &CatchClause<'a>) {
        self.push_keyword(KeywordType::Catch, self.word(clause.span.start));
        walk::walk_catch_clause(self, clause);
    }

    fn visit_block_statement(&mut self, block: &BlockStatement<'a>) {
        self.push_punc(PunctuationType::OpenBrace, head(block.span));
        walk::walk_block_statement(self, block);
        self.push_punc(PunctuationType::CloseBrace, tail(block.span));
    }

    fn visit_identifier_reference(&mut self, ident: &IdentifierReference<'a>) {
        self.push(TokenKind::Identifier(ident.name.to_string()), ident.span);
    }

    fn visit_binding_identifier(&mut self, ident: &BindingIdentifier<'a>) {
        self.push(TokenKind::Identifier(ident.name.to_string()), ident.span);
    }

    fn visit_string_literal(&mut self, lit: &StringLiteral<'a>) {
        self.push(
            TokenKind::StringLiteral(lit.value.text().into_owned()),
            lit.span,
        );
    }

    fn visit_numeric_literal(&mut self, lit: &NumericLiteral<'a>) {
        let raw_str = lit
            .raw
            .as_ref()
            .map_or_else(|| lit.value.to_string(), ToString::to_string);
        self.push(TokenKind::NumericLiteral(raw_str), lit.span);
    }

    fn visit_boolean_literal(&mut self, lit: &BooleanLiteral) {
        self.push(TokenKind::BooleanLiteral(lit.value), lit.span);
    }

    fn visit_null_literal(&mut self, lit: &NullLiteral) {
        self.push(TokenKind::NullLiteral, lit.span);
    }

    fn visit_template_literal(&mut self, lit: &TemplateLiteral<'a>) {
        self.push(TokenKind::TemplateLiteral, lit.span);
        walk::walk_template_literal(self, lit);
    }

    fn visit_reg_exp_literal(&mut self, lit: &RegExpLiteral<'a>) {
        self.push(TokenKind::RegExpLiteral, lit.span);
    }

    fn visit_this_expression(&mut self, expr: &ThisExpression) {
        self.push_keyword(KeywordType::This, expr.span);
    }

    fn visit_super(&mut self, expr: &Super) {
        self.push_keyword(KeywordType::Super, expr.span);
    }

    fn visit_array_expression(&mut self, expr: &ArrayExpression<'a>) {
        self.push_punc(PunctuationType::OpenBracket, head(expr.span));
        walk::walk_array_expression(self, expr);
        self.push_punc(PunctuationType::CloseBracket, tail(expr.span));
    }

    fn visit_object_expression(&mut self, expr: &ObjectExpression<'a>) {
        self.push_punc(PunctuationType::OpenBrace, head(expr.span));
        walk::walk_object_expression(self, expr);
        self.push_punc(PunctuationType::CloseBrace, tail(expr.span));
    }

    fn visit_call_expression(&mut self, expr: &CallExpression<'a>) {
        let functions_before = self.function_spans.len();
        self.visit_expression(&expr.callee);
        let open = point_span(expr.callee.span().end);
        self.push_punc(PunctuationType::OpenParen, open);
        for arg in &expr.arguments {
            self.visit_argument(arg);
            let comma = point_span(arg.span().end);
            self.push_op(OperatorType::Comma, comma);
        }
        let close = point_span(expr.span.end.saturating_sub(1));
        self.push_punc(PunctuationType::CloseParen, close);
        self.push_atomic_call_spans(expr.span, functions_before, &expr.arguments);
    }

    fn visit_new_expression(&mut self, expr: &NewExpression<'a>) {
        let functions_before = self.function_spans.len();
        self.push_keyword(KeywordType::New, self.word(expr.span.start));
        self.visit_expression(&expr.callee);
        let open = point_span(expr.callee.span().end);
        self.push_punc(PunctuationType::OpenParen, open);
        for arg in &expr.arguments {
            self.visit_argument(arg);
            let comma = point_span(arg.span().end);
            self.push_op(OperatorType::Comma, comma);
        }
        let close = point_span(expr.span.end.saturating_sub(1));
        self.push_punc(PunctuationType::CloseParen, close);
        self.push_atomic_call_spans(expr.span, functions_before, &expr.arguments);
    }

    fn visit_static_member_expression(&mut self, expr: &StaticMemberExpression<'a>) {
        self.visit_expression(&expr.object);
        let dot = point_span(expr.object.span().end);
        self.push_punc(PunctuationType::Dot, dot);
        self.push(
            TokenKind::Identifier(expr.property.name.to_string()),
            expr.property.span,
        );
    }

    fn visit_computed_member_expression(&mut self, expr: &ComputedMemberExpression<'a>) {
        self.visit_expression(&expr.object);
        let open = point_span(expr.object.span().end);
        self.push_punc(PunctuationType::OpenBracket, open);
        self.visit_expression(&expr.expression);
        let close = point_span(expr.span.end.saturating_sub(1));
        self.push_punc(PunctuationType::CloseBracket, close);
    }

    fn visit_assignment_expression(&mut self, expr: &AssignmentExpression<'a>) {
        self.visit_assignment_target(&expr.left);
        let op = match expr.operator {
            AssignmentOperator::Assign => OperatorType::Assign,
            AssignmentOperator::Addition => OperatorType::AddAssign,
            AssignmentOperator::Subtraction => OperatorType::SubAssign,
            AssignmentOperator::Multiplication => OperatorType::MulAssign,
            AssignmentOperator::Division => OperatorType::DivAssign,
            AssignmentOperator::Remainder => OperatorType::ModAssign,
            AssignmentOperator::Exponential => OperatorType::ExpAssign,
            AssignmentOperator::LogicalAnd => OperatorType::AndAssign,
            AssignmentOperator::LogicalOr => OperatorType::OrAssign,
            AssignmentOperator::LogicalNullish => OperatorType::NullishAssign,
            AssignmentOperator::BitwiseAnd => OperatorType::BitwiseAndAssign,
            AssignmentOperator::BitwiseOR => OperatorType::BitwiseOrAssign,
            AssignmentOperator::BitwiseXOR => OperatorType::BitwiseXorAssign,
            AssignmentOperator::ShiftLeft => OperatorType::ShiftLeftAssign,
            AssignmentOperator::ShiftRight => OperatorType::ShiftRightAssign,
            AssignmentOperator::ShiftRightZeroFill => OperatorType::UnsignedShiftRightAssign,
        };
        self.push_op(
            op,
            self.next_text(expr.left.span().end, expr.operator.as_str()),
        );
        self.visit_expression(&expr.right);
    }

    fn visit_binary_expression(&mut self, expr: &BinaryExpression<'a>) {
        self.visit_expression(&expr.left);
        let op = match expr.operator {
            BinaryOperator::Addition => OperatorType::Add,
            BinaryOperator::Subtraction => OperatorType::Sub,
            BinaryOperator::Multiplication => OperatorType::Mul,
            BinaryOperator::Division => OperatorType::Div,
            BinaryOperator::Remainder => OperatorType::Mod,
            BinaryOperator::Exponential => OperatorType::Exp,
            BinaryOperator::Equality => OperatorType::Eq,
            BinaryOperator::Inequality => OperatorType::NEq,
            BinaryOperator::StrictEquality => OperatorType::StrictEq,
            BinaryOperator::StrictInequality => OperatorType::StrictNEq,
            BinaryOperator::LessThan => OperatorType::Lt,
            BinaryOperator::GreaterThan => OperatorType::Gt,
            BinaryOperator::LessEqualThan => OperatorType::LtEq,
            BinaryOperator::GreaterEqualThan => OperatorType::GtEq,
            BinaryOperator::BitwiseAnd => OperatorType::BitwiseAnd,
            BinaryOperator::BitwiseOR => OperatorType::BitwiseOr,
            BinaryOperator::BitwiseXOR => OperatorType::BitwiseXor,
            BinaryOperator::ShiftLeft => OperatorType::ShiftLeft,
            BinaryOperator::ShiftRight => OperatorType::ShiftRight,
            BinaryOperator::ShiftRightZeroFill => OperatorType::UnsignedShiftRight,
            BinaryOperator::Instanceof => OperatorType::Instanceof,
            BinaryOperator::In => OperatorType::In,
        };
        self.push_op(
            op,
            self.next_text(expr.left.span().end, expr.operator.as_str()),
        );
        self.visit_expression(&expr.right);
    }

    fn visit_logical_expression(&mut self, expr: &LogicalExpression<'a>) {
        self.visit_expression(&expr.left);
        let op = match expr.operator {
            LogicalOperator::And => OperatorType::And,
            LogicalOperator::Or => OperatorType::Or,
            LogicalOperator::Coalesce => OperatorType::NullishCoalescing,
        };
        self.push_op(
            op,
            self.next_text(expr.left.span().end, expr.operator.as_str()),
        );
        self.visit_expression(&expr.right);
    }

    fn visit_unary_expression(&mut self, expr: &UnaryExpression<'a>) {
        let op = match expr.operator {
            UnaryOperator::UnaryPlus => OperatorType::Add,
            UnaryOperator::UnaryNegation => OperatorType::Sub,
            UnaryOperator::LogicalNot => OperatorType::Not,
            UnaryOperator::BitwiseNot => OperatorType::BitwiseNot,
            UnaryOperator::Typeof => {
                self.push_keyword(KeywordType::Typeof, self.word(expr.span.start));
                walk::walk_unary_expression(self, expr);
                return;
            }
            UnaryOperator::Void => {
                self.push_keyword(KeywordType::Void, self.word(expr.span.start));
                walk::walk_unary_expression(self, expr);
                return;
            }
            UnaryOperator::Delete => {
                self.push_keyword(KeywordType::Delete, self.word(expr.span.start));
                walk::walk_unary_expression(self, expr);
                return;
            }
        };
        self.push_op(op, self.text_at(expr.span.start, expr.operator.as_str()));
        walk::walk_unary_expression(self, expr);
    }

    fn visit_update_expression(&mut self, expr: &UpdateExpression<'a>) {
        let op = match expr.operator {
            UpdateOperator::Increment => OperatorType::Increment,
            UpdateOperator::Decrement => OperatorType::Decrement,
        };
        let text = expr.operator.as_str();
        if expr.prefix {
            self.push_op(op, self.text_at(expr.span.start, text));
        }
        walk::walk_update_expression(self, expr);
        if !expr.prefix {
            let start = expr.span.end.saturating_sub(len_u32(text));
            self.push_op(op, self.text_at(start, text));
        }
    }

    fn visit_conditional_expression(&mut self, expr: &ConditionalExpression<'a>) {
        self.visit_expression(&expr.test);
        self.push_op(
            OperatorType::Ternary,
            self.next_text(expr.test.span().end, "?"),
        );
        self.visit_expression(&expr.consequent);
        self.push_punc(
            PunctuationType::Colon,
            self.next_text(expr.consequent.span().end, ":"),
        );
        self.visit_expression(&expr.alternate);
    }

    fn visit_arrow_function_expression(&mut self, expr: &ArrowFunctionExpression<'a>) {
        self.function_spans.push(expr.span);
        if expr.r#async {
            self.push_keyword(KeywordType::Async, self.word(expr.span.start));
        }
        let params_span = expr.params.span;
        self.push_punc(PunctuationType::OpenParen, point_span(params_span.start));
        for param in &expr.params.items {
            self.visit_binding_pattern(&param.pattern);
            self.push_op(OperatorType::Comma, point_span(param.span.end));
        }
        self.push_punc(
            PunctuationType::CloseParen,
            point_span(params_span.end.saturating_sub(1)),
        );
        // The return type tokens come after the arrow token, so with a return
        // type the arrow keeps an empty span at the end of the parameters.
        self.push_op(OperatorType::Arrow, self.next_text(params_span.end, "=>"));
        walk::walk_arrow_function_expression(self, expr);
    }

    fn visit_yield_expression(&mut self, expr: &YieldExpression<'a>) {
        self.push_keyword(KeywordType::Yield, self.word(expr.span.start));
        walk::walk_yield_expression(self, expr);
    }

    fn visit_await_expression(&mut self, expr: &AwaitExpression<'a>) {
        self.push_keyword(KeywordType::Await, self.word(expr.span.start));
        walk::walk_await_expression(self, expr);
    }

    fn visit_spread_element(&mut self, elem: &SpreadElement<'a>) {
        self.push_op(OperatorType::Spread, self.text_at(elem.span.start, "..."));
        walk::walk_spread_element(self, elem);
    }

    fn visit_sequence_expression(&mut self, expr: &SequenceExpression<'a>) {
        let mut previous: Option<Span> = None;
        for sub_expr in &expr.expressions {
            if let Some(previous) = previous {
                self.push_op(OperatorType::Comma, self.next_text(previous.end, ","));
            }
            self.visit_expression(sub_expr);
            previous = Some(sub_expr.span());
        }
    }

    fn visit_function(&mut self, func: &Function<'a>, flags: ScopeFlags) {
        self.function_spans.push(func.span);
        // A method has no `function` keyword in the source, and the function
        // span of a method starts at its parameters, so these keyword tokens
        // can have an empty span.
        let function_start = if func.r#async {
            let async_span = self.keyword_text(func.span.start, "async");
            self.push_keyword(KeywordType::Async, async_span);
            len_u32_from(skip_trivia(self.source, async_span.end as usize))
        } else {
            func.span.start
        };
        self.push_keyword(
            KeywordType::Function,
            self.keyword_text(function_start, "function"),
        );
        if let Some(id) = &func.id {
            self.push(TokenKind::Identifier(id.name.to_string()), id.span);
        }
        let params_span = func.params.span;
        self.push_punc(PunctuationType::OpenParen, point_span(params_span.start));
        for param in &func.params.items {
            self.visit_binding_pattern(&param.pattern);
            self.push_op(OperatorType::Comma, point_span(param.span.end));
        }
        self.push_punc(
            PunctuationType::CloseParen,
            point_span(params_span.end.saturating_sub(1)),
        );
        walk::walk_function(self, func, flags);
    }

    fn visit_class(&mut self, class: &Class<'a>) {
        self.push_keyword(KeywordType::Class, self.word(class.span.start));
        if let Some(id) = &class.id {
            self.push(TokenKind::Identifier(id.name.to_string()), id.span);
        }
        if let Some(heritage) = &class.heritage {
            let extends = self.prev_text(heritage.expression.span().start, "extends");
            self.push_keyword(KeywordType::Extends, extends);
        }
        walk::walk_class(self, class);
    }

    fn visit_import_declaration(&mut self, decl: &ImportDeclaration<'a>) {
        if self.skip_imports {
            return;
        }
        if self.strip_types && decl.import_kind.is_type() {
            return;
        }
        self.push_keyword(KeywordType::Import, self.word(decl.span.start));
        walk::walk_import_declaration(self, decl);
        self.push_keyword(
            KeywordType::From,
            self.prev_text(decl.source.span.start, "from"),
        );
        self.push(
            TokenKind::StringLiteral(decl.source.value.text().into_owned()),
            decl.source.span,
        );
    }

    fn visit_export_declaration(&mut self, decl: &ExportDeclaration<'a>) {
        if self.strip_types && decl.export_kind().is_type() {
            return;
        }
        self.push_keyword(KeywordType::Export, self.word(decl.span.start));
        walk::walk_export_declaration(self, decl);
    }

    fn visit_export_named_declaration(&mut self, decl: &ExportNamedDeclaration<'a>) {
        if self.strip_types && decl.export_kind.is_type() {
            return;
        }
        self.push_keyword(KeywordType::Export, self.word(decl.span.start));
        walk::walk_export_named_declaration(self, decl);
    }

    fn visit_export_from_declaration(&mut self, decl: &ExportFromDeclaration<'a>) {
        if self.skip_imports {
            return;
        }
        if self.strip_types && decl.export_kind.is_type() {
            return;
        }
        self.push_keyword(KeywordType::Export, self.word(decl.span.start));
        walk::walk_export_from_declaration(self, decl);
    }

    fn visit_export_default_declaration(&mut self, decl: &ExportDefaultDeclaration<'a>) {
        let export = self.word(decl.span.start);
        self.push_keyword(KeywordType::Export, export);
        self.push_keyword(KeywordType::Default, self.next_text(export.end, "default"));
        walk::walk_export_default_declaration(self, decl);
    }

    fn visit_export_all_declaration(&mut self, decl: &ExportAllDeclaration<'a>) {
        if self.skip_imports {
            return;
        }
        self.push_keyword(KeywordType::Export, self.word(decl.span.start));
        self.push_keyword(
            KeywordType::From,
            self.prev_text(decl.source.span.start, "from"),
        );
        self.push(
            TokenKind::StringLiteral(decl.source.value.text().into_owned()),
            decl.source.span,
        );
    }

    fn visit_ts_interface_declaration(&mut self, decl: &TSInterfaceDeclaration<'a>) {
        if self.strip_types {
            return;
        }
        self.push_keyword(KeywordType::Interface, self.word(decl.span.start));
        walk::walk_ts_interface_declaration(self, decl);
    }

    fn visit_ts_interface_body(&mut self, body: &TSInterfaceBody<'a>) {
        self.push_punc(PunctuationType::OpenBrace, head(body.span));
        walk::walk_ts_interface_body(self, body);
        self.push_punc(PunctuationType::CloseBrace, tail(body.span));
    }

    fn visit_ts_type_alias_declaration(&mut self, decl: &TSTypeAliasDeclaration<'a>) {
        if self.strip_types {
            return;
        }
        self.push_keyword(KeywordType::Type, self.word(decl.span.start));
        walk::walk_ts_type_alias_declaration(self, decl);
    }

    fn visit_ts_namespace_declaration(&mut self, decl: &TSNamespaceDeclaration<'a>) {
        if self.strip_types && decl.declare {
            return;
        }
        walk::walk_ts_namespace_declaration(self, decl);
    }

    fn visit_ts_external_module_declaration(&mut self, decl: &TSExternalModuleDeclaration<'a>) {
        if self.strip_types && decl.declare {
            return;
        }
        walk::walk_ts_external_module_declaration(self, decl);
    }

    fn visit_ts_enum_declaration(&mut self, decl: &TSEnumDeclaration<'a>) {
        self.push_keyword(KeywordType::Enum, self.word(decl.span.start));
        walk::walk_ts_enum_declaration(self, decl);
    }

    fn visit_ts_enum_body(&mut self, body: &TSEnumBody<'a>) {
        self.push_punc(PunctuationType::OpenBrace, head(body.span));
        walk::walk_ts_enum_body(self, body);
        self.push_punc(PunctuationType::CloseBrace, tail(body.span));
    }

    fn visit_ts_property_signature(&mut self, sig: &TSPropertySignature<'a>) {
        walk::walk_ts_property_signature(self, sig);
        self.push_punc(PunctuationType::Semicolon, tail(sig.span));
    }

    fn visit_ts_type_annotation(&mut self, ann: &TSTypeAnnotation<'a>) {
        if self.strip_types {
            return;
        }
        self.push_punc(PunctuationType::Colon, head(ann.span));
        walk::walk_ts_type_annotation(self, ann);
    }

    fn visit_ts_type_parameter_declaration(&mut self, decl: &TSTypeParameterDeclaration<'a>) {
        if self.strip_types {
            return;
        }
        walk::walk_ts_type_parameter_declaration(self, decl);
    }

    fn visit_ts_type_parameter_instantiation(&mut self, inst: &TSTypeParameterInstantiation<'a>) {
        if self.strip_types {
            return;
        }
        walk::walk_ts_type_parameter_instantiation(self, inst);
    }

    fn visit_ts_as_expression(&mut self, expr: &TSAsExpression<'a>) {
        self.visit_expression(&expr.expression);
        if !self.strip_types {
            self.push_keyword(
                KeywordType::As,
                self.next_text(expr.expression.span().end, "as"),
            );
            self.visit_ts_type(&expr.type_annotation);
        }
    }

    fn visit_ts_satisfies_expression(&mut self, expr: &TSSatisfiesExpression<'a>) {
        self.visit_expression(&expr.expression);
        if !self.strip_types {
            self.push_keyword(
                KeywordType::Satisfies,
                self.next_text(expr.expression.span().end, "satisfies"),
            );
            self.visit_ts_type(&expr.type_annotation);
        }
    }

    fn visit_ts_non_null_expression(&mut self, expr: &TSNonNullExpression<'a>) {
        self.visit_expression(&expr.expression);
    }

    fn visit_identifier_name(&mut self, ident: &IdentifierName<'a>) {
        self.push(TokenKind::Identifier(ident.name.to_string()), ident.span);
    }

    // Before Oxc 0.151 `import.meta` and `new.target` were one meta-property
    // node with two identifier names, and the interface heritage name was an
    // expression. The overrides below keep the tokens those shapes produced, so
    // clone groups do not change with the parser version.

    fn visit_import_meta(&mut self, it: &ImportMeta) {
        self.push_meta_property_tokens(it.span, "import", "meta");
    }

    fn visit_new_target(&mut self, it: &NewTarget) {
        self.push_meta_property_tokens(it.span, "new", "target");
    }

    fn visit_ts_interface_heritage(&mut self, it: &TSInterfaceHeritage<'a>) {
        self.visit_heritage_type_name(&it.type_name);
        if let Some(type_arguments) = &it.type_arguments {
            self.visit_ts_type_parameter_instantiation(type_arguments);
        }
    }

    fn visit_ts_string_keyword(&mut self, it: &TSStringKeyword) {
        self.push(TokenKind::Identifier("string".to_string()), it.span);
    }

    fn visit_ts_number_keyword(&mut self, it: &TSNumberKeyword) {
        self.push(TokenKind::Identifier("number".to_string()), it.span);
    }

    fn visit_ts_boolean_keyword(&mut self, it: &TSBooleanKeyword) {
        self.push(TokenKind::Identifier("boolean".to_string()), it.span);
    }

    fn visit_ts_any_keyword(&mut self, it: &TSAnyKeyword) {
        self.push(TokenKind::Identifier("any".to_string()), it.span);
    }

    fn visit_ts_void_keyword(&mut self, it: &TSVoidKeyword) {
        self.push(TokenKind::Identifier("void".to_string()), it.span);
    }

    fn visit_ts_null_keyword(&mut self, it: &TSNullKeyword) {
        self.push(TokenKind::NullLiteral, it.span);
    }

    fn visit_ts_undefined_keyword(&mut self, it: &TSUndefinedKeyword) {
        self.push(TokenKind::Identifier("undefined".to_string()), it.span);
    }

    fn visit_ts_never_keyword(&mut self, it: &TSNeverKeyword) {
        self.push(TokenKind::Identifier("never".to_string()), it.span);
    }

    fn visit_ts_unknown_keyword(&mut self, it: &TSUnknownKeyword) {
        self.push(TokenKind::Identifier("unknown".to_string()), it.span);
    }

    fn visit_jsx_opening_element(&mut self, elem: &JSXOpeningElement<'a>) {
        self.push_punc(PunctuationType::OpenBracket, head(elem.span));
        walk::walk_jsx_opening_element(self, elem);
        self.push_punc(PunctuationType::CloseBracket, tail(elem.span));
    }

    fn visit_jsx_closing_element(&mut self, elem: &JSXClosingElement<'a>) {
        self.push_punc(PunctuationType::OpenBracket, head(elem.span));
        walk::walk_jsx_closing_element(self, elem);
        self.push_punc(PunctuationType::CloseBracket, tail(elem.span));
    }

    fn visit_jsx_identifier(&mut self, ident: &JSXIdentifier<'a>) {
        self.push(TokenKind::Identifier(ident.name.to_string()), ident.span);
    }

    fn visit_jsx_spread_attribute(&mut self, attr: &JSXSpreadAttribute<'a>) {
        self.push_op(OperatorType::Spread, head(attr.span));
        walk::walk_jsx_spread_attribute(self, attr);
    }

    fn visit_variable_declarator(&mut self, decl: &VariableDeclarator<'a>) {
        self.visit_binding_pattern(&decl.id);
        if let Some(init) = &decl.init {
            self.push_op(OperatorType::Assign, self.prev_text(init.span().start, "="));
            self.visit_expression(init);
        }
        self.push_punc(PunctuationType::Semicolon, tail(decl.span));
    }

    fn visit_expression_statement(&mut self, stmt: &ExpressionStatement<'a>) {
        self.visit_statement_expression(&stmt.expression, stmt.span);
    }

    fn visit_arrow_function_body(&mut self, body: &ArrowFunctionBody<'a>) {
        match body {
            ArrowFunctionBody::FunctionBody(block) => self.visit_function_body(block),
            // Before Oxc 0.151 a concise body was one expression statement with
            // the span of the expression. Keep its tokens, so clone groups do
            // not change with the parser version.
            match_expression!(ArrowFunctionBody) => {
                let expr = body.to_expression();
                self.visit_statement_expression(expr, expr.span());
            }
        }
    }
}
