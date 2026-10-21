//! SVGO plugin.
//!
//! Detects SVGO projects and marks config files as always used.

use super::Plugin;

const ENABLERS: &[&str] = &["svgo"];

const ALWAYS_USED: &[&str] = &["svgo.config.{js,cjs,mjs,ts}"];

const CONFIG_FILE_EXPORTS: &[(&str, &[&str])] = &[("svgo.config.{js,cjs,mjs,ts}", &["default"])];

const TOOLING_DEPENDENCIES: &[&str] = &["svgo"];

define_plugin! {
    struct SvgoPlugin => "svgo",
    enablers: ENABLERS,
    config_file_exports: CONFIG_FILE_EXPORTS,
    always_used: ALWAYS_USED,
    tooling_dependencies: TOOLING_DEPENDENCIES,
}
