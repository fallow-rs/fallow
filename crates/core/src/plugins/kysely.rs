//! Kysely plugin.
//!
//! Detects Kysely projects and marks config and migration files as always used.
//! Credits the `up` and `down` exports of migration files.

use super::Plugin;

const ENABLERS: &[&str] = &["kysely", "kysely-ctl"];

const ENTRY_PATTERNS: &[&str] = &["migrations/**/*.{ts,js}", "src/migrations/**/*.{ts,js}"];

const ALWAYS_USED: &[&str] = &["kysely.config.{ts,js}", ".config/kysely.config.{ts,js}"];

const CONFIG_FILE_EXPORTS: &[(&str, &[&str])] = &[("kysely.config.{ts,js}", &["default"])];

const TOOLING_DEPENDENCIES: &[&str] = &["kysely", "kysely-ctl"];

/// The files that a Kysely migration folder can hold. A `migrationFolder` can
/// sit anywhere, so the rule matches a `migrations` folder at any depth.
const MIGRATION_FILES: &str = "**/migrations/*.{ts,js,mts,mjs,cts,cjs}";

/// The exports that Kysely reads from each migration file.
const MIGRATION_EXPORTS: &[&str] = &["up", "down"];

define_plugin! {
    struct KyselyPlugin => "kysely",
    enablers: ENABLERS,
    entry_patterns: ENTRY_PATTERNS,
    config_file_exports: CONFIG_FILE_EXPORTS,
    always_used: ALWAYS_USED,
    tooling_dependencies: TOOLING_DEPENDENCIES,
    used_exports: [(MIGRATION_FILES, MIGRATION_EXPORTS)],
}
