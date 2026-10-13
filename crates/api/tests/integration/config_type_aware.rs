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
    ProgrammaticError, TYPE_AWARE_CONFIG_UNSUPPORTED, TypeAwareOptions, run_audit,
    run_boundary_violations, run_circular_dependencies, run_combined, run_dead_code, run_health,
};

use crate::common::{commit, git, rerun_with_type_aware_sidecar, write};

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

/// `enabled: Some(false)` keeps a call syntactic, as `--no-type-aware` does,
/// so every runner answers although both config keys turn type-aware
/// analysis on.
#[test]
fn explicit_off_runs_every_runner_syntactic() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = project(
        dir.path(),
        r#"{"typeAware":{"enabled":true},"audit":{"typeAware":true}}"#,
    );
    let off = AnalysisOptions {
        type_aware: TypeAwareOptions {
            enabled: Some(false),
            ..TypeAwareOptions::default()
        },
        ..analysis(&root)
    };
    let dead_code = DeadCodeOptions {
        analysis: off.clone(),
        ..DeadCodeOptions::default()
    };
    let report = run_dead_code(&dead_code).expect("dead code");
    assert!(
        report
            .output
            .meta
            .as_ref()
            .and_then(|meta| meta.type_aware.as_ref())
            .is_none()
    );
    run_circular_dependencies(&dead_code).expect("circular dependencies");
    run_boundary_violations(&dead_code).expect("boundary violations");
    run_health(&ComplexityOptions {
        analysis: off.clone(),
        ..ComplexityOptions::default()
    })
    .expect("health");
    run_combined(&CombinedOptions {
        analysis: off.clone(),
        ..CombinedOptions::default()
    })
    .expect("combined");
    run_audit(&AuditOptions {
        analysis: off,
        base: Some("HEAD~1".to_owned()),
        ..AuditOptions::default()
    })
    .expect("audit");
}

/// The unused class members of a project whose OpenTelemetry sampler
/// implements `Sampler`, with `enabled` as the type-aware choice.
fn sampler_members(config: &str, enabled: Option<bool>) -> Vec<String> {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path().join("project");
    write(
        &root,
        "package.json",
        r#"{"name":"otel-sampler","private":true,"main":"src/index.ts",
            "dependencies":{"@opentelemetry/sdk-trace-base":"1.30.0"}}"#,
    );
    write(&root, ".fallowrc.json", config);
    write(
        &root,
        "src/index.ts",
        "import { MySampler } from './sampler';\nconsole.log(new MySampler());\n",
    );
    write(
        &root,
        "src/sampler.ts",
        "import type { Sampler } from '@opentelemetry/sdk-trace-base';\n\
         export class MySampler implements Sampler {\n\
         \x20 shouldSample() { return { decision: 1 }; }\n\
         \x20 toString() { return 'MySampler'; }\n\
         \x20 unusedHelper() { return 1; }\n\
         }\n",
    );
    let report = run_dead_code(&DeadCodeOptions {
        analysis: AnalysisOptions {
            type_aware: TypeAwareOptions {
                enabled,
                ..TypeAwareOptions::default()
            },
            ..analysis(&root)
        },
        ..DeadCodeOptions::default()
    })
    .expect("dead code");
    let mut members: Vec<String> = report
        .output
        .results
        .unused_class_members
        .iter()
        .map(|finding| finding.member.member_name.clone())
        .collect();
    members.sort();
    members
}

/// `Some(false)` reaches detection as `--no-type-aware` does. With
/// `typeAware.enabled` in config, detection holds framework members back for
/// semantic verification. A syntactic run must credit them, as it does when
/// the config leaves type-aware analysis off.
#[test]
fn explicit_off_credits_framework_members_as_a_syntactic_config_does() {
    let off_in_config = sampler_members("{}", None);
    assert_eq!(off_in_config, vec!["unusedHelper".to_owned()]);
    assert_eq!(
        sampler_members(r#"{"typeAware":{"enabled":true}}"#, Some(false)),
        off_in_config
    );
}

const PRIVATE_LEAK_TEST: &str =
    "config_type_aware::explicit_on_turns_on_private_type_leaks_as_the_cli_does";

/// A type-aware run turns the opt-in `private-type-leaks` rule on, as
/// `fallow dead-code --type-aware` does, so `Some(true)` reports the
/// exported function that returns a non-exported interface.
#[test]
fn explicit_on_turns_on_private_type_leaks_as_the_cli_does() {
    if std::env::var_os("FALLOW_TYPE_AWARE_BIN").is_none() {
        rerun_with_type_aware_sidecar(PRIVATE_LEAK_TEST);
        return;
    }
    let leaks = |config: &str, enabled: Option<bool>| {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = project(dir.path(), config);
        run_dead_code(&DeadCodeOptions {
            analysis: AnalysisOptions {
                type_aware: TypeAwareOptions {
                    enabled,
                    ..TypeAwareOptions::default()
                },
                ..analysis(&root)
            },
            ..DeadCodeOptions::default()
        })
        .expect("dead code")
        .output
        .results
        .private_type_leaks
        .len()
    };
    assert_eq!(leaks("{}", Some(true)), 1);
    assert_eq!(leaks("{}", None), 0);
    // A rule that the config sets keeps its severity.
    assert_eq!(
        leaks(r#"{"rules":{"private-type-leaks":"off"}}"#, Some(true)),
        0
    );
}
