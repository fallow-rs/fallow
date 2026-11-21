//! One canonical view of a call or a `new` expression.
//!
//! The security sinks, the security controls and the framework taint sources
//! read a call through [`CanonicalCall`]. The view holds the receiver chain of
//! the callee (with `this` and call-result segments), the call form, and every
//! argument with its kind. One flattener thus gives the same callee path to
//! `a.b.m(x)`, `new a.b.m(x)`, `this.b.m(x)` and `a().m(x)`.

use std::borrow::Cow;

use oxc_ast::ast::*;
use oxc_span::Span;

use fallow_types::extract::SinkShape;

use super::super::super::ModuleInfoExtractor;
use super::{sink_literal_value, unwrap_static_expr};

/// The most call results that one receiver chain can hold. The visitor
/// reads each call of a fluent chain (`a.b().c().d()`), and each read walks
/// back through the earlier calls, so a bound keeps the work linear in the
/// chain length.
const MAX_CALL_RESULTS: usize = 8;

/// The root segment that stands for the earlier part of a chain past
/// [`MAX_CALL_RESULTS`]. It is not an identifier, so no exact catalogue
/// pattern names it, and a `*.` pattern still matches the tail of the chain.
const OPAQUE_CHAIN_ROOT: &str = "<chain>()";

/// The syntactic form of a call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::visitor) enum CallForm {
    /// `f(x)` or `a.f(x)`.
    Call,
    /// `new F(x)` or `new a.F(x)`.
    New,
}

/// The first segment of a receiver chain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::visitor) enum ReceiverRoot {
    /// A bare identifier (`document` in `document.write`).
    Identifier,
    /// `this` (`this.sanitizer.bypassSecurityTrustHtml`).
    This,
}

/// A receiver chain flattened to a dotted path.
///
/// A segment that a call returns keeps a `()` suffix, so
/// `$("#p").html` becomes `$().html` and `document.createRange()` becomes
/// `document.createRange()`. A catalogue pattern with a leading `*.` matches
/// the tail of the path, and an exact pattern never names a `()` segment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::visitor) struct ReceiverPath {
    pub(in crate::visitor) path: String,
    pub(in crate::visitor) root: ReceiverRoot,
    /// Whether a segment of the chain is a call result.
    pub(in crate::visitor) through_call: bool,
}

impl ReceiverPath {
    /// Whether the path names a binding and static members only: no `this`
    /// root and no call result. Only a static path can resolve through a
    /// local alias or a module binding.
    pub(in crate::visitor) fn is_static(&self) -> bool {
        self.root == ReceiverRoot::Identifier && !self.through_call
    }
    /// Whether the path is a member of `this` itself (`this.query`). Such a
    /// call runs code of the class or of its base in most cases, so it
    /// matches only the catalogue rows that set `this_receiver`.
    pub(in crate::visitor) fn is_own_member(&self) -> bool {
        self.root == ReceiverRoot::This
            && !self.through_call
            && self.path.bytes().filter(|byte| *byte == b'.').count() == 1
    }
}

/// The kind of one call argument.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::visitor) enum CallArgKind {
    /// A plain expression argument.
    Expression,
    /// A spread argument (`...xs`). Its position does not map to one
    /// parameter.
    Spread,
}

/// One argument of a [`CanonicalCall`].
#[derive(Clone, Copy)]
pub(in crate::visitor) struct CallArg<'c, 'a> {
    pub(in crate::visitor) index: u32,
    pub(in crate::visitor) expr: &'c Expression<'a>,
    pub(in crate::visitor) kind: CallArgKind,
}

/// A call or a `new` expression with its callee resolved to the catalogue
/// path.
pub(in crate::visitor) struct CanonicalCall<'c, 'a> {
    pub(in crate::visitor) form: CallForm,
    /// The callee path that the catalogue matches. A static path also
    /// resolves code-executing aliases and Node.js module bindings.
    pub(in crate::visitor) path: String,
    /// The written receiver chain of the callee.
    pub(in crate::visitor) receiver: ReceiverPath,
    pub(in crate::visitor) arguments: &'c [Argument<'a>],
    pub(in crate::visitor) span: Span,
}

impl<'c, 'a> CanonicalCall<'c, 'a> {
    /// The sink shape of the call: `new` expressions are
    /// [`SinkShape::NewExpression`], and a call is a member call when its path
    /// has a member segment.
    pub(in crate::visitor) fn sink_shape(&self) -> SinkShape {
        match self.form {
            CallForm::New => SinkShape::NewExpression,
            CallForm::Call if self.path.contains('.') => SinkShape::MemberCall,
            CallForm::Call => SinkShape::Call,
        }
    }

    /// The last member name of the callee (`html` for `$().html`).
    pub(in crate::visitor) fn method(&self) -> Option<&str> {
        self.path.rsplit_once('.').map(|(_, method)| method)
    }

    /// Every argument, in source order, with its kind.
    pub(in crate::visitor) fn args(&self) -> impl Iterator<Item = CallArg<'c, 'a>> + 'c {
        self.arguments
            .iter()
            .enumerate()
            .filter_map(|(index, arg)| {
                let index = u32::try_from(index).ok()?;
                Some(match arg {
                    Argument::SpreadElement(spread) => CallArg {
                        index,
                        expr: &spread.argument,
                        kind: CallArgKind::Spread,
                    },
                    other => CallArg {
                        index,
                        expr: other.as_expression()?,
                        kind: CallArgKind::Expression,
                    },
                })
            })
    }
}

impl ModuleInfoExtractor {
    /// The canonical view of a call. Returns `None` when the callee is not a
    /// receiver chain (a computed key that does not fold to a constant, a
    /// private field, a call of a call result such as `f()(x)`), so the caller
    /// can record a blind spot.
    pub(in crate::visitor) fn canonical_call<'c, 'a>(
        &self,
        callee: &Expression<'a>,
        arguments: &'c [Argument<'a>],
        form: CallForm,
        span: Span,
    ) -> Option<CanonicalCall<'c, 'a>> {
        if is_call_expression(callee) {
            return None;
        }
        let receiver = self.receiver_path(callee)?;
        let path = if receiver.is_static() {
            self.resolve_static_callee_path(receiver.path.clone())
        } else {
            receiver.path.clone()
        };
        Some(CanonicalCall {
            form,
            path,
            receiver,
            arguments,
            span,
        })
    }

    /// Flatten a callee or an assignment object to a [`ReceiverPath`].
    ///
    /// Accepts identifiers, `this`, static members, computed members whose
    /// key folds to a constant string (`win["ev" + "al"]`), optional chains,
    /// TypeScript wrappers (`el!`, `el as T`), call results, and the indirect
    /// call sequence `(0, eval)` whose leading expressions are literals. Past
    /// [`MAX_CALL_RESULTS`] call results, the earlier part of the chain
    /// becomes the opaque root [`OPAQUE_CHAIN_ROOT`].
    pub(in crate::visitor) fn receiver_path(&self, expr: &Expression<'_>) -> Option<ReceiverPath> {
        let mut segments: Vec<String> = Vec::new();
        let mut calls = 0_usize;
        let mut call_results = 0_usize;
        let mut expr = expr;
        let root = loop {
            crate::cost::walk_step();
            let at_start = segments.is_empty() && calls == 0;
            let (segment, next) = match self.receiver_step(expr, at_start)? {
                ReceiverStep::Call(callee) => {
                    calls += 1;
                    call_results += 1;
                    if call_results > MAX_CALL_RESULTS {
                        segments.push(OPAQUE_CHAIN_ROOT.to_string());
                        break ReceiverRoot::Identifier;
                    }
                    expr = callee;
                    continue;
                }
                ReceiverStep::Unwrap(inner) => {
                    expr = inner;
                    continue;
                }
                ReceiverStep::Segment(segment, next) => (segment, next),
            };
            let this_root = matches!(unwrap_static_expr(expr), Expression::ThisExpression(_));
            let mut segment = segment.into_owned();
            for _ in 0..calls {
                segment.push_str("()");
            }
            calls = 0;
            segments.push(segment);
            match next {
                Some(object) => expr = object,
                None if this_root => break ReceiverRoot::This,
                None => break ReceiverRoot::Identifier,
            }
        };
        segments.reverse();
        Some(ReceiverPath {
            path: segments.join("."),
            root,
            through_call: call_results > 0,
        })
    }

    /// One step of [`Self::receiver_path`]: a path segment with the object
    /// that follows it, a call whose callee continues the walk, or a wrapper
    /// whose inner expression continues the walk. `None` for an expression
    /// that has no static receiver path. `at_start` admits the indirect call
    /// sequence `(0, eval)` only as the outermost callee.
    fn receiver_step<'e>(
        &self,
        expr: &'e Expression<'e>,
        at_start: bool,
    ) -> Option<ReceiverStep<'e>> {
        let step = match unwrap_static_expr(expr) {
            Expression::Identifier(ident) => {
                ReceiverStep::Segment(Cow::Borrowed(ident.name.as_str()), None)
            }
            Expression::ThisExpression(_) => ReceiverStep::Segment(Cow::Borrowed("this"), None),
            Expression::StaticMemberExpression(member) => ReceiverStep::Segment(
                Cow::Borrowed(member.property.name.as_str()),
                Some(&member.object),
            ),
            Expression::ComputedMemberExpression(member) => ReceiverStep::Segment(
                Cow::Owned(self.static_property_key(&member.expression)?),
                Some(&member.object),
            ),
            Expression::CallExpression(call) => ReceiverStep::Call(&call.callee),
            Expression::SequenceExpression(seq) if at_start => {
                let (last, leading) = seq.expressions.split_last()?;
                if !leading
                    .iter()
                    .all(|item| sink_literal_value(item).is_some())
                {
                    return None;
                }
                ReceiverStep::Unwrap(last)
            }
            Expression::ChainExpression(chain) => match &chain.expression {
                ChainElement::StaticMemberExpression(member) => ReceiverStep::Segment(
                    Cow::Borrowed(member.property.name.as_str()),
                    Some(&member.object),
                ),
                ChainElement::ComputedMemberExpression(member) => ReceiverStep::Segment(
                    Cow::Owned(self.static_property_key(&member.expression)?),
                    Some(&member.object),
                ),
                ChainElement::CallExpression(call) => ReceiverStep::Call(&call.callee),
                ChainElement::TSNonNullExpression(non_null) => {
                    ReceiverStep::Unwrap(&non_null.expression)
                }
                ChainElement::PrivateFieldExpression(_) => return None,
            },
            _ => return None,
        };
        Some(step)
    }
}

/// One step of the receiver walk in [`ModuleInfoExtractor::receiver_path`].
enum ReceiverStep<'e> {
    /// A path segment, and the object that the walk reads next (`None` at
    /// the root).
    Segment(Cow<'e, str>, Option<&'e Expression<'e>>),
    /// A call; the walk continues with its callee.
    Call(&'e Expression<'e>),
    /// A wrapper; the walk continues with the inner expression.
    Unwrap(&'e Expression<'e>),
}

/// Whether `expr` is itself a call (`f()` in `f()(x)`), behind parentheses,
/// TypeScript wrappers or an optional chain.
fn is_call_expression(expr: &Expression<'_>) -> bool {
    match unwrap_static_expr(expr) {
        Expression::CallExpression(_) => true,
        Expression::ChainExpression(chain) => {
            matches!(chain.expression, ChainElement::CallExpression(_))
        }
        _ => false,
    }
}
