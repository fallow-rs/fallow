# CLI internals

Use this reference for command parsing, orchestration, mutation, and rendering.
Use live `fallow --help` and generated contracts for the complete public
inventory.

## Ownership

- `crates/cli/src/main.rs` is a thin binary delegator.
- `crates/cli/src/lib.rs` owns Clap definitions, top-level dispatch, and the
  multicall surface.
- `crates/cli/src/check/`, `audit.rs`, `dupes.rs`, `health/`, `security.rs`,
  and `coverage/` translate CLI options into engine or API calls.
- `crates/cli/src/report/` owns terminal rendering and CLI format dispatch.
- `crates/cli/src/fix/` owns mutation planning and application.
- `crates/api/` owns reusable typed execution and output assembly.
- `crates/engine/` owns analysis, duplication, health, discovery, and
  command-neutral project state.
- `crates/output/` owns serialized report types and stable envelopes.

Analysis logic belongs below the CLI. A CLI module may validate arguments,
resolve paths, select an execution mode, render a result, and map the result to
an exit code.

## High-value paths

- `crates/cli/src/audit.rs`: changed-code audit across dead code, complexity,
  duplication, and styling.
- `crates/cli/src/base_worktree.rs`: temporary base snapshots and cleanup.
- `crates/cli/src/check/`: dead-code filters, severities, workspaces, and
  baselines.
- `crates/cli/src/report/`: human and machine-readable rendering.
- `crates/cli/src/fix/`: dry-run plans and confirmed mutations.
- `crates/cli/src/coverage/` and `license/`: runtime coverage and license
  command orchestration.
- `crates/cli/src/telemetry.rs`: local opt-in telemetry state and spooling.
- `crates/cli/src/cli_impact.rs` and `impact.rs`: local Impact history,
  attribution, aggregation, and the status-bar surface.
- `crates/cli/src/runtime_support.rs`: shared config and ownership helpers.

## Invariants

- Resolve user-provided file inputs against the user's project root before an
  audit switches to a base worktree. Prefix values such as `--coverage-root`
  remain absolute prefixes and must not be reinterpreted as input files.
- Audit worktree cleanup must be scoped to Fallow-owned paths and registrations.
  Never prune unrelated user worktrees.
- JSON mode emits structured errors on stdout and keeps progress off stdout.
- Reported project paths remain relative unless an editor or protocol contract
  explicitly requires absolute paths.
- Serialized lists and human output use deterministic ordering.
- `fix` remains preview-first. Non-interactive mutation requires explicit
  confirmation.
- `fallow impact statusline` stays path-free, read-only, plain text, and
  epilogue-free. Its trend compares only whole-project scans.
- New output fields must move schemas, generated TypeScript contracts, MCP,
  LSP, VS Code, GitHub Action, and GitLab consumers together.

## Verification

Start with focused CLI tests for the changed command. For output or schema
changes also run:

```bash
npm run generate:contracts:check
cargo test -p fallow-cli
npm run verify:fast
```

Run the matching format or integration review skill when a public rendering
surface changes.
