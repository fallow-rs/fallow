use crate::tests::parse_ts as parse_source;

#[test]
fn unused_import_binding_detected() {
    let info = parse_source("import { foo } from './utils';");
    assert!(
        info.unused_import_bindings.contains(&"foo".to_string()),
        "Import 'foo' is never used and should be in unused_import_bindings"
    );
}

#[test]
fn used_import_binding_not_in_unused() {
    let info = parse_source("import { foo } from './utils';\nconsole.log(foo);");
    assert!(
        !info.unused_import_bindings.contains(&"foo".to_string()),
        "Import 'foo' is used and should NOT be in unused_import_bindings"
    );
}

#[test]
fn unused_namespace_import_detected() {
    let info = parse_source("import * as utils from './utils';");
    assert!(
        info.unused_import_bindings.contains(&"utils".to_string()),
        "Namespace import 'utils' is never used and should be in unused_import_bindings"
    );
}

#[test]
fn used_namespace_import_not_in_unused() {
    let info = parse_source("import * as utils from './utils';\nutils.foo();");
    assert!(
        !info.unused_import_bindings.contains(&"utils".to_string()),
        "Namespace import 'utils' is used and should NOT be in unused_import_bindings"
    );
}

#[test]
fn reexported_import_not_in_unused() {
    let info = parse_source("import { foo } from './utils';\nexport { foo };");
    assert!(
        !info.unused_import_bindings.contains(&"foo".to_string()),
        "Import 'foo' is re-exported and should NOT be in unused_import_bindings"
    );
}

#[test]
fn type_only_import_used_as_type_not_in_unused() {
    let info = parse_source("import type { Foo } from './types';\nconst x: Foo = {} as any;");
    assert!(
        !info.unused_import_bindings.contains(&"Foo".to_string()),
        "Type import 'Foo' is used as a type annotation and should NOT be in unused_import_bindings"
    );
}

#[test]
fn value_import_used_only_as_type_not_in_unused() {
    let info = parse_source("import { Foo } from './types';\nconst x: Foo = {} as any;");
    assert!(
        !info.unused_import_bindings.contains(&"Foo".to_string()),
        "Value import 'Foo' used as type annotation should NOT be in unused_import_bindings"
    );
}

#[test]
fn value_import_used_only_as_type_records_type_usage() {
    let info = parse_source("import { Foo } from './types';\nconst x = {} as Foo;");
    assert_eq!(
        info.type_referenced_import_bindings,
        vec!["Foo".to_string()]
    );
    assert_eq!(info.value_referenced_import_bindings, [] as [String; 0]);
}

#[test]
fn value_import_used_as_type_and_value_records_both_kinds() {
    let info =
        parse_source("import { Foo } from './types';\nconst x = {} as Foo;\nconsole.log(Foo);");
    assert_eq!(
        info.type_referenced_import_bindings,
        vec!["Foo".to_string()]
    );
    assert_eq!(
        info.value_referenced_import_bindings,
        vec!["Foo".to_string()]
    );
}

#[test]
fn side_effect_import_not_in_unused() {
    let info = parse_source("import './side-effect';");
    assert!(
        info.unused_import_bindings.is_empty(),
        "Side-effect imports have no binding and should not appear in unused_import_bindings"
    );
}

#[test]
fn mixed_used_and_unused_imports() {
    let info = parse_source("import { used, unused } from './utils';\nconsole.log(used);");
    assert!(
        !info.unused_import_bindings.contains(&"used".to_string()),
        "'used' is referenced"
    );
    assert!(
        info.unused_import_bindings.contains(&"unused".to_string()),
        "'unused' is not referenced"
    );
}

#[test]
fn unused_import_mixed_used_and_unused() {
    let info = parse_source("import { used, unused } from './mod';\nconsole.log(used);");
    assert!(
        info.unused_import_bindings.contains(&"unused".to_string()),
        "Unused import binding 'unused' should be detected"
    );
    assert!(
        !info.unused_import_bindings.contains(&"used".to_string()),
        "Used import binding 'used' should not be in unused list"
    );
}

#[test]
fn all_imports_used_empty_unused_list() {
    let info = parse_source("import { a, b } from './mod';\nconsole.log(a, b);");
    assert!(
        info.unused_import_bindings.is_empty(),
        "All imports used — no unused bindings expected"
    );
}

#[test]
fn side_effect_import_no_unused_bindings() {
    let info = parse_source("import './styles.css';");
    assert_eq!(info.unused_import_bindings, [] as [String; 0]);
}

#[test]
fn unused_default_import_in_unused_list() {
    let info = parse_source("import React from 'react';\nexport const x = 1;");
    assert!(
        info.unused_import_bindings.contains(&"React".to_string()),
        "Unused default import 'React' should be detected"
    );
}
