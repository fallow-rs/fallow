//! Fallow Cloud file-context read for `fallow inspect --file <path> --cloud`.
//!
//! One `GET /v1/coverage/:repo/file-context?path=<path>` request gives the
//! production data of one file: per function the tracking state, calls,
//! errors, CPU and never-run branch ranges, plus a file summary. Every
//! failure becomes a [`FileContextFailure`] that the caller shows as a section
//! status, so a cloud problem never fails the inspect run.

use std::path::Path;

use serde::Deserialize;

use super::cloud_client::url_encode_query_value;
use super::cloud_transport::{self, CloudAuth, CloudBody, CloudOutcome, detected_agent_source};
use super::upload_common::url_encode_path_segment;

/// Total time limit of the read, one attempt only. Inspect runs inside
/// editor hooks with a budget of about 2 s, and the static analysis runs in
/// parallel with the read. A slower read gives the `unavailable` status.
const FILE_CONTEXT_TIMEOUT_SECS: u64 = 2;
const OPERATION: &str = "file-context";
/// Byte cap that the CLI asks for: the cloud upper limit. The cloud drops
/// functions to fit its default cap of 12 000 bytes, and a dropped function
/// cannot pair with its local function.
const FILE_CONTEXT_MAX_BYTES: u32 = 200_000;

/// File-context answer, `schema_version` `fallow-cloud-file-context-v1`.
/// Unknown fields are ignored, so a newer cloud stays readable.
#[derive(Debug, Clone, Deserialize)]
pub struct FileContext {
    #[serde(default)]
    pub repo: String,
    /// `observed` or `untracked`.
    #[serde(default)]
    pub state: String,
    #[serde(default)]
    pub latest_sha: Option<String>,
    #[serde(default)]
    pub period_days: Option<u16>,
    pub summary: FileContextSummary,
    #[serde(default)]
    pub functions: Vec<FileContextFunction>,
    /// Present when the cloud left out functions to fit its byte cap.
    #[serde(default)]
    pub truncation: Option<FileContextTruncation>,
    /// True when the cloud left out the never_called rows, because the read
    /// is not actionable (for example during the warm-up of a deployment).
    #[serde(default)]
    pub never_called_withheld: bool,
    /// Why the read is not actionable, when the cloud says so.
    #[serde(default)]
    pub actionability_reason: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FileContextTruncation {
    #[serde(default)]
    pub omitted_functions: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FileContextSummary {
    #[serde(default)]
    pub functions: u64,
    #[serde(default)]
    pub called: u64,
    #[serde(default)]
    pub never_called: u64,
    #[serde(default)]
    pub untracked: u64,
    #[serde(default)]
    pub calls: Option<u64>,
    #[serde(default)]
    pub error_groups: Option<u64>,
    #[serde(default)]
    pub error_events: Option<u64>,
    #[serde(default)]
    pub text: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FileContextFunction {
    pub name: String,
    #[serde(default)]
    pub line: Option<u32>,
    #[serde(default)]
    pub stable_id: Option<String>,
    #[serde(default)]
    pub state: String,
    /// `called` when the function ran in this deployment or in an earlier
    /// deployment inside the period. A `never_called` function with
    /// `period_state` `called` did not run in this deployment only.
    #[serde(default)]
    pub period_state: Option<String>,
    #[serde(default)]
    pub calls: Option<u64>,
    #[serde(default)]
    pub errors: Option<FileContextErrors>,
    #[serde(default)]
    pub cpu: Option<FileContextCpu>,
    #[serde(default)]
    pub branches: Option<FileContextBranches>,
    #[serde(default)]
    pub summary: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FileContextErrors {
    #[serde(default)]
    pub state: String,
    #[serde(default)]
    pub groups: Option<u64>,
    #[serde(default)]
    pub events: Option<u64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FileContextCpu {
    #[serde(default)]
    pub state: String,
    #[serde(default)]
    pub self_ms_per_hour: Option<f64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FileContextBranches {
    #[serde(default)]
    pub state: String,
    #[serde(default)]
    pub ranges_never_run: Option<u64>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum FileContextResponse {
    Envelope { data: FileContext },
    Direct(FileContext),
}

/// Why the file-context read gave no data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileContextFailure {
    /// The cloud refused the API key (HTTP 401).
    KeyRefused(String),
    /// The organization plan does not include production data.
    PlanRequired(String),
    /// Network, server, repository or answer problem.
    Unavailable(String),
}

/// Resolve the Fallow Cloud repository: `--repo`, `$FALLOW_REPO`, then the
/// `origin` git remote. The same rule as `fallow coverage analyze --cloud`.
///
/// # Errors
///
/// Returns the message of the repository rule when no source names a repo.
pub fn resolve_cloud_repo(explicit: Option<&str>, root: &Path) -> Result<String, String> {
    super::analyze::resolve_repo(explicit, root).map_err(|err| {
        let message = err.message();
        if message.starts_with("Could not infer repository") {
            "could not infer the Fallow Cloud repository; pass --repo owner/repo or set FALLOW_REPO"
                .to_owned()
        } else {
            message.to_owned()
        }
    })
}

/// Read the file context of `path` (relative to the project root).
///
/// # Errors
///
/// Returns a [`FileContextFailure`] for each answer without data.
pub fn fetch_file_context(
    api_key: &str,
    repo: &str,
    path: &str,
) -> Result<FileContext, FileContextFailure> {
    let auth = CloudAuth {
        api_key: api_key.to_owned(),
        agent_source: detected_agent_source(),
    };
    let url = crate::api::api_url(&format!(
        "/v1/coverage/{}/file-context?path={}&maxBytes={FILE_CONTEXT_MAX_BYTES}",
        url_encode_path_segment(repo),
        url_encode_query_value(path)
    ));
    let outcome = cloud_transport::send_once_with_total_timeout(
        &auth,
        &url,
        &CloudBody::None,
        OPERATION,
        FILE_CONTEXT_TIMEOUT_SECS,
    )
    .map_err(|err| FileContextFailure::Unavailable(err.message().to_owned()))?;
    match outcome {
        CloudOutcome::Success(response) => parse_file_context(&response.body),
        CloudOutcome::Http(failure) => Err(map_failure(
            failure.status,
            failure.code.as_deref(),
            &failure.message,
            repo,
        )),
    }
}

fn parse_file_context(body: &str) -> Result<FileContext, FileContextFailure> {
    serde_json::from_str::<FileContextResponse>(body)
        .map(|response| match response {
            FileContextResponse::Envelope { data } | FileContextResponse::Direct(data) => data,
        })
        .map_err(|err| {
            FileContextFailure::Unavailable(format!("malformed {OPERATION} answer: {err}"))
        })
}

fn map_failure(status: u16, code: Option<&str>, message: &str, repo: &str) -> FileContextFailure {
    match (status, code) {
        (401, _) => FileContextFailure::KeyRefused(
            "Fallow Cloud refused the API key (invalid or revoked). Run `fallow cloud login` again or set FALLOW_API_KEY.".to_owned(),
        ),
        (402, _) | (403, Some("tier_required")) => FileContextFailure::PlanRequired(
            "production data needs a Fallow Cloud plan".to_owned(),
        ),
        (404, Some("repo_not_found")) => FileContextFailure::Unavailable(format!(
            "repository {repo} is not in your Fallow Cloud organization; pass --repo or set FALLOW_REPO"
        )),
        _ => FileContextFailure::Unavailable(format!(
            "{OPERATION} failed with HTTP {status}: {message}"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tier_gate_maps_to_plan_required() {
        assert!(matches!(
            map_failure(403, Some("tier_required"), "x", "a/b"),
            FileContextFailure::PlanRequired(_)
        ));
        assert!(matches!(
            map_failure(402, None, "x", "a/b"),
            FileContextFailure::PlanRequired(_)
        ));
        assert!(matches!(
            map_failure(403, Some("forbidden"), "x", "a/b"),
            FileContextFailure::Unavailable(_)
        ));
        assert!(matches!(
            map_failure(401, None, "x", "a/b"),
            FileContextFailure::KeyRefused(_)
        ));
    }

    #[test]
    fn parses_envelope_and_ignores_unknown_fields() {
        let body = r#"{"data":{"repo":"a/b","state":"observed","latest_sha":"abc","period_days":30,"new_field":1,"summary":{"functions":1,"called":1,"never_called":0,"untracked":0,"calls":3,"error_groups":null,"error_events":null,"text":"t"},"functions":[{"name":"f","line":2,"stable_id":"fallow:fn:x","state":"called","calls":3,"errors":null,"cpu":null,"branches":null,"summary":"s"}]}}"#;
        let context = parse_file_context(body).unwrap();
        assert_eq!(context.functions[0].calls, Some(3));
        assert_eq!(context.summary.error_groups, None);
        assert!(matches!(
            parse_file_context("{}"),
            Err(FileContextFailure::Unavailable(_))
        ));
    }
}
