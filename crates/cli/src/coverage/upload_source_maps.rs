//! `fallow coverage upload-source-maps` - upload build source maps to fallow cloud.
//!
//! This is a CI-side helper for bundled runtime coverage. The beacon reports
//! coverage against deployed bundle paths; source maps uploaded here let the
//! cloud resolver map those positions back to original source files.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::time::SystemTime;

use colored::Colorize as _;
use fallow_engine::clear_ambient_git_env;
use globset::{Glob, GlobSet, GlobSetBuilder};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};

use crate::api::{
    NETWORK_EXIT_CODE, ResponseBodyReader, api_url, parse_error_envelope, response_message_suffix,
    retry_delay_for_status, sanitize_network_error, should_retry_status,
    try_api_agent_with_timeout,
};

const LOG_PREFIX: &str = "fallow coverage upload-source-maps";
const DEFAULT_ENDPOINT: &str = "https://api.fallow.cloud";
const CONNECT_TIMEOUT_SECS: u64 = 5;
const TOTAL_TIMEOUT_SECS: u64 = 60;
const MAX_ATTEMPTS: u8 = 3;
const WARN_MAP_BYTES: u64 = 10 * 1024 * 1024;
const MAX_MAP_BYTES: u64 = 100 * 1024 * 1024;

#[derive(Debug, Clone)]
pub struct UploadSourceMapsArgs {
    pub dir: PathBuf,
    pub include: String,
    pub exclude: Vec<String>,
    pub repo: Option<String>,
    pub git_sha: Option<String>,
    pub endpoint: Option<String>,
    pub strip_path: bool,
    pub dry_run: bool,
    pub concurrency: usize,
    pub fail_fast: bool,
}

#[derive(Debug)]
enum UploadSourceMapsError {
    Validation(String),
    Network(String),
    Partial(Vec<MapOutcome>),
}

impl UploadSourceMapsError {
    fn into_exit(self) -> ExitCode {
        match self {
            Self::Validation(message) => {
                eprintln!("{LOG_PREFIX}: {}: {message}", "error".red().bold());
                ExitCode::from(2)
            }
            Self::Network(message) => {
                eprintln!("{LOG_PREFIX}: {}: {message}", "error".red().bold());
                ExitCode::from(NETWORK_EXIT_CODE)
            }
            Self::Partial(outcomes) => {
                print_failure_summary(&outcomes);
                ExitCode::from(1)
            }
        }
    }
}

pub fn run(args: &UploadSourceMapsArgs, root: &Path) -> ExitCode {
    match run_inner(args, root) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => err.into_exit(),
    }
}

fn run_inner(args: &UploadSourceMapsArgs, root: &Path) -> Result<(), UploadSourceMapsError> {
    let build_dir = resolve_build_dir(root, &args.dir);
    if !build_dir.is_dir() {
        return Err(UploadSourceMapsError::Validation(format!(
            "directory not found: {}",
            build_dir.display()
        )));
    }

    let include_patterns = vec![args.include.clone()];
    let include = compile_glob_set(&include_patterns, "--include")?;
    let exclude = compile_glob_set(&args.exclude, "--exclude")?;
    let repo = resolve_repo_name(args.repo.as_deref(), root)?;
    let git_sha = resolve_git_sha(args.git_sha.as_deref(), root)?;
    let maps = collect_source_maps(root, &build_dir, &include, &exclude, args.strip_path)?;

    if maps.is_empty() {
        return Err(UploadSourceMapsError::Validation(format!(
            "no .map files found in {} (did the build step run?)",
            build_dir.display()
        )));
    }

    if args.dry_run {
        print_dry_run(&repo, &git_sha, args.endpoint.as_deref(), &maps);
        return Ok(());
    }

    let api_key = resolve_api_key()?;
    upload_maps(args, &repo, &git_sha, &api_key, &maps)
}

fn resolve_build_dir(root: &Path, dir: &Path) -> PathBuf {
    if dir.is_absolute() {
        dir.to_path_buf()
    } else {
        root.join(dir)
    }
}

fn compile_glob_set(patterns: &[String], flag: &str) -> Result<GlobSet, UploadSourceMapsError> {
    let mut builder = GlobSetBuilder::new();
    for pattern in patterns {
        let glob = Glob::new(pattern).map_err(|err| {
            UploadSourceMapsError::Validation(format!("invalid {flag} '{pattern}': {err}"))
        })?;
        builder.add(glob);
        if let Some(without_prefix) = pattern.strip_prefix("**/") {
            builder.add(Glob::new(without_prefix).map_err(|err| {
                UploadSourceMapsError::Validation(format!("invalid {flag} '{pattern}': {err}"))
            })?);
        }
    }
    builder.build().map_err(|err| {
        UploadSourceMapsError::Validation(format!("failed to compile {flag}: {err}"))
    })
}

fn resolve_repo_name(explicit: Option<&str>, root: &Path) -> Result<String, UploadSourceMapsError> {
    if let Some(repo) = explicit {
        return validate_repo_name(repo.trim()).map(str::to_owned);
    }
    if let Some(repo) = package_json_repository_name(root) {
        return validate_repo_name(&repo).map(str::to_owned);
    }
    if let Some(repo) = git_origin_repo_name(root) {
        return validate_repo_name(&repo).map(str::to_owned);
    }
    Err(UploadSourceMapsError::Validation(
        "unable to determine repo name; pass --repo".to_owned(),
    ))
}

fn package_json_repository_name(root: &Path) -> Option<String> {
    let package_json = std::fs::read_to_string(root.join("package.json")).ok()?;
    let value: serde_json::Value = serde_json::from_str(&package_json).ok()?;
    let repository = value.get("repository")?;
    let url = match repository {
        serde_json::Value::String(url) => url.as_str(),
        serde_json::Value::Object(map) => map.get("url")?.as_str()?,
        _ => return None,
    };
    parse_repo_name_from_url(url)
}

fn git_origin_repo_name(root: &Path) -> Option<String> {
    let mut command = Command::new("git");
    command
        .args(["remote", "get-url", "origin"])
        .current_dir(root);
    clear_ambient_git_env(&mut command);
    let output = command.output().ok()?;
    if !output.status.success() {
        return None;
    }
    parse_repo_name_from_url(String::from_utf8_lossy(&output.stdout).trim())
}

fn parse_repo_name_from_url(url: &str) -> Option<String> {
    let stripped_suffix = url
        .trim()
        .trim_end_matches('/')
        .trim_end_matches(".git")
        .trim_end_matches('/');
    if !stripped_suffix.contains(':')
        && let Some(project_id) = take_last_two_segments(stripped_suffix)
    {
        return Some(project_id);
    }
    if let Some((_, path)) = stripped_suffix.split_once(':')
        && let Some(project_id) = take_last_two_segments(path)
    {
        return Some(project_id);
    }
    if let Some(path_part) = stripped_suffix.split("://").nth(1)
        && let Some((_, tail)) = path_part.split_once('/')
        && let Some(project_id) = take_last_two_segments(tail)
    {
        return Some(project_id);
    }
    None
}

fn take_last_two_segments(path: &str) -> Option<String> {
    let mut parts: Vec<&str> = path
        .trim_end_matches('/')
        .split('/')
        .filter(|segment| !segment.trim().is_empty())
        .collect();
    if parts.len() < 2 {
        return None;
    }
    let repo = parts.pop()?.trim();
    let owner = parts.pop()?.trim();
    (!owner.is_empty() && !repo.is_empty()).then(|| format!("{owner}/{repo}"))
}

fn validate_repo_name(repo: &str) -> Result<&str, UploadSourceMapsError> {
    if repo.is_empty() {
        return Err(UploadSourceMapsError::Validation(
            "unable to determine repo name; pass --repo".to_owned(),
        ));
    }
    if repo.contains("..") || repo.contains('\\') {
        return Err(UploadSourceMapsError::Validation(
            "repo name must not contain '..' or backslashes".to_owned(),
        ));
    }
    Ok(repo)
}

fn resolve_git_sha(explicit: Option<&str>, root: &Path) -> Result<String, UploadSourceMapsError> {
    let sha = if let Some(sha) = explicit {
        sha.trim().to_owned()
    } else if let Some(sha) = env_non_empty("GITHUB_SHA")
        .or_else(|| env_non_empty("CI_COMMIT_SHA"))
        .or_else(|| env_non_empty("COMMIT_SHA"))
    {
        sha
    } else {
        let mut command = Command::new("git");
        command.args(["rev-parse", "HEAD"]).current_dir(root);
        clear_ambient_git_env(&mut command);
        let output = command.output().map_err(|_| {
            UploadSourceMapsError::Validation(
                "unable to determine git SHA; pass --git-sha or set $GITHUB_SHA".to_owned(),
            )
        })?;
        if !output.status.success() {
            return Err(UploadSourceMapsError::Validation(
                "unable to determine git SHA; pass --git-sha or set $GITHUB_SHA".to_owned(),
            ));
        }
        String::from_utf8_lossy(&output.stdout).trim().to_owned()
    };
    validate_git_sha(&sha)?;
    Ok(sha)
}

fn env_non_empty(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

fn validate_git_sha(sha: &str) -> Result<(), UploadSourceMapsError> {
    if !(7..=40).contains(&sha.len()) || !sha.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return Err(UploadSourceMapsError::Validation(
            "unable to determine git SHA; pass --git-sha or set $GITHUB_SHA".to_owned(),
        ));
    }
    Ok(())
}

#[derive(Debug, Clone)]
struct SourceMapCandidate {
    path: PathBuf,
    rel_path: PathBuf,
    file_name: String,
    /// The map's path relative to the REPO ROOT (e.g.
    /// `dashboard/dist/assets/app.js.map`), posix-separated. `None` when the map
    /// is not under the repo root (an absolute `--dir` outside it) or the path
    /// fails the same safety checks as `file_name`. The cloud resolves each
    /// `sources[]` entry against this path's directory so a monorepo
    /// sub-package map's `../../src/x` resolves to `dashboard/src/x` instead of
    /// collapsing to `src/x` and losing the package prefix (issue #260). It is
    /// distinct from `file_name`, which keys the upload's storage + identity.
    map_path: Option<String>,
    bytes: u64,
}

fn collect_source_maps(
    repo_root: &Path,
    dir: &Path,
    include: &GlobSet,
    exclude: &GlobSet,
    strip_path: bool,
) -> Result<Vec<SourceMapCandidate>, UploadSourceMapsError> {
    let mut maps = Vec::new();
    let mut input = SourceMapWalkInput {
        repo_root,
        root: dir,
        include,
        exclude,
        strip_path,
        maps: &mut maps,
    };
    collect_source_maps_inner(&mut input, dir)?;
    maps.sort_by(|a, b| a.rel_path.cmp(&b.rel_path));
    Ok(maps)
}

struct SourceMapWalkInput<'a> {
    repo_root: &'a Path,
    root: &'a Path,
    include: &'a GlobSet,
    exclude: &'a GlobSet,
    strip_path: bool,
    maps: &'a mut Vec<SourceMapCandidate>,
}

fn collect_source_maps_inner(
    input: &mut SourceMapWalkInput<'_>,
    dir: &Path,
) -> Result<(), UploadSourceMapsError> {
    let entries = std::fs::read_dir(dir).map_err(|err| {
        UploadSourceMapsError::Validation(format!("failed to read {}: {err}", dir.display()))
    })?;
    for entry in entries {
        let entry = entry.map_err(|err| {
            UploadSourceMapsError::Validation(format!("failed to read {}: {err}", dir.display()))
        })?;
        let path = entry.path();
        let file_type = entry.file_type().map_err(|err| {
            UploadSourceMapsError::Validation(format!("failed to stat {}: {err}", path.display()))
        })?;
        let rel_path = path.strip_prefix(input.root).unwrap_or(&path).to_path_buf();
        if input.exclude.is_match(&rel_path) {
            continue;
        }
        if file_type.is_dir() {
            collect_source_maps_inner(input, &path)?;
            continue;
        }
        if !input.include.is_match(&rel_path) || !path.is_file() {
            continue;
        }
        let bytes = entry.metadata().map_or(0, |metadata| metadata.len());
        let file_name = map_file_name(&rel_path, input.strip_path)?;
        let map_path = repo_relative_map_path(input.repo_root, &path);
        input.maps.push(SourceMapCandidate {
            path,
            rel_path,
            file_name,
            map_path,
            bytes,
        });
    }
    Ok(())
}

/// The map file's path relative to `repo_root`, posix-separated, or `None` when
/// the map is not under the repo root or the result is not a safe relative path
/// (issue #260). Only `Some` paths are sent as `mapPath`; the cloud falls back
/// to root-anchored normalization when absent, so dropping it is always safe.
fn repo_relative_map_path(repo_root: &Path, path: &Path) -> Option<String> {
    let rel = path.strip_prefix(repo_root).ok()?;
    let value = to_posix_string(rel);
    validate_file_name(&value).ok().map(|()| value)
}

fn map_file_name(rel_path: &Path, strip_path: bool) -> Result<String, UploadSourceMapsError> {
    let value = if strip_path {
        rel_path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default()
            .to_owned()
    } else {
        to_posix_string(rel_path)
    };
    validate_file_name(&value)?;
    Ok(value)
}

fn validate_file_name(file_name: &str) -> Result<(), UploadSourceMapsError> {
    if file_name.is_empty()
        || file_name.len() > 255
        || file_name.starts_with('/')
        || file_name.contains('\\')
        || file_name
            .split('/')
            .any(|segment| segment.is_empty() || segment == "." || segment == "..")
    {
        return Err(UploadSourceMapsError::Validation(format!(
            "invalid source map fileName '{file_name}'"
        )));
    }
    Ok(())
}

fn to_posix_string(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn resolve_api_key() -> Result<String, UploadSourceMapsError> {
    env_non_empty("FALLOW_API_KEY")
        .ok_or_else(|| UploadSourceMapsError::Validation("FALLOW_API_KEY is required".to_owned()))
}

#[derive(Debug, Clone)]
struct PreparedSourceMap {
    candidate: SourceMapCandidate,
    source_map: serde_json::Value,
}

fn prepare_source_map(candidate: &SourceMapCandidate) -> MapOutcome {
    if candidate.bytes > MAX_MAP_BYTES {
        return MapOutcome::failed(
            candidate,
            FailureKind::Validation,
            format!(
                "source map is too large ({}); maximum is {}",
                format_bytes(candidate.bytes),
                format_bytes(MAX_MAP_BYTES)
            ),
        );
    }
    match std::fs::read_to_string(&candidate.path) {
        Ok(content) => match serde_json::from_str::<serde_json::Value>(&content) {
            Ok(source_map) => MapOutcome::Ready(PreparedSourceMap {
                candidate: candidate.clone(),
                source_map,
            }),
            Err(err) => MapOutcome::failed(
                candidate,
                FailureKind::Validation,
                format!("not valid JSON ({err}); skipping"),
            ),
        },
        Err(err) => MapOutcome::failed(
            candidate,
            FailureKind::Validation,
            format!("read failed: {err}"),
        ),
    }
}

fn upload_maps(
    args: &UploadSourceMapsArgs,
    repo: &str,
    git_sha: &str,
    api_key: &str,
    maps: &[SourceMapCandidate],
) -> Result<(), UploadSourceMapsError> {
    let (mut outcomes, ready) = prepare_source_maps_for_upload(args, maps)?;
    if ready.is_empty() {
        return Err(UploadSourceMapsError::Partial(outcomes));
    }

    print_upload_source_maps_summary(args, repo, git_sha, maps);

    let agent = try_api_agent_with_timeout(CONNECT_TIMEOUT_SECS, TOTAL_TIMEOUT_SECS)
        .map_err(|err| UploadSourceMapsError::Network(err.to_string()))?;
    let mut uploaded = upload_ready_source_maps(args, repo, git_sha, api_key, &agent, &ready)?;
    outcomes.append(&mut uploaded);
    finish_source_map_upload(outcomes)
}

fn prepare_source_maps_for_upload(
    args: &UploadSourceMapsArgs,
    maps: &[SourceMapCandidate],
) -> Result<(Vec<MapOutcome>, Vec<PreparedSourceMap>), UploadSourceMapsError> {
    let mut outcomes = Vec::with_capacity(maps.len());
    let mut ready = Vec::new();
    for candidate in maps {
        warn_large_source_map(candidate);
        match prepare_source_map(candidate) {
            MapOutcome::Ready(prepared) => ready.push(prepared),
            failed => {
                outcomes.push(failed);
                if args.fail_fast {
                    return Err(UploadSourceMapsError::Partial(outcomes));
                }
            }
        }
    }
    Ok((outcomes, ready))
}

fn warn_large_source_map(candidate: &SourceMapCandidate) {
    if candidate.bytes > WARN_MAP_BYTES && candidate.bytes <= MAX_MAP_BYTES {
        eprintln!(
            "{LOG_PREFIX}: {}: {} is large ({})",
            "warning".yellow().bold(),
            candidate.rel_path.display(),
            format_bytes(candidate.bytes),
        );
    }
}

fn print_upload_source_maps_summary(
    args: &UploadSourceMapsArgs,
    repo: &str,
    git_sha: &str,
    maps: &[SourceMapCandidate],
) {
    println!("{LOG_PREFIX}: repo={repo} sha={git_sha}");
    println!(
        "{LOG_PREFIX}: found {} maps ({})",
        maps.len(),
        format_bytes(maps.iter().map(|map| map.bytes).sum())
    );
    println!(
        "{LOG_PREFIX}: uploading to {}",
        display_endpoint_url(args.endpoint.as_deref(), repo)
    );
}

fn upload_ready_source_maps(
    args: &UploadSourceMapsArgs,
    repo: &str,
    git_sha: &str,
    api_key: &str,
    agent: &ureq::Agent,
    ready: &[PreparedSourceMap],
) -> Result<Vec<MapOutcome>, UploadSourceMapsError> {
    if args.fail_fast {
        return Ok(upload_ready_source_maps_fail_fast(
            args, repo, git_sha, api_key, agent, ready,
        ));
    }

    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(args.concurrency.max(1))
        .build()
        .map_err(|err| {
            UploadSourceMapsError::Validation(format!("invalid --concurrency: {err}"))
        })?;
    Ok(pool.install(|| {
        ready
            .par_iter()
            .map(|map| upload_one(agent, args.endpoint.as_deref(), repo, git_sha, api_key, map))
            .collect()
    }))
}

fn upload_ready_source_maps_fail_fast(
    args: &UploadSourceMapsArgs,
    repo: &str,
    git_sha: &str,
    api_key: &str,
    agent: &ureq::Agent,
    ready: &[PreparedSourceMap],
) -> Vec<MapOutcome> {
    let mut uploaded = Vec::new();
    for map in ready {
        let outcome = upload_one(agent, args.endpoint.as_deref(), repo, git_sha, api_key, map);
        let failed = matches!(outcome, MapOutcome::Failed { .. });
        uploaded.push(outcome);
        if failed {
            break;
        }
    }
    uploaded
}

fn finish_source_map_upload(outcomes: Vec<MapOutcome>) -> Result<(), UploadSourceMapsError> {
    let success_count = outcomes
        .iter()
        .filter(|outcome| outcome.is_success())
        .count();
    let failure_count = outcomes.len().saturating_sub(success_count);
    if failure_count > 0 {
        if success_count == 0
            && outcomes
                .iter()
                .filter_map(MapOutcome::failure_kind)
                .all(|kind| kind == FailureKind::Network)
        {
            let detail = outcomes
                .iter()
                .find_map(MapOutcome::failure_reason)
                .unwrap_or("network error");
            return Err(UploadSourceMapsError::Network(format!(
                "all source map uploads failed with network errors: {detail}"
            )));
        }
        return Err(UploadSourceMapsError::Partial(outcomes));
    }

    println!(
        "{LOG_PREFIX}: {}/{} uploaded",
        success_count,
        outcomes.len()
    );
    Ok(())
}

fn upload_one(
    agent: &ureq::Agent,
    endpoint_override: Option<&str>,
    repo: &str,
    git_sha: &str,
    api_key: &str,
    map: &PreparedSourceMap,
) -> MapOutcome {
    for attempt in 1..=MAX_ATTEMPTS {
        match send_source_map(agent, endpoint_override, repo, git_sha, api_key, map) {
            Ok(response) => {
                println!(
                    "  {} {} ({})",
                    "ok".green(),
                    map.candidate.file_name,
                    format_bytes(response.data.file_size),
                );
                return MapOutcome::Success;
            }
            Err(err) if err.retryable && attempt < MAX_ATTEMPTS => {
                let retry_delay = retry_delay_for_status(
                    err.status.unwrap_or(502),
                    err.retry_after.as_deref(),
                    attempt,
                    SystemTime::now(),
                );
                eprintln!(
                    "{LOG_PREFIX}: {} retrying {} in {}ms (attempt {}/{MAX_ATTEMPTS})",
                    "warning".yellow().bold(),
                    map.candidate.file_name,
                    retry_delay.as_millis(),
                    attempt + 1,
                );
                std::thread::sleep(retry_delay);
            }
            Err(err) => {
                return MapOutcome::failed(&map.candidate, err.kind, err.message);
            }
        }
    }
    MapOutcome::failed(
        &map.candidate,
        FailureKind::Network,
        "upload failed after retries".to_owned(),
    )
}

#[derive(Debug, Serialize)]
struct SourceMapRequest<'a> {
    #[serde(rename = "gitSha")]
    git_sha: &'a str,
    #[serde(rename = "fileName")]
    file_name: &'a str,
    /// The map's repo-relative path, omitted when not resolvable (#260). The
    /// cloud resolves `sources[]` against its directory for monorepo
    /// sub-packages; an older cloud ignores the field.
    #[serde(rename = "mapPath", skip_serializing_if = "Option::is_none")]
    map_path: Option<&'a str>,
    #[serde(rename = "sourceMap")]
    source_map: &'a serde_json::Value,
}

#[derive(Debug, Deserialize)]
struct SourceMapUploadEnvelope {
    data: SourceMapUploadData,
}

#[derive(Debug, Deserialize)]
struct SourceMapUploadData {
    #[serde(rename = "fileSize")]
    file_size: u64,
}

#[derive(Debug)]
struct UploadAttemptError {
    message: String,
    retryable: bool,
    status: Option<u16>,
    retry_after: Option<String>,
    kind: FailureKind,
}

fn send_source_map(
    agent: &ureq::Agent,
    endpoint_override: Option<&str>,
    repo: &str,
    git_sha: &str,
    api_key: &str,
    map: &PreparedSourceMap,
) -> Result<SourceMapUploadEnvelope, UploadAttemptError> {
    let url = endpoint_url(endpoint_override, repo);
    let payload = SourceMapRequest {
        git_sha,
        file_name: &map.candidate.file_name,
        map_path: map.candidate.map_path.as_deref(),
        source_map: &map.source_map,
    };
    let mut response = agent
        .post(&url)
        .header("Authorization", &format!("Bearer {api_key}"))
        .send_json(&payload)
        .map_err(|err| UploadAttemptError {
            message: sanitize_network_error(&format!("network error: {err}")),
            retryable: true,
            status: Some(502),
            retry_after: None,
            kind: FailureKind::Network,
        })?;

    let status = response.status().as_u16();
    if matches!(status, 200 | 201) {
        return response
            .read_json::<SourceMapUploadEnvelope>()
            .map_err(|err| UploadAttemptError {
                message: format!("malformed response body: {err}"),
                retryable: false,
                status: None,
                retry_after: None,
                kind: FailureKind::Http,
            });
    }

    let retry_after = response
        .headers()
        .get("Retry-After")
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let body = response.body_mut().read_to_string().unwrap_or_default();
    Err(UploadAttemptError {
        message: classify_http_error(status, &body),
        retryable: should_retry_status(status),
        status: Some(status),
        retry_after,
        kind: FailureKind::Http,
    })
}

fn endpoint_url(override_endpoint: Option<&str>, repo: &str) -> String {
    let path = format!("/v1/coverage/{}/source-maps", url_encode_path_segment(repo));
    match override_endpoint {
        Some(base) => format!("{}{path}", base.trim().trim_end_matches('/')),
        None => api_url(&path),
    }
}

fn display_endpoint_url(override_endpoint: Option<&str>, repo: &str) -> String {
    let base = override_endpoint.map_or_else(
        || {
            std::env::var("FALLOW_API_URL")
                .ok()
                .filter(|value| !value.trim().is_empty())
                .map_or_else(
                    || DEFAULT_ENDPOINT.to_owned(),
                    |value| value.trim().trim_end_matches('/').to_owned(),
                )
        },
        |value| value.trim().trim_end_matches('/').to_owned(),
    );
    format!(
        "{base}/v1/coverage/{}/source-maps",
        url_encode_path_segment(repo)
    )
}

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

fn classify_http_error(status: u16, body: &str) -> String {
    let envelope = parse_error_envelope(body);
    match status {
        401 | 403 => "authentication failed: invalid or expired API key".to_owned(),
        429 => "rate limited; retry with fewer concurrent uploads via --concurrency".to_owned(),
        500..=599 => {
            let suffix = source_map_message_suffix(body, &envelope);
            format!("server error: {status}{suffix}")
        }
        _ => {
            let suffix = source_map_message_suffix(body, &envelope);
            format!("server rejected: HTTP {status}{suffix}")
        }
    }
}

fn source_map_message_suffix(body: &str, envelope: &crate::api::ParsedErrorEnvelope) -> String {
    response_message_suffix(body, envelope)
        .strip_prefix(':')
        .map_or_else(String::new, ToOwned::to_owned)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FailureKind {
    Validation,
    Network,
    Http,
}

#[derive(Debug, Clone)]
enum MapOutcome {
    Ready(PreparedSourceMap),
    Success,
    Failed {
        file_name: String,
        reason: String,
        kind: FailureKind,
    },
}

impl MapOutcome {
    fn failed(candidate: &SourceMapCandidate, kind: FailureKind, reason: String) -> Self {
        Self::Failed {
            file_name: candidate.file_name.clone(),
            reason,
            kind,
        }
    }

    const fn is_success(&self) -> bool {
        matches!(self, Self::Success)
    }

    const fn failure_kind(&self) -> Option<FailureKind> {
        match self {
            Self::Failed { kind, .. } => Some(*kind),
            Self::Ready(_) | Self::Success => None,
        }
    }

    fn failure_reason(&self) -> Option<&str> {
        match self {
            Self::Failed { reason, .. } => Some(reason),
            Self::Ready(_) | Self::Success => None,
        }
    }
}

fn print_failure_summary(outcomes: &[MapOutcome]) {
    let total = outcomes.len();
    let success_count = outcomes
        .iter()
        .filter(|outcome| outcome.is_success())
        .count();
    eprintln!("{LOG_PREFIX}: {success_count}/{total} uploaded");
    eprintln!("{LOG_PREFIX}: failed:");
    for outcome in outcomes {
        if let MapOutcome::Failed {
            file_name, reason, ..
        } = outcome
        {
            eprintln!("  {} {file_name} ({reason})", "x".red());
        }
    }
    eprintln!("{LOG_PREFIX}: re-run to retry failed uploads");
}

fn print_dry_run(
    repo: &str,
    git_sha: &str,
    endpoint_override: Option<&str>,
    maps: &[SourceMapCandidate],
) {
    let total_bytes: u64 = maps.iter().map(|map| map.bytes).sum();
    println!("{LOG_PREFIX}: repo={repo} sha={git_sha}");
    println!(
        "{LOG_PREFIX}: would upload {} maps ({}) to {}",
        maps.len(),
        format_bytes(total_bytes),
        display_endpoint_url(endpoint_override, repo)
    );
    for map in maps.iter().take(20) {
        let map_path = map.map_path.as_deref().unwrap_or("-");
        println!(
            "  - {} ({}) -> fileName={} mapPath={}",
            map.rel_path.display(),
            format_bytes(map.bytes),
            map.file_name,
            map_path
        );
    }
    if maps.len() > 20 {
        println!("  ... and {} more", maps.len() - 20);
    }
    println!("{LOG_PREFIX}: dry run, no uploads performed");
}

#[expect(
    clippy::cast_precision_loss,
    reason = "source map byte sizes are well under f64 precision loss range"
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

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn parses_repo_name_from_common_urls() {
        assert_eq!(
            parse_repo_name_from_url("https://github.com/acme/widgets.git"),
            Some("acme/widgets".to_owned())
        );
        assert_eq!(
            parse_repo_name_from_url("git@github.com:acme/widgets.git"),
            Some("acme/widgets".to_owned())
        );
        assert_eq!(
            parse_repo_name_from_url("ssh://git@gitlab.com/acme/team/widgets"),
            Some("team/widgets".to_owned())
        );
        assert_eq!(
            parse_repo_name_from_url("acme/widgets"),
            Some("acme/widgets".to_owned())
        );
    }

    #[test]
    fn validates_git_sha_like_server_schema() {
        assert!(validate_git_sha("abcdef1").is_ok());
        assert!(validate_git_sha("abcdef1234567890abcdef1234567890abcdef12").is_ok());
        assert!(validate_git_sha("abc").is_err());
        assert!(validate_git_sha("xyz1234").is_err());
        assert!(validate_git_sha("abcdef1234567890abcdef1234567890abcdef123").is_err());
    }

    #[test]
    fn map_file_name_strips_path_by_default() {
        assert_eq!(
            map_file_name(Path::new("assets/bundle-a1b2.js.map"), true).unwrap(),
            "bundle-a1b2.js.map"
        );
    }

    #[test]
    fn map_file_name_keeps_relative_path_when_requested() {
        assert_eq!(
            map_file_name(Path::new("assets/bundle.js.map"), false).unwrap(),
            "assets/bundle.js.map"
        );
    }

    #[test]
    fn file_name_rejects_traversal_and_backslashes() {
        assert!(validate_file_name("../bundle.js.map").is_err());
        assert!(validate_file_name("assets/../bundle.js.map").is_err());
        assert!(validate_file_name("assets\\bundle.js.map").is_err());
    }

    #[test]
    fn collect_source_maps_applies_include_exclude_and_file_name_mode() {
        let dir = tempdir().expect("tempdir");
        std::fs::create_dir_all(dir.path().join("assets")).expect("assets dir");
        std::fs::create_dir_all(dir.path().join("node_modules/pkg")).expect("node_modules dir");
        std::fs::write(dir.path().join("root.js.map"), "{}").expect("root map");
        std::fs::write(dir.path().join("assets/app.js.map"), "{}").expect("asset map");
        std::fs::write(dir.path().join("node_modules/pkg/vendor.js.map"), "{}")
            .expect("vendor map");

        let include = compile_glob_set(&["**/*.map".to_owned()], "--include").unwrap();
        let exclude = compile_glob_set(&["**/node_modules/**".to_owned()], "--exclude").unwrap();
        let maps = collect_source_maps(dir.path(), dir.path(), &include, &exclude, false).unwrap();

        let file_names: Vec<&str> = maps.iter().map(|map| map.file_name.as_str()).collect();
        assert_eq!(file_names, vec!["assets/app.js.map", "root.js.map"]);
    }

    #[test]
    fn resolve_build_dir_joins_relative_paths() {
        let root = Path::new("/repo");
        assert_eq!(
            resolve_build_dir(root, Path::new("dist")),
            PathBuf::from("/repo/dist")
        );
        assert_eq!(
            resolve_build_dir(root, Path::new("/tmp/dist")),
            PathBuf::from("/tmp/dist")
        );
    }

    #[test]
    fn map_path_is_repo_root_relative_when_build_dir_is_a_subdirectory() {
        let repo_root = tempdir().expect("tempdir");
        let build_dir = repo_root.path().join("dashboard/dist");
        std::fs::create_dir_all(build_dir.join("assets")).expect("assets dir");
        std::fs::write(build_dir.join("assets/app-a1b2.js.map"), "{}").expect("map");

        let include = compile_glob_set(&["**/*.map".to_owned()], "--include").unwrap();
        let exclude = compile_glob_set(&["**/node_modules/**".to_owned()], "--exclude").unwrap();
        let maps =
            collect_source_maps(repo_root.path(), &build_dir, &include, &exclude, true).unwrap();

        assert_eq!(maps.len(), 1);
        assert_eq!(maps[0].file_name, "app-a1b2.js.map");
        assert_eq!(
            maps[0].map_path.as_deref(),
            Some("dashboard/dist/assets/app-a1b2.js.map")
        );
    }

    #[test]
    fn repo_relative_map_path_is_none_for_a_map_outside_the_repo_root() {
        let repo_root = tempdir().expect("repo root");
        let elsewhere = tempdir().expect("elsewhere");
        let outside = elsewhere.path().join("app.js.map");
        assert_eq!(repo_relative_map_path(repo_root.path(), &outside), None);
    }

    #[test]
    fn request_serializes_map_path_and_omits_it_when_absent() {
        let source_map = serde_json::json!({ "version": 3, "sources": [], "mappings": "" });
        let with_path = SourceMapRequest {
            git_sha: "abcdef1",
            file_name: "app.js.map",
            map_path: Some("dashboard/dist/assets/app.js.map"),
            source_map: &source_map,
        };
        let json = serde_json::to_string(&with_path).unwrap();
        assert!(json.contains(r#""mapPath":"dashboard/dist/assets/app.js.map""#));

        let without_path = SourceMapRequest {
            git_sha: "abcdef1",
            file_name: "app.js.map",
            map_path: None,
            source_map: &source_map,
        };
        let json = serde_json::to_string(&without_path).unwrap();
        assert!(!json.contains("mapPath"));
    }

    #[test]
    fn endpoint_url_encodes_repo_as_one_segment() {
        assert_eq!(
            endpoint_url(Some("http://localhost:3000"), "owner/repo"),
            "http://localhost:3000/v1/coverage/owner%2Frepo/source-maps"
        );
    }

    #[test]
    fn classify_http_errors_matches_spec_messages() {
        assert_eq!(
            classify_http_error(401, ""),
            "authentication failed: invalid or expired API key"
        );
        assert_eq!(
            classify_http_error(429, ""),
            "rate limited; retry with fewer concurrent uploads via --concurrency"
        );
        assert!(classify_http_error(500, "oops").starts_with("server error: 500"));
    }

    #[test]
    fn classify_http_error_preserves_malformed_response_body() {
        let message = classify_http_error(500, "<html>bad gateway</html>");
        assert!(message.contains("<html>bad gateway</html>"));
        assert!(message.contains("malformed error envelope"));
    }

    #[test]
    fn all_network_failures_are_reported_as_network_exit() {
        let candidate = SourceMapCandidate {
            path: PathBuf::from("dist/app.js.map"),
            rel_path: PathBuf::from("dist/app.js.map"),
            file_name: "dist/app.js.map".to_owned(),
            map_path: Some("dist/app.js.map".to_owned()),
            bytes: 10,
        };
        let outcomes = [MapOutcome::failed(
            &candidate,
            FailureKind::Network,
            "network error: connection refused".to_owned(),
        )];
        let success_count = outcomes
            .iter()
            .filter(|outcome| outcome.is_success())
            .count();
        assert_eq!(success_count, 0);
        assert!(
            outcomes
                .iter()
                .filter_map(MapOutcome::failure_kind)
                .all(|kind| kind == FailureKind::Network)
        );
        assert_eq!(
            outcomes.iter().find_map(MapOutcome::failure_reason),
            Some("network error: connection refused")
        );
    }

    fn candidate(bytes: u64, path: PathBuf) -> SourceMapCandidate {
        SourceMapCandidate {
            rel_path: PathBuf::from("app.js.map"),
            file_name: "app.js.map".to_owned(),
            map_path: Some("app.js.map".to_owned()),
            path,
            bytes,
        }
    }

    fn dry_run_args(dir: &Path) -> UploadSourceMapsArgs {
        UploadSourceMapsArgs {
            dir: dir.to_path_buf(),
            include: "**/*.map".to_owned(),
            exclude: Vec::new(),
            repo: Some("acme/widgets".to_owned()),
            git_sha: Some("abcdef1".to_owned()),
            endpoint: Some("http://localhost:3000".to_owned()),
            strip_path: true,
            dry_run: true,
            concurrency: 4,
            fail_fast: false,
        }
    }

    #[test]
    fn into_exit_maps_each_variant_to_its_exit_code() {
        assert_eq!(
            UploadSourceMapsError::Validation("x".to_owned()).into_exit(),
            ExitCode::from(2)
        );
        assert_eq!(
            UploadSourceMapsError::Network("x".to_owned()).into_exit(),
            ExitCode::from(NETWORK_EXIT_CODE)
        );
        let failed = MapOutcome::failed(
            &candidate(10, PathBuf::from("app.js.map")),
            FailureKind::Http,
            "server rejected".to_owned(),
        );
        assert_eq!(
            UploadSourceMapsError::Partial(vec![failed]).into_exit(),
            ExitCode::from(1)
        );
    }

    #[test]
    fn run_dry_run_on_temp_build_dir_exits_zero() {
        let dir = tempdir().expect("tempdir");
        std::fs::write(dir.path().join("app.js.map"), "{}").expect("map");
        // Explicit repo + git_sha + endpoint keep the dry run env- and git-free.
        let code = run(&dry_run_args(dir.path()), dir.path());
        assert_eq!(code, ExitCode::SUCCESS);
    }

    #[test]
    fn run_reports_missing_directory_as_validation_exit_2() {
        let dir = tempdir().expect("tempdir");
        let missing = dir.path().join("does-not-exist");
        let code = run(&dry_run_args(&missing), dir.path());
        assert_eq!(code, ExitCode::from(2));
    }

    #[test]
    fn run_reports_no_maps_as_validation_exit_2() {
        let dir = tempdir().expect("tempdir");
        // Directory exists but holds no .map files.
        let code = run(&dry_run_args(dir.path()), dir.path());
        assert_eq!(code, ExitCode::from(2));
    }

    #[test]
    fn validate_repo_name_accepts_clean_and_rejects_unsafe() {
        assert_eq!(validate_repo_name("acme/widgets").unwrap(), "acme/widgets");
        assert!(validate_repo_name("").is_err());
        assert!(validate_repo_name("acme/../widgets").is_err());
        assert!(validate_repo_name("acme\\widgets").is_err());
    }

    #[test]
    fn take_last_two_segments_needs_two_nonempty_segments() {
        assert_eq!(take_last_two_segments("widgets"), None);
        assert_eq!(
            take_last_two_segments("acme/widgets"),
            Some("acme/widgets".to_owned())
        );
        // Trailing slashes and empty interior segments are ignored.
        assert_eq!(
            take_last_two_segments("group/acme/widgets/"),
            Some("acme/widgets".to_owned())
        );
    }

    #[test]
    fn resolve_repo_name_reads_package_json_repository_url() {
        let dir = tempdir().expect("tempdir");
        std::fs::write(
            dir.path().join("package.json"),
            r#"{"name":"w","repository":{"url":"https://github.com/acme/widgets.git"}}"#,
        )
        .expect("package.json");
        assert_eq!(resolve_repo_name(None, dir.path()).unwrap(), "acme/widgets");
    }

    #[test]
    fn resolve_repo_name_reads_package_json_repository_string() {
        let dir = tempdir().expect("tempdir");
        std::fs::write(
            dir.path().join("package.json"),
            r#"{"name":"w","repository":"git@github.com:acme/widgets.git"}"#,
        )
        .expect("package.json");

        assert_eq!(resolve_repo_name(None, dir.path()).unwrap(), "acme/widgets");
    }

    #[test]
    fn resolve_repo_name_errors_when_no_source_is_available() {
        let dir = tempdir().expect("tempdir");
        let err = resolve_repo_name(None, dir.path()).expect_err("repo should be required");

        assert!(matches!(err, UploadSourceMapsError::Validation(_)));
    }

    #[test]
    fn prepare_source_map_classifies_size_json_and_read_failures() {
        let dir = tempdir().expect("tempdir");

        // Oversize is rejected before the file is even read.
        let too_big = candidate(MAX_MAP_BYTES + 1, dir.path().join("big.js.map"));
        assert!(matches!(
            prepare_source_map(&too_big),
            MapOutcome::Failed { kind: FailureKind::Validation, ref reason, .. } if reason.contains("too large")
        ));

        // Valid JSON parses into a Ready outcome.
        let ok_path = dir.path().join("ok.js.map");
        std::fs::write(&ok_path, r#"{"version":3,"sources":[],"mappings":""}"#).expect("ok map");
        assert!(matches!(
            prepare_source_map(&candidate(40, ok_path)),
            MapOutcome::Ready(_)
        ));

        // Non-JSON content is a validation failure.
        let bad_path = dir.path().join("bad.js.map");
        std::fs::write(&bad_path, "not json at all").expect("bad map");
        assert!(matches!(
            prepare_source_map(&candidate(15, bad_path)),
            MapOutcome::Failed { kind: FailureKind::Validation, ref reason, .. } if reason.contains("not valid JSON")
        ));

        // A missing file is a read failure (also a validation failure).
        let missing = candidate(10, dir.path().join("missing.js.map"));
        assert!(matches!(
            prepare_source_map(&missing),
            MapOutcome::Failed { kind: FailureKind::Validation, ref reason, .. } if reason.contains("read failed")
        ));
    }

    #[test]
    fn url_encode_path_segment_percent_encodes_reserved_bytes() {
        assert_eq!(url_encode_path_segment("owner/repo"), "owner%2Frepo");
        // Unreserved characters pass through unchanged.
        assert_eq!(url_encode_path_segment("a-b_c.d~e"), "a-b_c.d~e");
        assert_eq!(url_encode_path_segment("a b"), "a%20b");
    }

    #[test]
    fn display_endpoint_url_uses_override_and_trims_trailing_slash() {
        assert_eq!(
            display_endpoint_url(Some("http://localhost:3000/"), "owner/repo"),
            "http://localhost:3000/v1/coverage/owner%2Frepo/source-maps"
        );
    }

    #[test]
    fn format_bytes_scales_through_units() {
        assert_eq!(format_bytes(512), "512 B");
        assert_eq!(format_bytes(2 * 1024), "2 KiB");
        assert_eq!(format_bytes(5 * 1024 * 1024), "5.0 MiB");
        assert_eq!(format_bytes(3 * 1024 * 1024 * 1024), "3.0 GiB");
    }

    #[test]
    fn to_posix_string_normalizes_separators() {
        assert_eq!(to_posix_string(Path::new("a/b/c.map")), "a/b/c.map");
    }

    #[test]
    fn compile_glob_set_rejects_an_invalid_pattern() {
        let err = compile_glob_set(&["a[b".to_owned()], "--include")
            .expect_err("an unterminated character class must be rejected");
        assert!(matches!(err, UploadSourceMapsError::Validation(_)));
    }
}
