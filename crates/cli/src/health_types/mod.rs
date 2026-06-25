//! Health / complexity analysis report types.
//!
//! Separated from the `health` command module so that report formatters
//! (which are compiled as part of both the lib and bin targets) can
//! reference these types without pulling in binary-only dependencies.

mod coverage;
mod coverage_intelligence;
mod runtime_coverage;
mod scores;
mod targets;
mod trends;
mod vital_signs;

pub use coverage::*;
pub use coverage_intelligence::*;
#[allow(
    unused_imports,
    reason = "report tests construct CSS actions through the health_types facade"
)]
pub use fallow_output::CssCandidateActionType;
#[allow(
    unused_imports,
    reason = "CLI report tests exercise the compatibility facade for this output action builder"
)]
pub use fallow_output::build_health_finding_actions;
pub use fallow_output::{
    CssAnalyticsReport, CssAnalyticsSummary, CssBlockOccurrence, CssCandidateAction,
    CssDuplicateBlock, CssFileAnalytics, CssNotationConsistency, CssNotationCount,
    FrameworkHealthDetector, FrameworkHealthDetectorStatus, FrameworkHealthDiagnostics,
    HealthActionContext, HealthActionOptions, HealthActionsMeta, HealthFinding, HealthGroup,
    HealthGrouping, HealthReport, HealthTimings, HotspotFinding, RefactoringTargetFinding,
    ScopedUnusedClasses, TailwindArbitraryValue, UndefinedKeyframes, UnreferencedCssClass,
    UnreferencedKeyframes, UnresolvedClassReference, UnusedAtRule, UnusedAtRuleKind,
    UnusedFontFace, UnusedThemeToken,
};
pub use runtime_coverage::*;
pub use scores::*;
pub use targets::*;
pub use trends::*;
pub use vital_signs::*;
