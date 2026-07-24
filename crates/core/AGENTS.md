# Core Agent Guide

Use this file when editing `crates/core/**`.

## Ownership

- `discover/`: core discovery backend. Public orchestration lives in
  `crates/engine/src/discover.rs`.
- `analyze/`: dead-code and structural issue detection.
- `plugins/`: built-in framework and tool integrations.
- `scripts/`: package scripts and dependency-usage parsing.

## Rules

- Follow pipeline order: config, discovery, extract, graph, core, engine, API,
  output, then host adapters.
- Keep detector behavior conservative. Prefer one missed advisory finding over a noisy false positive unless the rule is explicitly strict.
- Do not hide diagnostics by broad ignores when a narrower fixture or parser fix is possible.
- Use `FxHashMap` and `FxHashSet` for hot analysis data structures.
- Preserve stable ordering before returning results to the CLI.
- Treat generated, vendored, malformed, and fixture-heavy projects as normal input. Warnings should explain the degraded path and avoid aborting unrelated analysis.

## Validation

- Add or update integration fixtures under `tests/fixtures` for behavior changes.
- Run targeted core tests first, then broaden to workspace tests when shared behavior changes.
- For changed detection behavior, smoke at least one real project in addition to synthetic fixtures.
