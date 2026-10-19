//! `scripts/fuzz-seed-extract.mjs` reads this list to map fixture files to a
//! selector byte, so keep one name per line.

/// File names that select each parser behind `parse_from_content`. The
/// Angular name makes the visitor scan inline `@Component` templates.
pub const PATHS: &[&str] = &[
    "src/index.js",
    "src/index.jsx",
    "src/index.mjs",
    "src/index.cjs",
    "src/index.ts",
    "src/index.tsx",
    "src/index.mts",
    "src/index.d.ts",
    "src/app.component.ts",
    "src/App.gts",
    "src/App.gjs",
    "src/App.vue",
    "src/App.svelte",
    "src/pages/index.astro",
    "docs/page.mdx",
    "src/styles.module.css",
    "src/styles.scss",
    "index.html",
    "schema.graphql",
];
