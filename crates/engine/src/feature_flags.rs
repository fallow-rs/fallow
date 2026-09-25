//! Feature flag extraction helpers owned by the engine boundary.

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
