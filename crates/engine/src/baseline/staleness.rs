//! Staleness of a saved baseline.

use super::*;

impl BaselineStaleness {
    /// Entries that matched no current finding on this run.
    #[must_use]
    pub const fn stale_entries(&self) -> usize {
        self.entries.saturating_sub(self.matched)
    }

    /// The advisory warning this run prints on stderr by default.
    ///
    /// Silent for a run narrowed to part of the project, because such a run
    /// legitimately sees only a slice of a whole-project baseline and re-saving
    /// from it would drop every entry the run never looked at. Silent as well
    /// when the run produced no findings: a cleaned project and a rotted
    /// baseline look identical from here, and the re-save advice is wrong when
    /// the right move is deleting the file.
    #[must_use]
    pub const fn warning(&self) -> BaselineStalenessWarning {
        if self.change_scoped || self.entries == 0 || self.current_findings == 0 {
            return BaselineStalenessWarning::None;
        }
        if self.matched == 0 {
            return BaselineStalenessWarning::ZeroOverlap;
        }
        if stale_share_warrants_warning(self.entries, self.stale_entries()) {
            return BaselineStalenessWarning::Partial;
        }
        BaselineStalenessWarning::None
    }

    /// Whether the opt-in `--fail-on-stale-baseline` gate fires.
    ///
    /// Deliberately stricter than [`Self::warning`]: any stale entry counts.
    /// The quarter threshold exists to keep an unasked-for line from training
    /// people to ignore it, and the empty-run silence exists because an
    /// advisory cannot tell rot from success; a repository that passes the flag
    /// has asked for both. Change-scope stays the one shared suppression,
    /// because a narrowed run still cannot judge a whole-project baseline.
    #[must_use]
    pub const fn trips_gate(&self) -> bool {
        stale_baseline_gate_trips(self.entries, self.matched, self.change_scoped)
    }
    /// This run's machine-readable view of the baseline, for the JSON envelope.
    ///
    /// Every member is derived here rather than in a consumer, so the advisory
    /// threshold and the gate rule have exactly one implementation. Only
    /// `health` can follow a file move; the commands that match entries by
    /// fingerprint pass `0`.
    ///
    /// `scope_reasons` and `unrecognised_format` come from the caller because
    /// only the command knows which channels it read and which format it read
    /// the file as; this struct carries the counts the analysis needs and
    /// nothing more. `change_scoped` and `scope_reasons` must agree, which is
    /// why every caller derives the boolean from the same reason set it passes
    /// here.
    ///
    /// `unrecognised_format` widens `gate_trips` here, at the single site that
    /// builds it, so the opt-in gate, the exit code, every CI surface that reads
    /// the boolean and the MCP sentences move together: a file this command
    /// cannot read as its own suppresses nothing, which no count can express.
    /// Unlike the count rule it is not suppressed by `change_scoped`, because
    /// telling a foreign file from this command's own needs no project-wide run.
    #[must_use]
    pub fn to_envelope(
        &self,
        moved_entries: usize,
        scope_reasons: fallow_output::BaselineScopeReasons,
        unrecognised_format: bool,
    ) -> fallow_output::BaselineStaleness {
        debug_assert_eq!(
            self.change_scoped,
            !scope_reasons.is_empty(),
            "change_scoped and scope_reasons must be derived from the same predicate"
        );
        let warning = self.warning();
        fallow_output::BaselineStaleness {
            baseline_entries: self.entries,
            matched_entries: self.matched,
            stale_entries: self.stale_entries(),
            current_findings: self.current_findings,
            remaining_findings: None,
            change_scoped: self.change_scoped,
            stale: warning != BaselineStalenessWarning::None,
            warning: match warning {
                BaselineStalenessWarning::None => fallow_output::BaselineStalenessAdvisory::None,
                BaselineStalenessWarning::ZeroOverlap => {
                    fallow_output::BaselineStalenessAdvisory::ZeroOverlap
                }
                BaselineStalenessWarning::Partial => {
                    fallow_output::BaselineStalenessAdvisory::Partial
                }
            },
            gate_trips: self.trips_gate() || unrecognised_format,
            moved_entries,
            unrecognised_format,
            saved_by: None,
            format: None,
            scope_reasons,
        }
    }
}
