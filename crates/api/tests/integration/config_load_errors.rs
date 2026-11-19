//! The config load errors of the typed runners. A broken explicit config and
//! a broken discovered config each give exit code 2, the code
//! `FALLOW_CONFIG_LOAD_FAILED` and the context `analysis.configPath`. The text
//! depends on the step that loads the config first: the API loader, or the
//! engine resolution of one analysis.

#![expect(
    clippy::expect_used,
    reason = "tests use expect to keep fixture setup concise"
)]

use std::path::{Path, PathBuf};

use fallow_api::{
    AnalysisOptions, CombinedOptions, DeadCodeOptions, ProgrammaticError, TypeAwareOptions,
    load_health_config, run_combined, run_dead_code,
};
use fallow_config::FallowConfig;

use crate::common::{rerun_without_env, write};

const CONFIG_NAME: &str = ".fallowrc.json";
const SYNTAX_ERROR: &str = r#"{ "rules": "#;
const WRONG_TYPE: &str = r#"{ "production": "yes" }"#;
const REMOTE_EXTENDS: &str = r#"{ "extends": "https://example.com/fallow.json" }"#;
/// No server listens on port 1, so the fetch fails at once and offline.
const UNREACHABLE_REMOTE_EXTENDS: &str = r#"{ "extends": "https://127.0.0.1:1/fallow.json" }"#;
const CODE: &str = "FALLOW_CONFIG_LOAD_FAILED";
const CONTEXT: &str = "analysis.configPath";
const TYPE_AWARE_ENV: &str = "FALLOW_TYPE_AWARE";

/// A project root that stops config discovery at itself. The root is
/// canonical, as the runners canonicalize it before discovery.
fn project(dir: &Path, config: Option<&str>) -> PathBuf {
    let root = dir.join("project");
    std::fs::create_dir_all(root.join(".git")).expect("git marker");
    let root = dunce::canonicalize(&root).expect("canonical root");
    write(
        &root,
        "package.json",
        r#"{"name":"config-load","main":"src/index.ts"}"#,
    );
    write(&root, "src/index.ts", "export const value = 1;\n");
    if let Some(config) = config {
        write(&root, CONFIG_NAME, config);
    }
    root
}

fn analysis(root: &Path, explicit: bool) -> AnalysisOptions {
    AnalysisOptions {
        root: Some(root.to_path_buf()),
        config_path: explicit.then(|| root.join(CONFIG_NAME)),
        no_cache: true,
        ..AnalysisOptions::default()
    }
}

/// The detail of the config error, as the API loader and the engine format it.
fn detail(root: &Path, explicit: bool) -> String {
    if explicit {
        let err = FallowConfig::load(&root.join(CONFIG_NAME)).expect_err("broken config");
        format!("{err:#}")
    } else {
        FallowConfig::find_and_load(root).expect_err("broken config")
    }
}

fn assert_config_error(err: &ProgrammaticError, message: &str) {
    assert_eq!(err.message, message);
    assert_eq!(err.exit_code, 2, "{}", err.message);
    assert_eq!(err.code.as_deref(), Some(CODE), "{}", err.message);
    assert_eq!(err.context.as_deref(), Some(CONTEXT), "{}", err.message);
}

#[test]
fn health_config_load_reports_the_api_loader_text() {
    for body in [SYNTAX_ERROR, WRONG_TYPE, REMOTE_EXTENDS] {
        for explicit in [true, false] {
            let dir = tempfile::tempdir().expect("tempdir");
            let root = project(dir.path(), Some(body));
            let err = load_health_config(&analysis(&root, explicit)).expect_err("load fails");
            let expected = format!("failed to load config: {}", detail(&root, explicit));
            assert_config_error(&err, &expected);
        }
    }
}

#[test]
fn remote_extends_refusal_names_the_opt_in() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = project(dir.path(), Some(REMOTE_EXTENDS));
    let err = load_health_config(&analysis(&root, true)).expect_err("load fails");
    assert!(
        err.message.contains("Remote config extends"),
        "{}",
        err.message
    );
}

#[test]
fn health_config_is_none_without_a_config_file() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = project(dir.path(), None);
    let health = load_health_config(&analysis(&root, false)).expect("defaults load");
    assert!(health.is_none());

    let dir = tempfile::tempdir().expect("tempdir");
    let root = project(dir.path(), Some(r#"{ "health": { "maxCyclomatic": 7 } }"#));
    let health = load_health_config(&analysis(&root, true))
        .expect("config loads")
        .expect("config file has a health section");
    assert_eq!(health.max_cyclomatic, 7);
}

#[test]
fn remote_extends_is_fetched_with_the_opt_in() {
    for explicit in [true, false] {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = project(dir.path(), Some(UNREACHABLE_REMOTE_EXTENDS));
        let err = load_health_config(&AnalysisOptions {
            allow_remote_extends: true,
            ..analysis(&root, explicit)
        })
        .expect_err("fetch fails");
        assert!(
            err.message.starts_with("failed to load config: ")
                && err.message.contains("Failed to fetch remote config"),
            "{}",
            err.message
        );
    }
}

/// The API loader reads the config before the analysis only when
/// `FALLOW_TYPE_AWARE` is unset. A test must not change the environment of
/// its own process, so a run with the variable set runs the test again in a
/// child process without it.
#[test]
fn runners_report_the_api_loader_text_before_the_analysis_loads() {
    if std::env::var_os(TYPE_AWARE_ENV).is_some() {
        rerun_without_env(
            "config_load_errors::runners_report_the_api_loader_text_before_the_analysis_loads",
            TYPE_AWARE_ENV,
        );
        return;
    }
    for explicit in [true, false] {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = project(dir.path(), Some(SYNTAX_ERROR));
        let expected = format!("failed to load config: {}", detail(&root, explicit));

        let dead_code = run_dead_code(&DeadCodeOptions {
            analysis: analysis(&root, explicit),
            ..DeadCodeOptions::default()
        })
        .expect_err("dead code fails");
        assert_config_error(&dead_code, &expected);

        let combined = run_combined(&CombinedOptions {
            analysis: analysis(&root, explicit),
            ..CombinedOptions::default()
        })
        .expect_err("combined fails");
        assert_config_error(&combined, &expected);
    }
}

#[test]
fn dead_code_reports_the_engine_text_when_the_analysis_loads_first() {
    for explicit in [true, false] {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = project(dir.path(), Some(SYNTAX_ERROR));
        let expected = format!(
            "failed to load config: invalid config: {}",
            detail(&root, explicit)
        );
        let err = run_dead_code(&DeadCodeOptions {
            analysis: AnalysisOptions {
                type_aware: TypeAwareOptions {
                    enabled: Some(false),
                    ..TypeAwareOptions::default()
                },
                ..analysis(&root, explicit)
            },
            ..DeadCodeOptions::default()
        })
        .expect_err("dead code fails");
        assert_config_error(&err, &expected);
    }
}
