use crate::common::{create_config, fixture_path};

/// Issue #4723: the static members of a class that code passes on as a
/// value are used. A call argument (`run(CallArgument)`), a property value
/// (`{ adapter: PropertyValue }`) and a `const` alias that code passes on
/// give the class to code that can read each static member through an
/// interface type. The instance members stay unused, because only an
/// instance can read them.
///
/// A class that code reads only by name (`DirectRead.used()`) does not
/// leave, so its other static member stays unused.
///
/// Issue #4724: a read through `Class.prototype.member` is a use of the
/// instance member `member`. A read through a same-file object holder,
/// `holder[name].prototype.member` or `holder.key.prototype.member`, is a
/// use of the member of each class that the holder can give.
#[test]
fn static_members_of_a_class_passed_as_a_value_are_used() {
    let root = fixture_path("issue-4723-class-value-escape");
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
            "AliasArgument.instanceOnly",
            "CallArgument.instanceOnly",
            "DirectRead.dead",
            "HolderRead.other",
            "KeyedRead.missed",
            "PropertyValue.instanceOnly",
            "PrototypeRead.other",
        ],
        "only instance members of a passed class and unread members are unused"
    );
}
