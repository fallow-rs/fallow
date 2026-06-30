//! `fallow coverage upload-static-findings` - push static dead-code verdicts
//! to fallow cloud.
//!
//! These are the **static side** of the source-evidence viewer (ADR 024). The
//! runtime coverage pipeline ships function hit-counts; this command ships
//! fallow's own static analysis verdicts (`unused_export`, `dead_file`) so the
//! cloud can overlay them onto the source view alongside the runtime overlay.
//!
//! The cloud join key is `filePath`, matched against the source-map
//! `sourcesContent` paths in the viewer. Findings are keyed to a git SHA and
//! the server applies **replace-by-SHA** semantics: each upload fully replaces
//! the prior finding set for `(org, repo, gitSha)` in one transaction. The
//! CLI therefore sends the complete finding set for the SHA on every run; no
//! incremental or merge logic is needed client-side. An empty finding set is a
//! valid clearing of the prior set for that SHA.
//!
//! Unlike `upload-inventory`, which only walks the source tree, this command
//! runs the full static analysis, so it is slower and can surface config or
//! parse errors that the inventory walk never hits. Those are surfaced as
//! validation errors (exit 10) so CI distinguishes a fixable input problem
//! from a transient server failure.
//!
//! This subcommand is a paid-tier workflow. It runs only when the user invokes
//! it explicitly; no other fallow command touches the network.

use std::fmt::{self, Write as _};
use std::path::Path;
use std::process::ExitCode;

use fallow_config::ResolvedConfig;
use serde::{Deserialize, Serialize};

use colored::Colorize as _;

use crate::api::{
    NETWORK_EXIT_CODE, ParsedErrorEnvelope, ResponseBodyReader, actionable_error_hint, api_url,
    parse_error_envelope, response_message_suffix, sanitize_network_error,
    try_api_agent_with_timeout,
};
use crate::coverage::upload_common;

/// Log prefix used on every human-facing line from this subcommand.
/// Matches the pattern established by sibling commands so CI log parsers can
/// anchor on it.
const LOG_PREFIX: &str = "fallow coverage upload-static-findings";

/// Server-enforced cap on the finding count. Mirrors `STATIC_FINDINGS_MAX` in
/// `fallow-cloud/src/routes/coverage.ts`. Validated client-side so users see a
/// specific error before a 413 round-trip.
const STATIC_FINDINGS_MAX: usize = 200_000;

/// HTTP timeouts for the upload. The body is small (<=200k findings) but can
/// take longer than license's 10s global cap on congested networks.
const UPLOAD_CONNECT_TIMEOUT_SECS: u64 = 5;
const UPLOAD_TOTAL_TIMEOUT_SECS: u64 = 30;

/// Stable wire-format kind for an unused export finding.
const KIND_UNUSED_EXPORT: &str = "unused_export";
/// Stable wire-format kind for a dead file finding.
const KIND_DEAD_FILE: &str = "dead_file";

/// Exit codes. Documented in `fallow coverage upload-static-findings --help`.
/// User-fixable errors are separated from transient server errors so CI
/// pipelines can distinguish retry vs fail-the-build.
const EXIT_VALIDATION: u8 = 10;
const EXIT_PAYLOAD_TOO_LARGE: u8 = 11;
const EXIT_AUTH_REJECTED: u8 = 12;
const EXIT_SERVER_ERROR: u8 = 13;

/// Arguments for `fallow coverage upload-static-findings`.
#[derive(Clone, Default)]
pub struct UploadStaticFindingsArgs {
    /// Explicit API key. Overrides `$FALLOW_API_KEY`.
    pub api_key: Option<String>,
    /// Explicit API endpoint base (e.g. staging, on-prem). Overrides
    /// `$FALLOW_API_URL` and the compiled-in default.
    pub api_endpoint: Option<String>,
    /// Explicit project identifier (`fallow-cloud-api` or `owner/repo`).
    /// Overrides the auto-detected git remote + `$GITHUB_REPOSITORY` /
    /// `$CI_PROJECT_PATH` heuristics.
    pub project_id: Option<String>,
    /// Explicit git SHA. Overrides `git rev-parse HEAD`.
    pub git_sha: Option<String>,
    /// Proceed even when the working tree has uncommitted changes.
    /// The findings are still generated from the working copy, so they may
    /// not match the uploaded git SHA.
    pub allow_dirty: bool,
    /// Print what would be uploaded and exit, without any network call.
    pub dry_run: bool,
    /// Soft-fail on upload errors: print the warning but return exit code 0.
    /// The default is to fail loud (exit nonzero) for any upload error.
    pub ignore_upload_errors: bool,
}

// Manual `Debug` to keep the API key out of stderr.
impl fmt::Debug for UploadStaticFindingsArgs {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("UploadStaticFindingsArgs")
            .field("api_key", &self.api_key.as_ref().map(|_| "***"))
            .field("api_endpoint", &self.api_endpoint)
            .field("project_id", &self.project_id)
            .field("git_sha", &self.git_sha)
            .field("allow_dirty", &self.allow_dirty)
            .field("dry_run", &self.dry_run)
            .field("ignore_upload_errors", &self.ignore_upload_errors)
            .finish()
    }
}

/// Dispatch `fallow coverage upload-static-findings`.
pub fn run(args: &UploadStaticFindingsArgs, root: &Path) -> ExitCode {
    match run_inner(args, root) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => err.into_exit(args.ignore_upload_errors),
    }
}

/// Outcome of the upload workflow. Errors carry an exit code so each call
/// site can pick a code matching the failure class, while the CLI dispatch
/// downgrades transient upload errors to a warning when the user opts in.
#[derive(Debug)]
enum UploadError {
    /// User-fixable input error (missing key, unresolvable project-id,
    /// analysis/config failure, ...).
    Validation(String),
    /// Finding set exceeds the server cap; user must scope the analysis.
    PayloadTooLarge(String),
    /// 401 / 403: auth rejected, the user needs to rotate or scope the key.
    AuthRejected(String),
    /// 5xx, timeout, transport failure; transient.
    ServerError(String),
    /// Transport-level failure before response (DNS, TLS, connect).
    Network(String),
}

impl UploadError {
    fn into_exit(self, ignore_upload_errors: bool) -> ExitCode {
        let soft_fail =
            ignore_upload_errors && matches!(&self, Self::ServerError(_) | Self::Network(_));
        let (code, body) = match self {
            Self::Validation(m) => (EXIT_VALIDATION, m),
            Self::PayloadTooLarge(m) => (EXIT_PAYLOAD_TOO_LARGE, m),
            Self::AuthRejected(m) => (EXIT_AUTH_REJECTED, m),
            Self::ServerError(m) => (EXIT_SERVER_ERROR, m),
            Self::Network(m) => (NETWORK_EXIT_CODE, m),
        };
        let severity = if soft_fail {
            "warning".yellow().bold()
        } else {
            "error".red().bold()
        };
        eprintln!("{LOG_PREFIX}: {severity}: {body}");
        if soft_fail {
            eprintln!("  -> --ignore-upload-errors set, continuing with exit 0");
            return ExitCode::SUCCESS;
        }
        ExitCode::from(code)
    }
}

fn run_inner(args: &UploadStaticFindingsArgs, root: &Path) -> Result<(), UploadError> {
    let project_id = resolve_project_id(args, root)?;
    let git_sha = resolve_git_sha(args, root)?;
    enforce_clean_worktree(args, root)?;

    let config = load_resolved_config(root)?;
    let results = fallow_engine::analyze(&config)
        .map(|analysis| analysis.results)
        .map_err(|err| UploadError::Validation(format!("analysis failed: {err}")))?;
    let findings = collect_findings(&config, &results);

    if findings.len() > STATIC_FINDINGS_MAX {
        return Err(UploadError::PayloadTooLarge(format!(
            "static analysis produced {} findings, exceeds the server limit of {}. \
             Scope the analysis with your fallow ignore rules, or open an issue if \
             your repo is legitimately larger.",
            findings.len(),
            STATIC_FINDINGS_MAX
        )));
    }

    let payload = StaticFindingsRequest {
        git_sha: &git_sha,
        findings: &findings,
    };

    if args.dry_run {
        print_dry_run_summary(
            &project_id,
            &git_sha,
            &findings,
            args.api_endpoint.as_deref(),
        );
        return Ok(());
    }

    let api_key = resolve_api_key(args)?;
    upload(
        &project_id,
        args.api_endpoint.as_deref(),
        &api_key,
        &payload,
    )
}

fn resolve_project_id(args: &UploadStaticFindingsArgs, root: &Path) -> Result<String, UploadError> {
    upload_common::resolve_project_id(args.project_id.as_deref(), root)
        .map_err(UploadError::Validation)
}

fn resolve_git_sha(args: &UploadStaticFindingsArgs, root: &Path) -> Result<String, UploadError> {
    upload_common::resolve_git_sha(args.git_sha.as_deref(), root).map_err(UploadError::Validation)
}

fn enforce_clean_worktree(args: &UploadStaticFindingsArgs, root: &Path) -> Result<(), UploadError> {
    if args.dry_run {
        return Ok(());
    }
    if !dirty_worktree(root) {
        return Ok(());
    }
    if args.allow_dirty {
        eprintln!(
            "{LOG_PREFIX}: {}: working tree has uncommitted changes. Proceeding because --allow-dirty was set, but the findings come from the working copy and may not match the uploaded git SHA.",
            "warning".yellow().bold(),
        );
        return Ok(());
    }
    Err(UploadError::Validation(
        "working tree has uncommitted changes. `upload-static-findings` is keyed to a git SHA, so uploading the working copy would drift from that commit. Commit or stash first, or pass --allow-dirty to intentionally upload the working copy."
            .to_owned(),
    ))
}

fn dirty_worktree(root: &Path) -> bool {
    upload_common::dirty_worktree(root)
}

fn load_resolved_config(root: &Path) -> Result<ResolvedConfig, UploadError> {
    upload_common::load_resolved_config(root).map_err(UploadError::Validation)
}

/// Map the static analysis results into the cloud finding wire shape.
///
/// `unused_files` become `dead_file` findings (no export name or line);
/// `unused_exports` become `unused_export` findings carrying the export name
/// and 1-based line. Paths are stripped to repo-relative and POSIX-normalized
/// identically to `upload-inventory::collect_inventory`, so `filePath` lines
/// up with the source-map `sources[]` paths in the viewer.
///
/// Type-only exports (`is_type_only == true`) are emitted as `unused_export`:
/// the v1 cloud kind set has no separate type kind and the column is lenient.
fn collect_findings(config: &ResolvedConfig, results: &impl AnalysisLike) -> Vec<StaticFinding> {
    let mut out: Vec<StaticFinding> = Vec::new();

    for finding in results.unused_files() {
        out.push(StaticFinding {
            kind: KIND_DEAD_FILE,
            file_path: repo_relative_posix(config, finding),
            export_name: None,
            line_number: None,
        });
    }

    for (path, export_name, line) in results.unused_exports() {
        out.push(StaticFinding {
            kind: KIND_UNUSED_EXPORT,
            file_path: repo_relative_posix(config, path),
            export_name: Some(export_name),
            line_number: Some(line),
        });
    }

    out.sort_by(|a, b| {
        a.file_path
            .cmp(&b.file_path)
            .then(a.kind.cmp(b.kind))
            .then(a.line_number.cmp(&b.line_number))
            .then(a.export_name.cmp(&b.export_name))
    });
    out
}

/// Strip the config root and POSIX-normalize a finding path. Falls back to the
/// raw path when the strip fails (path already relative or outside the root),
/// matching `collect_inventory`'s behavior.
fn repo_relative_posix(config: &ResolvedConfig, path: &Path) -> String {
    let rel = path
        .strip_prefix(&config.root)
        .map_or(path, |stripped| stripped);
    to_posix_string(rel)
}

fn to_posix_string(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

#[derive(Debug, Clone, Serialize)]
struct StaticFinding {
    kind: &'static str,
    #[serde(rename = "filePath")]
    file_path: String,
    #[serde(rename = "exportName", skip_serializing_if = "Option::is_none")]
    export_name: Option<String>,
    #[serde(rename = "lineNumber", skip_serializing_if = "Option::is_none")]
    line_number: Option<u32>,
}

#[derive(Debug, Serialize)]
struct StaticFindingsRequest<'a> {
    #[serde(rename = "gitSha")]
    git_sha: &'a str,
    findings: &'a [StaticFinding],
}

#[derive(Debug, Deserialize)]
struct StaticFindingsResponseData {
    #[serde(rename = "gitSha")]
    git_sha: String,
    count: u64,
}

#[derive(Debug, Deserialize)]
struct StaticFindingsResponseEnvelope {
    data: StaticFindingsResponseData,
}

fn resolve_api_key(args: &UploadStaticFindingsArgs) -> Result<String, UploadError> {
    if let Some(explicit) = args.api_key.as_deref() {
        let trimmed = explicit.trim();
        if !trimmed.is_empty() {
            return Ok(trimmed.to_owned());
        }
    }
    if let Ok(from_env) = std::env::var("FALLOW_API_KEY") {
        let trimmed = from_env.trim();
        if !trimmed.is_empty() {
            return Ok(trimmed.to_owned());
        }
    }
    Err(UploadError::Validation(
        "no API key. Set $FALLOW_API_KEY or pass --api-key <KEY>. Generate at \
         https://fallow.cloud/settings#api-keys."
            .to_owned(),
    ))
}

fn endpoint_url(override_endpoint: Option<&str>, project_id: &str) -> String {
    let path = format!(
        "/v1/coverage/{}/static-findings",
        url_encode_path_segment(project_id)
    );
    match override_endpoint {
        Some(base) => format!("{}{path}", base.trim().trim_end_matches('/')),
        None => api_url(&path),
    }
}

/// URL-encode the `{repo}` path segment.
///
/// Project IDs can be bare (`fallow-cloud-api`) or slash-scoped
/// (`acme/widgets`), but the server receives them as a single percent-encoded
/// segment under `/v1/coverage/{repo}/static-findings`, so `/` must be encoded
/// too.
#[expect(
    clippy::expect_used,
    reason = "formatting percent-encoded bytes into String is infallible"
)]
fn url_encode_path_segment(value: &str) -> String {
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

fn upload(
    project_id: &str,
    endpoint_override: Option<&str>,
    api_key: &str,
    payload: &StaticFindingsRequest<'_>,
) -> Result<(), UploadError> {
    let url = endpoint_url(endpoint_override, project_id);
    println!(
        "{LOG_PREFIX}: uploading {} findings for {project_id} @ {}",
        format_count(payload.findings.len()),
        payload.git_sha,
    );

    let agent = try_api_agent_with_timeout(UPLOAD_CONNECT_TIMEOUT_SECS, UPLOAD_TOTAL_TIMEOUT_SECS)
        .map_err(|err| UploadError::Network(err.to_string()))?;
    let mut response = agent
        .post(&url)
        .header("Authorization", &format!("Bearer {api_key}"))
        .send_json(payload)
        .map_err(|err| {
            UploadError::Network(sanitize_network_error(&format!("network error: {err}")))
        })?;

    let status = response.status().as_u16();
    if matches!(status, 200 | 201) {
        let data: StaticFindingsResponseEnvelope = response
            .read_json()
            .map_err(|err| UploadError::ServerError(format!("malformed response body: {err}")))?;
        let count = usize::try_from(data.data.count).unwrap_or(usize::MAX);
        println!(
            "{LOG_PREFIX}: {} · {} findings stored @ {}",
            "ok".green().bold(),
            format_count(count),
            data.data.git_sha,
        );
        println!(
            "  -> Static findings stored. View them on the source-evidence viewer: https://fallow.cloud/{project_id}"
        );
        return Ok(());
    }

    let body = response.read_to_string().unwrap_or_default();
    let envelope = parse_error_envelope(&body);
    let code = envelope.code();
    let message = format_upload_error_message(status, &body, code, &envelope);
    classify_upload_error(status, code, message)
}

fn format_upload_error_message(
    status: u16,
    body: &str,
    code: Option<&str>,
    envelope: &ParsedErrorEnvelope,
) -> String {
    if let Some(code) = code
        && let Some(hint) = actionable_error_hint("upload-static-findings", code)
    {
        return format!("{hint} (HTTP {status}, code {code})");
    }
    let body_suffix = response_message_suffix(body, envelope);
    format!("upload-static-findings request failed with HTTP {status}{body_suffix}")
}

/// Classify an error response into an [`UploadError`] variant.
///
/// Unlike `upload-inventory`, this endpoint returns **413** (not 400) for the
/// finding-count cap, so the cap maps off the status code, not a body code.
fn classify_upload_error(
    status: u16,
    _code: Option<&str>,
    message: String,
) -> Result<(), UploadError> {
    match status {
        413 => Err(UploadError::PayloadTooLarge(message)),
        400 => Err(UploadError::Validation(message)),
        401 | 403 => Err(UploadError::AuthRejected(message)),
        _ => Err(UploadError::ServerError(message)),
    }
}

fn format_count(n: usize) -> String {
    let mut s = n.to_string();
    let mut i = s.len();
    while i > 3 {
        i -= 3;
        s.insert(i, ',');
    }
    s
}

fn print_dry_run_summary(
    project_id: &str,
    git_sha: &str,
    findings: &[StaticFinding],
    endpoint_override: Option<&str>,
) {
    let decoded_url = display_endpoint_url(endpoint_override, project_id);
    let dead_files = findings.iter().filter(|f| f.kind == KIND_DEAD_FILE).count();
    let unused_exports = findings
        .iter()
        .filter(|f| f.kind == KIND_UNUSED_EXPORT)
        .count();
    println!("{LOG_PREFIX} {}", "(dry run)".bright_black());
    println!("  project-id:     {project_id}");
    println!("  git-sha:        {git_sha}");
    println!("  findings:       {}", format_count(findings.len()));
    println!("    dead_file:    {}", format_count(dead_files));
    println!("    unused_export:{}", format_count(unused_exports));
    println!("  endpoint:       {decoded_url}");
    println!();
    let shown = findings.len().min(5);
    let total = findings.len();
    println!("first {shown} of {} findings:", format_count(total));
    for finding in findings.iter().take(shown) {
        match (&finding.export_name, finding.line_number) {
            (Some(name), Some(line)) => {
                println!("  {} {}:{}  {name}", finding.kind, finding.file_path, line);
            }
            _ => {
                println!("  {} {}", finding.kind, finding.file_path);
            }
        }
    }
    if total > shown {
        println!(
            "  ... and {} more",
            format_count(total.saturating_sub(shown)),
        );
    }
}

fn display_endpoint_url(override_endpoint: Option<&str>, project_id: &str) -> String {
    let base = override_endpoint.map_or_else(
        || {
            std::env::var("FALLOW_API_URL")
                .ok()
                .filter(|v| !v.trim().is_empty())
                .map_or_else(
                    || "https://api.fallow.cloud".to_owned(),
                    |v| v.trim().trim_end_matches('/').to_owned(),
                )
        },
        |v| v.trim().trim_end_matches('/').to_owned(),
    );
    format!("{base}/v1/coverage/{project_id}/static-findings")
}

/// A minimal view over [`fallow_types::results::AnalysisResults`] that exposes
/// only the two finding categories this command maps. Defined as a trait so
/// the mapping in [`collect_findings`] can be unit-tested against an in-memory
/// stub without constructing a full `AnalysisResults`.
trait AnalysisLike {
    /// Absolute paths of files unreachable from any entry point.
    fn unused_files(&self) -> Vec<&Path>;
    /// `(path, export_name, line)` tuples for exports never imported, including
    /// type-only exports.
    fn unused_exports(&self) -> Vec<(&Path, String, u32)>;
}

impl AnalysisLike for fallow_types::results::AnalysisResults {
    fn unused_files(&self) -> Vec<&Path> {
        self.unused_files
            .iter()
            .map(|finding| finding.file.path.as_path())
            .collect()
    }

    fn unused_exports(&self) -> Vec<(&Path, String, u32)> {
        self.unused_exports
            .iter()
            .map(|finding| {
                (
                    finding.export.path.as_path(),
                    finding.export.export_name.clone(),
                    finding.export.line,
                )
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::coverage::upload_common::{
        GIT_SHA_MAX_LEN, parse_git_remote_to_project_id, validate_project_id,
    };
    use fallow_config::FallowConfig;
    use std::path::PathBuf;
    use std::process::Command;
    use tempfile::TempDir;

    /// In-memory analysis stub for [`collect_findings`] tests.
    struct StubResults {
        files: Vec<PathBuf>,
        exports: Vec<(PathBuf, String, u32)>,
    }

    impl AnalysisLike for StubResults {
        fn unused_files(&self) -> Vec<&Path> {
            self.files.iter().map(PathBuf::as_path).collect()
        }

        fn unused_exports(&self) -> Vec<(&Path, String, u32)> {
            self.exports
                .iter()
                .map(|(path, name, line)| (path.as_path(), name.clone(), *line))
                .collect()
        }
    }

    fn stub_config(root: &Path) -> ResolvedConfig {
        FallowConfig::default().resolve(
            root.to_path_buf(),
            fallow_config::OutputFormat::Human,
            1,
            true,
            true,
            None,
        )
    }

    #[test]
    fn upload_static_findings_args_debug_masks_api_key() {
        let args = UploadStaticFindingsArgs {
            api_key: Some("fallow_live_secret_token_value".to_owned()),
            api_endpoint: Some("https://api.fallow.cloud".to_owned()),
            project_id: Some("acme/web".to_owned()),
            ..UploadStaticFindingsArgs::default()
        };
        let formatted = format!("{args:?}");
        assert!(
            !formatted.contains("fallow_live_secret_token_value"),
            "api_key leaked through Debug: {formatted}"
        );
        assert!(
            formatted.contains("api_key: Some(\"***\")"),
            "expected explicit redaction marker, got: {formatted}"
        );
        let bare = UploadStaticFindingsArgs::default();
        let formatted_bare = format!("{bare:?}");
        assert!(
            formatted_bare.contains("api_key: None"),
            "expected None for unset api_key, got: {formatted_bare}"
        );
    }

    #[test]
    fn parse_git_remote_https_with_dot_git() {
        assert_eq!(
            parse_git_remote_to_project_id("https://github.com/fallow-rs/fallow.git"),
            Some("fallow-rs/fallow".to_owned())
        );
    }

    #[test]
    fn parse_git_remote_ssh_colon_shape() {
        assert_eq!(
            parse_git_remote_to_project_id("git@github.com:fallow-rs/fallow.git"),
            Some("fallow-rs/fallow".to_owned())
        );
    }

    #[test]
    fn parse_git_remote_protocol_shape() {
        assert_eq!(
            parse_git_remote_to_project_id("ssh://git@gitlab.com/fallow-rs/fallow.git"),
            Some("fallow-rs/fallow".to_owned())
        );
        assert_eq!(parse_git_remote_to_project_id("not-a-remote"), None);
    }

    #[test]
    fn validate_project_id_accepts_owner_repo_and_bare() {
        assert!(validate_project_id("fallow-rs/fallow").is_ok());
        assert!(validate_project_id("fallow-cloud-api").is_ok());
    }

    #[test]
    fn validate_project_id_rejects_path_traversal_and_empty() {
        assert!(validate_project_id("../etc/passwd").is_err());
        assert!(validate_project_id("acme/../secret").is_err());
        assert!(validate_project_id("").is_err());
    }

    #[test]
    fn url_encode_path_segment_encodes_slash() {
        assert_eq!(
            url_encode_path_segment("fallow-rs/fallow"),
            "fallow-rs%2Ffallow"
        );
        assert_eq!(url_encode_path_segment("a b"), "a%20b");
    }

    #[test]
    fn endpoint_url_builds_static_findings_path() {
        let url = endpoint_url(Some("http://127.0.0.1:3000"), "a/b");
        assert_eq!(
            url,
            "http://127.0.0.1:3000/v1/coverage/a%2Fb/static-findings"
        );
        let trimmed = endpoint_url(Some("http://127.0.0.1:3000/"), "a/b");
        assert_eq!(
            trimmed,
            "http://127.0.0.1:3000/v1/coverage/a%2Fb/static-findings"
        );
    }

    #[test]
    fn display_endpoint_url_uses_override_unencoded() {
        let url = display_endpoint_url(Some("http://127.0.0.1:3000/"), "a/b");
        assert_eq!(url, "http://127.0.0.1:3000/v1/coverage/a/b/static-findings");
    }

    #[test]
    fn to_posix_string_normalizes_windows_separators() {
        let p = Path::new("src\\foo\\bar.ts");
        assert_eq!(to_posix_string(p), "src/foo/bar.ts");
    }

    #[test]
    fn collect_findings_maps_kinds_with_repo_relative_paths() {
        let root = PathBuf::from("/repo");
        let config = stub_config(&root);
        let results = StubResults {
            files: vec![root.join("src/legacy/old.ts")],
            exports: vec![(
                root.join("src/utils/format.ts"),
                "formatBytes".to_owned(),
                42,
            )],
        };
        let findings = collect_findings(&config, &results);
        assert_eq!(findings.len(), 2);

        let dead = &findings[0];
        assert_eq!(dead.kind, KIND_DEAD_FILE);
        assert_eq!(dead.file_path, "src/legacy/old.ts");
        assert_eq!(dead.export_name, None);
        assert_eq!(dead.line_number, None);

        let export = &findings[1];
        assert_eq!(export.kind, KIND_UNUSED_EXPORT);
        assert_eq!(export.file_path, "src/utils/format.ts");
        assert_eq!(export.export_name.as_deref(), Some("formatBytes"));
        assert_eq!(export.line_number, Some(42));
    }

    #[test]
    fn collect_findings_empty_results_is_empty() {
        let root = PathBuf::from("/repo");
        let config = stub_config(&root);
        let results = StubResults {
            files: Vec::new(),
            exports: Vec::new(),
        };
        assert!(collect_findings(&config, &results).is_empty());
    }

    #[test]
    fn collect_findings_preserves_paths_outside_root() {
        let root = PathBuf::from("/repo");
        let config = stub_config(&root);
        let results = StubResults {
            files: vec![PathBuf::from("/outside/dead.ts")],
            exports: Vec::new(),
        };

        let findings = collect_findings(&config, &results);

        assert_eq!(findings[0].file_path, "/outside/dead.ts");
    }

    #[test]
    fn static_finding_serde_renames_and_skips_null_optionals() {
        let dead = StaticFinding {
            kind: KIND_DEAD_FILE,
            file_path: "src/dead.ts".to_owned(),
            export_name: None,
            line_number: None,
        };
        let json = serde_json::to_string(&dead).expect("serialize dead file");
        assert!(json.contains(r#""filePath":"src/dead.ts""#));
        assert!(
            !json.contains("exportName"),
            "null exportName must be omitted: {json}"
        );
        assert!(
            !json.contains("lineNumber"),
            "null lineNumber must be omitted: {json}"
        );

        let export = StaticFinding {
            kind: KIND_UNUSED_EXPORT,
            file_path: "src/a.ts".to_owned(),
            export_name: Some("foo".to_owned()),
            line_number: Some(7),
        };
        let json = serde_json::to_string(&export).expect("serialize export");
        assert!(json.contains(r#""exportName":"foo""#));
        assert!(json.contains(r#""lineNumber":7"#));
    }

    #[test]
    fn request_serde_renames_git_sha() {
        let findings: Vec<StaticFinding> = Vec::new();
        let req = StaticFindingsRequest {
            git_sha: "abc123",
            findings: &findings,
        };
        let json = serde_json::to_string(&req).expect("serialize request");
        assert!(json.contains(r#""gitSha":"abc123""#));
        assert!(json.contains(r#""findings":[]"#));
    }

    #[test]
    fn classify_upload_error_maps_413_to_payload_too_large() {
        let err = classify_upload_error(413, Some("payload_too_large"), "stub".to_owned())
            .expect_err("413 must error");
        assert!(matches!(err, UploadError::PayloadTooLarge(_)));
        let err = classify_upload_error(413, None, "stub".to_owned())
            .expect_err("413 must error without code");
        assert!(matches!(err, UploadError::PayloadTooLarge(_)));
    }

    #[test]
    fn classify_upload_error_maps_400_to_validation() {
        let err = classify_upload_error(400, Some("bad_request"), "stub".to_owned())
            .expect_err("400 must error");
        assert!(matches!(err, UploadError::Validation(_)));
    }

    #[test]
    fn classify_upload_error_maps_auth_codes_to_auth_rejected() {
        for status in [401, 403] {
            let err = classify_upload_error(status, Some("unauthorized"), "stub".to_owned())
                .expect_err("auth status must error");
            assert!(
                matches!(err, UploadError::AuthRejected(_)),
                "status={status}"
            );
        }
    }

    #[test]
    fn classify_upload_error_maps_5xx_to_server_error() {
        for status in [500, 502, 503, 504] {
            let err =
                classify_upload_error(status, None, "stub".to_owned()).expect_err("5xx must error");
            assert!(
                matches!(err, UploadError::ServerError(_)),
                "status={status}"
            );
        }
    }

    #[test]
    fn ignore_upload_errors_does_not_soft_fail_auth_rejection() {
        let exit = UploadError::AuthRejected("bad key".to_owned()).into_exit(true);
        assert_eq!(
            format!("{exit:?}"),
            format!("{:?}", ExitCode::from(EXIT_AUTH_REJECTED))
        );
    }

    #[test]
    fn ignore_upload_errors_does_not_soft_fail_payload_too_large() {
        let exit = UploadError::PayloadTooLarge("too big".to_owned()).into_exit(true);
        assert_eq!(
            format!("{exit:?}"),
            format!("{:?}", ExitCode::from(EXIT_PAYLOAD_TOO_LARGE))
        );
    }

    #[test]
    fn format_upload_error_message_uses_hint_for_known_code() {
        let envelope = parse_error_envelope(r#"{"code":"payload_too_large"}"#);
        let message = format_upload_error_message(413, "{}", Some("payload_too_large"), &envelope);
        assert!(message.contains("200,000"), "got: {message}");
        assert!(message.contains("HTTP 413"));
        assert!(message.contains("code payload_too_large"));
    }

    #[test]
    fn format_upload_error_message_falls_back_to_server_message() {
        let body = r#"{"code":"internal","message":"database timeout"}"#;
        let envelope = parse_error_envelope(body);
        let message = format_upload_error_message(500, body, Some("internal"), &envelope);
        assert!(message.starts_with("upload-static-findings request failed with HTTP 500"));
        assert!(message.ends_with(": database timeout"));
    }

    #[test]
    fn dirty_worktree_is_rejected_by_default() {
        let repo = create_dirty_git_repo();
        let err = enforce_clean_worktree(&UploadStaticFindingsArgs::default(), repo.path())
            .expect_err("dirty repo should fail without --allow-dirty");
        let UploadError::Validation(message) = err else {
            panic!("expected validation error, got {err:?}");
        };
        assert!(message.contains("working tree has uncommitted changes"));
        assert!(message.contains("--allow-dirty"));
    }

    #[test]
    fn dirty_worktree_is_allowed_with_explicit_opt_in() {
        let repo = create_dirty_git_repo();
        let args = UploadStaticFindingsArgs {
            allow_dirty: true,
            ..UploadStaticFindingsArgs::default()
        };
        assert!(enforce_clean_worktree(&args, repo.path()).is_ok());
    }

    #[test]
    fn dry_run_skips_dirty_worktree_validation() {
        let repo = create_dirty_git_repo();
        let args = UploadStaticFindingsArgs {
            dry_run: true,
            ..UploadStaticFindingsArgs::default()
        };
        assert!(enforce_clean_worktree(&args, repo.path()).is_ok());
    }

    fn create_dirty_git_repo() -> TempDir {
        let dir = tempfile::tempdir().expect("create temp repo");
        run_git(dir.path(), &["init", "-q"]);
        run_git(dir.path(), &["config", "commit.gpgsign", "false"]);
        run_git(dir.path(), &["config", "user.email", "review@example.com"]);
        run_git(dir.path(), &["config", "user.name", "Reviewer"]);
        std::fs::write(dir.path().join("a.js"), "function committed() {}\n")
            .expect("write committed file");
        run_git(dir.path(), &["add", "a.js"]);
        run_git(dir.path(), &["commit", "-qm", "init"]);
        std::fs::write(
            dir.path().join("a.js"),
            "function committed() {}\nfunction dirty() {}\n",
        )
        .expect("write dirty file");
        dir
    }

    fn run_git(root: &Path, args: &[&str]) {
        let status = Command::new("git")
            .args(args)
            .current_dir(root)
            .status()
            .expect("run git");
        assert!(status.success(), "git {args:?} failed");
    }

    fn project_with_unused_export() -> TempDir {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(
            root.join("package.json"),
            r#"{"name":"sf","main":"src/index.ts"}"#,
        )
        .unwrap();
        std::fs::write(root.join("src/index.ts"), "export const used = 1;\n").unwrap();
        // An unreferenced file/export gives the analysis something to report.
        std::fs::write(root.join("src/orphan.ts"), "export const orphan = 2;\n").unwrap();
        dir
    }

    fn dry_run_args() -> UploadStaticFindingsArgs {
        UploadStaticFindingsArgs {
            project_id: Some("acme/web".to_owned()),
            git_sha: Some("abcdef1".to_owned()),
            api_endpoint: Some("http://localhost:3000".to_owned()),
            allow_dirty: true,
            dry_run: true,
            ..UploadStaticFindingsArgs::default()
        }
    }

    #[test]
    fn run_dry_run_analyzes_and_exits_zero() {
        let project = project_with_unused_export();
        // Explicit project_id + git_sha keep this env- and git-free.
        let code = run(&dry_run_args(), project.path());
        assert_eq!(code, ExitCode::SUCCESS);
    }

    #[test]
    fn into_exit_maps_variants_and_soft_fails_transient_when_opted_in() {
        assert_eq!(
            UploadError::Validation("v".to_owned()).into_exit(false),
            ExitCode::from(EXIT_VALIDATION)
        );
        assert_eq!(
            UploadError::PayloadTooLarge("p".to_owned()).into_exit(false),
            ExitCode::from(EXIT_PAYLOAD_TOO_LARGE)
        );
        assert_eq!(
            UploadError::AuthRejected("a".to_owned()).into_exit(false),
            ExitCode::from(EXIT_AUTH_REJECTED)
        );
        assert_eq!(
            UploadError::ServerError("s".to_owned()).into_exit(false),
            ExitCode::from(EXIT_SERVER_ERROR)
        );
        assert_eq!(
            UploadError::Network("n".to_owned()).into_exit(false),
            ExitCode::from(NETWORK_EXIT_CODE)
        );
        // Only transient failures soft-fail under --ignore-upload-errors.
        assert_eq!(
            UploadError::ServerError("s".to_owned()).into_exit(true),
            ExitCode::SUCCESS
        );
        assert_eq!(
            UploadError::Network("n".to_owned()).into_exit(true),
            ExitCode::SUCCESS
        );
    }

    #[test]
    fn resolve_git_sha_validates_explicit_value() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        let with_sha = |sha: &str| UploadStaticFindingsArgs {
            git_sha: Some(sha.to_owned()),
            ..UploadStaticFindingsArgs::default()
        };
        assert_eq!(
            resolve_git_sha(&with_sha("abcdef1"), root).unwrap(),
            "abcdef1"
        );
        assert!(resolve_git_sha(&with_sha(""), root).is_err());
        assert!(resolve_git_sha(&with_sha(&"a".repeat(GIT_SHA_MAX_LEN + 1)), root).is_err());
        assert!(resolve_git_sha(&with_sha("bad sha!"), root).is_err());
    }
}
