//! Scoped Fallow Cloud reads: `fallow coverage review-packet` and
//! `fallow coverage deployment-changes`.
//!
//! `coverage analyze --cloud` pulls the runtime context of the whole
//! repository and joins it with a full local static analysis. These two
//! commands answer a narrower question with one small request each, and print
//! the cloud answer as JSON without a local analysis:
//!
//! - `review-packet` sends a set of changed files or functions to
//!   `POST /v1/coverage/:repo/review-packet` and prints the production facts of
//!   those functions.
//! - `deployment-changes` reads `GET /v1/coverage/:repo/deployments/:sha/changes`
//!   and prints how production behavior changed between two deployments.

use std::fmt;
use std::path::Path;
use std::process::ExitCode;

use fallow_config::OutputFormat;
use fallow_types::cloud::CloudCommand;
use serde_json::{Map, Value, json};

use super::RunContext;
use super::analyze::{emit_cloud_error, resolve_api_key, resolve_repo};
use super::cloud_client::{CloudError, map_http_failure, url_encode_query_value};
use super::cloud_transport::{self, CloudAuth, CloudBody, CloudOutcome, detected_agent_source};
use super::upload_common::url_encode_path_segment;
use crate::api::api_url;

/// Per-list limit of the review-packet scope on the cloud.
const REVIEW_PACKET_MAX_SCOPE: usize = 1000;
/// Widest window, in days, that the cloud serves.
const MAX_PERIOD_DAYS: u16 = 90;
/// Longest commit SHA the cloud accepts on these routes.
const MAX_SHA_LEN: usize = 64;
/// Page size limit of the deployment change report.
const MAX_CHANGE_LIMIT: u16 = 200;
/// Change kinds the deployment change report accepts as a filter.
const DEPLOYMENT_CHANGE_KINDS: &[&str] = &[
    "stopped",
    "new_not_called",
    "heated_up",
    "cooled_down",
    "new_called",
    "unchanged",
];
/// File extensions that the runtime inventory can hold.
const SOURCE_EXTENSIONS: &[&str] = &[
    "js", "jsx", "mjs", "cjs", "ts", "tsx", "mts", "cts", "vue", "svelte", "astro",
];

/// Arguments for `fallow coverage review-packet`.
#[derive(Clone, Default)]
pub struct ReviewPacketArgs {
    pub api_key: Option<String>,
    pub api_endpoint: Option<String>,
    pub repo: Option<String>,
    pub project_id: Option<String>,
    pub coverage_period: Option<u16>,
    pub commit_sha: Option<String>,
    /// Repo-relative files. Empty means: the files changed against the base.
    pub files: Vec<String>,
    /// `FILE:NAME` or `FILE:NAME:LINE` targets.
    pub functions: Vec<String>,
    /// Base ref for the changed-file default.
    pub base: Option<String>,
}

impl fmt::Debug for ReviewPacketArgs {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ReviewPacketArgs")
            .field("api_key", &self.api_key.as_ref().map(|_| "***"))
            .field("api_endpoint", &self.api_endpoint)
            .field("repo", &self.repo)
            .field("project_id", &self.project_id)
            .field("coverage_period", &self.coverage_period)
            .field("commit_sha", &self.commit_sha)
            .field("files", &self.files)
            .field("functions", &self.functions)
            .field("base", &self.base)
            .finish()
    }
}

/// Arguments for `fallow coverage deployment-changes`.
#[derive(Clone, Default)]
pub struct DeploymentChangesArgs {
    pub api_key: Option<String>,
    pub api_endpoint: Option<String>,
    pub repo: Option<String>,
    /// Deployment commit. `None` means `git rev-parse HEAD`.
    pub sha: Option<String>,
    /// Base deployment commit. `None` lets the cloud pick the previous one.
    pub base: Option<String>,
    pub change: Option<String>,
    pub limit: Option<u16>,
    pub cursor: Option<String>,
}

impl fmt::Debug for DeploymentChangesArgs {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DeploymentChangesArgs")
            .field("api_key", &self.api_key.as_ref().map(|_| "***"))
            .field("api_endpoint", &self.api_endpoint)
            .field("repo", &self.repo)
            .field("sha", &self.sha)
            .field("base", &self.base)
            .field("change", &self.change)
            .field("limit", &self.limit)
            .field("cursor", &self.cursor)
            .finish()
    }
}

/// One function target of a review packet.
#[derive(Debug, Clone, PartialEq, Eq)]
struct FunctionTarget {
    file: String,
    name: String,
    line: Option<u32>,
}

/// Run `fallow coverage review-packet`.
pub fn run_review_packet(args: &ReviewPacketArgs, ctx: &RunContext<'_>) -> ExitCode {
    match review_packet(args, ctx.root) {
        Ok(value) => crate::report::emit_report_json(&value, "review packet JSON", ctx.json_style),
        Err(err) => emit_cloud_error(&err, OutputFormat::Json),
    }
}

/// Run `fallow coverage deployment-changes`.
pub fn run_deployment_changes(args: &DeploymentChangesArgs, ctx: &RunContext<'_>) -> ExitCode {
    match deployment_changes(args, ctx.root) {
        Ok(value) => {
            crate::report::emit_report_json(&value, "deployment changes JSON", ctx.json_style)
        }
        Err(err) => emit_cloud_error(&err, OutputFormat::Json),
    }
}

fn review_packet(args: &ReviewPacketArgs, root: &Path) -> Result<Value, CloudError> {
    let period_days = validate_period(args.coverage_period)?;
    let functions = args
        .functions
        .iter()
        .map(|raw| parse_function_target(raw))
        .collect::<Result<Vec<_>, _>>()?;
    let mut files = normalize_files(&args.files);
    if files.is_empty() && functions.is_empty() {
        files = changed_source_files(root, args.base.as_deref())?;
        if files.is_empty() {
            return Err(CloudError::Validation(
                "no changed source files against the base. Pass --file or --function, or --base <ref>.".to_owned(),
            ));
        }
    }
    if files.len() > REVIEW_PACKET_MAX_SCOPE || functions.len() > REVIEW_PACKET_MAX_SCOPE {
        return Err(CloudError::Validation(format!(
            "the review packet takes at most {REVIEW_PACKET_MAX_SCOPE} files and {REVIEW_PACKET_MAX_SCOPE} functions. Pass --file to narrow the scope."
        )));
    }
    let commit_sha = args
        .commit_sha
        .as_deref()
        .map(|sha| validate_sha(sha, "--commit-sha"))
        .transpose()?;
    let auth = resolve_auth(args.api_key.as_deref(), CloudCommand::ReviewPacket)?;
    let repo = resolve_repo(args.repo.as_deref(), root)?;
    let body = review_packet_body(
        &files,
        &functions,
        period_days,
        args.project_id.as_deref(),
        commit_sha.as_deref(),
    );
    let url = endpoint_url(
        args.api_endpoint.as_deref(),
        &format!(
            "/v1/coverage/{}/review-packet",
            url_encode_path_segment(&repo)
        ),
    );
    send_and_parse(&auth, &url, &CloudBody::Json(&body), "review-packet", &repo)
}

fn deployment_changes(args: &DeploymentChangesArgs, root: &Path) -> Result<Value, CloudError> {
    let sha = match args.sha.as_deref() {
        Some(sha) => validate_sha(sha, "--sha")?,
        None => head_sha(root)?,
    };
    let base = args
        .base
        .as_deref()
        .map(|sha| validate_sha(sha, "--base"))
        .transpose()?;
    if let Some(change) = args.change.as_deref()
        && !DEPLOYMENT_CHANGE_KINDS.contains(&change)
    {
        return Err(CloudError::Validation(format!(
            "--change must be one of {}, got {change}",
            DEPLOYMENT_CHANGE_KINDS.join(", ")
        )));
    }
    if let Some(limit) = args.limit
        && (limit == 0 || limit > MAX_CHANGE_LIMIT)
    {
        return Err(CloudError::Validation(format!(
            "--limit must be between 1 and {MAX_CHANGE_LIMIT}, got {limit}"
        )));
    }
    let auth = resolve_auth(args.api_key.as_deref(), CloudCommand::DeploymentChanges)?;
    let repo = resolve_repo(args.repo.as_deref(), root)?;
    let mut query = Vec::new();
    if let Some(base) = base {
        query.push(format!("base={}", url_encode_query_value(&base)));
    }
    if let Some(change) = args.change.as_deref() {
        query.push(format!("change={change}"));
    }
    if let Some(limit) = args.limit {
        query.push(format!("limit={limit}"));
    }
    if let Some(cursor) = args
        .cursor
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
    {
        query.push(format!("cursor={}", url_encode_query_value(cursor)));
    }
    let path = format!(
        "/v1/coverage/{}/deployments/{}/changes",
        url_encode_path_segment(&repo),
        url_encode_path_segment(&sha)
    );
    let mut url = endpoint_url(args.api_endpoint.as_deref(), &path);
    if !query.is_empty() {
        url.push('?');
        url.push_str(&query.join("&"));
    }
    send_and_parse(&auth, &url, &CloudBody::None, "deployment-changes", &repo)
}

fn resolve_auth(explicit: Option<&str>, command: CloudCommand) -> Result<CloudAuth, CloudError> {
    Ok(CloudAuth {
        api_key: resolve_api_key(explicit, command)?,
        agent_source: detected_agent_source(),
    })
}

fn send_and_parse(
    auth: &CloudAuth,
    url: &str,
    body: &CloudBody<'_>,
    operation: &str,
    repo: &str,
) -> Result<Value, CloudError> {
    match cloud_transport::send(auth, url, body, operation)? {
        CloudOutcome::Success(response) => serde_json::from_str(&response.body)
            .map_err(|err| CloudError::Server(format!("malformed {operation} response: {err}"))),
        CloudOutcome::Http(failure) => Err(map_http_failure(&failure, operation, repo)),
    }
}

fn endpoint_url(explicit: Option<&str>, path: &str) -> String {
    match explicit.map(str::trim).filter(|base| !base.is_empty()) {
        Some(base) => format!("{}{path}", base.trim_end_matches('/')),
        None => api_url(path),
    }
}

fn review_packet_body(
    files: &[String],
    functions: &[FunctionTarget],
    period_days: Option<u16>,
    project_id: Option<&str>,
    commit_sha: Option<&str>,
) -> Value {
    let mut body = Map::new();
    if !files.is_empty() {
        body.insert("files".to_owned(), json!(files));
    }
    if !functions.is_empty() {
        let targets: Vec<Value> = functions
            .iter()
            .map(|target| {
                let mut entry = Map::new();
                entry.insert("file".to_owned(), json!(target.file));
                entry.insert("name".to_owned(), json!(target.name));
                if let Some(line) = target.line {
                    entry.insert("line".to_owned(), json!(line));
                }
                Value::Object(entry)
            })
            .collect();
        body.insert("functions".to_owned(), Value::Array(targets));
    }
    if let Some(days) = period_days {
        body.insert("periodDays".to_owned(), json!(days));
    }
    if let Some(project_id) = project_id.map(str::trim).filter(|v| !v.is_empty()) {
        body.insert("projectId".to_owned(), json!(project_id));
    }
    if let Some(sha) = commit_sha {
        body.insert("gitSha".to_owned(), json!(sha));
    }
    Value::Object(body)
}

fn validate_period(period: Option<u16>) -> Result<Option<u16>, CloudError> {
    match period {
        Some(days) if days == 0 || days > MAX_PERIOD_DAYS => Err(CloudError::Validation(format!(
            "--coverage-period must be between 1 and {MAX_PERIOD_DAYS} days"
        ))),
        other => Ok(other),
    }
}

/// Check a commit SHA against the cloud rule (`COMMIT_SHA_PATTERN` in
/// fallow-cloud): 1 to 64 letters, digits, dots, underscores and hyphens, and
/// not only dots. A deployment can carry a non-hex id such as `v1.2.3`, and
/// each SHA that the cloud stores must also be readable here.
fn validate_sha(raw: &str, flag: &str) -> Result<String, CloudError> {
    let sha = raw.trim();
    let allowed = |c: char| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-');
    let valid = !sha.is_empty()
        && sha.len() <= MAX_SHA_LEN
        && sha.chars().all(allowed)
        && !sha.chars().all(|c| c == '.');
    if !valid {
        return Err(CloudError::Validation(format!(
            "{flag} must be a commit SHA of 1 to {MAX_SHA_LEN} letters, digits, dots, underscores or hyphens (not only dots), got {raw}"
        )));
    }
    Ok(sha.to_owned())
}

fn head_sha(root: &Path) -> Result<String, CloudError> {
    fallow_engine::repo_refs::head_sha(root)
        .ok()
        .flatten()
        .map(|sha| sha.trim().to_owned())
        .filter(|sha| !sha.is_empty())
        .ok_or_else(|| {
            CloudError::Validation(
                "could not resolve the deployment commit with `git rev-parse HEAD`. Pass --sha <sha>."
                    .to_owned(),
            )
        })
}

/// Parse `FILE:NAME` or `FILE:NAME:LINE`. The name and the line are split
/// from the right, so a file path can hold a colon.
fn parse_function_target(raw: &str) -> Result<FunctionTarget, CloudError> {
    let invalid = || {
        CloudError::Validation(format!(
            "--function must be FILE:NAME or FILE:NAME:LINE, got {raw}"
        ))
    };
    let trimmed = raw.trim();
    let (head, last) = trimmed.rsplit_once(':').ok_or_else(invalid)?;
    let (file, name, line) = match last.parse::<u32>() {
        Ok(line) => {
            let (file, name) = head.rsplit_once(':').ok_or_else(invalid)?;
            (file, name, Some(line))
        }
        Err(_) => (head, last, None),
    };
    let file = normalize_file(file);
    let name = name.trim();
    if file.is_empty() || name.is_empty() {
        return Err(invalid());
    }
    Ok(FunctionTarget {
        file,
        name: name.to_owned(),
        line,
    })
}

fn normalize_file(raw: &str) -> String {
    raw.trim()
        .replace('\\', "/")
        .trim_start_matches("./")
        .to_owned()
}

fn normalize_files(raw: &[String]) -> Vec<String> {
    let mut files: Vec<String> = raw
        .iter()
        .map(|file| normalize_file(file))
        .filter(|file| !file.is_empty())
        .collect();
    files.sort();
    files.dedup();
    files
}

/// Source files changed against the base ref, repo-relative to the git
/// toplevel. The base resolves like `fallow audit`: `--base`, then
/// `FALLOW_AUDIT_BASE`, then the merge-base with the upstream or the remote
/// default branch.
fn changed_source_files(root: &Path, base: Option<&str>) -> Result<Vec<String>, CloudError> {
    let resolved = fallow_api::audit_run::resolve_audit_base(root, base).map_err(|_| {
        CloudError::Validation(
            "could not find a base ref for the changed files. Pass --base <ref>, --file or --function."
                .to_owned(),
        )
    })?;
    let toplevel = fallow_engine::changed_files::resolve_git_toplevel(root)
        .map_err(|err| CloudError::Validation(err.describe()))?;
    let changed = fallow_engine::changed_files::changed_files(root, &resolved.git_ref)
        .map_err(|err| CloudError::Validation(err.describe()))?;
    let mut files: Vec<String> = changed
        .iter()
        .filter(|path| is_source_file(path))
        .filter(|path| path.exists())
        .filter_map(|path| repo_relative(path, &toplevel))
        .collect();
    files.sort();
    files.dedup();
    Ok(files)
}

fn is_source_file(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| SOURCE_EXTENSIONS.contains(&ext))
}

fn repo_relative(path: &Path, toplevel: &Path) -> Option<String> {
    let relative = path.strip_prefix(toplevel).ok()?.to_path_buf();
    Some(relative.to_string_lossy().replace('\\', "/"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn function_target_parses_name_and_optional_line() {
        assert_eq!(
            parse_function_target("src/a.ts:handler:12").expect("valid"),
            FunctionTarget {
                file: "src/a.ts".to_owned(),
                name: "handler".to_owned(),
                line: Some(12),
            }
        );
        assert_eq!(
            parse_function_target("./src/a.ts:render").expect("valid"),
            FunctionTarget {
                file: "src/a.ts".to_owned(),
                name: "render".to_owned(),
                line: None,
            }
        );
        assert!(parse_function_target("src/a.ts").is_err());
        assert!(parse_function_target(":name").is_err());
        assert!(parse_function_target("src/a.ts::3").is_err());
    }

    #[test]
    fn review_packet_body_omits_absent_fields() {
        let body = review_packet_body(&["src/a.ts".to_owned()], &[], None, None, None);
        assert_eq!(body, json!({ "files": ["src/a.ts"] }));
    }

    #[test]
    fn sha_validation_matches_the_cloud_commit_sha_rule() {
        assert!(validate_sha("abc123", "--sha").is_ok());
        assert!(validate_sha("v1.2.3", "--sha").is_ok());
        assert!(validate_sha("build-42", "--sha").is_ok());
        assert!(validate_sha("release_7", "--sha").is_ok());
        assert!(validate_sha(&"a".repeat(64), "--sha").is_ok());
        assert!(validate_sha(&"a".repeat(65), "--sha").is_err());
        assert!(validate_sha("", "--sha").is_err());
        assert!(validate_sha(".", "--sha").is_err());
        assert!(validate_sha("..", "--sha").is_err());
        assert!(validate_sha("a/b", "--sha").is_err());
        assert!(validate_sha("a b", "--sha").is_err());
        assert!(validate_sha("abc?x=1", "--sha").is_err());
    }

    #[test]
    fn period_validation_matches_the_cloud_bounds() {
        assert!(validate_period(Some(0)).is_err());
        assert!(validate_period(Some(91)).is_err());
        assert_eq!(validate_period(Some(30)).expect("valid"), Some(30));
        assert_eq!(validate_period(None).expect("valid"), None);
    }

    #[test]
    fn debug_masks_the_api_key() {
        let args = ReviewPacketArgs {
            api_key: Some("fallow_live_secret".to_owned()),
            ..ReviewPacketArgs::default()
        };
        assert!(!format!("{args:?}").contains("fallow_live_secret"));
        let args = DeploymentChangesArgs {
            api_key: Some("fallow_live_secret".to_owned()),
            ..DeploymentChangesArgs::default()
        };
        assert!(!format!("{args:?}").contains("fallow_live_secret"));
    }
}
