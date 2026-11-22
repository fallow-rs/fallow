//! The load of the existing review comments of a pull or merge request, and
//! the split of its failures into setup errors and lookup errors.

use fallow_output::ReviewId;

use super::*;

/// Why the provider review state did not load.
#[derive(Debug)]
pub(super) enum ProviderStateError {
    /// A required target, repository, token or HTTP client is missing.
    Setup(String),
    /// The provider API failed or returned state that fallow cannot read, so
    /// fallow cannot tell which comments already exist.
    Lookup(String),
}

impl ProviderStateError {
    /// The stable error code of a lookup failure. The GitHub Action and the
    /// GitLab template read it to set their dedup lookup outputs.
    pub(super) const LOOKUP_CODE: &'static str = "FALLOW_CI_DEDUP_LOOKUP_FAILED";

    pub(super) fn message(&self) -> &str {
        match self {
            Self::Setup(message) | Self::Lookup(message) => message,
        }
    }

    pub(super) fn code(&self) -> Option<&'static str> {
        matches!(self, Self::Lookup(_)).then_some(Self::LOOKUP_CODE)
    }
}

/// Load existing provider review state (comments + threads/discussions) for the
/// reconcile plan, dispatching to the GitHub or GitLab loader.
pub(super) fn load_provider_state(
    provider: CiProvider,
    target: Option<&str>,
    opts: ReconcileOptions<'_>,
) -> Result<ProviderState, ProviderStateError> {
    match provider {
        CiProvider::Github => load_github_state(target, opts),
        CiProvider::Gitlab => load_gitlab_state(target, opts),
    }
}

fn load_github_state(
    target: Option<&str>,
    opts: ReconcileOptions<'_>,
) -> Result<ProviderState, ProviderStateError> {
    let bot_login_var = std::env::var("FALLOW_BOT_LOGIN");
    let bot_login = BotLogin::from_var(&bot_login_var);
    let pr = require_target("GitHub pull request", target).map_err(ProviderStateError::Setup)?;
    let repo = opts
        .repo
        .map(str::to_owned)
        .or_else(|| std::env::var("GH_REPO").ok())
        .or_else(|| std::env::var("GITHUB_REPOSITORY").ok())
        .ok_or_else(|| {
            ProviderStateError::Setup(
                "GitHub reconciliation requires --repo, GH_REPO, or GITHUB_REPOSITORY".to_owned(),
            )
        })?;
    let token = github_token().map_err(ProviderStateError::Setup)?;
    let api = opts
        .api_url
        .unwrap_or("https://api.github.com")
        .trim_end_matches('/');
    let agent = try_api_agent().map_err(|err| ProviderStateError::Setup(err.to_string()))?;
    let connection = GithubConnection {
        agent: &agent,
        repo: &repo,
        pr,
        token: &token,
        api,
    };
    fetch_github_state(&connection, opts.review_id, bot_login).map_err(ProviderStateError::Lookup)
}

fn fetch_github_state(
    connection: &GithubConnection<'_>,
    review_id: Option<&ReviewId>,
    bot_login: BotLogin<'_>,
) -> Result<ProviderState, String> {
    let GithubConnection {
        agent,
        repo,
        pr,
        token,
        api,
    } = *connection;
    let mut state = ProviderState::default();
    let mut review_comments = Vec::new();

    for page in 1..=100 {
        let url = format!(
            "{api}/repos/{repo}/pulls/{pr}/comments?per_page=100&page={page}&sort=created&direction=asc"
        );
        let value = github_get_json(agent, &url, token)?;
        let comments = value
            .as_array()
            .ok_or_else(|| "GitHub review comments response was not an array".to_owned())?;
        if comments.is_empty() {
            break;
        }
        review_comments.extend(comments.iter().cloned());
        if comments.len() < 100 {
            break;
        }
        if page == 100 {
            return Err(
                "GitHub review comments pagination exceeded 100 pages; refusing partial provider state"
                    .to_owned(),
            );
        }
    }

    // Root comments establish lifecycle identity. Replies are processed only
    // after every page is loaded so their markers can be tied to a root even
    // if provider ordering or pagination places the reply first.
    for (position, comment) in review_comments.iter().enumerate() {
        record_github_review_root(&mut state, comment, review_id, position, bot_login)?;
    }
    for (position, comment) in review_comments.iter().enumerate() {
        record_github_resolution_marker(&mut state, comment, review_id, position, bot_login)?;
    }

    load_github_review_threads(&mut state, *connection, review_id)?;
    finalize_github_state(&mut state);
    Ok(state)
}

fn load_gitlab_state(
    target: Option<&str>,
    opts: ReconcileOptions<'_>,
) -> Result<ProviderState, ProviderStateError> {
    let bot_login_var = std::env::var("FALLOW_BOT_LOGIN");
    let bot_login = BotLogin::from_var(&bot_login_var);
    let mr = require_target("GitLab merge request", target).map_err(ProviderStateError::Setup)?;
    let project_id = opts
        .project_id
        .map(str::to_owned)
        .or_else(|| std::env::var("CI_PROJECT_ID").ok())
        .ok_or_else(|| {
            ProviderStateError::Setup(
                "GitLab reconciliation requires --project-id or CI_PROJECT_ID".to_owned(),
            )
        })?;
    let token = std::env::var("GITLAB_TOKEN").map_err(|_| {
        ProviderStateError::Setup("GitLab reconciliation requires GITLAB_TOKEN".to_owned())
    })?;
    let api = opts
        .api_url
        .map(str::to_owned)
        .or_else(|| std::env::var("CI_API_V4_URL").ok())
        .unwrap_or_else(|| "https://gitlab.com/api/v4".to_owned());
    let api = api.trim_end_matches('/').to_owned();
    let agent = try_api_agent().map_err(|err| ProviderStateError::Setup(err.to_string()))?;
    fetch_gitlab_state(
        &agent,
        &api,
        &project_id,
        mr,
        &token,
        opts.review_id,
        bot_login,
    )
    .map_err(ProviderStateError::Lookup)
}

#[expect(
    clippy::too_many_arguments,
    reason = "the GitLab lookup takes the resolved connection values one by one"
)]
fn fetch_gitlab_state(
    agent: &ureq::Agent,
    api: &str,
    project_id: &str,
    mr: &str,
    token: &str,
    review_id: Option<&ReviewId>,
    bot_login: BotLogin<'_>,
) -> Result<ProviderState, String> {
    let mut state = ProviderState::default();
    let mut all_discussions = Vec::new();

    for page in 1..=100 {
        let url = format!(
            "{api}/projects/{}/merge_requests/{mr}/discussions?per_page={GITLAB_PER_PAGE}&page={page}",
            url_encode_path_segment(project_id)
        );
        let (value, next_page) = gitlab_get_page(agent, &url, token)?;
        let discussions = value
            .as_array()
            .ok_or_else(|| "GitLab discussions response was not an array".to_owned())?;
        all_discussions.extend(discussions.iter().cloned());
        if !gitlab_has_more_pages(next_page, discussions.len()) {
            break;
        }
        if page == 100 {
            return Err(
                "GitLab discussions pagination exceeded 100 pages; refusing partial provider state"
                    .to_owned(),
            );
        }
    }
    let authenticated_user = if needs_gitlab_authenticated_user(&all_discussions, bot_login) {
        Some(load_gitlab_authenticated_user(agent, api, token)?)
    } else {
        None
    };
    for discussion in &all_discussions {
        collect_gitlab_discussion_fingerprints(
            &mut state,
            discussion,
            review_id,
            authenticated_user.as_ref(),
            bot_login,
        )?;
    }
    Ok(state)
}
