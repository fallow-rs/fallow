//! Calls that give a whole class or enum to a library that reads every member.

use crate::tests::parse_ts as parse_source;

fn has_whole_use(source: &str, name: &str) -> bool {
    parse_source(source)
        .whole_object_uses
        .iter()
        .any(|used| used == name)
}

#[test]
fn repository_lookup_with_entity_argument_is_whole_use() {
    let source = "import { Company } from './company.entity';\n\
                  const repo = dataSource.getRepository(Company);";
    assert!(has_whole_use(source, "Company"));
}

#[test]
fn entity_manager_find_with_entity_argument_is_whole_use() {
    let source = "import { EntityManager } from 'typeorm';\n\
                  import { User } from './user.entity';\n\
                  declare const manager: EntityManager;\n\
                  await manager.findOneBy(User, { id: 1 });";
    assert!(has_whole_use(source, "User"));
}

#[test]
fn find_with_mikro_orm_import_is_whole_use() {
    let source = "import { EntityManager } from '@mikro-orm/core';\n\
                  import { User } from './user.entity';\n\
                  declare const em: EntityManager;\n\
                  await em.find(User, {});";
    assert!(has_whole_use(source, "User"));
}

#[test]
fn find_without_orm_import_is_not_whole_use() {
    let source = "import { Status } from './status';\n\
                  const match = list.find(Status);\n\
                  await manager.findOneBy(Status, { id: 1 });";
    assert!(!has_whole_use(source, "Status"));
}

#[test]
fn repository_lookup_without_orm_import_is_whole_use() {
    let source = "import { Company } from './company.entity';\n\
                  const repo = this.workspaceOrmManager.getRepository(Company);";
    assert!(has_whole_use(source, "Company"));
}

#[test]
fn repository_lookup_with_entity_type_argument_is_whole_use() {
    let source = "import { type Note } from './note.entity';\n\
                  const repo = orm.getRepository<Note>('note');";
    assert!(has_whole_use(source, "Note"));
}

#[test]
fn plain_call_with_class_argument_is_not_whole_use() {
    let source = "import { Company } from './company.entity';\n\
                  register(Company);\n\
                  service.lookup(Company);";
    assert!(!has_whole_use(source, "Company"));
}

#[test]
fn bare_repository_function_is_not_whole_use() {
    let source = "import { Company } from './company.entity';\n\
                  getRepository(Company);";
    assert!(!has_whole_use(source, "Company"));
}

#[test]
fn entity_in_a_later_argument_is_not_whole_use() {
    let source = "import { Company } from './company.entity';\n\
                  manager.find(Person, Company);";
    assert!(!has_whole_use(source, "Company"));
}

#[test]
fn find_type_argument_is_not_whole_use() {
    let source = "import { type Company } from './company.entity';\n\
                  manager.find<Company>(query);";
    assert!(!has_whole_use(source, "Company"));
}

#[test]
fn nest_graphql_register_enum_type_is_whole_use() {
    let source = "import { registerEnumType } from '@nestjs/graphql';\n\
                  export enum Status { Active = 'ACTIVE', Closed = 'CLOSED' }\n\
                  registerEnumType(Status, { name: 'Status' });";
    assert!(has_whole_use(source, "Status"));
}

#[test]
fn type_graphql_register_enum_type_is_whole_use() {
    let source = "import { registerEnumType as reg } from 'type-graphql';\n\
                  import { Role } from './role';\n\
                  reg(Role, { name: 'Role' });";
    assert!(has_whole_use(source, "Role"));
}

#[test]
fn register_enum_type_from_other_package_is_not_whole_use() {
    let source = "import { registerEnumType } from './local-helpers';\n\
                  import { Role } from './role';\n\
                  registerEnumType(Role, { name: 'Role' });";
    assert!(!has_whole_use(source, "Role"));
}

#[test]
fn shadowed_register_enum_type_is_not_whole_use() {
    let source = "import { registerEnumType } from '@nestjs/graphql';\n\
                  import { Role } from './role';\n\
                  function setup(registerEnumType: (value: unknown) => void) {\n\
                    registerEnumType(Role);\n\
                  }";
    assert!(!has_whole_use(source, "Role"));
}
