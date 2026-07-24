# CLI Agent Guide

Use this file when editing `crates/cli/**`.

## Ownership

- `main.rs`: thin binary delegator to the reusable CLI library.
- `lib.rs`: clap definition, top-level command dispatch, and multicall command surface.
- `check/`: dead-code command filters, severity handling, workspace filtering, and baseline routing.
- `audit.rs`: changed-code audit orchestration and base snapshot comparison.
- `report/`: user-visible and machine-readable output formats.
- `fix/`: mutation logic for auto-fixes.
- `health/`, `dupes.rs`, `coverage/`, `license/`: CLI-specific command orchestration over engine and API services.
- `crates/engine/`: reusable analysis orchestration consumed by CLI commands.
- `crates/api/`: programmatic facade used by the CLI and host integrations.
- `crates/output/`: shared report and serialization contracts.

## Rules

- Keep analysis logic out of the CLI when it belongs in `crates/engine`,
  `crates/api`, `crates/core`, `crates/graph`, or `crates/extract`.
- JSON output is an integration contract. Preserve existing fields unless the schema version and downstream consumers move together.
- Every issue emitted to users should keep a useful `actions` array where that issue family supports actions.
- Error output must stay structured when `--format json` is active.
- Paths in reports should remain project-relative unless a protocol or editor surface explicitly requires absolute paths.
- Use deterministic ordering for any new output list, map projection, or snapshot-covered text.

## Validation

- CLI output change: run the targeted CLI tests and update snapshots deliberately.
- JSON or schema change: regenerate and verify `docs/output-schema.json`, VS Code generated types, LSP, MCP, GitHub Action, and GitLab consumers.
- Action or CI format change: run the matching shell test suite under `action/tests` or `ci/tests`.
