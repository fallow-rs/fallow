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
            && unused.contains(&"SecondContext.secondDead".to_string()),
        "unrelated members must remain reportable: {unused:?}"
    );
}
