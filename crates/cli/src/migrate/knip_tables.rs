/// Knip rule names mapped to fallow rule names.
pub(super) const KNIP_RULE_MAP: &[(&str, &str)] = &[
    ("files", "unused-files"),
    ("dependencies", "unused-dependencies"),
    ("devDependencies", "unused-dev-dependencies"),
    ("exports", "unused-exports"),
    ("types", "unused-types"),
    ("enumMembers", "unused-enum-members"),
    ("classMembers", "unused-class-members"),
    ("unlisted", "unlisted-dependencies"),
    ("unresolved", "unresolved-imports"),
    ("duplicates", "duplicate-exports"),
    ("cycles", "circular-dependencies"),
];

/// Knip fields that cannot be mapped and generate warnings.
pub(super) const KNIP_UNMAPPABLE_FIELDS: &[(&str, &str, Option<&str>)] = &[
    ("project", "Fallow auto-discovers project files", None),
    (
        "paths",
        "Fallow reads path mappings from tsconfig.json automatically",
        None,
    ),
    (
        "ignoreFiles",
        "Fallow has no dedicated ignoreFiles field",
        Some(
            "use overrides[].files with rules.unused-files = \"off\"; this keeps matching files in the analysis graph",
        ),
    ),
    (
        "ignoreBinaries",
        "Binary filtering is not configurable in fallow",
        None,
    ),
    (
        "ignoreMembers",
        "Member-level ignoring is not configurable in fallow",
        Some("use inline suppression comments: // fallow-ignore-next-line"),
    ),
    (
        "ignoreUnresolved",
        "Unresolved import filtering is not configurable in fallow",
        Some("use inline suppression comments: // fallow-ignore-next-line unresolved-import"),
    ),
    (
        "ignoreWorkspaces",
        "Workspace filtering is not configurable per-workspace",
        Some("use --workspace flag to scope output to a single package"),
    ),
    (
        "ignoreIssues",
        "No global issue ignoring in fallow",
        Some("use inline suppression comments: // fallow-ignore-file [issue-type]"),
    ),
    (
        "includeEntryExports",
        "Entry export inclusion is not configurable in fallow",
        None,
    ),
    (
        "tags",
        "Tag-based filtering is not supported in fallow",
        None,
    ),
    (
        "compilers",
        "Custom compilers are not supported in fallow (uses Oxc parser)",
        None,
    ),
    ("treatConfigHintsAsErrors", "No equivalent in fallow", None),
    ("treatTagHintsAsErrors", "No equivalent in fallow", None),
];

/// Knip root keys that `migrate_knip` translates into fallow config, plus the
/// `$schema` key every JSON config may carry. A key here is neither a plugin
/// section nor an unmappable field, so the unknown-key ladder in
/// `warn_unhandled_root_keys` must stay quiet about it.
pub(super) const KNIP_MIGRATED_FIELDS: &[&str] = &[
    "$schema",
    "entry",
    "exclude",
    "ignore",
    "ignoreDependencies",
    "ignoreExportsUsedInFile",
    "include",
    "rules",
    "workspaces",
];

/// Knip issue type names that have no fallow equivalent.
pub(super) const KNIP_UNMAPPABLE_ISSUE_TYPES: &[&str] = &[
    "optionalPeerDependencies",
    "binaries",
    "nsExports",
    "nsTypes",
    "catalog",
    "catalogReferences",
    "namespaceMembers",
];

/// Knip plugin config keys that a built-in fallow plugin covers. Fallow
/// detects these frameworks and tools from the project itself, so the knip
/// section has no fallow counterpart and can be dropped.
///
/// Keys come from the `Plugins` map in knip and from the plugin sections of
/// knip's published JSON schema, which still accepts a few keys that the
/// plugin map has since renamed. A key belongs here only when a plugin in
/// `fallow_core::plugins` registers the same tool. Keys that no fallow plugin
/// covers belong in `KNIP_UNSUPPORTED_PLUGIN_KEYS` instead.
pub(super) const KNIP_PLUGIN_KEYS: &[&str] = &[
    "angular",
    "astro",
    "ava",
    "babel",
    "biome",
    "bun",
    "c8",
    "capacitor",
    "changesets",
    "commitizen",
    "commitlint",
    "convex",
    "cspell",
    "cucumber",
    "cypress",
    "danger",
    "dependency-cruiser",
    "docusaurus",
    "drizzle",
    "electron-vite",
    "eslint",
    "expo",
    "fumadocs",
    "gatsby",
    "graphql-codegen",
    "hardhat",
    "husky",
    "jest",
    "karma",
    "knex",
    "lefthook",
    "lint-staged",
    "lit",
    "markdownlint",
    "mocha",
    "msw",
    "nest",
    "next",
    "next-intl",
    "nitro",
    "nodemon",
    "nuxt",
    "nx",
    "nyc",
    "openapi-ts",
    "oxlint",
    "panda-css",
    "parcel",
    "playwright",
    "plop",
    "pm2",
    "pnpm",
    "postcss",
    "prettier",
    "prisma",
    "qwik",
    "react-native",
    "react-router",
    "relay",
    "remark",
    "remix",
    "rolldown",
    "rollup",
    "rsbuild",
    "rspack",
    "sanity",
    "semantic-release",
    "sentry",
    "simple-git-hooks",
    "size-limit",
    "storybook",
    "stryker",
    "stylelint",
    "sveltekit",
    "svgo",
    "svgr",
    "swc",
    "syncpack",
    "tailwind",
    "tanstack-router",
    "tsd",
    "tsdown",
    "tsup",
    "typedoc",
    "typescript",
    "unocss",
    "vercel",
    "vite",
    "vitepress",
    "vitest",
    "webdriver-io",
    "webpack",
    "wrangler",
    "wxt",
];

/// Knip plugin config keys that no built-in fallow plugin covers. The section
/// is still recognized so a migration reports it instead of dropping it in
/// silence, but fallow makes no claim to detect the tool.
pub(super) const KNIP_UNSUPPORTED_PLUGIN_KEYS: &[&str] = &[
    "astro-db",
    "astro-markdoc",
    "astro-og-canvas",
    "borp",
    "bumpp",
    "catalyst",
    "changelogen",
    "changelogithub",
    "create-typescript-app",
    "dotenv",
    "eleventy",
    "esbuild",
    "eve",
    "execa",
    "expressive-code",
    "fast",
    "github-action",
    "github-actions",
    "glob",
    "i18next-parser",
    "ladle",
    "laravel-vite-plugin",
    "linthtml",
    "lockfile-lint",
    "lost-pixel",
    "lunaria",
    "marko",
    "mdx",
    "mdxlint",
    "metro",
    "moonrepo",
    "nano-spawn",
    "nano-staged",
    "netlify",
    "next-mdx",
    "node",
    "node-modules-inspector",
    "node-test-runner",
    "npm-package-json-lint",
    "nuxtjs-i18n",
    "oclif",
    "openclaw",
    "orval",
    "oxfmt",
    "payload",
    "pino",
    "playwright-ct",
    "playwright-test",
    "pre-commit",
    "preconstruct",
    "quasar",
    "raycast",
    "react-cosmos",
    "react-email",
    "release-it",
    "rslib",
    "rstest",
    "serverless-framework",
    "sst",
    "starlight",
    "stencil",
    "svelte",
    "sveltejs-package",
    "tanstack-start",
    "taskfile",
    "tauri",
    "temporal",
    "travis",
    "ts-node",
    "tsx",
    "unbuild",
    "unplugin-auto-import",
    "unplugin-icons",
    "unplugin-vue-components",
    "unplugin-vue-i18n",
    "unplugin-vue-markdown",
    "unplugin-vue-router",
    "vercel-og",
    "vike",
    "vite-plugin-pages",
    "vite-plugin-pwa",
    "vite-plugin-vue-layouts-next",
    "vite-plus",
    "vite-pwa-assets-generator",
    "vue",
    "wireit",
    "xo",
    "yarn",
    "yorkie",
    "zx",
];

#[cfg(test)]
mod tests {
    use super::*;
    use rustc_hash::{FxHashMap, FxHashSet};

    /// Knip plugin keys whose spelling differs from the built-in fallow plugin
    /// that covers the same tool. Every other entry of `KNIP_PLUGIN_KEYS` has
    /// to match a registered plugin name exactly, so a sixth spelling
    /// difference fails `plugin_keys_resolve_to_builtin_plugins` until it is
    /// declared here.
    const KNIP_PLUGIN_KEY_ALIASES: &[(&str, &str)] = &[
        ("electron-vite", "electron"),
        ("nest", "nestjs"),
        ("next", "nextjs"),
        ("panda-css", "pandacss"),
        ("webdriver-io", "webdriverio"),
    ];

    fn builtin_plugins() -> FxHashSet<&'static str> {
        fallow_engine::plugins::registry::builtin_plugin_names()
            .into_iter()
            .collect()
    }

    #[test]
    fn rule_map_has_no_empty_keys_or_values() {
        for (knip, fallow) in KNIP_RULE_MAP {
            assert!(!knip.is_empty(), "KNIP_RULE_MAP contains an empty knip key");
            assert!(
                !fallow.is_empty(),
                "KNIP_RULE_MAP contains an empty fallow value for key `{knip}`"
            );
        }
    }

    #[test]
    fn rule_map_has_no_duplicate_knip_keys() {
        let mut seen = FxHashSet::default();
        for (knip, _) in KNIP_RULE_MAP {
            assert!(
                seen.insert(*knip),
                "KNIP_RULE_MAP has duplicate knip key `{knip}`"
            );
        }
    }

    #[test]
    fn rule_map_has_no_duplicate_fallow_values() {
        let mut seen = FxHashSet::default();
        for (_, fallow) in KNIP_RULE_MAP {
            assert!(
                seen.insert(*fallow),
                "KNIP_RULE_MAP has duplicate fallow value `{fallow}`"
            );
        }
    }

    #[test]
    fn rule_map_is_non_empty() {
        assert!(
            !KNIP_RULE_MAP.is_empty(),
            "KNIP_RULE_MAP should not be empty"
        );
    }

    #[test]
    fn unmappable_fields_is_non_empty() {
        assert!(
            !KNIP_UNMAPPABLE_FIELDS.is_empty(),
            "KNIP_UNMAPPABLE_FIELDS should not be empty"
        );
    }

    #[test]
    fn unmappable_fields_have_non_empty_names_and_messages() {
        for (field, message, _) in KNIP_UNMAPPABLE_FIELDS {
            assert!(
                !field.is_empty(),
                "KNIP_UNMAPPABLE_FIELDS contains an empty field name"
            );
            assert!(
                !message.is_empty(),
                "KNIP_UNMAPPABLE_FIELDS contains an empty message for `{field}`"
            );
        }
    }

    #[test]
    fn unmappable_fields_do_not_overlap_with_rule_map_keys() {
        let rule_keys: FxHashSet<&str> = KNIP_RULE_MAP.iter().map(|(k, _)| *k).collect();
        for (field, _, _) in KNIP_UNMAPPABLE_FIELDS {
            assert!(
                !rule_keys.contains(field),
                "KNIP_UNMAPPABLE_FIELDS entry `{field}` overlaps with KNIP_RULE_MAP"
            );
        }
    }

    #[test]
    fn unmappable_issue_types_is_non_empty() {
        assert!(
            !KNIP_UNMAPPABLE_ISSUE_TYPES.is_empty(),
            "KNIP_UNMAPPABLE_ISSUE_TYPES should not be empty"
        );
    }

    #[test]
    fn unmappable_issue_types_do_not_overlap_with_rule_map_keys() {
        let rule_keys: FxHashSet<&str> = KNIP_RULE_MAP.iter().map(|(k, _)| *k).collect();
        for issue_type in KNIP_UNMAPPABLE_ISSUE_TYPES {
            assert!(
                !rule_keys.contains(issue_type),
                "KNIP_UNMAPPABLE_ISSUE_TYPES entry `{issue_type}` overlaps with KNIP_RULE_MAP"
            );
        }
    }

    #[test]
    fn plugin_keys_is_non_empty() {
        assert!(
            !KNIP_PLUGIN_KEYS.is_empty(),
            "KNIP_PLUGIN_KEYS should not be empty"
        );
    }

    #[test]
    fn plugin_keys_contains_known_plugins() {
        let expected = ["eslint", "jest", "vitest", "next", "webpack", "storybook"];
        for name in expected {
            assert!(
                KNIP_PLUGIN_KEYS.contains(&name),
                "KNIP_PLUGIN_KEYS should contain `{name}`"
            );
        }
    }

    #[test]
    fn plugin_keys_are_sorted() {
        for window in KNIP_PLUGIN_KEYS.windows(2) {
            assert!(
                window[0] < window[1],
                "KNIP_PLUGIN_KEYS is not sorted: `{}` should come after `{}`",
                window[1],
                window[0]
            );
        }
    }

    #[test]
    fn plugin_keys_have_no_duplicates() {
        let mut seen = FxHashSet::default();
        for key in KNIP_PLUGIN_KEYS {
            assert!(
                seen.insert(*key),
                "KNIP_PLUGIN_KEYS has duplicate entry `{key}`"
            );
        }
    }

    #[test]
    fn plugin_keys_do_not_overlap_with_unmappable_fields() {
        let unmappable: FxHashSet<&str> =
            KNIP_UNMAPPABLE_FIELDS.iter().map(|(f, _, _)| *f).collect();
        for key in KNIP_PLUGIN_KEYS {
            assert!(
                !unmappable.contains(key),
                "KNIP_PLUGIN_KEYS entry `{key}` overlaps with KNIP_UNMAPPABLE_FIELDS"
            );
        }
    }

    /// The unknown-root-key ladder decides by table membership, so a key that
    /// sits in two tables would either warn twice or warn wrongly.
    #[test]
    fn migrated_fields_do_not_overlap_with_the_other_tables() {
        let unmappable: FxHashSet<&str> =
            KNIP_UNMAPPABLE_FIELDS.iter().map(|(f, _, _)| *f).collect();
        let covered: FxHashSet<&str> = KNIP_PLUGIN_KEYS.iter().copied().collect();
        let unsupported: FxHashSet<&str> = KNIP_UNSUPPORTED_PLUGIN_KEYS.iter().copied().collect();
        for field in KNIP_MIGRATED_FIELDS {
            assert!(
                !unmappable.contains(field),
                "KNIP_MIGRATED_FIELDS entry `{field}` overlaps with KNIP_UNMAPPABLE_FIELDS"
            );
            assert!(
                !covered.contains(field),
                "KNIP_MIGRATED_FIELDS entry `{field}` overlaps with KNIP_PLUGIN_KEYS"
            );
            assert!(
                !unsupported.contains(field),
                "KNIP_MIGRATED_FIELDS entry `{field}` overlaps with KNIP_UNSUPPORTED_PLUGIN_KEYS"
            );
        }
    }

    #[test]
    fn unsupported_plugin_keys_is_non_empty() {
        assert!(
            !KNIP_UNSUPPORTED_PLUGIN_KEYS.is_empty(),
            "KNIP_UNSUPPORTED_PLUGIN_KEYS should not be empty"
        );
    }

    #[test]
    fn unsupported_plugin_keys_are_sorted() {
        for window in KNIP_UNSUPPORTED_PLUGIN_KEYS.windows(2) {
            assert!(
                window[0] < window[1],
                "KNIP_UNSUPPORTED_PLUGIN_KEYS is not sorted: `{}` should come after `{}`",
                window[1],
                window[0]
            );
        }
    }

    #[test]
    fn unsupported_plugin_keys_have_no_duplicates() {
        let mut seen = FxHashSet::default();
        for key in KNIP_UNSUPPORTED_PLUGIN_KEYS {
            assert!(
                seen.insert(*key),
                "KNIP_UNSUPPORTED_PLUGIN_KEYS has duplicate entry `{key}`"
            );
        }
    }

    #[test]
    fn unsupported_plugin_keys_do_not_overlap_with_plugin_keys() {
        let covered: FxHashSet<&str> = KNIP_PLUGIN_KEYS.iter().copied().collect();
        for key in KNIP_UNSUPPORTED_PLUGIN_KEYS {
            assert!(
                !covered.contains(key),
                "`{key}` is in both KNIP_PLUGIN_KEYS and KNIP_UNSUPPORTED_PLUGIN_KEYS"
            );
        }
    }

    #[test]
    fn unsupported_plugin_keys_do_not_overlap_with_unmappable_fields() {
        let unmappable: FxHashSet<&str> =
            KNIP_UNMAPPABLE_FIELDS.iter().map(|(f, _, _)| *f).collect();
        for key in KNIP_UNSUPPORTED_PLUGIN_KEYS {
            assert!(
                !unmappable.contains(key),
                "KNIP_UNSUPPORTED_PLUGIN_KEYS entry `{key}` overlaps with KNIP_UNMAPPABLE_FIELDS"
            );
        }
    }

    /// Fallow has no Marko plugin, so the knip `marko` section must land in the
    /// unsupported table rather than claim auto-detection.
    #[test]
    fn marko_is_recognized_as_unsupported() {
        assert!(
            KNIP_UNSUPPORTED_PLUGIN_KEYS.contains(&"marko"),
            "KNIP_UNSUPPORTED_PLUGIN_KEYS should contain `marko`"
        );
        assert!(
            !KNIP_PLUGIN_KEYS.contains(&"marko"),
            "KNIP_PLUGIN_KEYS should not claim to cover `marko`"
        );
    }

    /// The claim `KNIP_PLUGIN_KEYS` makes is that fallow auto-detects the tool,
    /// so every key has to resolve to a plugin the built-in registry actually
    /// registers. Renaming or removing a plugin without moving its knip key
    /// fails here instead of shipping a migration that promises detection
    /// fallow no longer performs.
    #[test]
    fn plugin_keys_resolve_to_builtin_plugins() {
        let builtin = builtin_plugins();
        let aliases: FxHashMap<&str, &str> = KNIP_PLUGIN_KEY_ALIASES.iter().copied().collect();
        for key in KNIP_PLUGIN_KEYS {
            let plugin = aliases.get(key).copied().unwrap_or(key);
            assert!(
                builtin.contains(plugin),
                "KNIP_PLUGIN_KEYS entry `{key}` resolves to plugin `{plugin}`, which the built-in registry does not register; move the key to KNIP_UNSUPPORTED_PLUGIN_KEYS or declare its alias in KNIP_PLUGIN_KEY_ALIASES"
            );
        }
    }

    /// The other half of the same invariant: a key parked as unsupported must
    /// not name a plugin fallow does register, or the migration understates
    /// what fallow covers.
    #[test]
    fn unsupported_plugin_keys_name_no_builtin_plugin() {
        let builtin = builtin_plugins();
        for key in KNIP_UNSUPPORTED_PLUGIN_KEYS {
            assert!(
                !builtin.contains(key),
                "KNIP_UNSUPPORTED_PLUGIN_KEYS entry `{key}` names a registered built-in plugin; move it to KNIP_PLUGIN_KEYS"
            );
        }
    }

    #[test]
    fn plugin_key_aliases_are_used_and_needed() {
        let builtin = builtin_plugins();
        for (key, plugin) in KNIP_PLUGIN_KEY_ALIASES {
            assert!(
                KNIP_PLUGIN_KEYS.contains(key),
                "KNIP_PLUGIN_KEY_ALIASES declares `{key}`, which is not a knip plugin key"
            );
            assert!(
                !builtin.contains(key),
                "KNIP_PLUGIN_KEY_ALIASES entry `{key}` is itself a built-in plugin name; the alias to `{plugin}` is not needed"
            );
        }
    }

    #[test]
    fn plugin_keys_cover_recently_added_fallow_plugins() {
        let expected = [
            "bun",
            "electron-vite",
            "oxlint",
            "panda-css",
            "pnpm",
            "sveltekit",
            "tanstack-router",
            "webdriver-io",
        ];
        for name in expected {
            assert!(
                KNIP_PLUGIN_KEYS.contains(&name),
                "KNIP_PLUGIN_KEYS should contain `{name}`"
            );
        }
    }
}
