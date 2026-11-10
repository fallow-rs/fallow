//! Persisted call evidence for plain functions: the parameter contract of each
//! module-scope function binding, the direct calls of admitted callees, and
//! the references that let a local function escape.
//!
//! A rule reads these facts to report a parameter that gets the same value at
//! each call, or an optional parameter that no caller passes. The facts are
//! syntactic. The analysis resolves each call target through the module graph
//! and abstains when a caller can be invisible.

/// Call evidence of one module.
#[derive(Debug, Clone, Default, PartialEq, Eq, bitcode::Encode, bitcode::Decode)]
pub struct FunctionContractFacts {
    /// One contract for each function that a module-scope binding holds.
    pub declarations: Vec<FunctionContract>,
    /// Direct calls whose callee is an admitted import binding or a local
    /// function binding, one entry per callee, ordered by the first site.
    pub calls: Vec<FunctionCalls>,
    /// References to a local function binding that are not a direct callee,
    /// in source order.
    pub escapes: Vec<LocalFunctionEscape>,
    /// `true` for a Vue, Svelte, Astro, Glimmer or MDX host. A template, an
    /// inline script, or another script block of the file can call a binding,
    /// and the facts do not hold those calls. A consumer must treat each
    /// contract of the file, and each import that the file calls, as open.
    pub template_callers: bool,
}

impl FunctionContractFacts {
    /// Each call site with its callee, ordered by callee and then by site.
    pub fn call_sites(&self) -> impl Iterator<Item = (CallTarget, &str, &CallSite)> {
        self.calls.iter().flat_map(|calls| {
            calls
                .sites
                .iter()
                .map(|site| (calls.target, &*calls.member_path, site))
        })
    }

    /// `true` when the module has no call evidence to store.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.declarations.is_empty()
            && self.calls.is_empty()
            && self.escapes.is_empty()
            && !self.template_callers
    }
}

/// The parameter contract of a function that a module-scope binding holds:
/// `function f() {}`, or `const f = () => {}` and `const f = function () {}`.
#[derive(Debug, Clone, PartialEq, Eq, bitcode::Encode, bitcode::Decode)]
pub struct FunctionContract {
    /// The module-scope binding name.
    pub binding_name: Box<str>,
    /// Start byte of the binding identifier.
    pub span_start: u32,
    /// One fact for each formal parameter, in order. A TypeScript `this`
    /// parameter is not a formal parameter.
    pub params: Vec<ParamFact>,
    /// `true` when a `return` of the function has an expression, or the arrow
    /// function has an expression body.
    pub returns_value: bool,
    /// `true` for an `async` function declaration, function expression or
    /// arrow function. A call of an async function always gives a promise.
    pub is_async: bool,
    /// The first reason found that the extractor cannot see each use of the
    /// parameters. `None` for a closed contract.
    pub incomplete: Option<ContractIncomplete>,
}

/// One formal parameter of a [`FunctionContract`].
#[derive(Debug, Clone, PartialEq, Eq, bitcode::Encode, bitcode::Decode)]
pub struct ParamFact {
    /// The parameter name. `None` for an object or array pattern.
    pub name: Option<Box<str>>,
    /// The binding shape of the parameter.
    pub kind: ParamKind,
    /// `true` for a TypeScript `?` marker.
    pub optional: bool,
    /// `true` when the parameter has a default value.
    pub has_default: bool,
    /// `true` for a rest parameter.
    pub is_rest: bool,
    /// The count of conditions whose test is the parameter or its negation:
    /// the test of an `if`, a `?:`, a loop, or `p`, `!p` and `!!p` there.
    pub condition_reads: u32,
    /// `true` when the module writes the parameter binding: in the body, in
    /// a default value, or in a nested function or arrow, for example
    /// `p = !p`, `p ??= x`, `[p] = xs` or `p++`. For an object or array
    /// pattern, `true` when the module writes any binding of the pattern. A
    /// `var` or function declaration in the body that redeclares the
    /// parameter also counts as a write. A written parameter can hold a
    /// value that no caller passes.
    pub written: bool,
}

/// The binding shape of a [`ParamFact`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, bitcode::Encode, bitcode::Decode)]
pub enum ParamKind {
    /// A plain identifier.
    Identifier,
    /// An object destructuring pattern.
    Object,
    /// An array destructuring pattern.
    Array,
}

/// Why a [`FunctionContract`] is open.
#[derive(Debug, Clone, Copy, PartialEq, Eq, bitcode::Encode, bitcode::Decode)]
pub enum ContractIncomplete {
    /// The function has TypeScript overload signatures.
    Overload,
    /// The body or a default value reads `arguments`.
    ArgumentsRead,
    /// The function calls `eval` directly.
    DirectEval,
    /// A `let` or `var` declaration holds the function.
    MutableBinding,
    /// The module assigns the binding again.
    Reassigned,
    /// A parameter has a decorator.
    Decorator,
    /// The function is a generator.
    Generator,
    /// A second declaration has the same name.
    DuplicateDeclaration,
}

/// The direct calls of one callee: an admitted import binding with a member
/// path, or a local function binding.
///
/// The extractor groups the sites per callee, so the target and the path are
/// stored once. Optional calls, computed callees and calls through bindings
/// that are not admitted are not stored.
#[derive(Debug, Clone, PartialEq, Eq, bitcode::Encode, bitcode::Decode)]
pub struct FunctionCalls {
    /// The binding that the callee names.
    pub target: CallTarget,
    /// Dot-separated static member suffix after an import binding, empty for
    /// a bare call. Always empty for a local target.
    ///
    /// For a namespace import, the first segment is the export name. For a
    /// named or default import of a function, a non-empty path, for example
    /// `call`, `apply` or `bind`, is not a plain call of the function, so a
    /// consumer must treat the function as open for that site.
    pub member_path: Box<str>,
    /// Each call of this callee, in source order. Never empty.
    pub sites: Vec<CallSite>,
}

/// One direct call of a [`FunctionCalls`] callee.
#[derive(Debug, Clone, PartialEq, Eq, bitcode::Encode, bitcode::Decode)]
pub struct CallSite {
    /// The count of arguments, spread arguments included.
    pub arg_count: u32,
    /// One fact for each argument before the first spread, to a fixed
    /// maximum. A string literal longer than a fixed maximum is
    /// [`ArgFact::Unknown`]. A spread moves each later argument to an
    /// unknown parameter, so no fact is kept from the spread on. Empty when
    /// each kept argument is [`ArgFact::Unknown`], so `arg_count` then holds
    /// the shape.
    ///
    /// Do not index this list directly. Use [`CallSite::arg_at`], which
    /// gives one of three outcomes for a parameter position: a known fact,
    /// an unknown value, or an omitted argument.
    pub args: Vec<ArgFact>,
    /// The index of the first spread argument.
    pub spread_from: Option<u32>,
    /// `false` when the call, with or without `await` or `void`, is a whole
    /// expression statement.
    pub result_used: bool,
    /// Start byte of the call expression.
    pub span_start: u32,
}

impl CallSite {
    /// The argument that this call passes at parameter position `index`.
    ///
    /// - From the first spread on, each position is [`ArgAt::Unknown`],
    ///   because a spread can fill any later position.
    /// - A position at or past `arg_count` is [`ArgAt::Omitted`].
    /// - A position with a stored fact other than [`ArgFact::Unknown`] is
    ///   [`ArgAt::Fact`].
    /// - Each other position is [`ArgAt::Unknown`]: a stored unknown fact,
    ///   a position past the kept facts, or a call with an empty `args`.
    #[must_use]
    pub fn arg_at(&self, index: u32) -> ArgAt<'_> {
        if self.spread_from.is_some_and(|spread| index >= spread) {
            return ArgAt::Unknown;
        }
        if index >= self.arg_count {
            return ArgAt::Omitted;
        }
        match usize::try_from(index).ok().and_then(|at| self.args.get(at)) {
            Some(ArgFact::Unknown) | None => ArgAt::Unknown,
            Some(fact) => ArgAt::Fact(fact),
        }
    }
}

/// The argument of a [`CallSite`] at one parameter position. This type is
/// not stored in the cache.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArgAt<'a> {
    /// A known argument fact. Never [`ArgFact::Unknown`].
    Fact(&'a ArgFact),
    /// The call passes a value that the facts do not know.
    Unknown,
    /// The call passes no argument at this position.
    Omitted,
}

/// The binding that a [`FunctionCalls`] callee names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, bitcode::Encode, bitcode::Decode)]
pub enum CallTarget {
    /// Index into `ModuleInfo.imports` of an admitted runtime ESM import.
    Import(u32),
    /// Index into [`FunctionContractFacts::declarations`].
    Local(u32),
}

/// The syntactic value of one call argument.
#[derive(Debug, Clone, PartialEq, Eq, bitcode::Encode, bitcode::Decode)]
pub enum ArgFact {
    /// A literal value.
    Literal(LiteralValue),
    /// The value of the parameter at this index of the called function. Only
    /// a self-call has this fact: a call of a local function inside the
    /// body or a default value of that same function. The argument is the
    /// unchanged identifier of a formal identifier parameter that the module
    /// never writes, so the call passes the value that the outer call got. A
    /// rest parameter never gets this fact, because the callee gets `[r]`
    /// for `f(r)`.
    Param(u32),
    /// Each other expression. `args` never holds a spread argument, because
    /// the list stops before the first spread.
    Unknown,
}

/// A literal argument value in canonical form.
#[derive(Debug, Clone, PartialEq, Eq, bitcode::Encode, bitcode::Decode)]
pub enum LiteralValue {
    /// `true` or `false`.
    Boolean(bool),
    /// `null`.
    Null,
    /// The unshadowed identifier `undefined`.
    Undefined,
    /// The IEEE 754 bits of a numeric literal or its negation, so `1` and
    /// `1.0` are equal. `-0` is stored as `0`, so `-0` and `0` are equal; a
    /// condition reads both as false.
    Number(u64),
    /// The value of a string literal or of a template literal with no
    /// expression.
    String(Box<str>),
}

/// A reference to a local function binding that is not a direct callee, for
/// example `items.map(format)`, `const g = format`, `<Format />` or
/// `` format`x` ``.
#[derive(Debug, Clone, Copy, PartialEq, Eq, bitcode::Encode, bitcode::Decode)]
pub struct LocalFunctionEscape {
    /// Index into [`FunctionContractFacts::declarations`].
    pub contract: u32,
    /// Start byte of the reference.
    pub span_start: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn site(arg_count: u32, args: Vec<ArgFact>, spread_from: Option<u32>) -> CallSite {
        CallSite {
            arg_count,
            args,
            spread_from,
            result_used: false,
            span_start: 0,
        }
    }

    const TRUE: ArgFact = ArgFact::Literal(LiteralValue::Boolean(true));

    #[test]
    fn a_position_from_the_spread_on_is_unknown() {
        let call = site(2, vec![TRUE], Some(1));
        assert_eq!(call.arg_at(0), ArgAt::Fact(&TRUE));
        assert_eq!(call.arg_at(1), ArgAt::Unknown);
        assert_eq!(call.arg_at(3), ArgAt::Unknown);
        assert_eq!(site(1, Vec::new(), Some(0)).arg_at(3), ArgAt::Unknown);
    }

    #[test]
    fn a_position_past_the_count_is_omitted() {
        assert_eq!(site(0, Vec::new(), None).arg_at(0), ArgAt::Omitted);
        assert_eq!(site(1, vec![TRUE], None).arg_at(1), ArgAt::Omitted);
    }

    #[test]
    fn empty_args_with_a_count_are_unknown() {
        let call = site(2, Vec::new(), None);
        assert_eq!(call.arg_at(0), ArgAt::Unknown);
        assert_eq!(call.arg_at(1), ArgAt::Unknown);
        assert_eq!(call.arg_at(2), ArgAt::Omitted);
    }

    #[test]
    fn a_stored_unknown_is_unknown_and_a_param_is_a_fact() {
        let call = site(2, vec![ArgFact::Unknown, ArgFact::Param(0)], None);
        assert_eq!(call.arg_at(0), ArgAt::Unknown);
        assert_eq!(call.arg_at(1), ArgAt::Fact(&ArgFact::Param(0)));
    }

    #[test]
    fn a_position_past_the_kept_facts_is_unknown() {
        // The extractor keeps 16 argument facts and the full count.
        let call = site(40, vec![TRUE; 16], None);
        assert_eq!(call.arg_at(15), ArgAt::Fact(&TRUE));
        assert_eq!(call.arg_at(16), ArgAt::Unknown);
        assert_eq!(call.arg_at(39), ArgAt::Unknown);
        assert_eq!(call.arg_at(40), ArgAt::Omitted);
    }
}
