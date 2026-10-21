//! `CSpell` plugin.
//!
//! Detects `CSpell` projects and marks config files as always used.

use super::Plugin;

const ENABLERS: &[&str] = &["cspell"];

const ALWAYS_USED: &[&str] = &[
    ".cspell.{json,jsonc,yml,yaml}",
    "cspell.{json,jsonc}",
    "cspell.config.{js,cjs,mjs,yaml,yml}",
];

const CONFIG_FILE_EXPORTS: &[(&str, &[&str])] = &[("cspell.config.{js,cjs,mjs}", &["default"])];

const TOOLING_DEPENDENCIES: &[&str] = &["cspell"];

define_plugin! {
    struct CspellPlugin => "cspell",
    enablers: ENABLERS,
    config_file_exports: CONFIG_FILE_EXPORTS,
    always_used: ALWAYS_USED,
    tooling_dependencies: TOOLING_DEPENDENCIES,
}
