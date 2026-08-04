use std::fmt::Write as _;
use std::path::Path;
use std::process::Command;

use fallow_config::{FallowConfig, OutputFormat, ResolvedConfig};
use fallow_engine::changed_files::clear_ambient_git_env;

use crate::api::api_url;

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

pub(super) fn take_last_two_segments(path: &str) -> Option<String> {
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

pub(super) fn resolve_api_key(explicit: Option<&str>) -> Result<String, String> {
    if let Some(explicit) = explicit {
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
    Err(
        "no API key. Set $FALLOW_API_KEY or pass --api-key <KEY>. Generate at \
         https://fallow.cloud/settings#api-keys."
            .to_owned(),
    )
}

pub(super) fn endpoint_url(
    override_endpoint: Option<&str>,
    project_id: &str,
    path_suffix: &str,
) -> String {
    let path = format!(
        "/v1/coverage/{}/{path_suffix}",
        url_encode_path_segment(project_id)
    );
    match override_endpoint {
        Some(base) => format!("{}{path}", base.trim().trim_end_matches('/')),
        None => api_url(&path),
    }
}

/// URL-encode a single URL path segment (RFC 3986 unreserved set).
///
/// Project IDs can be bare (`fallow-cloud-api`) or slash-scoped
/// (`acme/widgets`), but the server receives them as a single percent-encoded
/// segment under `/v1/coverage/{repo}/...`, so `/` must be encoded too.
#[expect(
    clippy::expect_used,
    reason = "formatting percent-encoded bytes into String is infallible"
)]
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

#[cfg(test)]
pub(super) fn load_resolved_config(root: &Path) -> Result<ResolvedConfig, String> {
    load_resolved_config_with_options(root, false)
}

pub(super) fn load_resolved_config_with_options(
    root: &Path,
    allow_remote_extends: bool,
) -> Result<ResolvedConfig, String> {
    let user_config = match FallowConfig::find_and_load_with_options(
        root,
        fallow_config::ConfigLoadOptions {
            allow_remote_extends,
        },
    ) {
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

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(
            parse_git_remote_to_project_id("ssh://git@gitlab.com/group/subgroup/repo.git"),
            Some("subgroup/repo".to_owned())
        );
    }

    #[test]
    fn parse_git_remote_rejects_incomplete_urls() {
        assert_eq!(parse_git_remote_to_project_id("https://example.com/"), None);
        assert_eq!(parse_git_remote_to_project_id(""), None);
        assert_eq!(parse_git_remote_to_project_id("not-a-remote"), None);
        assert_eq!(parse_git_remote_to_project_id("git@github.com:owner"), None);
        assert_eq!(parse_git_remote_to_project_id("https://github.com"), None);
        assert_eq!(parse_git_remote_to_project_id("not a remote"), None);
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
    fn url_encode_path_segment_passthrough_for_unreserved_chars() {
        assert_eq!(
            url_encode_path_segment("abc-123_foo.bar~"),
            "abc-123_foo.bar~"
        );
        assert_eq!(url_encode_path_segment("a-b_c.d~e"), "a-b_c.d~e");
    }

    #[test]
    fn url_encode_path_segment_encodes_reserved_bytes() {
        assert_eq!(url_encode_path_segment("/"), "%2F");
        assert_eq!(url_encode_path_segment("@"), "%40");
        assert_eq!(url_encode_path_segment("a b"), "a%20b");
        assert_eq!(
            url_encode_path_segment("fallow-rs/fallow"),
            "fallow-rs%2Ffallow"
        );
        assert_eq!(url_encode_path_segment("a/b@c"), "a%2Fb%40c");
    }

    #[test]
    fn url_encode_path_segment_empty_string_returns_empty() {
        assert_eq!(url_encode_path_segment(""), "");
    }

    #[test]
    fn url_encode_path_segment_percent_encodes_utf8() {
        assert_eq!(url_encode_path_segment("caf\u{e9}"), "caf%C3%A9");
    }

    #[test]
    fn endpoint_url_uses_override_and_encodes_project_id() {
        assert_eq!(
            endpoint_url(Some("http://127.0.0.1:3000"), "a/b", "inventory"),
            "http://127.0.0.1:3000/v1/coverage/a%2Fb/inventory"
        );
        assert_eq!(
            endpoint_url(Some("http://127.0.0.1:3000/"), "a/b", "static-findings"),
            "http://127.0.0.1:3000/v1/coverage/a%2Fb/static-findings"
        );
        assert_eq!(
            endpoint_url(Some("http://localhost:3000"), "owner/repo", "source-maps"),
            "http://localhost:3000/v1/coverage/owner%2Frepo/source-maps"
        );
    }

    #[test]
    fn resolve_api_key_trims_explicit_value() {
        assert_eq!(
            resolve_api_key(Some("  fallow_key_123  ")).unwrap(),
            "fallow_key_123"
        );
    }
}
