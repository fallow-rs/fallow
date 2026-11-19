//! The run plan: which of the three analyses one run executes, and the reason
//! for each analysis that it skips.
//!
//! Every command and API route builds a [`RunRequest`] from its flags or
//! options and calls [`plan_run`]. The config keys that turn an analysis off
//! (`duplicates.enabled`, `audit.deadCode`, `audit.health`, `audit.dupes`)
//! have their precedence here only.

use super::{AuditConfig, DuplicatesConfig, FallowConfig, ResolvedConfig};

/// One of the analyses that a run plan selects.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PlannedAnalysis {
    /// The dead-code analysis. Its architecture findings are part of it.
    DeadCode,
    /// The duplication analysis.
    Duplication,
    /// The health analysis.
    Health,
}

impl PlannedAnalysis {
    /// The name of the analysis in JSON output: `dead-code`, `duplication`
    /// or `health`.
    #[must_use]
    pub const fn wire_name(self) -> &'static str {
        match self {
            Self::DeadCode => "dead-code",
            Self::Duplication => "duplication",
            Self::Health => "health",
        }
    }
}

/// How a run asks for one analysis.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnalysisRequest {
    /// The run names the analysis: `--only dupes`, an option of
    /// `fallow_api::run_audit` that is set to `true` (see
    /// [`RunRequest::from_options`]), or the only `true` boolean of
    /// [`RunRequest::from_booleans`]. The analysis runs, whatever the config
    /// says.
    Named,
    /// The run does not name the analysis. The config decides.
    Default,
    /// The run leaves the analysis out: `--skip dupes`, an `--only` list
    /// without it, or an API option that is set to `false`.
    Excluded,
}

impl AnalysisRequest {
    /// `Some(true)` gives [`Self::Named`], `Some(false)` gives
    /// [`Self::Excluded`] and `None` gives [`Self::Default`].
    #[must_use]
    pub const fn from_option(value: Option<bool>) -> Self {
        match value {
            Some(true) => Self::Named,
            Some(false) => Self::Excluded,
            None => Self::Default,
        }
    }

    const fn selected(self) -> bool {
        !matches!(self, Self::Excluded)
    }

    const fn is_default(self) -> bool {
        matches!(self, Self::Default)
    }
}

/// The command of a run. It decides which config keys apply.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunCommand {
    /// No command key applies: the bare `fallow` run,
    /// `fallow_api::run_combined`, the health score and the editor.
    Combined,
    /// `fallow audit`. The `audit.deadCode`, `audit.health` and `audit.dupes`
    /// keys apply.
    Audit,
}

/// What one run asks for: the command and the request for each analysis.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RunRequest {
    /// The command of the run.
    pub command: RunCommand,
    /// The request for the dead-code analysis.
    pub dead_code: AnalysisRequest,
    /// The request for the duplication analysis.
    pub duplication: AnalysisRequest,
    /// The request for the health analysis.
    pub health: AnalysisRequest,
}

impl RunRequest {
    /// A request that names every analysis, as the review brief does.
    #[must_use]
    pub const fn all_named(command: RunCommand) -> Self {
        Self {
            command,
            dead_code: AnalysisRequest::Named,
            duplication: AnalysisRequest::Named,
            health: AnalysisRequest::Named,
        }
    }

    /// A request from `--only` and `--skip` lists. A non-empty `only` names
    /// each listed analysis and excludes the others. Otherwise `skip`
    /// excludes each listed analysis, and the config decides the others.
    #[must_use]
    pub fn from_lists(
        command: RunCommand,
        only: &[PlannedAnalysis],
        skip: &[PlannedAnalysis],
    ) -> Self {
        let request = |analysis: PlannedAnalysis| {
            if !only.is_empty() {
                if only.contains(&analysis) {
                    AnalysisRequest::Named
                } else {
                    AnalysisRequest::Excluded
                }
            } else if skip.contains(&analysis) {
                AnalysisRequest::Excluded
            } else {
                AnalysisRequest::Default
            }
        };
        Self {
            command,
            dead_code: request(PlannedAnalysis::DeadCode),
            duplication: request(PlannedAnalysis::Duplication),
            health: request(PlannedAnalysis::Health),
        }
    }

    /// A request from one boolean for each analysis, as in the options of
    /// `fallow_api::run_combined`. `false` excludes the analysis. `true`
    /// names the analysis when it is the only one that is `true`. Otherwise
    /// the config decides.
    #[must_use]
    pub const fn from_booleans(
        command: RunCommand,
        dead_code: bool,
        duplication: bool,
        health: bool,
    ) -> Self {
        const fn request(selected: bool, alone: bool) -> AnalysisRequest {
            if !selected {
                AnalysisRequest::Excluded
            } else if alone {
                AnalysisRequest::Named
            } else {
                AnalysisRequest::Default
            }
        }
        Self {
            command,
            dead_code: request(dead_code, !duplication && !health),
            duplication: request(duplication, !dead_code && !health),
            health: request(health, !dead_code && !duplication),
        }
    }

    /// A request from one optional boolean for each analysis, as in the
    /// options of `fallow_api::run_audit`. See
    /// [`AnalysisRequest::from_option`].
    #[must_use]
    pub const fn from_options(
        command: RunCommand,
        dead_code: Option<bool>,
        duplication: Option<bool>,
        health: Option<bool>,
    ) -> Self {
        Self {
            command,
            dead_code: AnalysisRequest::from_option(dead_code),
            duplication: AnalysisRequest::from_option(duplication),
            health: AnalysisRequest::from_option(health),
        }
    }

    /// Whether the plan of this request depends on the config. A caller that
    /// has no loaded config needs to load one only for such a request.
    #[must_use]
    pub const fn reads_config(&self) -> bool {
        match self.command {
            RunCommand::Combined => self.duplication.is_default(),
            RunCommand::Audit => {
                self.dead_code.is_default()
                    || self.duplication.is_default()
                    || self.health.is_default()
            }
        }
    }

    /// Whether the request asks for `analysis`, before the config decides.
    #[must_use]
    pub const fn requested(&self, analysis: PlannedAnalysis) -> bool {
        self.get(analysis).selected()
    }

    const fn get(self, analysis: PlannedAnalysis) -> AnalysisRequest {
        match analysis {
            PlannedAnalysis::DeadCode => self.dead_code,
            PlannedAnalysis::Duplication => self.duplication,
            PlannedAnalysis::Health => self.health,
        }
    }
}

/// A config key that can turn an analysis off.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigKey {
    /// `audit.deadCode`.
    AuditDeadCode,
    /// `audit.health`.
    AuditHealth,
    /// `audit.dupes`.
    AuditDupes,
    /// `duplicates.enabled`.
    DuplicatesEnabled,
}

impl ConfigKey {
    const fn is(self, other: Self) -> bool {
        matches!(
            (self, other),
            (Self::AuditDeadCode, Self::AuditDeadCode)
                | (Self::AuditHealth, Self::AuditHealth)
                | (Self::AuditDupes, Self::AuditDupes)
                | (Self::DuplicatesEnabled, Self::DuplicatesEnabled)
        )
    }
}

/// Why a run plan skips an analysis.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkipReason {
    /// The request excluded the analysis.
    NotRequested,
    /// The config key turned the analysis off.
    Config(ConfigKey),
}

/// The decision of a run plan for one analysis.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnalysisSelection {
    /// The analysis runs.
    Run,
    /// The analysis does not run.
    Skipped(SkipReason),
}

impl AnalysisSelection {
    /// Whether the analysis runs.
    #[must_use]
    pub const fn runs(self) -> bool {
        matches!(self, Self::Run)
    }

    /// Whether the config key `key` turned the analysis off.
    #[must_use]
    pub const fn off_by(self, key: ConfigKey) -> bool {
        match self {
            Self::Skipped(SkipReason::Config(found)) => found.is(key),
            Self::Run | Self::Skipped(SkipReason::NotRequested) => false,
        }
    }
}

/// The decision of [`plan_run`] for each analysis.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RunPlan {
    /// The decision for the dead-code analysis.
    pub dead_code: AnalysisSelection,
    /// The decision for the duplication analysis.
    pub duplication: AnalysisSelection,
    /// The decision for the health analysis.
    pub health: AnalysisSelection,
}

impl RunPlan {
    /// Whether at least one analysis runs.
    #[must_use]
    pub const fn runs_any(&self) -> bool {
        self.dead_code.runs() || self.duplication.runs() || self.health.runs()
    }
}

/// The config sections that [`plan_run`] reads.
#[derive(Debug, Clone, Copy)]
pub struct PlanSections<'a> {
    /// The `duplicates` section.
    pub duplicates: &'a DuplicatesConfig,
    /// The `audit` section.
    pub audit: &'a AuditConfig,
}

impl FallowConfig {
    /// The sections of this config that [`plan_run`] reads.
    #[must_use]
    pub const fn plan_sections(&self) -> PlanSections<'_> {
        PlanSections {
            duplicates: &self.duplicates,
            audit: &self.audit,
        }
    }
}

impl ResolvedConfig {
    /// The sections of this config that [`plan_run`] reads.
    #[must_use]
    pub const fn plan_sections(&self) -> PlanSections<'_> {
        PlanSections {
            duplicates: &self.duplicates,
            audit: &self.audit,
        }
    }
}

/// Decide which analyses one run executes.
///
/// This is the one rule for every command and API route:
/// 1. A named analysis runs. An excluded analysis does not run.
/// 2. For `fallow audit`, the `audit.*` key of the analysis decides next,
///    when it is set. `audit.dupes: true` runs duplication also when
///    `duplicates.enabled` is false.
/// 3. Otherwise `duplicates.enabled: false` turns duplication off.
/// 4. Otherwise the analysis runs.
#[must_use]
pub const fn plan_run(sections: PlanSections<'_>, request: RunRequest) -> RunPlan {
    let command = request.command;
    let duplicates_off = if sections.duplicates.enabled {
        None
    } else {
        Some(ConfigKey::DuplicatesEnabled)
    };
    RunPlan {
        dead_code: select(
            request.dead_code,
            command_key(command, ConfigKey::AuditDeadCode, sections.audit.dead_code),
            None,
        ),
        duplication: select(
            request.duplication,
            command_key(command, ConfigKey::AuditDupes, sections.audit.dupes),
            duplicates_off,
        ),
        health: select(
            request.health,
            command_key(command, ConfigKey::AuditHealth, sections.audit.health),
            None,
        ),
    }
}

/// The `audit.*` key and its value, when `command` reads that key.
const fn command_key(
    command: RunCommand,
    key: ConfigKey,
    value: Option<bool>,
) -> Option<(ConfigKey, Option<bool>)> {
    match command {
        RunCommand::Audit => Some((key, value)),
        RunCommand::Combined => None,
    }
}

/// The decision for one analysis. `command_key` is the key of the command and
/// its value, when the command has one. `project_off` is the key that turns
/// the analysis off for the whole project, when it does.
const fn select(
    request: AnalysisRequest,
    command_key: Option<(ConfigKey, Option<bool>)>,
    project_off: Option<ConfigKey>,
) -> AnalysisSelection {
    match request {
        AnalysisRequest::Named => AnalysisSelection::Run,
        AnalysisRequest::Excluded => AnalysisSelection::Skipped(SkipReason::NotRequested),
        AnalysisRequest::Default => match (command_key, project_off) {
            (Some((key, Some(false))), _) => AnalysisSelection::Skipped(SkipReason::Config(key)),
            (Some((_, Some(true))), _) | (_, None) => AnalysisSelection::Run,
            (_, Some(key)) => AnalysisSelection::Skipped(SkipReason::Config(key)),
        },
    }
}

#[cfg(test)]
#[path = "run_plan_tests.rs"]
mod tests;
