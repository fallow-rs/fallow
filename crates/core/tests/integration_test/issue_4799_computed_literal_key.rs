use crate::common::{create_config, fixture_path};

/// Issue #4799: a computed member call reads the members that its key can
/// name.
///
/// `src/main.ts` has one class for each key form. Each class has the members
/// `zoomIn`, `zoomOut` and `other`:
/// - a class field with a key typed as a union of string literals
///   (`this.view[action]()`, `action: "zoomIn" | "zoomOut"`),
/// - a local variable with the same key type (`view[action]()`),
/// - a local variable with an unannotated `const` string key
///   (`const key = "zoomOut"`), which stays a use of the whole object,
/// - a class field with a key of another type (`action as keyof OpenView`),
/// - a class field with a parameter key that hides an outer parameter of the
///   same name with a literal type.
///
/// `src/shadow.ts` has a conditional key (`forward ? "zoomIn" : "zoomOut"`)
/// and five keys with a literal type that another binding of the same name
/// can hide: a `catch` parameter, a hoisted `var`, a later `const` in a
/// block, a parameter in a namespace, and a `const` in a class static block
/// that has the name of an import. The visitor does not resolve scopes, so
/// these keys can name any member.
///
/// A key with a literal type credits only the members that it names. A key of
/// another type can name any member, so each member of the class is used.
#[test]
fn computed_member_call_credits_literal_key_members() {
    let root = fixture_path("issue-4799-computed-literal-key");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let mut unused: Vec<String> = results
        .unused_class_members
        .iter()
        .map(|m| format!("{}.{}", m.member.parent_name, m.member.member_name))
        .collect();
    unused.sort_unstable();

    assert_eq!(
        unused,
        ["CondView.other", "FieldView.other", "LocalView.other",],
        "only the members that no computed key can name are unused"
    );
}
