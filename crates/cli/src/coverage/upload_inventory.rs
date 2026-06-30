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
//! into their own rows. The walker behind `fallow_engine::walk_source_with_complexity`
//! emits Istanbul / `oxc-coverage-instrument`-compatible names and unique
//! 1-based line numbers per function declaration.
//!
//! This subcommand is a paid-tier workflow. It runs only when the user
//! invokes it explicitly; no other fallow command touches the network.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::{self, Write as _};
use std::path::Path;
use std::process::ExitCode;

use fallow_config::ResolvedConfig;
use fallow_cov_protocol::{FunctionIdentity, IdentityResolution, function_identity_id};
use fallow_engine::{
    ChurnResult, ChurnTrend, FileChurn, InventoryComplexity, InventoryEntry, analyze_churn_cached,
    discover_files_with_plugin_scopes, parse_since, walk_source_with_complexity,
};
use globset::{Glob, GlobSet, GlobSetBuilder};
use rustc_hash::{FxHashMap, FxHashSet};
use serde::{Deserialize, Serialize};

use colored::Colorize as _;

use crate::api::{
    NETWORK_EXIT_CODE, ParsedErrorEnvelope, ResponseBodyReader, actionable_error_hint, api_url,
    parse_error_envelope, response_message_suffix, sanitize_network_error,
    try_api_agent_with_timeout,
};
use crate::coverage::upload_common;

/// Log prefix used on every human-facing line from this subcommand.
/// Matches the pattern `fallow license:` / `fallow coverage setup:` established
/// by sibling commands so CI log parsers can anchor on it.
const LOG_PREFIX: &str = "fallow coverage upload-inventory";

/// Client-side mirror of the server cap on inventory size. Validated here so
/// users see a specific error before a 400 round-trip.
const INVENTORY_MAX_FUNCTIONS: usize = 200_000;

/// Wire version of the inventory upload payload. Bumped from the implicit v1
/// (no version field, identity-only functions) to v2 the moment the payload
/// began carrying per-function complexity and a per-file churn map. The field
/// is informational and additive: the server reads the new fields by presence,
/// not by branching on the version, so a v1-shaped body (no version, no new
/// fields) still validates. Older servers ignore the unknown `version` and the
/// extra fields, so a newer CLI keeps uploading successfully.
const INVENTORY_BLOB_VERSION: u8 = 2;

/// Wire version once the payload also carries the importer-edge map (the
/// `--with-callers` opt-in). Same additive contract: the server reads
/// `callerEdges` by presence, so an older server ignores both the bumped version
/// and the field. Only emitted when caller edges are actually present.
const INVENTORY_BLOB_VERSION_WITH_CALLERS: u8 = 3;

/// Cap on importer sites recorded per function. Mirrors the server's per-callee
/// cap so a pathological fan-in (a barrel re-exported everywhere) cannot bloat
/// the payload or trip the server bound. Truncation is deterministic: sites are
/// ordered by importer path before the cut.
const MAX_CALLER_SITES_PER_FN: usize = 500;

/// Cap on imported symbol names recorded per importer site. Mirrors the server's
/// per-site bound. The attribution records exactly one symbol per site today
/// (the callee's own name), so this only guards against a future change.
const MAX_SYMBOLS_PER_CALLER_SITE: usize = 64;

/// Git-history window used to compute the per-file churn shipped alongside the
/// inventory. Matches the default window the local hotspot analysis uses, so
/// the uploaded churn lines up with what `fallow` reports locally. The signal
/// is recency-weighted (90-day half-life), so a longer window mostly adds
/// near-zero-weight history; six months captures the active hot files without
/// dragging in stale churn.
const CHURN_SINCE: &str = "6m";

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
#[derive(Clone, Default)]
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
    /// Also build the import graph and upload importer edges (which files import
    /// each function) alongside the inventory. Opt-in because it runs the full
    /// static analysis to build the graph, whereas the default upload is a fast
    /// per-file walk. The graph is cached, so a CI step that already ran the
    /// analysis pays little extra.
    pub with_callers: bool,
}

impl fmt::Debug for UploadInventoryArgs {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("UploadInventoryArgs")
            .field("api_key", &self.api_key.as_ref().map(|_| "***"))
            .field("api_endpoint", &self.api_endpoint)
            .field("project_id", &self.project_id)
            .field("git_sha", &self.git_sha)
            .field("allow_dirty", &self.allow_dirty)
            .field("exclude_paths", &self.exclude_paths)
            .field("path_prefix", &self.path_prefix)
            .field("dry_run", &self.dry_run)
            .field("ignore_upload_errors", &self.ignore_upload_errors)
            .field("with_callers", &self.with_callers)
            .finish()
    }
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
    let churn_by_path = collect_churn(&config, path_prefix.as_deref());

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

    // Importer edges are opt-in: building them runs the full static analysis to
    // get the import graph, while the default upload is a fast per-file walk.
    // Best-effort, so a graph-build failure still ships the inventory.
    let caller_edges = if args.with_callers {
        collect_caller_edges(&config, &functions)
    } else {
        BTreeMap::new()
    };
    let version = if caller_edges.is_empty() {
        INVENTORY_BLOB_VERSION
    } else {
        INVENTORY_BLOB_VERSION_WITH_CALLERS
    };

    let payload = InventoryRequest {
        version,
        git_sha: &git_sha,
        functions: &functions,
        churn_by_path,
        caller_edges,
    };

    if args.dry_run {
        print_dry_run_summary(
            &project_id,
            &git_sha,
            path_prefix.as_deref(),
            &functions,
            args.api_endpoint.as_deref(),
        );
        if args.with_callers {
            println!(
                "{LOG_PREFIX}: caller edges resolved for {} functions",
                format_count(payload.caller_edges.len()),
            );
        }
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

fn resolve_project_id(args: &UploadInventoryArgs, root: &Path) -> Result<String, UploadError> {
    upload_common::resolve_project_id(args.project_id.as_deref(), root)
        .map_err(UploadError::Validation)
}

fn resolve_git_sha(args: &UploadInventoryArgs, root: &Path) -> Result<String, UploadError> {
    upload_common::resolve_git_sha(args.git_sha.as_deref(), root).map_err(UploadError::Validation)
}

fn enforce_clean_worktree(args: &UploadInventoryArgs, root: &Path) -> Result<(), UploadError> {
    if args.dry_run {
        return Ok(());
    }
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
    upload_common::dirty_worktree(root)
}

fn load_resolved_config(root: &Path) -> Result<ResolvedConfig, UploadError> {
    upload_common::load_resolved_config(root).map_err(UploadError::Validation)
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
    let files = discover_files_with_plugin_scopes(config);
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
            None => repo_relative.clone(),
        };
        let (entries, complexity) = walk_source_with_complexity(&file.path, &source);
        for entry in entries {
            let dedupe_key = (posix_path.clone(), entry.name.clone(), entry.line);
            if !seen.insert(dedupe_key) {
                continue;
            }
            // Complexity is paired by `source_hash`, which both the inventory
            // walker and the complexity visitor derive from the identical
            // full-span slice over the same parse, so the lookup is exact.
            let metrics = complexity.get(&entry.source_hash).copied();
            out.push(InventoryFunction::from_entry(
                &posix_path,
                &repo_relative,
                entry,
                metrics,
            ));
        }
    }
    out.sort_by(|a, b| {
        a.file_path
            .cmp(&b.file_path)
            .then(a.line_number.cmp(&b.line_number))
    });
    out
}

/// Compute per-file git churn for the walked tree and key it by the SAME
/// `filePath` shape [`collect_inventory`] emits (repo-relative posix, with
/// `--path-prefix` applied), so the server can join a runtime file path to its
/// churn without any path-shape guessing.
///
/// Churn is best-effort context: a non-git root, a shallow clone, or any git
/// failure yields an empty map and never an error. The walked tree's ignore
/// rules don't apply to git history, so the result is filtered to files that
/// actually live under the project root and survive a posix normalization.
fn collect_churn(
    config: &ResolvedConfig,
    path_prefix: Option<&str>,
) -> BTreeMap<String, FileChurnPayload> {
    let Ok(since) = parse_since(CHURN_SINCE) else {
        return BTreeMap::new();
    };
    let Some((result, _cache_hit)) =
        analyze_churn_cached(&config.root, &since, &config.cache_dir, config.no_cache)
    else {
        return BTreeMap::new();
    };
    churn_to_payload(&config.root, &result, path_prefix)
}

/// Map an absolute-path-keyed [`ChurnResult`] to a posix-path-keyed payload map.
/// Files outside the project root are dropped (they can't join a runtime path
/// that is reported relative to the deployed tree).
fn churn_to_payload(
    root: &Path,
    result: &ChurnResult,
    path_prefix: Option<&str>,
) -> BTreeMap<String, FileChurnPayload> {
    let mut out: BTreeMap<String, FileChurnPayload> = BTreeMap::new();
    for file in result.files.values() {
        let Ok(rel) = file.path.strip_prefix(root) else {
            continue;
        };
        let repo_relative = to_posix_string(rel);
        if repo_relative.is_empty() {
            continue;
        }
        let key = match path_prefix {
            Some(prefix) => format!("{prefix}/{repo_relative}"),
            None => repo_relative,
        };
        out.insert(key, FileChurnPayload::from_file_churn(file));
    }
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
    if !trimmed.starts_with('/') {
        return Err(UploadError::Validation(format!(
            "--path-prefix '{trimmed}' must start with '/'. Runtime paths are absolute inside containers; a relative prefix won't match. Example: --path-prefix /app"
        )));
    }
    Ok(Some(trimmed.trim_end_matches('/').to_owned()))
}

fn extension_supported(path: &Path) -> bool {
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
    path.to_string_lossy().replace('\\', "/")
}

#[derive(Debug, Clone, Serialize)]
struct InventoryFunction {
    #[serde(rename = "filePath")]
    file_path: String,
    #[serde(rename = "functionName")]
    function_name: String,
    #[serde(rename = "lineNumber")]
    line_number: u32,
    /// Cross-surface `FunctionIdentity` v2 (protocol 0.6+), carried alongside
    /// the legacy `(filePath, functionName, lineNumber)` join key so the cloud
    /// can migrate to the stable-id join without a CLI change.
    ///
    /// Computed over the REPO-RELATIVE path, NOT the `--path-prefix`-prefixed
    /// `filePath`: the protocol contract is `FunctionIdentity.file` is relative
    /// to the project root, and fallow's own consumer (`coverage analyze`)
    /// computes its static index over the repo-relative path. If the identity
    /// hashed the prefixed path, the producer and consumer `stable_id` values
    /// would diverge and the join would silently break. `--path-prefix` only
    /// affects the legacy `filePath`, never the identity hash.
    identity: FunctionIdentity,
    /// `McCabe` cyclomatic complexity (1 + decision points). Optional so a
    /// function whose span slice could not be paired to a complexity result,
    /// and any future producer that skips complexity, simply omits the field.
    /// Descriptive context for downstream importance weighting; never a gate.
    #[serde(rename = "cyclomatic", skip_serializing_if = "Option::is_none")]
    cyclomatic: Option<u16>,
    /// `SonarSource` cognitive complexity (structural + nesting penalty).
    /// Optional with the same omit-when-absent semantics as `cyclomatic`.
    #[serde(rename = "cognitive", skip_serializing_if = "Option::is_none")]
    cognitive: Option<u16>,
}

impl InventoryFunction {
    fn from_entry(
        posix_path: &str,
        repo_relative: &str,
        entry: InventoryEntry,
        complexity: Option<InventoryComplexity>,
    ) -> Self {
        let stable_id = function_identity_id(repo_relative, &entry.name, entry.line);
        let identity = FunctionIdentity {
            file: repo_relative.to_owned(),
            name: entry.name.clone(),
            start_line: entry.line,
            start_column: Some(entry.start_column),
            end_line: Some(entry.end_line),
            end_column: Some(entry.end_column),
            source_hash: Some(entry.source_hash.clone()),
            resolution: IdentityResolution::Resolved,
            stable_id,
        };
        Self {
            file_path: posix_path.to_owned(),
            function_name: entry.name,
            line_number: entry.line,
            identity,
            cyclomatic: complexity.map(|c| c.cyclomatic),
            cognitive: complexity.map(|c| c.cognitive),
        }
    }
}

#[derive(Debug, Serialize)]
struct InventoryRequest<'a> {
    version: u8,
    #[serde(rename = "gitSha")]
    git_sha: &'a str,
    functions: &'a [InventoryFunction],
    /// Per-file git churn keyed by the same `filePath` shape `functions` use
    /// (repo-relative posix, `--path-prefix` applied). Skipped entirely when
    /// empty (non-git root, shallow clone, or git failure) so older servers and
    /// non-git uploads keep the exact pre-enrichment wire shape.
    #[serde(rename = "churnByPath", skip_serializing_if = "BTreeMap::is_empty")]
    churn_by_path: BTreeMap<String, FileChurnPayload>,
    /// Importer edges keyed by the callee export's `stable_id` (the SAME value
    /// `functions[].identity.stable_id` carries, so the server joins
    /// stable_id == stable_id). Each entry lists the files that import the
    /// function and the symbol names they import. Import-edge granularity, not a
    /// file:line call-site. Skipped entirely when empty (the default upload, or a
    /// graph build that produced no edges) so the pre-enrichment wire shape is
    /// unchanged. Context only; never a verdict input.
    #[serde(rename = "callerEdges", skip_serializing_if = "BTreeMap::is_empty")]
    caller_edges: BTreeMap<String, Vec<CallerSitePayload>>,
}

/// Wire form of one importer edge: a `file` that imports the callee plus the
/// `symbols` it imports. Repo-relative posix `file` (the `--path-prefix` shape
/// is irrelevant here: the join is by stable_id, and the importer path is
/// context for a human/agent). `symbols` omitted when empty.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
struct CallerSitePayload {
    file: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    symbols: Vec<String>,
}

/// A raw importer edge discovered from the module graph: `importer_file` imports
/// `symbol` from `callee_file`. All paths are repo-relative posix. Intermediate
/// shape so the symbol-name -> function attribution stays a pure, testable step
/// independent of the graph walk.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ImporterEdge {
    callee_file: String,
    importer_file: String,
    symbol: String,
}

/// Attribute raw importer edges to callee functions by matching each imported
/// symbol name to an inventory function of the same name in the callee file,
/// producing the `callerEdges` map keyed by callee `stable_id`.
///
/// Import-edge granularity, best-effort: `default` / `*` / side-effect imports
/// and any symbol that does not name an inventory function simply contribute
/// nothing (no false attribution). A name can repeat in a file (overloads,
/// same-named nested functions), so an edge attributes to every matching
/// stable_id. Pure: no graph or IO, so it is unit-tested directly.
fn attribute_caller_edges(
    functions: &[InventoryFunction],
    edges: &[ImporterEdge],
) -> BTreeMap<String, Vec<CallerSitePayload>> {
    // (callee repo-relative file, function name) -> stable_ids of functions with
    // that name in that file. identity.file is repo-relative, matching the edge
    // callee_file shape.
    let mut by_name: FxHashMap<(&str, &str), Vec<&str>> = FxHashMap::default();
    for func in functions {
        by_name
            .entry((func.identity.file.as_str(), func.function_name.as_str()))
            .or_default()
            .push(func.identity.stable_id.as_str());
    }

    // callee stable_id -> importer file -> imported symbols (BTree for dedup +
    // deterministic ordering of both sites and symbols).
    let mut acc: BTreeMap<String, BTreeMap<String, BTreeSet<String>>> = BTreeMap::new();
    for edge in edges {
        let Some(stable_ids) = by_name.get(&(edge.callee_file.as_str(), edge.symbol.as_str()))
        else {
            continue;
        };
        for stable_id in stable_ids {
            acc.entry((*stable_id).to_owned())
                .or_default()
                .entry(edge.importer_file.clone())
                .or_default()
                .insert(edge.symbol.clone());
        }
    }

    acc.into_iter()
        .map(|(stable_id, importers)| {
            let mut sites: Vec<CallerSitePayload> = importers
                .into_iter()
                .map(|(file, symbols)| {
                    // In practice exactly one symbol (the function's own name,
                    // since the match condition is `symbol == function_name`).
                    // The cap mirrors the server's per-site bound so a future
                    // attribution change (e.g. alias matching) can never produce
                    // a payload the server rejects.
                    let mut symbols: Vec<String> = symbols.into_iter().collect();
                    symbols.truncate(MAX_SYMBOLS_PER_CALLER_SITE);
                    CallerSitePayload { file, symbols }
                })
                .collect();
            sites.truncate(MAX_CALLER_SITES_PER_FN);
            (stable_id, sites)
        })
        .collect()
}

/// Build the importer-edge map for the inventory by running the static analysis
/// (graph retained) and attributing each import edge to a callee function.
///
/// Best-effort context: any failure (analysis error, no graph/files retained)
/// yields an empty map so the upload still ships the inventory. Never a verdict
/// input on the server side.
fn collect_caller_edges(
    config: &ResolvedConfig,
    functions: &[InventoryFunction],
) -> BTreeMap<String, Vec<CallerSitePayload>> {
    let artifacts = match fallow_engine::analyze_retaining_modules(config, false, true) {
        Ok(artifacts) => artifacts,
        Err(err) => {
            eprintln!(
                "{LOG_PREFIX}: {}: import graph build failed, uploading without caller edges ({err})",
                "warning".yellow().bold(),
            );
            return BTreeMap::new();
        }
    };
    let (Some(graph), Some(files)) = (artifacts.graph.as_ref(), artifacts.files.as_ref()) else {
        return BTreeMap::new();
    };

    // FileId -> repo-relative posix path, matching collect_inventory's shape so
    // the callee/importer paths join the inventory functions' identity.file.
    let mut repo_relative_by_id = FxHashMap::default();
    for file in files {
        let rel = file
            .path
            .strip_prefix(&config.root)
            .map_or_else(|_| file.path.clone(), Path::to_path_buf);
        repo_relative_by_id.insert(file.id, to_posix_string(&rel));
    }

    let mut edges: Vec<ImporterEdge> = Vec::new();
    for file in files {
        let Some(callee_file) = repo_relative_by_id.get(&file.id) else {
            continue;
        };
        for summary in graph.direct_importer_summaries(file.id) {
            let Some(importer_file) = repo_relative_by_id.get(&summary.source) else {
                continue;
            };
            for symbol in &summary.symbols {
                // Type-only imports cannot exercise a function at runtime; skip
                // them so blast-radius reflects value-level dependents only.
                if symbol.type_only {
                    continue;
                }
                edges.push(ImporterEdge {
                    callee_file: callee_file.clone(),
                    importer_file: importer_file.clone(),
                    symbol: symbol.imported.clone(),
                });
            }
        }
    }

    attribute_caller_edges(functions, &edges)
}

/// Wire form of a single file's churn. All fields optional so a future
/// producer can ship a subset and so the server treats each independently as
/// best-effort context. `trend` serializes as the same snake-case strings the
/// rest of fallow uses (`accelerating` / `stable` / `cooling`).
#[derive(Debug, Serialize)]
struct FileChurnPayload {
    #[serde(skip_serializing_if = "Option::is_none")]
    commits: Option<u32>,
    #[serde(rename = "weightedCommits", skip_serializing_if = "Option::is_none")]
    weighted_commits: Option<f64>,
    #[serde(rename = "linesAdded", skip_serializing_if = "Option::is_none")]
    lines_added: Option<u32>,
    #[serde(rename = "linesDeleted", skip_serializing_if = "Option::is_none")]
    lines_deleted: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    trend: Option<&'static str>,
    /// Distinct-author count for this file (number of authors touching it in the
    /// window). This is NOT an ownership signal: ownership is defined by
    /// CODEOWNERS and resolved separately. It is churn context only.
    #[serde(rename = "authorCount", skip_serializing_if = "Option::is_none")]
    author_count: Option<u32>,
    /// Most recent commit timestamp touching this file, epoch SECONDS, derived
    /// as the max over the file's per-author last-commit timestamps.
    #[serde(rename = "lastCommitTs", skip_serializing_if = "Option::is_none")]
    last_commit_ts: Option<u64>,
}

impl FileChurnPayload {
    fn from_file_churn(file: &FileChurn) -> Self {
        let trend = match file.trend {
            ChurnTrend::Accelerating => "accelerating",
            ChurnTrend::Stable => "stable",
            ChurnTrend::Cooling => "cooling",
        };
        let author_count = u32::try_from(file.authors.len()).ok();
        let last_commit_ts = file
            .authors
            .values()
            .map(|author| author.last_commit_ts)
            .max();
        Self {
            commits: Some(file.commits),
            weighted_commits: Some(file.weighted_commits),
            lines_added: Some(file.lines_added),
            lines_deleted: Some(file.lines_deleted),
            trend: Some(trend),
            author_count,
            last_commit_ts,
        }
    }
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

/// Print a yellow warning when the server reports that the just-uploaded
/// inventory's paths don't meaningfully overlap with recent runtime paths
/// for the same SHA. Fires when matched * 2 < sampled (less than half the
/// runtime paths are present in the inventory), which is the signature of
/// a Dockerfile WORKDIR / CI-runner prefix mismatch. Silent below that
/// threshold: some overlap is expected on real projects as the beacon
/// rolls up lazy-parsed functions.
fn print_overlap_warning_if_needed(overlap: &PathOverlap) {
    if overlap.sampled == 0 {
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
    println!(
        "{LOG_PREFIX}: uploading {} functions for {project_id} @ {}",
        format_count(payload.functions.len()),
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
        println!(
            "  -> Inventory stored. The Untracked filter lights up once runtime coverage arrives for this SHA. Dashboard: https://fallow.cloud/{project_id}"
        );
        if let Some(overlap) = data.data.path_overlap.as_ref() {
            print_overlap_warning_if_needed(overlap);
        }
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
        && let Some(hint) = actionable_error_hint("upload-inventory", code)
    {
        return format!("{hint} (HTTP {status}, code {code})");
    }
    let body_suffix = response_message_suffix(body, envelope);
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
    use crate::coverage::upload_common::{
        GIT_SHA_MAX_LEN, parse_git_remote_to_project_id, validate_project_id,
    };
    use std::path::PathBuf;
    use std::process::Command;
    use tempfile::TempDir;

    #[test]
    fn upload_inventory_args_debug_masks_api_key() {
        let args = UploadInventoryArgs {
            api_key: Some("fallow_live_secret_token_value".to_owned()),
            api_endpoint: Some("https://api.fallow.cloud".to_owned()),
            project_id: Some("acme/web".to_owned()),
            ..UploadInventoryArgs::default()
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
        let bare = UploadInventoryArgs::default();
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
    fn resolve_api_key_trims_explicit_value() {
        let args = UploadInventoryArgs {
            api_key: Some("  fallow_key_123  ".to_owned()),
            ..UploadInventoryArgs::default()
        };

        assert_eq!(resolve_api_key(&args).unwrap(), "fallow_key_123");
    }

    #[test]
    fn compile_exclude_matcher_rejects_invalid_glob() {
        let err = compile_exclude_matcher(&["[".to_owned()])
            .expect_err("invalid glob should be reported as validation");

        let UploadError::Validation(message) = err else {
            panic!("expected validation error, got {err:?}");
        };
        assert!(message.contains("invalid --exclude-paths"));
    }

    #[test]
    fn collect_inventory_applies_path_prefix_and_excludes() {
        let project = project_with_one_function();
        let config = load_resolved_config(project.path()).unwrap();
        let include_all = compile_exclude_matcher(&[]).unwrap();

        let functions = collect_inventory(&config, &include_all, Some("/app"));

        assert_eq!(functions.len(), 1);
        assert_eq!(functions[0].file_path, "/app/src/index.ts");
        assert_eq!(functions[0].identity.file, "src/index.ts");

        let exclude_src = compile_exclude_matcher(&["src/**".to_owned()]).unwrap();
        assert!(collect_inventory(&config, &exclude_src, Some("/app")).is_empty());
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
        let envelope = parse_error_envelope(r#"{"code":"payload_too_large"}"#);
        let message = format_upload_error_message(400, "{}", Some("payload_too_large"), &envelope);
        assert!(
            message.contains("200,000-function server limit"),
            "got: {message}"
        );
        assert!(message.contains("HTTP 400"));
        assert!(message.contains("code payload_too_large"));
    }

    #[test]
    fn format_upload_error_message_falls_back_to_server_message() {
        let body = r#"{"code":"internal","message":"database timeout"}"#;
        let envelope = parse_error_envelope(body);
        let message = format_upload_error_message(500, body, Some("internal"), &envelope);
        assert!(message.starts_with("upload-inventory request failed with HTTP 500"));
        assert!(message.ends_with(": database timeout"));
    }

    #[test]
    fn format_upload_error_message_handles_empty_body() {
        let envelope = parse_error_envelope("");
        let message = format_upload_error_message(502, "", None, &envelope);
        assert_eq!(message, "upload-inventory request failed with HTTP 502");
    }

    #[test]
    fn format_upload_error_message_preserves_malformed_body() {
        let body = "gateway timeout";
        let envelope = parse_error_envelope(body);
        let message = format_upload_error_message(500, body, None, &envelope);
        assert!(message.contains("gateway timeout"));
        assert!(message.contains("malformed error envelope"));
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
    fn dry_run_skips_dirty_worktree_validation() {
        let repo = create_dirty_git_repo();
        let args = UploadInventoryArgs {
            dry_run: true,
            ..UploadInventoryArgs::default()
        };
        assert!(enforce_clean_worktree(&args, repo.path()).is_ok());
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

    fn sample_entry() -> InventoryEntry {
        InventoryEntry {
            name: "render".to_owned(),
            line: 42,
            start_column: 1,
            end_line: 50,
            end_column: 2,
            source_hash: "0123456789abcdef".to_owned(),
        }
    }

    #[test]
    fn identity_stable_id_is_repo_relative_not_prefixed() {
        let func = InventoryFunction::from_entry(
            "/app/src/render.tsx",
            "src/render.tsx",
            sample_entry(),
            None,
        );
        assert_eq!(func.file_path, "/app/src/render.tsx");
        assert_eq!(func.identity.file, "src/render.tsx");
        assert_eq!(
            func.identity.stable_id,
            function_identity_id("src/render.tsx", "render", 42)
        );
    }

    #[test]
    fn identity_stable_id_unchanged_by_path_prefix() {
        let with_prefix = InventoryFunction::from_entry(
            "/app/src/render.tsx",
            "src/render.tsx",
            sample_entry(),
            None,
        );
        let without_prefix =
            InventoryFunction::from_entry("src/render.tsx", "src/render.tsx", sample_entry(), None);
        assert_ne!(with_prefix.file_path, without_prefix.file_path);
        assert_eq!(
            with_prefix.identity.stable_id,
            without_prefix.identity.stable_id
        );
        assert_eq!(with_prefix.identity.file, without_prefix.identity.file);
    }

    #[test]
    fn identity_matches_protocol_conformance_fixture() {
        assert_eq!(
            function_identity_id("src/render.tsx", "render", 42),
            "fallow:fn:cb4482d6aef7c79a"
        );
    }

    #[test]
    fn inventory_function_emits_resolved_columns() {
        let func = InventoryFunction::from_entry("src/a.ts", "src/a.ts", sample_entry(), None);
        assert_eq!(func.identity.resolution, IdentityResolution::Resolved);
        assert_eq!(func.identity.start_column, Some(1));
        assert_eq!(func.identity.end_line, Some(50));
        assert_eq!(func.identity.end_column, Some(2));
        assert_eq!(
            func.identity.source_hash.as_deref(),
            Some("0123456789abcdef")
        );
    }

    #[test]
    fn inventory_function_carries_complexity_when_paired() {
        let metrics = InventoryComplexity {
            cyclomatic: 7,
            cognitive: 4,
        };
        let func =
            InventoryFunction::from_entry("src/a.ts", "src/a.ts", sample_entry(), Some(metrics));
        assert_eq!(func.cyclomatic, Some(7));
        assert_eq!(func.cognitive, Some(4));
    }

    #[test]
    fn inventory_function_omits_complexity_when_unpaired() {
        let func = InventoryFunction::from_entry("src/a.ts", "src/a.ts", sample_entry(), None);
        assert_eq!(func.cyclomatic, None);
        assert_eq!(func.cognitive, None);
        // Optional + skip-if-none means the field is absent from the wire form,
        // so an older server and a non-complexity producer stay byte-compatible.
        let json = serde_json::to_string(&func).expect("serialize");
        assert!(
            !json.contains("cyclomatic"),
            "unpaired complexity must not appear on the wire: {json}"
        );
    }

    #[test]
    fn complexity_populates_from_real_walk() {
        // Drive the actual walk+complexity pairing over a branchy function so a
        // regression in the source_hash join surfaces as a missing metric.
        let project = project_with_branchy_function();
        let config = load_resolved_config(project.path()).unwrap();
        let include_all = compile_exclude_matcher(&[]).unwrap();
        let functions = collect_inventory(&config, &include_all, None);
        let branchy = functions
            .iter()
            .find(|f| f.function_name == "branchy")
            .expect("branchy function present");
        let cyclomatic = branchy.cyclomatic.expect("cyclomatic populated from walk");
        assert!(
            cyclomatic >= 3,
            "branchy has multiple decision points, got cyclomatic {cyclomatic}"
        );
    }

    fn project_with_one_function() -> TempDir {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("package.json"), r#"{"name":"inv"}"#).unwrap();
        std::fs::write(
            root.join("src/index.ts"),
            "export function boot() {\n  return 1;\n}\n",
        )
        .unwrap();
        dir
    }

    fn project_with_branchy_function() -> TempDir {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("package.json"), r#"{"name":"inv"}"#).unwrap();
        std::fs::write(
            root.join("src/index.ts"),
            "export function branchy(x: number) {\n  if (x > 0) {\n    return 1;\n  } else if (x < 0) {\n    return -1;\n  }\n  return 0;\n}\n",
        )
        .unwrap();
        dir
    }

    /// Build a committed single-file git repo so `analyze_churn_cached` has real
    /// history to read. Returns the temp dir; the committed file is `src/a.ts`.
    fn git_repo_with_history() -> TempDir {
        let dir = tempfile::tempdir().expect("create temp repo");
        let root = dir.path();
        run_git(root, &["init", "-q"]);
        run_git(root, &["config", "commit.gpgsign", "false"]);
        run_git(root, &["config", "user.email", "review@example.com"]);
        run_git(root, &["config", "user.name", "Reviewer"]);
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("package.json"), r#"{"name":"inv"}"#).unwrap();
        std::fs::write(
            root.join("src/a.ts"),
            "export function one() {\n  return 1;\n}\n",
        )
        .unwrap();
        run_git(root, &["add", "."]);
        run_git(root, &["commit", "-qm", "first"]);
        std::fs::write(
            root.join("src/a.ts"),
            "export function one() {\n  return 2;\n}\n",
        )
        .unwrap();
        run_git(root, &["add", "."]);
        run_git(root, &["commit", "-qm", "second"]);
        dir
    }

    #[test]
    fn churn_by_path_keys_match_prefixed_file_path() {
        let repo = git_repo_with_history();
        let config = load_resolved_config(repo.path()).unwrap();
        let churn = collect_churn(&config, Some("/app"));
        // A committed, twice-edited file must appear keyed by the SAME prefixed
        // posix path the inventory functions carry, so the server can join them.
        let entry = churn
            .get("/app/src/a.ts")
            .expect("committed file present in churn map keyed by prefixed path");
        assert!(
            entry.commits.unwrap_or(0) >= 2,
            "two commits touched the file"
        );
        assert!(entry.author_count.unwrap_or(0) >= 1, "one author present");
        assert!(entry.last_commit_ts.is_some(), "recency timestamp present");
        assert!(entry.trend.is_some(), "trend present");
        // The prefixed inventory path and the churn key must use the same shape.
        let include_all = compile_exclude_matcher(&[]).unwrap();
        let functions = collect_inventory(&config, &include_all, Some("/app"));
        let a_fn = functions
            .iter()
            .find(|f| f.file_path == "/app/src/a.ts")
            .expect("inventory function for src/a.ts");
        assert!(
            churn.contains_key(&a_fn.file_path),
            "inventory filePath {} must be a churn key",
            a_fn.file_path
        );
    }

    #[test]
    fn churn_is_empty_and_errorless_on_non_git_root() {
        // No `git init`: a plain directory must yield an empty churn map and no
        // error, so the enrichment degrades gracefully off a version-control
        // host.
        let project = project_with_one_function();
        let config = load_resolved_config(project.path()).unwrap();
        let churn = collect_churn(&config, None);
        assert!(churn.is_empty(), "non-git root must produce empty churn");
    }

    #[test]
    fn request_serializes_v2_version_and_churn_when_present() {
        let repo = git_repo_with_history();
        let config = load_resolved_config(repo.path()).unwrap();
        let include_all = compile_exclude_matcher(&[]).unwrap();
        let functions = collect_inventory(&config, &include_all, None);
        let churn_by_path = collect_churn(&config, None);
        let request = InventoryRequest {
            version: INVENTORY_BLOB_VERSION,
            git_sha: "deadbeef",
            functions: &functions,
            churn_by_path,
            caller_edges: BTreeMap::new(),
        };
        let json = serde_json::to_value(&request).expect("serialize request");
        assert_eq!(json["version"], 2, "v2 version field present on the wire");
        assert_eq!(json["gitSha"], "deadbeef");
        assert!(
            json["churnByPath"].is_object(),
            "churnByPath present when git history exists"
        );
        let file_churn = &json["churnByPath"]["src/a.ts"];
        assert!(file_churn["commits"].is_number(), "commits emitted");
        assert!(
            file_churn["weightedCommits"].is_number(),
            "weightedCommits emitted"
        );
        assert!(file_churn["authorCount"].is_number(), "authorCount emitted");
        assert!(
            file_churn["lastCommitTs"].is_number(),
            "lastCommitTs emitted"
        );
        assert!(file_churn["trend"].is_string(), "trend emitted as a string");
    }

    #[test]
    fn request_omits_churn_when_empty() {
        // A v2 request built off a non-git root must not emit `churnByPath` at
        // all, keeping the pre-enrichment wire shape for non-git uploads.
        let functions: Vec<InventoryFunction> = Vec::new();
        let request = InventoryRequest {
            version: INVENTORY_BLOB_VERSION,
            git_sha: "deadbeef",
            functions: &functions,
            churn_by_path: BTreeMap::new(),
            caller_edges: BTreeMap::new(),
        };
        let json = serde_json::to_value(&request).expect("serialize request");
        assert!(
            json.get("churnByPath").is_none(),
            "empty churn must be omitted from the wire"
        );
    }

    fn dry_run_args() -> UploadInventoryArgs {
        UploadInventoryArgs {
            project_id: Some("acme/web".to_owned()),
            git_sha: Some("abcdef1".to_owned()),
            api_endpoint: Some("http://localhost:3000".to_owned()),
            allow_dirty: true,
            dry_run: true,
            ..UploadInventoryArgs::default()
        }
    }

    #[test]
    fn run_dry_run_emits_inventory_and_exits_zero() {
        let project = project_with_one_function();
        // Explicit project_id + git_sha keep this env- and git-free.
        let code = run(&dry_run_args(), project.path());
        assert_eq!(code, ExitCode::SUCCESS);
    }

    #[test]
    fn run_with_no_functions_is_a_validation_exit() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("package.json"), r#"{"name":"inv"}"#).unwrap();
        // Only a declaration file: the walker intentionally skips `*.d.ts`, so
        // the inventory is empty and run_inner returns a validation error.
        std::fs::write(
            root.join("src/types.d.ts"),
            "export declare const x: number;\n",
        )
        .unwrap();
        let code = run(&dry_run_args(), root);
        assert_eq!(code, ExitCode::from(EXIT_VALIDATION));
    }

    #[test]
    fn into_exit_maps_variants_and_soft_fails_transient_when_opted_in() {
        // Hard-fail mapping (no soft-fail opt-in).
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

        // With --ignore-upload-errors, only transient (server/network) failures
        // downgrade to exit 0; auth rejection stays fatal.
        assert_eq!(
            UploadError::ServerError("s".to_owned()).into_exit(true),
            ExitCode::SUCCESS
        );
        assert_eq!(
            UploadError::Network("n".to_owned()).into_exit(true),
            ExitCode::SUCCESS
        );
        assert_eq!(
            UploadError::AuthRejected("a".to_owned()).into_exit(true),
            ExitCode::from(EXIT_AUTH_REJECTED)
        );
    }

    #[test]
    fn resolve_git_sha_validates_explicit_value() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        let with_sha = |sha: &str| UploadInventoryArgs {
            git_sha: Some(sha.to_owned()),
            ..UploadInventoryArgs::default()
        };

        assert_eq!(
            resolve_git_sha(&with_sha("abcdef1"), root).unwrap(),
            "abcdef1"
        );
        assert!(resolve_git_sha(&with_sha(""), root).is_err(), "empty sha");
        assert!(
            resolve_git_sha(&with_sha(&"a".repeat(GIT_SHA_MAX_LEN + 1)), root).is_err(),
            "over-length sha"
        );
        assert!(
            resolve_git_sha(&with_sha("bad sha!"), root).is_err(),
            "illegal characters"
        );
    }

    fn entry(name: &str, line: u32, hash: &str) -> InventoryEntry {
        InventoryEntry {
            name: name.to_owned(),
            line,
            start_column: 1,
            end_line: line + 1,
            end_column: 2,
            source_hash: hash.to_owned(),
        }
    }

    #[test]
    fn attribute_caller_edges_matches_symbol_to_function_by_name() {
        let foo =
            InventoryFunction::from_entry("src/foo.ts", "src/foo.ts", entry("foo", 3, "h1"), None);
        let bar =
            InventoryFunction::from_entry("src/foo.ts", "src/foo.ts", entry("bar", 8, "h2"), None);
        let functions = vec![foo.clone(), bar.clone()];
        let edge = |importer: &str, symbol: &str| ImporterEdge {
            callee_file: "src/foo.ts".to_owned(),
            importer_file: importer.to_owned(),
            symbol: symbol.to_owned(),
        };
        let edges = vec![
            edge("src/a.ts", "foo"),
            edge("src/b.ts", "foo"),
            edge("src/c.ts", "missing"), // no function named "missing" -> ignored
            edge("src/d.ts", "*"),       // namespace import -> ignored
        ];

        let map = attribute_caller_edges(&functions, &edges);

        let foo_sites = map
            .get(&foo.identity.stable_id)
            .expect("foo has importer edges");
        assert_eq!(
            foo_sites
                .iter()
                .map(|s| s.file.as_str())
                .collect::<Vec<_>>(),
            vec!["src/a.ts", "src/b.ts"]
        );
        assert_eq!(foo_sites[0].symbols, vec!["foo".to_owned()]);
        // bar is never imported -> absent (never a placeholder entry).
        assert!(!map.contains_key(&bar.identity.stable_id));
    }

    #[test]
    fn attribute_caller_edges_dedups_repeated_importer_symbol() {
        let foo =
            InventoryFunction::from_entry("src/foo.ts", "src/foo.ts", entry("foo", 1, "h1"), None);
        let edge = ImporterEdge {
            callee_file: "src/foo.ts".to_owned(),
            importer_file: "src/a.ts".to_owned(),
            symbol: "foo".to_owned(),
        };
        let map = attribute_caller_edges(std::slice::from_ref(&foo), &[edge.clone(), edge]);
        let sites = map.get(&foo.identity.stable_id).expect("foo edges");
        assert_eq!(sites.len(), 1, "same importer+symbol collapses to one site");
        assert_eq!(sites[0].symbols, vec!["foo".to_owned()]);
    }

    #[test]
    fn caller_edges_serialize_as_camel_case_and_skip_when_empty() {
        let empty = InventoryRequest {
            version: INVENTORY_BLOB_VERSION,
            git_sha: "abc",
            functions: &[],
            churn_by_path: BTreeMap::new(),
            caller_edges: BTreeMap::new(),
        };
        let value = serde_json::to_value(&empty).expect("serialize empty");
        assert!(
            value.get("callerEdges").is_none(),
            "empty caller edges must be omitted, got: {value}"
        );

        let mut caller_edges = BTreeMap::new();
        caller_edges.insert(
            "sid1".to_owned(),
            vec![CallerSitePayload {
                file: "src/a.ts".to_owned(),
                symbols: vec!["foo".to_owned()],
            }],
        );
        let populated = InventoryRequest {
            version: INVENTORY_BLOB_VERSION_WITH_CALLERS,
            git_sha: "abc",
            functions: &[],
            churn_by_path: BTreeMap::new(),
            caller_edges,
        };
        let value = serde_json::to_value(&populated).expect("serialize populated");
        assert_eq!(value["callerEdges"]["sid1"][0]["file"], "src/a.ts");
        assert_eq!(value["callerEdges"]["sid1"][0]["symbols"][0], "foo");
        assert_eq!(value["version"], 3);
    }
}
