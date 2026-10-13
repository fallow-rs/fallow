# Mutation testing

`fallow mutation` is an opt-in workflow that measures how well the tests of a
project detect changes in the code. It makes small changes (mutants) in the
source, runs the project tests against each mutant, and reports which mutants
no test detects. Fallow stays an evidence engine: it reports facts and
read-only next steps. It never writes a test and never proposes a code change.

## Command surface

| Invocation | Runs project code | Output kind |
|---|---|---|
| `fallow mutation --plan` | no | `mutation-plan` |
| `fallow mutation` | yes | `mutation` |
| `fallow mutation inspect <mutant_id> [--report <file>]` | no | `mutation-inspect` |

- The default scope is the changed code against the detected base ref, the same
  way as `fallow audit`. `--changed-since <ref>` (alias `--base`) sets the
  base, and `--diff-file` gives the diff.
- `--all` mutates each source file. Positional paths mutate only those files or
  directories. The three scopes are exclusive.
- `--workspace` and `--changed-workspaces` exit with code 2 in version 1,
  because the plan cannot narrow to a workspace yet. Pass the workspace
  directory as a path.
- The run flags are `--max-mutants`, `--time-budget` (`90s`, `10m`, `1h`),
  `--concurrency`, `--runtime`, `--allow-dirty`, `--allow-side-effects`,
  `--min-score`, `--operators` and `--verdicts`.
- Only `--format human` and `--format json` are valid. Version 1 has no SARIF,
  CodeClimate, LSP, gate, GitHub Action or GitLab surface.
- Each run saves its full JSON report to
  `<cache dir>/mutation/last-report.json` (by default `.fallow/mutation/`).
  The cache directory has a `.gitignore` with `*`, so the saved report does
  not make the worktree dirty.
- `-o <path>` (`--output-file`) writes the report only after the run. The
  worktree check leaves that path out, so a report from an earlier run does
  not make the worktree dirty. A shell redirect such as `> mutation.json`
  creates the file before Fallow starts, so the dirty-worktree check refuses
  the run. Use `-o` or the saved report instead.

## Safety boundary

- Only `fallow mutation` without `--plan` runs code. Project config cannot
  start a run. The MCP server exposes the plan and inspect only.
- The run refuses a dirty git worktree unless `--allow-dirty` is set. After the
  run, it compares the worktree with the start state and reports each changed
  path.
- Mutants in functions that reach a file system, process, network or database
  sink are pruned (`side-effect-sink`). `--allow-side-effects` keeps them. This
  override is a flag only: the config rejects `allowSideEffects`, so a project
  config cannot widen what a run executes.
- Fallow does not write to project files. The instrumented files live in the
  Fallow cache. The report lists any path that the tests changed.

## Exit codes

| Code | Meaning |
|---|---|
| `0` | The plan, the run or inspect finished. Survivors do not fail a run. |
| `1` | `--min-score` or `mutation.minScore` is set, and the score is lower. |
| `2` | Invalid input or an execution error: a dirty worktree without `--allow-dirty`, a missing runtime or runner, a companion file with a wrong digest, an invalid verdict document. |

The flag wins over the config key. The minimum applies to the score, in
percent. A run with no executed mutant has no score and exits `0`, also when
each mutant is `no_coverage`.

## Score and coverage

- `summary.score` covers the executed mutants only:
  `score = (killed + timeout) / (killed + timeout + survived)`.
- `summary.coverage` reports the unreached mutants apart:
  `coverage = covered / (covered + no_coverage)`, with
  `covered = killed + timeout + survived`.
- `no_coverage` never enters the score. A capped run of 200
  mutants in a project with 20000 unreached mutants still gets a score that
  describes the 200 tested mutants.
- `pending`, `equivalent`, `flaky`, `runtime_error` and `pruned` mutants count
  in neither value.
- Both values are fractions from 0 to 1 in JSON. `--min-score` and
  `mutation.minScore` use percent.
- Only `completion.status: "complete"` makes an empty survivor list
  conclusive.

## Runner notes

`runner.notes` lists facts of the runner setup that limit the result. The key
is absent when the runner states no fact.

- `typecheck-not-run`: the project config enables the Vitest type-check pass
  (`typecheck.enabled`). The run does not execute it, so a mutant that only a
  type test kills is not counted as killed. The human output prints one line
  for this note.

`runner` is absent when no companion started, for example in a project with
no test files.

## Verdicts for equivalent mutants

A verdict document replaces an inline suppression comment:

```json
{
  "schema_version": "1",
  "verdicts": [
    { "review_key": "3fc446a8dd3213b8", "verdict": "equivalent", "note": "same result for each input" }
  ]
}
```

- `review_key` is the `mutant_id`. The id hashes the path, the text of the
  enclosing function, the span in that function, the operator and the
  replacement. It stays the same until the function changes, so a separate
  `review_key` field would carry no extra information.
- A verdict for a `survived`, `no_coverage` or `flaky` mutant sets the status
  `equivalent`. A verdict for a killed or timed-out mutant is `contradicted`,
  and the status stays. A verdict for a `pending`, `runtime_error` or `pruned`
  mutant is `deferred`. A verdict that matches no mutant is `unmatched`; one
  document can serve many scoped runs.
- A malformed document, an empty `review_key` or a duplicate `review_key` stops
  the command with exit code 2 before any test runs. Unknown fields are
  refused. The check is `fallow_api::validate_mutation_verdicts`.
- The join is `fallow_api::apply_mutation_verdicts`. The report records the
  document digest and each list under `verdicts`. A run without `--verdicts`
  has no `verdicts` key.
- Version 1 still runs the mutants that a verdict marks. The join happens after
  the run.

## Inspect

`inspect` reads a saved `mutation` or `mutation-plan` document and the current
file. Without `--report`, it reads the report that the last run saved in the
Fallow cache. Pass the same `--root` and `--config` as the run, because the
config can move the cache directory. When no run saved a report, inspect exits
with code 2 and `FALLOW_MUTATION_NO_REPORT`. Each run replaces the saved
report, so an id from an earlier run exits with code 2 and
`FALLOW_MUTATION_UNKNOWN_MUTANT`. Pass `--report <file>` to read another saved
report. The report holds no source text,
so inspect instruments the current file in memory and finds the mutant by id
and span.

- `fresh`: the file has the SHA-256 digest of the report, and the instrumented
  file gives the mutant. The packet shows the original and mutated lines.
- `stale`: `file-missing`, `no-digest`, `source-changed` or
  `mutant-not-reproduced`. The packet shows no code, so a changed file never
  gives a wrong answer.

The run report lists a digest for each mutated file. The plan lists digests
only for the files that it instruments, so a plan mutant in a file that no test
reaches inspects as `no-digest`.

## Human output

- The output uses the section headers and colors of the other commands.
  `NO_COLOR`, a pipe and `-o` remove the colors.
- The header says `(ran the project tests)` only when a companion started or a
  mutant has a tested result. Otherwise it says `(no test ran)`.
- The score line: the score, killed, survived, timeout, and the completion
  status. The line adds `pending`, `equivalent`, `runtime error` and `flaky`
  only when the count is not zero.
- The scope line shows a full 40-character base SHA as its first 12
  characters, as `audit` does.
- The coverage line: the coverage and the no-coverage count.
- One line when `runner.notes` has `typecheck-not-run`: type-level kills are
  not counted.
- Survivors grouped by file, files and mutants in rank order, each with the
  location, the operator, the original and mutated code, and one line with
  the count of covering tests and the first test name, cut to 60 characters.
  A long code line shows a window around the change, so the changed token
  stays visible. The JSON report lists all covering tests.
- The `inspect` output shows the first 10 covering tests under a
  `Covering tests (N)` header and one `and N more tests` line. The
  `--format json` packet lists all covering tests.
- The plan lists the planned mutants under a `Planned (N)` header, in run
  order.
- The minimum-score line shows a green check mark when the score meets the
  minimum, a red cross when it misses the minimum, and the dimmed `[-]` prefix
  when no score is measured.
- A separate no-coverage section with one line for each file and its count.
  The section shows the 10 files with the most mutants and one
  `and N more files` line. The JSON report has the full list.
- The pruned counts by reason, and one line for "no tests" or "no mutation
  data", never one line for each function. A project with no test files gets
  the one "no tests" line with the mutant count, and no no-coverage section.
  An empty changed-code scope also says that `--all` or paths widen the scope.
- The next step is read-only: `fallow mutation inspect <id>` for the first
  survivor. It reads the saved report of the run, so no step runs the tests
  again. The step repeats `--root` and `--config` when the run used them.

## Config

```json
{
  "mutation": {
    "runner": "auto",
    "operators": ["arithmetic-operator", "relational-operator"],
    "minScore": 80,
    "concurrency": 4
  }
}
```

- `runner` is `auto` or `vitest`. Version 1 runs Vitest 4 and 5.
- `operators`, `minScore` and `concurrency` apply to each run. A CLI flag wins.
- The section has no source globs and no timeout keys. `MutationRunOptions`
  has no field for them, and a public key must have an effect. The section
  rejects unknown keys, so a config with `include`, `exclude`, `timeoutFactor`
  or `timeoutMs` fails to load. Add a key only together with its run option.
- The section is not part of the detection digest of dead-code findings.

## Ownership

| Part | Path |
|---|---|
| Mutant sites and instrumentation | `crates/mutation` |
| Selection, test map, prune and rank | `crates/engine/src/mutation` |
| Runner protocol, companion, scheduler, report, verdict join | `crates/api/src/mutation` |
| Envelopes | `crates/output/src/mutation.rs` |
| Config section | `crates/config/src/config/mutation.rs` |
| CLI adapter, human output, inspect | `crates/cli/src/mutation_cli.rs`, `mutation_human.rs`, `mutation_inspect.rs` |
| MCP plan and inspect tools | `crates/mcp/src/tools/mutation.rs` |
| Node companion | `tools/mutation-runner` |

## Tests

- `cargo test -p fallow-cli mutation` runs the CLI unit tests and the
  integration tests in `crates/cli/tests/integration/mutation_tests.rs`.
- The integration tests use a project with no test files for the exit policy,
  the verdict join and inspect, so they need no JavaScript runtime.
- The Vitest end-to-end test needs `node` on `PATH` and `npm ci` in
  `tools/mutation-runner`. Without them it prints a skip message and passes.
