//! An exported instance from a static singleton getter credits the members
//! that other modules call on it. See issue #5233.

use super::common::{create_config, fixture_path};

#[test]
fn exported_singleton_instance_credits_called_members() {
    let root = fixture_path("issue-5233-exported-singleton-instance");
    let mut config = create_config(root);
    config.rules.unused_class_members = fallow_config::Severity::Error;
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let mut unused: Vec<String> = results
        .unused_class_members
        .iter()
        .map(|m| format!("{}.{}", m.member.parent_name, m.member.member_name))
        .collect();
    unused.sort();

    assert_eq!(
        unused,
        [
            "Annotated.unusedAnnotated",
            "ArrowReset.createScene",
            "LazyThis.unusedLazyThis",
            "Local.createScene",
            "Local.reset",
            "Logical.unusedLogical",
            "NotInstance.ping",
            "NotInstance.pong",
            "Nullish.unusedNullish",
            "PrivateShadow.createScene",
            "Remote.reset",
            "Remote.unusedRemote",
            "Unannotated.unusedUnannotated",
            "Wrapped.unusedWrapped",
        ],
        "each `createScene` call on an exported singleton instance is a use. \
         These give no instance: a static method that returns a string or the \
         class, a getter that returns a field that only a nested arrow \
         function assigns, a private field that has the name of a public \
         instance field, and an export name that two different factory calls \
         bind"
    );
}
