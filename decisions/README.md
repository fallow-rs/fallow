# Architecture Decision Records

This directory records significant architectural decisions for fallow. Each ADR captures the context, options considered, and consequences of a decision, making them discoverable for contributors and AI agents without digging through git history.

## Format

Each ADR follows a consistent structure: status, context explaining the problem, the decision made, alternatives considered, and consequences. See `_template.md`.

## Index

| # | Decision | Status |
|---|----------|--------|
| 001 | [No TypeScript compiler](001-no-typescript-compiler.md) | Accepted |
| 002 | [Flat edge storage](002-flat-edge-storage.md) | Accepted |
| 003 | [FxHashMap over std HashMap](003-fxhashmap-over-std.md) | Accepted |
| 004 | [Path-sorted FileIds](004-path-sorted-file-ids.md) | Accepted |
| 005 | [Re-export chain resolution](005-re-export-chain-resolution.md) | Accepted |
| 006 | [Hidden directory allowlist](006-hidden-directory-allowlist.md) | Accepted |
| 007 | [Boundary zone subtree-relative root](007-boundary-zone-root.md) | Accepted |
| 008 | [fallow-core is internal; embedders use fallow-cli::programmatic](008-fallow-core-internal-policy.md) | Accepted |
| 009 | [Cache config-hash ingredients](009-cache-config-hash-ingredients.md) | Accepted |
| 010 | [fallow-v8-coverage and oxc_coverage_v8 stay separate](010-v8-coverage-vs-oxc-coverage-boundary.md) | Accepted |

## Creating a new ADR

1. Copy `_template.md` to `NNN-short-title.md`
2. Fill in all sections
3. Add to the index table above
4. Reference from CLAUDE.md if the decision affects contributor workflow
