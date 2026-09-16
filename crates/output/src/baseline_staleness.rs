//! The machine-readable view of a loaded baseline's staleness.
//!
//! One shape for every command that accepts `--baseline`, so a consumer reads
//! the same member names whether the envelope came from `dead-code`, `dupes` or
//! `health`. Carried as `baseline_staleness` at the dead-code and duplication
//! roots and inside `summary` on health, absent whenever no baseline was loaded.
//!
//! Every member is a projection of the run's
//! `fallow_engine::baseline::BaselineStaleness`, so nothing here restates a rule
//! that lives in the engine. `gate_trips` in particular is computed by the same
//! function the `--fail-on-stale-baseline` exit gate calls, which is why a CI
//! integration can read one boolean instead of reimplementing the condition in
//! jq.

use serde::Serialize;

/// Which advisory a loaded baseline earned on this run.
///
/// Mirrors `fallow_engine::baseline::BaselineStalenessWarning` so a consumer can
/// render the same distinction the stderr warning makes, instead of inferring it
/// from counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum BaselineStalenessAdvisory {
    /// Nothing to say: the baseline is fresh enough, or this run cannot judge
    /// it (a narrowed scope, an empty baseline, or a run with no findings to
    /// match against).
    None,
    /// Nothing in the baseline matched and there were findings to match, so the
    /// paths likely moved or the baseline was saved elsewhere.
    ZeroOverlap,
    /// A quarter or more of the baseline matched nothing, so it protects
    /// meaningfully less than what was saved.
    Partial,
}

/// One run's machine-readable view of a loaded baseline.
///
/// `stale` and `gate_trips` answer different questions and legitimately
/// disagree. `stale` mirrors the unasked-for stderr advisory, which stays silent
/// below a quarter of the baseline and on a run that produced no findings at
/// all, because a cleaned project and a rotted baseline look identical from
/// there. `gate_trips` mirrors the opt-in `--fail-on-stale-baseline` rule, which
/// a repository asks for precisely to catch those cases, so it fires on any
/// stale entry. A rotted baseline on a cleaned project reports
/// `stale: false` with `gate_trips: true`; that is the contract, not a defect.
///
/// `change_scoped` is the member a consumer must read before dividing
/// `matched_entries` by `baseline_entries`. A run narrowed to part of the
/// project compares a whole-project baseline against a slice of it and can
/// report `matched_entries: 0` while the baseline is perfectly healthy, so both
/// `stale` and `gate_trips` are false there by construction. The remedy for a
/// tripped gate is always the same: re-save the baseline from a whole-project
/// run with `--save-baseline`.
#[derive(Debug, Clone, Copy, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct BaselineStaleness {
    /// Entries carried by the loaded baseline file. On health these are the
    /// complexity and CRAP finding entries; runtime-coverage suppressions and
    /// refactoring target keys carried by the same file are not counted.
    pub baseline_entries: usize,
    /// Entries that matched a current finding on this run and were filtered out
    /// of the report. On health this includes entries matched through a
    /// followed file move.
    pub matched_entries: usize,
    /// Entries that matched no current finding on this run:
    /// `baseline_entries - matched_entries`.
    pub stale_entries: usize,
    /// Findings this run produced before the baseline filtered them. Zero means
    /// there was nothing to compare, either because the project is clean or
    /// because the scope was empty, which is why `stale` stays false there even
    /// when every entry went unmatched.
    pub current_findings: usize,
    /// True when this run analyzed only part of the project, so a whole-project
    /// baseline matches less of it for reasons that are not rot. The channels
    /// differ per command and include a diff, a base ref, `--changed-since`,
    /// `--workspace`, `--changed-workspaces`, `--scope`, `--file`, an
    /// issue-type filter, and production mode. Both `stale` and `gate_trips`
    /// are false whenever this is true.
    pub change_scoped: bool,
    /// True exactly when the advisory stderr warning fired: not change-scoped,
    /// at least one current finding before baseline filtering, and either
    /// nothing matched or `stale_entries` reached a quarter of
    /// `baseline_entries`.
    pub stale: bool,
    /// Which advisory this run earned, so a consumer can render the same
    /// distinction the stderr warning makes instead of inferring it from the
    /// counts. `none` whenever `stale` is false.
    pub warning: BaselineStalenessAdvisory,
    /// True exactly when
    /// `!change_scoped && baseline_entries > 0 && matched_entries < baseline_entries`,
    /// which is the rule `--fail-on-stale-baseline` applies. Deliberately
    /// stricter than `stale`: any unmatched entry counts. It describes the
    /// baseline, not the run's exit code: `health --report-only` is an explicit
    /// request never to fail, so that run exits 0 and says so on stderr while
    /// still reporting `gate_trips: true` here.
    pub gate_trips: bool,
    /// Entries that matched only by following a file move. Only `health` can
    /// follow one, in its identity baseline mode; `dead-code` and `dupes` match
    /// entries by fingerprint and never classify one as moved, so they report
    /// `0`. Always `0` in health's count mode too.
    pub moved_entries: usize,
}
