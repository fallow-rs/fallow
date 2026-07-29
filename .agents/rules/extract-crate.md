---
paths:
  - "crates/extract/**"
---

# fallow-extract crate

Key modules:
- `lib.rs` — Public API: `parse_all_files()` (parallel rayon dispatch, cache-aware), returns `ParseResult` with modules + cache hit/miss statistics
- `visitor.rs` — Oxc AST visitor extracting imports, exports, re-exports, members, whole-object uses, dynamic import patterns, namespace destructuring (`const { a, b } = ns` → member accesses)
- `sfc.rs` — Vue/Svelte SFC script extraction (HTML comment filtering, `<script src="...">` support, `lang="ts"`/`lang="tsx"` detection, handles `>` in quoted attributes). Orchestrates template usage tracking via `sfc_template/`.
- `sfc_template/` — Template-visible import usage tracking for Vue and Svelte. Framework-specific scanners (`vue.rs`, `svelte.rs`) parse template markup to detect import references (`{formatDate(x)}`, `utils.formatDate()`). Shared scanner (`scanners.rs`) and helpers (`shared.rs`) provide HTML tag/curly-section parsing and expression analysis.
- `template_usage.rs` — `TemplateUsage` struct and `analyze_template_snippet()` for parsing template expressions via Oxc to extract used bindings and member accesses.
- `astro.rs` — Astro frontmatter extraction between `---` delimiters
- `mdx.rs` — MDX import/export extraction with multi-line brace tracking
- `css.rs` — CSS Module class name extraction (`.module.css`/`.module.scss` → named exports)
- `html.rs` — HTML asset reference extraction (`<script src>`, `<link rel="stylesheet" href>`, `<link rel="modulepreload" href>` → `SideEffect` imports). Regex-based, comment-stripped.
- `parse.rs` — File type dispatcher. Runs `oxc_semantic` after parsing to detect unused import bindings (imports where the binding is never read in the file).
- `cache.rs` — Incremental bitcode cache with xxh3 hashing. Unchanged files skip parsing and load from cache. Pruned of stale entries on each run.
- `complexity.rs` — Per-function cyclomatic/cognitive complexity via single-pass `ComplexityVisitor`
- `tests/` — Integration tests split by parser type: `js_ts.rs`, `sfc.rs`, `astro.rs`, `mdx.rs`, `css.rs`
