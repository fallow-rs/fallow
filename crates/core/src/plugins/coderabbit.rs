//! CodeRabbit plugin.
//!
//! CodeRabbit loads `.coderabbit.config.ts` from the repository root by name.
//! The file imports its helpers from `@coderabbitai/config`.

use super::Plugin;

const ENABLERS: &[&str] = &["@coderabbitai/config"];

const ALWAYS_USED: &[&str] = &[".coderabbit.config.{ts,mts,cts,js,mjs,cjs}"];

const TOOLING_DEPENDENCIES: &[&str] = &["@coderabbitai/config"];

define_plugin! {
    struct CodeRabbitPlugin => "coderabbit",
    enablers: ENABLERS,
    always_used: ALWAYS_USED,
    tooling_dependencies: TOOLING_DEPENDENCIES,
}
