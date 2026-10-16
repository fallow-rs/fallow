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

## Source files

- JavaScript and TypeScript files (`.js`, `.jsx`, `.mjs`, `.cjs`, `.ts`,
  `.tsx`, `.mts`, `.cts`) are one program each.
- In a Vue or Svelte component and in an Astro component, only the script
  blocks get mutants: the Vue `<script>` and `<script setup>` blocks, the
  Svelte instance and module scripts, and the Astro frontmatter. A block with
  a `src` attribute has no inline code. The extract crate finds the blocks
  (`fallow_extract::sfc::extract_sfc_scripts`,
  `fallow_extract::astro::extract_astro_frontmatter`), so the plan uses the
  same blocks as the analysis.
- The instrumented copy of a component is the whole file with each script
  block replaced. The template and the styles stay the same, so the framework
  plugin of the project compiles the copy. The companion serves a component
  copy in the Vite `load` hook, so a framework plugin with its own `pre`
  `transform` hook also compiles the copy. A request with a query (a style or
  a template block) is not served: the framework plugin makes it from the
  component that it compiled.
- Spans, lines, columns and diff ranges use the coordinates of the whole
  file. A mutant id depends on the text of its function, not on the offset of
  the block, so an edit of the template keeps the ids.
- The prelude goes into each block with module scope (the Vue `<script>`, the
  Svelte module script). When a component has no such block, each block gets
  its own prelude.
- The framework compiler moves some code out of the block scope, where the
  prelude is not visible or where a switch is not valid. These positions give
  no mutant:
  - a call of `defineProps`, `defineEmits`, `defineExpose`, `defineModel`,
    `defineOptions`, `defineSlots` or `withDefaults` in a Vue
    `<script setup>` block, its arguments, and the destructure defaults of a
    declaration such as `const { size = 1 } = defineProps()`. A
    `removed-call` switch around `defineExpose()` hides the macro from the
    compiler, and the component then throws `defineExpose is not defined`;
  - an expression that holds a Svelte rune call (`$state`, `$derived`,
    `$effect`, `$props`, `$bindable`, `$inspect`, `$host`). The rune
    arguments still get mutants;
  - an exported declaration in the Astro frontmatter.
- Svelte module files (`.svelte.js`, `.svelte.ts`) can use runes. They are one
  program each with the Svelte rune rule.
- The script blocks of a component parse as one program for the side-effect
  and framework-config rules. A sink in a component template does not run when
  the module loads, so it does not prune the top-level mutants.

## Operators

| Operator | Change |
|---|---|
| `arithmetic-operator` | `+` and `-` swap, `*` and `/` swap, `%` becomes `*`, `+=` and `-=` swap |
| `relational-operator` | the non-redundant relational subset |
| `logical-operator` | `&&` and `\|\|` swap, `??` becomes `&&` |
| `negate-condition` | a plain condition becomes `true` and `false` |
| `unary-operator` | `!x` and `-x` become `x` |
| `update-operator` | `++` and `--` swap |
| `removed-call` | a call statement becomes a no-op |
| `return-value` | `return expr` becomes `return undefined` |
| `boolean-literal` | `true` and `false` swap |
| `string-literal` | a string becomes empty, an empty string becomes `"fallow"` |
| `optional-chaining` | `a?.b` becomes `a.b` |
| `regex-literal` | one pattern token changes |

The `regex-literal` operator gives one mutant for each of these tokens of a
regular expression literal:

- a `^` at the start of the pattern is removed;
- a `$` at the end of the pattern is removed;
- a character class is negated (`[a-z]` to `[^a-z]`, and the reverse). A
  pattern with the `v` flag gives no negation;
- a class escape gets the opposite case (`\d` to `\D`, and the same for `\w`
  and `\s`).

The flags stay the same, and an empty pattern becomes `(?:)`. The corpus for
this operator is the JavaScript and TypeScript source files of six public
projects (date-fns, zod, vue core, SvelteKit, reka-ui, runed), without tests.
Each of its 7971 regex mutants compiled with `new RegExp`. Most literals give
one mutant. Locale data with many patterns gives many: date-fns has 6723 regex
mutants, 6555 of them in its locale files.

No absolute-value operator (`x` to `-x`) is planned. Without types, it applies
to each operand of an arithmetic or relational expression. The same corpus has
114119 mutants, and the operator adds 6473 mutants (5.7 %), 24 % of them on
numeric literals. Each one is on an expression that already has an arithmetic
or relational mutant, so it adds little test-strength signal for its run time.

## Safety boundary

- Only `fallow mutation` without `--plan` runs code. Project config cannot
  start a run. The MCP server exposes the plan and inspect only.
- The run refuses a dirty git worktree unless `--allow-dirty` is set. After the
  run, it compares the worktree with the start state and reports each changed
  path.
- The check covers tracked and untracked paths. For ignored files, it covers
  the files that `git status --ignored=matching` lists at the start, up to
  10000 files in path order: a write to one of them or a delete is a change.
  Git lists an ignored directory such as `node_modules/` or `coverage/` as one
  entry, so the check does not see the files below it. A new ignored file is
  not a change, because test runs make such files (coverage, logs).
- The check leaves out `<cache>/mutation` and the output files of the run. For
  ignored files it also leaves out the whole cache directory, because the
  analysis writes its own cache files there. The run refuses a cache directory
  that is the project root or a directory above it, or that holds a planned
  source or test file (`FALLOW_MUTATION_UNSAFE_CACHE_DIR`, exit code `2`). Such
  a cache directory would hide the writes of the tests from the check.
- Mutants in a function whose own body calls a file system, process, network
  or database sink are pruned (`side-effect-sink`). A mutant in top-level code
  is pruned when the module calls a sink at the top level. The pruning does
  not follow calls: a mutant in a function that calls a helper, which then
  calls a sink, still runs. `--allow-side-effects` keeps the pruned mutants.
  This override is a flag only: the config rejects `allowSideEffects`, so a
  project config cannot widen what a run executes.
- Mutants in framework config are pruned (`framework-config`). Framework
  config is an export that an active plugin marks as used, for example
  `export const revalidate = 60` in a Next.js page. It is also the default
  export of a tool config file, which is a file that the `config_patterns`
  or the `always_used` globs of an active plugin match (`vite.config.ts`,
  `eslint.config.js`, `.eslintrc.js`, and the VitePress config file in
  `.vitepress`). Some plugins list their config file only as always used. Only a glob whose file name holds `config`, `.conf.` or an
  `rc` suffix counts, because some plugins also list application files as
  config, for example `codegen*.ts`, `src/module.ts` or `app/routes.ts`.
  The config and always-used globs of external plugins count too. In a tool config file, a
  top-level `module.exports = ...` statement counts as the default export. A mutant inside a function in such an export is not
  pruned, so an inline Vite plugin keeps its mutants. The rule lives in
  `framework_config_exports` (`crates/engine/src/mutation/mod.rs`) and
  `framework_regions` (`crates/engine/src/mutation/plan.rs`).
- Fallow does not write to project files. The instrumented files live in the
  Fallow cache. The report lists any path that the tests changed.
- The companions run in their own process group on Unix and in a Job Object on
  Windows. Fallow ends that group when a run ends, and when it receives
  SIGINT, SIGTERM, SIGHUP or SIGQUIT. When Fallow starts with SIGHUP ignored,
  as under `nohup`, it keeps SIGHUP ignored. The release binary aborts on a
  panic, and a panic hook ends the group first. A companion whose stdin closes
  without a `shutdown` request kills its own process group and exits, so it
  does not outlive a Fallow process that ended in another way.

## Exit codes

| Code | Meaning |
|---|---|
| `0` | The plan, the run or inspect finished. Survivors do not fail a run. |
| `1` | `--min-score` or `mutation.minScore` is set, and the score is lower. |
| `2` | Invalid input or an execution error: a dirty worktree without `--allow-dirty`, a cache directory that hides project files, a missing runtime or runner, a companion file with a wrong digest, an invalid verdict document, `--typecheck` without a type-aware companion. |

The flag wins over the config key. The minimum applies to the score, in
percent. A run with no executed mutant has no score and exits `0`, also when
each mutant is `no_coverage`.

## Mutant limit

`--max-mutants N` limits the run to the covered mutants. The plan applies
the limit in rank order before the dry run, so the plan cannot know the
coverage. The scheduler corrects this after the dry run:

- A planned mutant that a dry-run test covers keeps its place.
- A planned mutant that no dry-run test covers is `no_coverage` and does not
  use the limit.
- Each free place goes to the next covered `pending` mutant in rank order.
  That mutant runs and gets a normal status.
- The run executes `min(N, covered mutants)` mutants. The covered mutants
  after the filled limit stay `pending`.
- A statically `no_coverage` mutant that the dry run hits (a static map
  miss) runs outside the limit.
- The completion has `mutant-limit` only when a covered mutant stays
  `pending`. When the limit leaves only `no_coverage` mutants, the limit cut
  nothing.

`--plan` runs no dry run, so its `planned` and `pending` dispositions are a
view before the coverage is known. A run can execute a mutant that the plan
shows as `pending`.

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

### Measure the flaky rate

`scripts/mutation-flaky-rate.mjs` compares the saved reports of repeated runs
with the same scope:

```bash
fallow mutation --all --no-cache --root <project>
cp <project>/.fallow/mutation/last-report.json run-1.json
# Repeat for run-2.json and run-3.json, then:
node scripts/mutation-flaky-rate.mjs run-1.json run-2.json run-3.json
```

- `--no-cache` makes each run execute each mutant. A result from the result
  cache did not run: the script leaves it out of `compared`, and `cached`
  gives its count for each report.
- `compared` counts the mutants that each run executed.
- `unstable` counts the compared mutants with a different status between runs
  or a `flaky` status in one run.
- `score_flips` counts the unstable mutants that one run detected and another
  run did not. `timeout_flips` counts the score flips with a `timeout` in one
  run.
- Each count has a rate over `compared`.

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
- The dry run of a restarted runner process gets the larger of
  `--setup-timeout` and a limit that scales with the first dry run (see
  Baseline failures).
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
- The dry run of a restarted companion shares the host with the other
  workers. Its timeout is the first dry-run time times 2 times the worker
  count, and at least `--setup-timeout` (2 minutes by default). On a slow
  or loaded host, a fixed timeout made each mutant of a restarted companion a
  `runtime_error`. The time budget does not stop a dry run in progress, so a
  companion that hangs in its dry run can extend the run past the budget.
- A baseline failure makes the completion `partial` (`baseline-failure`).

## Runners

The companion has three runner adapters. The `mutation.runner` config key
selects one. The default `auto` reads the project in this order:

1. `vitest` in the root `package.json` dependencies selects Vitest.
2. Otherwise the test module that the most static test files import:
   `vitest`, `bun:test` or `node:test`. A tie goes to Vitest, then
   `bun test`.
3. Otherwise a `bunfig.toml`, `bun.lock` or `bun.lockb` at the root selects
   `bun test`.
4. Otherwise a `node --test` or `borp` command in a `package.json` script
   selects `node:test`.
5. Otherwise Vitest.

| Runner | Config value | Versions | Runtime of the companion |
|---|---|---|---|
| Vitest | `vitest` | 4.1 to 5 | `node`, then `bun` |
| `node:test` | `node-test` | Node.js 22.18 to 26 | `node` |
| `bun test` | `bun-test` | Bun 1.3 and later 1.x releases | `bun` |

The companion refuses a runner version outside this range. The run exits
with code 2 and `FALLOW_MUTATION_RUNNER_UNSUPPORTED`. The message names the
installed version and the supported range. The help gives the install step
for the range. When the minimum version is not a `.0.0` release, the help
also says that a release of that major version is enough. For Vitest, the
help is:

```text
install vitest 4.1.0 or a later version below 6.0.0; a vitest 4.x release from 4.1.0 is in the range
```

Vitest 4.0 is not supported. It reads the test-id filter of a test
specification as a list of test lines. Each mutant run then fails with an
error, and each mutant looks killed. Vitest 4.1 added the test-id filter.

`borp` is a test runner that runs `node:test`. The script parser reads the
test files of a `borp` command in the order of `borp` 1.0:

1. The positional arguments. A glob that starts with `!` excludes files.
2. Otherwise the glob of `-p` or `--pattern`.
3. Otherwise the default glob `**/*.test.{js,mjs,cjs,ts,mts,cts}`.

These files are test entry points, so the plan finds the static test files
of the project. The parser does not apply a `!` exclusion glob: a file that
the exclusion removes stays a test entry point when another glob matches it.
The parser does not read a `.borp.yaml` file. A project that sets its test
files in `.borp.yaml` and runs a bare `borp` command gets the default glob.

`--runtime` must name a runtime that the runner accepts. A Bun runtime for
`node-test`, or a Node runtime for `bun-test`, exits with code 2 and
`FALLOW_MUTATION_RUNTIME_NOT_FOUND`.

### Test environment

Each adapter gives the tests the environment variables that the native
runner command sets. Code that reads `NODE_ENV` then behaves as in a normal
test run.

| Runner | Variables | Source |
|---|---|---|
| Vitest | `TEST=true`, `VITEST=true`, `NODE_ENV=test` when it is not set | The adapter sets them as the `vitest` command does |
| `node:test` | None | `node --test` sets no `NODE_ENV` |
| `bun test` | `NODE_ENV=test` when it is not set | `bun test` sets it in each test process |

- The Vitest adapter calls `createVitest` directly and not the `vitest`
  command. Without these variables, Vite sets `NODE_ENV=development`, and
  the test workers inherit it.
- A `NODE_ENV` value of the environment wins, the same as with the Vitest
  CLI. For example, `NODE_ENV=production fallow mutation` runs the tests
  under `NODE_ENV=production`.
- Internal protocol detail: the companion applies the `options.env` values
  of the init request after these variables, so they win. The API sends an
  empty `options.env`.

### `node:test` and `bun test`

- Each dry run and each mutant run starts one test process for each test
  file. The `node:test` adapter uses `run()` of `node:test` with process
  isolation. The `bun test` adapter starts `bun test` with a JUnit report.
- A preload file serves the instrumented files. `node:test` uses a
  synchronous module hook (`module.registerHooks`), so `import` and
  `require` both get the instrumented file. Node strips the types of a
  TypeScript file. `bun test` uses a `Bun.plugin` load hook.
- The `node:test` adapter gives each test process the module loading flags
  of the project test script. The API reads the `package.json` scripts at
  the root: the `test` script first, then the other scripts in name order.
  The first `node ... --test` command gives the flags
  (`crates/api/src/mutation/node_args.rs`). The init request carries them
  in `options.node_args`, each flag with a value as two entries.
- These flags pass: `--experimental-transform-types`,
  `--experimental-strip-types`, `--no-experimental-strip-types`,
  `--import`, `--require` (`-r`), `--experimental-loader` (`--loader`) and
  `--conditions` (`-C`). Other flags of the command are ignored. A
  relative `--import`, `--require` or loader value resolves against the
  project root. The preload of the adapter comes before these flags, so a
  loader of the project wraps the hook of the preload.
- The detection reads only a command whose program is `node`. A wrapper
  command, for example `cross-env X=1 node --test` or `tsx --test`, gives
  no flags. `npm run` indirection needs no special step, because the API
  reads every script.
- `NODE_OPTIONS` of the `fallow mutation` process also reaches each test
  process. Use it for a flag that the script detection does not read, for
  example when the baseline fails with `ERR_UNSUPPORTED_TYPESCRIPT_SYNTAX`.
- A test file that does not load (for example a TypeScript `enum` without
  `--experimental-transform-types`) is a baseline failure. Its message
  starts with the error line of the test process stderr, for example
  `SyntaxError [ERR_UNSUPPORTED_TYPESCRIPT_SYNTAX]: TypeScript enum is not
  supported in strip-only mode`. The last lines of the stderr follow.
- The test files are the files that the default patterns of the runner
  match, without `node_modules` and `.git`. A static test file that the
  patterns do not match is not run, and the report says so.
- A test id is `<file>::<suite> > <test>`, as for Vitest.
- `node:test` reports the hits of a test in a test diagnostic. Bun hooks get
  no test context, so the `bun test` adapter joins the hits to the JUnit
  results by order. When the order is not safe, each test that ran in the
  file gets the hits of the file: tests that overlap (`concurrency` in
  `node:test`, `test.concurrent` in Bun), a count that differs, or
  `randomize = true` in `bunfig.toml`. The scheduler then runs a mutant
  against each test of the file that can kill it.
- A run selects the requested tests with a name pattern. Each whitespace
  run in the pattern matches any whitespace, because a Node test process
  matches a name without its trailing whitespace.
- With `bail`, a run stops the test process at the first failure. Node 22
  applies the run timeout to the whole test file process. A requested test
  with no result after that timeout counts as a timeout.
- Bun 1.3 gives each failed test the JUnit type `AssertionError`, so the
  `failure_kind` of a run on Bun 1.3 is `assertion`, also for a runtime
  error. The status does not change.
- The in-process mode of `node:test` (`isolation: "none"`) is not
  supported.
- Neither runtime can load a Vue, Svelte or Astro component file. The
  `node:test` hook calls the default loader first, and that loader refuses
  the file type. The Bun hook serves the copy with a TypeScript loader, and
  a project plugin that serves the file first serves the original. So the
  plan leaves each component file out, with the skip reason
  `runner-cannot-load` in `skipped_files`. Its mutants are not in the plan
  and not in the coverage.
- The runner decision reads the static test files of the plan. When the
  first plan has a component mutant and the runner cannot load components,
  the plan is built again from the same analysis without the component
  files (`plan_mutations_for_session_with`). The run keeps the runner of
  the first decision. `--plan` uses the same rule, so the plan and the run
  agree.

### Type tests

A type test (`*.test-d.ts`, `*.spec-d.ts`, or a TypeScript file below a
`test-d` directory) is not a test file of the plan. A compiler checks it (`tsd`,
`expect-type`, the Vitest type check), and no runner executes it. Before this
rule, a `tsd` file was the only static test file of some projects. The dry run
then ran it as a runtime test, it failed as a baseline failure, and runner
detection read its imports. A project whose only test entry points are type
tests has `no_test_files: true`. A file that only type tests reach has no
coverage, and its mutants get the test reach `none-file`.

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
- the runner name, the runner version and the runtime version;
- the mutant id, and the content of the mutated source file;
- the covering test ids;
- the content of the covering test files, and of the files that they reach
  through static imports in the module graph (`MutationPlan::test_file_reach`).
  The run computes one digest for each test file and its reach, and each
  mutant key combines the digests of its covering test files;
- the content of the runner setup files (Vitest `setupFiles`, the `preload`
  files of `bunfig.toml`), of the runner config modules at the root
  (`vitest.config.*`, `vite.config.*` and `vitest.workspace.*`), and of the
  files that they reach through static imports (`MutationPlan::support_files`).
  A change to a config fragment that the runner config imports changes the
  key;
- the lockfiles of the first directory from the root upward that has one;
- the runner config files at the root: `vitest.config.*`, `vite.config.*`,
  `vitest.workspace.*`, `bunfig.toml`, `package.json` and `tsconfig.json`. An
  absent file is also a key fact, so a new config file changes the key;
- the configs of the `extends` chain of the root `tsconfig.json`, at most 32
  files. A relative `extends` resolves to the path, or to the path with
  `.json`. A package `extends` resolves in each `node_modules` directory from
  the config upward, to the path, the path with `.json`, or `tsconfig.json`
  in the path. The `exports` map of a package is not read. An `extends` that
  does not resolve adds only its text to the key.

The reach ignores `vi.mock`, so a replaced module is still in the key. This
gives more misses, never a stale hit.

Known limit: dynamic imports that the module graph does not resolve, and files
that a test or a runner config reads, are not in the key. Runner configs and
tsconfig files below the root, for example in a workspace package, are not in
the key. A change to them does not invalidate a stored result. Each cache entry records this limit in `not_keyed`, and the
report repeats it in `cache.not_keyed`.

- A `timeout`, `flaky`, `runtime_error` or `pending` result is never stored.
  A timeout depends on the load of the machine. A survivor that the time
  budget left without a retry is not stored. A survivor of a run without a
  dropped covering test (see Baseline failures) is not stored, because the
  key covers that test.
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

## Run directory

The run writes the instrumented files, `paths.json` and the runtime hooks to
`<cache dir>/mutation/runs/<content hash>/`. The hash covers each planned
path and its instrumented text, so two runs of the same plan share one
directory.

- The write goes to a `.staging-*` directory below `runs/`, which the run
  renames into place. A crash or a concurrent run never leaves a partial
  directory, and a run that reads the directory never sees a partial file.
- A directory whose files have the expected content is used again without a
  write. A directory with other content is replaced in one rename.
- Each run records its use in a `.last-used` marker file. At the start of a
  run, Fallow removes the run directories that are not among the 5 most
  recently used ones and that no run used in the last hour. The hour keeps
  the directory of a concurrent run. The run never removes its own
  directory. A `.staging-*` directory older than one hour, which a crash can
  leave, is also removed.

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
  `source-changed`, `file-missing` and `unsupported-file` (a Vue, Svelte or
  Astro component). `typecheck.skipped` counts them.
- The run does not send a component survivor to the companion. The
  TypeScript checker cannot read a component file, so the run gives it the
  reason `unsupported-file` directly.
- The run looks for the companion before the tests start. When no trusted
  companion exists, the run exits with code 2 and
  `FALLOW_MUTATION_TYPECHECK_UNAVAILABLE`. Discovery is the same as for
  `--type-aware`: `FALLOW_TYPE_AWARE_BIN`, the npm wrapper, or a sibling of
  the Fallow executable.
- When the companion fails after the tests, the survivors keep the status
  `survived`. The report has `typecheck.completed: false`, an error with the
  code `typecheck-failed`, and the completion reason `typecheck-failed`.
  `typecheck.skipped` still counts the component survivors, because they
  never go to the companion.
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
- One `Skipped files:` line with the count of each skip reason, in the run
  and in the plan. The JSON report lists each path. A `runner-cannot-load` skip adds
  one line that names `mutation.runner` and Vitest as the next step.
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

- `runner` is `auto`, `vitest`, `node-test` or `bun-test`. See
  [Runners](#runners).
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
| Component script blocks | `crates/mutation/src/blocks.rs`, `crates/engine/src/mutation/layout.rs` |
| Selection, test map, prune and rank | `crates/engine/src/mutation` |
| Runner protocol, companion, scheduler, report, verdict join, survivor type check | `crates/api/src/mutation` |
| Type check request and response | `crates/api/src/type_aware/mutant_typecheck.rs` |
| Type check in the companion | `tools/type-aware-sidecar/src/mutant-typecheck.mjs` |
| Envelopes | `crates/output/src/mutation.rs` |
| Config section | `crates/config/src/config/mutation.rs` |
| CLI adapter, human output, inspect | `crates/cli/src/mutation_cli.rs`, `mutation_human.rs`, `mutation_inspect.rs` |
| Health join | `crates/cli/src/health/mutation_report.rs`, `crates/output/src/health_mutation.rs` |
| MCP plan and inspect tools | `crates/mcp/src/tools/mutation.rs` |
| Node companion and runner adapters | `tools/mutation-runner` |
| Runner selection | `crates/api/src/mutation/runner_select.rs` |

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
- The `node:test` end-to-end test needs a supported `node` on `PATH`, and
  the `bun test` end-to-end test needs a supported `bun`. Without it, the
  test prints a skip message and passes.
- `npm test` in `tools/mutation-runner` runs the companion suites. The
  `bun test` suite is skipped when `bun` is not on `PATH`.
