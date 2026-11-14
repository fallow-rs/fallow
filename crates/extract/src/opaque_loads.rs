//! Uses of the value of a load with an unknown source.
//!
//! `import(name)` or `require(name)` can load each module of the project.
//! This pass records each export name that the code reads on such a value:
//! `mod.pad`, `mod["pad"]`, `const { pad } = mod`, `'pad' in mod`, or a
//! `.then((m) => m.pad())` callback. When the value goes to code that the
//! pass does not follow, such as an alias, a call argument or a return
//! value, the pass records an escape. A discarded value reads no export.
//! A value that a module-scope function returns makes that function a
//! loader. The analysis then reads the result of each call of the function
//! with the same rules, so `const mod = await load(name); mod.pad()` reads
//! `pad`.
//! A read of the global `require` as a value, such as `require.call(...)`
//! or `[name].map(require)`, is also an escape: the pass cannot see the
//! loads that go through it.

use fallow_types::extract::{CallResultReads, LoaderReturn, OpaqueLoads};
use oxc_ast::AstKind;
use oxc_ast::ast::{Argument, BinaryOperator, BindingPattern, Expression, FormalParameters};
use oxc_semantic::{AstNodes, NodeId, Reference, Scoping, Semantic, SymbolId};
use oxc_span::{GetSpan, Span};
use oxc_syntax::operator::UnaryOperator;
use oxc_syntax::symbol::SymbolFlags;
use rustc_hash::{FxHashMap, FxHashSet};

use crate::js_text::JsText;
use crate::namespace_member_uses::{destructured_keys, member_chain, unwrap_upward};

/// The module-scope functions that can be a loader: the span of each
/// function with the index of its contract and its `async` flag. A loaded
/// value that another function returns is an escape.
pub type LoaderCandidates = FxHashMap<Span, (u32, bool)>;

/// The most names that the reads of the results of one call keep. A call
/// with more reads counts as an escape. This can only hide a finding, never
/// add one, and it keeps the facts of each call small.
const MAX_RESULT_NAMES: usize = 32;

/// What the reader needs to know about the program besides the AST.
#[derive(Clone, Copy)]
pub struct ReadContext<'c> {
    /// `true` for a component script whose template can read each
    /// top-level binding, such as a Vue `<script setup>` block.
    pub template_reads_root: bool,
    /// The functions whose `return` of a loaded value makes them a loader.
    pub loader_candidates: &'c LoaderCandidates,
}

/// Read the uses of each load whose `import()` or call span is in `loads`.
pub fn collect(
    semantic: &Semantic<'_>,
    loads: &FxHashSet<Span>,
    context: ReadContext<'_>,
) -> OpaqueLoads {
    let mut reader = Reader::new(semantic, context);
    reader.read_require_values();
    if loads.is_empty() {
        return reader.finish();
    }
    let mut found = 0;
    for node in reader.nodes.iter() {
        let (span, promise) = match node.kind() {
            AstKind::ImportExpression(expr) => (expr.span, true),
            AstKind::CallExpression(call) => (call.span, false),
            _ => continue,
        };
        if loads.contains(&span) {
            found += 1;
            reader.read_load(node.id(), span, promise);
        }
    }
    // A load that the pass cannot find has uses that it cannot read.
    reader.escapes |= found < loads.len();
    reader.finish()
}

/// Read the uses of the result of the call at `call`, once as the loaded
/// value and once as a promise of it. When the callee is a loader, the
/// analysis uses the reads that match the loader.
pub fn read_call_result(
    semantic: &Semantic<'_>,
    call: NodeId,
    context: ReadContext<'_>,
) -> CallResultReads {
    let span = semantic.nodes().kind(call).span();
    let read = |promise: bool| {
        let mut reader = Reader::new(semantic, context);
        reader.read_load(call, span, promise);
        let mut reads = reader.finish();
        limit_names(&mut reads);
        reads
    };
    CallResultReads {
        value: read(false),
        promise: read(true),
    }
}

/// Replace the names of call result reads with an escape when they pass
/// [`MAX_RESULT_NAMES`].
pub fn limit_names(reads: &mut OpaqueLoads) {
    if reads.names.len() > MAX_RESULT_NAMES {
        reads.names = Vec::new();
        reads.escapes = true;
    }
}

struct Reader<'s, 'a> {
    scoping: &'s Scoping,
    nodes: &'s AstNodes<'a>,
    names: FxHashSet<String>,
    escapes: bool,
    loaders: FxHashSet<LoaderReturn>,
    template_reads_root: bool,
    loader_candidates: &'s LoaderCandidates,
}

/// The members of `require` that call the loader with other arguments.
const REQUIRE_CALL_FORWARDS: &[&str] = &["call", "apply", "bind"];

impl<'s, 'a> Reader<'s, 'a> {
    fn new(semantic: &'s Semantic<'a>, context: ReadContext<'s>) -> Self {
        Self {
            scoping: semantic.scoping(),
            nodes: semantic.nodes(),
            names: FxHashSet::default(),
            escapes: false,
            loaders: FxHashSet::default(),
            template_reads_root: context.template_reads_root,
            loader_candidates: context.loader_candidates,
        }
    }

    fn finish(self) -> OpaqueLoads {
        let mut names: Vec<String> = self.names.into_iter().collect();
        names.sort_unstable();
        let mut loaders: Vec<LoaderReturn> = self.loaders.into_iter().collect();
        loaders.sort_unstable();
        OpaqueLoads {
            names,
            escapes: self.escapes,
            loaders,
        }
    }

    /// Record an escape for a read of the global `require` that is not a
    /// direct call. An ambient `declare` of `require` has no runtime
    /// binding, so its references also read the global loader.
    fn read_require_values(&mut self) {
        let scoping = self.scoping;
        let unresolved = scoping
            .root_unresolved_references()
            .get("require")
            .into_iter()
            .flatten()
            .map(|&id| scoping.get_reference(id));
        let ambient = scoping
            .get_root_binding("require".into())
            .filter(|&symbol| scoping.symbol_flags(symbol).contains(SymbolFlags::Ambient))
            .into_iter()
            .flat_map(|symbol| scoping.get_resolved_references(symbol));
        let escapes = unresolved
            .chain(ambient)
            .any(|reference| self.require_value_escapes(reference));
        self.escapes |= escapes;
    }

    fn require_value_escapes(&self, reference: &Reference) -> bool {
        if !reference.is_value() {
            return false;
        }
        let node = reference.node_id();
        let span = self.nodes.kind(node).span();
        let (node, span) = unwrap_upward(self.nodes, node, span);
        match self.nodes.parent_kind(node) {
            AstKind::CallExpression(call) => call.callee.span() != span,
            AstKind::NewExpression(new_expr) => new_expr.callee.span() != span,
            AstKind::StaticMemberExpression(member) => {
                REQUIRE_CALL_FORWARDS.contains(&member.property.name.as_str())
            }
            AstKind::UnaryExpression(unary) => unary.operator != UnaryOperator::Typeof,
            AstKind::BinaryExpression(binary) => !binary.operator.is_equality(),
            AstKind::IfStatement(_) | AstKind::ExpressionStatement(_) => false,
            AstKind::ConditionalExpression(cond) => cond.test.span() != span,
            _ => true,
        }
    }

    /// Read one load. An `import()` gives a promise until an `await`.
    fn read_load(&mut self, node: NodeId, span: Span, promise: bool) {
        let (mut node, mut span) = unwrap_upward(self.nodes, node, span);
        let mut promise = promise;
        let parent = self.nodes.parent_id(node);
        if let AstKind::AwaitExpression(await_expr) = self.nodes.kind(parent) {
            (node, span) = unwrap_upward(self.nodes, parent, await_expr.span);
            promise = false;
        }
        if promise {
            self.read_promise(node, span);
        } else {
            self.read_value(node, span, false);
        }
    }

    /// Read a promise of the loaded value: a `.then` callback, a handler
    /// whose result the code discards, or a discarded promise.
    fn read_promise(&mut self, node: NodeId, span: Span) {
        let parent = self.nodes.parent_id(node);
        let AstKind::StaticMemberExpression(member) = self.nodes.kind(parent) else {
            if !self.is_discarded(node) && !self.read_return(node, span, true) {
                self.escapes = true;
            }
            return;
        };
        let call_id = self.nodes.parent_id(parent);
        let AstKind::CallExpression(call) = self.nodes.kind(call_id) else {
            self.escapes = true;
            return;
        };
        if member.object.span() != span || call.callee.span() != member.span {
            self.escapes = true;
            return;
        }
        match member.property.name.as_str() {
            "then" => self.read_then_callback(call.arguments.first()),
            "catch" | "finally" => self.read_load(call_id, call.span, true),
            _ => self.escapes = true,
        }
    }

    /// Read the first callback of `.then()`. A callback that is not an
    /// inline function can read each export.
    fn read_then_callback(&mut self, callback: Option<&Argument<'_>>) {
        match callback {
            None => {}
            Some(Argument::ArrowFunctionExpression(arrow)) => self.read_params(&arrow.params),
            Some(Argument::FunctionExpression(function)) => {
                self.read_params(&function.params);
                if self.reads_arguments(function.span) {
                    self.escapes = true;
                }
            }
            Some(_) => self.escapes = true,
        }
    }

    /// `true` when the non-arrow function at `span` reads its `arguments`
    /// object, which holds the loaded value of a `.then` callback.
    fn reads_arguments(&self, span: Span) -> bool {
        let Some(references) = self.scoping.root_unresolved_references().get("arguments") else {
            return false;
        };
        references.iter().any(|&id| {
            let node = self.scoping.get_reference(id).node_id();
            self.nodes
                .ancestors(node)
                .find_map(|ancestor| match ancestor.kind() {
                    AstKind::Function(function) => Some(function.span == span),
                    _ => None,
                })
                .unwrap_or(false)
        })
    }

    fn read_params(&mut self, params: &FormalParameters<'_>) {
        match params.items.first() {
            Some(param) => self.read_pattern(&param.pattern, true),
            None if params.rest.is_some() => self.escapes = true,
            None => {}
        }
    }

    /// Read the loaded value at `node`. `bound` is `true` for a reference to
    /// a binding of the value, where a second binding is an alias.
    fn read_value(&mut self, node: NodeId, span: Span, bound: bool) {
        let chain = member_chain(self.nodes, node, span);
        if let Some(name) = chain.segments.into_iter().next() {
            self.names.insert(name);
            return;
        }
        let parent = self.nodes.parent_id(node);
        if parent == node {
            return;
        }
        match self.nodes.kind(parent) {
            AstKind::VariableDeclarator(declarator)
                if declarator.init.as_ref().map(GetSpan::span) == Some(span) =>
            {
                if self.is_exported_declarator(parent) && !declarator.id.is_destructuring_pattern()
                {
                    self.escapes = true;
                }
                self.read_pattern(&declarator.id, !bound);
            }
            AstKind::BinaryExpression(binary)
                if binary.operator == BinaryOperator::In && binary.right.span() == span =>
            {
                match binary.left.get_inner_expression() {
                    Expression::StringLiteral(key) => {
                        self.names.insert(key.value.text().into_owned());
                    }
                    _ => self.escapes = true,
                }
            }
            AstKind::BinaryExpression(binary) if binary.operator.is_equality() => {}
            AstKind::UnaryExpression(unary)
                if matches!(
                    unary.operator,
                    UnaryOperator::Typeof | UnaryOperator::Void | UnaryOperator::LogicalNot
                ) => {}
            AstKind::ExpressionStatement(_) if self.is_discarded(node) => {}
            _ => {
                if !self.read_return(node, span, false) {
                    self.escapes = true;
                }
            }
        }
    }

    /// Record a loader when a loader candidate returns the value at `node`,
    /// with a `return` or as the expression body of an arrow function.
    /// `promise` is `true` when the value is a promise of the loaded value.
    fn read_return(&mut self, node: NodeId, span: Span, promise: bool) -> bool {
        let parent = self.nodes.parent_id(node);
        let function = match self.nodes.kind(parent) {
            AstKind::ReturnStatement(statement)
                if statement.argument.as_ref().map(GetSpan::span) == Some(span) =>
            {
                self.nodes
                    .ancestors(parent)
                    .find_map(|ancestor| match ancestor.kind() {
                        AstKind::Function(function) => Some(function.span),
                        AstKind::ArrowFunctionExpression(arrow) => Some(arrow.span),
                        _ => None,
                    })
            }
            AstKind::ArrowFunctionExpression(arrow)
                if arrow.get_expression().map(GetSpan::span) == Some(span) =>
            {
                Some(arrow.span)
            }
            _ => None,
        };
        let Some(&(contract, is_async)) =
            function.and_then(|function| self.loader_candidates.get(&function))
        else {
            return false;
        };
        // An `async` function wraps its return value in a promise.
        self.loaders.insert(LoaderReturn {
            contract,
            promise: promise || is_async,
        });
        true
    }

    /// Read a binding of the loaded value. `alias` is `true` when a plain
    /// identifier binds the value itself, not a copy of another binding.
    fn read_pattern(&mut self, pattern: &BindingPattern<'_>, alias: bool) {
        match pattern {
            BindingPattern::BindingIdentifier(id) if alias => match id.symbol_id.get() {
                Some(symbol) => {
                    if self.template_reads_root
                        && self.scoping.symbol_scope_id(symbol) == self.scoping.root_scope_id()
                    {
                        self.escapes = true;
                    }
                    self.read_binding(symbol);
                }
                None => self.escapes = true,
            },
            BindingPattern::ObjectPattern(_) => match destructured_keys(pattern) {
                Some(keys) => self.names.extend(keys),
                None => self.escapes = true,
            },
            BindingPattern::AssignmentPattern(assign) => self.read_pattern(&assign.left, alias),
            _ => self.escapes = true,
        }
    }

    fn read_binding(&mut self, symbol: SymbolId) {
        for reference in self.scoping.get_resolved_references(symbol) {
            if !reference.is_read() {
                continue;
            }
            let node = reference.node_id();
            let AstKind::IdentifierReference(identifier) = self.nodes.kind(node) else {
                self.escapes = true;
                continue;
            };
            let (node, span) = unwrap_upward(self.nodes, node, identifier.span);
            self.read_value(node, span, true);
        }
    }

    /// `true` for a declarator of `export const x = ...`. Another module can
    /// import the binding and read each export of the loaded value.
    fn is_exported_declarator(&self, declarator: NodeId) -> bool {
        let declaration = self.nodes.parent_id(declarator);
        matches!(
            self.nodes.parent_kind(declaration),
            AstKind::ExportNamedDeclaration(_) | AstKind::ExportDeclaration(_)
        )
    }

    /// `true` when the code discards the value of the expression at `node`:
    /// an expression statement or a `void` operand. The expression body of
    /// an arrow function is not a statement, so it returns the value.
    fn is_discarded(&self, node: NodeId) -> bool {
        match self.nodes.parent_kind(node) {
            AstKind::ExpressionStatement(_) => true,
            AstKind::UnaryExpression(unary) => unary.operator == UnaryOperator::Void,
            _ => false,
        }
    }
}
