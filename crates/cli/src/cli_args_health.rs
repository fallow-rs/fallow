//! Value enums of the `fallow health` arguments.

/// Filter refactoring targets by effort level.
#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
pub enum EffortFilter {
    Low,
    Medium,
    High,
}

impl EffortFilter {
    /// Convert to the corresponding `EffortEstimate` for comparison.
    pub(crate) const fn to_estimate(self) -> fallow_output::EffortEstimate {
        match self {
            Self::Low => fallow_output::EffortEstimate::Low,
            Self::Medium => fallow_output::EffortEstimate::Medium,
            Self::High => fallow_output::EffortEstimate::High,
        }
    }
}

/// CLI parser for the health severity gate.
#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
pub enum HealthSeverityCli {
    Moderate,
    High,
    Critical,
}

impl HealthSeverityCli {
    /// Convert to the typed health output severity.
    pub(crate) const fn to_health_severity(self) -> fallow_output::FindingSeverity {
        match self {
            Self::Moderate => fallow_output::FindingSeverity::Moderate,
            Self::High => fallow_output::FindingSeverity::High,
            Self::Critical => fallow_output::FindingSeverity::Critical,
        }
    }
}

/// Privacy mode for author emails emitted by `--ownership`.
///
/// CLI mirror of [`fallow_config::EmailMode`]. Kept as a separate enum so
/// the help text controls rendering and we don't leak config-internal
/// schema details into clap.
#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
pub enum EmailModeArg {
    /// Show full email addresses as recorded in git history.
    Raw,
    /// Show local-part only (default). Unwraps GitHub-style noreply prefixes.
    Handle,
    /// Show stable non-cryptographic pseudonyms (`xxh3:<hex>`).
    Anonymized,
    /// Legacy spelling for anonymized output.
    #[value(hide = true)]
    Hash,
}

impl EmailModeArg {
    /// Convert to the equivalent config-level mode.
    pub(crate) const fn to_config(self) -> fallow_config::EmailMode {
        match self {
            Self::Raw => fallow_config::EmailMode::Raw,
            Self::Handle => fallow_config::EmailMode::Handle,
            Self::Anonymized => fallow_config::EmailMode::Anonymized,
            Self::Hash => fallow_config::EmailMode::Hash,
        }
    }
}
