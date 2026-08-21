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
- Audit base coverage attribution (#2347): the base-worktree pass scores from
  the same head-generated Istanbul map as the head pass, for explicit and
  auto-detected coverage alike. When no `--coverage-root` was given, the
  canonical head project root becomes the strip prefix so every recorded path
  rebases onto the base worktree; an explicit prefix is forwarded unchanged.
  Relocated base lookups (`coverage_relocated`) tolerate unbounded line drift
  only when every same-named entry in the file agrees on one value.
  Consequences: complexity growth in files the map reports as untested
  attributes as inherited once the base function also exceeds the threshold,
  and test deletions cannot surface as `introduced` complexity findings (the
  base is scored with the post-deletion map); coverage regressions belong to
  `rules.coverage-gaps` and health trends. The auto-detected map's content
  participates in the base-snapshot cache key exactly like `--coverage`.
- Audit coverage inputs (#2359) resolve through the same precedence as
  `fallow health` and bare `fallow`: `--coverage` / `--coverage-root`, then
  `FALLOW_COVERAGE` / `FALLOW_COVERAGE_ROOT`, then `health.coverage` /
  `health.coverageRoot`, then auto-detection. `resolve_coverage_inputs` in
  `crates/cli/src/lib.rs` is the single owner of that order; the resolved
  paths land in `AuditOptions`, so the head pass, the base-worktree rebase,
  and the base-snapshot cache key all see the same map. A configured path
  that does not exist fails audit with the same structured exit 2 as health.
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
- New-only duplication demotion (issues #2164, #2220): under `--gate new-only`
  an introduced clone group none of whose instances overlap an added line is
  demoted to inherited. Without an opt-in shared diff, the CLI uses the
  merge-base worktree diff for that decision (`crates/cli/src/audit.rs`,
  `demote_preexisting_dupe_introductions`); the programmatic runtime path
  (`crates/api/src/runtime/audit.rs`) also uses its merge-base worktree diff.
  When an opt-in shared diff (`--diff-file`, `--diff-stdin`, or
  `$FALLOW_DIFF_FILE`) is active, it has already filtered the head duplication
  report with the same added-line overlap predicate. Every retained clone group
  therefore vetoes demotion, so a shared diff can prevent a demotion but cannot
  produce a rendered shared-source demotion note. Demoted groups stay counted
  as inherited and additionally surface via
  `attribution.duplication_demoted` and a per-group `demotion_reason` field;
  human output names the deciding diff source in the demotion note.

## Audit cache maintenance

`fallow audit-cache` maintains the reusable base-snapshot caches
(`$TMPDIR/fallow-audit-base-cache-*`) that `fallow audit` builds and
garbage-collects. Two subcommands with distinct semantics:

- `audit-cache remove`: delete every cache owned by an explicit `--root`,
  warm or not. Requires `--root` and `--yes` (non-interactive), exits 2 on
  incomplete removal because it promises completeness.
- `audit-cache prune`: apply the same GC policy every audit run applies
  silently (orphaned-sidecar cleanup, age-based reclaim, cross-repo reclaim
  of abandoned entries), report every considered entry with sizes, and exit
  0 whenever the command ran, including lock-contention skips and per-entry
  failures. Machine consumers gate on the envelope's `complete` field.
  Defaults to the current directory as root. A pre-#1815 registration at the
  current cache path is only deregistered and stays warm on disk: it reports
  as kept with reason `legacy-deregistered`. The envelope's `deregistered`
  field is an informational subset of `kept`, not a fifth member of the
  `removed + kept + skipped + failed == found` partition. It also appears in
  the matching human summary line and never adds its size to
  `reclaimed_bytes`. Released SHA-keyed registrations are genuinely removed
  (reason `legacy-registered`) and stay counted as reclaimed.

Shared invariants (`crates/cli/src/base_worktree.rs`):

- Both prune modes and the per-audit sweep share one decision code path
  (`sweep_reusable_caches_with_report`), so prune can never drift from what
  audits actually reclaim.
- `--dry-run` performs zero filesystem mutation: no cache removal, no
  `.lock` sidecar creation (acquiring a lock would create one), no
  `.last-used` grace seeding, and no git worktree deregistration. The legacy
  registered-cache pass is still enumerated read-only with the same
  `legacy-registered` / `legacy-deregistered` split an apply run reports.
- `.lock` sidecars are permanent lock identities and are never deleted:
  removing an unlinked-but-still-flocked inode while a racer re-creates the
  path would split one lock across two inodes.
- Owner liveness for foreign entries is a NotFound-only probe on
  `std::fs::metadata`: only a definitive NotFound classifies the recorded
  owner root as dead. Every other probe error (EACCES, EIO, ENOTDIR) keeps
  the entry as `owner-unverifiable`, so a transient failure can never
  reclaim a live repo's cache or defeat its `cacheMaxAgeDays: 0` policy. A
  path below an unmounted mountpoint still reads NotFound and is not
  protected. A dangling-symlink owner root resolves NotFound (dead).
- Threshold precedence: `--max-age-days` flag, then
  `FALLOW_AUDIT_CACHE_MAX_AGE_DAYS`, then `audit.cacheMaxAgeDays`, then the
  30-day default. `0` disables age-based reclaim but still reclaims
  orphaned sidecars and dead-owner entries.
- Per-entry GC diagnostics are debug-level tracing shared by the audit sweep
  and prune: `RUST_LOG=fallow=debug fallow audit ...` (or any prune run)
  emits one `audit cache sweep considered entry` line per candidate with
  path, pass, mode, decision, reason, age, threshold, and owner fields. With
  `RUST_LOG` unset, audit stderr is unchanged.
- Prune entry sizes come from a plain recursive walk that never follows
  symlinks and deliberately ignores gitignore semantics: a cache entry is a
  checked-out snapshot whose `.gitignore` would otherwise hide
  `node_modules`, the bulk of the measurement.

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
