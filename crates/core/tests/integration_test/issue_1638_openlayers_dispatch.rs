use crate::common::{create_config, fixture_path};

/// Issue #1638 (GAP 1): a method on a subclass of an OpenLayers
/// `ol/interaction/*` base that OpenLayers dispatches by convention
/// (`handleEvent`, `handle*Event`, `stopDown`) is runtime-used and must not be
/// reported as an unused class member. The credit is gated on the
/// `ol/interaction/*` import source, so a same-named LOCAL base does not credit,
/// and a genuinely dead member on the same class still reports.
#[test]
fn openlayers_dispatched_methods_are_not_flagged() {
    let root = fixture_path("issue-1638-openlayers-dispatch");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_members: Vec<String> = results
        .unused_class_members
        .iter()
        .map(|m| format!("{}.{}", m.member.parent_name, m.member.member_name))
        .collect();

    assert!(
        !unused_members.contains(&"DragInteraction.handleEvent".to_string()),
        "OpenLayers-dispatched `handleEvent` must be credited: {unused_members:?}"
    );
    assert!(
        !unused_members.contains(&"DragInteraction.handleDragEvent".to_string()),
        "OpenLayers-dispatched `handleDragEvent` must be credited: {unused_members:?}"
    );

    // Non-vacuous control: a genuine dead member on the same credited class must
    // still report (proves the class IS scanned, not wholesale-skipped).
    assert!(
        unused_members.contains(&"DragInteraction.trulyUnused".to_string()),
        "a genuinely-unused member on the OpenLayers subclass must still report: {unused_members:?}"
    );

    // Import-source gate: a subclass of a LOCAL same-named `PointerInteraction`
    // must NOT have its dispatched-name member credited.
    assert!(
        unused_members.contains(&"FakeInteraction.handleEvent".to_string()),
        "a dispatched-name member on a LOCAL (non-OpenLayers) base must still report: \
         {unused_members:?}"
    );
}
