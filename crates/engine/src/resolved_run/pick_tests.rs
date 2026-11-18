use std::path::PathBuf;

use fallow_config::{
    PRODUCTION_DEAD_CODE_ENV, PRODUCTION_DUPES_ENV, PRODUCTION_ENV, PRODUCTION_HEALTH_ENV,
    ProductionAnalysis, ProductionConfig,
};
use fallow_types::sourced::{Given, Layer, Origin, Scope, Source, Sourced};
use proptest::prelude::*;

use super::{PerAnalysis, ProductionInputs, decide_production, pick};
use crate::project_config::ProductionFlags;

const GLOBAL_ORIGIN: Origin = Origin::Flag("--production");
const OWN_ORIGINS: [Origin; 3] = [
    Origin::Flag("--production-dead-code"),
    Origin::Flag("--production-health"),
    Origin::Flag("--production-dupes"),
];
const OWN_ENV: [&str; 3] = [
    PRODUCTION_DEAD_CODE_ENV,
    PRODUCTION_HEALTH_ENV,
    PRODUCTION_DUPES_ENV,
];
const OWN_KEYS: [&str; 3] = [
    "production.deadCode",
    "production.health",
    "production.dupes",
];

const TRI: [Option<bool>; 3] = [None, Some(true), Some(false)];
const ANALYSES: [ProductionAnalysis; 3] = [
    ProductionAnalysis::DeadCode,
    ProductionAnalysis::Health,
    ProductionAnalysis::Dupes,
];

/// The candidate that decides the production mode of one analysis.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Winner {
    OwnFlag,
    GlobalFlag,
    OwnEnv,
    GlobalEnv,
    ConfigAnalysis,
    ConfigGlobal,
    Fallback,
}

/// One combination of production inputs. The arrays hold the dead-code,
/// health and dupes values in that order.
#[derive(Debug, Clone)]
struct Case {
    global_flag: Option<bool>,
    own_flags: [Option<bool>; 3],
    global_env: Option<bool>,
    own_env: [Option<bool>; 3],
    file: Option<PathBuf>,
    config: ProductionConfig,
    config_json: &'static str,
}

const fn index(analysis: ProductionAnalysis) -> usize {
    match analysis {
        ProductionAnalysis::DeadCode => 0,
        ProductionAnalysis::Health => 1,
        ProductionAnalysis::Dupes => 2,
    }
}

/// An independent copy of the precedence of the CLI: the flag of the
/// analysis, the global flag, the variable of the analysis, the global
/// variable, then the config value.
fn reference(case: &Case, analysis: ProductionAnalysis) -> (bool, Winner) {
    let at = index(analysis);
    if let Some(value) = case.own_flags[at] {
        return (value, Winner::OwnFlag);
    }
    if let Some(value) = case.global_flag {
        return (value, Winner::GlobalFlag);
    }
    if let Some(value) = case.own_env[at] {
        return (value, Winner::OwnEnv);
    }
    if let Some(value) = case.global_env {
        return (value, Winner::GlobalEnv);
    }
    match &case.config {
        _ if case.file.is_none() => (case.config.for_analysis(analysis), Winner::Fallback),
        ProductionConfig::Global(value) => (*value, Winner::ConfigGlobal),
        ProductionConfig::PerAnalysis(_) => {
            (case.config.for_analysis(analysis), Winner::ConfigAnalysis)
        }
    }
}

/// The config forms: no file, the bool forms, the object form, and an
/// in-memory config without a file.
fn config_forms() -> Vec<(Option<PathBuf>, &'static str)> {
    let file = || Some(PathBuf::from("project/.fallowrc.json"));
    vec![
        (None, "false"),
        (file(), "true"),
        (file(), "false"),
        (file(), r#"{"deadCode":true,"health":false}"#),
        (None, "true"),
    ]
}

fn triples() -> impl Iterator<Item = [Option<bool>; 3]> {
    TRI.into_iter().flat_map(|a| {
        TRI.into_iter()
            .flat_map(move |b| TRI.into_iter().map(move |c| [a, b, c]))
    })
}

fn every_case() -> Vec<Case> {
    let mut cases = Vec::new();
    for (file, config_json) in config_forms() {
        let config: ProductionConfig = serde_json::from_str(config_json).unwrap();
        for global_flag in TRI {
            for own_flags in triples() {
                for global_env in TRI {
                    for own_env in triples() {
                        cases.push(Case {
                            global_flag,
                            own_flags,
                            global_env,
                            own_env,
                            file: file.clone(),
                            config: config.clone(),
                            config_json,
                        });
                    }
                }
            }
        }
    }
    cases
}

fn wrapper_modes(case: &Case) -> [bool; 3] {
    let flags = ProductionFlags {
        global: case.global_flag,
        dead_code: case.own_flags[0],
        health: case.own_flags[1],
        dupes: case.own_flags[2],
    };
    let env = ProductionFlags {
        global: case.global_env,
        dead_code: case.own_env[0],
        health: case.own_env[1],
        dupes: case.own_env[2],
    };
    if case.global_env.is_none() && case.own_env == [None; 3] {
        assert_direct_modes(case, flags);
    }
    let combined = flags.with_fallback(env);
    let modes = combined.effective_modes(&case.config);
    for analysis in ANALYSES {
        assert_eq!(
            combined.effective(analysis, &case.config),
            [modes.dead_code, modes.health, modes.dupes][index(analysis)],
            "{case:?} {analysis:?}"
        );
    }
    [modes.dead_code, modes.health, modes.dupes]
}

/// `with_fallback` always clears `global`, so this check calls
/// `effective` and `effective_modes` on the flags themselves. Only then
/// does a global flag reach both methods.
fn assert_direct_modes(case: &Case, flags: ProductionFlags) {
    let modes = flags.effective_modes(&case.config);
    let modes = [modes.dead_code, modes.health, modes.dupes];
    for analysis in ANALYSES {
        let (value, _) = reference(case, analysis);
        assert_eq!(
            modes[index(analysis)],
            value,
            "effective_modes, {} {case:?} {analysis:?}",
            case.config_json
        );
        assert_eq!(
            flags.effective(analysis, &case.config),
            value,
            "effective, {} {case:?} {analysis:?}",
            case.config_json
        );
    }
}

fn inputs(case: &Case) -> ProductionInputs {
    let own_flag = |at: usize| {
        case.own_flags[at].map(|value| Given {
            value,
            origin: OWN_ORIGINS[at],
        })
    };
    ProductionInputs {
        global_flag: case.global_flag.map(|value| Given {
            value,
            origin: GLOBAL_ORIGIN,
        }),
        own_flag: PerAnalysis {
            dead_code: own_flag(0),
            health: own_flag(1),
            dupes: own_flag(2),
        },
        global_env: case.global_env,
        own_env: PerAnalysis {
            dead_code: case.own_env[0],
            health: case.own_env[1],
            dupes: case.own_env[2],
        },
    }
}

fn expected(case: &Case, analysis: ProductionAnalysis) -> Sourced<bool> {
    let at = index(analysis);
    let (value, winner) = reference(case, analysis);
    let config_file = |key| Source::ConfigFile {
        file: case.file.clone().unwrap(),
        key,
    };
    let (source, scope) = match winner {
        Winner::OwnFlag => (
            Source::Given {
                origin: OWN_ORIGINS[at],
            },
            Scope::Analysis,
        ),
        Winner::GlobalFlag => (
            Source::Given {
                origin: GLOBAL_ORIGIN,
            },
            Scope::Global,
        ),
        Winner::OwnEnv => (Source::Environment { name: OWN_ENV[at] }, Scope::Analysis),
        Winner::GlobalEnv => (
            Source::Environment {
                name: PRODUCTION_ENV,
            },
            Scope::Global,
        ),
        Winner::ConfigAnalysis => (config_file(OWN_KEYS[at]), Scope::Analysis),
        Winner::ConfigGlobal => (config_file("production"), Scope::Global),
        Winner::Fallback if case.config == ProductionConfig::default() => (
            Source::Default {
                rule: "production-off",
            },
            Scope::Global,
        ),
        Winner::Fallback => (Source::Derived { from: "config" }, Scope::Global),
    };
    Sourced::new(value, source, scope)
}

#[test]
fn decide_production_equals_production_flags_for_every_input() {
    for case in every_case() {
        let wrapper = wrapper_modes(&case);
        let decided = decide_production(&inputs(&case), &case.config, case.file.as_deref());
        let decided = [decided.dead_code, decided.health, decided.dupes];
        for analysis in ANALYSES {
            let (value, _) = reference(&case, analysis);
            assert_eq!(
                wrapper[index(analysis)],
                value,
                "ProductionFlags, {} {case:?} {analysis:?}",
                case.config_json
            );
            assert_eq!(
                decided[index(analysis)],
                expected(&case, analysis),
                "decide_production, {} {case:?} {analysis:?}",
                case.config_json
            );
        }
    }
}

const LAYERS: [Layer; 4] = [
    Layer::Default,
    Layer::ConfigFile,
    Layer::Environment,
    Layer::Given,
];
const SCOPES: [Scope; 2] = [Scope::Global, Scope::Analysis];

fn source_of(layer: Layer) -> Source {
    match layer {
        Layer::Default => Source::Default { rule: "test" },
        Layer::ConfigFile => Source::ConfigFile {
            file: PathBuf::from("project/.fallowrc.json"),
            key: "test",
        },
        Layer::Environment => Source::Environment { name: "TEST" },
        Layer::Given => Source::Given {
            origin: Origin::Flag("--test"),
        },
    }
}

proptest! {
    #[test]
    fn pick_returns_the_highest_rank(
        slots in prop::collection::vec(prop::option::of((0..LAYERS.len(), 0..SCOPES.len())), 0..8)
    ) {
        let candidates: Vec<Option<Sourced<usize>>> = slots
            .iter()
            .enumerate()
            .map(|(at, slot)| {
                slot.map(|(layer, scope)| Sourced::new(at, source_of(LAYERS[layer]), SCOPES[scope]))
            })
            .collect();
        let fallback = Sourced::new(usize::MAX, Source::Default { rule: "fallback" }, Scope::Global);
        let picked = pick(fallback.clone(), candidates.clone());
        let present: Vec<&Sourced<usize>> = candidates.iter().flatten().collect();
        match present.iter().map(|candidate| candidate.rank()).max() {
            None => prop_assert_eq!(picked, fallback),
            Some(highest) => {
                let first = present
                    .iter()
                    .find(|candidate| candidate.rank() == highest)
                    .unwrap();
                prop_assert_eq!(&picked, *first);
            }
        }
    }
}
