use std::path::Path;
use std::process::Command;

use fallow_config::{FallowConfig, OutputFormat, ResolvedConfig};
use fallow_engine::clear_ambient_git_env;

pub(super) const GIT_SHA_MAX_LEN: usize = 64;

pub(super) fn resolve_project_id(
    explicit_project_id: Option<&str>,
    root: &Path,
) -> Result<String, String> {
    if let Some(explicit) = explicit_project_id {
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
    Err(
        "could not determine project id. Pass --project-id <project-id>, or set \
         $GITHUB_REPOSITORY / $CI_PROJECT_PATH, or ensure `git remote get-url origin` \
         returns a recognizable URL."
            .to_owned(),
    )
}

/// Validate the project identifier used as the `{repo}` URL segment.
///
/// The server accepts any non-empty string without path-traversal, whether
/// bare (`fallow-cloud-api`) or slash-scoped (`acme/widgets`). Keep validation
/// minimal: reject only what the server or filesystem would reject (empty,
/// `..`).
pub(super) fn validate_project_id(id: &str) -> Result<&str, String> {
    if id.is_empty() {
        return Err("project id is empty".to_owned());
    }
    if id.contains("..") {
        return Err("project id must not contain '..' path segments".to_owned());
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
pub(super) fn parse_git_remote_to_project_id(url: &str) -> Option<String> {
    let stripped_suffix = url.trim().trim_end_matches(".git");
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
    let mut parts: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    if parts.len() < 2 {
        return None;
    }
    let repo = parts.pop()?;
    let owner = parts.pop()?;
    Some(format!("{owner}/{repo}"))
}

pub(super) fn resolve_git_sha(
    explicit_git_sha: Option<&str>,
    root: &Path,
) -> Result<String, String> {
    let sha = if let Some(explicit) = explicit_git_sha {
        explicit.trim().to_owned()
    } else {
        let mut command = Command::new("git");
        command.args(["rev-parse", "HEAD"]).current_dir(root);
        clear_ambient_git_env(&mut command);
        let output = command.output().map_err(|err| {
            format!("could not resolve git SHA: {err}. Pass --git-sha <sha> explicitly.")
        })?;
        if !output.status.success() {
            return Err("`git rev-parse HEAD` failed. Pass --git-sha <sha> explicitly.".to_owned());
        }
        String::from_utf8_lossy(&output.stdout).trim().to_owned()
    };

    if sha.is_empty() {
        return Err("git sha is empty".to_owned());
    }
    if sha.len() > GIT_SHA_MAX_LEN {
        return Err(format!(
            "git sha is {} chars, server limit is {}",
            sha.len(),
            GIT_SHA_MAX_LEN
        ));
    }
    if !sha
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
    {
        return Err(format!(
            "git sha '{sha}' contains characters outside [A-Za-z0-9._-]"
        ));
    }
    Ok(sha)
}

pub(super) fn dirty_worktree(root: &Path) -> bool {
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

pub(super) fn load_resolved_config(root: &Path) -> Result<ResolvedConfig, String> {
    let user_config = match FallowConfig::find_and_load(root) {
        Ok(Some((config, _path))) => Some(config),
        Ok(None) => None,
        Err(e) => return Err(format!("config load failed: {e}")),
    };
    let config = user_config.unwrap_or_default();
    let threads = std::thread::available_parallelism().map_or(1, std::num::NonZero::get);
    Ok(config.resolve(
        root.to_path_buf(),
        OutputFormat::Human,
        threads,
        /* no_cache */ true,
        /* quiet */ true,
        /* cache_max_size_mb */ None,
    ))
}
