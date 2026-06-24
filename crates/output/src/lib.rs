//! Reusable output contract types for fallow.
//!
//! This crate owns stable report DTOs and output-format metadata that are not
//! tied to CLI rendering. Human, SARIF, markdown, CodeClimate, and JSON
//! builders still live in `fallow-cli`; this crate is the typed boundary those
//! builders and non-CLI consumers can share.
#![cfg_attr(
    test,
    allow(
        clippy::expect_used,
        reason = "tests use expect to keep serialization assertions concise"
    )
)]

mod check;
mod codeclimate;
mod dupes;
mod format;
mod issue_contract;

pub use check::{
    CHECK_SCHEMA_VERSION, CheckGroupedEntry, CheckGroupedOutput, CheckOutput, CheckOutputInput,
    GroupByMode, WorkspaceDiagnosticOutput, apply_config_fixable_to_duplicate_exports,
    build_check_output, build_check_summary,
};
pub use codeclimate::{
    CodeClimateIssue, CodeClimateIssueKind, CodeClimateLines, CodeClimateLocation,
    CodeClimateOutput, CodeClimateSeverity,
};
pub use dupes::{
    CloneFamilyAction, CloneFamilyActionType, CloneGroupAction, CloneGroupActionType,
    DUPES_SUPPRESS_COMMENT, DUPES_SUPPRESS_DESCRIPTION, clone_family_actions, clone_group_actions,
};
pub use fallow_types::envelope;
pub use fallow_types::output;
pub use fallow_types::output_dead_code;
pub use fallow_types::output_health;
pub use format::OutputFormat;
pub use issue_contract::{
    CODECLIMATE_RESULT_CODES, IssueOutputContract, TsAliasMeta, issue_output_contract_by_code,
    issue_output_contracts,
};
