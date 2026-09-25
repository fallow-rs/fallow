//! CLI integration tests, built as one test binary.
//!
//! Each module used to be a test binary of its own. Every binary linked the
//! whole CLI, so the build linked it once per file. One binary links it once.
//! Three targets stay separate. `runtime_coverage_tests` and
//! `audit_brief_runtime_focus_tests` need the `test-sidecar-key` feature, and
//! the `drift` harness has its own ignored cases. CI runs each one by name.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests and benches use unwrap and expect to keep fixture setup concise"
)]

#[path = "../common/mod.rs"]
mod common;
#[path = "../common/sign.rs"]
mod sign;

mod agent_tests;
mod audit_tests;
mod caveat_surface_tests;
mod changed_since_added_files_tests;
mod changed_workspaces_tests;
mod check_tests;
mod codeowners_tests;
mod combined_coverage_tests;
mod complexity_gate_tests;
mod coverage_analyze_tests;
mod deprecated_export_tests;
mod doctor_tests;
mod dupes_tests;
mod dupes_workspace_tests;
mod exit_code_tests;
mod fix_tests;
mod flags_tests;
mod gate_outcome_tests;
mod gate_severity_tests;
mod github_format_tests;
mod grouping_fallback_tests;
mod guard_tests;
mod health_baseline_tests;
mod health_diagnostic_tests;
mod health_tests;
mod init_tests;
mod inspect_tests;
mod json_format_tests;
mod json_style_aux_commands_tests;
mod license_refresh_tests;
mod list_tests;
mod logging_tests;
mod migrate_tests;
mod monorepo_report_paths_tests;
mod non_gating_ci_level_tests;
mod output_file_tests;
mod parse_error_gate_tests;
mod plugin_diagnostic_tests;
mod production_workspace_tests;
mod reconcile_review_tests;
mod release_publish_list;
mod report_from_levels_tests;
mod report_parity_tests;
mod request_outcome_tests;
mod rule_pack_tests;
mod schema_conformance;
mod schema_tests;
mod scope_path_tests;
mod security_gate_tests;
mod security_workflow_tests;
mod signal_tests;
mod snapshot_tests;
mod summary_mark_tests;
mod suppressions_tests;
mod telemetry_tests;
mod trace_error_tests;
mod trace_federation_tests;
mod trace_path_tests;
mod type_aware_degradation_tests;
