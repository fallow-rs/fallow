#![no_main]
use std::path::Path;

use fallow_core::discover::FileId;
use libfuzzer_sys::fuzz_target;

/// File names that select each parser behind `parse_from_content`. The
/// Angular name makes the visitor scan inline `@Component` templates.
const PATHS: &[&str] = &[
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

fuzz_target!(|data: &[u8]| {
    let Some((&selector, rest)) = data.split_first() else {
        return;
    };
    let Ok(source) = std::str::from_utf8(rest) else {
        return;
    };
    let path = Path::new(PATHS[usize::from(selector) % PATHS.len()]);
    let _ = fallow_core::extract::parse_from_content(FileId(0), path, source);
});
