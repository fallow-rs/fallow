//! The `(code, context)` pair of each input error of the typed runners. A
//! caller reads the code to decide what to do, so the code of an error does
//! not depend on the step that finds it.

#![expect(
    clippy::expect_used,
    reason = "tests use expect to keep fixture setup concise"
)]

use std::path::{Path, PathBuf};

use fallow_api::{
    AnalysisOptions, AuditOptions, CombinedOptions, ComplexityOptions, DeadCodeOptions,
    DuplicationOptions, FeatureFlagsOptions, ListBoundariesOptions, MutationRunOptions,
    MutationRunScope, ProgrammaticError, ProjectInfoOptions, load_health_config, plan_mutation,
    run_audit, run_combined, run_dead_code, run_duplication, run_feature_flags, run_health,
    run_list_boundaries, run_project_info,
};

use crate::common::{commit, git, write};

const BROKEN_CONFIG: &str = r#"{ "rules": "#;
const CONFIG_FAILED: (&str, &str) = ("FALLOW_CONFIG_LOAD_FAILED", "analysis.configPath");
/// An external plugin with an entry pattern outside the project root. The
/// config parses, but it does not validate.
const TRAVERSAL_PLUGIN: &str = r#"{ "name": "escape", "entryPoints": ["../outside/**"] }"#;

/// A git repository with two commits, so an audit has a base. `config` is the
/// body of `.fallowrc.json` of the head commit.
fn project(dir: &Path, config: Option<&str>) -> PathBuf {
    let root = dir.join("project");
    std::fs::create_dir_all(&root).expect("project dir");
    let root = dunce::canonicalize(&root).expect("canonical root");
    git(&root, &["init", "-q", "-b", "main"]);
    write(
        &root,
        "package.json",
        r#"{"name":"run-errors","main":"src/index.ts"}"#,
    );
    write(&root, "src/index.ts", "export const value = 1;\n");
    commit(&root, "base");
    write(&root, "src/extra.ts", "export const extra = 2;\n");
    if let Some(config) = config {
        write(&root, ".fallowrc.json", config);
    }
    commit(&root, "head");
    root
}

fn analysis(root: &Path) -> AnalysisOptions {
    AnalysisOptions {
        root: Some(root.to_path_buf()),
        no_cache: true,
        ..AnalysisOptions::default()
    }
}

fn pair(err: &ProgrammaticError) -> (String, String) {
    (
        err.code.clone().unwrap_or_default(),
        err.context.clone().unwrap_or_default(),
    )
}

fn assert_pair(label: &str, err: &ProgrammaticError, expected: (&str, &str)) {
    assert_eq!(
        pair(err),
        (expected.0.to_owned(), expected.1.to_owned()),
        "{label}: {}",
        err.message
    );
    assert_eq!(err.exit_code, 2, "{label}: {}", err.message);
}

fn dead_code(analysis: AnalysisOptions) -> ProgrammaticError {
    run_dead_code(&DeadCodeOptions {
        analysis,
        ..DeadCodeOptions::default()
    })
    .expect_err("dead code fails")
}

#[test]
fn input_errors_keep_their_code_and_context() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = project(dir.path(), None);

    let missing_root = root.join("missing");
    assert_pair(
        "root missing",
        &dead_code(analysis(&missing_root)),
        ("FALLOW_INVALID_ROOT", "analysis.root"),
    );
    assert_pair(
        "root a file",
        &dead_code(analysis(&root.join("package.json"))),
        ("FALLOW_INVALID_ROOT", "analysis.root"),
    );
    assert_pair(
        "explicit config missing",
        &dead_code(AnalysisOptions {
            config_path: Some(root.join("missing.json")),
            ..analysis(&root)
        }),
        ("FALLOW_INVALID_CONFIG_PATH", "analysis.configPath"),
    );
    assert_pair(
        "threads 0",
        &dead_code(AnalysisOptions {
            threads: Some(0),
            ..analysis(&root)
        }),
        ("FALLOW_INVALID_THREADS", "analysis.threads"),
    );

    let combined = run_combined(&CombinedOptions {
        analysis: analysis(&root),
        dead_code: false,
        duplication: false,
        health: false,
        ..CombinedOptions::default()
    })
    .expect_err("combined fails");
    assert_pair(
        "combined all false",
        &combined,
        ("FALLOW_COMBINED_EMPTY", "combined"),
    );

    let audit = run_audit(&AuditOptions {
        analysis: analysis(&root),
        base: Some("HEAD~1".to_owned()),
        dead_code: Some(false),
        duplication: Some(false),
        health: Some(false),
        ..AuditOptions::default()
    })
    .expect_err("audit fails");
    assert_pair(
        "audit all off",
        &audit,
        ("FALLOW_AUDIT_NO_ANALYSIS", "audit"),
    );
}

#[test]
fn a_broken_config_keeps_the_code_of_each_route() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = project(dir.path(), Some(BROKEN_CONFIG));
    let options = analysis(&root);

    assert_pair("dead code", &dead_code(options.clone()), CONFIG_FAILED);

    let duplication = run_duplication(&DuplicationOptions {
        analysis: options.clone(),
        ..DuplicationOptions::default()
    })
    .expect_err("duplication fails");
    assert_pair("duplication", &duplication, CONFIG_FAILED);

    let flags = run_feature_flags(&FeatureFlagsOptions {
        analysis: options.clone(),
        ..FeatureFlagsOptions::default()
    })
    .expect_err("feature flags fail");
    assert_pair("feature flags", &flags, CONFIG_FAILED);

    let info = run_project_info(&ProjectInfoOptions {
        analysis: options.clone(),
        ..ProjectInfoOptions::default()
    })
    .expect_err("project info fails");
    assert_pair("project info", &info, CONFIG_FAILED);

    let boundaries = run_list_boundaries(&ListBoundariesOptions {
        analysis: options.clone(),
    })
    .expect_err("list boundaries fails");
    assert_pair("list boundaries", &boundaries, CONFIG_FAILED);

    let combined = run_combined(&CombinedOptions {
        analysis: options.clone(),
        ..CombinedOptions::default()
    })
    .expect_err("combined fails");
    assert_pair("combined", &combined, CONFIG_FAILED);

    let audit = run_audit(&AuditOptions {
        analysis: options.clone(),
        base: Some("HEAD~1".to_owned()),
        ..AuditOptions::default()
    })
    .expect_err("audit fails");
    assert_pair("audit", &audit, CONFIG_FAILED);

    let mutation = plan_mutation(&MutationRunOptions {
        analysis: options.clone(),
        scope: MutationRunScope::All,
        allow_dirty: true,
        ..MutationRunOptions::default()
    })
    .expect_err("mutation plan fails");
    assert_pair("mutation", &mutation, CONFIG_FAILED);

    // A selected health section skips the type-aware config check, so the
    // health runner loads the config first and reports its own code.
    let health = run_health(&ComplexityOptions {
        analysis: options,
        complexity: true,
        ..ComplexityOptions::default()
    })
    .expect_err("health fails");
    assert_pair("health", &health, ("FALLOW_HEALTH_FAILED", "fallow health"));
}

/// A config that parses but does not validate: an external plugin with an
/// entry pattern outside the project root. Each route keeps the code of the
/// step that finds the problem, as before the routes shared one run. The
/// health analysis reports a config that does not load with its own code.
#[test]
fn an_invalid_config_keeps_the_code_of_each_route() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = project(dir.path(), Some(r#"{"health": {"maxCyclomatic": 7}}"#));
    write(&root, ".fallow/plugins/escape.json", TRAVERSAL_PLUGIN);
    let options = analysis(&root);
    let health_failed = ("FALLOW_HEALTH_FAILED", "fallow health");

    assert_pair("dead code", &dead_code(options.clone()), CONFIG_FAILED);
    let combined = |dead_code: bool, duplication: bool, health: bool| {
        run_combined(&CombinedOptions {
            analysis: options.clone(),
            dead_code,
            duplication,
            health,
            ..CombinedOptions::default()
        })
        .expect_err("combined fails")
    };
    assert_pair("combined", &combined(true, true, true), CONFIG_FAILED);
    assert_pair(
        "combined duplication and health",
        &combined(false, true, true),
        CONFIG_FAILED,
    );
    assert_pair(
        "combined health only",
        &combined(false, false, true),
        health_failed,
    );
    let health = run_health(&ComplexityOptions {
        analysis: options.clone(),
        complexity: true,
        ..ComplexityOptions::default()
    })
    .expect_err("health fails");
    assert_pair("health", &health, health_failed);

    // `duplicates.enabled: false` leaves the health analysis alone.
    let dir = tempfile::tempdir().expect("tempdir");
    let root = project(dir.path(), Some(r#"{"duplicates": {"enabled": false}}"#));
    write(&root, ".fallow/plugins/escape.json", TRAVERSAL_PLUGIN);
    let combined = run_combined(&CombinedOptions {
        analysis: analysis(&root),
        dead_code: false,
        ..CombinedOptions::default()
    })
    .expect_err("combined fails");
    assert_pair(
        "combined duplication off by config",
        &combined,
        health_failed,
    );
}

/// The coverage layer of the typed health route reads the `health` section
/// only, so a config that parses but does not validate gives that section.
#[test]
fn the_health_config_layer_reads_a_config_that_does_not_validate() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = project(dir.path(), Some(r#"{"health": {"maxCyclomatic": 7}}"#));
    write(&root, ".fallow/plugins/escape.json", TRAVERSAL_PLUGIN);
    let config = load_health_config(&analysis(&root))
        .expect("the health section loads")
        .expect("the project has a config file");
    assert_eq!(config.max_cyclomatic, 7);
}
