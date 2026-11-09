//! vite-plugin-pages plugin.
//!
//! vite-plugin-pages makes a route of each page file under its page
//! directories, so no source file imports a page. The plugin reads the
//! `Pages(...)` call in the Vite config and makes the files under each `dirs`
//! entry, with the `extensions` of the call, entry points. Without `dirs`,
//! the page directory is `src/pages`. The plugin reads `dirs` relative to the
//! Vite root, the same as vite-plugin-pages.

use std::path::{Path, PathBuf};

use fallow_config::EntryPointRole;

use super::config_parser::vite_plugin_call::{
    StringListOption, extract_vite_plugin_call_string_lists,
};
use super::{PathRule, Plugin, PluginResult, config_parser};

const PACKAGE: &str = "vite-plugin-pages";

const ENABLERS: &[&str] = &[PACKAGE];

const CONFIG_PATTERNS: &[&str] = &[
    "vite.config.{ts,js,mts,mjs,cts,cjs}",
    "vite.config.*.{ts,js,mts,mjs,cts,cjs}",
];

/// The `Pages(...)` options that the plugin reads, in this order.
const OPTION_KEYS: &[&str] = &["dirs", "extensions", "exclude"];

/// The key of an object entry in `dirs`, such as `{ dir: "src/pages" }`.
const DIR_KEY: &str = "dir";

const DEFAULT_DIR: &str = "src/pages";

/// The default page extensions: the union of the defaults of the Vue, React
/// and Solid resolvers.
const DEFAULT_EXTENSIONS: &[&str] = &["vue", "ts", "tsx", "js", "jsx"];

pub struct VitePluginPagesPlugin;

impl Plugin for VitePluginPagesPlugin {
    fn name(&self) -> &'static str {
        PACKAGE
    }

    fn enablers(&self) -> &'static [&'static str] {
        ENABLERS
    }

    fn entry_point_role(&self) -> EntryPointRole {
        EntryPointRole::Runtime
    }

    fn config_patterns(&self) -> &'static [&'static str] {
        CONFIG_PATTERNS
    }

    fn resolve_config(&self, config_path: &Path, source: &str, root: &Path) -> PluginResult {
        let mut result = PluginResult::default();
        let Some(options) = extract_vite_plugin_call_string_lists(
            source,
            config_path,
            PACKAGE,
            OPTION_KEYS,
            DIR_KEY,
        ) else {
            return result;
        };
        let [dirs, extensions, exclude] = options.as_slice() else {
            return result;
        };
        let exclude: &[String] = match exclude {
            StringListOption::Values(globs) => globs,
            _ => &[],
        };
        let dirs = match dirs {
            StringListOption::Missing => vec![DEFAULT_DIR.to_string()],
            StringListOption::Values(dirs) => dirs.clone(),
            StringListOption::Unreadable => return result,
        };
        let extensions = match extensions {
            StringListOption::Values(extensions) if !extensions.is_empty() => extensions
                .iter()
                .map(|extension| extension.trim_start_matches('.').to_string())
                .collect(),
            _ => DEFAULT_EXTENSIONS
                .iter()
                .map(|extension| (*extension).to_string())
                .collect::<Vec<_>>(),
        };
        let base = vite_root_base(source, config_path, root);
        let extension_glob = format!("{{{}}}", extensions.join(","));
        for dir in dirs {
            let dir = dir.trim().trim_end_matches('/');
            if dir.is_empty() {
                continue;
            }
            let normalized = config_parser::normalize_config_path(dir, &base, root)
                .or_else(|| config_parser::parent_relative_config_path(dir, &base, root));
            let Some(normalized) = normalized else {
                continue;
            };
            let dir_glob = globset::escape(normalized.trim_end_matches('/'));
            let mut rule = PathRule::new(format!("{dir_glob}/**/*.{extension_glob}"))
                .with_excluded_globs(
                    exclude
                        .iter()
                        .map(|glob| format!("{dir_glob}/{}", glob.trim_start_matches("./"))),
                );
            rule.parent_relative = normalized.starts_with("../");
            result.entry_patterns.push(rule);
        }
        result
    }
}

/// A path inside the Vite root, for reading `dirs` relative to that root. The
/// root is the `root` option when the config sets it, else the config
/// directory.
fn vite_root_base(source: &str, config_path: &Path, root: &Path) -> PathBuf {
    config_parser::extract_config_path(source, config_path, &["root"])
        .and_then(|raw| config_parser::normalize_filesystem_config_path(&raw, config_path, root))
        .map_or_else(
            || config_path.to_path_buf(),
            |dir| root.join(dir.trim_end_matches('/')).join("vite.config"),
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry_patterns(source: &str) -> Vec<String> {
        VitePluginPagesPlugin
            .resolve_config(
                Path::new("/project/apps/web/vite.config.ts"),
                source,
                Path::new("/project/apps/web"),
            )
            .entry_patterns
            .iter()
            .map(|rule| rule.pattern.clone())
            .collect()
    }

    #[test]
    fn a_call_without_options_uses_the_default_directory() {
        let source = r#"
            import Pages from "vite-plugin-pages";
            export default { plugins: [Pages()] };
        "#;
        assert_eq!(
            entry_patterns(source),
            vec!["src/pages/**/*.{vue,ts,tsx,js,jsx}"]
        );
    }

    #[test]
    fn dirs_strings_and_objects_and_extensions_are_read() {
        let source = r#"
            import Pages from "vite-plugin-pages";
            export default {
                plugins: [
                    Pages({
                        dirs: ["../common/src/pages", { dir: "src/admin", baseRoute: "admin" }],
                        extensions: ["vue", ".md"],
                    }),
                ],
            };
        "#;
        assert_eq!(
            entry_patterns(source),
            vec![
                "../common/src/pages/**/*.{vue,md}",
                "src/admin/**/*.{vue,md}",
            ]
        );
    }

    #[test]
    fn exclude_globs_are_relative_to_each_page_directory() {
        let source = r#"
            import Pages from "vite-plugin-pages";
            export default {
                plugins: [Pages({ dirs: ["src/pages"], exclude: ["**/components/*.vue"] })],
            };
        "#;
        let rules = VitePluginPagesPlugin
            .resolve_config(
                Path::new("/project/vite.config.ts"),
                source,
                Path::new("/project"),
            )
            .entry_patterns;
        assert_eq!(rules.len(), 1);
        assert_eq!(
            rules[0].exclude_globs,
            vec!["src/pages/**/components/*.vue"]
        );
    }

    #[test]
    fn dirs_follow_the_vite_root_option() {
        let source = r#"
            import Pages from "vite-plugin-pages";
            export default { root: "client", plugins: [Pages({ dirs: "pages" })] };
        "#;
        assert_eq!(
            entry_patterns(source),
            vec!["client/pages/**/*.{vue,ts,tsx,js,jsx}"]
        );
    }

    #[test]
    fn unknown_options_and_unrelated_calls_give_no_entries() {
        let variable_options = r#"
            import Pages from "vite-plugin-pages";
            const options = { dirs: "src/views" };
            export default { plugins: [Pages(options)] };
        "#;
        assert!(entry_patterns(variable_options).is_empty());

        let variable_dirs = r#"
            import Pages from "vite-plugin-pages";
            export default { plugins: [Pages({ dirs: pageDirs })] };
        "#;
        assert!(entry_patterns(variable_dirs).is_empty());

        let local_function = r"
            function Pages(options) { return options; }
            export default { plugins: [Pages()] };
        ";
        assert!(entry_patterns(local_function).is_empty());
    }
}
