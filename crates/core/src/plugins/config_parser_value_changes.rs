//! Find the names whose value the program changes through another name.
//!
//! A config can change a value through an alias: `const r = common.resolve;
//! r.alias.a = x` changes `common`, and `const config = { entry };
//! config.entry.admin = x` changes `entry`. This module follows each change
//! along a key path, so `config.devtool = x` or `config.plugins.push(x)` does
//! not change `entry`.
//!
//! Only a change that the file shows counts: a write, a known mutating call
//! such as `push` or `Object.assign`, and a call to a function of the same
//! file whose body changes its parameter. A call to an imported or unknown
//! function does not count. A missed change can only hide an unused file, but
//! a change that is not real reports a used file as unused.

#[allow(clippy::wildcard_imports, reason = "many AST types used")]
use oxc_ast::ast::*;
use oxc_ast_visit::{Visit, walk};
use oxc_syntax::scope::ScopeFlags;
use rustc_hash::{FxHashMap, FxHashSet};

use super::mutated_target;

/// The keys of a member chain after its root name, in source order. `None`
/// stands for a computed key, which can be any key.
type Chain = Vec<Option<String>>;

/// The maximum number of chain keys that one change keeps. A longer chain
/// keeps its first keys, so a recursive function such as
/// `function mark(o) { o.seen = true; mark(o.next); }` ends with a bounded set
/// of changes.
const MAX_CHAIN: usize = 32;

/// The maximum number of steps that one analysis takes: each change that it
/// records and each alias that it follows counts as one step.
const MAX_STEPS: usize = 200_000;

/// The maximum number of aliases that one analysis collects.
const MAX_EDGES: usize = 20_000;

/// One change of a value, with the key chain under the changed name.
#[derive(Clone, PartialEq, Eq, Hash)]
enum Change {
    /// A write to a member, such as `x.a.b = 1` (chain `[a, b]`).
    Write(Chain),
    /// A mutating call on the value at the chain, such as `x.a.push(1)`.
    Mutation(Chain),
}

/// One step of the way that an object or array literal holds a value.
#[derive(Clone)]
enum Hold {
    /// A property with a static key, as `entry` in `{ entry }`.
    Key(String),
    /// An array element or a computed key, which can be any key.
    Any,
    /// A spread, as in `{ ...common }`. The copy has the same keys, but a
    /// write to a key of the copy does not change the spread value. The list
    /// holds the static keys that the same literal sets after the spread, as
    /// `plugins` in `{ ...common, plugins: [] }`: the copy does not share the
    /// value of such a key with the spread value.
    Spread(Vec<String>),
}

/// A name that holds the value of `root`, or a part of it. A change of `name`
/// with chain `c` is a change of the value at `prefix ++ c`. That value holds
/// the root value under `holds`, and `part` is the member chain from the root
/// value, as `[plugins]` in `const p = config.plugins`.
struct Edge {
    name: String,
    root: String,
    prefix: Chain,
    holds: Vec<Hold>,
    part: Chain,
}

impl Edge {
    /// The change of `root` that a change of `name` makes, if any.
    fn translate(&self, change: &Change) -> Option<Change> {
        let (chain, is_write) = match change {
            Change::Write(chain) => (chain, true),
            Change::Mutation(chain) => (chain, false),
        };
        let full: Chain = self.prefix.iter().chain(chain).cloned().collect();
        let mut index = 0;
        let mut shallow = 0;
        for hold in &self.holds {
            match hold {
                Hold::Key(key) => {
                    let found = full.get(index)?;
                    if found.as_ref().is_some_and(|found| found != key) {
                        return None;
                    }
                    index += 1;
                }
                Hold::Any => {
                    full.get(index)?;
                    index += 1;
                }
                Hold::Spread(overridden) => {
                    let first = full.get(index)?;
                    if first
                        .as_ref()
                        .is_some_and(|key| overridden.iter().any(|own| own == key))
                    {
                        return None;
                    }
                    shallow = 1;
                }
            }
        }
        let rest = &full[index..];
        let needed = if is_write { 1 + shallow } else { shallow };
        if rest.len() < needed {
            return None;
        }
        let chain: Chain = self
            .part
            .iter()
            .chain(rest)
            .take(MAX_CHAIN)
            .cloned()
            .collect();
        Some(if is_write {
            Change::Write(chain)
        } else {
            Change::Mutation(chain)
        })
    }
}

/// The names that the program changes, directly, through an alias or through
/// a call to a local function. A name of the program that no change reaches
/// is not in the set.
///
/// When the program has more than [`MAX_EDGES`] aliases, or the analysis
/// needs more than [`MAX_STEPS`] steps, the result is empty: no change
/// through an alias counts, and the caller reads the literal values. A missed
/// change can only hide an unused file.
pub(super) fn changed_names(program: &Program<'_>) -> FxHashSet<String> {
    let mut functions = FunctionFinder::default();
    functions.visit_program(program);
    let local: FxHashMap<String, Option<usize>> = functions
        .functions
        .into_iter()
        .filter(|(name, _)| functions.bindings.get(name) == Some(&1))
        .collect();
    let mut collector = Collector {
        local: &local,
        scopes: Vec::new(),
        next_function: None,
        edges: Vec::new(),
        changes: Vec::new(),
        overflow: false,
    };
    collector.visit_program(program);
    if collector.overflow {
        return FxHashSet::default();
    }
    propagate(&collector.edges, collector.changes).unwrap_or_default()
}

/// Follow each change along the aliases to the roots. Returns `None` when the
/// analysis stops at [`MAX_STEPS`].
fn propagate(edges: &[Edge], changes: Vec<(String, Change)>) -> Option<FxHashSet<String>> {
    let mut edges_by_name: FxHashMap<&str, Vec<&Edge>> = FxHashMap::default();
    for edge in edges {
        edges_by_name
            .entry(edge.name.as_str())
            .or_default()
            .push(edge);
    }
    let mut steps = changes.len();
    if steps > MAX_STEPS {
        return None;
    }
    let mut seen: FxHashSet<(String, Change)> = changes.iter().cloned().collect();
    let mut pending = changes;
    while let Some((name, change)) = pending.pop() {
        for edge in edges_by_name.get(name.as_str()).into_iter().flatten() {
            steps += 1;
            if steps > MAX_STEPS {
                return None;
            }
            let Some(next) = edge.translate(&change) else {
                continue;
            };
            let next = (edge.root.clone(), next);
            if seen.insert(next.clone()) {
                pending.push(next);
            }
        }
    }
    Some(seen.into_iter().map(|(name, _)| name).collect())
}

/// The functions of the program that a name binds, with the argument index
/// of the rest parameter, and the number of bindings of each name.
#[derive(Default)]
struct FunctionFinder {
    functions: Vec<(String, Option<usize>)>,
    bindings: FxHashMap<String, usize>,
}

impl<'a> Visit<'a> for FunctionFinder {
    fn visit_binding_identifier(&mut self, identifier: &BindingIdentifier<'a>) {
        *self
            .bindings
            .entry(identifier.name.to_string())
            .or_default() += 1;
    }

    fn visit_function(&mut self, func: &Function<'a>, flags: ScopeFlags) {
        if let Some(id) = &func.id
            && func.is_declaration()
        {
            self.functions
                .push((id.name.to_string(), rest_index(&func.params)));
        }
        walk::walk_function(self, func, flags);
    }

    fn visit_variable_declarator(&mut self, declarator: &VariableDeclarator<'a>) {
        if let (BindingPattern::BindingIdentifier(id), Some(init)) =
            (&declarator.id, &declarator.init)
            && let Some(params) = function_params(init)
        {
            self.functions
                .push((id.name.to_string(), rest_index(params)));
        }
        walk::walk_variable_declarator(self, declarator);
    }
}

/// The parameters of a function or arrow value.
fn function_params<'e, 'a>(value: &'e Expression<'a>) -> Option<&'e FormalParameters<'a>> {
    match value.get_inner_expression() {
        Expression::ArrowFunctionExpression(arrow) => Some(&arrow.params),
        Expression::FunctionExpression(func) => Some(&func.params),
        _ => None,
    }
}

fn rest_index(params: &FormalParameters<'_>) -> Option<usize> {
    params.rest.as_ref().map(|_| params.items.len())
}

/// The name of the value that a call passes to argument `index` of a local
/// function. No source identifier contains `#`.
fn argument_name(function: &str, index: usize) -> String {
    format!("{function}#{index}")
}

/// Collects the aliases and the direct changes of the program.
///
/// Inside the body of a local function, each parameter name stands for the
/// value of its argument: a change of the parameter is a change of the
/// argument name of that function. A call of the function binds each
/// argument name to the argument value. So a body that changes its parameter
/// changes the value that each call passes, and a body that only reads it
/// changes nothing.
struct Collector<'l> {
    /// The rest argument index of each local function, by name.
    local: &'l FxHashMap<String, Option<usize>>,
    /// The name that each parameter stands for, one map for each function
    /// that the walk is in.
    scopes: Vec<FxHashMap<String, String>>,
    /// The name of the local function that the next function value binds.
    next_function: Option<String>,
    edges: Vec<Edge>,
    changes: Vec<(String, Change)>,
    /// Set when the program has more than [`MAX_EDGES`] aliases.
    overflow: bool,
}

impl Collector<'_> {
    fn push_edge(&mut self, edge: Edge) {
        if self.edges.len() >= MAX_EDGES {
            self.overflow = true;
            return;
        }
        self.edges.push(edge);
    }

    /// The name that `name` stands for in the current function.
    fn resolve(&self, name: &str) -> String {
        self.scopes
            .iter()
            .rev()
            .find_map(|scope| scope.get(name))
            .map_or_else(|| name.to_owned(), Clone::clone)
    }

    /// Record that `name`, at `prefix`, holds `value`.
    fn bind(&mut self, name: &str, prefix: &Chain, value: &Expression<'_>) {
        let name = self.resolve(name);
        self.bind_held(&name, prefix, value, &mut Vec::new(), &[]);
    }

    fn bind_held(
        &mut self,
        name: &str,
        prefix: &Chain,
        value: &Expression<'_>,
        holds: &mut Vec<Hold>,
        suffix: &[Option<String>],
    ) {
        match value.without_parentheses().get_inner_expression() {
            Expression::LogicalExpression(logical) => {
                self.bind_held(name, prefix, &logical.left, holds, suffix);
                self.bind_held(name, prefix, &logical.right, holds, suffix);
            }
            Expression::ConditionalExpression(conditional) => {
                self.bind_held(name, prefix, &conditional.consequent, holds, suffix);
                self.bind_held(name, prefix, &conditional.alternate, holds, suffix);
            }
            Expression::AssignmentExpression(assign) => {
                self.bind_held(name, prefix, &assign.right, holds, suffix);
            }
            Expression::SequenceExpression(sequence) => {
                if let Some(last) = sequence.expressions.last() {
                    self.bind_held(name, prefix, last, holds, suffix);
                }
            }
            Expression::ArrayExpression(array) => {
                holds.push(Hold::Any);
                for element in &array.elements {
                    match element {
                        ArrayExpressionElement::SpreadElement(spread) => {
                            // `[...list]` holds each element of `list`.
                            let suffix: Chain = std::iter::once(None)
                                .chain(suffix.iter().cloned())
                                .collect();
                            self.bind_held(name, prefix, &spread.argument, holds, &suffix);
                        }
                        element => {
                            if let Some(expr) = element.as_expression() {
                                self.bind_held(name, prefix, expr, holds, suffix);
                            }
                        }
                    }
                }
                holds.pop();
            }
            Expression::ObjectExpression(object) => {
                for (position, property) in object.properties.iter().enumerate() {
                    match property {
                        ObjectPropertyKind::ObjectProperty(property) => {
                            let hold = static_property_key(property)
                                .map_or(Hold::Any, |key| Hold::Key(key.to_owned()));
                            holds.push(hold);
                            self.bind_held(name, prefix, &property.value, holds, suffix);
                            holds.pop();
                        }
                        ObjectPropertyKind::SpreadProperty(spread) => {
                            let literal = matches!(
                                spread.argument.get_inner_expression(),
                                Expression::ObjectExpression(_)
                            );
                            if !literal {
                                let overridden = object.properties[position + 1..]
                                    .iter()
                                    .filter_map(|later| match later {
                                        ObjectPropertyKind::ObjectProperty(later) => {
                                            static_property_key(later).map(str::to_owned)
                                        }
                                        ObjectPropertyKind::SpreadProperty(_) => None,
                                    })
                                    .collect();
                                holds.push(Hold::Spread(overridden));
                            }
                            self.bind_held(name, prefix, &spread.argument, holds, suffix);
                            if !literal {
                                holds.pop();
                            }
                        }
                    }
                }
            }
            other => {
                if let Some((root, keys)) = member_chain(other) {
                    let root = self.resolve(root);
                    self.push_edge(Edge {
                        name: name.to_owned(),
                        root,
                        prefix: prefix.clone(),
                        holds: holds.clone(),
                        part: keys.into_iter().chain(suffix.iter().cloned()).collect(),
                    });
                }
            }
        }
    }

    /// Bind each name of a binding pattern to `value`.
    fn bind_pattern(&mut self, pattern: &BindingPattern<'_>, value: &Expression<'_>) {
        let mut names = Vec::new();
        pattern_names(pattern, &mut Vec::new(), &mut names);
        for (name, prefix) in names {
            self.bind(&name, &prefix, value);
        }
    }

    fn push_change(&mut self, root: &str, change: Change) {
        let root = self.resolve(root);
        self.changes.push((root, change));
    }

    fn record_mutation(&mut self, expr: &Expression<'_>) {
        if let Some((root, chain)) = member_chain(expr) {
            self.push_change(root, Change::Mutation(chain));
        }
    }

    fn record_member_write(&mut self, member: &MemberExpression<'_>) {
        let Some((root, mut chain)) = member_chain(member.object()) else {
            return;
        };
        chain.push(member_key(member));
        self.push_change(root, Change::Write(chain));
    }

    /// A call of a local function binds each argument name to its value.
    fn bind_arguments(&mut self, callee: &Expression<'_>, arguments: &[Argument<'_>]) {
        let Expression::Identifier(id) = callee.without_parentheses() else {
            return;
        };
        if self.resolve(id.name.as_str()) != id.name.as_str() {
            return;
        }
        let Some(&rest) = self.local.get(id.name.as_str()) else {
            return;
        };
        for (index, argument) in arguments.iter().enumerate() {
            let value = match argument {
                Argument::SpreadElement(spread) => &spread.argument,
                argument => match argument.as_expression() {
                    Some(expr) => expr,
                    None => continue,
                },
            };
            let target = rest.map_or(index, |rest| index.min(rest));
            let name = argument_name(id.name.as_str(), target);
            self.bind_held(&name, &Vec::new(), value, &mut Vec::new(), &[]);
        }
    }

    /// Enter a function: map each parameter name to the argument name of a
    /// local function, or to itself for any other function, so that an outer
    /// parameter with the same name does not apply.
    fn enter_function(&mut self, local: Option<&str>, params: &FormalParameters<'_>) {
        let mut scope = FxHashMap::default();
        let rest = rest_index(params);
        let items = params
            .items
            .iter()
            .map(|item| &item.pattern)
            .chain(params.rest.iter().map(|rest| &rest.rest.argument));
        for (index, pattern) in items.enumerate() {
            let mut names = Vec::new();
            pattern_names(pattern, &mut Vec::new(), &mut names);
            for (name, prefix) in names {
                let Some(function) = local else {
                    scope.insert(name.clone(), name);
                    continue;
                };
                let argument = argument_name(function, index);
                let stand_in = format!("{argument}#{name}");
                // A rest parameter holds each later argument as an element.
                let holds = if rest == Some(index) {
                    vec![Hold::Any]
                } else {
                    Vec::new()
                };
                self.push_edge(Edge {
                    name: stand_in.clone(),
                    root: argument,
                    prefix,
                    holds,
                    part: Vec::new(),
                });
                scope.insert(name, stand_in);
            }
        }
        self.scopes.push(scope);
    }

    /// The local function name of a function: its own declared name, or the
    /// name of the binding whose value it is.
    fn local_name(&mut self, own: Option<&BindingIdentifier<'_>>) -> Option<String> {
        let bound = self.next_function.take();
        own.map(|id| id.name.to_string())
            .or(bound)
            .filter(|name| self.local.contains_key(name))
    }
}

impl<'a> Visit<'a> for Collector<'_> {
    fn visit_variable_declarator(&mut self, declarator: &VariableDeclarator<'a>) {
        if let Some(init) = &declarator.init {
            self.bind_pattern(&declarator.id, init);
            if let BindingPattern::BindingIdentifier(id) = &declarator.id
                && function_params(init).is_some()
            {
                self.next_function = Some(id.name.to_string());
            }
        }
        walk::walk_variable_declarator(self, declarator);
        self.next_function = None;
    }

    fn visit_function(&mut self, func: &Function<'a>, flags: ScopeFlags) {
        let own = func.id.as_ref().filter(|_| func.is_declaration());
        let local = self.local_name(own);
        self.enter_function(local.as_deref(), &func.params);
        walk::walk_function(self, func, flags);
        self.scopes.pop();
    }

    fn visit_arrow_function_expression(&mut self, arrow: &ArrowFunctionExpression<'a>) {
        let local = self.local_name(None);
        self.enter_function(local.as_deref(), &arrow.params);
        walk::walk_arrow_function_expression(self, arrow);
        self.scopes.pop();
    }

    fn visit_assignment_expression(&mut self, assign: &AssignmentExpression<'a>) {
        if let AssignmentTarget::AssignmentTargetIdentifier(target) = &assign.left {
            self.bind(target.name.as_str(), &Vec::new(), &assign.right);
        } else if let Some(member) = assign
            .left
            .as_simple_assignment_target()
            .and_then(SimpleAssignmentTarget::as_member_expression)
        {
            self.record_member_write(member);
        } else if assign.left.as_assignment_target_pattern().is_some() {
            // A destructuring assignment binds each target name to a part of
            // the value, and writes each member target.
            let mut names = Vec::new();
            let mut members = Vec::new();
            pattern_targets(&assign.left, &mut names, &mut members);
            for name in names {
                self.bind(&name, &vec![None], &assign.right);
            }
            for member in members {
                self.record_member_write(member);
            }
        }
        walk::walk_assignment_expression(self, assign);
    }

    fn visit_update_expression(&mut self, expression: &UpdateExpression<'a>) {
        if let Some(member) = expression.argument.as_member_expression() {
            self.record_member_write(member);
        }
        walk::walk_update_expression(self, expression);
    }

    fn visit_unary_expression(&mut self, expression: &UnaryExpression<'a>) {
        if expression.operator == UnaryOperator::Delete
            && let Some(member) = expression.argument.as_member_expression()
        {
            self.record_member_write(member);
        }
        walk::walk_unary_expression(self, expression);
    }

    fn visit_call_expression(&mut self, call: &CallExpression<'a>) {
        if let Some(target) = mutated_target(call) {
            self.record_mutation(target);
        }
        self.bind_arguments(&call.callee, &call.arguments);
        walk::walk_call_expression(self, call);
    }

    fn visit_for_of_statement(&mut self, statement: &ForOfStatement<'a>) {
        // `for (const c of list)` binds `c` to each element of `list`.
        let mut names = Vec::new();
        match &statement.left {
            ForStatementLeft::VariableDeclaration(decl) => {
                for declarator in &decl.declarations {
                    pattern_names(&declarator.id, &mut Vec::new(), &mut names);
                }
            }
            left => {
                if let Some(target) = left.as_assignment_target() {
                    names.extend(
                        target_names(target)
                            .into_iter()
                            .map(|name| (name, Vec::new())),
                    );
                }
            }
        }
        for (name, prefix) in names {
            let name = self.resolve(&name);
            match statement.right.get_inner_expression() {
                Expression::ArrayExpression(array) => {
                    for element in &array.elements {
                        let value = match element {
                            ArrayExpressionElement::SpreadElement(spread) => &spread.argument,
                            element => match element.as_expression() {
                                Some(expr) => expr,
                                None => continue,
                            },
                        };
                        self.bind_held(&name, &prefix, value, &mut Vec::new(), &[]);
                    }
                }
                right => self.bind_held(&name, &prefix, right, &mut Vec::new(), &[None]),
            }
        }
        walk::walk_for_of_statement(self, statement);
    }
}

/// The names that a binding pattern binds, each with its key chain in the
/// bound value. A rest element can hold any part, so it keeps the chain of
/// its pattern.
fn pattern_names(
    pattern: &BindingPattern<'_>,
    prefix: &mut Chain,
    names: &mut Vec<(String, Chain)>,
) {
    match pattern {
        BindingPattern::BindingIdentifier(id) => {
            names.push((id.name.to_string(), prefix.clone()));
        }
        BindingPattern::ObjectPattern(object) => {
            for property in &object.properties {
                let key = if property.computed {
                    None
                } else {
                    match &property.key {
                        PropertyKey::StaticIdentifier(id) => Some(id.name.to_string()),
                        PropertyKey::StringLiteral(literal) => {
                            literal.value.as_str().map(str::to_owned)
                        }
                        _ => None,
                    }
                };
                prefix.push(key);
                pattern_names(&property.value, prefix, names);
                prefix.pop();
            }
            if let Some(rest) = &object.rest {
                pattern_names(&rest.argument, prefix, names);
            }
        }
        BindingPattern::ArrayPattern(array) => {
            for element in array.elements.iter().flatten() {
                prefix.push(None);
                pattern_names(element, prefix, names);
                prefix.pop();
            }
            if let Some(rest) = &array.rest {
                pattern_names(&rest.argument, prefix, names);
            }
        }
        BindingPattern::AssignmentPattern(assignment) => {
            pattern_names(&assignment.left, prefix, names);
        }
    }
}

/// The identifier names in an assignment target, such as `r` and `a` in
/// `{ resolve: r, a }`. A name in a default value or a computed key is only
/// read, so it is not a target.
fn target_names(target: &AssignmentTarget<'_>) -> Vec<String> {
    let mut names = Vec::new();
    pattern_targets(target, &mut names, &mut Vec::new());
    names
}

/// The identifier targets and the member targets of an assignment target.
fn pattern_targets<'e>(
    target: &'e AssignmentTarget<'_>,
    names: &mut Vec<String>,
    members: &mut Vec<&'e MemberExpression<'e>>,
) {
    match target {
        AssignmentTarget::AssignmentTargetIdentifier(id) => names.push(id.name.to_string()),
        AssignmentTarget::ArrayAssignmentTarget(array) => {
            for element in array.elements.iter().flatten() {
                maybe_default_targets(element, names, members);
            }
            if let Some(rest) = &array.rest {
                pattern_targets(&rest.target, names, members);
            }
        }
        AssignmentTarget::ObjectAssignmentTarget(object) => {
            for property in &object.properties {
                match property {
                    AssignmentTargetProperty::AssignmentTargetPropertyIdentifier(shorthand) => {
                        names.push(shorthand.binding.name.to_string());
                    }
                    AssignmentTargetProperty::AssignmentTargetPropertyProperty(property) => {
                        maybe_default_targets(&property.binding, names, members);
                    }
                }
            }
            if let Some(rest) = &object.rest {
                pattern_targets(&rest.target, names, members);
            }
        }
        other => {
            if let Some(member) = other
                .as_simple_assignment_target()
                .and_then(SimpleAssignmentTarget::as_member_expression)
            {
                members.push(member);
            }
        }
    }
}

fn maybe_default_targets<'e>(
    target: &'e AssignmentTargetMaybeDefault<'_>,
    names: &mut Vec<String>,
    members: &mut Vec<&'e MemberExpression<'e>>,
) {
    match target {
        AssignmentTargetMaybeDefault::AssignmentTargetWithDefault(with_default) => {
            pattern_targets(&with_default.binding, names, members);
        }
        other => {
            if let Some(target) = other.as_assignment_target() {
                pattern_targets(target, names, members);
            }
        }
    }
}

/// The key of a property that is not computed, such as `entry` in `{ entry }`
/// or in `{ "entry": value }`.
fn static_property_key<'p>(property: &'p ObjectProperty<'_>) -> Option<&'p str> {
    if property.computed {
        return None;
    }
    match &property.key {
        PropertyKey::StaticIdentifier(id) => Some(id.name.as_str()),
        PropertyKey::StringLiteral(literal) => literal.value.as_str(),
        _ => None,
    }
}

/// The root name of a member chain and its keys in source order, such as
/// `config` and `[entry, admin]` for `config.entry.admin`. A computed key with
/// a string literal is a static key, and another computed key is `None`.
fn member_chain<'e>(expr: &'e Expression<'_>) -> Option<(&'e str, Chain)> {
    let mut keys = Vec::new();
    let mut current = expr.without_parentheses().get_inner_expression();
    loop {
        let member = match current {
            Expression::Identifier(identifier) => {
                keys.reverse();
                return Some((identifier.name.as_str(), keys));
            }
            Expression::ChainExpression(chain) => chain.expression.as_member_expression()?,
            other => other.as_member_expression()?,
        };
        keys.push(member_key(member));
        current = member.object().without_parentheses().get_inner_expression();
    }
}

fn member_key(member: &MemberExpression<'_>) -> Option<String> {
    member
        .static_property_name()
        .and_then(|name| name.as_str().map(str::to_owned))
}
