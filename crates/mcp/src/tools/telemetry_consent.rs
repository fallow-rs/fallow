//! The one-time consent form for opt-in telemetry.
//!
//! Agent users run Fallow through MCP and never see the telemetry note of
//! the human CLI. So the server can ask the person once, in an MCP form,
//! whether to share usage data. The form follows these rules:
//!
//! - It goes out only on the `elicitation/create` route (a client that
//!   declares form elicitation on a protocol before `2026-07-28`), after the
//!   first tool call of the process, and only when no Fallow tool call has
//!   run for [`IDLE_BEFORE_ASK`]. A `fix_apply` confirmation runs inside a
//!   call, so the two forms do not overlap while that call waits.
//! - `fallow telemetry claim-prompt` decides whether the machine may ask. It
//!   refuses with a kill switch, in CI, with an environment override, after
//!   a recorded decision, and past the ask limit that all servers of the
//!   machine share.
//! - Only an explicit `true` in the field turns telemetry on. A submit with
//!   the box clear, or a decline, turns it off and records that decision. A
//!   cancel, a missing field or a client error records nothing.
//! - The server never mentions the form to the agent, in instructions or in
//!   tool results.

use std::collections::BTreeMap;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use rmcp::model::{
    BooleanSchema, ClientResult, ElicitRequest, ElicitRequestParams, ElicitResult,
    ElicitationAction, ElicitationSchema, PrimitiveSchemaDefinition, ServerRequest,
};
use rmcp::service::{Peer, PeerRequestOptions, RoleServer};
use serde_json::Value;
use tokio::process::Command;

use super::call_telemetry;
use super::fix_confirm::ConfirmRoute;

/// How long the server waits with no running call before it asks. A quiet
/// server is a sign that the agent turn ended and the person reads the
/// screen.
const IDLE_BEFORE_ASK: Duration = Duration::from_secs(20);

/// How often the waiting task looks at the call activity.
const IDLE_POLL: Duration = Duration::from_secs(2);

/// How long the server waits for one `fallow telemetry` command.
const COMMAND_TIMEOUT: Duration = Duration::from_secs(10);

/// The one boolean field of the form.
const SHARE_FIELD: &str = "share_usage_data";

const MESSAGE: &str = "Fallow asks once: do you want to share anonymous usage data? \
A yes turns on telemetry for all Fallow runs on this machine, from the CLI and from MCP. \
Each event holds only coarse, allowlisted values, for example the command or tool, \
a time range, the result type, size ranges, the operating system and the agent name. \
Fallow never sends code, file paths, package names, repository names or error text. \
A random install ID goes with the events. Fallow deletes it from this machine when you \
turn telemetry off. The events go to api.fallow.cloud. Full field list: \
https://github.com/fallow-rs/fallow/blob/main/docs/telemetry.md. \
To change your choice later, run `fallow telemetry enable` or `fallow telemetry disable`.";

/// Set when this process has started its one consent attempt.
static STARTED: AtomicBool = AtomicBool::new(false);

/// The decision that a form answer gives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Decision {
    /// Run `fallow telemetry enable`.
    Share,
    /// Run `fallow telemetry disable`, which records the decision.
    KeepOff,
    /// Record nothing. A later session may ask again.
    Nothing,
}

/// Start the consent attempt of this process after a completed tool call,
/// when the client can show the form. Returns at once.
pub fn offer_after_call(binary: &str, peer: Peer<RoleServer>, route: ConfirmRoute) {
    if route != ConfirmRoute::Request
        || fallow_types::telemetry_opt_out::telemetry_opted_out()
        || STARTED.swap(true, Ordering::SeqCst)
    {
        return;
    }
    let binary = binary.to_owned();
    tokio::spawn(async move {
        wait_until_idle().await;
        if !claim(&binary).await {
            return;
        }
        // A call can start while the claim runs; wait for quiet again.
        wait_until_idle().await;
        let decision = ask(&peer).await;
        apply(&binary, decision).await;
    });
}

async fn wait_until_idle() {
    loop {
        tokio::time::sleep(IDLE_POLL).await;
        if call_telemetry::idle_for().is_some_and(|idle| idle >= IDLE_BEFORE_ASK) {
            return;
        }
    }
}

/// Ask the CLI whether this machine may show the form now. The CLI counts
/// the ask when it answers yes.
async fn claim(binary: &str) -> bool {
    let Some(stdout) = run_telemetry(binary, &["claim-prompt"]).await else {
        return false;
    };
    serde_json::from_slice::<Value>(&stdout).is_ok_and(|value| value["ask"] == Value::Bool(true))
}

/// Send the form and wait for the answer. There is no timeout: nothing
/// waits for the answer, and a form that stays on screen after the server
/// stopped listening would take an answer that nobody records.
async fn ask(peer: &Peer<RoleServer>) -> Decision {
    let request = ServerRequest::ElicitRequest(ElicitRequest::new(form_params()));
    let Ok(handle) = peer
        .send_request_with_option(request, PeerRequestOptions::no_options())
        .await
    else {
        return Decision::Nothing;
    };
    match handle.await_response().await {
        Ok(ClientResult::ElicitResult(result)) => decision_from(&result),
        _ => Decision::Nothing,
    }
}

fn form_params() -> ElicitRequestParams {
    let field = BooleanSchema::new()
        .with_default(false)
        .title("Share anonymous usage data")
        .description("Leave this clear to keep telemetry off. Fallow will not ask again.");
    let mut properties = BTreeMap::new();
    properties.insert(
        SHARE_FIELD.to_string(),
        PrimitiveSchemaDefinition::Boolean(field),
    );
    let mut schema = ElicitationSchema::new(properties);
    schema.required = Some(vec![SHARE_FIELD.to_string()]);
    ElicitRequestParams::FormElicitationParams {
        meta: None,
        message: MESSAGE.to_string(),
        requested_schema: schema,
    }
}

/// Read the decision from a form answer. Only an explicit `true` shares,
/// so a client or a hook that accepts forms without a person cannot turn
/// telemetry on.
fn decision_from(result: &ElicitResult) -> Decision {
    match result.action {
        ElicitationAction::Accept => match result
            .content
            .as_ref()
            .and_then(|content| content.get(SHARE_FIELD))
        {
            Some(Value::Bool(true)) => Decision::Share,
            Some(Value::Bool(false)) => Decision::KeepOff,
            _ => Decision::Nothing,
        },
        ElicitationAction::Decline => Decision::KeepOff,
        _ => Decision::Nothing,
    }
}

async fn apply(binary: &str, decision: Decision) {
    let command = match decision {
        Decision::Share => "enable",
        Decision::KeepOff => "disable",
        Decision::Nothing => return,
    };
    if run_telemetry(binary, &[command]).await.is_some() {
        call_telemetry::forget_state().await;
    }
}

/// Run `fallow telemetry <args> --format json --quiet` and return its
/// stdout on success.
async fn run_telemetry(binary: &str, args: &[&str]) -> Option<Vec<u8>> {
    let mut command = Command::new(binary);
    command
        .arg("telemetry")
        .args(args)
        .args(["--format", "json", "--quiet"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    let output = tokio::time::timeout(COMMAND_TIMEOUT, command.output())
        .await
        .ok()?
        .ok()?;
    output.status.success().then_some(output.stdout)
}

#[cfg(test)]
mod tests {
    use rmcp::model::ElicitResult;

    use super::*;

    fn accept(content: Option<Value>) -> ElicitResult {
        let mut result = ElicitResult::new(ElicitationAction::Accept);
        result.content = content;
        result
    }

    #[test]
    fn only_an_explicit_true_shares() {
        let yes = accept(Some(serde_json::json!({ SHARE_FIELD: true })));
        assert_eq!(decision_from(&yes), Decision::Share);
    }

    #[test]
    fn a_clear_box_or_a_decline_keeps_telemetry_off() {
        let no = accept(Some(serde_json::json!({ SHARE_FIELD: false })));
        assert_eq!(decision_from(&no), Decision::KeepOff);
        let decline = ElicitResult::new(ElicitationAction::Decline);
        assert_eq!(decision_from(&decline), Decision::KeepOff);
    }

    #[test]
    fn a_missing_or_malformed_field_or_a_cancel_records_nothing() {
        for content in [
            None,
            Some(serde_json::json!({})),
            Some(serde_json::json!({ SHARE_FIELD: "true" })),
            Some(serde_json::json!({ "apply": true })),
        ] {
            assert_eq!(decision_from(&accept(content)), Decision::Nothing);
        }
        let cancel = ElicitResult::new(ElicitationAction::Cancel);
        assert_eq!(decision_from(&cancel), Decision::Nothing);
    }

    #[test]
    fn the_form_has_one_required_field_that_defaults_to_off() {
        let ElicitRequestParams::FormElicitationParams {
            message,
            requested_schema,
            ..
        } = form_params()
        else {
            panic!("a form");
        };
        assert!(message.starts_with("Fallow"));
        let schema = serde_json::to_value(&requested_schema).expect("schema JSON");
        assert_eq!(schema["required"], serde_json::json!([SHARE_FIELD]));
        assert_eq!(schema["properties"][SHARE_FIELD]["type"], "boolean");
        assert_eq!(schema["properties"][SHARE_FIELD]["default"], false);
    }

    #[test]
    fn the_message_names_the_scope_the_id_and_the_endpoint() {
        for part in [
            "all Fallow runs on this machine",
            "random install ID",
            "api.fallow.cloud",
            "never sends code",
            "fallow telemetry disable",
        ] {
            assert!(MESSAGE.contains(part), "the message must say: {part}");
        }
    }
}
