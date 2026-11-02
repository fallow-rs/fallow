//! Syntactic Effect schema shapes for `effect.schemaExportsAsTypes`.
//!
//! A declaration is a schema when its initializer, or the `extends` clause of
//! its class, is a chain of calls and static members whose root is:
//! - the `Schema` binding of an Effect import, with a schema constructor as
//!   the first member (`Schema.Struct(...)`, `Schema.String`,
//!   `Schema.Class<X>("X")({...})`), or
//! - a module-scope binding that is already a schema (`User.pipe(...)`).
//!
//! Each later member must be a method that returns a schema (`pipe`,
//! `annotations`, `extend`). The rule is conservative: a chain that ends in
//! `make`, `decode*` or a field read is not a schema.

use oxc_ast::ast::{Class, Expression};
use oxc_semantic::{Semantic, SymbolId};
use rustc_hash::FxHashSet;

use fallow_types::extract::{ImportInfo, ImportedName};

/// `Schema` functions with a lowercase name that return a schema. Every
/// other lowercase function (`decodeUnknownSync`, `is`, `isGreaterThan`)
/// returns a decoder, a guard or a filter. Each PascalCase member is a schema
/// constructor or a schema class factory (`Struct`, `String`, `Class`,
/// `TaggedClass`, `TaggedError`, `TaggedRequest`, `Error`, `ErrorClass`,
/// `TaggedErrorClass`, `Opaque`).
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
    name.starts_with(|c: char| c.is_ascii_uppercase())
        || LOWERCASE_SCHEMA_CONSTRUCTORS.contains(&name)
}

/// Schema namespace bindings and the module-scope bindings found to be
/// schemas so far, in source order.
pub struct SchemaShapes<'s, 'a> {
    semantic: &'s Semantic<'a>,
    namespaces: FxHashSet<SymbolId>,
    locals: FxHashSet<SymbolId>,
    local_names: FxHashSet<String>,
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
        Self {
            semantic,
            namespaces,
            locals: FxHashSet::default(),
            local_names: FxHashSet::default(),
        }
    }

    /// Remember a module-scope binding as a schema.
    pub fn record(&mut self, symbol: SymbolId, name: &str) {
        self.locals.insert(symbol);
        self.local_names.insert(name.to_string());
    }

    /// True when the module-scope binding `name` is a schema.
    pub fn is_schema_name(&self, name: &str) -> bool {
        self.local_names.contains(name)
    }

    /// True when the class extends a schema class factory or a schema class.
    pub fn is_schema_class(&self, class: &Class<'a>) -> bool {
        class
            .heritage
            .as_ref()
            .is_some_and(|heritage| self.is_schema_expression(&heritage.expression))
    }

    /// True when the expression evaluates to a schema.
    pub fn is_schema_expression(&self, expression: &Expression<'a>) -> bool {
        if self.namespaces.is_empty() {
            return false;
        }
        let mut members: Vec<&str> = Vec::new();
        let mut current = expression;
        let root = loop {
            current = match current.without_parentheses() {
                Expression::TSAsExpression(node) => &node.expression,
                Expression::TSSatisfiesExpression(node) => &node.expression,
                Expression::TSNonNullExpression(node) => &node.expression,
                Expression::TSInstantiationExpression(node) => &node.expression,
                Expression::CallExpression(call) => &call.callee,
                Expression::StaticMemberExpression(member) => {
                    members.push(member.property.name.as_str());
                    &member.object
                }
                Expression::Identifier(identifier) => break identifier,
                _ => return false,
            };
        };
        let Some(symbol) = root
            .reference_id
            .get()
            .and_then(|reference| self.semantic.scoping().get_reference(reference).symbol_id())
        else {
            return false;
        };
        if self.namespaces.contains(&symbol) {
            let Some((first, rest)) = members.split_last() else {
                return false;
            };
            return is_schema_constructor(first)
                && rest.iter().all(|method| SCHEMA_METHODS.contains(method));
        }
        self.locals.contains(&symbol)
            && members.iter().all(|method| SCHEMA_METHODS.contains(method))
    }
}
