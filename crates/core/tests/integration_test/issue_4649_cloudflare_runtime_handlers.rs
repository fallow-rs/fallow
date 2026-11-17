use fallow_config::Severity;

use crate::common::{create_config, fixture_path};

/// The Cloudflare Workers runtime calls the handlers of a `DurableObject`,
/// `WorkerEntrypoint` or `WorkflowEntrypoint` subclass by name. A subclass of a
/// base imported from `cloudflare:workers` gets credit for the handlers that
/// the base declares. Other members, other bases and other import sources
/// stay reported.
#[test]
fn cloudflare_runtime_handlers_are_credited() {
    let root = fixture_path("issue-4649-cloudflare-runtime-handlers");
    let mut config = create_config(root);
    config.rules.unused_protected_class_members = Severity::Error;
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let render =
        |m: &fallow_core::results::UnusedMember| format!("{}.{}", m.parent_name, m.member_name);
    let unused: Vec<String> = results
        .unused_class_members
        .iter()
        .map(|finding| render(&finding.member))
        .collect();
    let unused_protected: Vec<String> = results
        .unused_protected_class_members
        .iter()
        .map(|finding| render(&finding.member))
        .collect();

    let credited = [
        "Room.fetch",
        "Room.alarm",
        "Room.connect",
        "Room.webSocketMessage",
        "Room.webSocketClose",
        "Room.webSocketError",
        "ChatRoom.webSocketMessage",
        "AliasRoom.webSocketClose",
        "Api.fetch",
        "Api.scheduled",
        "Api.queue",
        "Api.email",
        "Api.tail",
        "Api.tailStream",
        "Api.trace",
        "Api.test",
        "Api.connect",
        "Pipeline.run",
        "NsRoom.webSocketError",
        "LocalChainRoom.alarm",
        "GenericChainRoom.alarm",
        "RenamedRoom.webSocketClose",
        "default.alarm",
        "CrossFileRoom.webSocketMessage",
        "SpecifierRoom.alarm",
        "default.run",
    ];
    for member in credited {
        assert!(
            !unused.contains(&member.to_string()),
            "{member} is a runtime handler and must be credited: {unused:?}"
        );
    }
    assert!(
        !unused_protected.contains(&"ProtectedRoom.alarm".to_string()),
        "a protected runtime handler must be credited: {unused_protected:?}"
    );

    let reported = [
        "Room.run",
        "Room.unusedRoomHelper",
        "Api.alarm",
        "Pipeline.fetch",
        "FakeRoom.webSocketMessage",
        "OtherRoom.webSocketMessage",
        "PlainRoom.webSocketClose",
        "NsUnknownBase.webSocketError",
        "OtherNsRoom.webSocketError",
        "LocalChainRoom.unusedLocalChainHelper",
        "default.unusedWorkflowStep",
    ];
    for member in reported {
        assert!(
            unused.contains(&member.to_string()),
            "{member} is not a runtime handler of its base and must be reported: {unused:?}"
        );
    }
    assert!(
        unused_protected.contains(&"ProtectedRoom.unusedProtectedHelper".to_string()),
        "a protected member that is not a handler must be reported: {unused_protected:?}"
    );
}
