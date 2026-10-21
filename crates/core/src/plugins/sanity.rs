//! Sanity plugin.
//!
//! Detects Sanity CMS projects and marks config and schema files as entry points.

use super::Plugin;

const ENABLERS: &[&str] = &["sanity", "@sanity/"];

const ENTRY_PATTERNS: &[&str] = &[
    "schemas/**/*.{ts,tsx,js,jsx}",
    "sanity/**/*.{ts,tsx,js,jsx}",
];

const ALWAYS_USED: &[&str] = &["sanity.config.{ts,tsx,js,jsx}", "sanity.cli.{ts,js}"];

const CONFIG_FILE_EXPORTS: &[(&str, &[&str])] = &[
    ("sanity.config.{ts,tsx,js,jsx}", &["default"]),
    ("sanity.cli.{ts,js}", &["default"]),
];

const TOOLING_DEPENDENCIES: &[&str] =
    &["sanity", "@sanity/client", "@sanity/vision", "@sanity/cli"];

define_plugin! {
    struct SanityPlugin => "sanity",
    enablers: ENABLERS,
    entry_patterns: ENTRY_PATTERNS,
    config_file_exports: CONFIG_FILE_EXPORTS,
    always_used: ALWAYS_USED,
    tooling_dependencies: TOOLING_DEPENDENCIES,
}
