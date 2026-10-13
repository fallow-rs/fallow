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
| `fallow mutation inspect <mutant_id> --report <file>` | no | `mutation-inspect` |

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
- The project files on disk never change. The instrumented files live in the
  Fallow cache.

## Exit codes

| Code | Meaning |
|---|---|
| `0` | The plan, the run or inspect finished. Survivors do not fail a run. |
| `1` | `--min-score` or `mutation.minScore` is set, and the score is lower. |
| `2` | Invalid input or an execution error: a dirty worktree without `--allow-dirty`, a missing runtime or runner, a companion file with a wrong digest, an invalid verdict document. |

The flag wins over the config key. A run with no counted mutant has no score
and exits `0`.

## Score

`score = (killed + timeout) / (killed + timeout + survived + no_coverage)`.
`runtime_error`, `flaky`, `pruned`, `pending` and `equivalent` mutants do not
count. Only `completion.status: "complete"` makes an empty survivor list
conclusive.

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
file. The report holds no source text, so inspect instruments the current file
in memory and finds the mutant by id and span.

- `fresh`: the file has the SHA-256 digest of the report, and the instrumented
  file gives the mutant. The packet shows the original and mutated lines.
- `stale`: `file-missing`, `no-digest`, `source-changed` or
  `mutant-not-reproduced`. The packet shows no code, so a changed file never
  gives a wrong answer.

The run report lists a digest for each mutated file. The plan lists digests
only for the files that it instruments, so a plan mutant in a file that no test
reaches inspects as `no-digest`.

## Human output

- The header says `(ran the project tests)` only when a companion started or a
  mutant has a tested result. Otherwise it says `(no test ran)`.
- One summary line: the score, killed, survived, no coverage, timeout, and the
  completion status. The line adds `equivalent`, `runtime error` and `flaky`
  only when the count is not zero. A partial run adds the pending count to the
  completion status, for example `partial (mutant-limit, 4 pending)`.
- Survivors grouped by file, files and mutants in rank order, each with the
  location, the operator, the original and mutated line, and the covering
  tests.
- A separate no-coverage section, the pruned counts by reason, and one line for
  "no tests" or "no mutation data", never one line for each function. A
  project with no test files gets the one "no tests" line with the mutant
  count, and no no-coverage section. An empty changed-code scope also says
  that `--all` or paths widen the scope.
- The next steps. A human run saves no report, so the first step is the
  command that saves a JSON report, marked as a step that runs the tests
  again. The command repeats the inputs that select or decide mutants: the
  scope with the base ref of the run, the paths, the operators, the limits,
  the side-effect override, the runtime and the verdict document. The second
  step is `fallow mutation inspect`, which runs no code.

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
