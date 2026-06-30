//! Feature flag extraction helpers owned by the engine boundary.

use std::path::Path;

use fallow_types::extract::FlagUse;

/// Built-in environment variable prefixes treated as feature flags.
#[must_use]
pub fn builtin_env_prefixes() -> &'static [&'static str] {
    fallow_extract::flags::builtin_env_prefixes()
}

/// Distinct built-in SDK provider labels, in declaration order.
#[must_use]
pub fn builtin_sdk_providers() -> Vec<&'static str> {
    fallow_extract::flags::builtin_sdk_providers()
}

/// Extract feature flags from source text using caller-provided config.
#[must_use]
pub fn extract_flags_from_source(
    source: &str,
    path: &Path,
    extra_sdk_patterns: &[(String, usize, String)],
    extra_env_prefixes: &[String],
    config_object_heuristics: bool,
) -> Vec<FlagUse> {
    fallow_extract::flags::extract_flags_from_source(
        source,
        path,
        extra_sdk_patterns,
        extra_env_prefixes,
        config_object_heuristics,
    )
}
