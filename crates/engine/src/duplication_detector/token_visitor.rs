//! AST visitor that extracts a flat sequence of normalized tokens.
//!
//! The `TokenExtractor` walks an Oxc AST and emits a `Vec<SourceToken>` suitable
//! for suffix-array based clone detection.

use oxc_ast::ast::*;
use oxc_ast_visit::Visit;
use oxc_ast_visit::walk;
use oxc_span::{GetSpan, Span};
use oxc_syntax::scope::ScopeFlags;

use super::token_types::{
    KeywordType, OperatorType, PunctuationType, SourceToken, TokenKind, point_span,
};

/// AST visitor that extracts a flat sequence of normalized tokens.
pub(super) struct TokenExtractor {
    pub(super) tokens: Vec<SourceToken>,
    pub(super) atomic_invocation_spans: Vec<Span>,
    /// When true, skip TypeScript type annotations, interfaces, and type aliases
    /// to enable cross-language clone detection between .ts and .js files.
    strip_types: bool,
    /// When true, skip all `import` declarations from the token stream to reduce
    /// noise from sorted import blocks that naturally look similar across files.
    skip_imports: bool,
}

impl TokenExtractor {
    pub(super) const fn new(strip_types: bool, skip_imports: bool) -> Self {
        Self {
            tokens: Vec::new(),
            atomic_invocation_spans: Vec::new(),
            strip_types,
            skip_imports,
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
}

fn is_atomic_invocation_expr(expr: &Expression<'_>) -> bool {
    match expr {
        Expression::CallExpression(expr) => is_atomic_call_expression(expr),
        Expression::NewExpression(expr) => is_atomic_new_expression(expr),
        Expression::AwaitExpression(expr) => is_atomic_invocation_expr(&expr.argument),
        Expression::ParenthesizedExpression(expr) => is_atomic_invocation_expr(&expr.expression),
        Expression::TSAsExpression(expr) => is_atomic_invocation_expr(&expr.expression),
        Expression::TSSatisfiesExpression(expr) => is_atomic_invocation_expr(&expr.expression),
        Expression::TSNonNullExpression(expr) => is_atomic_invocation_expr(&expr.expression),
        Expression::ChainExpression(expr) => match &expr.expression {
            ChainElement::CallExpression(call) => is_atomic_call_expression(call),
            _ => false,
        },
        _ => false,
    }
}

fn is_atomic_call_expression(expr: &CallExpression<'_>) -> bool {
    !expr.arguments.iter().any(argument_is_function_like)
}

fn is_atomic_new_expression(expr: &NewExpression<'_>) -> bool {
    !expr.arguments.iter().any(argument_is_function_like)
}

fn argument_is_function_like(arg: &Argument<'_>) -> bool {
    match arg {
        Argument::ArrowFunctionExpression(_) | Argument::FunctionExpression(_) => true,
        Argument::ParenthesizedExpression(expr) => expression_is_function_like(&expr.expression),
        _ => false,
    }
}

fn expression_is_function_like(expr: &Expression<'_>) -> bool {
    match expr {
        Expression::ArrowFunctionExpression(_) | Expression::FunctionExpression(_) => true,
        Expression::ParenthesizedExpression(expr) => expression_is_function_like(&expr.expression),
        Expression::TSAsExpression(expr) => expression_is_function_like(&expr.expression),
        Expression::TSSatisfiesExpression(expr) => expression_is_function_like(&expr.expression),
        Expression::TSNonNullExpression(expr) => expression_is_function_like(&expr.expression),
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

impl<'a> Visit<'a> for TokenExtractor {
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
        self.push_keyword(kw, decl.span);
        walk::walk_variable_declaration(self, decl);
    }

    fn visit_return_statement(&mut self, stmt: &ReturnStatement<'a>) {
        if let Some(argument) = &stmt.argument
            && is_atomic_invocation_expr(argument)
        {
            self.push_atomic_invocation_span(stmt.span);
        }
        self.push_keyword(KeywordType::Return, stmt.span);
        walk::walk_return_statement(self, stmt);
    }

    fn visit_if_statement(&mut self, stmt: &IfStatement<'a>) {
        self.push_keyword(KeywordType::If, stmt.span);
        self.push_punc(PunctuationType::OpenParen, stmt.span);
        self.visit_expression(&stmt.test);
        self.push_punc(PunctuationType::CloseParen, stmt.span);
        self.visit_statement(&stmt.consequent);
        if let Some(alt) = &stmt.alternate {
            self.push_keyword(KeywordType::Else, stmt.span);
            self.visit_statement(alt);
        }
    }

    fn visit_for_statement(&mut self, stmt: &ForStatement<'a>) {
        self.push_keyword(KeywordType::For, stmt.span);
        self.push_punc(PunctuationType::OpenParen, stmt.span);
        walk::walk_for_statement(self, stmt);
        self.push_punc(PunctuationType::CloseParen, stmt.span);
    }

    fn visit_for_in_statement(&mut self, stmt: &ForInStatement<'a>) {
        self.push_keyword(KeywordType::For, stmt.span);
        self.push_punc(PunctuationType::OpenParen, stmt.span);
        self.visit_for_statement_left(&stmt.left);
        self.push_keyword(KeywordType::In, stmt.span);
        self.visit_expression(&stmt.right);
        self.push_punc(PunctuationType::CloseParen, stmt.span);
        self.visit_statement(&stmt.body);
    }

    fn visit_for_of_statement(&mut self, stmt: &ForOfStatement<'a>) {
        self.push_keyword(KeywordType::For, stmt.span);
        self.push_punc(PunctuationType::OpenParen, stmt.span);
        self.visit_for_statement_left(&stmt.left);
        self.push_keyword(KeywordType::Of, stmt.span);
        self.visit_expression(&stmt.right);
        self.push_punc(PunctuationType::CloseParen, stmt.span);
        self.visit_statement(&stmt.body);
    }

    fn visit_while_statement(&mut self, stmt: &WhileStatement<'a>) {
        self.push_keyword(KeywordType::While, stmt.span);
        self.push_punc(PunctuationType::OpenParen, stmt.span);
        walk::walk_while_statement(self, stmt);
        self.push_punc(PunctuationType::CloseParen, stmt.span);
    }

    fn visit_do_while_statement(&mut self, stmt: &DoWhileStatement<'a>) {
        self.push_keyword(KeywordType::Do, stmt.span);
        walk::walk_do_while_statement(self, stmt);
    }

    fn visit_switch_statement(&mut self, stmt: &SwitchStatement<'a>) {
        self.push_keyword(KeywordType::Switch, stmt.span);
        self.push_punc(PunctuationType::OpenParen, stmt.span);
        walk::walk_switch_statement(self, stmt);
        self.push_punc(PunctuationType::CloseParen, stmt.span);
    }

    fn visit_switch_case(&mut self, case: &SwitchCase<'a>) {
        if case.test.is_some() {
            self.push_keyword(KeywordType::Case, case.span);
        } else {
            self.push_keyword(KeywordType::Default, case.span);
        }
        self.push_punc(PunctuationType::Colon, case.span);
        walk::walk_switch_case(self, case);
    }

    fn visit_break_statement(&mut self, stmt: &BreakStatement<'a>) {
        self.push_keyword(KeywordType::Break, stmt.span);
    }

    fn visit_continue_statement(&mut self, stmt: &ContinueStatement<'a>) {
        self.push_keyword(KeywordType::Continue, stmt.span);
    }

    fn visit_throw_statement(&mut self, stmt: &ThrowStatement<'a>) {
        self.push_keyword(KeywordType::Throw, stmt.span);
        walk::walk_throw_statement(self, stmt);
    }

    fn visit_try_statement(&mut self, stmt: &TryStatement<'a>) {
        self.push_keyword(KeywordType::Try, stmt.span);
        walk::walk_try_statement(self, stmt);
    }

    fn visit_catch_clause(&mut self, clause: &CatchClause<'a>) {
        self.push_keyword(KeywordType::Catch, clause.span);
        walk::walk_catch_clause(self, clause);
    }

    fn visit_block_statement(&mut self, block: &BlockStatement<'a>) {
        self.push_punc(PunctuationType::OpenBrace, block.span);
        walk::walk_block_statement(self, block);
        self.push_punc(PunctuationType::CloseBrace, block.span);
    }

    fn visit_identifier_reference(&mut self, ident: &IdentifierReference<'a>) {
        self.push(TokenKind::Identifier(ident.name.to_string()), ident.span);
    }

    fn visit_binding_identifier(&mut self, ident: &BindingIdentifier<'a>) {
        self.push(TokenKind::Identifier(ident.name.to_string()), ident.span);
    }

    fn visit_string_literal(&mut self, lit: &StringLiteral<'a>) {
        self.push(TokenKind::StringLiteral(lit.value.to_string()), lit.span);
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
        self.push_punc(PunctuationType::OpenBracket, expr.span);
        walk::walk_array_expression(self, expr);
        self.push_punc(PunctuationType::CloseBracket, expr.span);
    }

    fn visit_object_expression(&mut self, expr: &ObjectExpression<'a>) {
        self.push_punc(PunctuationType::OpenBrace, expr.span);
        walk::walk_object_expression(self, expr);
        self.push_punc(PunctuationType::CloseBrace, expr.span);
    }

    fn visit_call_expression(&mut self, expr: &CallExpression<'a>) {
        if is_atomic_call_expression(expr) {
            self.push_atomic_invocation_span(expr.span);
        }
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
    }

    fn visit_new_expression(&mut self, expr: &NewExpression<'a>) {
        if is_atomic_new_expression(expr) {
            self.push_atomic_invocation_span(expr.span);
        }
        self.push_keyword(KeywordType::New, expr.span);
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
        self.push_op(op, expr.span);
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
        self.push_op(op, expr.span);
        self.visit_expression(&expr.right);
    }

    fn visit_logical_expression(&mut self, expr: &LogicalExpression<'a>) {
        self.visit_expression(&expr.left);
        let op = match expr.operator {
            LogicalOperator::And => OperatorType::And,
            LogicalOperator::Or => OperatorType::Or,
            LogicalOperator::Coalesce => OperatorType::NullishCoalescing,
        };
        self.push_op(op, expr.span);
        self.visit_expression(&expr.right);
    }

    fn visit_unary_expression(&mut self, expr: &UnaryExpression<'a>) {
        let op = match expr.operator {
            UnaryOperator::UnaryPlus => OperatorType::Add,
            UnaryOperator::UnaryNegation => OperatorType::Sub,
            UnaryOperator::LogicalNot => OperatorType::Not,
            UnaryOperator::BitwiseNot => OperatorType::BitwiseNot,
            UnaryOperator::Typeof => {
                self.push_keyword(KeywordType::Typeof, expr.span);
                walk::walk_unary_expression(self, expr);
                return;
            }
            UnaryOperator::Void => {
                self.push_keyword(KeywordType::Void, expr.span);
                walk::walk_unary_expression(self, expr);
                return;
            }
            UnaryOperator::Delete => {
                self.push_keyword(KeywordType::Delete, expr.span);
                walk::walk_unary_expression(self, expr);
                return;
            }
        };
        self.push_op(op, expr.span);
        walk::walk_unary_expression(self, expr);
    }

    fn visit_update_expression(&mut self, expr: &UpdateExpression<'a>) {
        let op = match expr.operator {
            UpdateOperator::Increment => OperatorType::Increment,
            UpdateOperator::Decrement => OperatorType::Decrement,
        };
        if expr.prefix {
            self.push_op(op, expr.span);
        }
        walk::walk_update_expression(self, expr);
        if !expr.prefix {
            self.push_op(op, expr.span);
        }
    }

    fn visit_conditional_expression(&mut self, expr: &ConditionalExpression<'a>) {
        self.visit_expression(&expr.test);
        self.push_op(OperatorType::Ternary, expr.span);
        self.visit_expression(&expr.consequent);
        self.push_punc(PunctuationType::Colon, expr.span);
        self.visit_expression(&expr.alternate);
    }

    fn visit_arrow_function_expression(&mut self, expr: &ArrowFunctionExpression<'a>) {
        if expr.r#async {
            self.push_keyword(KeywordType::Async, expr.span);
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
        self.push_op(OperatorType::Arrow, point_span(params_span.end));
        walk::walk_arrow_function_expression(self, expr);
    }

    fn visit_yield_expression(&mut self, expr: &YieldExpression<'a>) {
        self.push_keyword(KeywordType::Yield, expr.span);
        walk::walk_yield_expression(self, expr);
    }

    fn visit_await_expression(&mut self, expr: &AwaitExpression<'a>) {
        self.push_keyword(KeywordType::Await, expr.span);
        walk::walk_await_expression(self, expr);
    }

    fn visit_spread_element(&mut self, elem: &SpreadElement<'a>) {
        self.push_op(OperatorType::Spread, elem.span);
        walk::walk_spread_element(self, elem);
    }

    fn visit_sequence_expression(&mut self, expr: &SequenceExpression<'a>) {
        for (i, sub_expr) in expr.expressions.iter().enumerate() {
            if i > 0 {
                self.push_op(OperatorType::Comma, expr.span);
            }
            self.visit_expression(sub_expr);
        }
    }

    fn visit_function(&mut self, func: &Function<'a>, flags: ScopeFlags) {
        if func.r#async {
            self.push_keyword(KeywordType::Async, func.span);
        }
        self.push_keyword(KeywordType::Function, func.span);
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
        self.push_keyword(KeywordType::Class, class.span);
        if let Some(id) = &class.id {
            self.push(TokenKind::Identifier(id.name.to_string()), id.span);
        }
        if class.super_class.is_some() {
            self.push_keyword(KeywordType::Extends, class.span);
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
        self.push_keyword(KeywordType::Import, decl.span);
        walk::walk_import_declaration(self, decl);
        self.push_keyword(KeywordType::From, decl.span);
        self.push(
            TokenKind::StringLiteral(decl.source.value.to_string()),
            decl.source.span,
        );
    }

    fn visit_export_named_declaration(&mut self, decl: &ExportNamedDeclaration<'a>) {
        if self.skip_imports && decl.source.is_some() {
            return;
        }
        if self.strip_types && decl.export_kind.is_type() {
            return;
        }
        self.push_keyword(KeywordType::Export, decl.span);
        walk::walk_export_named_declaration(self, decl);
    }

    fn visit_export_default_declaration(&mut self, decl: &ExportDefaultDeclaration<'a>) {
        self.push_keyword(KeywordType::Export, decl.span);
        self.push_keyword(KeywordType::Default, decl.span);
        walk::walk_export_default_declaration(self, decl);
    }

    fn visit_export_all_declaration(&mut self, decl: &ExportAllDeclaration<'a>) {
        if self.skip_imports {
            return;
        }
        self.push_keyword(KeywordType::Export, decl.span);
        self.push_keyword(KeywordType::From, decl.span);
        self.push(
            TokenKind::StringLiteral(decl.source.value.to_string()),
            decl.source.span,
        );
    }

    fn visit_ts_interface_declaration(&mut self, decl: &TSInterfaceDeclaration<'a>) {
        if self.strip_types {
            return;
        }
        self.push_keyword(KeywordType::Interface, decl.span);
        walk::walk_ts_interface_declaration(self, decl);
    }

    fn visit_ts_interface_body(&mut self, body: &TSInterfaceBody<'a>) {
        self.push_punc(PunctuationType::OpenBrace, body.span);
        walk::walk_ts_interface_body(self, body);
        self.push_punc(PunctuationType::CloseBrace, body.span);
    }

    fn visit_ts_type_alias_declaration(&mut self, decl: &TSTypeAliasDeclaration<'a>) {
        if self.strip_types {
            return;
        }
        self.push_keyword(KeywordType::Type, decl.span);
        walk::walk_ts_type_alias_declaration(self, decl);
    }

    fn visit_ts_module_declaration(&mut self, decl: &TSModuleDeclaration<'a>) {
        if self.strip_types && decl.declare {
            return;
        }
        walk::walk_ts_module_declaration(self, decl);
    }

    fn visit_ts_enum_declaration(&mut self, decl: &TSEnumDeclaration<'a>) {
        self.push_keyword(KeywordType::Enum, decl.span);
        walk::walk_ts_enum_declaration(self, decl);
    }

    fn visit_ts_enum_body(&mut self, body: &TSEnumBody<'a>) {
        self.push_punc(PunctuationType::OpenBrace, body.span);
        walk::walk_ts_enum_body(self, body);
        self.push_punc(PunctuationType::CloseBrace, body.span);
    }

    fn visit_ts_property_signature(&mut self, sig: &TSPropertySignature<'a>) {
        walk::walk_ts_property_signature(self, sig);
        self.push_punc(PunctuationType::Semicolon, sig.span);
    }

    fn visit_ts_type_annotation(&mut self, ann: &TSTypeAnnotation<'a>) {
        if self.strip_types {
            return;
        }
        self.push_punc(PunctuationType::Colon, ann.span);
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
            self.push_keyword(KeywordType::As, expr.span);
            self.visit_ts_type(&expr.type_annotation);
        }
    }

    fn visit_ts_satisfies_expression(&mut self, expr: &TSSatisfiesExpression<'a>) {
        self.visit_expression(&expr.expression);
        if !self.strip_types {
            self.push_keyword(KeywordType::Satisfies, expr.span);
            self.visit_ts_type(&expr.type_annotation);
        }
    }

    fn visit_ts_non_null_expression(&mut self, expr: &TSNonNullExpression<'a>) {
        self.visit_expression(&expr.expression);
    }

    fn visit_identifier_name(&mut self, ident: &IdentifierName<'a>) {
        self.push(TokenKind::Identifier(ident.name.to_string()), ident.span);
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
        self.push_punc(PunctuationType::OpenBracket, elem.span);
        walk::walk_jsx_opening_element(self, elem);
        self.push_punc(PunctuationType::CloseBracket, elem.span);
    }

    fn visit_jsx_closing_element(&mut self, elem: &JSXClosingElement<'a>) {
        self.push_punc(PunctuationType::OpenBracket, elem.span);
        walk::walk_jsx_closing_element(self, elem);
        self.push_punc(PunctuationType::CloseBracket, elem.span);
    }

    fn visit_jsx_identifier(&mut self, ident: &JSXIdentifier<'a>) {
        self.push(TokenKind::Identifier(ident.name.to_string()), ident.span);
    }

    fn visit_jsx_spread_attribute(&mut self, attr: &JSXSpreadAttribute<'a>) {
        self.push_op(OperatorType::Spread, attr.span);
        walk::walk_jsx_spread_attribute(self, attr);
    }

    fn visit_variable_declarator(&mut self, decl: &VariableDeclarator<'a>) {
        self.visit_binding_pattern(&decl.id);
        if let Some(init) = &decl.init {
            self.push_op(OperatorType::Assign, decl.span);
            self.visit_expression(init);
        }
        self.push_punc(PunctuationType::Semicolon, decl.span);
    }

    fn visit_expression_statement(&mut self, stmt: &ExpressionStatement<'a>) {
        if is_atomic_invocation_expr(&stmt.expression) {
            self.push_atomic_invocation_span(stmt.span);
        }
        walk::walk_expression_statement(self, stmt);
        self.push_punc(PunctuationType::Semicolon, stmt.span);
    }
}
