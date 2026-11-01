//! Value types of the `fallow audit` arguments.

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
