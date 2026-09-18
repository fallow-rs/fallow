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

use serde::{Serialize, Serializer};

/// One channel that narrowed a run to part of the project.
///
/// Serialized as kebab-case inside `scope_reasons` and published as an OPEN
/// set, the same tolerate-unknown contract `gate_outcomes` keys carry: a name
/// this build does not emit means "some narrowing", not an error.
///
/// Which names a command can emit differs per command, because the three
/// narrowing predicates see different state. `dead-code` reads the flags
/// themselves and can name every channel. `dupes` and `health` see an already
/// resolved changed-file set and report `changed-files`, because at that point
/// the flag that produced it is gone. `health` reports `workspace` for both
/// `--workspace` and `--changed-workspaces` for the same reason. A consumer
/// must therefore not assume a given command emits a given name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum ScopeReason {
    /// A diff index reached the analysis, from `--diff-file`, `--diff-stdin`,
    /// `FALLOW_DIFF_FILE` or the shared index a CI format installs.
    Diff,
    /// `--changed-since`.
    ChangedSince,
    /// A resolved changed-file set, on the commands that see the set rather
    /// than the flag that produced it.
    ChangedFiles,
    /// `--workspace`. On `health` this also covers `--changed-workspaces`,
    /// which it cannot distinguish.
    Workspace,
    /// `--changed-workspaces`.
    ChangedWorkspaces,
    /// `--scope`.
    Scope,
    /// One or more `--file`.
    File,
    /// An active issue-type filter such as `--unused-exports`, which drops
    /// whole baseline categories before the comparison.
    IssueTypeFilter,
    /// Production mode, from the flag or the resolved project config. It drops
    /// test, story and dev files at discovery.
    Production,
}

impl ScopeReason {
    /// Every reason, in the declaration order `scope_reasons` serializes in.
    const ALL: [Self; 9] = [
        Self::Diff,
        Self::ChangedSince,
        Self::ChangedFiles,
        Self::Workspace,
        Self::ChangedWorkspaces,
        Self::Scope,
        Self::File,
        Self::IssueTypeFilter,
        Self::Production,
    ];

    /// The kebab-case name this reason serializes as, for prose that has to
    /// name it outside the JSON envelope.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Diff => "diff",
            Self::ChangedSince => "changed-since",
            Self::ChangedFiles => "changed-files",
            Self::Workspace => "workspace",
            Self::ChangedWorkspaces => "changed-workspaces",
            Self::Scope => "scope",
            Self::File => "file",
            Self::IssueTypeFilter => "issue-type-filter",
            Self::Production => "production",
        }
    }

    /// Whether repeating the run without this channel judges the same project.
    ///
    /// A channel the caller added for one run, a diff, a base ref, a path or an
    /// issue-type filter, is removable: dropping it widens the run to the whole
    /// project, which is exactly what judging a whole-project baseline needs.
    /// Production mode and workspace scoping are the caller's own statement
    /// about what the project is, and they resolve from the project config and
    /// the environment as well as from a flag, so repeating the command without
    /// the flag analyzes something nobody asked about and, on the config and
    /// environment routes, is not even narrower.
    ///
    /// This is the rule the GitHub Action and the GitLab template already apply
    /// before re-reading a baseline unscoped, and the one the `scope_reasons`
    /// documentation states.
    #[must_use]
    pub const fn is_removable_by_rerun(self) -> bool {
        match self {
            Self::Diff
            | Self::ChangedSince
            | Self::ChangedFiles
            | Self::Scope
            | Self::File
            | Self::IssueTypeFilter => true,
            Self::Workspace | Self::ChangedWorkspaces | Self::Production => false,
        }
    }

    const fn bit(self) -> u16 {
        1 << (self as u16)
    }
}

/// The set of channels that narrowed one run.
///
/// A bitset rather than a `Vec` so [`BaselineStaleness`] keeps `Copy`, which
/// the gate builder's `const fn` and three envelope structs that hold the
/// object by value rely on. Serializes as an array of [`ScopeReason`] in
/// declaration order, so two identical runs produce identical bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BaselineScopeReasons(u16);

impl BaselineScopeReasons {
    /// A run that was not narrowed.
    #[must_use]
    pub const fn empty() -> Self {
        Self(0)
    }

    /// This set plus `reason`.
    #[must_use]
    pub const fn with(self, reason: ScopeReason) -> Self {
        Self(self.0 | reason.bit())
    }

    /// This set plus `reason` when `active`, unchanged otherwise.
    #[must_use]
    pub const fn insert_if(self, active: bool, reason: ScopeReason) -> Self {
        if active { self.with(reason) } else { self }
    }

    /// True when nothing narrowed the run, which is exactly when
    /// `change_scoped` is false.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.0 == 0
    }

    /// Whether `reason` narrowed the run.
    #[must_use]
    pub const fn contains(self, reason: ScopeReason) -> bool {
        self.0 & reason.bit() != 0
    }

    /// The reasons in wire order.
    pub fn iter(self) -> impl Iterator<Item = ScopeReason> {
        ScopeReason::ALL
            .into_iter()
            .filter(move |reason| self.contains(*reason))
    }

    /// True when repeating the run without every channel in this set judges
    /// the same project, so a command that drops them all is worth suggesting.
    ///
    /// Vacuously true for an empty set; callers that mean "this run was
    /// narrowed and can be widened" check [`Self::is_empty`] first.
    #[must_use]
    pub fn all_removable_by_rerun(self) -> bool {
        self.iter().all(ScopeReason::is_removable_by_rerun)
    }

    /// The reasons as a comma-joined list of kebab-case names, for prose.
    /// Empty when the run was not narrowed.
    #[must_use]
    pub fn join(self) -> String {
        self.iter()
            .map(ScopeReason::as_str)
            .collect::<Vec<_>>()
            .join(", ")
    }
}

impl Serialize for BaselineScopeReasons {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(self.iter())
    }
}

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
    /// are false whenever this is true. `scope_reasons` names the channels
    /// that fired.
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
    /// True when the loaded file carries no key this command's own baseline
    /// format writes, so it is a baseline another command saved or an object
    /// with nothing of this command's in it. Read this, not
    /// `baseline_entries == 0`, before telling anyone their baseline is the
    /// wrong file: a baseline saved from a project that had nothing to record
    /// is legitimately empty and is not a mistake.
    ///
    /// Present only when true, so an envelope from a run that loaded its own
    /// baseline is unchanged. `dead-code` never sets it: five of its baseline
    /// fields have no serde default, so a file that is not one fails to load
    /// with exit 2 long before this.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub unrecognised_format: bool,
    /// Which channels narrowed this run, present and non-empty exactly when
    /// `change_scoped` is true. Both members are derived from one function, so
    /// the boolean and the array cannot disagree.
    ///
    /// Read it to decide whether the narrowing is removable: a run narrowed
    /// only by `diff`, `changed-since`, `changed-files`, `scope`, `file` or
    /// `issue-type-filter` can be repeated unscoped to judge the baseline,
    /// while `production`, `workspace` and `changed-workspaces` are the
    /// caller's own choice about what to analyze and an unscoped repeat would
    /// contradict it.
    ///
    /// The name set is OPEN and the names a command can emit differ per
    /// command; see [`ScopeReason`].
    #[serde(default, skip_serializing_if = "BaselineScopeReasons::is_empty")]
    #[cfg_attr(
        feature = "schema",
        schemars(with = "std::collections::BTreeSet<ScopeReason>")
    )]
    pub scope_reasons: BaselineScopeReasons,
}

#[cfg(test)]
mod tests {
    use super::{BaselineScopeReasons, BaselineStaleness, BaselineStalenessAdvisory, ScopeReason};

    fn staleness(scope_reasons: BaselineScopeReasons) -> BaselineStaleness {
        BaselineStaleness {
            baseline_entries: 8,
            matched_entries: 0,
            stale_entries: 8,
            current_findings: 0,
            change_scoped: !scope_reasons.is_empty(),
            stale: false,
            warning: BaselineStalenessAdvisory::None,
            gate_trips: false,
            moved_entries: 0,
            unrecognised_format: false,
            scope_reasons,
        }
    }

    #[test]
    fn reasons_serialize_as_a_kebab_case_array() {
        let value = serde_json::to_value(staleness(
            BaselineScopeReasons::empty()
                .with(ScopeReason::Production)
                .with(ScopeReason::ChangedSince),
        ))
        .expect("staleness serializes");

        assert_eq!(
            value.get("scope_reasons"),
            Some(&serde_json::json!(["changed-since", "production"]))
        );
    }

    #[test]
    fn reasons_serialize_in_declaration_order_whatever_the_insertion_order() {
        let forwards = BaselineScopeReasons::empty()
            .with(ScopeReason::Diff)
            .with(ScopeReason::IssueTypeFilter)
            .with(ScopeReason::Production);
        let backwards = BaselineScopeReasons::empty()
            .with(ScopeReason::Production)
            .with(ScopeReason::IssueTypeFilter)
            .with(ScopeReason::Diff);

        let expected = serde_json::json!(["diff", "issue-type-filter", "production"]);
        assert_eq!(
            serde_json::to_value(forwards).expect("reasons serialize"),
            expected
        );
        assert_eq!(
            serde_json::to_value(backwards).expect("reasons serialize"),
            expected
        );
    }

    #[test]
    fn an_unscoped_run_keeps_the_member_off_the_wire() {
        let value = serde_json::to_value(staleness(BaselineScopeReasons::empty()))
            .expect("staleness serializes");

        assert!(
            value.get("scope_reasons").is_none(),
            "a whole-project run must stay byte-identical to a pre-change run"
        );
        assert_eq!(value.get("change_scoped"), Some(&serde_json::json!(false)));
    }

    #[test]
    fn the_member_is_non_empty_exactly_when_the_run_was_narrowed() {
        for reason in [
            ScopeReason::Diff,
            ScopeReason::ChangedSince,
            ScopeReason::ChangedFiles,
            ScopeReason::Workspace,
            ScopeReason::ChangedWorkspaces,
            ScopeReason::Scope,
            ScopeReason::File,
            ScopeReason::IssueTypeFilter,
            ScopeReason::Production,
        ] {
            let reasons = BaselineScopeReasons::empty().with(reason);
            assert!(reasons.contains(reason), "{reason:?} must round-trip");
            assert!(!reasons.is_empty());
            assert_eq!(reasons.join(), reason.as_str());
        }
    }

    /// The same split the GitHub Action and the GitLab template encode in
    /// `BASELINE_REMOVABLE_SCOPE_REASONS`. Kept as one list here so a new
    /// channel has to answer the question rather than inherit an answer.
    #[test]
    fn removable_channels_are_the_ones_a_repeat_can_drop() {
        let removable: Vec<&str> = ScopeReason::ALL
            .into_iter()
            .filter(|reason| reason.is_removable_by_rerun())
            .map(ScopeReason::as_str)
            .collect();

        assert_eq!(
            removable,
            [
                "diff",
                "changed-since",
                "changed-files",
                "scope",
                "file",
                "issue-type-filter"
            ]
        );
    }

    #[test]
    fn a_set_is_removable_only_when_every_channel_in_it_is() {
        let removable = BaselineScopeReasons::empty()
            .with(ScopeReason::ChangedSince)
            .with(ScopeReason::Scope);
        assert!(removable.all_removable_by_rerun());

        assert!(
            !removable
                .with(ScopeReason::Production)
                .all_removable_by_rerun(),
            "a repeat that drops the base ref still runs in production mode"
        );
    }

    #[test]
    fn insert_if_is_the_only_gate_on_membership() {
        let reasons = BaselineScopeReasons::empty()
            .insert_if(false, ScopeReason::Diff)
            .insert_if(true, ScopeReason::Scope);

        assert!(!reasons.contains(ScopeReason::Diff));
        assert!(reasons.contains(ScopeReason::Scope));
    }

    #[test]
    fn every_reason_has_a_distinct_bit_and_a_distinct_name() {
        let mut combined = BaselineScopeReasons::empty();
        for reason in ScopeReason::ALL {
            combined = combined.with(reason);
        }
        assert_eq!(combined.iter().count(), ScopeReason::ALL.len());

        let names = ScopeReason::ALL.map(ScopeReason::as_str);
        let mut sorted = names.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), names.len());
    }

    #[test]
    fn the_kebab_name_matches_what_serde_emits() {
        for reason in ScopeReason::ALL {
            assert_eq!(
                serde_json::to_value(reason).expect("reason serializes"),
                serde_json::json!(reason.as_str()),
            );
        }
    }
}
