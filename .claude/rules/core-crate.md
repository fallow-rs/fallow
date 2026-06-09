---
paths:
  - "crates/core/**"
---

# fallow-core crate

Re-exports fallow-extract and fallow-graph for backwards compatibility.

Key modules:
- `discover.rs` — File walking + entry point detection (workspace-aware). Hidden directory allowlist (`.storybook`, `.vitepress`, `.well-known`, `.changeset`, `.github`). Only root-level `build/` is ignored (not nested).
- `discover/walk.rs` per-file size skip + extra default ignores (issue #1086): default ignores also drop `*.min.cjs` and `*.bundle.js` (alongside the existing `*.min.js`/`*.min.mjs`). After the walk, `partition_by_size` drops files strictly larger than `ResolvedConfig.max_file_size_bytes` (default 5 MB via `resolve_max_file_size_bytes`; the CLI overrides post-resolve from `--max-file-size`/`FALLOW_MAX_FILE_SIZE`, `0` = `None` = unlimited) so a single multi-MB generated/vendored/bundled file cannot OOM the all-in-memory parse. Declaration files (`.d.ts`/`.d.mts`/`.d.cts`) are exempt (reachability roots for global types). Skipped files are appended to the `WorkspaceDiagnostic` registry as `SkippedLargeFile { size_bytes }` (surfaces in `workspace_diagnostics[]` JSON) plus an aggregated `tracing::warn!`; `note_largest_files` (via the pure `build_largest_files_note`) emits a pre-parse largest-files warn when the set exceeds `LARGE_SET_THRESHOLD` (20k) or a file exceeds `LARGE_FILE_NOTE_BYTES` (4 MB, just under the 5 MB skip); the listed files are filtered to those `>= NOTE_FILE_FLOOR_BYTES` (256 KB) with a count-only fallback, so a parse-stage hang is diagnosable without `0.0 MB` chaff.
- `analyze/mod.rs` — Orchestration: runs all detectors, collects `AnalysisResults`
- `analyze/predicates.rs` — Lookup tables and helper predicates for detection logic
- `analyze/unused_files.rs`, `unused_exports.rs`, `unused_deps.rs`, `unused_members.rs` — Per-issue-type detection
- `scripts/` — Shell command parser for package.json scripts: extracts binary names, `--config` args, file path args. Shell operators split correctly. `ci.rs` scans `.gitlab-ci.yml` and `.github/workflows/*.yml` for binary invocations.
- `suppress.rs` — Inline suppression parsing; 12 issue kinds including `code-duplication` and `circular-dependency`
- `duplicates/` — Clone detection: `families.rs` (grouping + refactoring suggestions), `normalize.rs` (configurable normalization), `tokenize.rs` (AST tokenizer with type stripping)
- `cross_reference.rs` — Cross-references duplication with dead code analysis
- `plugins/` - Plugin system: `Plugin` trait, registry (110 built-in, ~42 with AST-based config parsing), `config_parser.rs` (Oxc-based helpers), `tooling.rs` (general tooling dep detection)
- `trace.rs` — Debug/trace tooling and `PipelineTimings` for `--performance`
- `spawn.rs`: Canonical process-spawn boundary. `spawn::git()` is the ONLY sanctioned `std::process::Command::new` in fallow-core/extract/graph; those crates pin `#![cfg_attr(not(test), deny(clippy::disallowed_methods))]` (banning `Command::new` via `.clippy.toml`, set to `allow` workspace-wide for cli/mcp). Analysis must never execute the analyzed project's code; route any new git invocation through `spawn::git()`. Adding a raw `Command::new` to these crates is a build failure by design. See `SECURITY.md` and the `safe_analysis` integration test.
