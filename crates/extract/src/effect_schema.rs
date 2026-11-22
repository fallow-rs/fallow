//! Syntactic Effect schema shapes for `effect.schemaExportsAsTypes`.
//!
//! A declaration is a schema when its initializer, or the `extends` clause of
//! its class, is a chain of calls and static members whose root is:
//! - the `Schema` binding of an Effect import, with a schema constructor as
//!   the first member (`Schema.Struct(...)`, `Schema.String`), or a class
//!   factory in a class heritage (`Schema.Class<X>("X")({...})`), or
//! - a module-scope binding that is already a schema (`User.pipe(...)`), or
//! - a named or default import (`Imported.pipe(...)`, `Imported`). The chain
//!   is then a derived schema: it is a schema only when the import is one,
//!   and the analyze layer resolves the import to decide. A chain without a
//!   `pipe` call needs no `Schema` import (`Imported.annotations({...})`),
//!   but the collector still runs only in a module that imports Effect.
//!
//! Each later member must be a method that returns a schema (`pipe`,
//! `annotations`, `extend`), and each `pipe` argument must be a `Schema`
//! transform (`Schema.brand("Id")`, `Schema.minLength(1)`) or a call of an
//! `HttpApiSchema` transform (`HttpApiSchema.asFormUrlEncoded()`). The rule is
//! conservative: a chain that ends in `make`, `decode*`, a field read or an
//! arrow function is not a schema.
//!
//! A chain can also start at a member of a namespace import
//! (`Imported.User.pipe(...)`, `Imported.makeUser()`). The chain is then a
//! derived schema of the export with that member name.
//!
//! A conditional expression is a schema when both branches are schemas
//! (`flag ? Schema.String : Schema.Number`).
//!
//! A call of a function is a schema when each `return` of the function is a
//! schema (`const makeUser = () => Schema.Struct({...})`, then `makeUser()`),
//! or when the function returns a parameter and the argument at that position
//! is a schema (`identity(Schema.String)`). `effect_schema_helpers.rs` has the
//! rules for such a function.
//!
//! A member read of a local `const` object or array literal is a schema when
//! the member is a schema (`tuple[0]`, `record.User`, `record.make()`).
//! `effect_schema_containers.rs` has the rules for such a literal.

use std::cell::RefCell;

use oxc_ast::ast::{Argument, CallExpression, Class, Expression, IdentifierReference};
use oxc_semantic::{Semantic, SymbolId};
use rustc_hash::{FxHashMap, FxHashSet};

use fallow_types::extract::{ImportInfo, ImportedName};

#[path = "effect_schema_names.rs"]
mod names;
use names::{HTTP_API_PIPE_TRANSFORMS, PIPE_TRANSFORMS, TYPE_ONLY_NAMES};

#[path = "effect_schema_helpers.rs"]
mod helpers;

#[path = "effect_schema_containers.rs"]
mod containers;

/// `Schema` functions with a lowercase name that return a schema. Every
/// other lowercase function (`decodeUnknownSync`, `is`, `isGreaterThan`)
/// returns a decoder, a guard or a filter.
const LOWERCASE_SCHEMA_CONSTRUCTORS: &[&str] = &[
    "compose",
    "declare",
    "encodedSchema",
    "extend",
    "parseJson",
    "suspend",
    "toEncoded",
    "toType",
    "transform",
    "transformOrFail",
    "typeSchema",
];

/// Class factories of effect 3 (`Class`, `TaggedClass`, `TaggedError`,
/// `TaggedRequest`) and effect 4 (also `Error` and `Opaque`).
const CLASS_FACTORIES: &[&str] = &[
    "Class",
    "Error",
    "Opaque",
    "TaggedClass",
    "TaggedError",
    "TaggedRequest",
];

/// Schema constructors that `pipe` can take as a bare function
/// (`Schema.String.pipe(Schema.NullOr)`).
const PIPE_CONSTRUCTORS: &[&str] = &[
    "Array",
    "NonEmptyArray",
    "NullOr",
    "NullishOr",
    "UndefinedOr",
];

/// Methods on a schema value that return a schema.
const SCHEMA_METHODS: &[&str] = &[
    "annotate",
    "annotateKey",
    "annotations",
    "brand",
    "check",
    "extend",
    "mapFields",
    "mapMembers",
    "omit",
    "pick",
    "pipe",
    "rename",
];

/// True when the import binds the Effect `Schema` module: `{ Schema }` from
/// `effect` or `@effect/schema`, or a namespace import of `effect/Schema` or
/// `@effect/schema/Schema`.
fn is_schema_module_import(import: &ImportInfo) -> bool {
    match &import.imported_name {
        ImportedName::Named(name) => {
            name == "Schema" && matches!(import.source.as_str(), "effect" | "@effect/schema")
        }
        ImportedName::Namespace => {
            matches!(
                import.source.as_str(),
                "effect/Schema" | "@effect/schema/Schema"
            )
        }
        ImportedName::Default | ImportedName::SideEffect => false,
    }
}

/// True when the import binds the Effect `HttpApiSchema` module: the named
/// export of `effect/http-api` (effect 4) or `@effect/platform` (effect 3),
/// or a namespace import of the module itself.
fn is_http_api_schema_module_import(import: &ImportInfo) -> bool {
    match &import.imported_name {
        ImportedName::Named(name) => {
            name == "HttpApiSchema"
                && matches!(
                    import.source.as_str(),
                    "effect/http-api" | "@effect/platform"
                )
        }
        ImportedName::Namespace => {
            matches!(
                import.source.as_str(),
                "effect/http-api/HttpApiSchema" | "@effect/platform/HttpApiSchema"
            )
        }
        ImportedName::Default | ImportedName::SideEffect => false,
    }
}

fn is_schema_constructor(name: &str) -> bool {
    if name.starts_with(|c: char| c.is_ascii_uppercase()) {
        return !TYPE_ONLY_NAMES.contains(&name);
    }
    LOWERCASE_SCHEMA_CONSTRUCTORS.contains(&name)
}

fn is_class_factory(name: &str) -> bool {
    CLASS_FACTORIES.contains(&name)
}

fn is_pipe_call(callee: &Expression<'_>) -> bool {
    matches!(
        unwrap(callee),
        Expression::StaticMemberExpression(member) if member.property.name == "pipe"
    )
}

/// Strip parentheses and the TypeScript wrappers that keep the runtime value.
fn unwrap<'e, 'a>(expression: &'e Expression<'a>) -> &'e Expression<'a> {
    let mut current = expression.without_parentheses();
    loop {
        current = match current {
            Expression::TSAsExpression(node) => node.expression.without_parentheses(),
            Expression::TSSatisfiesExpression(node) => node.expression.without_parentheses(),
            Expression::TSNonNullExpression(node) => node.expression.without_parentheses(),
            Expression::TSInstantiationExpression(node) => node.expression.without_parentheses(),
            _ => return current,
        };
    }
}

/// The kind of schema that an expression or a class evaluates to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SchemaShape {
    /// The module itself proves the schema.
    Known,
    /// A schema only when the import with the local name `import_local` is
    /// a schema. With `through_call`, the chain calls the import itself
    /// (`makeImported()`), so the import can also be a function that returns
    /// a schema. With `member`, the import is a namespace and the chain starts
    /// at its export `member` (`Imported.makeUser()`).
    Derived {
        import_local: String,
        through_call: bool,
        member: Option<String>,
    },
}

impl SchemaShape {
    /// The shape of a value that is one of two schemas. A known schema adds
    /// no condition. Two different derived shapes give no shape, because one
    /// fact names one import.
    fn join(self, other: Self) -> Option<Self> {
        match (self, other) {
            (Self::Known, shape) | (shape, Self::Known) => Some(shape),
            (left, right) => (left == right).then_some(left),
        }
    }
}

/// One step of a chain, from the root outward.
enum Link<'e, 'a> {
    /// A static member read: `.pipe`, `.User`.
    Member(&'e str),
    /// A computed member read with a literal key: `tuple[0]`, `record["a"]`.
    Key(String),
    Call(&'e CallExpression<'a>),
}

/// True when the links after the root of a chain keep a schema: each member
/// is a call of a method that returns a schema. A method read without a call
/// (`User.annotations`) is a function.
fn is_schema_tail(links: &[Link<'_, '_>]) -> bool {
    links.iter().enumerate().all(|(index, link)| match link {
        Link::Member(name) => {
            SCHEMA_METHODS.contains(name) && matches!(links.get(index + 1), Some(Link::Call(_)))
        }
        Link::Key(_) => false,
        Link::Call(_) => true,
    })
}

/// The key of a computed member read with a string or a non-negative integer
/// literal.
fn literal_key(expression: &Expression<'_>) -> Option<String> {
    match unwrap(expression) {
        Expression::StringLiteral(literal) => literal.value.as_str().map(str::to_string),
        // JavaScript writes 1e21 and larger in exponent form, so the key text
        // would differ.
        Expression::NumericLiteral(literal)
            if (0.0..1e21).contains(&literal.value) && literal.value.fract() == 0.0 =>
        {
            Some(format!("{}", literal.value))
        }
        _ => None,
    }
}

/// Schema namespace bindings and the module-scope bindings found to be
/// schemas so far, in source order.
pub struct SchemaShapes<'s, 'a> {
    semantic: &'s Semantic<'a>,
    namespaces: FxHashSet<SymbolId>,
    /// Bindings of the `HttpApiSchema` module.
    http_api_namespaces: FxHashSet<SymbolId>,
    /// Named and default value imports that a derived chain can start at.
    imports: FxHashMap<SymbolId, String>,
    /// Namespace value imports whose member a derived chain can start at.
    namespace_imports: FxHashMap<SymbolId, String>,
    locals: FxHashMap<SymbolId, SchemaShape>,
    local_names: FxHashMap<String, SchemaShape>,
    /// Functions whose each `return` is a schema or one parameter, with what
    /// a call gives.
    helpers: RefCell<FxHashMap<SymbolId, helpers::HelperResult>>,
    /// `const` schemas in the body of an inspected function.
    helper_locals: RefCell<FxHashMap<SymbolId, SchemaShape>>,
    /// The span starts of the function bodies and literal members under
    /// inspection: a function that returns a call of a second function adds
    /// one.
    in_progress: RefCell<FxHashSet<u32>>,
    /// `const` bindings checked for use as a literal container, with the
    /// result.
    containers: RefCell<FxHashMap<SymbolId, bool>>,
}

impl<'s, 'a> SchemaShapes<'s, 'a> {
    pub fn new(semantic: &'s Semantic<'a>, imports: &[ImportInfo]) -> Self {
        let namespaces = imports
            .iter()
            .filter(|import| is_schema_module_import(import))
            .filter_map(|import| {
                crate::binding_references::admitted_import_symbol(semantic, import)
            })
            .collect();
        let http_api_namespaces = imports
            .iter()
            .filter(|import| is_http_api_schema_module_import(import))
            .filter_map(|import| {
                crate::binding_references::admitted_import_symbol(semantic, import)
            })
            .collect();
        let namespace_imports = imports
            .iter()
            .filter(|import| {
                !import.is_type_only
                    && import.imported_name == ImportedName::Namespace
                    && !is_schema_module_import(import)
                    && !is_http_api_schema_module_import(import)
            })
            .filter_map(|import| {
                crate::binding_references::admitted_import_symbol(semantic, import)
                    .map(|symbol| (symbol, import.local_name.clone()))
            })
            .collect();
        let imports = imports
            .iter()
            .filter(|import| {
                !import.is_type_only
                    && matches!(
                        import.imported_name,
                        ImportedName::Named(_) | ImportedName::Default
                    )
                    && !is_schema_module_import(import)
                    && !is_http_api_schema_module_import(import)
            })
            .filter_map(|import| {
                crate::binding_references::admitted_import_symbol(semantic, import)
                    .map(|symbol| (symbol, import.local_name.clone()))
            })
            .collect();
        Self {
            semantic,
            namespaces,
            http_api_namespaces,
            imports,
            namespace_imports,
            locals: FxHashMap::default(),
            local_names: FxHashMap::default(),
            helpers: RefCell::new(FxHashMap::default()),
            helper_locals: RefCell::new(FxHashMap::default()),
            in_progress: RefCell::new(FxHashSet::default()),
            containers: RefCell::new(FxHashMap::default()),
        }
    }

    /// Remember a module-scope binding with its schema shape. A binding that
    /// the module assigns again (`let Base = Schema.String; Base = other`)
    /// holds an unknown value, so it gets no shape.
    pub fn record(&mut self, symbol: SymbolId, name: &str, shape: SchemaShape) {
        if self.semantic.scoping().symbol_is_mutated(symbol) {
            return;
        }
        self.locals.insert(symbol, shape.clone());
        self.local_names.insert(name.to_string(), shape);
    }

    /// The schema shape of the module-scope binding `name`.
    pub fn name_shape(&self, name: &str) -> Option<&SchemaShape> {
        self.local_names.get(name)
    }

    /// The shape when the class extends a schema class factory or a schema
    /// class.
    pub fn class_shape(&self, class: &Class<'a>) -> Option<SchemaShape> {
        class
            .heritage
            .as_ref()
            .and_then(|heritage| self.schema_chain(&heritage.expression, is_class_factory))
    }

    /// The shape when the expression evaluates to a schema.
    pub fn expression_shape(&self, expression: &Expression<'a>) -> Option<SchemaShape> {
        if let Expression::ConditionalExpression(conditional) = unwrap(expression) {
            let consequent = self.expression_shape(&conditional.consequent)?;
            return consequent.join(self.expression_shape(&conditional.alternate)?);
        }
        self.schema_chain(expression, is_schema_constructor)
    }

    /// Walk a chain of calls and members to its root identifier.
    /// `accepts_head` checks the member read directly off the `Schema` binding.
    fn schema_chain(
        &self,
        expression: &Expression<'a>,
        accepts_head: fn(&str) -> bool,
    ) -> Option<SchemaShape> {
        if self.namespaces.is_empty()
            && self.imports.is_empty()
            && self.namespace_imports.is_empty()
        {
            return None;
        }
        let (root, links) = self.flatten(expression)?;
        let symbol = self.symbol(root)?;
        if self.namespaces.contains(&symbol) {
            let Some((Link::Member(head), rest)) = links.split_first() else {
                return None;
            };
            return (accepts_head(head) && is_schema_tail(rest)).then_some(SchemaShape::Known);
        }
        if let Some(local) = self.namespace_imports.get(&symbol) {
            return namespace_member_shape(local, &links);
        }
        if self.is_container(symbol) {
            return self.container_chain(symbol, &links);
        }
        if !is_schema_tail(&links) {
            return None;
        }
        if let Some(shape) = self.locals.get(&symbol) {
            return Some(shape.clone());
        }
        if let Some(shape) = self.helper_locals.borrow().get(&symbol) {
            return Some(shape.clone());
        }
        // The count of calls directly on the root: 1 for `root()` and
        // `root().pipe(...)`, 0 for `root.pipe(...)`, 2 for `root()()`.
        let root_calls = links
            .iter()
            .take_while(|link| matches!(link, Link::Call(_)))
            .count();
        if let Some(local) = self.imports.get(&symbol) {
            return Some(SchemaShape::Derived {
                import_local: local.clone(),
                through_call: root_calls == 1,
                member: None,
            });
        }
        match links.first() {
            Some(Link::Call(call)) if root_calls == 1 => {
                self.helper_call_shape(symbol, &call.arguments)
            }
            _ => None,
        }
    }

    /// The root identifier of a chain and its links from the root outward.
    /// A `pipe` call with an argument that is not a transform ends the walk.
    fn flatten<'e>(
        &self,
        expression: &'e Expression<'a>,
    ) -> Option<(&'e IdentifierReference<'a>, Vec<Link<'e, 'a>>)> {
        let mut links = Vec::new();
        let mut current = expression;
        let root = loop {
            current = match unwrap(current) {
                Expression::CallExpression(call) => {
                    if is_pipe_call(&call.callee)
                        && !call
                            .arguments
                            .iter()
                            .all(|argument| self.is_pipe_transform(argument))
                    {
                        return None;
                    }
                    links.push(Link::Call(call));
                    &call.callee
                }
                Expression::StaticMemberExpression(member) => {
                    links.push(Link::Member(member.property.name.as_str()));
                    &member.object
                }
                Expression::ComputedMemberExpression(member) => {
                    links.push(Link::Key(literal_key(&member.expression)?));
                    &member.object
                }
                Expression::Identifier(identifier) => break identifier,
                _ => return None,
            };
        };
        links.reverse();
        Some((root, links))
    }

    /// True when a `pipe` argument is a `Schema` transform: a call such as
    /// `Schema.brand("Id")` or `Schema.minLength(1)`, a bare transform such as
    /// `Schema.optional`, or a bare constructor such as `Schema.NullOr`. A
    /// call of an `HttpApiSchema` transform is one too. Its bare form is not:
    /// each is a factory, so `pipe` would return a function.
    fn is_pipe_transform(&self, argument: &Argument<'a>) -> bool {
        let Some(expression) = argument.as_expression() else {
            return false;
        };
        let (callee, called) = match unwrap(expression) {
            Expression::CallExpression(call) => (unwrap(&call.callee), true),
            other => (other, false),
        };
        let Expression::StaticMemberExpression(member) = callee else {
            return false;
        };
        let Expression::Identifier(object) = unwrap(&member.object) else {
            return false;
        };
        let name = member.property.name.as_str();
        let Some(symbol) = self.symbol(object) else {
            return false;
        };
        if self.http_api_namespaces.contains(&symbol) {
            return called && HTTP_API_PIPE_TRANSFORMS.contains(&name);
        }
        let known =
            PIPE_TRANSFORMS.contains(&name) || (!called && PIPE_CONSTRUCTORS.contains(&name));
        known && self.namespaces.contains(&symbol)
    }

    /// Run `inspect` unless the node that starts at `key` is already under
    /// inspection, or the inspection is [`helpers::MAX_HELPER_DEPTH`] deep.
    fn guarded<T>(&self, key: u32, inspect: impl FnOnce() -> Option<T>) -> Option<T> {
        if self.in_progress.borrow().len() >= helpers::MAX_HELPER_DEPTH
            || !self.in_progress.borrow_mut().insert(key)
        {
            return None;
        }
        let result = inspect();
        self.in_progress.borrow_mut().remove(&key);
        result
    }

    fn symbol(&self, identifier: &IdentifierReference<'a>) -> Option<SymbolId> {
        let reference = identifier.reference_id.get()?;
        self.semantic.scoping().get_reference(reference).symbol_id()
    }
}

/// The shape of a chain on the namespace import `local`: its first link reads
/// the export, and an optional call of that export follows.
fn namespace_member_shape(local: &str, links: &[Link<'_, '_>]) -> Option<SchemaShape> {
    let Some((Link::Member(member), rest)) = links.split_first() else {
        return None;
    };
    let (through_call, rest) = match rest.split_first() {
        Some((Link::Call(_), tail)) => (true, tail),
        _ => (false, rest),
    };
    if matches!(rest.first(), Some(Link::Call(_))) || !is_schema_tail(rest) {
        return None;
    }
    Some(SchemaShape::Derived {
        import_local: local.to_string(),
        through_call,
        member: Some((*member).to_string()),
    })
}
