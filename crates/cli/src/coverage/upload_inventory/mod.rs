//! `fallow coverage upload-inventory` - push a static function inventory to
//! Fallow Cloud.
//!
//! The inventory is the **static side** of the three-state Production
//! Coverage story. The runtime coverage pipeline ships function hit-counts;
//! the cloud computes `inventory minus runtime-seen = untracked`.
//!
//! The cloud join key is `(filePath, functionName, lineNumber)` since the
//! line-aware function-identity migration (`0010`), so distinct same-named
//! functions at different lines in the same file are preserved and merged
//! into their own rows. The walker behind `fallow_engine::source::inventory`
//! emits Istanbul / `oxc-coverage-instrument`-compatible names and unique
//! 1-based line numbers per function declaration.
//!
//! This subcommand is a paid-tier workflow. It runs only when the user
//! invokes it explicitly; no other fallow command touches the network.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::Path;
use std::process::ExitCode;

use fallow_config::ResolvedConfig;
use fallow_cov_protocol::{FunctionIdentity, IdentityResolution, function_identity_id};
use fallow_engine::churn::{ChurnResult, ChurnTrend, FileChurn, analyze_churn_cached, parse_since};
use fallow_engine::session::AnalysisSession;
use fallow_engine::source::inventory::{
    InventoryComplexity, InventoryEntry, walk_source_with_complexity,
};
use globset::{Glob, GlobSet, GlobSetBuilder};
use rustc_hash::{FxHashMap, FxHashSet};
use serde::{Deserialize, Serialize};

use colored::Colorize as _;

use crate::api::{
    ResponseBodyReader, parse_error_envelope, sanitize_network_error, try_api_agent_with_timeout,
};
use crate::coverage::upload_common::{
    self, UploadError, display_endpoint_url, format_count, format_upload_error_message,
    to_posix_string,
};
use crate::report::format_bytes;

mod routes;

use routes::{InventoryRoutes, RouteLimits, RoutePayload};

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

/// Wire version once the payload also carries entry routes (`routes` plus the
/// per-function `routeIds`). Same additive contract as version 3.
const INVENTORY_BLOB_VERSION_WITH_ROUTES: u8 = 4;

/// Cap on importer sites recorded per function. Mirrors the server's per-callee
/// cap so a pathological fan-in (a barrel re-exported everywhere) cannot bloat
/// the payload or trip the server bound. Truncation is deterministic: sites are
/// ordered by importer path before the cut, and the cap travels in the blob
/// header ([`CallerEdgeLimits`]) so a reader can tell a fan-in of exactly the
/// cap apart from an untruncated one.
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
    /// Bind this upload to a Vercel deployment, verified by the cloud.
    pub vercel_deployment_id: Option<String>,
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
            .field("vercel_deployment_id", &self.vercel_deployment_id)
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
pub fn run(args: &UploadInventoryArgs, root: &Path, allow_remote_extends: bool) -> ExitCode {
    match run_inner(args, root, allow_remote_extends) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => err.into_exit(LOG_PREFIX, args.ignore_upload_errors),
    }
}

fn run_inner(
    args: &UploadInventoryArgs,
    root: &Path,
    allow_remote_extends: bool,
) -> Result<(), UploadError> {
    let deployment_id =
        super::vercel_upload::resolve_deployment_id(args.vercel_deployment_id.as_deref())
            .map_err(UploadError::Validation)?;
    let prepared = prepare_inventory_upload(args, root, allow_remote_extends)?;
    let caller_edge_limits = prepared.caller_edges.limits();
    let payload = InventoryRequest {
        version: prepared.version,
        git_sha: &prepared.git_sha,
        functions: &prepared.functions,
        churn_by_path: prepared.churn_by_path,
        caller_edges: prepared.caller_edges.by_callee,
        caller_edge_limits,
        routes: &prepared.routes.routes,
        route_limits: prepared.routes.limits(),
    };

    if args.dry_run {
        print_dry_run_summary(
            &prepared.project_id,
            &prepared.git_sha,
            prepared.path_prefix.as_deref(),
            &prepared.functions,
            args.api_endpoint.as_deref(),
        );
        if args.with_callers {
            println!(
                "{LOG_PREFIX}: caller edges resolved for {} functions",
                format_count(payload.caller_edges.len()),
            );
            for (framework, count) in routes::routes_per_framework(payload.routes) {
                println!(
                    "{LOG_PREFIX}: entry routes detected: {} ({framework})",
                    format_count(count),
                );
            }
        }
        return Ok(());
    }

    let api_key =
        upload_common::resolve_api_key(args.api_key.as_deref()).map_err(UploadError::Validation)?;
    upload(
        &prepared.project_id,
        args.api_endpoint.as_deref(),
        &api_key,
        &payload,
        deployment_id.as_deref(),
    )
}

struct PreparedInventory {
    project_id: String,
    git_sha: String,
    path_prefix: Option<String>,
    functions: Vec<InventoryFunction>,
    churn_by_path: BTreeMap<String, FileChurnPayload>,
    caller_edges: CallerEdges,
    routes: InventoryRoutes,
    version: u8,
}

fn prepare_inventory_upload(
    args: &UploadInventoryArgs,
    root: &Path,
    allow_remote_extends: bool,
) -> Result<PreparedInventory, UploadError> {
    let project_id = upload_common::resolve_project_id(args.project_id.as_deref(), root)
        .map_err(UploadError::Validation)?;
    let git_sha = upload_common::resolve_git_sha(args.git_sha.as_deref(), root)
        .map_err(UploadError::Validation)?;
    let path_prefix = normalize_path_prefix(args.path_prefix.as_deref())?;
    upload_common::enforce_clean_worktree(
        LOG_PREFIX,
        "upload-inventory",
        "the inventory comes",
        args.dry_run,
        args.allow_dirty,
        root,
    )?;

    let config = upload_common::load_resolved_config_with_options(root, allow_remote_extends)
        .map_err(UploadError::Validation)?;
    let session = AnalysisSession::from_resolved_config(config.clone())
        .map_err(|err| UploadError::Validation(format!("analysis failed: {err}")))?;
    let exclude_matcher = compile_exclude_matcher(&args.exclude_paths)?;
    let mut functions = collect_inventory(&session, &exclude_matcher, path_prefix.as_deref());
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

    // Importer edges and entry routes are opt-in: building them runs the full
    // static analysis to get the import graph, while the default upload is a
    // fast per-file walk. Best-effort, so a graph-build failure still ships the
    // inventory.
    let (caller_edges, routes) = if args.with_callers {
        collect_graph_context(&session, &mut functions)
    } else {
        (CallerEdges::default(), InventoryRoutes::default())
    };
    warn_on_truncated_caller_edges(&caller_edges);
    routes::warn_on_route_limits(&routes);
    let version = if !routes.routes.is_empty() {
        INVENTORY_BLOB_VERSION_WITH_ROUTES
    } else if caller_edges.by_callee.is_empty() {
        INVENTORY_BLOB_VERSION
    } else {
        INVENTORY_BLOB_VERSION_WITH_CALLERS
    };

    Ok(PreparedInventory {
        project_id,
        git_sha,
        path_prefix,
        functions,
        churn_by_path,
        caller_edges,
        routes,
        version,
    })
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
    session: &AnalysisSession,
    exclude_matcher: &GlobSet,
    path_prefix: Option<&str>,
) -> Vec<InventoryFunction> {
    let config = session.config();
    let mut seen: FxHashSet<(String, String, u32)> = FxHashSet::default();
    let mut out: Vec<InventoryFunction> = Vec::new();
    for file in session.files() {
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
            let metrics = entry
                .source_hash
                .as_deref()
                .and_then(|h| complexity.get(h))
                .copied();
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
    if result.shallow_clone {
        return BTreeMap::new();
    }
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

/// Whether the inventory walker can parse this file. Shared with the cloud
/// analyze path, which walks the same tree to index instrumenter names.
pub(super) fn extension_supported(path: &Path) -> bool {
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
    /// Ids of the `routes` entries whose files can statically reach the file of
    /// this function. Omitted when empty. Context only; never a gate.
    #[serde(rename = "routeIds", skip_serializing_if = "Vec::is_empty")]
    route_ids: Vec<String>,
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
            source_hash: entry.source_hash.clone(),
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
            route_ids: Vec::new(),
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
    /// Blob header for `callerEdges`: the size guard the producer applied plus
    /// how many callees it actually cut. Present only when `callerEdges` is,
    /// so a v1/v2 body keeps its exact wire shape. Without it a reader cannot
    /// tell a callee that genuinely has `maxSitesPerFunction` importers apart
    /// from one whose fan-in was truncated, so a consumer that reports fan-in
    /// reads the cap from here rather than hard-coding the producer's constant.
    #[serde(rename = "callerEdgeLimits", skip_serializing_if = "Option::is_none")]
    caller_edge_limits: Option<CallerEdgeLimits>,
    /// Framework entry routes, with the patterns the route file names give.
    /// Skipped when empty, so a body without routes keeps the version 3 shape.
    #[serde(skip_serializing_if = "<[RoutePayload]>::is_empty")]
    routes: &'a [RoutePayload],
    /// Size guard of `routes`. Present only when `routes` is.
    #[serde(rename = "routeLimits", skip_serializing_if = "Option::is_none")]
    route_limits: Option<RouteLimits>,
}

/// Wire form of the `callerEdges` size guard. `truncatedFunctions` counts
/// callees whose importer list hit `maxSitesPerFunction` and lost entries; a
/// callee with exactly `maxSitesPerFunction` sites is only truncated when that
/// count is non-zero, which is why the cap alone is not enough to report.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
struct CallerEdgeLimits {
    #[serde(rename = "maxSitesPerFunction")]
    max_sites_per_function: u32,
    #[serde(rename = "maxSymbolsPerSite")]
    max_symbols_per_site: u32,
    #[serde(rename = "truncatedFunctions")]
    truncated_functions: u32,
}

/// The attributed importer-edge map plus the bookkeeping the blob header
/// reports. Kept together so the truncation count can never drift away from the
/// map it describes.
#[derive(Debug, Default)]
struct CallerEdges {
    /// Importer sites keyed by the callee export's `stable_id`.
    by_callee: BTreeMap<String, Vec<CallerSitePayload>>,
    /// Callees whose importer list was cut to [`MAX_CALLER_SITES_PER_FN`].
    truncated_functions: usize,
}

impl CallerEdges {
    /// Blob header for this map, or `None` when there are no edges to describe.
    fn limits(&self) -> Option<CallerEdgeLimits> {
        if self.by_callee.is_empty() {
            return None;
        }
        Some(CallerEdgeLimits {
            max_sites_per_function: u32::try_from(MAX_CALLER_SITES_PER_FN).unwrap_or(u32::MAX),
            max_symbols_per_site: u32::try_from(MAX_SYMBOLS_PER_CALLER_SITE).unwrap_or(u32::MAX),
            truncated_functions: u32::try_from(self.truncated_functions).unwrap_or(u32::MAX),
        })
    }
}

/// Tell the user when the size guard actually dropped importer sites, so a
/// surprisingly small fan-in on the dashboard has a visible cause in the CI log.
fn warn_on_truncated_caller_edges(edges: &CallerEdges) {
    if edges.truncated_functions == 0 {
        return;
    }
    eprintln!(
        "{LOG_PREFIX}: {}: {} function(s) have more than {} importers; the extra importer sites were dropped from the upload.",
        "warning".yellow().bold(),
        format_count(edges.truncated_functions),
        format_count(MAX_CALLER_SITES_PER_FN),
    );
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
/// producing the `callerEdges` map keyed by callee `stable_id` plus the
/// truncation bookkeeping the blob header reports.
///
/// Import-edge granularity, best-effort: `default` / `*` / side-effect imports
/// and any symbol that does not name an inventory function simply contribute
/// nothing (no false attribution). A name can repeat in a file (overloads,
/// same-named nested functions), so an edge attributes to every matching
/// stable_id. Pure: no graph or IO, so it is unit-tested directly.
fn attribute_caller_edges(functions: &[InventoryFunction], edges: &[ImporterEdge]) -> CallerEdges {
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

    let mut truncated_functions = 0_usize;
    let by_callee = acc
        .into_iter()
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
            if sites.len() > MAX_CALLER_SITES_PER_FN {
                truncated_functions += 1;
                sites.truncate(MAX_CALLER_SITES_PER_FN);
            }
            (stable_id, sites)
        })
        .collect();

    CallerEdges {
        by_callee,
        truncated_functions,
    }
}

/// Build the importer-edge map and the entry routes for the inventory by
/// running the static analysis once (graph retained). Each import edge is
/// attributed to a callee function, and each route file is walked forward
/// through the runtime imports to the functions it can reach.
///
/// Best-effort context: any failure (analysis error, no graph/files retained)
/// yields empty results so the upload still ships the inventory. Never a
/// verdict input on the server side.
fn collect_graph_context(
    session: &AnalysisSession,
    functions: &mut [InventoryFunction],
) -> (CallerEdges, InventoryRoutes) {
    let artifacts = match session.analyze_dead_code_retaining_files(false, true) {
        Ok(artifacts) => artifacts,
        Err(err) => {
            eprintln!(
                "{LOG_PREFIX}: {}: import graph build failed, uploading without caller edges ({err})",
                "warning".yellow().bold(),
            );
            return (CallerEdges::default(), InventoryRoutes::default());
        }
    };
    let (Some(graph), Some(files)) = (artifacts.graph.as_ref(), artifacts.files.as_ref()) else {
        return (CallerEdges::default(), InventoryRoutes::default());
    };
    let config = session.config();

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

    let file_slots = files.iter().map(|f| f.id.0 as usize + 1).max().unwrap_or(0);
    let mut forward: Vec<Vec<usize>> = vec![Vec::new(); file_slots];
    let mut edges: Vec<ImporterEdge> = Vec::new();
    for file in files {
        let Some(callee_file) = repo_relative_by_id.get(&file.id) else {
            continue;
        };
        for summary in graph.direct_importer_summaries(file.id) {
            // A side-effect import has no symbols and still runs the module; a
            // type-only import cannot run it.
            if summary.symbols.iter().all(|s| s.type_only) && !summary.symbols.is_empty() {
                continue;
            }
            if let Some(slot) = forward.get_mut(summary.source.0 as usize) {
                slot.push(file.id.0 as usize);
            }
            let Some(importer_file) = repo_relative_by_id.get(&summary.source) else {
                continue;
            };
            for symbol in summary.symbols.iter().filter(|s| !s.type_only) {
                edges.push(ImporterEdge {
                    callee_file: callee_file.clone(),
                    importer_file: importer_file.clone(),
                    symbol: symbol.imported.clone(),
                });
            }
        }
    }

    let caller_edges = attribute_caller_edges(functions, &edges);
    let routes = collect_routes(
        session,
        graph,
        files,
        &forward,
        &repo_relative_by_id,
        functions,
    );
    (caller_edges, routes)
}

/// Find the framework route files among the plugin entry points and the routes
/// that Elysia code defines, and attach the routes that reach each function.
/// Empty when the project has no route framework.
fn collect_routes(
    session: &AnalysisSession,
    graph: &fallow_engine::module_graph::RetainedModuleGraph,
    files: &[fallow_engine::discover::DiscoveredFile],
    forward: &[Vec<usize>],
    repo_relative_by_id: &FxHashMap<fallow_engine::discover::FileId, String>,
    functions: &mut [InventoryFunction],
) -> InventoryRoutes {
    let entry_points = match fallow_engine::list_inventory::collect_listing_inventory(session, true)
    {
        Ok(listing) => listing.entry_points.unwrap_or_default(),
        Err(err) => {
            eprintln!(
                "{LOG_PREFIX}: {}: plugin stage failed, uploading without file-based entry routes ({})",
                "warning".yellow().bold(),
                err.message(),
            );
            Vec::new()
        }
    };
    let config = session.config();
    let mut roots: Vec<&Path> = vec![config.root.as_path()];
    roots.extend(session.workspaces().iter().map(|w| w.root.as_path()));
    let file_index_by_path: FxHashMap<&Path, usize> = files
        .iter()
        .map(|f| (f.path.as_path(), f.id.0 as usize))
        .collect();
    let value_exports = fallow_engine::module_graph::module_value_exports(graph);
    let exports = routes::method_exports_by_file(
        value_exports
            .iter()
            .map(|e| (e.file_id.0 as usize, e.name.as_str())),
    );
    let mut candidates = routes::route_candidates(
        &entry_points,
        &roots,
        &config.root,
        &file_index_by_path,
        &exports,
    );
    let path_by_index: FxHashMap<usize, &Path> = file_index_by_path
        .iter()
        .map(|(&path, &index)| (index, path))
        .collect();
    candidates.extend(routes::code_route_candidates(
        &fallow_engine::code_routes::elysia_routes(graph),
        &roots,
        &config.root,
        &path_by_index,
    ));
    let file_index_by_repo_path: FxHashMap<String, usize> = repo_relative_by_id
        .iter()
        .map(|(id, path)| (path.clone(), id.0 as usize))
        .collect();
    routes::attach_routes(candidates, forward, &file_index_by_repo_path, functions)
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
    deployment_id: Option<&str>,
) -> Result<(), UploadError> {
    let url = upload_common::endpoint_url(endpoint_override, project_id, "inventory");
    println!(
        "{LOG_PREFIX}: uploading {} functions for {project_id} @ {}",
        format_count(payload.functions.len()),
        payload.git_sha,
    );

    let agent = try_api_agent_with_timeout(UPLOAD_CONNECT_TIMEOUT_SECS, UPLOAD_TOTAL_TIMEOUT_SECS)
        .map_err(|err| UploadError::Network(err.to_string()))?;
    let mut request = agent
        .post(&url)
        .header("Authorization", &format!("Bearer {api_key}"));
    if let Some(id) = deployment_id {
        request = request.header(super::vercel_upload::DEPLOYMENT_ID_HEADER, id);
    }
    let mut response = request.send_json(payload).map_err(|err| {
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
            "  -> Inventory stored. The Untracked filter lights up once runtime coverage arrives for this SHA. Dashboard: {}",
            upload_common::dashboard_repo_url(project_id)
        );
        if let Some(overlap) = data.data.path_overlap.as_ref() {
            print_overlap_warning_if_needed(overlap);
        }
        return Ok(());
    }

    let body = response.read_to_string().unwrap_or_default();
    let envelope = parse_error_envelope(&body);
    let code = envelope.code();
    let message = format_upload_error_message("upload-inventory", status, &body, code, &envelope);
    classify_upload_error(status, code, message)
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

fn print_dry_run_summary(
    project_id: &str,
    git_sha: &str,
    path_prefix: Option<&str>,
    functions: &[InventoryFunction],
    endpoint_override: Option<&str>,
) {
    let decoded_url = display_endpoint_url(endpoint_override, project_id, "inventory");
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
mod tests;
