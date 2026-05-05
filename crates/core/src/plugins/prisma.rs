//! Prisma ORM plugin.
//!
//! Detects Prisma projects and marks seed files as entry points
//! and schema/config files as always used.

use super::Plugin;

const ENABLERS: &[&str] = &["prisma", "@prisma/client"];

const ENTRY_PATTERNS: &[&str] = &["prisma/seed.{ts,js}"];

// `prisma.config.{ts,mts,cts,js,mjs,cjs}` is the officially-supported config
// file location introduced in Prisma 6.x. Prisma loads it directly, so no
// source file imports it; without this entry it is reported as unused.
const CONFIG_PATTERNS: &[&str] = &["prisma.config.{ts,mts,cts,js,mjs,cjs}"];

const ALWAYS_USED: &[&str] = &[
    "prisma/schema.prisma",
    "prisma.config.{ts,mts,cts,js,mjs,cjs}",
];

const TOOLING_DEPENDENCIES: &[&str] = &["prisma", "@prisma/client"];

define_plugin! {
    struct PrismaPlugin => "prisma",
    enablers: ENABLERS,
    entry_patterns: ENTRY_PATTERNS,
    config_patterns: CONFIG_PATTERNS,
    always_used: ALWAYS_USED,
    tooling_dependencies: TOOLING_DEPENDENCIES,
}
