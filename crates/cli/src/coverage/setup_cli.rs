//! Clap flags of `fallow coverage setup` and their mapping to a subcommand.

use clap::Args;

use super::{CloudSetupArgs, CoverageSubcommand, SetupArgs};

/// Flags of `fallow coverage setup`.
#[derive(Args, Debug, Clone, Default)]
pub struct CoverageSetupCli {
    /// Accept all prompts automatically.
    #[arg(short = 'y', long)]
    pub yes: bool,

    /// Print instructions instead of prompting.
    #[arg(long)]
    pub non_interactive: bool,

    /// Emit deterministic setup instructions as JSON. Implies --non-interactive.
    #[arg(long)]
    pub json: bool,

    /// Plan the Fallow Cloud setup: beacon snippets with the deployed
    /// commit SHA, and CI steps that upload the inventory and source maps
    /// and register the deployment for that SHA. Writes nothing.
    #[arg(long, conflicts_with_all = ["yes", "non_interactive"])]
    pub cloud: bool,

    /// With --cloud: poll Fallow Cloud until every setup step is done.
    /// Reads `$FALLOW_API_KEY`. Exit 0 when done, 1 on timeout.
    #[arg(long, requires = "cloud")]
    pub wait: bool,

    /// With --wait: the longest wait in seconds.
    #[arg(long, value_name = "SECS", default_value_t = 600, requires = "wait")]
    pub wait_timeout: u64,

    /// With --cloud: project identifier (`owner/repo`). Default:
    /// $GITHUB_REPOSITORY, then $CI_PROJECT_PATH, then the git remote.
    #[arg(long, value_name = "PROJECT_ID", requires = "cloud")]
    pub project_id: Option<String>,

    /// With --cloud: the deployed commit SHA that --wait checks.
    /// Default: `git rev-parse HEAD`.
    #[arg(long, value_name = "SHA", requires = "cloud")]
    pub git_sha: Option<String>,

    /// With --wait: Fallow Cloud API key. Precedence: this flag > $FALLOW_API_KEY.
    #[arg(long, value_name = "KEY", requires = "wait")]
    pub api_key: Option<String>,

    /// With --cloud: Fallow Cloud API base URL. Precedence: this flag >
    /// $FALLOW_API_URL > the default Fallow Cloud URL.
    #[arg(long, value_name = "URL", requires = "cloud")]
    pub api_endpoint: Option<String>,
}

impl CoverageSetupCli {
    /// The local setup flow, or the Fallow Cloud plan with `--cloud`.
    pub fn to_subcommand(&self, explain: bool) -> CoverageSubcommand {
        if !self.cloud {
            return CoverageSubcommand::Setup(SetupArgs {
                yes: self.yes,
                non_interactive: self.non_interactive || self.json,
                json: self.json,
                explain,
            });
        }
        CoverageSubcommand::SetupCloud(CloudSetupArgs {
            json: self.json,
            explain,
            wait: self.wait,
            wait_timeout_secs: self.wait_timeout,
            project_id: self.project_id.clone(),
            git_sha: self.git_sha.clone(),
            api_key: self.api_key.clone(),
            api_endpoint: self.api_endpoint.clone(),
        })
    }
}
