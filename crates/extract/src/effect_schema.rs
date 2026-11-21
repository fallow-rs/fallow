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
//! arrow function is not a schema. A chain on a member of a namespace import
//! (`Imported.User.pipe(...)`) is not a schema: the root is the namespace.
//!
//! A call of a function is a schema when each `return` of the function is a
//! schema (`const makeUser = () => Schema.Struct({...})`, then `makeUser()`).
//! `effect_schema_helpers.rs` has the rules for such a function.

use std::cell::RefCell;

use oxc_ast::ast::{Argument, Class, Expression, IdentifierReference};
use oxc_semantic::{Semantic, SymbolId};
use rustc_hash::{FxHashMap, FxHashSet};

use fallow_types::extract::{ImportInfo, ImportedName};

#[path = "effect_schema_names.rs"]
mod names;
use names::{HTTP_API_PIPE_TRANSFORMS, PIPE_TRANSFORMS, TYPE_ONLY_NAMES};

#[path = "effect_schema_helpers.rs"]
mod helpers;

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
    /// a schema.
    Derived {
        import_local: String,
        through_call: bool,
    },
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
    locals: FxHashMap<SymbolId, SchemaShape>,
    local_names: FxHashMap<String, SchemaShape>,
    /// Functions whose each `return` is a schema, with the shape that a call
    /// gives.
    helpers: RefCell<FxHashMap<SymbolId, SchemaShape>>,
    /// `const` schemas in the body of an inspected function.
    helper_locals: RefCell<FxHashMap<SymbolId, SchemaShape>>,
    /// The functions under inspection: a function that returns a call of a
    /// second function adds one.
    helpers_in_progress: RefCell<FxHashSet<SymbolId>>,
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
            locals: FxHashMap::default(),
            local_names: FxHashMap::default(),
            helpers: RefCell::new(FxHashMap::default()),
            helper_locals: RefCell::new(FxHashMap::default()),
            helpers_in_progress: RefCell::new(FxHashSet::default()),
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
        // The count of calls directly on the root: 1 for `root()` and
        // `root().pipe(...)`, 0 for `root.pipe(...)`, 2 for `root()()`.
        let mut root_calls = 0_u32;
        let mut current = expression;
        let root = loop {
            current = match unwrap(current) {
                Expression::CallExpression(call) => {
                    root_calls += 1;
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
                    root_calls = 0;
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
        if let Some(shape) = self.helper_locals.borrow().get(&symbol) {
            return Some(shape.clone());
        }
        let through_call = root_calls == 1;
        if let Some(local) = self.imports.get(&symbol) {
            return Some(SchemaShape::Derived {
                import_local: local.clone(),
                through_call,
            });
        }
        if through_call {
            return self.helper_call_shape(symbol);
        }
        None
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

    fn symbol(&self, identifier: &IdentifierReference<'a>) -> Option<SymbolId> {
        let reference = identifier.reference_id.get()?;
        self.semantic.scoping().get_reference(reference).symbol_id()
    }
}
