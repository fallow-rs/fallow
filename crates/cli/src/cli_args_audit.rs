//! Value enums of the `fallow audit` flags.

/// CLI mirror of [`fallow_config::AuditGate`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
pub enum AuditGateArg {
    /// Only findings introduced by the current changeset affect the verdict.
    NewOnly,
    /// All findings in changed files affect the verdict.
    All,
}

/// CLI value of `--worsened`, mirroring `fallow_config::AuditWorsened`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
pub enum AuditWorsenedArg {
    /// A worsened finding makes the verdict at least warn.
    Warn,
    /// A worsened finding counts like an introduced finding.
    Error,
    /// Only growth of an exceeded metric is worsened (warn).
    Off,
}

impl From<AuditWorsenedArg> for fallow_config::AuditWorsened {
    fn from(value: AuditWorsenedArg) -> Self {
        match value {
            AuditWorsenedArg::Warn => Self::Warn,
            AuditWorsenedArg::Error => Self::Error,
            AuditWorsenedArg::Off => Self::Off,
        }
    }
}

impl From<AuditGateArg> for fallow_config::AuditGate {
    fn from(value: AuditGateArg) -> Self {
        match value {
            AuditGateArg::NewOnly => Self::NewOnly,
            AuditGateArg::All => Self::All,
        }
    }
}

/// The subcommands of `fallow audit-cache`.
#[derive(clap::Subcommand)]
pub enum AuditCacheCli {
    /// Remove reusable audit caches owned by an explicit project root.
    ///
    /// Deletes this project's cache entries unconditionally, warm or not. To
    /// apply the age-based GC policy across every cache entry instead, use
    /// `fallow audit-cache prune`.
    Remove {
        /// Print what would be removed without touching the filesystem.
        #[arg(long)]
        dry_run: bool,

        /// Confirm removal in non-interactive environments.
        #[arg(long, alias = "force")]
        yes: bool,
    },

    /// Apply the audit cache GC policy now and report every entry.
    ///
    /// Runs the same reclaim policy every `fallow audit` run already applies
    /// silently: orphaned-sidecar cleanup, age-based reclaim under the
    /// resolved threshold, and cross-repo reclaim of abandoned entries whose
    /// recorded owner root no longer exists. It also bounds the base-snapshot
    /// files in the project cache directory (`pass: "snapshot"`): files older
    /// than the threshold go, and only the newest 32 stay. Entries owned by
    /// other live projects are never touched. Use `--dry-run` to preview every decision
    /// the policy would take without touching the filesystem. `--root` is
    /// optional and defaults to the current directory. Reported sizes come
    /// from a full recursive walk of each cache entry, which can take a few
    /// seconds on large caches. To delete one project's caches
    /// unconditionally, use `fallow audit-cache remove --root <path> --yes`.
    Prune {
        /// Preview decisions without touching the filesystem.
        #[arg(long)]
        dry_run: bool,

        /// Age threshold in days for this invocation. Overrides
        /// FALLOW_AUDIT_CACHE_MAX_AGE_DAYS and the `audit.cacheMaxAgeDays`
        /// config field (default 30). `0` disables age-based reclaim but
        /// still reclaims orphaned sidecars and entries whose recorded owner
        /// root is gone; unconditional deletion of one project's caches is
        /// `fallow audit-cache remove --root <path> --yes`.
        #[arg(long, value_name = "N")]
        max_age_days: Option<u32>,
    },
}
