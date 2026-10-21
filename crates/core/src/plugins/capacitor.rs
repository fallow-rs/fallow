//! Capacitor plugin.
//!
//! Detects Capacitor cross-platform mobile projects and marks config files as always used.

use super::Plugin;

const ENABLERS: &[&str] = &["@capacitor/core", "@capacitor/cli"];

const ALWAYS_USED: &[&str] = &["capacitor.config.{ts,js,json}"];

const CONFIG_FILE_EXPORTS: &[(&str, &[&str])] = &[("capacitor.config.{ts,js}", &["default"])];

const TOOLING_DEPENDENCIES: &[&str] = &["@capacitor/core", "@capacitor/cli"];

define_plugin! {
    struct CapacitorPlugin => "capacitor",
    enablers: ENABLERS,
    config_file_exports: CONFIG_FILE_EXPORTS,
    always_used: ALWAYS_USED,
    tooling_dependencies: TOOLING_DEPENDENCIES,
}
