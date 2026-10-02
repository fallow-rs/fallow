use crate::common::{create_config, fixture_path};

/// The `nestjs-trpc` module calls `create`, `onError`, and `use` on classes
/// that implement its interfaces. These methods must not be reported as
/// `unused-class-member`. Other methods on the same classes, and same-named
/// methods on classes that implement no such interface, must still report.
#[test]
fn nestjs_trpc_interface_methods_are_credited() {
    let root = fixture_path("nestjs-trpc-interfaces");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let mut unused: Vec<String> = results
        .unused_class_members
        .iter()
        .map(|m| format!("{}.{}", m.member.parent_name, m.member.member_name))
        .collect();
    unused.sort();

    assert_eq!(
        unused,
        vec![
            "AppContext.helper".to_string(),
            "PlainHandler.onError".to_string(),
        ],
        "only methods that the module does not call must report"
    );
}
