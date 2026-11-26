# `mutation --kill-matrix`: Redundant test evidence

```bash
fallow mutation --all --kill-matrix --format json --quiet
```

This opt-in mode runs all covering tests with bail disabled. It can increase execution time.
Read the matrix evidence status before you review candidates. Normal mode reports `kill-matrix-disabled` and no candidates.
Incomplete evidence also produces no candidates.

`kills` and `unique_kills` are arrays of mutant IDs.
Individual subsumers cover a candidate's full kill set. Other tests can also cover that set collectively.
Review each candidate separately, including equal kill sets. Removing equal candidates together can remove their shared protection.
A candidate does not prove safe removal. Baseline-failing tests cannot become candidates or subsumers.

Tests with runtime coverage and no observed kills appear separately. Zero kills do not prove that a test has no assertions.
Sole-file notes use observed instrumented-file coverage. Public-export notes use static test-file reach, without per-test export coverage claims.

The `mutation` and `mutation-inspect` JSON envelopes use schema version `"2"`. The `mutation-plan` envelope and verdict document keep schema version `"1"`.

The `kill_matrix` object has these fields:

| Field | Meaning |
|---|---|
| `status` | `complete`, `incomplete`, or `unavailable`, separate from mutation completion. |
| `reasons` | Evidence limitations, such as `kill-matrix-disabled`, `incomplete-mutant-results`, `baseline-failure`, `partial-mutation-run`, `missing-test-file-metadata`, or `unmatched-target`. |
| `tests` | Entries with `test_id`, `file`, `kills`, `unique_kills`, `subsuming_tests`, and `notes`. |
| `candidates` | Test IDs whose observed kill sets other tests cover. |
| `coverage_without_kills` | Test IDs with runtime coverage and no observed kills. |

Each mutant and attempt records `killing_tests` and `kill_matrix_complete`. The existing `killed_by` field keeps the first observed killer. Outside matrix mode, `killing_tests` is empty.

Matrix mode does not add the other tests of the covering test files to the retry of a survivor. A survivor that normal mode kills in that retry can stay `survived` in matrix mode, so the two modes can give different scores.

## `mutation --kill-matrix-for`: Check named tests

```bash
fallow mutation src/math.ts --kill-matrix-for "test/math.test.ts::adds" --format json --quiet
```

Use this mode to check whether specific tests are redundant without a full kill matrix. Repeat the flag for each target. A target is a test file (each test in it) or a test ID (`file::name`). It is exclusive with `--kill-matrix`.

- Only the mutants that a target test covers run. The other mutants are `pruned` with `prune_reason: "no-target-coverage"`.
- Each mutant runs its target tests with bail disabled. When no target kills it, the mutant is `not_killed_by_target`: no other test runs. After a target kill, the other covering tests run fastest first with bail enabled, up to the first failure. Only a kill by exactly one target test and no other test gets a retry. A retry that does not kill again with the same tests makes the mutant `flaky` and the target `incomplete`.
- This `target-first` mode has no score: `summary.score` is `null`, and `--min-score` exits `2`.
- `--kill-matrix-full` also runs the other covering tests of a mutant that no target kills, and retries each survivor, as in normal mode. Each mutant then gets a normal status, and the run has a score. `kill_matrix.target_mode` is `target-first` or `full`. For deterministic tests, both modes give the same `kills`, `unique_kills` and `target_only_kills` for each target.
- `kill_matrix.targets` lists each target with the `test_ids` that it selects. A target that selects no test is `incomplete` with the reason `unmatched-target`, and the run goes on. The reason `not-a-test-file` means that the runner does not list the file. The reason `no-planned-code` means that the file reaches no planned code, for example a test that runs the code only in a child process. When no target selects a test, the run exits `2` with `FALLOW_MUTATION_TARGET_UNMATCHED`. When the dry run runs the file of no target, the run stops before the dry run. A test ID target that names a duplicate test ID (two tests of one file with the same title) exits `2` with `FALLOW_MUTATION_TARGET_DUPLICATE_TEST_ID`. A file target drops the duplicate IDs, and the `duplicate-test-ids` entry of `errors` names them.
- Each target has its own `status` (`complete` or `incomplete`) and `reasons`. Only the tests of a complete target can be candidates. `candidates` can be nonempty while the top-level `status` is `incomplete`.
- `kill_matrix.tests` has rows for target tests only. `unique_kills` are kills that no other test detects, target or not. `target_only_kills` lists kills that only other targets share, each with those `targets`. `subsuming_tests` is empty (note `subsumers-not-computed`).
- A target with kills and no `unique_kills` is a candidate. With `target_only_kills`, it stays redundant only while one of the sharing targets stays. Do not remove all sharing targets together.
- The evidence covers the planned scope only. A unique kill outside the scope is not visible.

## `mutation inspect --replay`: Find a test input for a survivor

```bash
fallow mutation inspect <mutant-id> --replay --args '[2, 3]' --format json --quiet
```

The replay calls the export that holds the mutated function with `--args` on the original and on the mutant, three times each, in turns. It runs project code, but no test. Read `replay.verdict`:

- `witness`: each side repeats its result, and the sides differ. Read `calls[]` of the `original` side. When its `outcome` is `returned`, write a test with this input that asserts that `value`. When its `outcome` is `threw`, assert that the call throws (`value.name`, `value.message`).
- `no_witness_found`: this input shows no difference. Try another input. This verdict never means that the mutant is equivalent.
- `unstable`: one side changes between calls (for example `crypto.randomUUID()`). Do not use the difference as a test oracle.
- `timeout`, `not_callable`, `not_replayable`: read `replay.reason`. A function that no callable export holds (a method of a class or of an object literal, or a local function) needs `--export <name>` of an export that calls it. `--call-timeout 30s` gives a slow call more time.

Without `--args`, the replay uses up to 3 inputs that `fallow mutation --record-args values` recorded (`replay.input.source` is `recorded`, with its `input_id`; `replay.inputs_checked` counts the inputs that ran). The values stay in the local cache, and the inspect output shows them, so keep that output local. The run report shows only counts and reasons under `recording`.

Each call runs in a new worker with a fresh module, a clock that starts at a fixed time, and a fixed `Math.random` seed. Arguments and results use the replay encoding: JSON, plus objects with a `$replay` tag for values that JSON cannot hold. Decode them when you write the test: `{"$replay": "undefined"}` is `undefined`, `{"$replay": "number", "value": "NaN"}` is `NaN`, `{"$replay": "bigint", "value": "12"}` is `12n`, and `map`, `set`, `date`, `regexp` and `bytes` are a `Map`, a `Set`, a `Date`, a `RegExp` and a typed array. Only the Vitest and `node:test` runners replay.
