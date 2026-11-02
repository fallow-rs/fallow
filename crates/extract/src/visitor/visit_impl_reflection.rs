//! Calls that give a whole class or enum to a library that reads every member
//! by reflection.
//!
//! An ORM maps each property of an entity class to a column, so a repository
//! lookup for the entity reads and writes every property. A GraphQL schema
//! builder exposes each value of an enum that `registerEnumType` receives.
//! Project code does not name these members, so without this recognizer they
//! are reported as unused.
//!
//! A runtime probe for a static member name (`"name" in value`,
//! `Reflect.has(value, "name")`) is recorded as a member name probe.

#[allow(clippy::wildcard_imports, reason = "many call AST types used")]
use oxc_ast::ast::*;

use fallow_types::extract::{ImportedName, MemberNameProbeFact, SemanticFact};

use super::super::ModuleInfoExtractor;
use super::visit_security_classifiers::static_string_literal_value;

/// Member callees whose first value argument is an ORM entity class. These
/// names are specific to ORM code, so they need no import evidence. A project
/// can wrap the ORM in its own manager class with the same method names.
const ENTITY_ARGUMENT_METHODS: &[&str] = &[
    "getRepository",
    "getCustomRepository",
    "getTreeRepository",
    "getMongoRepository",
    "createQueryBuilder",
];

/// `EntityManager` methods in TypeORM and MikroORM whose first value argument
/// is an entity class. Arrays and many other values have methods with these
/// names, so the file must import from an ORM package.
const ORM_FIND_METHODS: &[&str] = &[
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

/// Packages whose import makes the `find` family an ORM call.
const ORM_PACKAGES: &[&str] = &["typeorm", "@nestjs/typeorm"];

/// Scope whose packages make the `find` family an ORM call.
const ORM_PACKAGE_SCOPE: &str = "@mikro-orm/";

/// Packages whose `registerEnumType` exposes every value of the enum.
const REGISTER_ENUM_TYPE_SOURCES: &[&str] = &["@nestjs/graphql", "type-graphql"];

const REGISTER_ENUM_TYPE: &str = "registerEnumType";

impl ModuleInfoExtractor {
    /// Record the entity or enum that a reflective library call reads whole.
    pub(super) fn record_reflective_whole_object_use(&mut self, expr: &CallExpression<'_>) {
        self.record_reflect_has_member_probe(expr);
        if let Some(name) = self.reflective_call_target(expr) {
            self.record_whole_object_identifier_use(name.as_str());
        }
    }

    fn reflective_call_target(&self, expr: &CallExpression<'_>) -> Option<String> {
        match &expr.callee {
            Expression::StaticMemberExpression(member) => self.orm_entity_target(expr, member),
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

    fn orm_entity_target(
        &self,
        expr: &CallExpression<'_>,
        member: &StaticMemberExpression<'_>,
    ) -> Option<String> {
        let method = member.property.name.as_str();
        if ENTITY_TYPE_ARGUMENT_METHODS.contains(&method)
            && let Some(name) = first_type_argument_name(expr)
        {
            return Some(name);
        }
        let entity_method = ENTITY_ARGUMENT_METHODS.contains(&method)
            || (ORM_FIND_METHODS.contains(&method) && self.imports_orm_package());
        if entity_method {
            return first_identifier_argument(expr);
        }
        None
    }

    /// Record the member name of a `"name" in value` probe. A probe for a
    /// static name is a runtime check for a member with that name, so a class
    /// can supply the member only for the probe to read it.
    pub(super) fn record_in_operator_member_probe(&mut self, expr: &BinaryExpression<'_>) {
        if expr.operator == BinaryOperator::In
            && let Some(name) = static_string_literal_value(&expr.left)
        {
            self.semantic_facts
                .push(SemanticFact::MemberNameProbe(MemberNameProbeFact { name }));
        }
    }

    /// Record the member name of a `Reflect.has(value, "name")` probe, which
    /// has the semantics of the `in` operator. `Object.hasOwn` and
    /// `hasOwnProperty` do not see a getter or a method on the class
    /// prototype, so they are not probes for class members.
    fn record_reflect_has_member_probe(&mut self, expr: &CallExpression<'_>) {
        let Expression::StaticMemberExpression(member) = &expr.callee else {
            return;
        };
        if member.property.name != "has" {
            return;
        }
        let Expression::Identifier(object) = &member.object else {
            return;
        };
        if object.name != "Reflect" || self.nested_scope_shadows("Reflect") {
            return;
        }
        if let Some(name) = expr
            .arguments
            .get(1)
            .and_then(Argument::as_expression)
            .and_then(static_string_literal_value)
        {
            self.semantic_facts
                .push(SemanticFact::MemberNameProbe(MemberNameProbeFact { name }));
        }
    }

    /// Whether the file imports from an ORM package before the current call.
    fn imports_orm_package(&self) -> bool {
        self.imports.iter().any(|import| {
            let source = import.source.as_str();
            ORM_PACKAGES.contains(&source) || source.starts_with(ORM_PACKAGE_SCOPE)
        })
    }
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
