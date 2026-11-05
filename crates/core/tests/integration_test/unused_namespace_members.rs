use super::common::{create_config, fixture_path};

fn unused_namespace_members(fixture: &str) -> Vec<String> {
    let root = fixture_path(fixture);
    let config = create_config(root.clone());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let mut members: Vec<String> = results
        .unused_namespace_members
        .iter()
        .map(|finding| {
            let member = &finding.member;
            let path = member.path.strip_prefix(&root).unwrap_or(&member.path);
            format!(
                "{}:{}.{}",
                path.to_string_lossy().replace('\\', "/"),
                member.parent_name,
                member.member_name
            )
        })
        .collect();
    members.sort();
    members
}

/// Every reported member is a member that no code reads. The fixture also
/// holds each form that must stay silent: a qualified value and type read,
/// `import x = N.a`, destructuring, a nested chain, a namespace import, a
/// whole use (argument, alias, spread, `Object.keys`), a reference inside the
/// body, a merged namespace, a re-export from the entry point, a declaration
/// file, `declare global`, a module augmentation, a local namespace, and a
/// suppression comment.
#[test]
fn reports_only_unread_namespace_members() {
    assert_eq!(
        unused_namespace_members("unused-namespace-members"),
        [
            "src/legacy.ts:Color.unusedColorHelper",
            "src/legacy.ts:Legacy.unusedFn",
            "src/legacy.ts:Legacy.unusedValue",
            "src/legacy.ts:Merged.unusedHelper",
            "src/legacy.ts:Outer.Inner.unusedDeep",
            "src/legacy.ts:Outer.UnusedNested",
            "src/legacy.ts:Typed.UnusedShape",
            "src/legacy.ts:make.unusedDefaults",
            "src/more.ts:Api.Types.UnusedResponse",
            "src/more.ts:Dotted.Inner.unusedDotted",
        ]
    );
}

/// A declaration merge of a namespace with a class, an enum or a function is
/// one export, not a duplicate export.
#[test]
fn declaration_merge_with_namespace_is_not_a_duplicate_export() {
    let root = fixture_path("unused-namespace-members");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let duplicates: Vec<&str> = results
        .duplicate_exports
        .iter()
        .map(|duplicate| duplicate.export.export_name.as_str())
        .collect();
    assert!(duplicates.is_empty(), "{duplicates:?}");
}

/// `import * as Lib; export { Lib }` hands the whole module on. Fallow does
/// not follow that re-export member by member, so every member of the module
/// counts as used. A namespace in another module stays checked.
#[test]
fn re_exported_namespace_import_credits_the_whole_module() {
    assert_eq!(
        unused_namespace_members("unused-namespace-members-reexport"),
        ["src/other.ts:Other.unread"]
    );
}

/// An inline import type has no local binding, so its qualifier is the only
/// record of the member read: `typeof import("./lib").Legacy.used` reads
/// `Legacy.used`, and `import("./lib").Shapes.Point` reads the type
/// `Shapes.Point`, also through a barrel. A `typeof` of a nested namespace
/// counts as a whole use.
#[test]
fn inline_import_type_credits_the_qualified_member() {
    assert_eq!(
        unused_namespace_members("unused-namespace-members-import-type"),
        [
            "src/lib.ts:Legacy.unread",
            "src/lib.ts:Outer.Inner.unusedDeep",
            "src/lib.ts:Shapes.Unused",
        ]
    );
}
