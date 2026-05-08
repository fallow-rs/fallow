//! `fallow coverage upload-inventory` - push a static function inventory to
//! fallow cloud.
//!
//! The inventory is the **static side** of the three-state Production
//! Coverage story. The runtime coverage pipeline ships function hit-counts;
//! the cloud computes `inventory minus runtime-seen = untracked`.
//!
//! The cloud join key is `(filePath, functionName, lineNumber)` since the
//! line-aware function-identity migration (`0010`), so distinct same-named
//! functions at different lines in the same file are preserved and merged
//! into their own rows. The walker in `fallow_core::extract::inventory`
//! emits Istanbul / `oxc-coverage-instrument`-compatible names and unique
//! 1-based line numbers per function declaration.
//!
//! This subcommand is a paid-tier workflow. It runs only when the user
//! invokes it explicitly; no other fallow command touches the network.

use std::fmt::Write as _;
use std::path::Path;
use std::process::{Command, ExitCode};

use fallow_config::{FallowConfig, ResolvedConfig};
use fallow_core::extract::inventory::{InventoryEntry, walk_source};
use fallow_core::git_env::clear_ambient_git_env;
use globset::{Glob, GlobSet, GlobSetBuilder};
use rustc_hash::FxHashSet;
use serde::{Deserialize, Serialize};

use colored::Colorize as _;

use crate::api::{
    ErrorEnvelope, NETWORK_EXIT_CODE, ResponseBodyReader, actionable_error_hint,
    api_agent_with_timeout, api_url,
};

/// Log prefix used on every human-facing line from this subcommand.
/// Matches the pattern `fallow license:` / `fallow coverage setup:` established
/// by sibling commands so CI log parsers can anchor on it.
const LOG_PREFIX: &str = "fallow coverage upload-inventory";

/// Server-enforced cap on inventory size. Mirrors `INVENTORY_MAX_FUNCTIONS` in
/// `fallow-cloud/src/services/inventory.ts`. Validated client-side so users
/// see a specific error before a 400 round-trip.
const INVENTORY_MAX_FUNCTIONS: usize = 200_000;

/// Server-enforced `gitSha` length cap (inclusive).
const GIT_SHA_MAX_LEN: usize = 64;

/// HTTP timeouts for the upload. The body is small (<=200k function entries)
/// but can take longer than license's 10s global cap on congested networks.
const UPLOAD_CONNECT_TIMEOUT_SECS: u64 = 5;
const UPLOAD_TOTAL_TIMEOUT_SECS: u64 = 30;

/// Exit codes. Documented in `fallow coverage upload-inventory --help`.
/// User-fixable errors are separated from transient server errors so CI
/// pipelines can distinguish retry vs fail-the-build.
const EXIT_VALIDATION: u8 = 10;
const EXIT_PAYLOAD_TOO_LARGE: u8 = 11;
const EXIT_AUTH_REJECTED: u8 = 12;
const EXIT_SERVER_ERROR: u8 = 13;

/// File extensions the inventory walker handles. Plain JS/TS/JSX/TSX only;
/// SFC / Astro / MDX / CSS / HTML are out of scope for v1 and emit nothing.
const SUPPORTED_EXTENSIONS: &[&str] = &["js", "jsx", "mjs", "cjs", "ts", "tsx", "mts", "cts"];

/// Arguments for `fallow coverage upload-inventory`.
#[derive(Debug, Clone, Default)]
pub struct UploadInventoryArgs {
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
    /// The inventory is still generated from the working copy, so it may
    /// not match the uploaded git SHA.
    pub allow_dirty: bool,
    /// Additional glob patterns excluded from the walk (applied after the
    /// configured fallow ignore rules).
    pub exclude_paths: Vec<String>,
    /// Prefix prepended to every emitted filePath so the static inventory
    /// can match the path shape the runtime beacon reports. Required for
    /// containerized deployments where the Dockerfile `WORKDIR` (e.g.
    /// `/app`) rebases paths at runtime; the CLI emits repo-relative paths
    /// by default, which produce zero joins against `/app/*` runtime paths.
    pub path_prefix: Option<String>,
    /// Print what would be uploaded and exit, without any network call.
    pub dry_run: bool,
    /// Soft-fail on upload errors: print the warning but return exit code 0.
    /// The default is to fail loud (exit nonzero) for any upload error.
    pub ignore_upload_errors: bool,
}

/// Dispatch `fallow coverage upload-inventory`.
pub fn run(args: &UploadInventoryArgs, root: &Path) -> ExitCode {
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
    /// User-fixable input error (missing key, unresolvable project-id, ...).
    Validation(String),
    /// Inventory exceeds the server cap; user must scope the walk.
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
        // Validation, payload, and auth errors are always fatal; the user
        // needs to fix their inputs or credentials. The
        // --ignore-upload-errors opt-out only applies to transient transport
        // and server failures.
        if soft_fail {
            eprintln!("  -> --ignore-upload-errors set, continuing with exit 0");
            return ExitCode::SUCCESS;
        }
        ExitCode::from(code)
    }
}

fn run_inner(args: &UploadInventoryArgs, root: &Path) -> Result<(), UploadError> {
    let project_id = resolve_project_id(args, root)?;
    let git_sha = resolve_git_sha(args, root)?;
    let path_prefix = normalize_path_prefix(args.path_prefix.as_deref())?;
    enforce_clean_worktree(args, root)?;

    let config = load_resolved_config(root)?;
    let exclude_matcher = compile_exclude_matcher(&args.exclude_paths)?;
    let functions = collect_inventory(&config, &exclude_matcher, path_prefix.as_deref());

    if functions.is_empty() {
        return Err(UploadError::Validation(
            "no functions found in walk. Check --exclude-paths and your project's ignore \
             rules, or verify that the root contains JS/TS sources (declaration files \
             `*.d.ts` are intentionally skipped)."
                .to_owned(),
        ));
    }

    if functions.len() > INVENTORY_MAX_FUNCTIONS {
        return Err(UploadError::PayloadTooLarge(format!(
            "inventory has {} functions, exceeds the server limit of {}. \
             Scope the walk with --exclude-paths '<glob>' or open an issue if \
             your repo is legitimately larger.",
            functions.len(),
            INVENTORY_MAX_FUNCTIONS
        )));
    }

    let payload = InventoryRequest {
        git_sha: &git_sha,
        functions: &functions,
    };

    if args.dry_run {
        print_dry_run_summary(
            &project_id,
            &git_sha,
            path_prefix.as_deref(),
            &functions,
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

// ── Project ID resolution ────────────────────────────────────────────

fn resolve_project_id(args: &UploadInventoryArgs, root: &Path) -> Result<String, UploadError> {
    if let Some(explicit) = args.project_id.as_deref() {
        return validate_project_id(explicit.trim()).map(str::to_owned);
    }
    if let Ok(github_repo) = std::env::var("GITHUB_REPOSITORY") {
        let trimmed = github_repo.trim();
        if !trimmed.is_empty() {
            return validate_project_id(trimmed).map(str::to_owned);
        }
    }
    if let Ok(gitlab_path) = std::env::var("CI_PROJECT_PATH") {
        let trimmed = gitlab_path.trim();
        if !trimmed.is_empty() {
            return validate_project_id(trimmed).map(str::to_owned);
        }
    }
    if let Some(from_remote) = git_origin_project_id(root) {
        return Ok(from_remote);
    }
    Err(UploadError::Validation(
        "could not determine project id. Pass --project-id <project-id>, or set \
         $GITHUB_REPOSITORY / $CI_PROJECT_PATH, or ensure `git remote get-url origin` \
         returns a recognizable URL."
            .to_owned(),
    ))
}

/// Validate the project identifier used as the `{repo}` URL segment.
///
/// The server accepts any non-empty string without path-traversal, whether
/// bare (`fallow-cloud-api`) or slash-scoped (`acme/widgets`). Both shapes
/// appear in real usage: the dogfood projects use bare names, while
/// GitHub-origin parsing produces `owner/repo`. Keep validation minimal:
/// reject only what the server or filesystem would reject (empty, `..`).
fn validate_project_id(id: &str) -> Result<&str, UploadError> {
    if id.is_empty() {
        return Err(UploadError::Validation("project id is empty".to_owned()));
    }
    if id.contains("..") {
        return Err(UploadError::Validation(
            "project id must not contain '..' path segments".to_owned(),
        ));
    }
    Ok(id)
}

fn git_origin_project_id(root: &Path) -> Option<String> {
    let mut command = Command::new("git");
    command
        .args(["remote", "get-url", "origin"])
        .current_dir(root);
    clear_ambient_git_env(&mut command);
    let output = command.output().ok()?;
    if !output.status.success() {
        return None;
    }
    let url = String::from_utf8_lossy(&output.stdout).trim().to_string();
    parse_git_remote_to_project_id(&url)
}

/// Parse common git remote URL shapes into `owner/repo`. Covers HTTPS
/// (`https://github.com/owner/repo(.git)?`), SSH
/// (`git@github.com:owner/repo(.git)?`), and `ssh://` / `git://` variants.
fn parse_git_remote_to_project_id(url: &str) -> Option<String> {
    let stripped_suffix = url.trim().trim_end_matches(".git");
    // Shape 1: `git@host:owner/repo`
    if let Some((_, path)) = stripped_suffix.split_once(':')
        && let Some(project_id) = take_last_two_segments(path)
    {
        return Some(project_id);
    }
    // Shape 2: `scheme://host/owner/repo`
    if let Some(path_part) = stripped_suffix.split("://").nth(1)
        && let Some((_, tail)) = path_part.split_once('/')
        && let Some(project_id) = take_last_two_segments(tail)
    {
        return Some(project_id);
    }
    None
}

fn take_last_two_segments(path: &str) -> Option<String> {
    let mut parts: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    if parts.len() < 2 {
        return None;
    }
    let repo = parts.pop()?;
    let owner = parts.pop()?;
    Some(format!("{owner}/{repo}"))
}

// ── Git SHA resolution ───────────────────────────────────────────────

fn resolve_git_sha(args: &UploadInventoryArgs, root: &Path) -> Result<String, UploadError> {
    let sha = if let Some(explicit) = args.git_sha.as_deref() {
        explicit.trim().to_owned()
    } else {
        let mut command = Command::new("git");
        command.args(["rev-parse", "HEAD"]).current_dir(root);
        clear_ambient_git_env(&mut command);
        let output = command.output().map_err(|err| {
            UploadError::Validation(format!(
                "could not resolve git SHA: {err}. Pass --git-sha <sha> explicitly."
            ))
        })?;
        if !output.status.success() {
            return Err(UploadError::Validation(
                "`git rev-parse HEAD` failed. Pass --git-sha <sha> explicitly.".to_owned(),
            ));
        }
        String::from_utf8_lossy(&output.stdout).trim().to_owned()
    };

    if sha.is_empty() {
        return Err(UploadError::Validation("git sha is empty".to_owned()));
    }
    if sha.len() > GIT_SHA_MAX_LEN {
        return Err(UploadError::Validation(format!(
            "git sha is {} chars, server limit is {}",
            sha.len(),
            GIT_SHA_MAX_LEN
        )));
    }
    if !sha
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
    {
        return Err(UploadError::Validation(format!(
            "git sha '{sha}' contains characters outside [A-Za-z0-9._-]"
        )));
    }
    Ok(sha)
}

fn enforce_clean_worktree(args: &UploadInventoryArgs, root: &Path) -> Result<(), UploadError> {
    if !dirty_worktree(root) {
        return Ok(());
    }
    if args.allow_dirty {
        eprintln!(
            "{LOG_PREFIX}: {}: working tree has uncommitted changes. Proceeding because --allow-dirty was set, but the inventory comes from the working copy and may not match the uploaded git SHA.",
            "warning".yellow().bold(),
        );
        return Ok(());
    }
    Err(UploadError::Validation(
        "working tree has uncommitted changes. `upload-inventory` is keyed to a git SHA, so uploading the working copy would drift from that commit. Commit or stash first, or pass --allow-dirty to intentionally upload the working copy."
            .to_owned(),
    ))
}

fn dirty_worktree(root: &Path) -> bool {
    let mut command = Command::new("git");
    command.args(["status", "--porcelain"]).current_dir(root);
    clear_ambient_git_env(&mut command);
    let Ok(output) = command.output() else {
        return false;
    };
    if !output.status.success() {
        return false;
    }
    output.stdout.iter().any(|b| !b.is_ascii_whitespace())
}

// ── Config + discovery ───────────────────────────────────────────────

fn load_resolved_config(root: &Path) -> Result<ResolvedConfig, UploadError> {
    let user_config = match FallowConfig::find_and_load(root) {
        Ok(Some((config, _path))) => Some(config),
        Ok(None) => None,
        Err(e) => return Err(UploadError::Validation(format!("config load failed: {e}"))),
    };
    let config = user_config.unwrap_or_default();
    let threads = std::thread::available_parallelism().map_or(1, std::num::NonZero::get);
    Ok(config.resolve(
        root.to_path_buf(),
        fallow_config::OutputFormat::Human,
        threads,
        /* no_cache */ true,
        /* quiet */ true,
    ))
}

fn compile_exclude_matcher(patterns: &[String]) -> Result<GlobSet, UploadError> {
    let mut builder = GlobSetBuilder::new();
    for pattern in patterns {
        let glob = Glob::new(pattern).map_err(|e| {
            UploadError::Validation(format!("invalid --exclude-paths '{pattern}': {e}"))
        })?;
        builder.add(glob);
    }
    builder
        .build()
        .map_err(|e| UploadError::Validation(format!("failed to compile --exclude-paths: {e}")))
}

fn collect_inventory(
    config: &ResolvedConfig,
    exclude_matcher: &GlobSet,
    path_prefix: Option<&str>,
) -> Vec<InventoryFunction> {
    let files = fallow_core::discover::discover_files_with_plugin_scopes(config);
    let mut seen: FxHashSet<(String, String, u32)> = FxHashSet::default();
    let mut out: Vec<InventoryFunction> = Vec::new();
    for file in files {
        let rel = file
            .path
            .strip_prefix(&config.root)
            .map_or_else(|_| file.path.clone(), Path::to_path_buf);
        if !extension_supported(&rel) {
            continue;
        }
        if exclude_matcher_matches(exclude_matcher, &rel) {
            continue;
        }
        let source = match std::fs::read_to_string(&file.path) {
            Ok(content) => content,
            Err(err) => {
                eprintln!(
                    "{LOG_PREFIX}: {}: skipping {} (read failed: {err})",
                    "warning".yellow().bold(),
                    file.path.display(),
                );
                continue;
            }
        };
        let repo_relative = to_posix_string(&rel);
        let posix_path = match path_prefix {
            Some(prefix) => format!("{prefix}/{repo_relative}"),
            None => repo_relative,
        };
        for entry in walk_source(&file.path, &source) {
            let dedupe_key = (posix_path.clone(), entry.name.clone(), entry.line);
            if !seen.insert(dedupe_key) {
                continue;
            }
            out.push(InventoryFunction::from_entry(&posix_path, entry));
        }
    }
    out.sort_by(|a, b| {
        a.file_path
            .cmp(&b.file_path)
            .then(a.line_number.cmp(&b.line_number))
    });
    out
}

/// Validate and normalize the user-supplied `--path-prefix` value.
///
/// Accepts only POSIX-style absolute or rooted prefixes (`/app`,
/// `/workspace`, `/home/runner/work/my-repo/my-repo`, ...). Trailing slashes
/// are trimmed so the join `{prefix}/{repoRelative}` produces exactly one
/// separator. Empty strings and Windows backslashes are rejected so a
/// typo doesn't silently corrupt every uploaded path.
///
/// Returns `Ok(None)` when `raw` is `None` (flag not set). The walker then
/// emits repo-relative paths unchanged, matching the default for
/// non-container deployments (local dev, CI runners where the runtime
/// reports repo-relative paths).
fn normalize_path_prefix(raw: Option<&str>) -> Result<Option<String>, UploadError> {
    let Some(raw) = raw else { return Ok(None) };
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(UploadError::Validation(
            "--path-prefix is empty. Pass a POSIX path like `/app`, `/workspace`, or `/home/runner/work/<repo>/<repo>`, matching your runtime's WORKDIR.".to_owned(),
        ));
    }
    if trimmed.contains('\\') {
        return Err(UploadError::Validation(format!(
            "--path-prefix '{trimmed}' contains backslashes. Use POSIX separators (forward slashes) even on Windows, because the runtime beacon emits POSIX paths."
        )));
    }
    // Leading slash requirement: runtime paths are always absolute inside
    // containers (V8 reports `/app/src/*`, `/workspace/src/*`). A
    // relative-looking prefix (`app`) would silently join to
    // `app/src/foo.ts` and miss every runtime row. Keep the guard strict
    // so typos surface immediately.
    if !trimmed.starts_with('/') {
        return Err(UploadError::Validation(format!(
            "--path-prefix '{trimmed}' must start with '/'. Runtime paths are absolute inside containers; a relative prefix won't match. Example: --path-prefix /app"
        )));
    }
    Ok(Some(trimmed.trim_end_matches('/').to_owned()))
}

fn extension_supported(path: &Path) -> bool {
    // Skip TypeScript declaration files. Their "functions" are ambient type
    // signatures, not runtime code - including them would make every signature
    // appear as `untracked` in the dashboard.
    if is_typescript_declaration(path) {
        return false;
    }
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|ext| {
            SUPPORTED_EXTENSIONS
                .iter()
                .any(|s| s.eq_ignore_ascii_case(ext))
        })
}

fn is_typescript_declaration(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| {
            let lower = name.to_ascii_lowercase();
            lower.ends_with(".d.ts")
                || lower.ends_with(".d.mts")
                || lower.ends_with(".d.cts")
                || lower.ends_with(".d.tsx")
        })
}

fn exclude_matcher_matches(matcher: &GlobSet, rel_path: &Path) -> bool {
    if matcher.is_empty() {
        return false;
    }
    matcher.is_match(rel_path)
}

fn to_posix_string(path: &Path) -> String {
    // Windows walker paths carry `\` separators; the server and the beacon
    // both key on POSIX slashes, so normalize before sending.
    path.to_string_lossy().replace('\\', "/")
}

// ── Payload + HTTP ───────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
struct InventoryFunction {
    #[serde(rename = "filePath")]
    file_path: String,
    #[serde(rename = "functionName")]
    function_name: String,
    #[serde(rename = "lineNumber")]
    line_number: u32,
}

impl InventoryFunction {
    fn from_entry(posix_path: &str, entry: InventoryEntry) -> Self {
        Self {
            file_path: posix_path.to_owned(),
            function_name: entry.name,
            line_number: entry.line,
        }
    }
}

#[derive(Debug, Serialize)]
struct InventoryRequest<'a> {
    #[serde(rename = "gitSha")]
    git_sha: &'a str,
    functions: &'a [InventoryFunction],
}

#[derive(Debug, Deserialize)]
struct InventoryResponseData {
    id: String,
    #[serde(rename = "functionCount")]
    function_count: u64,
    #[serde(rename = "blobSize")]
    blob_size: u64,
    /// Server-computed overlap between the just-uploaded inventory and
    /// recent runtime coverage paths. Optional so older servers (before
    /// the `pathOverlap` field shipped) still deserialize cleanly.
    #[serde(rename = "pathOverlap", default)]
    path_overlap: Option<PathOverlap>,
}

#[derive(Debug, Deserialize)]
struct PathOverlap {
    sampled: u64,
    matched: u64,
    #[serde(rename = "exampleMismatch", default)]
    example_mismatch: Option<ExampleMismatch>,
}

#[derive(Debug, Deserialize)]
struct ExampleMismatch {
    #[serde(rename = "inventoryPath")]
    inventory_path: String,
    #[serde(rename = "runtimePath")]
    runtime_path: String,
}

#[derive(Debug, Deserialize)]
struct InventoryResponseEnvelope {
    data: InventoryResponseData,
}

fn resolve_api_key(args: &UploadInventoryArgs) -> Result<String, UploadError> {
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
        "/v1/coverage/{}/inventory",
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
/// segment under `/v1/coverage/{repo}/inventory`, so `/` must be encoded too.
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

/// Print a yellow warning when the server reports that the just-uploaded
/// inventory's paths don't meaningfully overlap with recent runtime paths
/// for the same SHA. Fires when matched * 2 < sampled (less than half the
/// runtime paths are present in the inventory), which is the signature of
/// a Dockerfile WORKDIR / CI-runner prefix mismatch. Silent below that
/// threshold: some overlap is expected on real projects as the beacon
/// rolls up lazy-parsed functions.
fn print_overlap_warning_if_needed(overlap: &PathOverlap) {
    if overlap.sampled == 0 {
        // No runtime data for this SHA yet. The success message already
        // tells the user to wait for the beacon; don't add noise here.
        return;
    }
    if overlap.matched.saturating_mul(2) >= overlap.sampled {
        return;
    }
    eprintln!(
        "{LOG_PREFIX}: {}: inventory paths don't overlap with runtime coverage for this SHA ({}/{} runtime paths matched).",
        "warning".yellow().bold(),
        overlap.matched,
        overlap.sampled,
    );
    if let Some(example) = overlap.example_mismatch.as_ref() {
        eprintln!("  runtime:   {}", example.runtime_path);
        eprintln!("  inventory: {}", example.inventory_path);
        eprintln!(
            "  -> If your app runs in a container, pass --path-prefix matching the deployed WORKDIR"
        );
        eprintln!("     (e.g. --path-prefix /app). Without a matching prefix, the dashboard's");
        eprintln!("     Untracked filter will fill with false positives.");
    } else {
        eprintln!(
            "  -> If your app runs in a container, pass --path-prefix matching the deployed WORKDIR (e.g. --path-prefix /app)."
        );
    }
}

fn upload(
    project_id: &str,
    endpoint_override: Option<&str>,
    api_key: &str,
    payload: &InventoryRequest<'_>,
) -> Result<(), UploadError> {
    let url = endpoint_url(endpoint_override, project_id);
    // Informational progress output goes to stdout alongside the dry-run
    // summary for symmetry. Only errors and warnings use stderr.
    println!(
        "{LOG_PREFIX}: uploading {} functions for {project_id} @ {}",
        format_count(payload.functions.len()),
        payload.git_sha,
    );

    let agent = api_agent_with_timeout(UPLOAD_CONNECT_TIMEOUT_SECS, UPLOAD_TOTAL_TIMEOUT_SECS);
    let mut response = agent
        .post(&url)
        .header("Authorization", &format!("Bearer {api_key}"))
        .send_json(payload)
        .map_err(|err| UploadError::Network(format!("network error: {err}")))?;

    let status = response.status().as_u16();
    if matches!(status, 200 | 201) {
        let data: InventoryResponseEnvelope = response
            .read_json()
            .map_err(|err| UploadError::ServerError(format!("malformed response body: {err}")))?;
        let func_count = usize::try_from(data.data.function_count).unwrap_or(usize::MAX);
        println!(
            "{LOG_PREFIX}: {} ({}) · {} functions · {} stored",
            "ok".green().bold(),
            data.data.id,
            format_count(func_count),
            format_bytes(data.data.blob_size),
        );
        // Intentional wording: the Untracked filter needs BOTH the static
        // inventory (this upload) AND runtime coverage from the beacon for
        // the same SHA. Users who upload first on a new SHA will see a
        // "waiting for runtime data" state; do not promise immediate results
        // or the first-run UX looks broken.
        println!(
            "  -> Inventory stored. The Untracked filter lights up once runtime coverage arrives for this SHA. Dashboard: https://fallow.cloud/{project_id}"
        );
        if let Some(overlap) = data.data.path_overlap.as_ref() {
            print_overlap_warning_if_needed(overlap);
        }
        return Ok(());
    }

    // Parse the body once so we can dispatch by the machine-readable `code`
    // field and also render a human-friendly message. We deliberately do NOT
    // route through `http_status_message`; it collapses code + message into
    // one formatted string, which forces callers to string-scan to classify.
    let body = response.read_to_string().unwrap_or_default();
    let envelope: ErrorEnvelope = serde_json::from_str(&body).unwrap_or_default();
    let code = envelope.code.as_deref();
    let message = format_upload_error_message(status, &body, code, envelope.message.as_deref());
    classify_upload_error(status, code, message)
}

fn format_upload_error_message(
    status: u16,
    body: &str,
    code: Option<&str>,
    message: Option<&str>,
) -> String {
    if let Some(code) = code
        && let Some(hint) = actionable_error_hint("upload-inventory", code)
    {
        return format!("{hint} (HTTP {status}, code {code})");
    }
    let body_suffix = match message {
        Some(m) if !m.trim().is_empty() => format!(": {}", m.trim()),
        _ if !body.trim().is_empty() => format!(": {}", body.trim()),
        _ => String::new(),
    };
    format!("upload-inventory request failed with HTTP {status}{body_suffix}")
}

fn classify_upload_error(
    status: u16,
    code: Option<&str>,
    message: String,
) -> Result<(), UploadError> {
    match (status, code) {
        (400, Some("payload_too_large")) => Err(UploadError::PayloadTooLarge(message)),
        (400, _) => Err(UploadError::Validation(message)),
        (401 | 403, _) => Err(UploadError::AuthRejected(message)),
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

/// Format a byte count in KiB / MiB / GiB for terminal output. Byte-exact
/// sizes are available in JSON output paths; humans get a readable form.
#[expect(
    clippy::cast_precision_loss,
    reason = "inventory blob sizes are well under f64 precision loss range"
)]
fn format_bytes(bytes: u64) -> String {
    const KIB: u64 = 1024;
    const MIB: u64 = KIB * 1024;
    const GIB: u64 = MIB * 1024;
    if bytes >= GIB {
        format!("{:.1} GiB", bytes as f64 / GIB as f64)
    } else if bytes >= MIB {
        format!("{:.1} MiB", bytes as f64 / MIB as f64)
    } else if bytes >= KIB {
        format!("{:.0} KiB", bytes as f64 / KIB as f64)
    } else {
        format!("{bytes} B")
    }
}

// ── Dry-run output ───────────────────────────────────────────────────

fn print_dry_run_summary(
    project_id: &str,
    git_sha: &str,
    path_prefix: Option<&str>,
    functions: &[InventoryFunction],
    endpoint_override: Option<&str>,
) {
    let decoded_url = display_endpoint_url(endpoint_override, project_id);
    println!("{LOG_PREFIX} {}", "(dry run)".bright_black());
    println!("  project-id:    {project_id}");
    println!("  git-sha:       {git_sha}");
    println!("  functions:     {}", format_count(functions.len()));
    if let Some(prefix) = path_prefix {
        println!("  path-prefix:   {prefix}");
    }
    println!("  endpoint:      {decoded_url}");
    println!();
    let shown = functions.len().min(5);
    let total = functions.len();
    println!("first {shown} of {} entries:", format_count(total));
    let width = functions
        .iter()
        .take(shown)
        .map(|e| e.file_path.len() + 1 + count_digits(e.line_number))
        .max()
        .unwrap_or(0);
    for entry in functions.iter().take(shown) {
        let location = format!("{}:{}", entry.file_path, entry.line_number);
        println!("  {location:<width$}  {}", entry.function_name);
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
    format!("{base}/v1/coverage/{project_id}/inventory")
}

fn count_digits(mut n: u32) -> usize {
    if n == 0 {
        return 1;
    }
    let mut d = 0;
    while n > 0 {
        d += 1;
        n /= 10;
    }
    d
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use tempfile::TempDir;

    #[test]
    fn parse_git_remote_https_with_dot_git() {
        assert_eq!(
            parse_git_remote_to_project_id("https://github.com/fallow-rs/fallow.git"),
            Some("fallow-rs/fallow".to_owned())
        );
    }

    #[test]
    fn parse_git_remote_https_without_dot_git() {
        assert_eq!(
            parse_git_remote_to_project_id("https://gitlab.com/acme/widgets"),
            Some("acme/widgets".to_owned())
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
    fn parse_git_remote_ssh_scheme_shape() {
        assert_eq!(
            parse_git_remote_to_project_id("ssh://git@github.com/fallow-rs/fallow.git"),
            Some("fallow-rs/fallow".to_owned())
        );
    }

    #[test]
    fn parse_git_remote_nested_group_uses_last_two_segments() {
        // GitLab supports nested groups. Auto-detection keeps the familiar
        // trailing `owner/repo` pair; repos that want the full namespace can
        // pass --project-id explicitly.
        assert_eq!(
            parse_git_remote_to_project_id("https://gitlab.com/acme/team/widgets.git"),
            Some("team/widgets".to_owned())
        );
    }

    #[test]
    fn parse_git_remote_rejects_single_segment() {
        assert_eq!(parse_git_remote_to_project_id("https://example.com/"), None);
        assert_eq!(parse_git_remote_to_project_id(""), None);
    }

    #[test]
    fn validate_project_id_accepts_owner_repo() {
        assert!(validate_project_id("fallow-rs/fallow").is_ok());
    }

    #[test]
    fn validate_project_id_accepts_bare_name() {
        // Dogfood projects use bare repo names (`fallow-cloud-api`), not
        // `owner/repo`. Both shapes are legitimate on the server side.
        assert!(validate_project_id("fallow-cloud-api").is_ok());
    }

    #[test]
    fn validate_project_id_rejects_path_traversal() {
        assert!(validate_project_id("../etc/passwd").is_err());
        assert!(validate_project_id("acme/../secret").is_err());
    }

    #[test]
    fn validate_project_id_rejects_empty() {
        assert!(validate_project_id("").is_err());
    }

    #[test]
    fn url_encode_path_segment_preserves_safe_chars() {
        assert_eq!(
            url_encode_path_segment("fallow-rs/fallow"),
            "fallow-rs%2Ffallow"
        );
    }

    #[test]
    fn url_encode_path_segment_handles_utf8() {
        assert_eq!(url_encode_path_segment("a b"), "a%20b");
    }

    #[test]
    fn endpoint_url_uses_override_when_provided() {
        let url = endpoint_url(Some("http://127.0.0.1:3000"), "a/b");
        assert_eq!(url, "http://127.0.0.1:3000/v1/coverage/a%2Fb/inventory");
    }

    #[test]
    fn endpoint_url_strips_override_trailing_slash() {
        let url = endpoint_url(Some("http://127.0.0.1:3000/"), "a/b");
        assert_eq!(url, "http://127.0.0.1:3000/v1/coverage/a%2Fb/inventory");
    }

    #[test]
    fn display_endpoint_url_uses_override_when_provided() {
        let url = display_endpoint_url(Some("http://127.0.0.1:3000/"), "a/b");
        assert_eq!(url, "http://127.0.0.1:3000/v1/coverage/a/b/inventory");
    }

    #[test]
    fn normalize_path_prefix_rejects_empty() {
        assert!(matches!(
            normalize_path_prefix(Some("")),
            Err(UploadError::Validation(_))
        ));
        assert!(matches!(
            normalize_path_prefix(Some("   ")),
            Err(UploadError::Validation(_))
        ));
    }

    #[test]
    fn normalize_path_prefix_rejects_backslash() {
        assert!(matches!(
            normalize_path_prefix(Some("\\app")),
            Err(UploadError::Validation(_))
        ));
        assert!(matches!(
            normalize_path_prefix(Some("/home\\runner")),
            Err(UploadError::Validation(_))
        ));
    }

    #[test]
    fn normalize_path_prefix_rejects_relative() {
        assert!(matches!(
            normalize_path_prefix(Some("app")),
            Err(UploadError::Validation(_))
        ));
        assert!(matches!(
            normalize_path_prefix(Some("./app")),
            Err(UploadError::Validation(_))
        ));
    }

    #[test]
    fn normalize_path_prefix_accepts_absolute_posix() {
        assert_eq!(
            normalize_path_prefix(Some("/app")).unwrap(),
            Some("/app".to_owned())
        );
        assert_eq!(
            normalize_path_prefix(Some("/home/runner/work/my-repo/my-repo")).unwrap(),
            Some("/home/runner/work/my-repo/my-repo".to_owned())
        );
    }

    #[test]
    fn normalize_path_prefix_trims_trailing_slash_and_whitespace() {
        assert_eq!(
            normalize_path_prefix(Some("/app/")).unwrap(),
            Some("/app".to_owned())
        );
        assert_eq!(
            normalize_path_prefix(Some("  /workspace/  ")).unwrap(),
            Some("/workspace".to_owned())
        );
    }

    #[test]
    fn normalize_path_prefix_none_stays_none() {
        assert_eq!(normalize_path_prefix(None).unwrap(), None);
    }

    #[test]
    fn format_count_groups_thousands() {
        assert_eq!(format_count(0), "0");
        assert_eq!(format_count(999), "999");
        assert_eq!(format_count(1_000), "1,000");
        assert_eq!(format_count(14_280), "14,280");
        assert_eq!(format_count(1_234_567), "1,234,567");
    }

    #[test]
    fn format_bytes_pivots_at_power_of_1024() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(1023), "1023 B");
        assert_eq!(format_bytes(1024), "1 KiB");
        assert_eq!(format_bytes(2048), "2 KiB");
        assert_eq!(format_bytes(1_048_576), "1.0 MiB");
        assert_eq!(format_bytes(10_485_760), "10.0 MiB");
        assert_eq!(format_bytes(1_073_741_824), "1.0 GiB");
    }

    #[test]
    fn count_digits_matches_base10_length() {
        assert_eq!(count_digits(0), 1);
        assert_eq!(count_digits(1), 1);
        assert_eq!(count_digits(9), 1);
        assert_eq!(count_digits(10), 2);
        assert_eq!(count_digits(99), 2);
        assert_eq!(count_digits(100), 3);
        assert_eq!(count_digits(9_999), 4);
    }

    #[test]
    fn extension_supported_handles_all_js_ts_variants() {
        for ext in ["js", "jsx", "mjs", "cjs", "ts", "tsx", "mts", "cts"] {
            let path = PathBuf::from(format!("a.{ext}"));
            assert!(extension_supported(&path), "missing support for .{ext}");
        }
    }

    #[test]
    fn extension_supported_rejects_non_js_ts() {
        for ext in ["md", "json", "css", "html", "vue", "svelte", "astro", "mdx"] {
            let path = PathBuf::from(format!("a.{ext}"));
            assert!(!extension_supported(&path), ".{ext} must be skipped in v1");
        }
    }

    #[test]
    fn extension_supported_skips_typescript_declaration_files() {
        for name in [
            "types.d.ts",
            "client.d.ts",
            "Index.D.TS",
            "lib.d.mts",
            "a.d.cts",
            "b.d.tsx",
        ] {
            let path = PathBuf::from(name);
            assert!(
                !extension_supported(&path),
                "{name} should be skipped as a declaration file"
            );
        }
    }

    #[test]
    fn extension_supported_still_accepts_non_declaration_ts() {
        // Regression guard: the .d.ts skip must not accidentally reject
        // files whose names contain ".d." but are not declarations.
        for name in ["vite.config.ts", "file.weird.d.name.ts"] {
            let path = PathBuf::from(name);
            assert!(extension_supported(&path), "{name} should still be walked");
        }
    }

    #[test]
    fn to_posix_string_normalizes_windows_separators() {
        let p = Path::new("src\\foo\\bar.ts");
        assert_eq!(to_posix_string(p), "src/foo/bar.ts");
    }

    #[test]
    fn classify_upload_error_maps_400_payload_too_large_to_dedicated_exit() {
        let err = classify_upload_error(400, Some("payload_too_large"), "stub".to_owned())
            .expect_err("400 must error");
        assert!(matches!(err, UploadError::PayloadTooLarge(_)));
    }

    #[test]
    fn classify_upload_error_falls_back_to_validation_on_other_400_codes() {
        let err = classify_upload_error(400, Some("bad_request"), "stub".to_owned())
            .expect_err("400 must error");
        assert!(matches!(err, UploadError::Validation(_)));
        let err = classify_upload_error(400, None, "stub".to_owned())
            .expect_err("400 with no code must error");
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
    fn ignore_upload_errors_does_not_soft_fail_auth_rejection() {
        let exit = UploadError::AuthRejected("bad key".to_owned()).into_exit(true);
        assert_eq!(
            format!("{exit:?}"),
            format!("{:?}", ExitCode::from(EXIT_AUTH_REJECTED))
        );
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
        let message = format_upload_error_message(400, "{}", Some("payload_too_large"), None);
        assert!(
            message.contains("200,000-function server limit"),
            "got: {message}"
        );
        assert!(message.contains("HTTP 400"));
        assert!(message.contains("code payload_too_large"));
    }

    #[test]
    fn format_upload_error_message_falls_back_to_server_message() {
        let message =
            format_upload_error_message(500, "{}", Some("internal"), Some("database timeout"));
        assert!(message.starts_with("upload-inventory request failed with HTTP 500"));
        assert!(message.ends_with(": database timeout"));
    }

    #[test]
    fn format_upload_error_message_handles_empty_body() {
        let message = format_upload_error_message(502, "", None, None);
        assert_eq!(message, "upload-inventory request failed with HTTP 502");
    }

    #[test]
    fn dirty_worktree_is_rejected_by_default() {
        let repo = create_dirty_git_repo();
        let err = enforce_clean_worktree(&UploadInventoryArgs::default(), repo.path())
            .expect_err("dirty repo should fail without --allow-dirty");
        let UploadError::Validation(message) = err else {
            panic!("expected validation error, got {err:?}");
        };
        assert!(message.contains("working tree has uncommitted changes"));
        assert!(message.contains("--allow-dirty"));
    }

    #[test]
    fn dirty_worktree_does_not_bypass_validation_with_explicit_git_sha() {
        let repo = create_dirty_git_repo();
        let args = UploadInventoryArgs {
            git_sha: Some("abc123".to_owned()),
            ..UploadInventoryArgs::default()
        };
        let err = enforce_clean_worktree(&args, repo.path())
            .expect_err("explicit git sha must not bypass dirty-tree validation");
        assert!(matches!(err, UploadError::Validation(_)));
    }

    #[test]
    fn dirty_worktree_is_allowed_with_explicit_opt_in() {
        let repo = create_dirty_git_repo();
        let args = UploadInventoryArgs {
            allow_dirty: true,
            ..UploadInventoryArgs::default()
        };
        assert!(enforce_clean_worktree(&args, repo.path()).is_ok());
    }

    fn create_dirty_git_repo() -> TempDir {
        let dir = tempfile::tempdir().expect("create temp repo");
        run_git(dir.path(), &["init", "-q"]);
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
}
