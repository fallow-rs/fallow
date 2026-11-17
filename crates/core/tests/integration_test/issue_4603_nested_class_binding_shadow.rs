use crate::common::{create_config, fixture_path};

/// Issue #4603: a member read at module scope resolves through the binding of
/// module scope. A class binding with the same name in a function body does
/// not take the read.
///
/// Each file has a module-scope `const Alias = <enum>` with the read
/// `Alias.One`, and a function body with `const Alias = new <Class>()`. In
/// `read-after.ts` the read follows the function, in `read-before.ts` it comes
/// first. No code reads the `One` field of a class, so both are unused, and
/// the enum member `One` is used.
#[test]
fn module_scope_read_does_not_credit_nested_class_binding() {
    let root = fixture_path("issue-4603-nested-class-binding-shadow");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let mut unused_class: Vec<String> = results
        .unused_class_members
        .iter()
        .map(|m| format!("{}.{}", m.member.parent_name, m.member.member_name))
        .collect();
    unused_class.sort_unstable();
    assert_eq!(unused_class, ["Early.One", "Late.One"]);

    let mut unused_enum: Vec<String> = results
        .unused_enum_members
        .iter()
        .map(|m| format!("{}.{}", m.member.parent_name, m.member.member_name))
        .collect();
    unused_enum.sort_unstable();
    assert_eq!(unused_enum, ["After.Two", "Before.Two"]);
}
