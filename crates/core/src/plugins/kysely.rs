//! Kysely plugin.
//!
//! Detects Kysely projects and marks config and migration files as always used.

use super::Plugin;

const ENABLERS: &[&str] = &["kysely", "kysely-ctl"];

const ENTRY_PATTERNS: &[&str] = &["migrations/**/*.{ts,js}", "src/migrations/**/*.{ts,js}"];

const ALWAYS_USED: &[&str] = &["kysely.config.{ts,js}", ".config/kysely.config.{ts,js}"];

const CONFIG_FILE_EXPORTS: &[(&str, &[&str])] = &[("kysely.config.{ts,js}", &["default"])];

const TOOLING_DEPENDENCIES: &[&str] = &["kysely", "kysely-ctl"];

define_plugin! {
    struct KyselyPlugin => "kysely",
    enablers: ENABLERS,
    entry_patterns: ENTRY_PATTERNS,
    config_file_exports: CONFIG_FILE_EXPORTS,
    always_used: ALWAYS_USED,
    tooling_dependencies: TOOLING_DEPENDENCIES,
}
