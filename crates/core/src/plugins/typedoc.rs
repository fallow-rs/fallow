//! `TypeDoc` plugin.
//!
//! Detects `TypeDoc` projects and marks config files as always used.

use super::Plugin;

const ENABLERS: &[&str] = &["typedoc"];

const CONFIG_PATTERNS: &[&str] = &["typedoc.config.{js,cjs,mjs,ts}"];

const ALWAYS_USED: &[&str] = &["typedoc.{json,jsonc}", "typedoc.config.{js,cjs,mjs,ts}"];

const TOOLING_DEPENDENCIES: &[&str] = &["typedoc"];

define_plugin! {
    struct TypedocPlugin => "typedoc",
    enablers: ENABLERS,
    config_patterns: CONFIG_PATTERNS,
    always_used: ALWAYS_USED,
    tooling_dependencies: TOOLING_DEPENDENCIES,
}
