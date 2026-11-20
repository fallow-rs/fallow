use crate::common::{create_config, fixture_path};

/// Issue #4824: a computed member call through a local is a use of the whole
/// object that the local holds, also when a sibling function declares a local
/// of the same name that holds another class.
///
/// - `src/main.ts`: two functions each declare `view` with another class and
///   call a member through a key of type `string`. Each member of `A` and `B`
///   is used.
/// - `src/partial.ts`: one function uses the whole object of `C`. A sibling
///   function calls only `D.a`. The whole-object use of `C` must not credit
///   `D`, so `D.b` stays unused.
#[test]
fn whole_object_use_survives_same_name_local_in_sibling_function() {
    let root = fixture_path("issue-4824-whole-object-same-name-local");
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
        ["D.b"],
        "each computed access credits only the class of its own local"
    );
}
