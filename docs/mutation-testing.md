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
| `fallow health --mutation-report <file>` | no | `health` |

- The default scope is the changed code against the detected base ref, the same
  way as `fallow audit`. `--changed-since <ref>` (alias `--base`) sets the
  base, and `--diff-file` gives the diff.
- `--all` mutates each source file. Positional paths mutate only those files or
  directories. The three scopes are exclusive.
- `--workspace` and `--changed-workspaces` exit with code 2 in version 1,
  because the plan cannot narrow to a workspace yet. Pass the workspace
  directory as a path.
- The run flags are `--max-mutants`, `--time-budget` (`90s`, `10m`, `1h`),
  `--setup-timeout` (default `2m`),
  `--concurrency`, `--runtime`, `--allow-dirty`, `--allow-side-effects`,
  `--min-score`, `--operators`, `--verdicts` and `--typecheck`. The global
  `--no-cache` flag also turns off the result cache (see "Result cache").
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
| `2` | Invalid input or an execution error: a dirty worktree without `--allow-dirty`, a missing runtime or runner, a companion file with a wrong digest, an invalid verdict document, `--typecheck` without a type-aware companion. |

The flag wins over the config key. The minimum applies to the score, in
percent. A run with no executed mutant has no score and exits `0`, also when
each mutant is `no_coverage`.

## Score and coverage

- `summary.score` covers the executed mutants only:
  `score = (killed + timeout) / (killed + timeout + survived)`.
- `summary.coverage` reports the unreached mutants apart:
  `coverage = covered / (covered + no_coverage)` over the whole plan.
  `covered` counts each executed mutant (`killed`, `timeout`, `survived`,
  `runtime_error`, `flaky`, `compile_error`) with a covering test, and each
  `pending` mutant
  that a passing dry-run test covers. The dry run loads each mutant of the
  plan, also a mutant after the `--max-mutants` limit. A test covers a
  mutant when it hits the mutant, or when its file loads the module of a
  mutant that the module load hits. A static import path alone is not a
  cover. A `pending` mutant that no dry-run test covers is `no_coverage`.
  A run that `--max-mutants` caps therefore reports the coverage and the
  `no_coverage` mutants of the full run.
- `no_coverage` never enters the score. A capped run of 200
  mutants in a project with 20000 unreached mutants still gets a score that
  describes the 200 tested mutants.
- `pending`, `equivalent`, `flaky`, `runtime_error`, `pruned` and
  `compile_error` mutants never count in the score. `equivalent` and `pruned`
  mutants never count in the coverage.
- Both values are fractions from 0 to 1 in JSON. `--min-score` and
  `mutation.minScore` use percent.
- Only `completion.status: "complete"` makes an empty survivor list
  conclusive.

## Retries

A `survived` or `timeout` result gets one retry in a new companion. Host
load can make a test time out, so a single timeout is not proof that the
tests detect the mutant.

| First result | Retry result | Final status |
|---|---|---|
| `survived` | `survived` | `survived` |
| `timeout` | `timeout` | `timeout` |
| `timeout` | `killed` | `killed` |
| any | `runtime_error` | `runtime_error` |
| any | other result | `flaky` |

- The retry of a `timeout` waits twice as long. Host load often lasts into
  the retry, and a real infinite loop still times out.
- A `killed` result gets no retry. In repeated runs under host load, no
  kill became a survivor, and a retry of each kill doubles the run time.
- A retry starts only before the `--time-budget` deadline. A mutant that
  gets no retry keeps its first status, and the completion is `partial`
  (`time-budget`).
- A run that started before the deadline continues until its watchdog
  fires, so the command can end after the deadline. For a mutant run, the
  overrun is at most `timeout_ms` plus the setup time plus 2 seconds. For
  a timeout retry, it is at most twice `timeout_ms` and the setup time,
  plus 2 seconds.
- `retried: true` on a mutant result marks a status from a first run and
  a retry. A `survived` or `timeout` mutant without the field got no retry.
- A retry of a real infinite loop costs two more timeout windows.
- A `runtime_error` retry takes the mutant out of the score, also after a
  first `timeout`. The completion is then `partial` (`runtime-error`).
- A `killed` result after a retry reports the duration of the retry. Each
  other status keeps the duration of the first run.

## Test reach of no-coverage mutants

Each `no_coverage` mutant has a `test_reach` object in the run report and in
the plan. It gives the static reason why no test covers the mutant. The other
statuses do not have the field. It never changes the score or the coverage.

- `reach` uses the classes of the `audit` test reach: `mocked-only`, `none`,
  `none-file`, `transitive` and `direct`. Treat the set as open.
- A mutant in the top-level statement that declares a value export gets the
  class of that export, and `export_name` names the export.
- A mutant outside each value export declaration gets the class of its file
  and has no `export_name`. The class is `none-file` when no test loads the
  file, `mocked-only` when no test loads the file and a mock explains an
  export of the file, and `none` when a test loads the file.
- `tests` and `mocked_targets` are present only for `mocked-only`. They list
  the test files whose references a mock removed and the mocked modules.
- A project with no test files gives `none-file` to each mutant.
- `direct` or `transitive` on a `no_coverage` mutant is not a conflict. The
  graph has a covered reference to the export, but no test ran the code of
  the mutant.

The plan classifies the exports of each instrumented file once, with
`module_graph::module_exports` (the per-file part of
`changed_exports_for_changed_paths`) and
`module_graph::classify_export_test_reach` (the per-export part of the `audit`
test reach). `ModuleGraph::test_entry_points_reaching` stays a separate query:
it names each test entry point that loads a file, with the mask of that test,
and the runner selects the tests from that list. `export_test_reach` reads
the shared profiles of the reachability index. It names only the test files
that reference the export, so it cannot give that list.

## Setup timeout

`--setup-timeout` limits the start, the dry run and each reload of a runner
process. The default is 2 minutes, and units from `ms` to `h` are valid. A
slow or busy host can need more, because the dry run executes each test file.

- For the first runner process, the run stops with exit code 2 and
  `FALLOW_MUTATION_RUNNER_FAILED`, and the help names the flag.
- For a restarted runner process, the mutant is `runtime_error`, and its error
  names the flag. The run continues.
- A reload that passes the limit restarts the runner process.
- The config has no key for this limit.

## Baseline failures

`baseline_failures` lists the tests and test files that fail with no active
mutant. Their results decide no mutant status.

- A test that fails in the first dry run never covers a mutant.
- Each restarted companion runs its own dry run. A covering test that does
  not pass there is a baseline failure of that companion, with the ids of the
  affected mutants in `mutant_ids`. The mutant runs with its other covering
  tests. Only a mutant with no covering test left is `runtime_error`, and its
  `errors[]` entry names the mutant.
- A baseline failure makes the completion `partial` (`baseline-failure`).

## Runner notes

`runner.notes` lists facts of the runner setup that limit the result. The key
is absent when the runner states no fact.

- `typecheck-not-run`: the project config enables the Vitest type-check pass
  (`typecheck.enabled`). The run does not execute it, so a mutant that only a
  type test kills is not counted as killed. The human output prints one line
  for this note.

`runner` is absent when no companion started, for example in a project with
no test files.

## Result cache

A run stores the result of each `killed` mutant and each `survived` mutant
that a retry confirmed. The file is `<cache dir>/mutation/results.json`. A
later run uses a stored result when the key is equal, and does not run that
mutant. The dry run always runs, because it gives the covering tests.

The key hashes these parts:

- the key format and the Fallow version;
- the runner version and the runtime version;
- the mutant id, and the content of the mutated source file;
- the covering test ids;
- the content of the covering test files, and of the files that they reach
  through static imports in the module graph (`MutationPlan::test_reach`).
  The run computes one digest for each test file and its reach, and each
  mutant key combines the digests of its covering test files;
- the content of the runner setup files (Vitest `setupFiles`) and of the
  files that they reach (`MutationPlan::support_files`);
- the lockfiles of the first directory from the root upward that has one;
- the runner config files at the root: `vitest.config.*`, `vite.config.*`,
  `vitest.workspace.*`, `package.json` and `tsconfig.json`. An absent file is
  also a key fact, so a new config file changes the key.

The reach ignores `vi.mock`, so a replaced module is still in the key. This
gives more misses, never a stale hit.

Known limit: dynamic imports that the module graph does not resolve, and files
that a test reads, are not in the key. A change to them does not invalidate a
stored result. Each cache entry records this limit in `not_keyed`, and the
report repeats it in `cache.not_keyed`.

- A `timeout`, `flaky`, `runtime_error` or `pending` result is never stored.
  A timeout depends on the load of the machine. A survivor that the time
  budget left without a retry is not stored.
- A reused result has `cached: true` in `mutants[]`. It counts in the score
  and the coverage the same as a result of this run.
- `cache` in the report gives `hits`, `stored` and `not_keyed`. It is absent
  with `--no-cache` and when no companion started.
- The human output prints one `Cache:` line when a result came from the cache.
- The global `--no-cache` flag turns the result cache off. The run then reads
  and writes no entry.
- The file keeps one entry for each mutant id, and at most 20000 entries. It
  drops the entries that no recent run stored or used. A file that does not
  parse, or that has another format version, gives an empty cache.
- A failed write of the file adds a `cache-write-failed` error and keeps the
  run result.
- The scheduler talks to the cache through the `ResultLookup` trait in
  `crates/api/src/mutation/scheduler`. The key and the file live in
  `crates/api/src/mutation/result_cache`.

## Type check of survivors

`--typecheck` (or `mutation.typecheck: true`) sends each `survived` mutant to
the TypeScript-Go companion of `--type-aware` after the tests. Other mutants
are not checked, so the cost grows with the number of survivors only.

- The companion applies one mutant at a time to an in-memory copy of its file.
  It never writes a project file.
- It compares the error diagnostics of the mutated file with the diagnostics
  of the original file. The comparison uses the diagnostic code and message,
  because the positions move. An error that the original file already has
  does not count.
- A survivor with a new error gets the status `compile_error`, and
  `type_error_codes` lists the codes (at most 8). The mutant does not count in
  the score, and it counts as covered.
- The check reads only the mutated file. A mutant that breaks only another
  file, for example a caller of a function with an inferred return type,
  stays `survived`. The check also does not run Vitest type tests, so the
  `typecheck-not-run` note stays.
- A survivor that the checker cannot check stays `survived`. The reasons are
  `no-project` (no `tsconfig.json` includes the file), `not-type-checked`
  (`// @ts-nocheck`, or a JavaScript file without `checkJs`),
  `source-changed` and `file-missing`. `typecheck.skipped` counts them.
- The run looks for the companion before the tests start. When no trusted
  companion exists, the run exits with code 2 and
  `FALLOW_MUTATION_TYPECHECK_UNAVAILABLE`. Discovery is the same as for
  `--type-aware`: `FALLOW_TYPE_AWARE_BIN`, the npm wrapper, or a sibling of
  the Fallow executable.
- When the companion fails after the tests, the survivors keep the status
  `survived`. The report has `typecheck.completed: false`, an error with the
  code `typecheck-failed`, and the completion reason `typecheck-failed`.
- `typecheck` is absent from the report when the option is off. A run
  without the option never sets `compile_error`.
- `--plan --typecheck` exits with code 2, because a plan has no survivors.
  The config key has no effect on a plan.
- The protocol operation is `mutant-typecheck` in
  `crates/api/type-aware-protocol.json`. It is a one-shot request; a semantic
  session refuses it.

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
  `equivalent`. A verdict for a killed, timed-out or `compile_error` mutant is
  `contradicted`, and the status stays. A verdict for a `pending`, `runtime_error` or `pruned`
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
config can move the cache directory. Without `--root` and `--config`, a
search from a subdirectory walks up to the git worktree root, so it finds the
report of a run at the project root and reads the files of that root. When no run saved a report, inspect exits
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

## Health join

`fallow health --mutation-report <file>` reads a saved run report and adds
the mutation score of each complexity finding next to its CRAP score. It
reads files only. It runs no test.

- The file is the saved report (`<cache dir>/mutation/last-report.json`) or
  the `-o` file of a run. A plan has no results, so health refuses it with
  exit code 2 and `FALLOW_MUTATION_REPORT_INVALID`. A missing or invalid file
  gets the same exit code and code.
- A finding matches one entry of `functions[]` when the path, the first line
  and the name are equal. Two entries with the same key make the match
  ambiguous, and the finding gets no evidence.
- The finding gets `mutation.state`. `fresh` means the file has the SHA-256
  digest of the report. Then `mutation.counts` has the counts of the function
  and `mutation.score` has its score. The score is absent when no mutant of
  the function ran to a tested result.
- `stale` means the file changed (`source-changed`), is missing
  (`file-missing`) or has no digest in the report (`no-digest`). A stale
  finding has `stale_reason` and no score and no counts, so a changed
  function never shows an old value.
- The top-level `mutation` object has the completion status of the run, the
  number of function results, the number of fresh, stale and unmatched
  findings, and the stale files. `sections` then has `mutation`.
- The join runs after the health report is complete. The mutation score
  never changes `crap`, the health score, the vital signs or a gate.
- Only complexity findings get a score. A function under each threshold has
  no finding, so its score stays in the mutation report. The findings of
  each `--group-by` group get the same evidence.
- The human output prints one line under the CRAP line of each matched
  finding, and one note line with the matched and stale counts. The other
  output formats (SARIF, CodeClimate, markdown, compact) do not show it.
- The score uses the final statuses after the retries (see Retries).

## Human output

- The output uses the section headers and colors of the other commands.
  `NO_COLOR`, a pipe and `-o` remove the colors.
- The header says `(ran the project tests)` only when a companion started or a
  mutant has a tested result. Otherwise it says `(no test ran)`.
- The score line: the score, killed, survived, timeout, and the completion
  status. The line adds `pending`, `equivalent`, `runtime error`, `flaky` and
  `compile error` only when the count is not zero.
- The scope line shows a full 40-character base SHA as its first 12
  characters, as `audit` does. No other line repeats the base ref.
- The coverage line: the coverage and the no-coverage count.
- One line when `runner.notes` has `typecheck-not-run`: type-level kills are
  not counted. When the survivor type check ran, the line says that only type
  errors in the mutated file are counted.
- One line for the survivor type check: the checked count, the count that
  does not type-check, and the skipped count by reason. A failed check says
  that the survivors keep the status `survived`.
- One `Cache:` line when a result came from the result cache. It names
  `--no-cache`.
- The 10 best-ranked survivors grouped by file, files and mutants in rank
  order, and one `and N more survivors` line. Each survivor has the location,
  the operator, the original and mutated code, and one line with the count of
  covering tests and the first test name, cut to 60 characters. A long code
  line shows a window around the changed token, the text that differs
  between the original and the replacement. Both sides use the same window
  start. The JSON report lists all survivors and all covering tests.
- The `inspect` output shows the first 10 covering tests under a
  `Covering tests (N)` header, or the first 10 static test files under a
  `Test files that reach the file (N)` header, and one `and N more` line. The
  `--format json` packet lists all of them.
- The plan lists the first 20 planned mutants under a `Planned (N)` header,
  in run order, and one `and N more mutants` line.
- The minimum-score line uses the status prefixes of the other commands:
  `[OK]` when the score meets the minimum, `[X]` when it misses the minimum,
  and `[-]` when no score is measured.
- One line for the baseline failures. It names the failing test when there
  is one, and the count and the first test when there are more.
- Each `Error` line names its mutant when the error belongs to one mutant.
- A separate no-coverage section with one line for each file and its count.
  The line adds the counts by test reach class, for example
  `test reach: 2 mocked-only, 1 none-file`, in the class order of the
  `audit` test reach. The section shows the 10 files with the most
  mutants and one `and N more files` line. The JSON report has the full list.
- The plan prints one `No coverage by test reach` line with the same counts
  when a mutant is `no_coverage` and the project has test files.
- The pruned counts by reason, and one line for "no tests" or "no mutation
  data", never one line for each function. A project with no test files gets
  the one "no tests" line with the mutant count, and no no-coverage section.
  An empty changed-code scope names `fallow mutation --plan --all`, which
  lists the mutants of each file and runs no code.
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
    "concurrency": 4,
    "typecheck": true
  }
}
```

- `runner` is `auto` or `vitest`. Version 1 runs Vitest 4 and 5.
- `operators`, `minScore` and `concurrency` apply to each run. A CLI flag wins.
- `typecheck: true` turns on the survivor type check for each run.
  `--typecheck` turns it on for one run. No flag turns it off.
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
| Runner protocol, companion, scheduler, report, verdict join, survivor type check | `crates/api/src/mutation` |
| Type check request and response | `crates/api/src/type_aware/mutant_typecheck.rs` |
| Type check in the companion | `tools/type-aware-sidecar/src/mutant-typecheck.mjs` |
| Envelopes | `crates/output/src/mutation.rs` |
| Config section | `crates/config/src/config/mutation.rs` |
| CLI adapter, human output, inspect | `crates/cli/src/mutation_cli.rs`, `mutation_human.rs`, `mutation_inspect.rs` |
| Health join | `crates/cli/src/health/mutation_report.rs`, `crates/output/src/health_mutation.rs` |
| MCP plan and inspect tools | `crates/mcp/src/tools/mutation.rs` |
| Node companion | `tools/mutation-runner` |

## Tests

- `cargo test -p fallow-cli mutation` runs the CLI unit tests and the
  integration tests in `crates/cli/tests/integration/mutation_tests.rs`.
- The integration tests use a project with no test files for the exit policy,
  the verdict join and inspect, so they need no JavaScript runtime.
- The Vitest end-to-end test needs `node` on `PATH` and `npm ci` in
  `tools/mutation-runner`. Without them it prints a skip message and passes.
  The type check end-to-end test also needs `npm ci` in
  `tools/type-aware-sidecar`.
- `node --test test/mutant-typecheck.test.mjs` in `tools/type-aware-sidecar`
  tests the companion side of the type check.
