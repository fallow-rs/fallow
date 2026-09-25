# Drift contract

Fallow reports one analysis through several commands and several surfaces. The
rules below say which results must agree. A result that breaks a rule is drift.

The drift harness in `crates/cli/tests/drift/` generates small projects and
checks the rules that have the status "checked by the harness". A rule with the
status "pending" breaks on `main` today. The change that fixes it also adds it
to the harness, as the test that failed before the fix.

## Terms

- **Surface**: one way to get a result. The harness drives three surfaces:
  - the CLI binary with `--format json`. I7 also reads the exit code of the
    human format,
  - the `fallow-mcp` server over stdio JSON-RPC, on the typed path (in-process
    `fallow_api`) and on the CLI-fallback path (a `fallow` subprocess),
  - `fallow_api` in-process. It stands in for the Node bindings, which call
    the same functions. The Node test `crates/napi/test.mjs` checks the
    marshaling of the Node bindings: it compares the finding keys of
    `detectDeadCode`, `detectDuplication` and `computeHealth` with the CLI
    `dead-code`, `dupes` and `health` output, without a scope and with one
    workspace.

  The LSP server is not in the contract yet. It needs a scripted editor session.
- **Finding key**: the identity of one finding. It is the tuple (issue kind,
  root-relative path, symbol or package name, line). The harness compares key
  sets, not presentation fields such as `actions`, columns, or prose.
- **Volatile field**: a field that changes between two runs of the same
  analysis. `canonical_report` in `crates/cli/tests/common/mod.rs` removes
  them before a comparison:
  - `elapsed_ms` and `head_sha`, at any depth (`VOLATILE_REPORT_FIELDS`),
  - `_meta.telemetry.analysis_run_id`.

The harness builds keys with one normalizer per envelope shape
(`crates/cli/tests/drift/keys.rs`):

| Envelope | Issue kind | Path | Symbol | Line |
|---|---|---|---|---|
| Dead code | The array name, for example `unused_exports` | `path`, or the `files` joined with ` -> ` | The first field that is present, in this order: `export_name`, `package_name`, `member_name`, `name`, `specifier`, `entry_name`, `catalog_name`. When the finding has a `parent_name`, the key is `parent_name.symbol` | `line`, or 0 |
| Dupes | `code-duplication`, one key for each clone group | The instance files joined with ` -> ` | Each instance as `file:start-end` | The first start line |
| Health | `complexity`, one key for each entry in `findings` | `path` | Function `name` | `line` |
| Combined | The three sections above | | | |
| Audit | The three sections above, split into introduced and inherited | | | |

An MCP result goes through the normalizer of the envelope in its text content.

## Invariants

| ID | Rule | Status |
|---|---|---|
| I1 | `check` output equals `dead-code` output | Checked by the harness |
| I2 | Finding sets are equal on every surface | Checked by the harness |
| I3 | Each section of bare `fallow` equals its standalone command | Checked by the harness |
| I4 | Audit attribution covers the head findings in changed files | Checked by the harness |
| I5 | Audit gives the same result on every surface | Checked by the harness |
| I6 | A suppression or a baseline entry never adds a finding | Checked by the harness |
| I7 | Every machine envelope carries the verdict of the human run | Checked by the harness |
| I8 | Scope flags narrow the same way on every command and surface | Checked by the harness |
| I9 | The `--performance` work counters do not depend on the thread count or the command alias | Checked by the harness |

### I1: `check` is an alias of `dead-code`

- **Statement**: `fallow check` and `fallow dead-code` with the same flags give
  byte-identical JSON and the same exit code.
- **Surfaces**: CLI.
- **Comparison**: the full JSON report, after the volatile fields are removed.
- **Designed exceptions**: the volatile fields.
- **Status**: checked by the harness.

### I2: surface equality

- **Statement**: for the same project and flags, the dead-code, dupes and
  health finding sets are equal on the CLI, on MCP (typed path and
  CLI-fallback path) and on `fallow_api` in-process.
- **Surfaces**: CLI `dead-code`, `dupes` and `health`; MCP `analyze`,
  `find_dupes` and `check_health`; `fallow_api::run_dead_code`,
  `run_duplication` and `run_health`.
- **Comparison**: finding keys.
- **Designed exceptions**: health compares only `findings`. The other health
  sections (file scores, hotspots, targets) are not finding sets.
- **Status**: checked by the harness.

### I3: combined composition

- **Statement**: each section of bare `fallow` equals the standalone command
  with the same flags and baselines.
- **Surfaces**: CLI bare `fallow` against `dead-code`, `dupes` and `health`.
  Bare `fallow` loads the baselines with `--baseline`, `--dupes-baseline` and
  `--health-baseline`, the names `audit` uses.
- **Comparison**: finding keys for each section. The harness compares each
  project twice: without baselines, and with a partial baseline of each
  analysis (the same partial files as I6).
- **Positive control**: on the fixed project, a full baseline of each
  analysis empties the matching section of bare `fallow`, and each section
  still equals its standalone command.
- **Designed exceptions**: none.
- **Status**: checked by the harness.

### I4: audit attribution

- **Statement**: the introduced findings plus the inherited findings of
  `audit` equal the head findings in the changed files. A finding is
  introduced when its key is absent at the base commit, after the base keys
  follow renames. Dependency findings are in scope only when the manifest
  changed.
- **Surfaces**: CLI `audit`.
- **Comparison**: finding keys, split by attribution. The harness builds the
  expected split without the audit code:
  - The head findings are the keys of `dead-code`, `dupes` and `health` on
    the head commit. A key is in scope when one of its paths is a changed
    file: a file whose content differs from the base commit, or a new path.
    A dependency finding has its manifest as its path.
  - The base findings are the keys of the same commands on a copy of the
    base commit. Their paths follow the renames of the head commit.
  - A rename counts only when git detects it (`git diff --find-renames`,
    default similarity threshold), so an edit that drops the similarity below
    the threshold makes a delete plus an add.
  - A head key is introduced when no base key has the same identity: the
    kind, the paths and the symbol, without line numbers. A clone group has
    the line counts of its instances in its identity in place of its symbol,
    because its symbol holds line ranges. The audit key of a clone group
    holds its size, so a clone that grows with added lines is a new clone
    group.
  - A clone group with a new identity is inherited when none of its
    instances holds an added line of the diff against the base commit. This
    models the clone-group demotion of `new-only` (#2164): the change did not
    write the duplicated text.
- **Positive control**: on a fixed project whose head commit renames a file
  with an unused export and adds a dependency to the manifest, the expected
  split holds the moved export and the old dependency as inherited and the new
  dependency as introduced, and the audit matches it. Git detects the rename
  of the control. The same project
  without the manifest change has no dependency finding in the audit.
- **Designed exceptions**: none.
- **Status**: checked by the harness.

### I5: audit surfaces

- **Statement**: CLI `audit`, MCP `audit` and `fallow_api::run_audit` give the
  same introduced set, the same inherited set and the same verdict.
- **Surfaces**: CLI, MCP (typed path), `fallow_api` in-process. The MCP
  server of this check has `FALLOW_BIN` set to a file that does not exist, so
  a CLI fallback cannot answer the call.
- **Comparison**: finding keys, split by attribution, and the verdict.
- **Positive control**: the fixed project of the I4 control, on all three
  surfaces, with and without the manifest change. Without the manifest
  change, no surface reports a dependency finding.
- **Designed exceptions**: none. All three surfaces run one implementation,
  `fallow_api::audit_run`. Each surface only supplies the runners of the three
  analyses and the base checkout.
- **Status**: checked by the harness.

### I6: suppression and baseline monotonicity

- **Statement**: adding a suppression comment or a baseline entry never adds a
  finding, on any command.
- **Surfaces**: CLI `dead-code`, `dupes`, `health` and bare `fallow` for
  suppression comments; CLI `dead-code`, `dupes` and `health` for baselines.
  For a dead-code baseline, `fallow_api::run_dead_code_with_baseline` must
  also give the same keys as the CLI with the partial baseline. Both read the
  file with `fallow_engine::baseline::apply_dead_code_baseline`.
- **Comparison**: finding keys. The harness renders each project twice. One
  copy has the suppression comments, the other has plain comments on the same
  lines, so no finding moves to another line. For baselines, the harness saves
  a baseline, keeps a part of its entries, and checks that the full baseline
  reports a subset of the partial baseline, which reports a subset of no
  baseline.
- **Positive control**: on a fixed project, each finding with a suppression
  comment disappears from `dead-code`, `dupes`, `health` and bare `fallow`, and
  a full baseline removes every finding of each analysis. Without this control,
  a run that ignores comments and baselines passes every subset check.
- **Designed exceptions**: `stale_suppressions` findings. They report the
  suppression comment itself when it matches nothing.
- **Status**: checked by the harness.

### I7: verdict in every envelope

- **Statement**: every machine envelope carries a verdict in `gate_outcomes`,
  and that verdict equals the verdict of the human run. The exit code follows
  the documented rule for each command.
- **Surfaces**: CLI `dead-code`, `dupes`, `health`, `security`, `audit` and
  bare `fallow`, in JSON and in the human format. `dead-code`, `dupes`,
  `health` and bare `fallow` also in grouped JSON (`--group-by directory`).
  MCP `analyze`, `find_dupes` and `check_health` on the CLI-fallback path.
  Each case also arms gates beyond the default rules:
  - `security --gate new --changed-since` against the base commit,
  - `dead-code` and bare `fallow` with `--fail-on-regression` against a
    regression baseline. The baseline holds the counts of the head commit,
    or zero counts, so the gate passes in some cases and fails in others.
- **Comparison**: the stated verdict of `gate_outcomes` and the exit codes.
  A run fails when an entry has `status: "fail"`. The machine run fails when
  such an entry is also `enforced`. A failed gate exits 1, except
  `security --gate`, which exits 8.
  - Standalone commands: the JSON exit code, the grouped JSON exit code and
    the human exit code all equal the code of the enforced entries that fail.
  - Bare `fallow`: the JSON runs exit with the code of the enforced entries
    that fail, and the human run exits 1 exactly when an entry has
    `status: "fail"`.
  - MCP: the CLI-fallback result states the same verdict as the CLI run.
- **Positive control**:
  - On the fixed project, bare `fallow --format json` states a failing
    verdict and exits 0, and the human run exits 1.
  - Each armed gate fails on a fixed project, and the exit code follows:
    `--fail-on-regression` against zero counts and `--fail-on-stale-baseline`
    against a baseline with stale entries exit 1 on `dead-code` and on bare
    `fallow --format json`. `security --gate new` on a head commit that adds
    a sink exits 8.
  - A fixed project with a `.fallowrc.json` runs every I7 command. An
    `overrides` entry for `package.json` sets the severity of an unused
    dependency override, and `dead-code --changed-since` and both audit gates
    reach the verdict that the override sets. The same project holds a
    duplicate export that only `ignoreFindings` files hold after
    `--changed-since`, and every surface hides it (I8). The generator writes no
    config file, so only this project reaches per-file severity.
- **Designed exceptions**:
  - Bare `fallow` in a machine format exits 0 when it has findings. Its
    entries report `enforced: false`, except `regression`,
    `stale-baseline`, `type-aware-require` and `parse-error`.
  - `dupes` has no default exit rule. Its envelope carries `gate_outcomes`
    only when a gate armed, and an absent object means that the run passed.
  - `fallow_api` and the MCP typed path run no CLI gate and publish no
    `gate_outcomes`, so the harness compares verdicts only across the CLI and
    the MCP tools that return the CLI envelope.
  - A stale-baseline verdict that `--fail-on-stale-baseline` did not arm has
    `status: "fail"` and does not fail the human run. The generated cases run
    I7 without baselines. The positive control arms the gate.
  - The harness cannot make the type-aware pass incomplete, so it does not
    arm `type-aware-require`. Unit tests in `crates/cli/src/gates.rs` pin that
    entry against the exit code.
  - SARIF and CodeClimate have no place for a verdict.
- **Status**: checked by the harness.

### I8: scope flags

- **Statement**: `--changed-since`, `--workspace` and `--production` narrow
  the same way on every command and every surface.
- **Surfaces**: the surfaces of I2. MCP `check_changed` takes the place of
  `analyze` when the run has `--changed-since`.
- **Comparison**: for each scope flag and each analysis:
  - the finding keys are equal on every surface,
  - the scoped run holds no key that the run without the flag lacks
    (`--changed-since` and `--workspace` only),
  - every key touches the scope: a changed file, or a path in the selected
    workspace package (`--changed-since` and `--workspace` only).
- **Designed exceptions**:
  - `--changed-since` keeps dependency-level findings (for example
    `unused_dependencies`) whatever changed. Whether a dependency is unused is
    a fact about the whole graph, not about one file.
  - A clone group is in scope when one of its instances is in scope.
  - MCP `find_dupes` has no `production` parameter, so the harness does not
    compare MCP for dupes with `--production`.
  - `--production` can add findings, so it has only the equality check.
- **Positive control**: on a fixed workspace project, one clone group has an
  instance in `pkg-a` and an instance in `pkg-b`. With `--workspace pkg-a`,
  every surface keeps the whole group. Without this control, a generator that
  never puts a clone in two packages passes I8 without a real check.
- **Status**: checked by the harness. The scope filters have one
  implementation each in `fallow-engine`: `dead_code::apply_scope`,
  `duplicates::apply_scope` and the diff filters in `diff_scope`. The CLI and
  `fallow_api` call them.

### I9: work counters are deterministic

- **Statement**: the `counters` object of `--performance` is the same for
  `dead-code` with one thread, `dead-code` with four threads and `check` with
  four threads on the same project.
- **Surfaces**: CLI `dead-code` and `check`, with `--performance --no-cache`.
- **Comparison**: the full `counters` object, with exact equality.
- **Designed exceptions**: none. The millisecond fields are not compared,
  because they change from run to run.
- **Status**: checked by the harness. The exact values for three pinned
  fixtures are in `performance_counters_are_exact_on_pinned_fixtures` in
  `crates/cli/tests/check_tests.rs`.

## How the harness works

The generator (`crates/cli/tests/drift/model.rs`) is a proptest strategy for a
small project:

- TypeScript files with used and unused exports, value exports and type
  exports, and imports between files,
- `package.json` dependencies, used and unused, in `dependencies` and
  `devDependencies`,
- inline suppression comments (`// fallow-ignore-next-line <kind>` and
  `// fallow-ignore-file unused-file`),
- a duplicated function and a function above the complexity thresholds,
- an optional npm workspaces layout with two packages,
- a base commit and a head commit. The diff adds, edits, renames (`git mv`)
  and deletes files, and can add an unused dependency to a manifest.

The generator writes no config file. The fixed projects of the positive
controls in `crates/cli/tests/drift/main.rs` cover config: `overrides`,
`ignoreFindings` and baselines.

The runners are in `crates/cli/tests/drift/surfaces.rs`. The MCP runner starts
the `fallow-mcp` binary next to the `fallow` binary, with `FALLOW_BIN` set to
the CLI under test. To force the CLI-fallback path, it passes `save_baseline`.
Each analysis tool sends a baseline parameter to the CLI, and saving a baseline
does not change the findings of the run. The runner then checks that the
baseline file exists, which proves that the fallback path ran.

The runners remove `FALLOW_*` variables from each child process, so a developer
shell cannot change one surface and not the others.

## Budgets and CI

| Setting | Default | Meaning |
|---|---|---|
| `FALLOW_DRIFT_CASES` | A small fixed count | Cases for each invariant |
| `FALLOW_DRIFT_SEED` | A fixed seed | A number, or `random` |

- The `drift` job in `.github/workflows/ci.yml` runs the harness with the fixed
  seed on each pull request that changes `crates/**`. It blocks the merge.
- The `drift-full` job in `.github/workflows/release-validation.yml` runs a
  large case count with a random seed. Release validation gates publication and
  also runs every week.

The harness needs the `fallow-mcp` binary. When the binary is missing, the
harness fails with the build command. It never skips the MCP surface.
`cargo test -p fallow-cli` does not rebuild `fallow-mcp`. When a source file
of `fallow-mcp` is newer than the binary, the harness fails with the same
build command.

```bash
cargo build -p fallow-mcp
cargo test -p fallow-cli --test drift -- --include-ignored
```

The invariant tests are marked `#[ignore]`, because a workspace-wide
`cargo test` does not build `fallow-mcp`. The `drift` CI job and the
release validation job run them with `--include-ignored`.

## Reproduce a failure

A failure message gives the seed, the case count, the shrunk project with its
files, and the difference between the two key sets.

1. Build the binaries: `cargo build -p fallow-mcp`. A local run needs this
   step again after each change outside `crates/cli`.
2. Run the failing test with the same seed and case count:

   ```bash
   FALLOW_DRIFT_SEED=<seed> FALLOW_DRIFT_CASES=<cases> \
     cargo test -p fallow-cli --test drift <test name> -- --include-ignored
   ```

3. Proptest writes the failing case to
   `crates/cli/tests/drift/drift.proptest-regressions`. Commit that line with
   the fix. The harness replays each saved case before it generates new cases.

## Add an invariant

1. Write the predicate in `crates/cli/tests/drift/invariants.rs`. It returns
   `Ok(())` or a readable difference.
2. When the invariant reads a new envelope shape, add a normalizer to
   `crates/cli/tests/drift/keys.rs`.
3. Add a test in `crates/cli/tests/drift/main.rs` that calls `run_invariant`.
4. Add a section to this document with the status "checked by the harness".
5. When the invariant fails on `main`, fix the drift in the same change. Do not
   add the invariant with an expected failure.
