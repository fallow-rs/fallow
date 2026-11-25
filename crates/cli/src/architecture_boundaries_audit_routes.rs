//! The audit command reaches repository references through the engine.

use super::*;

#[test]
fn audit_repo_ref_orchestration_routes_through_engine() {
    let source_path = "crates/api/src/runtime/audit.rs";
    let source = read_source_without_line_comments(source_path).expect("read audit source");
    let production_source = source
        .split("\n#[cfg(test)]")
        .next()
        .expect("audit source before tests");
    assert!(
        production_source.contains("repo_refs::{self, ResolvedAuditBase, TemporaryBaseWorktree}"),
        "audit runtime must use engine-owned repo-ref and base-worktree helpers"
    );
    for forbidden in [
        "Command::new(\"git\")",
        "std::process::Command::new(\"git\")",
        "clear_ambient_git_env",
        "fn git_stdout",
        "fn git_ref_exists",
        "fn git_upstream_ref",
        "fn git_merge_base",
        "fn detect_remote_default_ref",
        "fn get_head_sha",
        "struct BaseWorktree",
    ] {
        assert!(
            !production_source.contains(forbidden),
            "{source_path} must not own audit git orchestration helper `{forbidden}`"
        );
    }

    // The copy that drifted and caused #2699 lived on the CLI side, so the
    // same rule holds for the CLI entry point into the detection. Both routes
    // resolve the base through the one audit resolver in fallow-api.
    let resolver_path = "crates/api/src/audit_run/base_ref.rs";
    let resolver = read_source_without_line_comments(resolver_path).expect("read base resolver");
    assert!(
        resolver.contains("repo_refs::auto_detect_audit_base_ref")
            && !resolver.contains("Command::new(\"git\")"),
        "{resolver_path} must resolve the auto-detected base through the engine"
    );
    assert!(
        read_source_without_line_comments("crates/api/src/runtime/audit_base_ref.rs")
            .is_ok_and(|typed| typed.contains("crate::audit_run::resolve_audit_base")),
        "the typed audit must resolve the base through the shared audit resolver"
    );
    let cli_path = "crates/cli/src/audit_base_ref.rs";
    let cli_source = read_source_without_line_comments(cli_path).expect("read cli base-ref source");
    assert!(
        cli_source.contains("fallow_api::audit_run::resolve_audit_base"),
        "{cli_path} must resolve the base through the shared audit resolver"
    );
    for forbidden in [
        "fn git_stdout",
        "fn git_ref_exists",
        "fn git_upstream_ref",
        "fn git_merge_base",
        "fn detect_remote_default_ref",
        "fn get_head_sha",
        "Command::new(\"git\")",
    ] {
        assert!(
            !cli_source.contains(forbidden),
            "{cli_path} must not own audit git orchestration helper `{forbidden}`"
        );
    }

    let decision_surface_path = "crates/api/src/runtime/decision_surface.rs";
    let decision_surface =
        read_source_without_line_comments(decision_surface_path).expect("read decision surface");
    assert!(
        decision_surface.contains("fallow_engine::repo_refs::{self, TemporaryBaseWorktree}"),
        "decision surface must use engine-owned base-worktree helpers"
    );
    assert!(
        !decision_surface.contains("super::audit::BaseWorktree")
            && !decision_surface.contains("super::audit::base_analysis_root"),
        "{decision_surface_path} must not depend on audit-internal base-worktree helpers"
    );
}
