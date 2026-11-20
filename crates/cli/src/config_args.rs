//! Arguments of the `fallow config` subcommand.

use clap::{Args, ValueEnum};

use crate::AnalysisKind;

/// The flags of `fallow config`.
#[derive(Args)]
pub struct ConfigArgs {
    /// Print only the config file path (one line, no JSON)
    #[arg(long, help_heading = "Output")]
    pub path: bool,

    /// Print each decision of a run and its cause as JSON: for each analysis
    /// whether it runs and its production mode, and the run settings, each
    /// with the flag, environment variable, config key or default that gave
    /// it. Environment variables show by name only.
    #[arg(long, conflicts_with = "path", help_heading = "Output")]
    pub resolved: bool,

    /// The command whose rules `--resolved` applies. Without it, the rules of
    /// the bare `fallow` run apply.
    #[arg(
        long = "for",
        value_enum,
        value_name = "COMMAND",
        requires = "resolved",
        help_heading = "Run"
    )]
    pub for_command: Option<ForCommand>,

    /// With `--resolved`: run only these analyses, as `--only` of the bare
    /// run or of `fallow audit` does.
    #[arg(
        long,
        value_delimiter = ',',
        requires = "resolved",
        help_heading = "Run"
    )]
    pub only: Vec<AnalysisKind>,

    /// With `--resolved`: skip these analyses, as `--skip` of the bare run or
    /// of `fallow audit` does.
    #[arg(
        long,
        value_delimiter = ',',
        requires = "resolved",
        conflicts_with = "only",
        help_heading = "Run"
    )]
    pub skip: Vec<AnalysisKind>,

    /// With `--resolved`: run dead-code analysis in production mode.
    #[arg(
        long = "production-dead-code",
        requires = "resolved",
        help_heading = "Run"
    )]
    pub production_dead_code: bool,

    /// With `--resolved`: run health analysis in production mode.
    #[arg(
        long = "production-health",
        requires = "resolved",
        help_heading = "Run"
    )]
    pub production_health: bool,

    /// With `--resolved`: run duplication analysis in production mode.
    #[arg(long = "production-dupes", requires = "resolved", help_heading = "Run")]
    pub production_dupes: bool,
}

/// The command whose rules `fallow config --resolved` applies.
#[derive(Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum ForCommand {
    /// The bare `fallow` run.
    Combined,
    /// `fallow audit`.
    Audit,
    /// `fallow dead-code`.
    DeadCode,
    /// `fallow dupes`.
    Dupes,
    /// `fallow health`.
    Health,
}
