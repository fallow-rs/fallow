# Extract Agent Guide

Use this file when editing `crates/extract/**`.

## Ownership

- `lib.rs`: parse entry points, cache-aware dispatch, source normalization.
- `visitor/`: import, export, re-export, member, and dynamic import extraction.
- `parse.rs`: Oxc parser setup and semantic pass handling.
- `cache/`: incremental parse-cache types, conversion, storage, and eviction.
- `complexity.rs`: cyclomatic and cognitive complexity extraction.
- `sfc.rs`, `astro.rs`, `mdx.rs`, `css.rs`, `graphql.rs`: embedded language extraction.
- `sfc_template/`: Vue and Svelte template-visible usage tracking.

## Rules

- Keep extraction syntactic. Do not introduce TypeScript compiler dependence.
- Preserve byte and line mapping when transforming embedded source.
- Cache keys must change when parsed semantics change.
- Avoid panics on malformed user input. Return partial extraction plus diagnostics where possible.
- Keep fixtures minimal, especially for SFC and embedded-language cases.
- Template usage fixes should cover both value references and type-only references when the syntax supports them.

## Validation

- Add focused parser or visitor tests for the exact syntax boundary.
- Add integration coverage when extracted symbols affect reachability or issue detection.
- Include malformed-input coverage when the bug came from invalid or incomplete code.
