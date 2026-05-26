---
paths:
  - "crates/core/**"
---

# fallow-core crate

Re-exports fallow-extract and fallow-graph for backwards compatibility.

Key modules:
- `discover.rs` — File walking + entry point detection (workspace-aware). Hidden directory allowlist (`.storybook`, `.vitepress`, `.well-known`, `.changeset`, `.github`). Only root-level `build/` is ignored (not nested).
- `analyze/mod.rs` — Orchestration: runs all detectors, collects `AnalysisResults`
- `analyze/predicates.rs` — Lookup tables and helper predicates for detection logic
- `analyze/unused_files.rs`, `unused_exports.rs`, `unused_deps.rs`, `unused_members.rs` — Per-issue-type detection
- `scripts/` — Shell command parser for package.json scripts: extracts binary names, `--config` args, file path args. Shell operators split correctly. `ci.rs` scans `.gitlab-ci.yml` and `.github/workflows/*.yml` for binary invocations.
- `suppress.rs` — Inline suppression parsing; 12 issue kinds including `code-duplication` and `circular-dependency`
- `duplicates/` — Clone detection: `families.rs` (grouping + refactoring suggestions), `normalize.rs` (configurable normalization), `tokenize.rs` (AST tokenizer with type stripping)
- `cross_reference.rs` — Cross-references duplication with dead code analysis
- `plugins/` - Plugin system: `Plugin` trait, registry (107 built-in, ~41 with AST-based config parsing), `config_parser.rs` (Oxc-based helpers), `tooling.rs` (general tooling dep detection)
- `trace.rs` — Debug/trace tooling and `PipelineTimings` for `--performance`
