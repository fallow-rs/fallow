use std::path::{Path, PathBuf};

use fallow_config::{
    CommandKind, EnvSnapshot, EnvVar, TypeAwareConfig, TypeAwareInputs, TypeAwareRequire,
};
use fallow_types::sourced::{Given, Origin, Source};

use super::*;

const FILE: &str = "/p/.fallowrc.json";

fn config(section: &TypeAwareConfig, audit: Option<bool>) -> TypeAwareConfigInput<'_> {
    TypeAwareConfigInput {
        section,
        audit,
        file: Some(Path::new(FILE)),
    }
}

fn decide(
    command: CommandKind,
    given: &TypeAwareInputs,
    env: &[(&str, &str)],
    section: &TypeAwareConfig,
    audit: Option<bool>,
) -> Result<TypeAwareSettings, ResolveError> {
    decide_type_aware(
        command,
        given,
        &EnvSnapshot::from_pairs(env, None),
        config(section, audit),
    )
}

fn config_key(key: &'static str) -> Source {
    Source::ConfigFile {
        file: PathBuf::from(FILE),
        key,
    }
}

#[test]
fn a_flag_wins_over_the_environment_and_the_config() {
    let given = TypeAwareInputs {
        enabled: Some(Given {
            value: false,
            origin: Origin::Flag("--no-type-aware"),
        }),
        ..TypeAwareInputs::default()
    };
    let section = TypeAwareConfig {
        enabled: true,
        ..TypeAwareConfig::default()
    };
    let settings = decide(
        CommandKind::DeadCode,
        &given,
        &[("FALLOW_TYPE_AWARE", "1")],
        &section,
        None,
    )
    .expect("settings");
    assert!(!settings.enabled.value());
    assert_eq!(
        settings.enabled.source(),
        &Source::Given {
            origin: Origin::Flag("--no-type-aware")
        }
    );
}

#[test]
fn the_environment_wins_over_the_config() {
    let section = TypeAwareConfig {
        enabled: true,
        projects: vec!["tsconfig.app.json".to_owned()],
        require: TypeAwareRequire::Complete,
    };
    let settings = decide(
        CommandKind::Health,
        &TypeAwareInputs::default(),
        &[
            ("FALLOW_TYPE_AWARE", "off"),
            ("FALLOW_TYPE_AWARE_REQUIRE", "best-effort"),
            ("FALLOW_TYPE_AWARE_PROJECTS", "a.json"),
        ],
        &section,
        None,
    )
    .expect("settings");
    assert!(!settings.enabled.value());
    assert_eq!(
        settings.enabled.source(),
        &Source::Environment {
            name: "FALLOW_TYPE_AWARE"
        }
    );
    assert_eq!(settings.projects.value(), &vec![PathBuf::from("a.json")]);
    assert_eq!(settings.require.value(), &TypeAwareRequire::BestEffort);
}

#[test]
fn the_config_decides_without_inputs() {
    let section = TypeAwareConfig {
        enabled: true,
        projects: vec!["tsconfig.app.json".to_owned()],
        require: TypeAwareRequire::Complete,
    };
    let settings = decide(
        CommandKind::DeadCode,
        &TypeAwareInputs::default(),
        &[],
        &section,
        None,
    )
    .expect("settings");
    assert_eq!(settings.enabled.source(), &config_key("typeAware.enabled"));
    assert_eq!(
        settings.projects.value(),
        &vec![PathBuf::from("tsconfig.app.json")]
    );
    assert_eq!(settings.require.source(), &config_key("typeAware.require"));
    let defaults = decide(
        CommandKind::DeadCode,
        &TypeAwareInputs::default(),
        &[],
        &TypeAwareConfig::default(),
        None,
    )
    .expect("settings");
    assert!(!defaults.enabled.value());
    assert!(matches!(defaults.enabled.source(), Source::Default { .. }));
}

#[test]
fn the_audit_key_wins_over_the_top_level_key_for_audit_only() {
    let section = TypeAwareConfig {
        enabled: true,
        ..TypeAwareConfig::default()
    };
    let audit = decide(
        CommandKind::Audit,
        &TypeAwareInputs::default(),
        &[],
        &section,
        Some(false),
    )
    .expect("settings");
    assert!(!audit.enabled.value());
    assert_eq!(audit.enabled.source(), &config_key("audit.typeAware"));
    let dead_code = decide(
        CommandKind::DeadCode,
        &TypeAwareInputs::default(),
        &[],
        &section,
        Some(false),
    )
    .expect("settings");
    assert!(dead_code.enabled.value());
    let env = decide(
        CommandKind::Audit,
        &TypeAwareInputs::default(),
        &[("FALLOW_TYPE_AWARE", "yes")],
        &section,
        Some(false),
    )
    .expect("settings");
    assert!(env.enabled.value());
}

fn input_error(err: &ResolveError) -> (InputField, InputProblem, String) {
    match err {
        ResolveError::Input {
            field,
            problem,
            message,
        } => (*field, *problem, message.clone()),
        other => panic!("not an input error: {other:?}"),
    }
}

#[test]
fn an_unknown_value_is_an_input_error_also_with_a_flag() {
    let given = TypeAwareInputs {
        enabled: Some(Given {
            value: true,
            origin: Origin::Flag("--type-aware"),
        }),
        ..TypeAwareInputs::default()
    };
    let err = decide(
        CommandKind::DeadCode,
        &given,
        &[("FALLOW_TYPE_AWARE", "maybe")],
        &TypeAwareConfig::default(),
        None,
    )
    .expect_err("unknown value");
    assert_eq!(
        input_error(&err),
        (
            InputField::Env(EnvVar::TypeAware),
            InputProblem::InvalidValue,
            "FALLOW_TYPE_AWARE must be one of true, false, 1, 0, yes, no, on, or off".to_owned()
        )
    );
}

#[test]
fn a_given_policy_hides_an_unknown_require_value() {
    let env = [("FALLOW_TYPE_AWARE_REQUIRE", "sometimes")];
    let err = decide(
        CommandKind::DeadCode,
        &TypeAwareInputs::default(),
        &env,
        &TypeAwareConfig::default(),
        None,
    )
    .expect_err("unknown value");
    assert_eq!(
        input_error(&err),
        (
            InputField::Env(EnvVar::TypeAwareRequire),
            InputProblem::InvalidValue,
            "FALLOW_TYPE_AWARE_REQUIRE must be best-effort or complete".to_owned()
        )
    );
    let given = TypeAwareInputs {
        require: Some(Given {
            value: TypeAwareRequire::Complete,
            origin: Origin::Flag("--type-aware-require"),
        }),
        ..TypeAwareInputs::default()
    };
    let settings = decide(
        CommandKind::DeadCode,
        &given,
        &env,
        &TypeAwareConfig::default(),
        None,
    )
    .expect("settings");
    assert_eq!(settings.require.value(), &TypeAwareRequire::Complete);
}

/// The first variable in the order of the CLI names the error.
#[test]
fn the_on_value_error_comes_before_the_policy_error() {
    let err = decide(
        CommandKind::DeadCode,
        &TypeAwareInputs::default(),
        &[
            ("FALLOW_TYPE_AWARE", "maybe"),
            ("FALLOW_TYPE_AWARE_REQUIRE", "sometimes"),
        ],
        &TypeAwareConfig::default(),
        None,
    )
    .expect_err("unknown value");
    assert_eq!(input_error(&err).0, InputField::Env(EnvVar::TypeAware));
}
