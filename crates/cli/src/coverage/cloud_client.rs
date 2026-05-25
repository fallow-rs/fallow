//! HTTP client for explicit cloud runtime coverage pulls.
//!
//! This is intentionally the only runtime-coverage module that talks to the
//! network. Local `health --runtime-coverage` analysis stays disk-only.

use std::fmt::{self, Write as _};

use serde::Deserialize;

use crate::api::{
    NETWORK_EXIT_CODE, api_url, parse_error_envelope, sanitize_network_error,
    try_api_agent_with_timeout,
};

const CLOUD_CONNECT_TIMEOUT_SECS: u64 = 5;
const CLOUD_TOTAL_TIMEOUT_SECS: u64 = 30;
const RUNTIME_CONTEXT_FORMAT: &str = "fallow-cloud-runtime-v1";

#[derive(Clone)]
pub struct CloudRequest {
    pub api_key: String,
    pub api_endpoint: Option<String>,
    pub repo: String,
    pub project_id: Option<String>,
    pub period_days: u16,
    pub environment: Option<String>,
    pub commit_sha: Option<String>,
}

// Manual `Debug` so any future `tracing::debug!`, `dbg!`, or `unwrap`-on-Err
// with the `Debug` formatter does not leak the bearer token through
// stderr. The derive would do; explicit redaction is unmissable.
impl fmt::Debug for CloudRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CloudRequest")
            .field("api_key", &"***")
            .field("api_endpoint", &self.api_endpoint)
            .field("repo", &self.repo)
            .field("project_id", &self.project_id)
            .field("period_days", &self.period_days)
            .field("environment", &self.environment)
            .field("commit_sha", &self.commit_sha)
            .finish()
    }
}

#[derive(Debug)]
pub enum CloudError {
    Validation(String),
    Auth(String),
    TierRequired(String),
    NotFound(String),
    Network(String),
    Server(String),
}

impl CloudError {
    pub const fn exit_code(&self) -> u8 {
        match self {
            Self::Validation(_) => 2,
            Self::Auth(_) | Self::TierRequired(_) | Self::NotFound(_) => 3,
            Self::Network(_) | Self::Server(_) => NETWORK_EXIT_CODE,
        }
    }

    pub fn message(&self) -> &str {
        match self {
            Self::Validation(message)
            | Self::Auth(message)
            | Self::TierRequired(message)
            | Self::NotFound(message)
            | Self::Network(message)
            | Self::Server(message) => message,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum CloudRuntimeContextResponse {
    Envelope { data: CloudRuntimeContext },
    Direct(CloudRuntimeContext),
}

impl CloudRuntimeContextResponse {
    fn into_context(self) -> CloudRuntimeContext {
        match self {
            Self::Envelope { data } => data,
            Self::Direct(context) => context,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct CloudRuntimeContext {
    #[serde(default)]
    pub repo: String,
    #[serde(default)]
    pub window: CloudRuntimeWindow,
    pub summary: CloudRuntimeSummary,
    #[serde(default)]
    pub functions: Vec<CloudRuntimeFunction>,
    #[serde(default)]
    pub blast_radius: Vec<CloudRuntimeBlastRadiusEntry>,
    #[serde(default)]
    pub importance: Vec<CloudRuntimeImportanceEntry>,
    #[serde(default)]
    pub warnings: Vec<CloudRuntimeWarning>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct CloudRuntimeWindow {
    #[serde(default)]
    pub period_days: u32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CloudRuntimeSummary {
    #[serde(default)]
    pub trace_count: u64,
    #[serde(default)]
    pub deployments_seen: u32,
    #[serde(default)]
    pub functions_tracked: usize,
    #[serde(default)]
    pub functions_hit: usize,
    #[serde(default)]
    pub functions_unhit: usize,
    #[serde(default)]
    pub functions_untracked: usize,
    #[serde(default)]
    pub coverage_percent: f64,
    #[serde(default)]
    pub last_received_at: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CloudRuntimeFunction {
    pub file_path: String,
    pub function_name: String,
    #[serde(default)]
    pub line_number: Option<u32>,
    #[serde(default)]
    pub start_line: Option<u32>,
    #[serde(default)]
    pub end_line: Option<u32>,
    #[serde(default)]
    pub hit_count: Option<u64>,
    #[serde(default)]
    pub tracking_state: CloudTrackingState,
    #[serde(default)]
    pub deployments_observed: u32,
    #[serde(default)]
    pub untracked_reason: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CloudRuntimeBlastRadiusEntry {
    pub id: String,
    pub file: String,
    pub function: String,
    pub line: u32,
    pub caller_count: u32,
    pub caller_count_weighted_by_traffic: u64,
    #[serde(default)]
    pub deploys_touched: Option<u32>,
    pub risk_band: CloudRuntimeRiskBand,
}

#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CloudRuntimeRiskBand {
    Low,
    Medium,
    High,
    #[default]
    Unknown,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CloudRuntimeImportanceEntry {
    pub id: String,
    pub file: String,
    pub function: String,
    pub line: u32,
    pub invocations: u64,
    pub cyclomatic: u32,
    pub owner_count: u32,
    pub importance_score: f64,
    pub reason: String,
}

#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CloudTrackingState {
    Called,
    NeverCalled,
    Untracked,
    #[default]
    Unknown,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum CloudRuntimeWarning {
    Message(String),
    Object {
        #[serde(default)]
        code: Option<String>,
        #[serde(default)]
        message: Option<String>,
    },
}

pub fn fetch_runtime_context(request: &CloudRequest) -> Result<CloudRuntimeContext, CloudError> {
    validate_request(request)?;
    let url = runtime_context_url(request);
    let agent = try_api_agent_with_timeout(CLOUD_CONNECT_TIMEOUT_SECS, CLOUD_TOTAL_TIMEOUT_SECS)
        .map_err(|err| CloudError::Network(network_message(&err.to_string())))?;
    let mut response = agent
        .get(&url)
        .header("Authorization", &format!("Bearer {}", request.api_key))
        .header("Accept", "application/json")
        .header("Accept-Encoding", "identity")
        .call()
        .map_err(|err| {
            CloudError::Network(network_message(&sanitize_network_error(&format!("{err}"))))
        })?;

    let status = response.status().as_u16();
    if response.status().is_success() {
        let envelope: CloudRuntimeContextResponse =
            response.body_mut().read_json().map_err(|err| {
                CloudError::Server(format!("malformed runtime-context response: {err}"))
            })?;
        return Ok(envelope.into_context());
    }

    let body = response.body_mut().read_to_string().unwrap_or_default();
    let envelope = parse_error_envelope(&body);
    let code = envelope.code();
    let message = envelope
        .message()
        .filter(|message| !message.trim().is_empty())
        .unwrap_or_else(|| body.trim());

    match (status, code) {
        (401, _) => Err(CloudError::Auth(
            "Fallow API key is invalid or revoked.".to_owned(),
        )),
        (403, Some("tier_required")) => Err(CloudError::TierRequired(
            "cloud-pull is a Team-tier feature. Start a free trial:\n\n  fallow license activate --trial --email <addr>".to_owned(),
        )),
        (404, Some("repo_not_found")) => Err(CloudError::NotFound(format!(
            "Repo not accessible to your org: {}",
            request.repo
        ))),
        (400, Some("validation_error")) => Err(CloudError::Validation(format!(
            "Cloud rejected the request: {message}"
        ))),
        (500..=599, _) => Err(CloudError::Network(network_message(message))),
        _ => Err(CloudError::Server(format!(
            "runtime-context request failed with HTTP {status}: {message}"
        ))),
    }
}

fn validate_request(request: &CloudRequest) -> Result<(), CloudError> {
    if request.api_key.trim().is_empty() {
        return Err(CloudError::Auth(
            "Cloud runtime coverage requires an API key.\n\nSet FALLOW_API_KEY or pass --api-key:\n\n  FALLOW_API_KEY=fallow_live_... fallow coverage analyze --cloud --repo owner/repo".to_owned(),
        ));
    }
    if request.repo.trim().is_empty() {
        return Err(CloudError::Validation(
            "repository is empty; pass --repo owner/repo".to_owned(),
        ));
    }
    if request.period_days == 0 || request.period_days > 90 {
        return Err(CloudError::Validation(
            "--coverage-period must be between 1 and 90 days".to_owned(),
        ));
    }
    Ok(())
}

pub fn runtime_context_url(request: &CloudRequest) -> String {
    let path = format!(
        "/v1/coverage/{}/runtime-context",
        url_encode_path_segment(request.repo.trim())
    );
    let base = match request.api_endpoint.as_deref() {
        Some(base) => format!("{}{}", base.trim().trim_end_matches('/'), path),
        None => api_url(&path),
    };
    let mut query = vec![
        ("periodDays", request.period_days.to_string()),
        ("format", RUNTIME_CONTEXT_FORMAT.to_owned()),
    ];
    if let Some(project_id) = request
        .project_id
        .as_deref()
        .filter(|v| !v.trim().is_empty())
    {
        query.push(("projectId", url_encode_query_value(project_id.trim())));
    }
    if let Some(environment) = request
        .environment
        .as_deref()
        .filter(|v| !v.trim().is_empty())
    {
        query.push(("environment", url_encode_query_value(environment.trim())));
    }
    if let Some(commit_sha) = request
        .commit_sha
        .as_deref()
        .filter(|v| !v.trim().is_empty())
    {
        query.push(("commitSha", url_encode_query_value(commit_sha.trim())));
    }
    format!(
        "{base}?{}",
        query
            .into_iter()
            .map(|(key, value)| format!("{key}={value}"))
            .collect::<Vec<_>>()
            .join("&")
    )
}

fn network_message(detail: &str) -> String {
    let suffix = if detail.trim().is_empty() {
        String::new()
    } else {
        format!(" ({})", detail.trim())
    };
    format!(
        "Could not reach fallow.cloud for cloud runtime coverage{suffix}.\n\nCloud mode is explicitly network-backed. Local runtime coverage still works:\n\n  fallow coverage analyze --runtime-coverage ./coverage"
    )
}

pub fn url_encode_path_segment(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(byte as char);
            }
            _ => {
                write!(out, "%{byte:02X}").expect("writing to String never fails");
            }
        }
    }
    out
}

fn url_encode_query_value(value: &str) -> String {
    url_encode_path_segment(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(repo: &str) -> CloudRequest {
        CloudRequest {
            api_key: "fallow_live_test".to_owned(),
            api_endpoint: Some("http://127.0.0.1:3000/".to_owned()),
            repo: repo.to_owned(),
            project_id: None,
            period_days: 30,
            environment: None,
            commit_sha: None,
        }
    }

    #[test]
    fn runtime_context_url_percent_encodes_repo_as_single_segment() {
        let url = runtime_context_url(&request("acme/web"));
        assert!(url.starts_with("http://127.0.0.1:3000/v1/coverage/acme%2Fweb/runtime-context?"));
        assert!(url.contains("periodDays=30"));
        assert!(url.contains("format=fallow-cloud-runtime-v1"));
    }

    #[test]
    fn runtime_context_url_encodes_optional_query_values() {
        let mut req = request("acme/web");
        req.project_id = Some("app one".to_owned());
        req.environment = Some("prod/eu".to_owned());
        req.commit_sha = Some("abc123".to_owned());
        let url = runtime_context_url(&req);
        assert!(url.contains("projectId=app%20one"));
        assert!(url.contains("environment=prod%2Feu"));
        assert!(url.contains("commitSha=abc123"));
    }

    #[test]
    fn validate_request_rejects_invalid_period() {
        let mut req = request("acme/web");
        req.period_days = 91;
        assert!(matches!(
            validate_request(&req),
            Err(CloudError::Validation(_))
        ));
    }

    #[test]
    fn cloud_request_debug_masks_api_key() {
        // Future `tracing::debug!(?req)`, `dbg!(req)`, or unwrap-on-Err with
        // the Debug formatter would surface the api_key in stderr. Mask at
        // the type level so the next contributor cannot reintroduce it by
        // accident.
        let req = CloudRequest {
            api_key: "fallow_live_secret_token_value".to_owned(),
            api_endpoint: Some("https://api.fallow.cloud".to_owned()),
            repo: "acme/web".to_owned(),
            project_id: None,
            period_days: 30,
            environment: None,
            commit_sha: None,
        };
        let formatted = format!("{req:?}");
        assert!(
            !formatted.contains("fallow_live_secret_token_value"),
            "api_key leaked through Debug: {formatted}"
        );
        assert!(
            formatted.contains("api_key: \"***\""),
            "expected explicit redaction marker, got: {formatted}"
        );
        // Non-secret fields must still be inspectable so Debug stays useful.
        assert!(formatted.contains("repo: \"acme/web\""));
        assert!(formatted.contains("period_days: 30"));
    }

    #[test]
    fn cloud_error_exit_code_for_validation_is_two() {
        // Regression: HTTP 400 with code=validation_error must surface as
        // CloudError::Validation (exit 2), not CloudError::Server (exit 7).
        // Caught live against api.fallow.cloud during the v2.57.0 smoke when
        // --environment was rejected with HTTP 400.
        assert_eq!(CloudError::Validation("any".to_owned()).exit_code(), 2);
    }
}
