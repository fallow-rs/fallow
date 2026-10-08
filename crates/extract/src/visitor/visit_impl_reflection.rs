//! Calls that give a whole class or enum to a library that reads every member
//! by reflection.
//!
//! An ORM maps each property of an entity class to a column, so a repository
//! lookup for the entity reads and writes every property. A GraphQL schema
//! builder exposes each value of an enum that `registerEnumType` receives.
//! Project code does not name these members, so without this recognizer they
//! are reported as unused.

#[allow(clippy::wildcard_imports, reason = "many call AST types used")]
use oxc_ast::ast::*;

use fallow_types::extract::ImportedName;

use super::super::ModuleInfoExtractor;

/// Member callees whose first value argument is an ORM entity class. The
/// repository getters come from `DataSource` and `EntityManager`, the `find`
/// family from `EntityManager` in TypeORM and MikroORM.
const ENTITY_ARGUMENT_METHODS: &[&str] = &[
    "getRepository",
    "getCustomRepository",
    "getTreeRepository",
    "getMongoRepository",
    "createQueryBuilder",
    "find",
    "findBy",
    "findOne",
    "findOneBy",
    "findOneOrFail",
    "findOneByOrFail",
    "findAndCount",
    "findAndCountBy",
];

/// Member callees whose first type argument is an ORM entity class. Only the
/// repository getters qualify: a generic `find<T>` is too common in code that
/// is not ORM code.
const ENTITY_TYPE_ARGUMENT_METHODS: &[&str] = &[
    "getRepository",
    "getCustomRepository",
    "getTreeRepository",
    "getMongoRepository",
];

/// Packages whose `registerEnumType` exposes every value of the enum.
const REGISTER_ENUM_TYPE_SOURCES: &[&str] = &["@nestjs/graphql", "type-graphql"];

const REGISTER_ENUM_TYPE: &str = "registerEnumType";

impl ModuleInfoExtractor {
    /// Record the entity or enum that a reflective library call reads whole.
    pub(super) fn record_reflective_whole_object_use(&mut self, expr: &CallExpression<'_>) {
        if let Some(name) = self.reflective_call_target(expr) {
            self.record_whole_object_identifier_use(name.as_str());
        }
    }

    fn reflective_call_target(&self, expr: &CallExpression<'_>) -> Option<String> {
        match &expr.callee {
            Expression::StaticMemberExpression(member) => orm_entity_target(expr, member),
            Expression::Identifier(callee) if self.is_register_enum_type(callee.name.as_str()) => {
                first_identifier_argument(expr)
            }
            _ => None,
        }
    }

    /// Whether `local` is `registerEnumType` imported from a GraphQL schema
    /// package and not shadowed at the current position.
    fn is_register_enum_type(&self, local: &str) -> bool {
        !self.nested_scope_shadows(local)
            && self.imports.iter().any(|import| {
                import.local_name == local
                    && !import.is_type_only
                    && REGISTER_ENUM_TYPE_SOURCES.contains(&import.source.as_str())
                    && matches!(
                        &import.imported_name,
                        ImportedName::Named(name) if name == REGISTER_ENUM_TYPE
                    )
            })
    }
}

fn orm_entity_target(
    expr: &CallExpression<'_>,
    member: &StaticMemberExpression<'_>,
) -> Option<String> {
    let method = member.property.name.as_str();
    if ENTITY_TYPE_ARGUMENT_METHODS.contains(&method)
        && let Some(name) = first_type_argument_name(expr)
    {
        return Some(name);
    }
    if ENTITY_ARGUMENT_METHODS.contains(&method) {
        return first_identifier_argument(expr);
    }
    None
}

fn first_identifier_argument(expr: &CallExpression<'_>) -> Option<String> {
    match expr.arguments.first()? {
        Argument::Identifier(ident) => Some(ident.name.to_string()),
        _ => None,
    }
}

fn first_type_argument_name(expr: &CallExpression<'_>) -> Option<String> {
    let first = expr.type_arguments.as_deref()?.params.first()?;
    let TSType::TSTypeReference(reference) = first else {
        return None;
    };
    match &reference.type_name {
        TSTypeName::IdentifierReference(ident) => Some(ident.name.to_string()),
        _ => None,
    }
}
