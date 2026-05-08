//! Turborepo monorepo build system plugin.
//!
//! Detects Turborepo projects and marks turbo.json and generator config files
//! as always used.

use super::{Plugin, PluginResult};

const ENABLERS: &[&str] = &["turbo"];

const GENERATOR_CONFIG: &str = "turbo/generators/config.{ts,js}";

const CONFIG_PATTERNS: &[&str] = &[GENERATOR_CONFIG];

const ALWAYS_USED: &[&str] = &["turbo.json", GENERATOR_CONFIG];

const TOOLING_DEPENDENCIES: &[&str] = &["turbo"];

define_plugin! {
    struct TurborepoPlugin => "turborepo",
    enablers: ENABLERS,
    config_patterns: CONFIG_PATTERNS,
    always_used: ALWAYS_USED,
    tooling_dependencies: TOOLING_DEPENDENCIES,
    resolve_config: imports_only,
}
