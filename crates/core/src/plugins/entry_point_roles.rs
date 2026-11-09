//! The coverage reachability role of each built-in plugin's entry points.

use fallow_config::EntryPointRole;

pub(super) const TEST_ENTRY_POINT_PLUGINS: &[&str] = &[
    "ava",
    "bun",
    "deno",
    "cucumber",
    "cypress",
    "jasmine",
    "jest",
    "k6",
    "mocha",
    "playwright",
    "rstest",
    "tap",
    "tsd",
    "vitest",
    "webdriverio",
];

pub(super) const RUNTIME_ENTRY_POINT_PLUGINS: &[&str] = &[
    "adonis",
    "angular",
    "astro",
    "browser-extension",
    "convex",
    "docusaurus",
    "egg",
    "electron",
    "ember",
    "eve",
    "expo",
    "expo-router",
    "flue",
    "gatsby",
    "hardhat",
    "kibana",
    "module-federation",
    "nestjs",
    "next-intl",
    "nextjs",
    "nitro",
    "nuxt",
    "obsidian",
    "parcel",
    "qwik",
    "react-native",
    "react-router",
    "redwoodsdk",
    "remix",
    "rolldown",
    "rollup",
    "rsbuild",
    "rspack",
    "sanity",
    "supabase",
    "sveltekit",
    "tanstack-router",
    "tsdown",
    "tsup",
    "vercel",
    "vite",
    "vite-plus",
    "vitepress",
    "waku",
    "webpack",
    "wrangler",
    "wxt",
];

#[cfg(test)]
pub(super) const SUPPORT_ENTRY_POINT_PLUGINS: &[&str] = &[
    "alchemy",
    "content-collections",
    "contentlayer",
    "danger",
    "drizzle",
    "fumadocs",
    "i18next",
    "knex",
    "kysely",
    "mintlify",
    "msw",
    "opencode",
    "prisma",
    "storybook",
    "stryker",
    "typeorm",
    "velite",
];

pub(super) fn builtin_entry_point_role(name: &str) -> EntryPointRole {
    if TEST_ENTRY_POINT_PLUGINS.contains(&name) {
        EntryPointRole::Test
    } else if RUNTIME_ENTRY_POINT_PLUGINS.contains(&name) {
        EntryPointRole::Runtime
    } else {
        EntryPointRole::Support
    }
}
