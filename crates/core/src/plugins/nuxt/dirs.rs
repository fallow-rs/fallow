//! Source and convention directories of a Nuxt config: `srcDir` and the
//! `dir` object.

use std::path::{Path, PathBuf};

use super::{
    PluginResult, add_default_used_export, add_prefixed_default_used_exports, config_parser,
    extend_prefixed_patterns, normalize_nuxt_path, prefix_with_src_dir,
};

const SRC_DIR_ENTRY_PATTERNS: &[&str] = &[
    "pages/**/*.{vue,ts,tsx,js,jsx}",
    "layouts/**/*.{vue,ts,tsx,js,jsx}",
    "middleware/*.{ts,tsx,js,jsx,mts,cts,mjs,cjs}",
    "middleware/*/index.{ts,tsx,js,jsx,mts,cts,mjs,cjs}",
    "plugins/*.{ts,tsx,js,jsx,mts,cts,mjs,cjs}",
    "plugins/*/index.{ts,tsx,js,jsx,mts,cts,mjs,cjs}",
    "composables/*.{ts,tsx,js,jsx,mts,cts,mjs,cjs}",
    "utils/*.{ts,tsx,js,jsx,mts,cts,mjs,cjs}",
    "components/**/*.{vue,ts,tsx,js,jsx}",
    "components/global/**/*.{vue,ts,tsx,js,jsx}",
    "components/**/*.global.{vue,ts,tsx,js,jsx}",
];

const SRC_DIR_ALWAYS_USED: &[&str] = &["app.vue", "app.config.{ts,js}", "error.vue"];

const SRC_DIR_DEFAULT_EXPORT_PATTERNS: &[&str] = &[
    "pages/**/*.{vue,ts,tsx,js,jsx}",
    "layouts/**/*.{vue,ts,tsx,js,jsx}",
    "components/**/*.{vue,ts,tsx,js,jsx}",
    "modules/*.{ts,tsx,js,jsx,mts,cts,mjs,cjs}",
    "modules/*/index.{ts,tsx,js,jsx,mts,cts,mjs,cjs}",
    "modules/*/runtime/**/*.{vue,ts,tsx,js,jsx,mts,cts,mjs,cjs}",
    "plugins/*.{ts,tsx,js,jsx,mts,cts,mjs,cjs}",
    "plugins/*/index.{ts,tsx,js,jsx,mts,cts,mjs,cjs}",
    "middleware/*.{ts,tsx,js,jsx,mts,cts,mjs,cjs}",
    "middleware/*/index.{ts,tsx,js,jsx,mts,cts,mjs,cjs}",
];

pub(super) fn default_nuxt_src_dir(root: &Path) -> PathBuf {
    if root.join("app").is_dir() {
        PathBuf::from("app")
    } else {
        PathBuf::new()
    }
}

pub(super) fn extract_nuxt_src_dir(
    source: &str,
    config_path: &Path,
    root: &Path,
) -> Option<PathBuf> {
    let raw = config_parser::extract_config_path(source, config_path, &["srcDir"])?;
    normalize_nuxt_src_dir(&raw, config_path, root)
}

fn normalize_nuxt_src_dir(raw: &Path, config_path: &Path, root: &Path) -> Option<PathBuf> {
    let raw_string = config_parser::path_to_config_string(raw);
    let trimmed = raw_string.trim().trim_end_matches('/');
    if trimmed.is_empty() || trimmed == "." {
        return Some(PathBuf::new());
    }
    config_parser::normalize_config_path_buf(
        config_parser::path_from_config_string(trimmed),
        config_path,
        root,
    )
}

pub(super) fn add_src_dir_support(result: &mut PluginResult, src_dir: &Path) {
    let src_dir_string = config_parser::path_to_config_string(src_dir);
    result
        .path_aliases
        .push(("~/".to_string(), src_dir_string.clone()));
    result.path_aliases.push(("@/".to_string(), src_dir_string));

    if src_dir.as_os_str().is_empty() {
        return;
    }

    result.extend_entry_patterns(
        SRC_DIR_ENTRY_PATTERNS
            .iter()
            .map(|pattern| prefix_with_src_dir(src_dir, pattern)),
    );
    extend_prefixed_patterns(&mut result.always_used_files, src_dir, SRC_DIR_ALWAYS_USED);
    add_prefixed_default_used_exports(result, src_dir, SRC_DIR_DEFAULT_EXPORT_PATTERNS);
    add_prefixed_default_used_exports(result, src_dir, SRC_DIR_ALWAYS_USED);
}

/// Add the entry patterns of each custom directory in the `dir` object of a
/// Nuxt config. The default directories keep their patterns.
///
/// `@nuxt/kit` (`getLayerDirectories`) resolves `dir.pages`, `dir.layouts`,
/// `dir.middleware` and `dir.plugins` against `srcDir`. `@nuxt/schema`
/// resolves `dir.modules` against `rootDir`, which is the directory of the
/// config file. Only a value that the config parser reads as a static string
/// counts.
pub(super) fn add_nuxt_dir_entries(
    result: &mut PluginResult,
    source: &str,
    config_path: &Path,
    root: &Path,
    src_dir: &Path,
) {
    let root_dir = config_path
        .parent()
        .and_then(|dir| dir.strip_prefix(root).ok())
        .unwrap_or_else(|| Path::new(""));
    for (key, layout) in NUXT_DIR_KEYS {
        let Some(raw) = config_parser::extract_config_string(source, config_path, &["dir", key])
        else {
            continue;
        };
        let base = if matches!(layout, NuxtDirLayout::Modules) {
            root_dir
        } else {
            src_dir
        };
        let Some(dir) = normalize_nuxt_dir(&raw, config_path, root, src_dir, base) else {
            continue;
        };
        for suffix in layout.suffixes() {
            let pattern = format!("{dir}/{suffix}");
            add_default_used_export(result, &pattern);
            result.push_entry_pattern(pattern);
        }
    }
}

/// The file layout that Nuxt scans in a directory of the `dir` object.
#[derive(Clone, Copy)]
enum NuxtDirLayout {
    /// Every component file at any depth (`pages`, `layouts`).
    Components,
    /// A direct child file or the `index` file of a direct child directory
    /// (`middleware`, `plugins`).
    Scripts,
    /// The `Scripts` layout plus the `runtime/` files of each local module.
    Modules,
}

impl NuxtDirLayout {
    const fn suffixes(self) -> &'static [&'static str] {
        match self {
            Self::Components => &["**/*.{vue,ts,tsx,js,jsx}"],
            Self::Scripts => &[
                "*.{ts,tsx,js,jsx,mts,cts,mjs,cjs}",
                "*/index.{ts,tsx,js,jsx,mts,cts,mjs,cjs}",
            ],
            Self::Modules => &[
                "*.{ts,tsx,js,jsx,mts,cts,mjs,cjs}",
                "*/index.{ts,tsx,js,jsx,mts,cts,mjs,cjs}",
                "*/runtime/**/*.{vue,ts,tsx,js,jsx,mts,cts,mjs,cjs}",
            ],
        }
    }
}

/// The keys of the Nuxt `dir` object that hold convention entry points.
const NUXT_DIR_KEYS: &[(&str, NuxtDirLayout)] = &[
    ("pages", NuxtDirLayout::Components),
    ("layouts", NuxtDirLayout::Components),
    ("middleware", NuxtDirLayout::Scripts),
    ("plugins", NuxtDirLayout::Scripts),
    ("modules", NuxtDirLayout::Modules),
];

/// Resolve a `dir` value to a project-root-relative directory. A `~/`, `@/`,
/// `~~/` or `@@/` alias resolves as Nuxt resolves it. Any other relative value
/// resolves against `base`, as `path.resolve(base, value)` does.
fn normalize_nuxt_dir(
    raw: &str,
    config_path: &Path,
    root: &Path,
    src_dir: &Path,
    base: &Path,
) -> Option<String> {
    let raw = raw.trim().trim_end_matches('/');
    if raw.is_empty() {
        return None;
    }
    let dir = if ["~/", "@/", "~~/", "@@/"]
        .iter()
        .any(|alias| raw.starts_with(alias))
    {
        normalize_nuxt_path(raw, config_path, root, src_dir)?
    } else {
        let base_config = root.join(base).join("nuxt.config.ts");
        config_parser::normalize_filesystem_config_path(raw, &base_config, root)?
    };
    let dir = dir.trim_end_matches('/');
    (!dir.is_empty()).then(|| dir.to_string())
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::super::NuxtPlugin;
    use super::super::tests::{has_entry_pattern, has_used_export_rule};
    use crate::plugins::Plugin;

    const DIR_SCRIPT_EXTS: &str = "{ts,tsx,js,jsx,mts,cts,mjs,cjs}";

    #[test]
    fn resolve_config_dir_keys_resolve_against_src_dir_and_root_dir() {
        let source = r#"
            export default defineNuxtConfig({
                srcDir: "src/",
                dir: {
                    plugins: "custom-plugins",
                    middleware: "guards",
                    pages: "views",
                    layouts: "templates",
                    modules: "nuxt-modules"
                }
            });
        "#;
        let result = NuxtPlugin.resolve_config(
            Path::new("/project/nuxt.config.ts"),
            source,
            Path::new("/project"),
        );

        let script_dirs = ["src/custom-plugins", "src/guards", "nuxt-modules"];
        for dir in script_dirs {
            for pattern in [
                format!("{dir}/*.{DIR_SCRIPT_EXTS}"),
                format!("{dir}/*/index.{DIR_SCRIPT_EXTS}"),
            ] {
                assert!(has_entry_pattern(&result, &pattern), "missing {pattern}");
                assert!(
                    has_used_export_rule(&result, &pattern, &["default"]),
                    "{pattern} should keep the default export alive"
                );
            }
        }
        let runtime = "nuxt-modules/*/runtime/**/*.{vue,ts,tsx,js,jsx,mts,cts,mjs,cjs}";
        assert!(has_entry_pattern(&result, runtime), "missing {runtime}");
        for pattern in [
            "src/views/**/*.{vue,ts,tsx,js,jsx}",
            "src/templates/**/*.{vue,ts,tsx,js,jsx}",
        ] {
            assert!(has_entry_pattern(&result, pattern), "missing {pattern}");
            assert!(
                has_used_export_rule(&result, pattern, &["default"]),
                "{pattern} should keep the default export alive"
            );
        }
        assert!(
            !has_entry_pattern(&result, &format!("src/nuxt-modules/*.{DIR_SCRIPT_EXTS}")),
            "dir.modules resolves against rootDir, not srcDir"
        );
    }

    #[test]
    fn resolve_config_dir_keys_use_the_default_src_dir_and_aliases() {
        let temp = tempfile::tempdir().expect("temp dir should be created");
        std::fs::create_dir(temp.path().join("app")).expect("app dir should exist");
        let source = r#"
            export default defineNuxtConfig({
                dir: {
                    plugins: "~/client-plugins",
                    middleware: "~~/shared-guards/"
                }
            });
        "#;
        let config_path = temp.path().join("nuxt.config.ts");
        let result = NuxtPlugin.resolve_config(&config_path, source, temp.path());

        for pattern in [
            format!("app/client-plugins/*.{DIR_SCRIPT_EXTS}"),
            format!("shared-guards/*.{DIR_SCRIPT_EXTS}"),
        ] {
            assert!(has_entry_pattern(&result, &pattern), "missing {pattern}");
        }
    }

    #[test]
    fn resolve_config_dir_keys_ignore_non_literal_values() {
        let source = r#"
            export default defineNuxtConfig({
                dir: { plugins: pickDir("custom-plugins"), middleware: process.env.GUARDS }
            });
        "#;
        let result = NuxtPlugin.resolve_config(
            Path::new("/project/nuxt.config.ts"),
            source,
            Path::new("/project"),
        );
        let without_dir = NuxtPlugin.resolve_config(
            Path::new("/project/nuxt.config.ts"),
            "export default defineNuxtConfig({});",
            Path::new("/project"),
        );

        assert_eq!(
            result.entry_patterns, without_dir.entry_patterns,
            "a non-literal dir value adds no entry pattern"
        );
    }
}
