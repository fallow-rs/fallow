use crate::common::{create_config, fixture_path};

/// The Cloudflare Workers runtime calls the handlers of a `DurableObject`,
/// `WorkerEntrypoint` or `WorkflowEntrypoint` subclass by name. A subclass of a
/// base imported from `cloudflare:workers` gets credit for the handlers that
/// the base declares. Other members, other bases and other import sources
/// stay reported.
#[test]
fn cloudflare_runtime_handlers_are_credited() {
    let root = fixture_path("issue-4649-cloudflare-runtime-handlers");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused: Vec<String> = results
        .unused_class_members
        .iter()
        .map(|m| format!("{}.{}", m.member.parent_name, m.member.member_name))
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
    ];
    for member in credited {
        assert!(
            !unused.contains(&member.to_string()),
            "{member} is a runtime handler and must be credited: {unused:?}"
        );
    }

    let reported = [
        "Room.run",
        "Room.unusedRoomHelper",
        "Api.alarm",
        "Pipeline.fetch",
        "FakeRoom.webSocketMessage",
        "OtherRoom.webSocketMessage",
        "PlainRoom.webSocketClose",
    ];
    for member in reported {
        assert!(
            unused.contains(&member.to_string()),
            "{member} is not a runtime handler of its base and must be reported: {unused:?}"
        );
    }
}
