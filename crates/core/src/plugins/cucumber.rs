//! Cucumber plugin.
//!
//! Detects Cucumber BDD projects and marks step definitions as entry points.

use super::Plugin;

const ENABLERS: &[&str] = &["@cucumber/cucumber"];

const ENTRY_PATTERNS: &[&str] = &[
    "features/**/*.{ts,js}",
    "**/*.steps.{ts,js}",
    "step_definitions/**/*.{ts,js}",
    "support/**/*.{ts,js}",
];

const ALWAYS_USED: &[&str] = &[
    "cucumber.{js,cjs,mjs,ts}",
    "cucumber.config.{js,cjs,mjs,ts}",
    ".cucumber.{js,yml}",
];

/// cucumber-js reads the default export as the default profile and each
/// named export as a named profile.
const CONFIG_FILE_EXPORTS: &[(&str, &[&str])] = &[
    ("cucumber.{js,cjs,mjs,ts}", &["*"]),
    ("cucumber.config.{js,cjs,mjs,ts}", &["*"]),
];

const TOOLING_DEPENDENCIES: &[&str] = &["@cucumber/cucumber"];

define_plugin! {
    struct CucumberPlugin => "cucumber",
    enablers: ENABLERS,
    entry_patterns: ENTRY_PATTERNS,
    config_file_exports: CONFIG_FILE_EXPORTS,
    always_used: ALWAYS_USED,
    tooling_dependencies: TOOLING_DEPENDENCIES,
}
