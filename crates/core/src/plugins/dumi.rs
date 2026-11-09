//! dumi documentation framework plugin.
//!
//! dumi renders the demo file that each `<code src="...">` tag of a Markdown
//! page names. A relative `src` is read from the directory of the Markdown
//! file, and a `src` that starts with `/` from the project root. The local
//! theme lives in the hidden `.dumi` directory: the theme loader reads
//! `builtins`, `slots`, the three layouts and `plugin` from `.dumi/theme`, and
//! the convention router reads `.dumi/pages`. dumi reads its config from
//! `.dumirc.ts` or `.dumirc.js`.

use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;

use super::{Plugin, PluginResult, config_parser};

const ENABLERS: &[&str] = &["dumi"];

/// dumi's `DEFAULT_CONFIG_FILES`.
const CONFIG_PATTERNS: &[&str] = &[".dumirc.{ts,js}"];

const DISCOVERY_HIDDEN_DIRS: &[&str] = &[".dumi"];

/// The files that dumi and its umi runtime load from `.dumi`.
const ENTRY_PATTERNS: &[&str] = &[
    ".dumi/theme/{plugin,plugin/index}.{js,ts}",
    ".dumi/theme/{builtins,slots}/*.{js,jsx,ts,tsx}",
    ".dumi/theme/{builtins,slots}/*/index.{js,jsx,ts,tsx}",
    ".dumi/theme/layouts/{GlobalLayout,DocLayout,DemoLayout}.{js,jsx,ts,tsx}",
    ".dumi/theme/layouts/{GlobalLayout,DocLayout,DemoLayout}/index.{js,jsx,ts,tsx}",
    ".dumi/pages/**/*.{js,jsx,ts,tsx}",
    ".dumi/{global,app,loading}.{js,jsx,ts,tsx}",
];

const ALWAYS_USED: &[&str] = &[
    ".dumirc.{ts,js}",
    ".dumi/{global,overrides}.{css,less,scss,sass}",
];

/// The extensions that a `src` without an extension resolves to.
const DEMO_EXTENSIONS: &str = "{js,jsx,ts,tsx}";

/// Markdown files larger than this are not read for demo tags.
const MAX_MARKDOWN_BYTES: u64 = 2 * 1024 * 1024;

const DEMO_EXPORTS: &[&str] = &["default"];

#[expect(
    clippy::expect_used,
    reason = "the static demo tag pattern is hard-coded and covered by plugin tests"
)]
static CODE_SRC_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"<code\b[^>]*?\ssrc\s*=\s*(?:"([^"]+)"|'([^']+)')"#).expect("valid regex")
});

pub struct DumiPlugin;

impl Plugin for DumiPlugin {
    fn name(&self) -> &'static str {
        "dumi"
    }

    fn enablers(&self) -> &'static [&'static str] {
        ENABLERS
    }

    fn entry_patterns(&self) -> &'static [&'static str] {
        ENTRY_PATTERNS
    }

    fn config_patterns(&self) -> &'static [&'static str] {
        CONFIG_PATTERNS
    }

    fn always_used(&self) -> &'static [&'static str] {
        ALWAYS_USED
    }

    fn discovery_hidden_dirs(&self) -> &'static [&'static str] {
        DISCOVERY_HIDDEN_DIRS
    }

    fn resolve_package_json(&self, _pkg: &fallow_config::PackageJson, root: &Path) -> PluginResult {
        let mut result = PluginResult::default();
        for pattern in demo_patterns(root) {
            result.push_entry_export_credit(pattern.clone(), DEMO_EXPORTS.iter().copied());
            result.push_entry_pattern(pattern);
        }
        result
    }

    fn resolve_config(&self, config_path: &Path, source: &str, root: &Path) -> PluginResult {
        let mut result = PluginResult::default();
        for import in config_parser::extract_imports(source, config_path) {
            if let Some(package) = config_parser::config_string_package_name(&import) {
                result.referenced_dependencies.push(package);
            }
        }
        for (find, replacement) in
            config_parser::extract_config_path_aliases(source, config_path, &["alias"])
        {
            if let Some(normalized) =
                config_parser::normalize_config_path(&replacement, config_path, root)
            {
                result.path_aliases.push((find, normalized));
            }
        }
        for key in ["plugins", "presets"] {
            for value in config_parser::extract_config_shallow_strings(source, config_path, key) {
                if let Some(package) = config_parser::config_string_package_name(&value) {
                    result.referenced_dependencies.push(package);
                } else if let Some(path) =
                    config_parser::normalize_config_path(value.trim(), config_path, root)
                {
                    result.push_entry_pattern(globset::escape(&path));
                }
            }
        }
        result
    }
}

/// The entry patterns of the demo files that the Markdown files below `root`
/// name in `<code src>` tags, sorted and without duplicates.
fn demo_patterns(root: &Path) -> Vec<String> {
    let mut patterns = Vec::new();
    for markdown in markdown_files(root) {
        let Ok(text) = std::fs::read_to_string(&markdown) else {
            continue;
        };
        let Some(dir) = markdown.parent() else {
            continue;
        };
        for captures in CODE_SRC_RE.captures_iter(&text) {
            let Some(src) = captures.get(1).or_else(|| captures.get(2)) else {
                continue;
            };
            if let Some(relative) = demo_path(src.as_str(), dir, root) {
                push_demo_patterns(&mut patterns, &relative);
            }
        }
    }
    patterns.sort_unstable();
    patterns.dedup();
    patterns
}

/// The Markdown files below `root`. The walk respects `.gitignore` and skips
/// `node_modules` and hidden directories.
fn markdown_files(root: &Path) -> Vec<PathBuf> {
    let walker = fallow_config::source_walk_builder(root)
        .filter_entry(|entry| {
            let is_dir = entry.file_type().is_some_and(|kind| kind.is_dir());
            let name = entry.file_name().to_string_lossy();
            !(is_dir && entry.depth() > 0 && (name == "node_modules" || name.starts_with('.')))
        })
        .build();
    walker
        .flatten()
        .filter(|entry| entry.file_type().is_some_and(|kind| !kind.is_dir()))
        .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "md"))
        .filter(|entry| {
            entry
                .metadata()
                .is_ok_and(|meta| meta.len() <= MAX_MARKDOWN_BYTES)
        })
        .map(ignore::DirEntry::into_path)
        .collect()
}

/// The root-relative forward-slash path that a `src` value names, or `None`
/// when the value is a URL or leaves `root`.
fn demo_path(src: &str, dir: &Path, root: &Path) -> Option<String> {
    let src = src.trim();
    if src.is_empty() || src.contains("://") {
        return None;
    }
    let absolute = match src.strip_prefix('/') {
        Some(rest) => root.join(config_parser::path_from_config_string(rest)),
        None => dir.join(config_parser::path_from_config_string(src)),
    };
    let normalized = config_parser::lexical_normalize(&absolute);
    let relative = normalized.strip_prefix(root).ok()?;
    let relative = config_parser::path_to_config_string(relative);
    (!relative.is_empty()).then_some(relative)
}

/// Push the pattern of an exact file, or the patterns of the files that a
/// `src` without a source extension resolves to.
fn push_demo_patterns(patterns: &mut Vec<String>, relative: &str) {
    let escaped = globset::escape(relative);
    let has_extension = Path::new(relative)
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| crate::discover::SOURCE_EXTENSIONS.contains(&ext));
    if has_extension {
        patterns.push(escaped);
    } else {
        patterns.push(format!("{escaped}.{DEMO_EXTENSIONS}"));
        patterns.push(format!("{escaped}/index.{DEMO_EXTENSIONS}"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn srcs(text: &str) -> Vec<String> {
        CODE_SRC_RE
            .captures_iter(text)
            .filter_map(|c| c.get(1).or_else(|| c.get(2)))
            .map(|m| m.as_str().to_string())
            .collect()
    }

    #[test]
    fn code_tags_with_attributes_in_any_order_are_read() {
        let text = r#"
<code src="./demo/a.tsx">A</code>
<code debug src='./demo/b.tsx'></code>
<code version="6.0.0" src="./demo/c.tsx" iframe="360"></code>
<code>inline</code>
<codes src="./demo/not-a-tag.tsx"></codes>
"#;
        assert_eq!(
            srcs(text),
            vec!["./demo/a.tsx", "./demo/b.tsx", "./demo/c.tsx"]
        );
    }

    #[test]
    fn demo_paths_resolve_from_the_markdown_dir_or_the_root() {
        let root = Path::new("/project");
        let dir = Path::new("/project/src/button");
        assert_eq!(
            demo_path("./demo/a.tsx", dir, root).as_deref(),
            Some("src/button/demo/a.tsx")
        );
        assert_eq!(
            demo_path("/docs/b.tsx", dir, root).as_deref(),
            Some("docs/b.tsx")
        );
        assert_eq!(demo_path("../../../x.tsx", dir, root), None);
        assert_eq!(demo_path("https://example.com/x.tsx", dir, root), None);
    }

    #[test]
    fn a_src_without_extension_matches_files_and_index_files() {
        let mut patterns = Vec::new();
        push_demo_patterns(&mut patterns, "src/demo/[id]");
        assert_eq!(
            patterns,
            vec![
                "src/demo/[[]id[]].{js,jsx,ts,tsx}",
                "src/demo/[[]id[]]/index.{js,jsx,ts,tsx}",
            ]
        );
    }

    #[test]
    fn config_plugins_credit_packages_and_local_files() {
        let source = r#"
            import path from "node:path";
            import { defineConfig } from "dumi";
            export default defineConfig({
                alias: { "antd/es": path.join(__dirname, "components") },
                plugins: ["dumi-plugin-color-chunk", "./plugins/local.ts"],
                presets: ["@org/dumi-preset/sub"],
            });
        "#;
        let result = DumiPlugin.resolve_config(
            Path::new("/project/.dumirc.ts"),
            source,
            Path::new("/project"),
        );
        assert!(result.referenced_dependencies.contains(&"dumi".to_string()));
        assert!(
            result
                .referenced_dependencies
                .contains(&"dumi-plugin-color-chunk".to_string())
        );
        assert!(
            result
                .referenced_dependencies
                .contains(&"@org/dumi-preset".to_string())
        );
        assert!(
            result
                .entry_patterns
                .iter()
                .any(|rule| rule.pattern == "plugins/local.ts")
        );
        assert_eq!(
            result.path_aliases,
            vec![("antd/es".to_string(), "components".to_string())]
        );
    }
}
