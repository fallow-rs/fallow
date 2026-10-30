//! Arguments of the Fallow Cloud account commands: `fallow cloud` and
//! `fallow license`.

use std::path::PathBuf;

#[derive(clap::Subcommand)]
pub enum CloudCli {
    /// Log in with the device flow and store a read-only API key.
    Login {
        /// Name of this machine on the key, shown in the dashboard. Default:
        /// the host name.
        #[arg(long, value_name = "NAME")]
        machine_name: Option<String>,

        /// Fallow Cloud API base URL. Default: $FALLOW_API_URL, else
        /// <https://api.fallow.cloud>.
        #[arg(long, value_name = "URL")]
        api_url: Option<String>,
    },
    /// Remove the stored API key from the keychain and the credentials file.
    Logout {
        /// Fallow Cloud API base URL of the key to remove. Default:
        /// $FALLOW_API_URL, else <https://api.fallow.cloud>.
        #[arg(long, value_name = "URL")]
        api_url: Option<String>,
    },
}

#[derive(clap::Subcommand)]
pub enum LicenseCli {
    /// Activate a license JWT.
    ///
    /// JWT input precedence: positional arg > `--from-file` > stdin (`-`).
    /// All paths normalize whitespace before crypto verification.
    Activate {
        /// JWT as a positional argument.
        #[arg(value_name = "JWT")]
        jwt: Option<String>,

        /// Path to a file containing the JWT.
        #[arg(long, value_name = "PATH")]
        from_file: Option<PathBuf>,

        /// Read JWT from stdin.
        #[arg(long, conflicts_with_all = ["jwt", "from_file"])]
        stdin: bool,

        /// Start a 30-day email-gated trial in one step.
        ///
        /// The trial endpoint is rate-limited to 5 requests per hour per IP.
        /// In CI or behind a shared NAT, start the trial from a developer
        /// machine and set FALLOW_LICENSE (or FALLOW_LICENSE_PATH) on the
        /// runner instead of re-running `activate --trial` per job.
        #[arg(long, requires = "email")]
        trial: bool,

        /// Email address for the trial flow.
        #[arg(long, value_name = "ADDR")]
        email: Option<String>,
    },
    /// Show the active license tier, seats, features, and days remaining.
    Status,
    /// Fetch a fresh JWT from `api.fallow.cloud` (network-only).
    ///
    /// The stored license JWT is the primary identity proof. When it is
    /// missing, or the cloud reports it as too stale to exchange, the request
    /// is retried with a full-access API key.
    Refresh {
        /// Fallow Cloud API key (bearer token) used when the stored license
        /// JWT cannot be refreshed.
        ///
        /// Precedence: this flag > $FALLOW_API_KEY. Generate at
        /// <https://fallow.cloud/settings#api-keys>.
        ///
        /// Security: prefer $FALLOW_API_KEY on shared CI runners. Passing a
        /// secret on the command line may be visible to other processes via
        /// `ps` and can leak into shell history or process audit logs.
        #[arg(long, value_name = "KEY")]
        api_key: Option<String>,
    },
    /// Remove the local license file.
    Deactivate,
}
