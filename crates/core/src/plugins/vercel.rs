//! Vercel plugin.

use super::{Plugin, PluginResult};

const ENABLERS: &[&str] = &["vercel", "@vercel/config"];

const CONFIG_PATTERNS: &[&str] = &["vercel.{ts,js,mjs,cjs,mts}"];

const ALWAYS_USED: &[&str] = &["vercel.{ts,js,mjs,cjs,mts}"];

const TOOLING_DEPENDENCIES: &[&str] = &["vercel", "@vercel/config"];

define_plugin! {
    struct VercelPlugin => "vercel",
    enablers: ENABLERS,
    config_patterns: CONFIG_PATTERNS,
    always_used: ALWAYS_USED,
    tooling_dependencies: TOOLING_DEPENDENCIES,
    resolve_config: imports_only,
}
