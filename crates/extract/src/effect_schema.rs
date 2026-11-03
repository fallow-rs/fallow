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
//! transform (`Schema.brand("Id")`, `Schema.minLength(1)`). The rule is
//! conservative: a chain that ends in `make`, `decode*`, a field read or an
//! arrow function is not a schema. A chain on a member of a namespace import
//! (`Imported.User.pipe(...)`) is not a schema: the root is the namespace.

use oxc_ast::ast::{Argument, Class, Expression, IdentifierReference};
use oxc_semantic::{Semantic, SymbolId};
use rustc_hash::{FxHashMap, FxHashSet};

use fallow_types::extract::{ImportInfo, ImportedName};

#[path = "effect_schema_names.rs"]
mod names;
use names::{PIPE_TRANSFORMS, TYPE_ONLY_NAMES};

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
    /// A schema only when the import with this local name is a schema.
    Derived(String),
}

/// Schema namespace bindings and the module-scope bindings found to be
/// schemas so far, in source order.
pub struct SchemaShapes<'s, 'a> {
    semantic: &'s Semantic<'a>,
    namespaces: FxHashSet<SymbolId>,
    /// Named and default value imports that a derived chain can start at.
    imports: FxHashMap<SymbolId, String>,
    locals: FxHashMap<SymbolId, SchemaShape>,
    local_names: FxHashMap<String, SchemaShape>,
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
        let imports = imports
            .iter()
            .filter(|import| {
                !import.is_type_only
                    && matches!(
                        import.imported_name,
                        ImportedName::Named(_) | ImportedName::Default
                    )
                    && !is_schema_module_import(import)
            })
            .filter_map(|import| {
                crate::binding_references::admitted_import_symbol(semantic, import)
                    .map(|symbol| (symbol, import.local_name.clone()))
            })
            .collect();
        Self {
            semantic,
            namespaces,
            imports,
            locals: FxHashMap::default(),
            local_names: FxHashMap::default(),
        }
    }

    /// Remember a module-scope binding with its schema shape.
    pub fn record(&mut self, symbol: SymbolId, name: &str, shape: SchemaShape) {
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
        self.schema_chain(expression, is_schema_constructor)
    }

    /// Walk a chain of calls and static members to its root identifier.
    /// `accepts_head` checks the member read directly off the `Schema` binding.
    fn schema_chain(
        &self,
        expression: &Expression<'a>,
        accepts_head: fn(&str) -> bool,
    ) -> Option<SchemaShape> {
        if self.namespaces.is_empty() && self.imports.is_empty() {
            return None;
        }
        let mut members: Vec<&str> = Vec::new();
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
                    &call.callee
                }
                Expression::StaticMemberExpression(member) => {
                    members.push(member.property.name.as_str());
                    &member.object
                }
                Expression::Identifier(identifier) => break identifier,
                _ => return None,
            };
        };
        let symbol = self.symbol(root)?;
        if self.namespaces.contains(&symbol) {
            let (head, rest) = members.split_last()?;
            return (accepts_head(head)
                && rest.iter().all(|method| SCHEMA_METHODS.contains(method)))
            .then_some(SchemaShape::Known);
        }
        if !members.iter().all(|method| SCHEMA_METHODS.contains(method)) {
            return None;
        }
        if let Some(shape) = self.locals.get(&symbol) {
            return Some(shape.clone());
        }
        self.imports
            .get(&symbol)
            .map(|local| SchemaShape::Derived(local.clone()))
    }

    /// True when a `pipe` argument is a `Schema` transform: a call such as
    /// `Schema.brand("Id")` or `Schema.minLength(1)`, a bare transform such as
    /// `Schema.optional`, or a bare constructor such as `Schema.NullOr`.
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
        let known =
            PIPE_TRANSFORMS.contains(&name) || (!called && PIPE_CONSTRUCTORS.contains(&name));
        known
            && self
                .symbol(object)
                .is_some_and(|symbol| self.namespaces.contains(&symbol))
    }

    fn symbol(&self, identifier: &IdentifierReference<'a>) -> Option<SymbolId> {
        let reference = identifier.reference_id.get()?;
        self.semantic.scoping().get_reference(reference).symbol_id()
    }
}
