//! `VitePress` plugin.
//!
//! Detects `VitePress` projects and marks config/theme files as always used.

use super::Plugin;

const ENABLERS: &[&str] = &["vitepress"];

const ENTRY_PATTERNS: &[&str] = &[
    ".vitepress/theme/index.{ts,js,mts,mjs}",
    ".vitepress/theme/**/*.{vue,ts,js,mts,mjs}",
    "docs/.vitepress/theme/index.{ts,js,mts,mjs}",
    "docs/.vitepress/theme/**/*.{vue,ts,js,mts,mjs}",
];

const ALWAYS_USED: &[&str] = &[
    ".vitepress/config.{ts,js,mts,mjs}",
    "docs/.vitepress/config.{ts,js,mts,mjs}",
];

const CONFIG_FILE_EXPORTS: &[(&str, &[&str])] = &[
    (".vitepress/config.{ts,js,mts,mjs}", &["default"]),
    ("docs/.vitepress/config.{ts,js,mts,mjs}", &["default"]),
];

const TOOLING_DEPENDENCIES: &[&str] = &["vitepress"];

const THEME_ENTRY_EXPORTS: &[&str] = &["default"];

define_plugin! {
    struct VitePressPlugin => "vitepress",
    enablers: ENABLERS,
    entry_patterns: ENTRY_PATTERNS,
    config_file_exports: CONFIG_FILE_EXPORTS,
    always_used: ALWAYS_USED,
    tooling_dependencies: TOOLING_DEPENDENCIES,
    used_exports: [
        (".vitepress/theme/index.{ts,js,mts,mjs}", THEME_ENTRY_EXPORTS),
        ("docs/.vitepress/theme/index.{ts,js,mts,mjs}", THEME_ENTRY_EXPORTS),
    ],
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn supports_docs_scaffold_layout_and_theme_entry_exports() {
        let plugin = VitePressPlugin;

        assert!(
            plugin
                .entry_patterns()
                .contains(&"docs/.vitepress/theme/index.{ts,js,mts,mjs}")
        );
        assert!(
            plugin
                .always_used()
                .contains(&"docs/.vitepress/config.{ts,js,mts,mjs}")
        );
        assert!(plugin.used_exports().iter().any(|(pattern, names)| {
            pattern == &"docs/.vitepress/theme/index.{ts,js,mts,mjs}" && names == &["default"]
        }));
    }
}
