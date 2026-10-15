//! Parity check between the two routes of one tool.
//!
//! A tool with a typed route and a CLI route maps its params twice: once to
//! `AnalysisOptions`, once to argv. [`assert_routes_agree`] builds both from
//! the same params and checks that they express the same shared settings, so
//! a field that one route reads and the other drops fails a test.

use std::path::PathBuf;

use fallow_api::AnalysisOptions;
use serde::de::DeserializeOwned;

/// The production values a call can send: on, an explicit off, and unset.
const PRODUCTION_CASES: [Option<bool>; 3] = [Some(true), Some(false), None];

/// Params with every shared setting off its default, or with the string
/// settings blank, merged with the fields the tool itself requires.
fn params<P: DeserializeOwned>(
    required: &serde_json::Value,
    production: Option<bool>,
    blank: bool,
) -> P {
    let text = |value: &str| {
        if blank {
            String::new()
        } else {
            value.to_string()
        }
    };
    let mut value = serde_json::json!({
        "root": text("/parity/root"),
        "config": text("parity.json"),
        "allow_remote_extends": true,
        "no_cache": true,
        "threads": 3,
        "workspace": text("web, admin"),
    });
    let fields = value.as_object_mut().expect("params object");
    if let Some(production) = production {
        fields.insert("production".to_string(), production.into());
    }
    for (key, field) in required.as_object().expect("required fields object") {
        fields.insert(key.clone(), field.clone());
    }
    serde_json::from_value(value).expect("parity params deserialize")
}

fn flag_value<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    args.iter()
        .position(|arg| arg == flag)
        .and_then(|index| args.get(index + 1))
        .map(String::as_str)
}

fn has_flag(args: &[String], flag: &str) -> bool {
    args.iter().any(|arg| arg == flag)
}

/// The production choice the argv expresses, in the typed field's terms.
fn argv_production(args: &[String]) -> Option<bool> {
    if has_flag(args, "--production") {
        Some(true)
    } else if has_flag(args, "--no-production") {
        Some(false)
    } else {
        None
    }
}

fn assert_settings_match(tool: &str, case: &str, args: &[String], analysis: &AnalysisOptions) {
    let context = format!("{tool} ({case}): argv {args:?}");
    assert_eq!(
        analysis.root,
        flag_value(args, "--root").map(PathBuf::from),
        "root differs, {context}"
    );
    assert_eq!(
        analysis.config_path,
        flag_value(args, "--config").map(PathBuf::from),
        "config differs, {context}"
    );
    assert_eq!(
        analysis.no_cache,
        has_flag(args, "--no-cache"),
        "no_cache differs, {context}"
    );
    assert_eq!(
        analysis.threads,
        flag_value(args, "--threads").map(|value| value.parse().expect("threads number")),
        "threads differs, {context}"
    );
    assert_eq!(
        analysis.allow_remote_extends,
        has_flag(args, "--allow-remote-extends"),
        "allow_remote_extends differs, {context}"
    );
    assert_eq!(
        analysis.production_override,
        argv_production(args),
        "production differs, {context}"
    );
    assert_eq!(
        analysis.workspace,
        super::api_runtime::workspace_patterns_from_param(flag_value(args, "--workspace")),
        "workspace differs, {context}"
    );
}

/// Build both routes of `tool` from equal params and assert that they
/// agree on root, config, cache, threads, remote extends, production and
/// workspace. `required` holds the tool's own mandatory fields.
#[track_caller]
pub(super) fn assert_routes_agree<P: DeserializeOwned>(
    tool: &str,
    required: &serde_json::Value,
    argv: impl Fn(P) -> Vec<String>,
    typed: impl Fn(P) -> AnalysisOptions,
) {
    for blank in [false, true] {
        for production in PRODUCTION_CASES {
            let args = argv(params(required, production, blank));
            let analysis = typed(params(required, production, blank));
            let case = format!("production {production:?}, blank strings {blank}");
            assert_settings_match(tool, &case, &args, &analysis);
        }
    }
}
