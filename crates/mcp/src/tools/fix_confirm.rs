//! Confirmation of `fix_apply` through MCP form elicitation.
//!
//! A client that declares form elicitation gets one confirmation form before
//! `fix_apply` writes. The route depends on the protocol revision:
//!
//! - Before `2026-07-28`, the server sends `elicitation/create` and waits for
//!   the answer inside the same call.
//! - From `2026-07-28`, the protocol forbids server-to-client requests. The
//!   call returns `InputRequiredResult` with the form, and the client calls
//!   the tool again with the answer in `inputResponses`.
//!
//! A client without form elicitation keeps the unconfirmed apply.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::time::Duration;

use rmcp::model::{
    BooleanSchema, CallToolResponse, CallToolResult, ClientCapabilities, ContentBlock,
    ElicitRequest, ElicitRequestParams, ElicitResult, ElicitationAction, ElicitationSchema,
    InputRequest, InputRequests, InputRequiredResult, InputResponses, PrimitiveSchemaDefinition,
    ProtocolVersion,
};
use serde_json::{Value, json};

/// The key of the confirmation form in `inputRequests` and `inputResponses`.
pub const CONFIRM_INPUT_KEY: &str = "confirm_fix_apply";

/// The boolean field of the form. An accepted form with this field `false`
/// counts as a decline.
const CONFIRM_FIELD: &str = "apply";

/// The prefix of `requestState`. The rest is the digest of the planned fixes.
const STATE_PREFIX: &str = "fix-plan-v1:";

/// How long the server waits for an answer to `elicitation/create`. A client
/// that declares elicitation but has no person to answer must not hold the
/// call open for the full tool timeout.
pub const CONFIRM_TIMEOUT: Duration = Duration::from_secs(600);

/// The most fix types that the form message names one by one.
const MAX_LISTED_TYPES: usize = 8;

/// How `fix_apply` confirms with the client.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfirmRoute {
    /// The client has no form elicitation: apply without a question.
    Unconfirmed,
    /// Send `elicitation/create` and wait for the answer.
    Request,
    /// Return `InputRequiredResult` and read the answer on the retry.
    InputRequired,
}

/// Select the confirmation route from the protocol revision of the request
/// and the client capabilities.
///
/// A bare `elicitation: {}` declares form mode, as the spec defines for
/// backward compatibility. A declaration with only `url` does not.
#[must_use]
pub fn confirm_route(
    version: Option<&ProtocolVersion>,
    capabilities: Option<&ClientCapabilities>,
) -> ConfirmRoute {
    let Some(elicitation) = capabilities.and_then(|caps| caps.elicitation.as_ref()) else {
        return ConfirmRoute::Unconfirmed;
    };
    let form = elicitation.form.is_some() || elicitation.url.is_none();
    if !form {
        return ConfirmRoute::Unconfirmed;
    }
    if version.is_some_and(|version| *version >= ProtocolVersion::V_2026_07_28) {
        ConfirmRoute::InputRequired
    } else {
        ConfirmRoute::Request
    }
}

/// The counts of a readable `fallow fix --dry-run` plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlanCounts {
    /// Planned fixes, without skipped entries.
    pub fixes: usize,
    /// Distinct files that the planned fixes change or create.
    pub files: usize,
    /// Entries that the run skips.
    pub skipped: usize,
}

/// The summary of a `fallow fix --dry-run` plan that the form shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FixPlan {
    /// `None` when the preview output is not a fix envelope, for example
    /// when the output limit cut it.
    pub counts: Option<PlanCounts>,
    /// Planned fixes per fix type.
    pub by_type: BTreeMap<String, usize>,
    /// Digest of the planned fixes, carried in `requestState`.
    pub digest: String,
}

impl FixPlan {
    /// True when the preview has no fix to apply.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.counts.is_some_and(|counts| counts.fixes == 0)
    }
}

/// Summarize the text of a `fix_preview` result.
///
/// The digest covers the `fixes` array when the text is a fix envelope, and
/// the raw text otherwise, so the confirmation binds to the plan that the
/// person saw.
#[must_use]
pub fn summarize_preview(text: &str) -> FixPlan {
    let parsed = serde_json::from_str::<Value>(text).ok();
    let fixes = parsed
        .as_ref()
        .and_then(|value| value.get("fixes"))
        .and_then(Value::as_array);
    let Some(fixes) = fixes else {
        return FixPlan {
            counts: None,
            by_type: BTreeMap::new(),
            digest: digest(text),
        };
    };
    let mut by_type = BTreeMap::new();
    let mut files = rustc_hash::FxHashSet::default();
    let mut planned = 0;
    let mut skipped = 0;
    for fix in fixes {
        if fix.get("skipped").and_then(Value::as_bool) == Some(true) {
            skipped += 1;
            continue;
        }
        planned += 1;
        let kind = fix
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or("other")
            .to_string();
        *by_type.entry(kind).or_insert(0) += 1;
        // Source edits name the file in `path`; manifest, catalog and config
        // edits name it in `file`. A config write can also create files.
        for key in ["path", "file"] {
            if let Some(path) = fix.get(key).and_then(Value::as_str) {
                files.insert(path.to_string());
            }
        }
        if let Some(created) = fix.get("created_files").and_then(Value::as_array) {
            files.extend(created.iter().filter_map(Value::as_str).map(str::to_string));
        }
    }
    let canonical: Vec<String> = fixes.iter().map(Value::to_string).collect();
    FixPlan {
        counts: Some(PlanCounts {
            fixes: planned,
            files: files.len(),
            skipped,
        }),
        by_type,
        digest: digest(&canonical.join(",")),
    }
}

fn digest(text: &str) -> String {
    format!("{:016x}", xxhash_rust::xxh3::xxh3_64(text.as_bytes()))
}

/// The text of the confirmation form.
#[must_use]
pub fn confirmation_message(plan: &FixPlan) -> String {
    let Some(counts) = plan.counts else {
        return "fallow fix_apply wants to change files in this project. The server could not \
                summarize the plan. Run fix_preview to see it. Apply the fixes?"
            .to_string();
    };
    let mut message = format!(
        "fallow fix_apply wants to apply {} {} in {} {}:",
        counts.fixes,
        plural(counts.fixes, "fix", "fixes"),
        counts.files,
        plural(counts.files, "file", "files"),
    );
    let mut by_count: Vec<(&String, &usize)> = plan.by_type.iter().collect();
    by_count.sort_by(|a, b| b.1.cmp(a.1).then_with(|| a.0.cmp(b.0)));
    for (kind, count) in by_count.iter().take(MAX_LISTED_TYPES) {
        let _ = write!(message, "\n- {count} {}", kind.replace('_', " "));
    }
    if by_count.len() > MAX_LISTED_TYPES {
        let rest: usize = by_count
            .iter()
            .skip(MAX_LISTED_TYPES)
            .map(|(_, n)| **n)
            .sum();
        let _ = write!(message, "\n- {rest} other");
    }
    if counts.skipped > 0 {
        let _ = write!(
            message,
            "\nfallow skips {} more {}. fix_preview gives the reasons.",
            counts.skipped,
            plural(counts.skipped, "fix", "fixes"),
        );
    }
    message.push_str("\nApply the fixes?");
    message
}

const fn plural<'a>(count: usize, one: &'a str, many: &'a str) -> &'a str {
    if count == 1 { one } else { many }
}

/// The schema of the confirmation form: one boolean field, `true` by default.
#[must_use]
pub fn confirmation_schema() -> ElicitationSchema {
    let field = BooleanSchema::new()
        .with_default(true)
        .title("Apply the fixes")
        .description("Clear this box, or decline the form, to keep every file as it is.");
    let mut properties = BTreeMap::new();
    properties.insert(
        CONFIRM_FIELD.to_string(),
        PrimitiveSchemaDefinition::Boolean(field),
    );
    ElicitationSchema::new(properties)
}

/// The `elicitation/create` parameters for `plan`.
#[must_use]
pub fn elicit_params(plan: &FixPlan) -> ElicitRequestParams {
    ElicitRequestParams::FormElicitationParams {
        meta: None,
        message: confirmation_message(plan),
        requested_schema: confirmation_schema(),
    }
}

/// The `InputRequiredResult` that asks the client to confirm `plan`.
#[must_use]
pub fn input_required(plan: &FixPlan) -> InputRequiredResult {
    let mut requests = InputRequests::new();
    requests.insert(
        CONFIRM_INPUT_KEY.to_string(),
        InputRequest::Elicitation(ElicitRequest::new(elicit_params(plan))),
    );
    InputRequiredResult::new(
        Some(requests),
        Some(format!("{STATE_PREFIX}{}", plan.digest)),
    )
}

/// The answer of a person to the confirmation form.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Answer {
    /// The person confirmed the fixes.
    Accepted,
    /// The person declined the form, or accepted it with the box cleared.
    Declined,
    /// The person or the client dismissed the form without an answer.
    Cancelled,
}

/// Read the answer from an elicitation result.
///
/// An accept is a yes only when `apply` is absent (the schema default is
/// `true`) or `true`. Any other `apply` value is a decline, so a client that
/// sends a malformed answer writes nothing.
#[must_use]
pub fn answer_from_result(result: &ElicitResult) -> Answer {
    match result.action {
        ElicitationAction::Accept => {
            let apply = result
                .content
                .as_ref()
                .and_then(|content| content.get(CONFIRM_FIELD));
            match apply {
                None | Some(Value::Bool(true)) => Answer::Accepted,
                Some(_) => Answer::Declined,
            }
        }
        ElicitationAction::Decline => Answer::Declined,
        // A cancel, and any action that a later protocol revision adds,
        // writes nothing.
        _ => Answer::Cancelled,
    }
}

/// Read the answer of a retry on the `InputRequired` route. `None` means
/// that the server must send the form again.
///
/// An answer counts only when `requestState` names the digest of the
/// current plan. When the plan changed between the two calls, the person
/// confirmed a different plan. `requestState` is not a credential: the
/// client builds `inputResponses` from the answer of its user, and the
/// digest only detects a stale plan.
#[must_use]
pub fn read_retry(
    responses: Option<&InputResponses>,
    state: Option<&str>,
    plan: &FixPlan,
) -> Option<Answer> {
    let response = responses?.get(CONFIRM_INPUT_KEY)?;
    let digest = state?.strip_prefix(STATE_PREFIX)?;
    if digest != plan.digest {
        return None;
    }
    let result = serde_json::from_value::<ElicitResult>(response.clone()).ok()?;
    Some(answer_from_result(&result))
}

/// Why `fix_apply` wrote no file after a confirmation attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotApplied {
    /// The person declined.
    Declined,
    /// The person or the client cancelled the form.
    Cancelled,
    /// No answer came in time.
    TimedOut,
    /// The elicitation request failed in the client or the transport.
    Failed,
    /// The plan changed while the form was open.
    PlanChanged,
}

impl NotApplied {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Declined => "declined",
            Self::Cancelled => "cancelled",
            Self::TimedOut => "timed_out",
            Self::Failed => "failed",
            Self::PlanChanged => "plan_changed",
        }
    }

    const fn message(self) -> &'static str {
        match self {
            Self::Declined => {
                "The user declined the fixes. fallow changed no file. Do not apply these fixes \
                 in another way. Call fix_apply again only when the user asks for it."
            }
            Self::Cancelled => {
                "The confirmation form closed without an answer. fallow changed no file. A \
                 session with no person to answer cancels the form. Do not apply the fixes in \
                 another way. Tell the user which fixes wait for a confirmation."
            }
            Self::TimedOut => {
                "No answer to the confirmation form came in time. fallow changed no file. Do \
                 not apply the fixes in another way. Ask the user, then call fix_apply again."
            }
            Self::Failed => {
                "The client could not show the confirmation form. fallow changed no file. Do \
                 not apply the fixes in another way. Tell the user which fixes wait for a \
                 confirmation."
            }
            Self::PlanChanged => {
                "The project changed while the confirmation form was open, so the plan is not \
                 the plan that the user confirmed. fallow changed no file. Call fix_apply \
                 again to confirm the new plan."
            }
        }
    }
}

/// The result of a `fix_apply` call that wrote no file.
///
/// It is not an error result: the call did what the person chose. The
/// `applied: false` field and the `confirmation` value tell the agent why.
#[must_use]
pub fn not_applied(reason: NotApplied, plan: &FixPlan) -> CallToolResponse {
    let body = json!({
        "applied": false,
        "confirmation": reason.as_str(),
        "planned_fixes": plan.counts.map(|counts| counts.fixes),
        "planned_files": plan.counts.map(|counts| counts.files),
        "message": reason.message(),
    });
    CallToolResult::success(vec![ContentBlock::text(body.to_string())]).into()
}

#[cfg(test)]
mod tests {
    use rmcp::model::{ElicitationCapability, FormElicitationCapability, UrlElicitationCapability};

    use super::*;

    fn caps(form: bool, url: bool) -> ClientCapabilities {
        let mut elicitation = ElicitationCapability::new();
        if form {
            elicitation.form = Some(FormElicitationCapability::default());
        }
        if url {
            elicitation.url = Some(UrlElicitationCapability::default());
        }
        let mut caps = ClientCapabilities::default();
        caps.elicitation = Some(elicitation);
        caps
    }

    const PREVIEW: &str = r#"{"dry_run":true,"fixes":[
        {"type":"remove_export","path":"src/a.ts","name":"x"},
        {"type":"remove_export","path":"src/a.ts","name":"y"},
        {"type":"remove_export","path":"src/b.ts","name":"z"},
        {"type":"remove_dependency","file":"package.json","package":"left-pad"},
        {"type":"remove_export","path":"src/c.ts","skipped":true,"skip_reason":"content_changed"}
    ],"total_fixed":0}"#;

    #[test]
    fn route_without_elicitation_is_unconfirmed() {
        let old = ProtocolVersion::V_2025_11_25;
        assert_eq!(confirm_route(Some(&old), None), ConfirmRoute::Unconfirmed);
        let none = ClientCapabilities::default();
        assert_eq!(
            confirm_route(Some(&old), Some(&none)),
            ConfirmRoute::Unconfirmed
        );
    }

    #[test]
    fn route_with_only_url_elicitation_is_unconfirmed() {
        let old = ProtocolVersion::V_2025_11_25;
        assert_eq!(
            confirm_route(Some(&old), Some(&caps(false, true))),
            ConfirmRoute::Unconfirmed
        );
    }

    #[test]
    fn route_with_bare_elicitation_counts_as_form() {
        let old = ProtocolVersion::V_2025_06_18;
        assert_eq!(
            confirm_route(Some(&old), Some(&caps(false, false))),
            ConfirmRoute::Request
        );
    }

    #[test]
    fn route_follows_the_protocol_revision() {
        let old = ProtocolVersion::V_2025_11_25;
        let new = ProtocolVersion::V_2026_07_28;
        let both = caps(true, true);
        assert_eq!(
            confirm_route(Some(&old), Some(&both)),
            ConfirmRoute::Request
        );
        assert_eq!(
            confirm_route(Some(&new), Some(&both)),
            ConfirmRoute::InputRequired
        );
    }

    #[test]
    fn summary_counts_planned_fixes_and_files() {
        let plan = summarize_preview(PREVIEW);
        assert_eq!(
            plan.counts,
            Some(PlanCounts {
                fixes: 4,
                files: 3,
                skipped: 1
            })
        );
        assert_eq!(plan.by_type.get("remove_export"), Some(&3));
        assert_eq!(plan.by_type.get("remove_dependency"), Some(&1));
        assert!(!plan.is_empty());
    }

    #[test]
    fn message_names_counts_and_types() {
        let message = confirmation_message(&summarize_preview(PREVIEW));
        assert!(message.contains("4 fixes in 3 files"), "{message}");
        assert!(message.contains("- 3 remove export"), "{message}");
        assert!(message.contains("- 1 remove dependency"), "{message}");
    }

    #[test]
    fn unreadable_preview_still_asks() {
        let plan = summarize_preview("not json");
        assert_eq!(plan.counts, None);
        assert!(!plan.is_empty());
        assert!(confirmation_message(&plan).contains("fix_preview"));
    }

    #[test]
    fn empty_plan_is_empty() {
        let plan = summarize_preview(r#"{"fixes":[]}"#);
        assert!(plan.is_empty());
    }

    #[test]
    fn accept_with_cleared_box_is_a_decline() {
        let result =
            ElicitResult::new(ElicitationAction::Accept).with_content(json!({"apply": false}));
        assert_eq!(answer_from_result(&result), Answer::Declined);
        let result =
            ElicitResult::new(ElicitationAction::Accept).with_content(json!({"apply": true}));
        assert_eq!(answer_from_result(&result), Answer::Accepted);
        let result = ElicitResult::new(ElicitationAction::Accept);
        assert_eq!(answer_from_result(&result), Answer::Accepted);
        let result = ElicitResult::new(ElicitationAction::Cancel);
        assert_eq!(answer_from_result(&result), Answer::Cancelled);
    }

    fn responses(action: &str) -> InputResponses {
        let mut map = InputResponses::new();
        map.insert(
            CONFIRM_INPUT_KEY.to_string(),
            json!({"action": action, "content": {"apply": true}}),
        );
        map
    }

    #[test]
    fn retry_reads_the_answer_for_the_current_plan() {
        let plan = summarize_preview(PREVIEW);
        let state = format!("{STATE_PREFIX}{}", plan.digest);
        assert_eq!(
            read_retry(Some(&responses("accept")), Some(&state), &plan),
            Some(Answer::Accepted)
        );
        assert_eq!(
            read_retry(Some(&responses("decline")), Some(&state), &plan),
            Some(Answer::Declined)
        );
    }

    #[test]
    fn retry_for_a_changed_plan_asks_again() {
        let plan = summarize_preview(PREVIEW);
        let stale = format!("{STATE_PREFIX}0000000000000000");
        assert_eq!(
            read_retry(Some(&responses("accept")), Some(&stale), &plan),
            None
        );
        assert_eq!(read_retry(Some(&responses("accept")), None, &plan), None);
        assert_eq!(read_retry(None, None, &plan), None);
    }

    #[test]
    fn input_required_carries_the_form_and_the_digest() {
        let plan = summarize_preview(PREVIEW);
        let value = serde_json::to_value(input_required(&plan)).unwrap();
        assert_eq!(value["resultType"], "input_required");
        assert_eq!(
            value["requestState"],
            format!("{STATE_PREFIX}{}", plan.digest)
        );
        let form = &value["inputRequests"][CONFIRM_INPUT_KEY];
        assert_eq!(form["method"], "elicitation/create");
        assert_eq!(form["params"]["mode"], "form");
        assert_eq!(
            form["params"]["requestedSchema"]["properties"]["apply"]["type"],
            "boolean"
        );
    }

    #[test]
    fn not_applied_result_is_no_error() {
        let plan = summarize_preview(PREVIEW);
        let CallToolResponse::Complete(result) = not_applied(NotApplied::Declined, &plan) else {
            panic!("a complete result");
        };
        assert_ne!(result.is_error, Some(true));
        let text = result.content[0].as_text().unwrap().text.clone();
        let body: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(body["applied"], false);
        assert_eq!(body["confirmation"], "declined");
        assert_eq!(body["planned_fixes"], 4);
        assert_eq!(body["planned_files"], 3);
    }

    /// No message sends the agent to a route that writes without the form.
    #[test]
    fn no_message_offers_a_way_around_the_form() {
        for reason in [
            NotApplied::Declined,
            NotApplied::Cancelled,
            NotApplied::TimedOut,
            NotApplied::Failed,
            NotApplied::PlanChanged,
        ] {
            let message = reason.message();
            assert!(!message.contains("--yes"), "{message}");
            assert!(message.contains("changed no file"), "{message}");
        }
    }

    #[test]
    fn a_malformed_apply_value_is_a_decline() {
        for content in [
            json!({"apply": "true"}),
            json!({"apply": "false"}),
            json!({"apply": 1}),
            json!({"apply": null}),
        ] {
            let result = ElicitResult::new(ElicitationAction::Accept).with_content(content.clone());
            assert_eq!(answer_from_result(&result), Answer::Declined, "{content}");
        }
    }

    #[test]
    fn message_names_the_skipped_fixes() {
        let message = confirmation_message(&summarize_preview(PREVIEW));
        assert!(message.contains("skips 1 more fix"), "{message}");
    }
}
