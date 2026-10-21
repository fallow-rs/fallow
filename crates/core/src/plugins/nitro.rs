//! Nitro plugin.
//!
//! Detects Nitro server engine projects and marks config files as always used.

use super::Plugin;

const ENABLERS: &[&str] = &["nitropack"];

const ENTRY_PATTERNS: &[&str] = &[
    "server/**/*.{ts,js}",
    "routes/**/*.{ts,js}",
    "api/**/*.{ts,js}",
    "middleware/**/*.{ts,js}",
];

const ALWAYS_USED: &[&str] = &["nitro.config.{ts,js}"];

const CONFIG_FILE_EXPORTS: &[(&str, &[&str])] = &[("nitro.config.{ts,js}", &["default"])];

const TOOLING_DEPENDENCIES: &[&str] = &["nitropack"];

define_plugin! {
    struct NitroPlugin => "nitro",
    enablers: ENABLERS,
    entry_patterns: ENTRY_PATTERNS,
    config_file_exports: CONFIG_FILE_EXPORTS,
    always_used: ALWAYS_USED,
    tooling_dependencies: TOOLING_DEPENDENCIES,
}
