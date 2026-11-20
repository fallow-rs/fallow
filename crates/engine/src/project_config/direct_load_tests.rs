//! The config load behaviour of the three direct project config loads:
//! `config_for_project`, `config_for_project_with_load_options` and
//! `config_for_project_analysis`.
//!
//! Each test pins the error text, the validation level, the production
//! flatten or the trust policy of remote `extends` that a host sees.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests use unwrap and expect to keep fixture setup concise"
)]

use std::path::{Path, PathBuf};

use crate::project_config::{
    ProjectConfig, ProjectConfigOptions, config_for_project, config_for_project_analysis,
    config_for_project_with_load_options,
};
use fallow_config::{ConfigLoadOptions, FallowConfig, ProductionAnalysis};
use fallow_types::output_format::OutputFormat;

const CONFIG_NAME: &str = ".fallowrc.json";
const SYNTAX_ERROR: &str = r#"{ "rules": "#;
const WRONG_TYPE: &str = r#"{ "production": "yes" }"#;
const REMOTE_EXTENDS: &str = r#"{ "extends": "https://example.com/fallow.json" }"#;
/// No server listens on port 1, so the fetch fails at once and offline.
const UNREACHABLE_REMOTE_EXTENDS: &str = r#"{ "extends": "https://127.0.0.1:1/fallow.json" }"#;
const PER_ANALYSIS_PRODUCTION: &str = r#"{
    "production": { "deadCode": true, "health": false, "dupes": false, "ignore": ["scripts/**"] }
}"#;
const UNKNOWN_ZONE: &str = r#"{
    "boundaries": {
        "zones": [{ "name": "ui", "patterns": ["src/ui/**"] }],
        "rules": [{ "from": "ui", "allow": ["typo-zone"] }]
    }
}"#;
const MISSING_RULE_PACK: &str = r#"{ "rulePacks": ["packs/missing.json"] }"#;
const TRAVERSAL_PLUGIN: &str = r#"{ "name": "escape", "entryPoints": ["../outside/**"] }"#;

/// One entry point of project config resolution, called with an explicit
/// config path or with discovery.
#[derive(Clone, Copy, Debug)]
enum Wrapper {
    Project,
    WithLoadOptions,
    Analysis(ProductionAnalysis, Option<bool>),
}

const ALL_WRAPPERS: [Wrapper; 3] = [
    Wrapper::Project,
    Wrapper::WithLoadOptions,
    Wrapper::Analysis(ProductionAnalysis::DeadCode, None),
];

/// The two entry points that take the remote `extends` opt-in.
const OPT_IN_WRAPPERS: [Wrapper; 2] = [
    Wrapper::WithLoadOptions,
    Wrapper::Analysis(ProductionAnalysis::DeadCode, None),
];

fn options(
    analysis: ProductionAnalysis,
    production_override: Option<bool>,
    allow_remote_extends: bool,
) -> ProjectConfigOptions {
    ProjectConfigOptions {
        output: OutputFormat::Json,
        no_cache: true,
        threads: 1,
        production_override,
        quiet: true,
        analysis,
        allow_remote_extends,
    }
}

fn load(
    wrapper: Wrapper,
    root: &Path,
    config_path: Option<&Path>,
) -> Result<ProjectConfig, String> {
    load_with_trust(wrapper, root, config_path, false)
}

fn load_with_trust(
    wrapper: Wrapper,
    root: &Path,
    config_path: Option<&Path>,
    allow_remote_extends: bool,
) -> Result<ProjectConfig, String> {
    let result = match wrapper {
        Wrapper::Project => config_for_project(root, config_path),
        Wrapper::WithLoadOptions => config_for_project_with_load_options(
            root,
            config_path,
            ConfigLoadOptions::new(allow_remote_extends),
        ),
        Wrapper::Analysis(analysis, production) => config_for_project_analysis(
            root,
            config_path,
            options(analysis, production, allow_remote_extends),
        ),
    };
    result.map_err(|err| err.to_string())
}

/// A project root that stops config discovery at itself.
fn project(config: Option<&str>) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path().to_path_buf();
    std::fs::create_dir(root.join(".git")).expect("git marker");
    std::fs::write(root.join("package.json"), r#"{"name":"config-load"}"#).expect("package.json");
    if let Some(config) = config {
        std::fs::write(root.join(CONFIG_NAME), config).expect("config");
    }
    (dir, root)
}

fn write_traversal_plugin(root: &Path) {
    let plugins = root.join(".fallow").join("plugins");
    std::fs::create_dir_all(&plugins).expect("plugins dir");
    std::fs::write(plugins.join("escape.json"), TRAVERSAL_PLUGIN).expect("plugin");
}

#[test]
fn broken_explicit_config_reports_the_full_error_chain() {
    for body in [SYNTAX_ERROR, WRONG_TYPE] {
        let (_dir, root) = project(Some(body));
        let path = root.join(CONFIG_NAME);
        let cause = FallowConfig::load(&path).expect_err("broken config");
        let expected = format!("invalid config: {cause:#}");
        for wrapper in ALL_WRAPPERS {
            let err = load(wrapper, &root, Some(&path)).expect_err("explicit config fails");
            assert_eq!(err, expected, "{wrapper:?} with {body}");
        }
    }
}

#[test]
fn broken_discovered_config_reports_the_discovery_message() {
    for body in [SYNTAX_ERROR, WRONG_TYPE] {
        let (_dir, root) = project(Some(body));
        let cause = FallowConfig::find_and_load(&root).expect_err("broken config");
        let expected = format!("invalid config: {cause}");
        for wrapper in ALL_WRAPPERS {
            let err = load(wrapper, &root, None).expect_err("discovered config fails");
            assert_eq!(err, expected, "{wrapper:?} with {body}");
        }
    }
}

#[test]
fn remote_extends_is_refused_without_the_opt_in() {
    let (_dir, root) = project(Some(REMOTE_EXTENDS));
    let path = root.join(CONFIG_NAME);
    let explicit = format!(
        "invalid config: {:#}",
        FallowConfig::load(&path).expect_err("remote extends refused")
    );
    let discovered = format!(
        "invalid config: {}",
        FallowConfig::find_and_load(&root).expect_err("remote extends refused")
    );
    assert!(explicit.contains("Remote config extends"), "{explicit}");
    for wrapper in ALL_WRAPPERS {
        assert_eq!(
            load(wrapper, &root, Some(&path)).expect_err("explicit"),
            explicit
        );
        assert_eq!(
            load(wrapper, &root, None).expect_err("discovered"),
            discovered
        );
    }
}

#[test]
fn remote_extends_is_fetched_with_the_opt_in() {
    let (_dir, root) = project(Some(UNREACHABLE_REMOTE_EXTENDS));
    let path = root.join(CONFIG_NAME);
    for wrapper in OPT_IN_WRAPPERS {
        for config_path in [Some(path.as_path()), None] {
            let err = load_with_trust(wrapper, &root, config_path, true).expect_err("fetch fails");
            assert!(
                err.starts_with("invalid config: ")
                    && err.contains("Failed to fetch remote config"),
                "{wrapper:?} with {config_path:?}: {err}"
            );
        }
    }
}

#[test]
fn loaded_path_is_the_explicit_or_the_discovered_file() {
    let (_dir, root) = project(Some("{}"));
    let other = root.join("other.json");
    std::fs::write(&other, "{}").expect("other config");
    for wrapper in ALL_WRAPPERS {
        let explicit = load(wrapper, &root, Some(&other)).expect("explicit loads");
        assert_eq!(
            explicit.path.as_deref(),
            Some(other.as_path()),
            "{wrapper:?}"
        );
        let discovered = load(wrapper, &root, None).expect("discovered loads");
        assert_eq!(
            discovered.path.as_deref(),
            Some(root.join(CONFIG_NAME).as_path()),
            "{wrapper:?}"
        );
    }

    let (_dir, bare) = project(None);
    for wrapper in ALL_WRAPPERS {
        let defaults = load(wrapper, &bare, None).expect("defaults load");
        assert_eq!(defaults.path, None, "{wrapper:?}");
    }
}

#[test]
fn only_the_analysis_entry_points_validate_external_plugins() {
    for config in [None, Some("{}")] {
        let (_dir, root) = project(config);
        write_traversal_plugin(&root);
        assert!(load(Wrapper::Project, &root, None).is_ok(), "{config:?}");
        assert!(
            load(Wrapper::WithLoadOptions, &root, None).is_ok(),
            "{config:?}"
        );
        let err = load(
            Wrapper::Analysis(ProductionAnalysis::DeadCode, None),
            &root,
            None,
        )
        .expect_err("plugin rejected");
        assert!(
            err.starts_with("invalid external plugin definition:\n  - "),
            "{config:?}: {err}"
        );
        assert!(err.contains("../outside/**"), "{err}");
    }
}

#[test]
fn every_entry_point_validates_boundaries_and_rule_packs_of_a_loaded_file() {
    let (_dir, root) = project(Some(UNKNOWN_ZONE));
    let config = FallowConfig::load(&root.join(CONFIG_NAME)).expect("config parses");
    let zone_errors = config
        .validate_resolved_boundaries(&root)
        .expect_err("unknown zone");
    let expected = format!(
        "invalid boundary configuration:\n  - {}",
        zone_errors
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n  - ")
    );
    for wrapper in ALL_WRAPPERS {
        assert_eq!(
            load(wrapper, &root, None).expect_err("boundaries"),
            expected,
            "{wrapper:?}"
        );
    }

    let (_dir, root) = project(Some(MISSING_RULE_PACK));
    for wrapper in ALL_WRAPPERS {
        let err = load(wrapper, &root, None).expect_err("rule pack");
        assert!(
            err.starts_with("invalid rule pack:\n  - "),
            "{wrapper:?}: {err}"
        );
        assert!(err.contains("packs/missing.json"), "{wrapper:?}: {err}");
    }
}

#[test]
fn per_analysis_production_flattens_for_the_requested_analysis() {
    let (_dir, root) = project(Some(PER_ANALYSIS_PRODUCTION));
    let cases = [
        (Wrapper::Project, true),
        (Wrapper::WithLoadOptions, true),
        (Wrapper::Analysis(ProductionAnalysis::DeadCode, None), true),
        (Wrapper::Analysis(ProductionAnalysis::Health, None), false),
        (Wrapper::Analysis(ProductionAnalysis::Dupes, None), false),
        (
            Wrapper::Analysis(ProductionAnalysis::Dupes, Some(true)),
            true,
        ),
        (
            Wrapper::Analysis(ProductionAnalysis::DeadCode, Some(false)),
            false,
        ),
        (
            Wrapper::Analysis(ProductionAnalysis::Health, Some(true)),
            true,
        ),
    ];
    for (wrapper, production) in cases {
        let project = load(wrapper, &root, None).expect("config loads");
        assert_eq!(project.config.production, production, "{wrapper:?}");
        assert_eq!(
            project.config.production_ignore,
            vec!["scripts/**".to_owned()],
            "{wrapper:?}"
        );
    }
}

#[test]
fn production_override_without_a_config_file_reaches_only_the_analysis_entry_points() {
    let (_dir, root) = project(None);
    let cases = [
        (Wrapper::Project, false),
        (Wrapper::WithLoadOptions, false),
        (Wrapper::Analysis(ProductionAnalysis::Health, None), false),
        (
            Wrapper::Analysis(ProductionAnalysis::Health, Some(true)),
            true,
        ),
        (
            Wrapper::Analysis(ProductionAnalysis::Dupes, Some(true)),
            true,
        ),
    ];
    for (wrapper, production) in cases {
        let project = load(wrapper, &root, None).expect("defaults load");
        assert_eq!(project.config.production, production, "{wrapper:?}");
    }
}
