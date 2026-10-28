//! Put the value of a same-file constant in the config positions that name it.
//!
//! A config often keeps a part of its value in a top-level constant:
//! `const UNIT = { test: { setupFiles: [...] } }` and later
//! `projects: [UNIT]`, or `setupFiles: SETUP`. The config walkers read
//! literal objects, arrays and strings only. This pass replaces each such
//! identifier in a property value or an array element with a copy of the
//! constant's literal value, so that every walker reads it without change.

use oxc_allocator::{Allocator, CloneIn};
#[allow(clippy::wildcard_imports, reason = "many AST types used")]
use oxc_ast::ast::*;
use oxc_ast_visit::{Visit, VisitMut, walk, walk_mut};
use rustc_hash::{FxHashMap, FxHashSet};

use super::top_level_variable_declarations;

/// The maximum number of constants that one inlined value can pass through,
/// as in `const A = {...}; const B = [A]; export default { x: B }`.
pub(super) const MAX_INLINE_NESTING: usize = 4;

/// Replace each identifier in a property value or an array element that names
/// a stable top-level constant with a literal value, and replace each object
/// spread of such a constant with the properties of the constant.
///
/// A constant qualifies when one top-level `const` or `let` is the only
/// binding of its name, no expression writes to it, and its value is an
/// object, array or string literal.
/// The copied AST nodes stay at or below `source_len`, and each node of the
/// parsed source spans at least one byte, so the AST grows at most to two
/// times its parsed size.
pub(super) fn inline_value_constants<'a>(
    allocator: &'a Allocator,
    program: &mut Program<'a>,
    source_len: usize,
) {
    let values = constant_values(allocator, program);
    if values.is_empty() {
        return;
    }
    Inliner::new(allocator, &values, source_len).visit_program(program);
}

/// A copy of the literal value of each stable top-level constant, by name.
pub(super) fn constant_values<'a>(
    allocator: &'a Allocator,
    program: &Program<'a>,
) -> FxHashMap<String, Expression<'a>> {
    let candidates: Vec<_> = top_level_variable_declarations(program)
        .flat_map(|decl| &decl.declarations)
        .filter_map(|declarator| {
            let BindingPattern::BindingIdentifier(id) = &declarator.id else {
                return None;
            };
            let value = declarator.init.as_ref().and_then(literal_value)?;
            Some((id.name.as_str(), value))
        })
        .collect();
    if candidates.is_empty() {
        return FxHashMap::default();
    }
    let stable = stable_values(program, candidates.iter().copied(), true);
    candidates
        .into_iter()
        .filter(|(name, _)| stable.contains(name))
        .map(|(name, value)| (name.to_string(), value.clone_in(allocator)))
        .collect()
}

/// The literal value of a constant initializer, through parentheses and type
/// assertions.
///
/// The value can hold functions and writes. A copy of one adds a binding or a
/// write only for a name that the source already binds or writes inside it, so
/// a later stable-binding check of the same program gives the same answer.
pub(super) fn literal_value<'e, 'a>(init: &'e Expression<'a>) -> Option<&'e Expression<'a>> {
    match init {
        Expression::ParenthesizedExpression(paren) => literal_value(&paren.expression),
        Expression::TSAsExpression(ts_as) => literal_value(&ts_as.expression),
        Expression::TSSatisfiesExpression(ts_sat) => literal_value(&ts_sat.expression),
        Expression::StringLiteral(_)
        | Expression::ObjectExpression(_)
        | Expression::ArrayExpression(_) => Some(init),
        Expression::TemplateLiteral(template) if template.expressions.is_empty() => Some(init),
        _ => None,
    }
}

/// Copies literal values into the positions that name them. Each copy charges
/// its number of AST nodes to `budget`, and a copy inside a copy adds one to
/// `nesting`. A value can hold copies from an earlier pass, so its source span
/// can be much smaller than the copy.
pub(super) struct Inliner<'a, 'v> {
    allocator: &'a Allocator,
    values: &'v FxHashMap<String, Expression<'a>>,
    nesting: usize,
    budget: usize,
    /// The node count of each value or object that was copied, by address.
    /// The values do not change during a pass, so each count is computed once.
    node_counts: FxHashMap<usize, usize>,
}

impl<'a, 'v> Inliner<'a, 'v> {
    pub(super) fn new(
        allocator: &'a Allocator,
        values: &'v FxHashMap<String, Expression<'a>>,
        budget: usize,
    ) -> Self {
        Self {
            allocator,
            values,
            nesting: 0,
            budget,
            node_counts: FxHashMap::default(),
        }
    }

    /// Charge the node count of a value to the budget. Returns `None` when
    /// the budget is too small.
    fn charge_value(&mut self, value: &Expression<'a>) -> Option<()> {
        let key = std::ptr::from_ref(value) as usize;
        let count = *self.node_counts.entry(key).or_insert_with(|| {
            let mut counter = NodeCounter::default();
            counter.visit_expression(value);
            counter.count
        });
        self.budget = self.budget.checked_sub(count)?;
        Some(())
    }

    /// Charge the node count of an object literal to the budget. Returns
    /// `None` when the budget is too small.
    fn charge_object(&mut self, object: &ObjectExpression<'a>) -> Option<()> {
        let key = std::ptr::from_ref(object) as usize;
        let count = *self.node_counts.entry(key).or_insert_with(|| {
            let mut counter = NodeCounter::default();
            counter.visit_object_expression(object);
            counter.count
        });
        self.budget = self.budget.checked_sub(count)?;
        Some(())
    }

    /// Replace `expr` with a copy of the value it names, then inline inside it.
    /// This is for a whole value position, such as a `return` argument.
    pub(super) fn inline_value(&mut self, expr: &mut Expression<'a>) {
        let Some(value) = self.copy_for(expr) else {
            self.visit_expression(expr);
            return;
        };
        *expr = value;
        self.nesting += 1;
        self.visit_expression(expr);
        self.nesting -= 1;
    }

    /// A copy of the constant value that `expr` names, when the nesting and
    /// size limits allow one more copy.
    fn copy_for(&mut self, expr: &Expression<'a>) -> Option<Expression<'a>> {
        if self.nesting >= MAX_INLINE_NESTING {
            return None;
        }
        let Expression::Identifier(id) = expr else {
            return None;
        };
        let value = self.values.get(id.name.as_str())?;
        self.charge_value(value)?;
        Some(value.clone_in(self.allocator))
    }

    /// The object literal that a spread argument names: a constant, or a static
    /// member chain such as `common.resolve` whose root is a constant. The node
    /// count of the object is charged to the budget.
    fn spread_source(&mut self, argument: &Expression<'a>) -> Option<&'v ObjectExpression<'a>> {
        if self.nesting >= MAX_INLINE_NESTING {
            return None;
        }
        let object = static_object(self.values, argument, MAX_INLINE_NESTING)?;
        self.charge_object(object)?;
        Some(object)
    }

    /// Replace each spread of a known object literal with copies of its
    /// properties. Returns, for each resulting property, whether it is a copy.
    /// Returns `None` when no spread resolves, and leaves `obj` unchanged.
    fn splice_spreads(&mut self, obj: &mut ObjectExpression<'a>) -> Option<Vec<bool>> {
        let sources: Vec<_> = obj
            .properties
            .iter()
            .map(|prop| match prop {
                ObjectPropertyKind::SpreadProperty(spread) => self.spread_source(&spread.argument),
                ObjectPropertyKind::ObjectProperty(_) => None,
            })
            .collect();
        if sources.iter().all(Option::is_none) {
            return None;
        }
        let old = std::mem::replace(
            &mut obj.properties,
            oxc_allocator::Vec::new_in(&self.allocator),
        );
        let mut copied = Vec::with_capacity(old.len());
        for (prop, source) in old.into_iter().zip(sources) {
            let Some(source) = source else {
                obj.properties.push(prop);
                copied.push(false);
                continue;
            };
            for source_prop in &source.properties {
                obj.properties.push(source_prop.clone_in(self.allocator));
                copied.push(true);
            }
        }
        drop_overridden_properties(obj, &mut copied);
        Some(copied)
    }
}

/// Counts the AST nodes of a value.
#[derive(Default)]
struct NodeCounter {
    count: usize,
}

impl<'a> Visit<'a> for NodeCounter {
    fn enter_node(&mut self, _kind: oxc_ast::AstKind<'a>) {
        self.count += 1;
    }
}

/// The object literal that `expr` names through the constants in `values`:
/// an identifier, or a static member chain whose root resolves. A member takes
/// the last property with that key, and declines when a spread or a computed
/// key follows it, because that one can override the key.
fn static_object<'v, 'a>(
    values: &'v FxHashMap<String, Expression<'a>>,
    expr: &Expression<'a>,
    hops: usize,
) -> Option<&'v ObjectExpression<'a>> {
    match static_value(values, expr, hops)?.get_inner_expression() {
        Expression::ObjectExpression(object) => Some(object),
        _ => None,
    }
}

fn static_value<'v, 'a>(
    values: &'v FxHashMap<String, Expression<'a>>,
    expr: &Expression<'a>,
    hops: usize,
) -> Option<&'v Expression<'a>> {
    let hops = hops.checked_sub(1)?;
    match expr.get_inner_expression() {
        Expression::Identifier(id) => values.get(id.name.as_str()),
        Expression::StaticMemberExpression(member) => {
            let object = static_object(values, &member.object, hops)?;
            let value = last_static_property(object, member.property.name.as_str())?;
            match value.get_inner_expression() {
                Expression::Identifier(_) => static_value(values, value, hops),
                _ => Some(value),
            }
        }
        _ => None,
    }
}

/// The value of the last plain property `key` of `object`, when no spread or
/// computed key follows it.
fn last_static_property<'v, 'a>(
    object: &'v ObjectExpression<'a>,
    key: &str,
) -> Option<&'v Expression<'a>> {
    let index = object
        .properties
        .iter()
        .rposition(|prop| static_key(prop) == Some(key))?;
    let overridable = object.properties[index + 1..]
        .iter()
        .any(|prop| match prop {
            ObjectPropertyKind::SpreadProperty(_) => true,
            ObjectPropertyKind::ObjectProperty(prop) => prop.computed,
        });
    if overridable {
        return None;
    }
    match &object.properties[index] {
        ObjectPropertyKind::ObjectProperty(prop) => Some(&prop.value),
        ObjectPropertyKind::SpreadProperty(_) => None,
    }
}

/// The key of a plain `key: value` property that is not computed.
fn static_key<'p>(prop: &'p ObjectPropertyKind<'_>) -> Option<&'p str> {
    let ObjectPropertyKind::ObjectProperty(prop) = prop else {
        return None;
    };
    if prop.computed || prop.kind != PropertyKind::Init {
        return None;
    }
    match &prop.key {
        PropertyKey::StaticIdentifier(id) => Some(id.name.as_str()),
        PropertyKey::StringLiteral(literal) => literal.value.as_str(),
        _ => None,
    }
}

/// Remove each plain property that a later plain property with the same key
/// overrides. JavaScript keeps the last value of a key, but the config readers
/// take the first match. A property that only a later unresolved spread can
/// override stays.
fn drop_overridden_properties(obj: &mut ObjectExpression<'_>, copied: &mut Vec<bool>) {
    let mut later_keys = FxHashSet::default();
    let keep: Vec<bool> = obj
        .properties
        .iter()
        .rev()
        .map(|prop| static_key(prop).is_none_or(|key| later_keys.insert(key.to_string())))
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    if keep.iter().all(|kept| *kept) {
        return;
    }
    let mut keep_iter = keep.iter();
    obj.properties
        .retain(|_| *keep_iter.next().unwrap_or(&true));
    let mut keep_iter = keep.iter();
    copied.retain(|_| *keep_iter.next().unwrap_or(&true));
}

impl<'a> VisitMut<'a> for Inliner<'a, '_> {
    fn visit_object_expression(&mut self, obj: &mut ObjectExpression<'a>) {
        let Some(copied) = self.splice_spreads(obj) else {
            walk_mut::walk_object_expression(self, obj);
            return;
        };
        for (prop, is_copy) in obj.properties.iter_mut().zip(copied) {
            if is_copy {
                self.nesting += 1;
                self.visit_object_property_kind(prop);
                self.nesting -= 1;
            } else {
                self.visit_object_property_kind(prop);
            }
        }
    }

    fn visit_object_property(&mut self, prop: &mut ObjectProperty<'a>) {
        let Some(value) = self.copy_for(&prop.value) else {
            walk_mut::walk_object_property(self, prop);
            return;
        };
        prop.value = value;
        prop.shorthand = false;
        self.nesting += 1;
        walk_mut::walk_object_property(self, prop);
        self.nesting -= 1;
    }

    fn visit_array_expression_element(&mut self, element: &mut ArrayExpressionElement<'a>) {
        let copy = element.as_expression().and_then(|expr| self.copy_for(expr));
        let Some(value) = copy else {
            walk_mut::walk_array_expression_element(self, element);
            return;
        };
        *element = ArrayExpressionElement::from(value);
        self.nesting += 1;
        walk_mut::walk_array_expression_element(self, element);
        self.nesting -= 1;
    }
}

/// Whether one top-level `const` or `let` is the only binding of `name` in the
/// program, and no expression writes to it or to one of its members. A
/// mutating method call, an `Object.assign` call and an alias such as
/// `const { resolve } = name` count as a write unless `allow_mutating_calls`
/// is set.
pub(super) fn holds_one_stable_object(
    program: &Program<'_>,
    name: &str,
    allow_mutating_calls: bool,
) -> bool {
    !stable_bindings(program, [name], allow_mutating_calls, true).is_empty()
}

/// The subset of `names` that [`holds_one_stable_object`] accepts, from one
/// walk of the program. With `require_top_level` unset, the caller checks
/// where the one binding of each name is.
pub(super) fn stable_bindings<'n>(
    program: &Program<'_>,
    names: impl IntoIterator<Item = &'n str>,
    allow_mutating_calls: bool,
    require_top_level: bool,
) -> FxHashSet<&'n str> {
    binding_counts(program, names, require_top_level)
        .into_iter()
        .filter(|(_, count)| count.is_stable(allow_mutating_calls))
        .map(|(name, _)| name)
        .collect()
}

/// The subset of the named literal values that no code can change. This is
/// stricter than [`stable_bindings`]: a value that the program passes to a
/// call, as in `configure(config)`, counts as changed, unless the callee only
/// reads its arguments or the value is a string. A called function can change
/// an object or an array that it gets.
pub(super) fn stable_values<'n, 'e, 'a: 'e>(
    program: &Program<'_>,
    candidates: impl IntoIterator<Item = (&'n str, &'e Expression<'a>)>,
    require_top_level: bool,
) -> FxHashSet<&'n str> {
    let mut immutable = FxHashSet::default();
    let names: Vec<&'n str> = candidates
        .into_iter()
        .map(|(name, value)| {
            if matches!(
                value.get_inner_expression(),
                Expression::StringLiteral(_) | Expression::TemplateLiteral(_)
            ) {
                immutable.insert(name);
            }
            name
        })
        .collect();
    binding_counts(program, names, require_top_level)
        .into_iter()
        .filter(|(name, count)| {
            count.is_stable(false) && (count.call_arguments == 0 || immutable.contains(name))
        })
        .map(|(name, _)| name)
        .collect()
}

/// The [`NameCount`] of each name, with the changes and calls through aliases
/// added to the root names.
fn binding_counts<'n>(
    program: &Program<'_>,
    names: impl IntoIterator<Item = &'n str>,
    require_top_level: bool,
) -> FxHashMap<&'n str, NameCount> {
    let mut usage = NameUsage::new(
        names
            .into_iter()
            .filter(|name| !require_top_level || declares_top_level_const_or_let(program, name)),
    );
    if usage.counts.is_empty() {
        return FxHashMap::default();
    }
    usage.visit_program(program);
    let aliased = changed_alias_names(program);
    for name in aliased.changed {
        usage.count_mutation(&name);
    }
    for name in aliased.passed_to_calls {
        usage.count_call_argument(&name);
    }
    usage.counts
}

/// Whether a top-level `const` or `let` statement declares `name`. A `var` is
/// function scoped and hoisted, so a later statement can hold its value.
fn declares_top_level_const_or_let(program: &Program<'_>, name: &str) -> bool {
    top_level_variable_declarations(program).any(|decl| {
        matches!(
            decl.kind,
            VariableDeclarationKind::Const | VariableDeclarationKind::Let
        ) && decl.declarations.iter().any(|declarator| {
            matches!(&declarator.id, BindingPattern::BindingIdentifier(id) if id.name == name)
        })
    })
}

/// The maximum number of aliases that [`changed_alias_names`] follows. Above
/// it, every name in an alias counts as changed, so no value is inlined past
/// an alias that was not checked.
const MAX_ALIASES: usize = 4096;

/// The root names that the program changes or passes to a call through an
/// alias.
#[derive(Default)]
struct AliasedNames {
    changed: Vec<String>,
    passed_to_calls: Vec<String>,
}

/// The names that the program changes through an alias. An alias is a
/// binding, an assignment or a loop binding that holds a name or a member
/// chain of it, such as `const r = common.resolve`, `const { resolve } =
/// common`, `({ resolve: r } = common)`, `const list = [common]` or
/// `for (const c of [common])`. A write or a mutating call through the alias,
/// or through an alias of the alias, changes the root name. A call argument
/// that names the alias passes the root name to that call. An alias that is
/// only read changes nothing.
fn changed_alias_names(program: &Program<'_>) -> AliasedNames {
    let mut collector = AliasCollector::default();
    collector.visit_program(program);
    if collector.aliases.is_empty() {
        return AliasedNames::default();
    }
    if collector.aliases.len() > MAX_ALIASES {
        // Too many aliases to follow: every name in them counts as changed.
        return AliasedNames {
            changed: collector
                .aliases
                .into_iter()
                .flat_map(|alias| [alias.name, alias.root])
                .collect(),
            passed_to_calls: Vec::new(),
        };
    }
    let mut usage = NameUsage::new(collector.aliases.iter().map(|alias| alias.name.as_str()));
    usage.visit_program(program);
    let mut roots: FxHashMap<&str, Vec<&str>> = FxHashMap::default();
    for alias in &collector.aliases {
        roots
            .entry(alias.name.as_str())
            .or_default()
            .push(alias.root.as_str());
    }
    let aliases_where = |accept: fn(&NameCount) -> bool| -> Vec<String> {
        let start = collector
            .aliases
            .iter()
            .map(|alias| alias.name.as_str())
            .filter(|name| usage.counts.get(name).is_some_and(accept));
        roots_of(&roots, start)
    };
    AliasedNames {
        changed: aliases_where(|count| count.writes > count.rebinds || count.mutating_calls > 0),
        passed_to_calls: aliases_where(|count| count.call_arguments > 0),
    }
}

/// The start names and every root that they reach through `roots`. An alias of
/// a changed alias changes its root as well. Each name enters the worklist at
/// most once.
fn roots_of<'r>(
    roots: &FxHashMap<&'r str, Vec<&'r str>>,
    start: impl Iterator<Item = &'r str>,
) -> Vec<String> {
    let mut pending: Vec<&str> = start.collect();
    let mut reached: FxHashSet<&str> = pending.iter().copied().collect();
    while let Some(name) = pending.pop() {
        for &root in roots.get(name).into_iter().flatten() {
            if reached.insert(root) {
                pending.push(root);
            }
        }
    }
    reached.into_iter().map(str::to_owned).collect()
}

/// A name that holds the value of another name or of one of its members.
struct Alias {
    name: String,
    root: String,
}

#[derive(Default)]
struct AliasCollector {
    aliases: Vec<Alias>,
}

impl AliasCollector {
    fn push(&mut self, name: &str, value: &Expression<'_>) {
        for root in alias_roots(value) {
            self.aliases.push(Alias {
                name: name.to_owned(),
                root: root.to_owned(),
            });
        }
    }
}

impl<'a> Visit<'a> for AliasCollector {
    fn visit_variable_declarator(&mut self, declarator: &VariableDeclarator<'a>) {
        if let Some(init) = &declarator.init {
            for identifier in declarator.id.get_binding_identifiers() {
                self.push(identifier.name.as_str(), init);
            }
        }
        walk::walk_variable_declarator(self, declarator);
    }

    fn visit_assignment_expression(&mut self, assign: &AssignmentExpression<'a>) {
        if let AssignmentTarget::AssignmentTargetIdentifier(target) = &assign.left {
            self.push(target.name.as_str(), &assign.right);
        } else if assign.left.as_assignment_target_pattern().is_some() {
            // `({ resolve: r } = common)` binds `r` to a member of `common`.
            for name in target_names(&assign.left) {
                self.push(&name, &assign.right);
            }
        }
        walk::walk_assignment_expression(self, assign);
    }

    fn visit_for_of_statement(&mut self, statement: &ForOfStatement<'a>) {
        // `for (const c of [common])` binds `c` to each element.
        match &statement.left {
            ForStatementLeft::VariableDeclaration(decl) => {
                for declarator in &decl.declarations {
                    for identifier in declarator.id.get_binding_identifiers() {
                        self.push(identifier.name.as_str(), &statement.right);
                    }
                }
            }
            left => {
                if let Some(target) = left.as_assignment_target() {
                    for name in target_names(target) {
                        self.push(&name, &statement.right);
                    }
                }
            }
        }
        walk::walk_for_of_statement(self, statement);
    }
}

/// The identifier names in an assignment target, such as `r` and `a` in
/// `{ resolve: r, a }`. A name in a default value or a computed key counts
/// too, which can only add an alias.
fn target_names(target: &AssignmentTarget<'_>) -> Vec<String> {
    #[derive(Default)]
    struct Names(Vec<String>);
    impl<'a> Visit<'a> for Names {
        fn visit_identifier_reference(&mut self, identifier: &IdentifierReference<'a>) {
            self.0.push(identifier.name.to_string());
        }
    }
    let mut names = Names::default();
    names.visit_assignment_target(target);
    names.0
}

/// The root names that a value can hold: the root of a member chain, the
/// roots of each branch of `a || b`, `a ?? b` or `cond ? a : b`, and the roots
/// of each element, property value and spread of an array or object literal,
/// such as `[common]` or `{ c: common }`.
fn alias_roots<'e>(value: &'e Expression<'_>) -> Vec<&'e str> {
    let mut roots = Vec::new();
    push_alias_roots(value, &mut roots);
    roots
}

fn push_alias_roots<'e>(value: &'e Expression<'_>, roots: &mut Vec<&'e str>) {
    match value.without_parentheses().get_inner_expression() {
        Expression::LogicalExpression(logical) => {
            push_alias_roots(&logical.left, roots);
            push_alias_roots(&logical.right, roots);
        }
        Expression::ConditionalExpression(conditional) => {
            push_alias_roots(&conditional.consequent, roots);
            push_alias_roots(&conditional.alternate, roots);
        }
        Expression::ArrayExpression(array) => {
            for element in &array.elements {
                match element {
                    ArrayExpressionElement::SpreadElement(spread) => {
                        push_alias_roots(&spread.argument, roots);
                    }
                    element => {
                        if let Some(expr) = element.as_expression() {
                            push_alias_roots(expr, roots);
                        }
                    }
                }
            }
        }
        Expression::ObjectExpression(object) => {
            for property in &object.properties {
                match property {
                    ObjectPropertyKind::ObjectProperty(property) => {
                        push_alias_roots(&property.value, roots);
                    }
                    ObjectPropertyKind::SpreadProperty(spread) => {
                        push_alias_roots(&spread.argument, roots);
                    }
                }
            }
        }
        other => roots.extend(member_root(other)),
    }
}

/// What one program does with one name: the number of bindings that declare it,
/// the number of expressions that write to it or to one of its members, the
/// number of those writes that assign a new value to the name itself, the
/// number of calls such as `name.push(...)` that change it, and the number of
/// call arguments that pass it to a function that can change it.
#[derive(Default)]
struct NameCount {
    bindings: usize,
    writes: usize,
    rebinds: usize,
    mutating_calls: usize,
    call_arguments: usize,
}

impl NameCount {
    /// One binding, no writes, and no mutating calls unless they are allowed.
    /// A call argument does not count here: see [`stable_values`].
    fn is_stable(&self, allow_mutating_calls: bool) -> bool {
        self.bindings == 1 && self.writes == 0 && (allow_mutating_calls || self.mutating_calls == 0)
    }
}

/// The [`NameCount`] of each name in a set, from one walk of a program.
struct NameUsage<'n> {
    counts: FxHashMap<&'n str, NameCount>,
    write_depth: usize,
}

impl<'n> NameUsage<'n> {
    fn new(names: impl IntoIterator<Item = &'n str>) -> Self {
        Self {
            counts: names
                .into_iter()
                .map(|name| (name, NameCount::default()))
                .collect(),
            write_depth: 0,
        }
    }
}

impl NameUsage<'_> {
    fn count_mutation(&mut self, name: &str) {
        if let Some(count) = self.counts.get_mut(name) {
            count.mutating_calls += 1;
        }
    }

    fn count_call_argument(&mut self, name: &str) {
        if let Some(count) = self.counts.get_mut(name) {
            count.call_arguments += 1;
        }
    }
}

impl<'a> Visit<'a> for NameUsage<'_> {
    fn visit_binding_identifier(&mut self, identifier: &BindingIdentifier<'a>) {
        if let Some(count) = self.counts.get_mut(identifier.name.as_str()) {
            count.bindings += 1;
        }
    }

    fn visit_identifier_reference(&mut self, identifier: &IdentifierReference<'a>) {
        if self.write_depth > 0
            && let Some(count) = self.counts.get_mut(identifier.name.as_str())
        {
            count.writes += 1;
        }
    }

    fn visit_assignment_target(&mut self, target: &AssignmentTarget<'a>) {
        self.write_depth += 1;
        walk::walk_assignment_target(self, target);
        self.write_depth -= 1;
    }

    fn visit_assignment_expression(&mut self, assign: &AssignmentExpression<'a>) {
        if let AssignmentTarget::AssignmentTargetIdentifier(target) = &assign.left
            && let Some(count) = self.counts.get_mut(target.name.as_str())
        {
            count.rebinds += 1;
        }
        walk::walk_assignment_expression(self, assign);
    }

    fn visit_update_expression(&mut self, expression: &UpdateExpression<'a>) {
        self.write_depth += 1;
        walk::walk_update_expression(self, expression);
        self.write_depth -= 1;
    }

    fn visit_call_expression(&mut self, call: &CallExpression<'a>) {
        if let Some(name) = mutated_receiver(call) {
            self.count_mutation(name);
        }
        for name in changeable_arguments(call) {
            self.count_call_argument(name);
        }
        walk::walk_call_expression(self, call);
    }

    fn visit_unary_expression(&mut self, expression: &UnaryExpression<'a>) {
        if expression.operator != UnaryOperator::Delete {
            walk::walk_unary_expression(self, expression);
            return;
        }
        self.write_depth += 1;
        walk::walk_unary_expression(self, expression);
        self.write_depth -= 1;
    }
}

/// Methods that change the array, map or set they are called on.
const MUTATING_METHODS: &[&str] = &[
    "push",
    "unshift",
    "splice",
    "pop",
    "shift",
    "sort",
    "reverse",
    "fill",
    "copyWithin",
    "set",
    "add",
    "delete",
    "clear",
];

/// The root name of the value that a call changes: the receiver of a call
/// such as `name.push(...)`, `name!.push(...)` or `name.plugins.push(...)`
/// whose method changes the receiver, or the target of a call such as
/// `Object.assign(name.resolve, ...)` or `Reflect.set(name, ...)`. A change to a
/// member changes the value of the root name too.
fn mutated_receiver<'e>(call: &'e CallExpression<'_>) -> Option<&'e str> {
    let Expression::StaticMemberExpression(member) = call.callee.without_parentheses() else {
        return None;
    };
    if changes_first_argument(member) {
        return call
            .arguments
            .first()
            .and_then(Argument::as_expression)
            .and_then(member_root);
    }
    if !MUTATING_METHODS.contains(&member.property.name.as_str()) {
        return None;
    }
    member_root(&member.object)
}

/// Static methods that change the object in their first argument.
const FIRST_ARGUMENT_MUTATORS: &[(&str, &str)] = &[
    ("Object", "assign"),
    ("Object", "defineProperty"),
    ("Object", "defineProperties"),
    ("Object", "setPrototypeOf"),
    ("Reflect", "set"),
    ("Reflect", "defineProperty"),
    ("Reflect", "deleteProperty"),
    ("Reflect", "setPrototypeOf"),
];

fn changes_first_argument(member: &StaticMemberExpression<'_>) -> bool {
    let Some(owner) = global_name(&member.object) else {
        return false;
    };
    FIRST_ARGUMENT_MUTATORS
        .iter()
        .any(|(name, method)| owner == *name && member.property.name == *method)
}

/// Objects that hold the globals, as in `globalThis.Object`.
const GLOBAL_OBJECTS: &[&str] = &["globalThis", "global", "window", "self"];

/// The name of a global such as `Object` in `Object` or `globalThis.Object`.
fn global_name<'e>(expr: &'e Expression<'_>) -> Option<&'e str> {
    match expr.without_parentheses() {
        Expression::Identifier(id) => Some(id.name.as_str()),
        Expression::StaticMemberExpression(member) => match member.object.without_parentheses() {
            Expression::Identifier(owner) if GLOBAL_OBJECTS.contains(&owner.name.as_str()) => {
                Some(member.property.name.as_str())
            }
            _ => None,
        },
        _ => None,
    }
}

/// Functions that only read their arguments.
const READ_ONLY_FUNCTIONS: &[&str] = &[
    "require",
    "defineConfig",
    "defineProject",
    "defineWorkspace",
    "mergeConfig",
    "String",
    "Number",
    "Boolean",
    "fileURLToPath",
];

/// Methods of a global object that only read their arguments. An empty
/// method name stands for each method of that object.
const READ_ONLY_METHODS: &[(&str, &str)] = &[
    ("path", ""),
    ("console", ""),
    ("JSON", "stringify"),
    ("require", "resolve"),
    ("Object", "keys"),
    ("Object", "values"),
    ("Object", "entries"),
    ("Object", "hasOwn"),
    ("Object", "getOwnPropertyNames"),
    ("Array", "isArray"),
];

/// The root names that a call passes to a function that can change them.
///
/// A config can change a value in a helper, as in `addPlugins(config)` or
/// `_.merge(config, extra)`. A callee that only reads its arguments, such as
/// `path.resolve` or `defineConfig`, changes nothing. The first argument of a
/// mutator such as `Object.assign` is a change that [`mutated_receiver`]
/// counts, and its other arguments are only read.
fn changeable_arguments<'e>(call: &'e CallExpression<'_>) -> Vec<&'e str> {
    let callee = call.callee.without_parentheses();
    if reads_arguments_only(callee) {
        return Vec::new();
    }
    if matches!(callee, Expression::StaticMemberExpression(member) if changes_first_argument(member))
    {
        return Vec::new();
    }
    let mut roots = Vec::new();
    for argument in &call.arguments {
        match argument {
            Argument::SpreadElement(spread) => push_alias_roots(&spread.argument, &mut roots),
            argument => {
                if let Some(expr) = argument.as_expression() {
                    push_alias_roots(expr, &mut roots);
                }
            }
        }
    }
    roots
}

fn reads_arguments_only(callee: &Expression<'_>) -> bool {
    match callee {
        Expression::Identifier(id) => READ_ONLY_FUNCTIONS.contains(&id.name.as_str()),
        Expression::StaticMemberExpression(member) => {
            let Expression::Identifier(owner) = member.object.without_parentheses() else {
                return false;
            };
            READ_ONLY_METHODS.iter().any(|(name, method)| {
                owner.name == *name && (method.is_empty() || member.property.name == *method)
            })
        }
        _ => false,
    }
}

/// The root name of an identifier or of a member chain such as
/// `name.resolve.alias`. A binding or an assignment that holds such a value
/// is an alias, and a write through the alias changes the root name.
fn member_root<'e>(expr: &'e Expression<'_>) -> Option<&'e str> {
    let mut current = expr.without_parentheses().get_inner_expression();
    loop {
        current = match current {
            Expression::Identifier(identifier) => return Some(identifier.name.as_str()),
            Expression::StaticMemberExpression(inner) => &inner.object,
            Expression::ComputedMemberExpression(inner) => &inner.object,
            Expression::ChainExpression(chain) => match &chain.expression {
                ChainElement::StaticMemberExpression(inner) => &inner.object,
                ChainElement::ComputedMemberExpression(inner) => &inner.object,
                _ => return None,
            },
            _ => return None,
        }
        .without_parentheses()
        .get_inner_expression();
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::super::{
        extract_config_array_nested_string_or_array, extract_config_string,
        extract_config_string_or_array,
    };

    fn ts_path() -> &'static Path {
        Path::new("config.ts")
    }

    fn js_path() -> &'static Path {
        Path::new("config.js")
    }

    #[test]
    fn constant_that_a_binding_only_reads_still_resolves() {
        let path = Path::new("vitest.config.ts");
        let cases = [
            r#"const BASE = ["./a.ts"]; const first = BASE[0]; export default { setupFiles: BASE };"#,
            r#"const S = "./a.ts"; const T = S; export default { setupFiles: S };"#,
            r#"const BASE = ["./a.ts"]; const n = BASE.length; export default { setupFiles: BASE };"#,
        ];
        for source in cases {
            assert_eq!(
                extract_config_string_or_array(source, path, &["setupFiles"]),
                vec!["./a.ts"],
                "{source}"
            );
        }
        let source = r#"const OPTS = { files: ["./a.ts"] }; const f = OPTS.files; export default { x: OPTS };"#;
        assert_eq!(
            extract_config_string_or_array(source, path, &["x", "files"]),
            vec!["./a.ts"]
        );
    }

    #[test]
    fn constant_that_an_alias_changes_does_not_resolve() {
        let path = Path::new("vitest.config.ts");
        let cases = [
            r#"const OPTS = { files: ["./a.ts"] }; const f = OPTS.files; f.push("./b.ts"); export default { x: OPTS };"#,
            r#"const OPTS = { files: ["./a.ts"] }; const f = OPTS.files || []; f.push("./b.ts"); export default { x: OPTS };"#,
            r#"const OPTS = { files: ["./a.ts"] }; const f = flag ? [] : OPTS.files; f.push("./b.ts"); export default { x: OPTS };"#,
        ];
        for source in cases {
            assert!(
                extract_config_string_or_array(source, path, &["x", "files"]).is_empty(),
                "{source}"
            );
        }
    }

    #[test]
    fn constant_whose_alias_only_gets_a_new_value_still_resolves() {
        let path = Path::new("vitest.config.ts");
        let source = r#"const OPTS = { files: ["./a.ts"] }; let f = OPTS.files; f = []; export default { x: OPTS };"#;
        assert_eq!(
            extract_config_string_or_array(source, path, &["x", "files"]),
            vec!["./a.ts"]
        );
    }

    #[test]
    fn same_file_constants_resolve_in_values_and_array_elements() {
        let source = r#"
            const SETUP = "./a.ts";
            const MORE = ["./b.ts", SETUP] as const;
            const ITEM = { files: MORE } satisfies Item;
            export default { items: [ITEM], single: SETUP };
        "#;
        assert_eq!(
            extract_config_array_nested_string_or_array(source, ts_path(), &["items"], &["files"]),
            vec!["./b.ts", "./a.ts"]
        );
        assert_eq!(
            extract_config_string(source, ts_path(), &["single"]),
            Some("./a.ts".to_string())
        );
    }

    #[test]
    fn constant_with_a_function_resolves_and_unstable_constants_do_not() {
        let source = r#"
            const WITH_FN = { files: ["./a.ts"], load: (x) => x };
            const SHADOWED = ["./b.ts"];
            function f(SHADOWED) { return SHADOWED; }
            const SELF = [SELF];
            export default { a: WITH_FN, b: SHADOWED, c: SELF };
        "#;
        assert_eq!(
            extract_config_string_or_array(source, js_path(), &["a", "files"]),
            vec!["./a.ts"]
        );
        assert!(extract_config_string_or_array(source, js_path(), &["b"]).is_empty());
        assert!(extract_config_string_or_array(source, js_path(), &["c"]).is_empty());
    }

    #[test]
    fn constant_that_a_method_call_mutates_does_not_resolve() {
        let source = r#"
            const PUSHED = ["./a.ts"];
            PUSHED.push("./b.ts");
            const ASSERTED = ["./c.ts"];
            ASSERTED!.splice(0, 1);
            const READ = ["./d.ts"];
            READ.map((file) => file);
            export default { a: PUSHED, b: ASSERTED, c: READ };
        "#;
        assert!(extract_config_string_or_array(source, ts_path(), &["a"]).is_empty());
        assert!(extract_config_string_or_array(source, ts_path(), &["b"]).is_empty());
        assert_eq!(
            extract_config_string_or_array(source, ts_path(), &["c"]),
            vec!["./d.ts"]
        );
    }
}
