//! Ava test runner plugin.
//!
//! Detects Ava projects and marks test files as entry points.

use super::Plugin;

const ENABLERS: &[&str] = &["ava"];

const ENTRY_PATTERNS: &[&str] = &[
    "test/**/*.{ts,tsx,js,jsx}",
    "tests/**/*.{ts,tsx,js,jsx}",
    "**/*.test.{ts,tsx,js,jsx}",
    "**/*.spec.{ts,tsx,js,jsx}",
];

const CONFIG_PATTERNS: &[&str] = &["ava.config.{js,cjs,mjs}"];

const ALWAYS_USED: &[&str] = &["ava.config.{js,cjs,mjs}"];

const TOOLING_DEPENDENCIES: &[&str] = &["ava", "@ava/typescript"];

define_plugin! {
    struct AvaPlugin => "ava",
    enablers: ENABLERS,
    entry_patterns: ENTRY_PATTERNS,
    config_patterns: CONFIG_PATTERNS,
    always_used: ALWAYS_USED,
    tooling_dependencies: TOOLING_DEPENDENCIES,
}
