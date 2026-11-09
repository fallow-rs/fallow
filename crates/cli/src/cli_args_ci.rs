//! The `fallow ci` subcommands.

use std::path::PathBuf;

use clap::Subcommand;

use super::CiProviderArg;

#[derive(Subcommand)]
pub enum CiCli {
    /// Compute the provider action for a rendered sticky PR summary comment.
    PlanPrComment {
        /// Path to the rendered PR comment Markdown body.
        #[arg(long)]
        body: PathBuf,

        /// Sticky marker id used in the rendered body.
        #[arg(long)]
        marker_id: String,

        /// Treat the rendered body as a clean no-findings result.
        #[arg(long)]
        clean: bool,

        /// Existing provider comment id, when a matching sticky comment exists.
        #[arg(long)]
        existing_comment_id: Option<String>,

        /// Path to the existing provider comment body. Enables unchanged-skip planning.
        #[arg(long)]
        existing_body: Option<PathBuf>,
    },

    /// Post, update, or skip a rendered sticky PR summary comment.
    PostPrComment {
        /// Provider whose PR comment is being posted.
        #[arg(long, value_enum)]
        provider: CiProviderArg,

        /// Pull request number (GitHub).
        #[arg(long)]
        pr: Option<String>,

        /// Merge request IID (GitLab).
        #[arg(long)]
        mr: Option<String>,

        /// Path to the rendered PR comment Markdown body.
        #[arg(long)]
        body: PathBuf,

        /// Path to the typed PR comment envelope JSON, when available.
        #[arg(long)]
        envelope: Option<PathBuf>,

        /// Sticky marker id used in the rendered body.
        #[arg(long)]
        marker_id: String,

        /// Treat the rendered body as a clean no-findings result.
        #[arg(long)]
        clean: bool,

        /// GitHub repository in owner/name form. Defaults to GH_REPO or GITHUB_REPOSITORY.
        #[arg(long)]
        repo: Option<String>,

        /// GitLab project id or path. Defaults to CI_PROJECT_ID.
        #[arg(long = "project-id")]
        project_id: Option<String>,

        /// Provider API base URL. Defaults to github.com.
        #[arg(long = "api-url")]
        api_url: Option<String>,

        /// Compute the post plan without creating or updating the provider comment.
        #[arg(long)]
        dry_run: bool,
    },

    /// Post a rendered review envelope as a provider review or summary comment.
    PostReview {
        /// Provider whose review envelope is being posted.
        #[arg(long, value_enum)]
        provider: CiProviderArg,

        /// Pull request number (GitHub).
        #[arg(long)]
        pr: Option<String>,

        /// Merge request IID (GitLab).
        #[arg(long)]
        mr: Option<String>,

        /// Path to a review-github or review-gitlab JSON envelope.
        #[arg(long)]
        envelope: PathBuf,

        /// GitHub repository in owner/name form. Defaults to GH_REPO or GITHUB_REPOSITORY.
        #[arg(long)]
        repo: Option<String>,

        /// GitLab project id or path. Defaults to CI_PROJECT_ID.
        #[arg(long = "project-id")]
        project_id: Option<String>,

        /// Provider API base URL. Defaults to github.com or CI_API_V4_URL/gitlab.com.
        #[arg(long = "api-url")]
        api_url: Option<String>,

        /// Compute the post plan without creating provider comments.
        #[arg(long)]
        dry_run: bool,
    },

    /// Post a GitHub Check Run from a typed PR decision surface.
    PostCheckRun {
        /// Provider whose check run is being posted. Only GitHub is supported.
        #[arg(long, value_enum)]
        provider: CiProviderArg,

        /// Path to a fallow-pr-decision JSON sidecar.
        #[arg(long)]
        decision: PathBuf,

        /// GitHub repository in owner/name form.
        #[arg(long)]
        repo: String,

        /// Head SHA the check run should attach to.
        #[arg(long = "head-sha")]
        head_sha: String,

        /// Provider API base URL. Defaults to github.com.
        #[arg(long = "api-url")]
        api_url: Option<String>,

        /// Post one check run per decision gate instead of one aggregate check.
        #[arg(long = "split-gates")]
        split_gates: bool,

        /// Print the check run payload without posting it.
        #[arg(long)]
        dry_run: bool,
    },

    /// Validate a rendered review envelope and compute a stable reconcile plan.
    ReconcileReview {
        /// Provider whose review envelope is being reconciled.
        #[arg(long, value_enum)]
        provider: CiProviderArg,

        /// Pull request number (GitHub).
        #[arg(long)]
        pr: Option<String>,

        /// Merge request IID (GitLab).
        #[arg(long)]
        mr: Option<String>,

        /// Path to a review-github or review-gitlab JSON envelope.
        #[arg(long)]
        envelope: PathBuf,

        /// GitHub repository in owner/name form. Defaults to GH_REPO or GITHUB_REPOSITORY.
        #[arg(long)]
        repo: Option<String>,

        /// GitLab project id or path. Defaults to CI_PROJECT_ID.
        #[arg(long = "project-id")]
        project_id: Option<String>,

        /// Provider API base URL. Defaults to github.com or CI_API_V4_URL/gitlab.com.
        #[arg(long = "api-url")]
        api_url: Option<String>,

        /// Compute the reconcile plan without posting resolution notes or resolving threads.
        #[arg(long)]
        dry_run: bool,
    },
}
