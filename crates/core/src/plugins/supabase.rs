//! Supabase Edge Functions plugin.
//!
//! Supabase Edge Functions run in Deno under `supabase/functions/<name>/`, with
//! `index.ts` as the deployed entry point by convention. The Node application
//! graph never imports these files, so without this plugin every function entry
//! surfaces as an `unused-file`. This plugin marks the per-function `index.*`
//! files as runtime entry roots; shared code under `supabase/functions/_shared`
//! and any per-function helpers stay reachable through their normal relative
//! imports (and genuinely-orphaned shared files remain reportable).
//!
//! Deno import schemes (`jsr:`, `npm:`, URL) are handled globally in the
//! resolver rather than here, because they are unambiguous and appear outside
//! Supabase projects too. See issue #624.

use std::path::Path;

use super::Plugin;

/// Package names that activate the plugin from package.json. `supabase` is the
/// CLI, typically a devDependency invoked from scripts (`supabase functions
/// deploy`); crediting it as tooling keeps it out of `unused-dependencies`.
const ENABLERS: &[&str] = &["supabase"];

/// Per-function entry points. Supabase deploys `supabase/functions/<name>/index.ts`
/// by default; the glob matches one function-directory level and the JS/TS-family
/// extensions Deno accepts. `_shared` helpers are reached via relative imports
/// from these entries rather than being entries themselves.
const ENTRY_PATTERNS: &[&str] = &["supabase/functions/*/index.{ts,tsx,js,jsx,mts,mjs,cts,cjs}"];

/// The Supabase CLI is invoked from scripts, not imported, so credit it as a
/// tooling dependency so it never reports as unused.
const TOOLING_DEPENDENCIES: &[&str] = &["supabase"];

/// Built-in plugin for Supabase Edge Function projects.
pub struct SupabasePlugin;

impl Plugin for SupabasePlugin {
    fn name(&self) -> &'static str {
        "supabase"
    }

    fn enablers(&self) -> &'static [&'static str] {
        ENABLERS
    }

    /// Activate on the `supabase` CLI dependency OR a filesystem signal. Supabase
    /// is frequently installed globally (brew / npx) rather than as a project
    /// dependency, so the `supabase/config.toml` file and the
    /// `supabase/functions/` directory are the robust activation signals.
    fn is_enabled_with_deps(&self, deps: &[String], root: &Path) -> bool {
        deps.iter().any(|dep| dep == "supabase")
            || root.join("supabase/config.toml").is_file()
            || root.join("supabase/functions").is_dir()
    }

    fn entry_patterns(&self) -> &'static [&'static str] {
        ENTRY_PATTERNS
    }

    fn tooling_dependencies(&self) -> &'static [&'static str] {
        TOOLING_DEPENDENCIES
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn activates_on_supabase_dependency() {
        let plugin = SupabasePlugin;
        assert!(plugin.is_enabled_with_deps(&["supabase".to_string()], Path::new("/nonexistent")));
    }

    #[test]
    fn activates_on_config_toml() {
        let temp = std::env::temp_dir().join("fallow-supabase-config-test");
        let _ = std::fs::remove_dir_all(&temp);
        std::fs::create_dir_all(temp.join("supabase")).unwrap();
        std::fs::write(temp.join("supabase/config.toml"), "project_id = \"x\"\n").unwrap();

        let plugin = SupabasePlugin;
        assert!(plugin.is_enabled_with_deps(&[], &temp));

        let _ = std::fs::remove_dir_all(&temp);
    }

    #[test]
    fn activates_on_functions_directory() {
        let temp = std::env::temp_dir().join("fallow-supabase-functions-test");
        let _ = std::fs::remove_dir_all(&temp);
        std::fs::create_dir_all(temp.join("supabase/functions/hello")).unwrap();

        let plugin = SupabasePlugin;
        assert!(plugin.is_enabled_with_deps(&[], &temp));

        let _ = std::fs::remove_dir_all(&temp);
    }

    #[test]
    fn does_not_activate_without_any_signal() {
        let temp = std::env::temp_dir().join("fallow-supabase-negative-test");
        let _ = std::fs::remove_dir_all(&temp);
        std::fs::create_dir_all(&temp).unwrap();

        let plugin = SupabasePlugin;
        assert!(!plugin.is_enabled_with_deps(&["react".to_string()], &temp));

        let _ = std::fs::remove_dir_all(&temp);
    }

    #[test]
    fn entry_pattern_targets_per_function_index() {
        let plugin = SupabasePlugin;
        assert_eq!(
            plugin.entry_patterns(),
            &["supabase/functions/*/index.{ts,tsx,js,jsx,mts,mjs,cts,cjs}"]
        );
    }

    #[test]
    fn credits_supabase_cli_as_tooling() {
        let plugin = SupabasePlugin;
        assert!(plugin.tooling_dependencies().contains(&"supabase"));
    }

    #[test]
    fn claims_no_config_patterns() {
        // config.toml is not a JS/TS source file, so there is nothing to parse;
        // activation reads it directly via is_enabled_with_deps.
        let plugin = SupabasePlugin;
        assert!(plugin.config_patterns().is_empty());
    }
}
