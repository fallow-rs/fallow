//! The runner protocol manifest, embedded at compile time and validated
//! before the first spawn.

use std::sync::OnceLock;

use serde::Deserialize;
use serde::de::IgnoredAny;

const MANIFEST_SOURCE: &str = include_str!("../../mutation-runner-protocol.json");

/// The request types that the scheduler sends.
const REQUIRED_REQUESTS: &[&str] = &["init", "dry_run", "run", "reload", "shutdown"];
/// The response types that the transport decodes.
const REQUIRED_RESPONSES: &[&str] = &["ready", "dry_run_result", "run_result", "reloaded", "error"];
/// The run statuses that the scheduler maps.
const REQUIRED_RUN_STATUSES: &[&str] = &["killed", "survived", "timeout", "runtime_error"];
/// The failure kinds that a run result can carry.
const REQUIRED_FAILURE_KINDS: &[&str] = &["assertion", "error", "timeout"];
/// The reload methods that the scheduler maps.
const REQUIRED_RELOAD_METHODS: &[&str] = &["invalidate", "noop", "restart_required"];
/// The capabilities that the scheduler reads.
const REQUIRED_CAPABILITIES: &[&str] = &[
    "per_test_attribution",
    "test_filter",
    "hot_switch",
    "bail",
    "fresh_modules_per_run",
    "batch_mutants",
];
/// The test statuses that the scheduler reads.
const REQUIRED_TEST_STATUSES: &[&str] = &["pass", "fail", "skip", "todo"];

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub schema_version: u32,
    pub wire_protocol_version: u32,
    pub runtime_semantics_version: u32,
    /// The environment variable that carries the session token of a
    /// companion. Each protocol line starts with the token (`protocol.md`
    /// section 2).
    pub session_token_env: String,
    pub request_types: Vec<String>,
    pub response_types: Vec<String>,
    pub test_statuses: Vec<String>,
    pub run_statuses: Vec<String>,
    pub failure_kinds: Vec<String>,
    pub reload_methods: Vec<String>,
    pub capabilities: Vec<String>,
    // The companion owns these values. The API never reads them, but the
    // manifest still names them, so `deny_unknown_fields` must accept them.
    #[serde(rename = "error_codes")]
    _error_codes: IgnoredAny,
    #[serde(rename = "isolation_modes")]
    _isolation_modes: IgnoredAny,
    #[serde(rename = "in_band_markers")]
    _in_band_markers: IgnoredAny,
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "only the drift test reads the runtime names")
    )]
    pub runtime: RuntimeNames,
    pub runners: Vec<RunnerRange>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "only the drift test reads the runtime names")
)]
pub struct RuntimeNames {
    #[serde(rename = "global_name")]
    _global_name: IgnoredAny,
    pub active_function: String,
    pub hit_function: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunnerRange {
    pub name: String,
    pub package: String,
    pub min_version: String,
    pub max_version_exclusive: String,
}

impl RunnerRange {
    /// True when `version` is in `[min_version, max_version_exclusive)`. A
    /// version that does not parse is outside the range.
    pub fn contains(&self, version: &str) -> bool {
        let (Some(version), Some(min), Some(max)) = (
            parse_version(version),
            parse_version(&self.min_version),
            parse_version(&self.max_version_exclusive),
        ) else {
            return false;
        };
        min <= version && version < max
    }
}

/// Parses the `major.minor.patch` prefix of a version. A pre-release suffix
/// is ignored.
fn parse_version(text: &str) -> Option<(u64, u64, u64)> {
    let core = text
        .trim()
        .trim_start_matches('v')
        .split(['-', '+'])
        .next()?;
    let mut parts = core.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = parts.next()?.parse().ok()?;
    parts.next().is_none().then_some((major, minor, patch))
}

impl Manifest {
    pub fn runner(&self, name: &str) -> Option<&RunnerRange> {
        self.runners.iter().find(|runner| runner.name == name)
    }
}

fn require_all(field: &str, values: &[String], required: &[&str]) -> Result<(), String> {
    for name in required {
        if !values.iter().any(|value| value == name) {
            return Err(format!("the manifest field `{field}` has no `{name}`"));
        }
    }
    Ok(())
}

/// True for an uppercase environment variable name, such as `FALLOW_X`.
fn is_env_name(name: &str) -> bool {
    name.starts_with(|first: char| first.is_ascii_uppercase())
        && name
            .chars()
            .all(|char| char.is_ascii_uppercase() || char.is_ascii_digit() || char == '_')
}

/// Parses and validates a manifest text.
pub fn parse_manifest(source: &str) -> Result<Manifest, String> {
    let manifest: Manifest = serde_json::from_str(source)
        .map_err(|error| format!("the mutation runner manifest does not parse: {error}"))?;
    if manifest.schema_version != 1 {
        return Err(format!(
            "unsupported mutation runner manifest schema {}",
            manifest.schema_version
        ));
    }
    if manifest.wire_protocol_version == 0 || manifest.runtime_semantics_version == 0 {
        return Err("the wire and runtime semantics versions must be positive".to_owned());
    }
    if !is_env_name(&manifest.session_token_env) {
        return Err(format!(
            "the session token variable `{}` is not an environment variable name",
            manifest.session_token_env
        ));
    }
    require_all("request_types", &manifest.request_types, REQUIRED_REQUESTS)?;
    require_all(
        "response_types",
        &manifest.response_types,
        REQUIRED_RESPONSES,
    )?;
    require_all(
        "run_statuses",
        &manifest.run_statuses,
        REQUIRED_RUN_STATUSES,
    )?;
    require_all(
        "test_statuses",
        &manifest.test_statuses,
        REQUIRED_TEST_STATUSES,
    )?;
    require_all(
        "failure_kinds",
        &manifest.failure_kinds,
        REQUIRED_FAILURE_KINDS,
    )?;
    require_all(
        "reload_methods",
        &manifest.reload_methods,
        REQUIRED_RELOAD_METHODS,
    )?;
    require_all(
        "capabilities",
        &manifest.capabilities,
        REQUIRED_CAPABILITIES,
    )?;
    if manifest.runners.is_empty() {
        return Err("the manifest names no runner".to_owned());
    }
    for runner in &manifest.runners {
        let (Some(min), Some(max)) = (
            parse_version(&runner.min_version),
            parse_version(&runner.max_version_exclusive),
        ) else {
            return Err(format!(
                "runner `{}` has a version that does not parse",
                runner.name
            ));
        };
        if min >= max {
            return Err(format!(
                "runner `{}` has an empty version range",
                runner.name
            ));
        }
    }
    Ok(manifest)
}

/// The embedded manifest.
///
/// # Errors
///
/// Returns the validation error of the embedded manifest. A build with an
/// invalid manifest cannot start a companion.
pub fn manifest() -> Result<&'static Manifest, String> {
    static MANIFEST: OnceLock<Result<Manifest, String>> = OnceLock::new();
    MANIFEST
        .get_or_init(|| parse_manifest(MANIFEST_SOURCE))
        .as_ref()
        .map_err(Clone::clone)
}

#[expect(clippy::unwrap_used, reason = "tests unwrap to keep assertions short")]
#[cfg(test)]
mod tests {
    use super::*;

    fn embedded() -> serde_json::Value {
        serde_json::from_str(MANIFEST_SOURCE).expect("the manifest is JSON")
    }

    fn parse_value(value: &serde_json::Value) -> Result<Manifest, String> {
        parse_manifest(&value.to_string())
    }

    #[test]
    fn the_embedded_manifest_validates_and_names_vitest_4_and_5() {
        let manifest = manifest().expect("valid manifest");
        assert_eq!(manifest.wire_protocol_version, 1);
        let vitest = manifest.runner("vitest").expect("vitest runner");
        assert!(vitest.contains("4.0.0"));
        assert!(vitest.contains("5.0.4"));
        assert!(vitest.contains("v4.1.11"));
        assert!(!vitest.contains("3.2.4"));
        assert!(!vitest.contains("6.0.0"));
        assert!(!vitest.contains("not-a-version"));
        assert!(manifest.runner("jest").is_none());
    }

    #[test]
    fn the_runtime_names_match_the_instrumentation_crate() {
        let manifest = manifest().expect("valid manifest");
        assert_eq!(
            manifest.runtime.active_function,
            fallow_mutation::ACTIVE_FUNCTION
        );
        assert_eq!(manifest.runtime.hit_function, fallow_mutation::HIT_FUNCTION);
    }

    #[test]
    fn validation_rejects_a_zero_wire_version() {
        let mut value = embedded();
        value["wire_protocol_version"] = serde_json::json!(0);
        assert!(parse_value(&value).unwrap_err().contains("positive"));
    }

    #[test]
    fn validation_rejects_a_missing_run_status() {
        let mut value = embedded();
        value["run_statuses"] = serde_json::json!(["killed", "timeout", "runtime_error"]);
        assert_eq!(
            parse_value(&value).unwrap_err(),
            "the manifest field `run_statuses` has no `survived`"
        );
    }

    #[test]
    fn validation_rejects_an_unknown_field_and_an_empty_range() {
        let mut value = embedded();
        value["extra"] = serde_json::json!(true);
        assert!(parse_value(&value).unwrap_err().contains("does not parse"));

        let mut value = embedded();
        value["runners"][0]["max_version_exclusive"] = serde_json::json!("4.0.0");
        assert!(
            parse_value(&value)
                .unwrap_err()
                .contains("empty version range")
        );
    }

    #[test]
    fn validation_rejects_a_session_token_variable_that_is_not_a_name() {
        for name in ["", "fallow_token", "1TOKEN", "FALLOW TOKEN"] {
            let mut value = embedded();
            value["session_token_env"] = serde_json::json!(name);
            assert!(
                parse_value(&value)
                    .unwrap_err()
                    .contains("not an environment variable name"),
                "{name:?}"
            );
        }
        let mut value = embedded();
        value["session_token_env"] = serde_json::json!("FALLOW_TOKEN_2");
        assert_eq!(
            parse_value(&value).unwrap().session_token_env,
            "FALLOW_TOKEN_2"
        );
    }

    #[test]
    fn a_pre_release_parses_as_its_core_version() {
        assert_eq!(parse_version("5.1.0-beta.2"), Some((5, 1, 0)));
        assert_eq!(parse_version("5.1"), None);
        assert_eq!(parse_version("5.1.0.1"), None);
    }
}
