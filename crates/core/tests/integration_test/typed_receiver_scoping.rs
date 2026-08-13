use super::common::{create_config, fixture_path};

#[test]
fn typed_receivers_with_reused_parameter_names_are_scoped_per_function() {
    let root = fixture_path("typed-receiver-scoping");
    let mut config = create_config(root);
    config.rules.unused_class_members = fallow_config::Severity::Error;
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused: Vec<String> = results
        .unused_class_members
        .iter()
        .map(|finding| {
            format!(
                "{}.{}",
                finding.member.parent_name, finding.member.member_name
            )
        })
        .collect();

    assert!(
        !unused.contains(&"FirstContext.firstUsed".to_string()),
        "FirstContext.firstUsed is reached through the first function's typed receiver: \
         {unused:?}"
    );
    assert!(
        !unused.contains(&"SecondContext.secondUsed".to_string()),
        "SecondContext.secondUsed is reached through the second function's typed receiver: \
         {unused:?}"
    );
    assert!(
        unused.contains(&"FirstContext.firstDead".to_string())
            && unused.contains(&"SecondContext.secondDead".to_string())
            && unused.contains(&"AliasContext.aliasDead".to_string()),
        "unrelated members must remain reportable: {unused:?}"
    );
    assert!(
        !unused.contains(&"AliasContext.aliasUsed".to_string()),
        "AliasContext.aliasUsed is exposed by the parameter's Pick surface: {unused:?}"
    );
    assert!(
        !unused.contains(&"AliasContext.pickedOnly".to_string()),
        "a literal Pick key is itself a type-level member use: {unused:?}"
    );
    assert!(
        unused.contains(&"NestedContext.aliasUsed".to_string())
            && unused.contains(&"NestedContext.nestedDead".to_string()),
        "a nested property type must not inherit direct receiver-member credit: {unused:?}"
    );
    assert!(
        !unused.contains(&"InnerHandlerContext.innerUsed".to_string()),
        "the nearest function alias must type its contextual receiver: {unused:?}"
    );
    assert!(
        unused.contains(&"InnerHandlerContext.innerDead".to_string())
            && unused.contains(&"OuterHandlerContext.outerDead".to_string()),
        "lexical alias resolution must not over-credit either class: {unused:?}"
    );
}
