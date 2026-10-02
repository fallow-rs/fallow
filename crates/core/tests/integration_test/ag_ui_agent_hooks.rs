//! End-to-end test for the AG-UI plugin against the
//! `tests/fixtures/ag-ui-agent-hooks/` fixture. The agent runtime calls the
//! `AbstractAgent` hooks (`run`, `clone`, `onFinalize`) on a subclass, so the
//! project code never calls them. A non-hook method on the same class must
//! still be reported.

use super::common::{create_config, fixture_path};

#[test]
fn agent_runtime_hooks_are_credited_but_real_dead_members_survive() {
    let root = fixture_path("ag-ui-agent-hooks");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_members: Vec<String> = results
        .unused_class_members
        .iter()
        .map(|finding| {
            format!(
                "{}.{}",
                finding.member.parent_name, finding.member.member_name
            )
        })
        .collect();

    assert_eq!(
        unused_members,
        vec!["EchoAgent.formatReply".to_string()],
        "only the non-hook method may surface as unused-class-member"
    );
}
