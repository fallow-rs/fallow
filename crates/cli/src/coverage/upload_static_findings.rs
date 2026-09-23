//! `fallow coverage upload-static-findings` - push static dead-code verdicts
//! to fallow cloud.
//!
//! These are the **static side** of the cloud source-evidence viewer. The
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

use std::fmt;
use std::path::Path;
use std::process::ExitCode;

use fallow_config::ResolvedConfig;
use serde::{Deserialize, Serialize};

use colored::Colorize as _;

use crate::api::{
    ResponseBodyReader, parse_error_envelope, sanitize_network_error, try_api_agent_with_timeout,
};
use crate::coverage::upload_common::{
    self, UploadError, display_endpoint_url, format_count, format_upload_error_message,
    to_posix_string,
};

/// Log prefix used on every human-facing line from this subcommand.
/// Matches the pattern established by sibling commands so CI log parsers can
/// anchor on it.
const LOG_PREFIX: &str = "fallow coverage upload-static-findings";

/// Matches the finding-count limit that the server enforces. The client checks
/// it first so users see a specific error before a 413 response.
const STATIC_FINDINGS_MAX: usize = 200_000;

/// HTTP timeouts for the upload. The body is small (<=200k findings) but can
/// take longer than license's 10s global cap on congested networks.
const UPLOAD_CONNECT_TIMEOUT_SECS: u64 = 5;
const UPLOAD_TOTAL_TIMEOUT_SECS: u64 = 30;

/// Stable wire-format kind for an unused export finding.
const KIND_UNUSED_EXPORT: &str = "unused_export";
/// Stable wire-format kind for a dead file finding.
const KIND_DEAD_FILE: &str = "dead_file";

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
pub fn run(args: &UploadStaticFindingsArgs, root: &Path, allow_remote_extends: bool) -> ExitCode {
    match run_inner(args, root, allow_remote_extends) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => err.into_exit(LOG_PREFIX, args.ignore_upload_errors),
    }
}

fn run_inner(
    args: &UploadStaticFindingsArgs,
    root: &Path,
    allow_remote_extends: bool,
) -> Result<(), UploadError> {
    let project_id = upload_common::resolve_project_id(args.project_id.as_deref(), root)
        .map_err(UploadError::Validation)?;
    let git_sha = upload_common::resolve_git_sha(args.git_sha.as_deref(), root)
        .map_err(UploadError::Validation)?;
    upload_common::enforce_clean_worktree(
        LOG_PREFIX,
        "upload-static-findings",
        "the findings come",
        args.dry_run,
        args.allow_dirty,
        root,
    )?;

    let config = upload_common::load_resolved_config_with_options(root, allow_remote_extends)
        .map_err(UploadError::Validation)?;
    let results = fallow_engine::session::AnalysisSession::from_resolved_config(config.clone())
        .and_then(|session| session.analyze_dead_code_with_artifacts(false, false))
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

    let api_key =
        upload_common::resolve_api_key(args.api_key.as_deref()).map_err(UploadError::Validation)?;
    upload(
        &project_id,
        args.api_endpoint.as_deref(),
        &api_key,
        &payload,
    )
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

fn upload(
    project_id: &str,
    endpoint_override: Option<&str>,
    api_key: &str,
    payload: &StaticFindingsRequest<'_>,
) -> Result<(), UploadError> {
    let url = upload_common::endpoint_url(endpoint_override, project_id, "static-findings");
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
            "  -> Static findings stored. View them on the source-evidence viewer: {}",
            upload_common::dashboard_repo_url(project_id)
        );
        return Ok(());
    }

    let body = response.read_to_string().unwrap_or_default();
    let envelope = parse_error_envelope(&body);
    let code = envelope.code();
    let message =
        format_upload_error_message("upload-static-findings", status, &body, code, &envelope);
    classify_upload_error(status, code, message)
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

fn print_dry_run_summary(
    project_id: &str,
    git_sha: &str,
    findings: &[StaticFinding],
    endpoint_override: Option<&str>,
) {
    let decoded_url = display_endpoint_url(endpoint_override, project_id, "static-findings");
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
    use fallow_config::FallowConfig;
    use std::path::PathBuf;
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
    fn format_upload_error_message_uses_hint_for_known_code() {
        let envelope = parse_error_envelope(r#"{"code":"payload_too_large"}"#);
        let message = format_upload_error_message(
            "upload-static-findings",
            413,
            "{}",
            Some("payload_too_large"),
            &envelope,
        );
        assert!(message.contains("200,000"), "got: {message}");
        assert!(message.contains("HTTP 413"));
        assert!(message.contains("code payload_too_large"));
    }

    #[test]
    fn format_upload_error_message_falls_back_to_server_message() {
        let body = r#"{"code":"internal","message":"database timeout"}"#;
        let envelope = parse_error_envelope(body);
        let message = format_upload_error_message(
            "upload-static-findings",
            500,
            body,
            Some("internal"),
            &envelope,
        );
        assert!(message.starts_with("upload-static-findings request failed with HTTP 500"));
        assert!(message.ends_with(": database timeout"));
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
        let code = run(&dry_run_args(), project.path(), false);
        assert_eq!(code, ExitCode::SUCCESS);
    }
}
