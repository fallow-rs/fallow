//! Alchemy infrastructure-as-code plugin.
//!
//! Alchemy reads a Stack from `alchemy.run.ts` in the working directory, or
//! from the file that a script passes to the CLI (`alchemy deploy
//! ./stacks/app.ts`). Nothing in the project imports a stack file, and a root
//! stack imports the Worker modules that it deploys. Thus each
//! `alchemy.run.*` file is an entry point. A stack runs at deploy time, not in
//! production, so the role is support. A script file argument is an entry
//! point through the shared script reader.
//!
//! The bundle entry that a resource names in its `main` property is read
//! during extraction (`fallow_extract::alchemy`), because a Worker module
//! declares it as often as a stack file does.
//!
//! The Alchemy CLI loads `@effect/platform-node` or `@effect/platform-bun`
//! through a dynamic `import()` from its own code, so a project that uses
//! Alchemy declares one of them without an import of its own.

use super::{Plugin, PluginResult};

const ENABLERS: &[&str] = &["alchemy"];

const STACK_PATTERNS: &[&str] = &["**/alchemy.run.{ts,mts,cts,js,mjs,cjs}"];

/// `alchemy` is the CLI. The two Effect platform packages are optional peers
/// that the CLI loads at runtime for the JavaScript runtime it runs on.
const TOOLING_DEPENDENCIES: &[&str] = &["alchemy", "@effect/platform-node", "@effect/platform-bun"];

define_plugin! {
    struct AlchemyPlugin => "alchemy",
    enablers: ENABLERS,
    entry_patterns: STACK_PATTERNS,
    config_patterns: STACK_PATTERNS,
    always_used: STACK_PATTERNS,
    tooling_dependencies: TOOLING_DEPENDENCIES,
    resolve_config: imports_only,
}

#[cfg(test)]
mod tests {
    use fallow_config::EntryPointRole;

    use super::*;

    #[test]
    fn stack_files_are_support_entry_points_at_any_depth() {
        let plugin = AlchemyPlugin;
        assert_eq!(plugin.entry_point_role(), EntryPointRole::Support);
        let matcher = globset::Glob::new(STACK_PATTERNS[0])
            .expect("valid stack pattern")
            .compile_matcher();
        for path in [
            "alchemy.run.ts",
            "apps/api/alchemy.run.mts",
            "infra/alchemy.run.js",
        ] {
            assert!(matcher.is_match(path), "{path} should match");
        }
        assert!(!matcher.is_match("src/alchemy.ts"));
    }

    #[test]
    fn cli_and_platform_peers_are_tooling_dependencies() {
        let plugin = AlchemyPlugin;
        for dependency in ["alchemy", "@effect/platform-node", "@effect/platform-bun"] {
            assert!(plugin.tooling_dependencies().contains(&dependency));
        }
    }
}
