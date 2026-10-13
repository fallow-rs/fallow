//! Config-sourced type-aware analysis on the typed runners. `fallow` turns
//! type-aware analysis on from `typeAware.enabled`, and `fallow audit` also
//! from `audit.typeAware`. The typed runners run it only when
//! `AnalysisOptions.type_aware` asks for it, so they refuse a config that
//! turns it on with `TYPE_AWARE_CONFIG_UNSUPPORTED` instead of returning
//! syntactic results. The refusal comes before any companion run, so these
//! tests need no sidecar.

#![expect(
    clippy::expect_used,
    reason = "tests use expect to keep fixture setup concise"
)]

use std::path::{Path, PathBuf};

use fallow_api::{
    AnalysisOptions, AuditOptions, CombinedOptions, ComplexityOptions, DeadCodeOptions,
    ProgrammaticError, TYPE_AWARE_CONFIG_UNSUPPORTED, run_audit, run_boundary_violations,
    run_circular_dependencies, run_combined, run_dead_code, run_health,
};

use crate::common::{commit, git, write};

/// A git project whose head commit changes one exported file.
fn project(dir: &Path, config: &str) -> PathBuf {
    let root = dir.join("project");
    std::fs::create_dir_all(&root).expect("create project");
    git(&root, &["init", "-q", "-b", "main"]);
    write(
        &root,
        "package.json",
        r#"{"name":"config-type-aware","private":true,"main":"src/index.ts"}"#,
    );
    write(&root, ".fallowrc.json", config);
    write(&root, "src/index.ts", "export { make } from './lib';\n");
    write(
        &root,
        "src/lib.ts",
        "interface Internal { value: number }\nexport function make(): Internal { return { value: 1 }; }\n",
    );
    commit(&root, "base");
    write(
        &root,
        "src/lib.ts",
        "interface Internal { value: number }\nexport function make(): Internal { return { value: 1 }; }\nexport const extra = 1;\n",
    );
    commit(&root, "change");
    root
}

fn analysis(root: &Path) -> AnalysisOptions {
    AnalysisOptions {
        root: Some(root.to_path_buf()),
        no_cache: true,
        ..AnalysisOptions::default()
    }
}

fn audit(root: &Path) -> Result<(), ProgrammaticError> {
    run_audit(&AuditOptions {
        analysis: analysis(root),
        base: Some("HEAD~1".to_owned()),
        ..AuditOptions::default()
    })
    .map(drop)
}

fn dead_code_options(root: &Path) -> DeadCodeOptions {
    DeadCodeOptions {
        analysis: analysis(root),
        ..DeadCodeOptions::default()
    }
}

fn health(root: &Path) -> Result<(), ProgrammaticError> {
    run_health(&ComplexityOptions {
        analysis: analysis(root),
        ..ComplexityOptions::default()
    })
    .map(drop)
}

#[track_caller]
fn assert_refused(result: Result<(), ProgrammaticError>, key: &str) {
    let err = result.expect_err("the typed runner must refuse config type-aware analysis");
    assert_eq!(
        err.code.as_deref(),
        Some(TYPE_AWARE_CONFIG_UNSUPPORTED),
        "{err:?}"
    );
    assert_eq!(err.context.as_deref(), Some(key), "{err:?}");
}

#[test]
fn type_aware_enabled_in_config_is_refused_by_every_runner() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = project(dir.path(), r#"{"typeAware":{"enabled":true}}"#);

    assert_refused(
        run_dead_code(&dead_code_options(&root)).map(drop),
        "typeAware.enabled",
    );
    assert_refused(
        run_circular_dependencies(&dead_code_options(&root)).map(drop),
        "typeAware.enabled",
    );
    assert_refused(
        run_boundary_violations(&dead_code_options(&root)).map(drop),
        "typeAware.enabled",
    );
    assert_refused(health(&root), "typeAware.enabled");
    assert_refused(audit(&root), "typeAware.enabled");
}

/// Bare `fallow` makes dead code type-aware and adds type coupling to health
/// from config. The default sections share one session, which does not pass
/// through `run_dead_code` or `run_health`, so `run_combined` checks itself.
#[test]
fn combined_refuses_when_dead_code_or_health_runs() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = project(dir.path(), r#"{"typeAware":{"enabled":true}}"#);
    let combined = |dead_code: bool, health: bool| {
        run_combined(&CombinedOptions {
            analysis: analysis(&root),
            dead_code,
            health,
            ..CombinedOptions::default()
        })
        .map(drop)
    };
    assert_refused(combined(true, true), "typeAware.enabled");
    assert_refused(combined(false, true), "typeAware.enabled");
    combined(false, false).expect("duplication alone runs without type-aware analysis");
}

/// `audit.typeAware` applies to audit only, and it wins over
/// `typeAware.enabled` in both directions, as in `fallow audit`.
#[test]
fn audit_type_aware_overrides_the_top_level_key_for_audit_only() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = project(dir.path(), r#"{"audit":{"typeAware":true}}"#);
    assert_refused(audit(&root), "audit.typeAware");
    run_dead_code(&dead_code_options(&root)).expect("dead code stays syntactic");
    health(&root).expect("health stays syntactic");

    let dir = tempfile::tempdir().expect("tempdir");
    let root = project(
        dir.path(),
        r#"{"typeAware":{"enabled":true},"audit":{"typeAware":false}}"#,
    );
    audit(&root).expect("`audit.typeAware: false` keeps the audit syntactic");
}

/// `fallow health` runs type coupling from config only when no section is
/// selected, so a run with a section stays on the typed route.
#[test]
fn health_with_a_section_ignores_the_config_key() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = project(dir.path(), r#"{"typeAware":{"enabled":true}}"#);
    run_health(&ComplexityOptions {
        analysis: analysis(&root),
        complexity: true,
        ..ComplexityOptions::default()
    })
    .expect("a health section runs without type coupling");
}

#[test]
fn syntactic_config_runs_on_the_typed_route() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = project(dir.path(), r#"{"typeAware":{"enabled":false}}"#);
    run_dead_code(&dead_code_options(&root)).expect("dead code");
    health(&root).expect("health");
    audit(&root).expect("audit");
}
