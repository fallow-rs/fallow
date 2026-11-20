use crate::common::{create_config, fixture_path};

/// Issue #4812: a computed member access on a typed parameter credits the
/// members that its key can name.
///
/// `src/main.ts` has one function for each key form. Each class in
/// `src/views.ts` has the members `zoomIn`, `zoomOut` and `other`:
/// - a key typed as a union of string literals (`view[action]()`), which
///   credits only `zoomIn` and `zoomOut`,
/// - a key of type `string` (`view[action as keyof StringView]()`), which
///   can name any member, so it credits each member,
/// - a key whose name an inner parameter declares again, which can be the
///   inner `string` parameter, so it credits each member,
/// - a string-literal key (`view["zoomIn"]()`), which credits only `zoomIn`,
/// - a computed key on an inner parameter that hides the typed parameter,
///   which does not credit the class of the outer parameter.
#[test]
fn computed_key_on_typed_parameter_credits_named_members() {
    let root = fixture_path("issue-4812-computed-key-param-member");
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
        [
            "LiteralView.other",
            "LiteralView.zoomOut",
            "ShadowView.other",
            "ShadowView.zoomOut",
            "UnionView.other",
        ],
        "a computed key credits only the members that it can name"
    );
}
