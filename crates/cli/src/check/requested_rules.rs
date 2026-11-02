//! The selector flags of a run, for the `requested-rule-off` note (issue
//! #4067).

use super::{CheckSurface, IssueFilters};

/// The selector flags that the user passed to `fallow architecture`. The issue
/// filters cannot tell this: no selector sets every architecture filter, and so
/// does every selector.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ArchitectureSelectors {
    pub cycles: bool,
    pub boundaries: bool,
    pub policy: bool,
}

impl CheckSurface {
    /// The `fallow architecture` surface with the selectors that the user passed.
    #[must_use]
    pub const fn architecture(cycles: bool, boundaries: bool, policy: bool) -> Self {
        Self::Architecture(ArchitectureSelectors {
            cycles,
            boundaries,
            policy,
        })
    }

    /// The selector flags that the user passed to this surface.
    pub(super) fn requested_flags(self, filters: &IssueFilters) -> Vec<&'static str> {
        match self {
            Self::DeadCode => fallow_engine::requested_rule_off::explicit_filter_flags(|flag| {
                let mut enabled = filters.clone();
                enabled.enable_cli_filter_flag(flag) && enabled == *filters
            }),
            Self::Architecture(selectors) => {
                fallow_engine::requested_rule_off::architecture_requested_flags(
                    selectors.cycles,
                    selectors.boundaries,
                    selectors.policy,
                )
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each registered flag maps to itself, so a new flag reaches the note.
    #[test]
    fn requested_flags_name_each_flag_that_the_user_passed() {
        let no_filters = IssueFilters::default();
        assert!(
            CheckSurface::DeadCode
                .requested_flags(&no_filters)
                .is_empty()
        );
        for flag in fallow_types::issue_meta::DEAD_CODE_FILTER_FLAGS
            .iter()
            .copied()
        {
            let mut filters = no_filters.clone();
            assert!(filters.enable_cli_filter_flag(flag), "{flag}");
            assert_eq!(CheckSurface::DeadCode.requested_flags(&filters), vec![flag]);
        }
    }

    /// No architecture selector sets every filter, and that asks for nothing.
    #[test]
    fn requested_flags_of_architecture_read_the_selectors_not_the_filters() {
        let every_filter = crate::dispatch::architecture_issue_filters(false, false, false);
        assert!(
            CheckSurface::architecture(false, false, false)
                .requested_flags(&every_filter)
                .is_empty()
        );
        assert_eq!(
            CheckSurface::architecture(true, false, true).requested_flags(&every_filter),
            vec!["--cycles", "--policy"]
        );
    }
}
