//! CLI-specific duplication output wrappers.
//!
//! The shared clone group, clone family, and report payload contracts live in
//! `fallow-api`. This module keeps only the grouped-output attribution wrapper
//! because it depends on CLI grouping types.

use fallow_engine::duplicates::fingerprint_for_fragment;
use fallow_output::{CloneGroupAction, clone_group_actions};
use serde::Serialize;

use crate::report::dupes_grouping::AttributedCloneGroup;

#[allow(
    unused_imports,
    reason = "compatibility re-export while dupes payload contracts move to fallow-api"
)]
pub use fallow_api::{CloneFamilyFinding, CloneGroupFinding, DupesReportPayload};

/// Wire-shape envelope for an [`AttributedCloneGroup`] finding.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct AttributedCloneGroupFinding {
    /// The underlying attributed clone group.
    #[serde(flatten)]
    pub group: AttributedCloneGroup,
    /// Stable content fingerprint, usually `dup:<8hex>`.
    pub fingerprint: String,
    /// Suggested next steps. Always emitted.
    pub actions: Vec<CloneGroupAction>,
}

impl AttributedCloneGroupFinding {
    /// Build the wrapper from an [`AttributedCloneGroup`].
    #[allow(
        dead_code,
        reason = "kept for focused wrapper tests and non-report construction paths"
    )]
    #[must_use]
    pub fn with_actions(group: AttributedCloneGroup) -> Self {
        let fingerprint = group.instances.first().map_or_else(
            || fingerprint_for_fragment(""),
            |ai| fingerprint_for_fragment(&ai.instance.fragment),
        );
        Self::with_fingerprint(group, fingerprint)
    }

    /// Build the wrapper with a precomputed report-scoped fingerprint.
    #[must_use]
    pub fn with_fingerprint(group: AttributedCloneGroup, fingerprint: String) -> Self {
        let actions = clone_group_actions(group.line_count, group.instances.len());
        Self {
            group,
            fingerprint,
            actions,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use fallow_engine::duplicates::CloneInstance;
    use fallow_output::CloneGroupActionType;

    use super::*;
    use crate::report::dupes_grouping::AttributedInstance;

    fn instance(path: &str) -> CloneInstance {
        CloneInstance {
            file: PathBuf::from(path),
            start_line: 1,
            end_line: 10,
            start_col: 0,
            end_col: 0,
            fragment: String::new(),
        }
    }

    #[test]
    fn attributed_clone_group_finding_actions_match_clone_group_shape() {
        let attributed = AttributedCloneGroup {
            primary_owner: "src".to_string(),
            token_count: 100,
            line_count: 20,
            instances: vec![
                AttributedInstance {
                    instance: instance("/root/src/a.ts"),
                    owner: "src".to_string(),
                },
                AttributedInstance {
                    instance: instance("/root/src/b.ts"),
                    owner: "src".to_string(),
                },
            ],
        };
        let finding = AttributedCloneGroupFinding::with_actions(attributed);
        assert_eq!(finding.actions.len(), 2);
        assert_eq!(finding.actions[0].kind, CloneGroupActionType::ExtractShared);
        assert_eq!(finding.actions[1].kind, CloneGroupActionType::SuppressLine);
    }
}
