# Quality gates

Use this before large changes, reviews, commits, and pushes.

## One-time local setup

```bash
mise install     # Optional: pinned Node.js, pnpm, hk, pkl and cargo tools
pnpm bootstrap   # Install the git hooks and all dependencies
```

`pnpm bootstrap` runs `scripts/bootstrap.mjs`. The script uses only Node
built-ins, so it runs before any install. It checks for Node.js 22.13.0 or
later, pnpm and hk, and prints one install hint for each missing tool. It
removes a `core.hooksPath=.githooks` setting from the repository config of an
old clone, and warns about any other `core.hooksPath` value. Before git 2.54,
hk installs hook files that git ignores while `core.hooksPath` is set, so the
warning then tells you that the hk hooks do not run. Then it installs the hk hooks and runs
`pnpm install --frozen-lockfile` at the root and in `editors/vscode`. You can
run it again at any time.

### Git hooks with hk

[hk](https://hk.jdx.dev) runs the git hooks. The root `hk.pkl` defines the
`pre-commit`, `commit-msg` and `pre-push` hooks. hk evaluates `hk.pkl` with the
`pkl` CLI, so install both. `pnpm bootstrap` then runs `hk install --mise`
when mise is on `PATH`, and `hk install` when it is not.

Without mise, install `hk` and `pkl` at the versions in `mise.toml`.
`--mise` starts the hooks through `mise x`, so a shell or git
client without mise activation still finds hk. `HK=0 git commit` skips the
hooks for one command. `HK_SKIP_STEPS=<step> git commit` skips only the named
step, for example `HK_SKIP_STEPS=file-size`.

hk runs the steps of a hook in parallel. A pre-commit step with a `glob` runs
only when a staged file matches it, so a commit without Rust inputs skips
`cargo fmt`, Clippy and the Miri cfg check. hk does not match deleted files, so
a commit that only deletes a Rust file also skips them. The pre-push hook runs
`cargo fmt` and Clippy with no glob and catches that case. The pre-commit hook only checks.
Run `hk fix` to apply the fix commands, for example `cargo fmt --all`. The
hooks use `stash = "none"`, because every worktree of this repository shares
one git stash stack.

Run one hook by hand with `hk run pre-commit` or `hk run pre-push`. Add
`--plan` to see which steps run and why.

### Pinned tools with mise (optional)

The root `mise.toml` pins the local tools that the hooks and quality gates
call: hk, pkl, Node.js, pnpm, `typos`, `cargo-fallow`, `cargo-deny`, `cargo-audit`,
`cargo-nextest`, `cargo-llvm-cov` and `cargo-insta`. With
[mise](https://mise.jdx.dev) installed, run `mise install` to get the pinned
set. mise downloads prebuilt binaries, so the install does not compile tools.

mise is optional. Without mise, install each tool by hand at the version in
`mise.toml`. When a hook does not find a tool, it prints one hint line with the
pinned version and skips that check. `rust-toolchain.toml` owns the Rust
version through rustup, so `mise.toml` does not pin Rust.

Where CI pins a tool version, `mise.toml` uses the same version.
`scripts/workflow-policy.test.mjs` fails when the CI Node.js version or a
`tool: name@version` pin changes and `mise.toml` does not follow. The test
does not check typos, cargo-deny and cargo-fallow, because their CI actions
fix the tool version in the action commit. A Dependabot action bump then does
not fail. Update those three versions by hand. The same test fails when the pnpm version
in `mise.toml` is not the lower bound of `engines.pnpm` in the root
`package.json`,
the `packageManager` of `editors/vscode`, or a `pnpm/action-setup` step in
`ci.yml`, `commitlint.yml` or `contributors.yml`.

### Package installs

The root tools and `editors/vscode` are two separate pnpm projects. Each has
its own `pnpm-workspace.yaml` and `pnpm-lock.yaml`. The root
`pnpm-workspace.yaml` sets a 7-day minimum release age, fails the install on
an unapproved dependency build script, and keeps the flat `node_modules`
layout (`nodeLinker: hoisted`) that tools under `tools/` resolve through.

The root `package.json` sets `engines.pnpm` to `^11.25.0`. pnpm stops with
`ERR_PNPM_UNSUPPORTED_ENGINE` when its version is older than 11.25.0 or is
not pnpm 11. `mise install` gives pnpm 11.25.0. The root does not use the other
two pin fields:

- `packageManager`: `pnpm/action-setup` rejects a `packageManager` that
  differs from its `version` input. The PGO and ecosystem jobs set up pnpm 12
  for their public fixtures.
- `devEngines.packageManager`: npm fails `npm run` when no entry names npm.
  An entry that names npm makes `actions/setup-node` turn on npm caching, and
  that step fails because the root has no npm lockfile. Corepack also ignores
  an array value, so Dependabot then updates the lockfile with pnpm 10.

Dependabot reads `engines.pnpm`, so it updates `pnpm-lock.yaml` with pnpm 11.

The root `.npmrc` disables npm dependency lifecycle scripts by default. Keep
`--ignore-scripts` explicit on CI installs, including installs with `--prefix`
that load another package's configuration. Run required native builds and
extension-host downloads through the existing explicit build and test commands.
Do not turn install scripts back on for the whole dependency tree.

A root-only `pnpm install` is enough for the type-aware CLI test targets. The
type-aware CLI tests launch the real sidecar from `tools/type-aware-sidecar/`;
without a sidecar-local install it resolves `typescript` from ancestor
`node_modules` directories, and the root install pins the same `typescript`
version as the sidecar (kept in lockstep through the `overrides` entry in the
root `pnpm-workspace.yaml`). The sidecar's own `node --test` suite resolves
`typescript` the same way, so it also passes after a root-only `pnpm install`.

The sidecar-local install is still needed in two cases:

```bash
npm ci --prefix tools/type-aware-sidecar
```

- The sidecar bench (`npm run bench --prefix tools/type-aware-sidecar`)
  imports sidecar-local devDependencies that the root install does not
  provide.
- When the resolvable `typescript` is missing or too old (for example the
  root install is absent or out of lockstep), the sidecar exits with code 2
  and a stderr message naming the resolved version, its location, and the
  install fix; that failure is a missing install, not a code defect.

CI installs the sidecar explicitly, so these tests pass there either way.

The mutation runner companion has its own install, because its suite runs
against Vitest 5 and Vitest 4 from `tools/mutation-runner/node_modules`:

```bash
npm ci --prefix tools/mutation-runner
node --test tools/mutation-runner/test/*.test.mjs
```

The `fallow-api` end-to-end mutation tests in
`crates/api/src/mutation/e2e_tests.rs` also use this install, and the bun-test
end-to-end test needs `bun` on PATH. Without a dependency, each affected test
prints a skip message and passes. Set `FALLOW_MUTATION_E2E_REQUIRED=1` to make
these tests fail instead. The `Check` CI job installs the runner and Bun and
sets this variable, so a broken install cannot pass in CI.

The bun-test suite needs `bun` on PATH. Without it, the suite prints a skip
message. Set `FALLOW_REQUIRE_BUN=1` to make the suite fail instead. The CI job
`mutation-runner-floor` sets it. That job reads the lowest Node.js and Bun
versions from `crates/api/mutation-runner-protocol.json`, installs those exact
versions, and runs the companion tests.

The coverage producer corpus pins four JavaScript coverage producers in a
package of its own, so the root `pnpm install` every contributor and every CI job
runs stays untouched:

```bash
npm ci --prefix tests/coverage-producer-corpus/producers \
  --no-audit --no-fund --ignore-scripts
```

That install is needed only to re-record the corpus
(`npm run refresh:coverage-producers`) or to compare the committed maps against
the pinned producers (`npm run check:coverage-producer-drift`). Do not add
`--omit=optional`: `oxc-coverage-instrument` ships its platform bindings as
optional dependencies, and its WASI fallback does not stand in for them.
Importing the package then throws `Cannot find native binding`, so recording
stops instead of recording something subtly different.

The conformance gate itself (`npm run check:coverage-producers`, part of
`verify:full` and of the CI `check` job) reads the committed maps and needs no
producer install. It does need the binary: it runs `target/debug/fallow` and
stops with `fallow binary not found at <path>` when that is missing, so build
it first with `cargo build -p fallow-cli --bin fallow`, or run any `cargo test`
target that already does. Both re-recording commands name the Node they run on
when it differs from the Node the corpus was recorded on. That line is
provenance, not a gate: the recorded maps are byte-identical on Node 22.21.1,
22.23.2, 24.18.0 and 26.7.0, and a real V8 change surfaces as a map difference
with a message that names the row.

## Local resolution invariant

Every checkout runs the dependency versions it pins. Node resolves a bare
specifier by walking ancestor directories until it finds a matching
`node_modules` entry, and `npm run` extends `PATH` the same way, so a checkout
nested inside another checkout (a git worktree placed inside the clone) borrows
the outer install whenever it has none of its own. The tools then run at
whatever version the outer checkout pinned, and the results do not describe the
branch under test.

Install into the checkout you are working in:

```bash
pnpm install
npm ci --prefix tools/type-aware-sidecar
npm ci --prefix crates/napi
pnpm --dir editors/vscode install
```

`verify:full` runs `npm --prefix crates/napi run build:debug`, whose `napi`
binary comes from that package's own devDependencies.

`verify:fast` and `verify:full` check the local installs that their gates need
before the first gate runs. When an install is missing or stale, the run stops
at once and lists every install with its fix command, so one run finds all of
them. The list is in `scripts/verify-repo.mjs`. Add an entry there when a gate
starts to need a new local install.

A `verify:full` result only describes the checkout it ran in once all four of
those installs happened there. A checkout that reuses another checkout's
`node_modules` reports on that other checkout's pinned versions, so a finding
appears or disappears for a reason the branch does not contain.

`target/debug/incremental` grows without bound when several checkouts build
against the same profile. Deleting that one directory is safe and costs a single
slower build; never delete `target/` itself.

`scripts/assert-local-resolution.mjs` enforces the invariant for the
entrypoints that load third-party modules. The JavaScript lint, format, and
commitlint commands run it in their main script bodies, so npm's
`--ignore-scripts` option cannot skip the guard. Contract generation runs its
extension dependency preflight before any Cargo schema generation. When the
guard fires it names the foreign path it resolved and the install command that
fixes it. Entrypoints that import only `node:` builtins cannot escape and need
no guard. The type-aware sidecar keeps its own preflight in
`tools/type-aware-sidecar/src/backend-preflight.mjs` because it also checks the
backend version.

## Canonical commands

Run the smallest useful scope first:

```bash
npm run verify:fast
npm run verify:full
```

The underlying repository checks include:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo clippy -p fallow-cli --features test-sidecar-key,schema-emit --all-targets -- -D warnings
cargo test --workspace --lib --bins --tests --examples
cargo check --workspace --benches
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --document-private-items
```

Focused integration checks:

- `bash action/tests/run.sh` for GitHub Action changes.
- `bash ci/tests/run.sh` for GitLab CI changes.
- `pnpm --dir editors/vscode run lint` and relevant editor tests for VS Code
  changes.
- `npm run conformance:public-smoke` for changes that need public
  real-project evidence.
- `node scripts/compare-binaries.mjs --base <bin> --head <bin>` for
  extraction or analysis changes that must not change the output. See
  [No-change evidence](#no-change-evidence-for-extraction-and-analysis-changes).
- `cargo test -p fallow-cli --test drift -- --include-ignored` for changes to a finding that the
  CLI, MCP, and `fallow_api` all report. Build `fallow-mcp` first; see the
  [drift contract](drift-contract.md).
- `npm run check:knowledge-architecture` for docs and routing changes.
- `npm run check:agent-adapters` for skill or adapter changes.
- `python3 scripts/check_telemetry_doc_sync.py` when telemetry agent-source
  guidance or a public companion contract changes.
- `node scripts/check-audit-schema-doc-sync.mjs` when audit or dead-code JSON
  envelope versions or the public audit example change.

- `npm run check:conformance-fixtures` for dead-code detection changes: it
  scores the committed fixtures under `tests/conformance/fixtures/` against
  their `expected.json`. Runs in `verify:fast`.
- `node --test scripts/workflow-policy.test.mjs` for workflow changes and for
  Rust files with Windows-only code: each such file must start a Windows CI
  job. Runs in `verify:fast`. It also checks each script that a workflow runs
  with `node <path>.mjs`, directly or through an `npm run` or `pnpm` package
  script. When the script has a main guard, its sibling `<name>.test.mjs` must
  start it with `process.execPath` and check that its work began.
  A guard that never matches makes the script exit with 0 and do nothing, and
  only a start test finds this.
- `npm run check:dupes-accuracy` for duplication changes: it re-runs the
  hand-written corpus in `tests/benchmark-corpus/` and exits non-zero below the
  committed floor in `results/accuracy-baseline.json`. Runs in `verify:full`.
  The floor is a regression tripwire, not a published accuracy claim.

Both companion parity checks look for a sibling of the main checkout. In a
linked Git worktree, the main checkout is the clone that owns the worktree.
`FALLOW_DOCS_DIR` and `FALLOW_SKILLS_DIR` override this lookup. CI sets these
paths explicitly, and a missing companion at an explicit path fails the check.

When a guessed companion path is absent, the check prints `skipped:` with the
path it tried. It still reports a missing expected document when the companion
checkout exists.

### Test temp directory

On Unix hosts, `cargo nextest run` gives each run its own `TMPDIR`. The setup
script `scripts/nextest-run-tmpdir.sh` creates the directory. It also removes
each run directory that has not changed for two hours, because nothing removes
a directory at the end of its run. `.config/nextest.toml` attaches the script
to the `default` profile, and the other profiles inherit it. The config needs
nextest 0.9.123 or later.

The reason is the audit base snapshot cache. Each `fallow audit` in the suite
leaves one cache entry in the temp directory, and each later audit probes all
entries. The fixture of a test is gone after the test, so its entry stays for
the 30-day age limit. In a shared temp directory, a local suite run then
spends most of its audit time on this probe. `cargo test` does not run the
setup script, so it keeps the shared temp directory.

On a long-lived host, the entries of earlier runs stay in the shared temp
directory until their age limit. To remove them once, run
`fallow audit-cache prune --max-age-days 0` from a repository. With age `0`,
the command removes the entries whose recorded owner directory is gone. Add
`--dry-run` to see the decisions first.

## CI placement

The repository uses the GitHub free plan: 20 concurrent jobs, and 5 of them on
macOS. Every job in a pull request run takes a runner slot from the other open
pull requests, so a long queue slows every pull request. For this reason, a
check runs on pull requests only when it must run there. The other checks run
on push to `main`, on a schedule, or with the `ci:perf` label. A failure that
shows only on `main` is fixed in the next commit on `main`. The release gate
stops a release until `main` is green.

Concurrency follows the same goal. On a pull request, a new push cancels the
older run. On `main`, a new push cancels the older push run of most
workflows, because `main` gets many merges a day and only the newest commit
needs a result. `CI` is the exception: each push run on `main` gets a group of
its own and always finishes. The README badge shows the last `CI` run on
`main`, and a cancelled run shows as failing. Miri takes longer than the gap
between most merges, so a cancelling group left no finished `CI` run on `main`.
The release commit, with a message that starts with `chore: release v`, and
manual or scheduled runs get a concurrency group of their own. Nothing cancels
them, so the release gate always gets a result for the release commit.

On pull requests:

- `CI` (`ci.yml`). Its jobs include the required status checks on `main`.
- `Commitlint` (job `Commit messages`).
- `Type-aware Benchmarks`, when `tools/type-aware-sidecar/**` changes.
- `Protocol parity`, when `crates/cli/Cargo.toml` or `Cargo.lock` changes.
- `PGO Validate`, when `scripts/pgo-train.sh`,
  `.github/scripts/pgo-compare.mjs`, `.github/scripts/pgo-profile-match.mjs`,
  `benchmarks/download-fixtures.mjs`, `.github/actions/setup-rust/**`,
  `.cargo/config.toml`, `release.yml`, `pgo-validate.yml`, `Cargo.toml`, or
  `rust-toolchain.toml` changes. It skips Dependabot Cargo branches. It
  compares a PGO build with a base build of the same pull request. A weekly
  run on `main` covers dependency changes.
- `Test GitHub Action`, when its own paths change.

On push to `main` only (each one also has `workflow_dispatch`):

- `Coverage`, with the coverage floor.
- `Cross-Architecture`.
- `Ecosystem CI`, when Rust sources or `tests/ecosystem/**` change. It is not
  a required check and its analysis step never fails, so it does not run on
  pull requests.
- `Fuzz Smoke`, which also runs every week.
- `Scorecard`.
- `osv-scanner ignores`, which also runs every week. It fails when an
  `IgnoredVulns` entry matches no advisory in its lockfile.

On push to `main`, and on a pull request only with the `ci:perf` label:

- `Benchmarks` (CodSpeed).

Add the `ci:perf` label to a pull request that changes a hot path. The label event starts the run. A pull request
without the label starts these workflows, but every job skips and uses no
runner.

`CI` uses no macOS runner. The `VS Code target host` job runs the Linux and
Windows legs. The darwin-x64 leg runs in `Release Validation`: every week, in
the release pre-flight, and in each release. The release `vscode-host-smoke`
job also loads the darwin-x64 extension before the publish jobs can start. On
busy days, the macOS queue held a `CI` run on `main` for up to two hours.

On a schedule only: `Branch Coverage`, `Ecosystem (Full)`, and `Release
Validation`. `Branch Coverage` runs every week and with
`workflow_dispatch`. It runs `cargo llvm-cov --branch` on the pinned nightly of
`Fuzz Smoke`, because `--branch` needs a nightly compiler. It writes line and
branch coverage to the job summary. It has no floor and does not gate a
release.

### Prose-only main scheduling

CI and Coverage still start on every push to main. Ordinary CI pull requests
keep shallow change-detection checkout and skip the main-only detector setup.
Their shared
`scripts/ci-change-policy.mjs` helper compares the full pushed range from the
event's `before` SHA to its `after` SHA. Only modifications confined to
`docs/development/ai-tooling.md` and `docs/development/review-routing.md`
skip heavy CI jobs and fresh coverage. Typos, script policy tests, formatting,
and documentation checks still run. These guides have no runtime or build
consumers. Keep the allowlist explicit and review consumers before extending it.

Mixed changes, other Markdown (including embedded documentation), generated
contracts, unknown paths, additions, deletions, renames, empty ranges and failed
detection run full checks. The release subject `chore: release v` always runs
full checks, and manual coverage runs do too. The CI aggregate includes change
detection, so a detector job failure cannot become successful skipped evidence.
Coverage falls back to computation on detector failure and publishes only after
successful fresh computation. Supersession and release concurrency remain as
specified above.

### Parallel heavy jobs

The slowest `CI` jobs have parts that run in parallel:

- `Check (1/2)` and `Check (2/2)` each build every test binary and run half
  of the nextest suite (`--partition hash:N/2`). The suite uses about 29
  CPU-minutes, so one 4-vCPU runner needs more than 7 minutes for it.
  `Check (1/2)` also runs the schema drift gate and the coverage producer
  conformance check. `Check (2/2)` also runs the runtime-coverage feature
  tests. The two partitions then take about the same time.
- `Lint and contracts` runs both Clippy passes, `cargo fmt --check`, the
  contract bundle drift gate and the staged subgenerator tests.
- `NAPI package` runs the NAPI build and smoke test. On `main` the build uses
  the `napi-release` profile and takes about 7 minutes, so it runs beside the
  lint steps and not after them.
- `Windows path and subprocess handling` runs the Windows nextest selection
  and the platform Clippy pass. `Windows drift harness` runs the drift harness
  on Windows. The two Windows jobs have the same trigger.
- `Miri` runs `cargo miri test` for `fallow-types`, `fallow-graph` and
  `fallow-extract` as parallel processes in one step. The Miri interpreter
  uses one core, so the job takes as long as the slowest process.
  `fallow-graph` runs as three shards with test name filters. The last shard
  skips every filter of the other two, so each test runs exactly once.
  Do not use `cargo miri nextest run`: it starts a new Miri process for each
  test, and each start costs seconds. A test that only checks safe logic at
  scale gets `#[cfg_attr(miri, ignore = "too slow under Miri: ...")]`. Miri
  looks for undefined behavior, and the normal test run still covers the
  logic. One such test once took 16 minutes under Miri.

`Check`, `Lint and contracts` and `NAPI package` have the same trigger. Only
`Check (1/2)` and `Lint and contracts` may use the runner that the
`heavy-runner` selector step picks, because the Blacksmith credit model
reserves those two jobs. Each split job uses one more runner slot in a run. `scripts/workflow-policy.test.mjs` asserts that each moved step runs in
exactly one of these jobs.

### Rule for new workflows

A job that runs on pull requests must meet one of these conditions:

1. It is a required status check on `main`.
2. It catches a bug class that `main` cannot catch one commit later. An example
   is a comparison of the pull request with its base.

Put every other job on push to `main`, on a schedule, or behind the `ci:perf`
label. When you add a workflow that runs on push to `main`, or move a check
from pull requests to `main`, update `REQUIRED_WORKFLOWS` in
`scripts/verify-release-ci.mjs`.

### Release gate

A release must not start unless every check passed on the release commit.
The `release-context` job in `release.yml` runs
`scripts/verify-release-ci.mjs --sha "$GITHUB_SHA"` before anything builds or
publishes. The script reads the workflow runs for the release SHA and:

- requires a successful run of each workflow in `REQUIRED_WORKFLOWS`;
- requires every other push run on that SHA to end as success, skipped, or
  neutral;
- ignores pull request runs and the release workflows;
- uses the newest run per workflow, and the latest attempt of that run;
- waits while a run is queued or in progress, and polls every 60 s for up to
  150 min (`--timeout-minutes`).

Path filters can skip a required workflow on a release commit. Run the gate
locally with `--dispatch-missing` before the release dispatch. After the grace
period, the script starts each missing workflow once with
`gh workflow run <file> --ref main` and waits for it. It starts runs only while
`main` on GitHub is the release commit. Outside GitHub Actions, the token
defaults to `gh auth token` and `--repo` defaults to the `origin` remote.

The release procedure tells you how to fix a missing or failed run.

### CI metrics

`node scripts/ci-metrics.mjs` reads recent completed runs with `gh api`. It
prints the queue time, run time, and wall time as p50 and p90 per workflow and
per job, and the number of jobs per pull request run. Use `--limit`,
`--workflow`, `--event`, and `--json` to select and export the data. Measure
before and after a CI change.

### Optional bounded Blacksmith Miri trial

Miri uses `ubuntu-26.04` by default. A maintainer can opt in to a small trial
with the repository variable `BLACKSMITH_MIRI_ALLOCATION` on `fallow-rs/fallow`:

```json
{"month":"2026-09","firstRunNumber":100,"slots":2}
```

These are example values, not an active allocation. Before setting the variable,
check Blacksmith's current allowance, consumption, and running jobs across the
whole organization. Choose the current UTC month and a future CI workflow run
number. Do not set an organization-wide variable with this name: CI run numbers
are unique within this workflow, not across repositories or workflows.

Each slot reserves 150 equivalent 2-vCPU minutes: the existing Miri timeout of
60 minutes on a 4-vCPU runner consumes up to 120, with 30 reserved for overhead.
The three-key allocation permits at most 16 slots, reserving at most 2,400
equivalent minutes. An extended allocation accepts exactly these five keys:

```json
{"month":"2026-09","firstRunNumber":100,"slots":2,"budgetCredits":8000,"priorReservedCredits":0}
```

This example does not activate a larger allowance. `budgetCredits` is a positive
safe integer up to 8,000. `priorReservedCredits` is a nonnegative safe integer
no greater than that budget. The new slots must fit the remaining envelope:
`slots <= floor((budgetCredits - priorReservedCredits) / 150)`. The original
three-key schema keeps its 2,400-credit cap. Partial schemas, extra keys, numeric
strings, unsafe integers and malformed values fail closed to GitHub.

Before authorizing more than 2,400 credits, manually confirm the organization's
larger allowance for that UTC month. Leave at least 20% of the confirmed
allowance as organization headroom and reduce the envelope for other usage and
outstanding reservations. An offered allowance is not an active allowance.
Use fewer slots when other usage reduces available capacity.

The allocation covers a fixed range of CI run numbers, so concurrent runs
cannot claim the same slot. Slots spent on skipped, cancelled, ineligible, or
failed CI runs are not recycled. Only the first attempt of a push to `main` or
an internal pull request may use Blacksmith. Forks, Dependabot, and every rerun
use GitHub. The runner expression checks the attempt again because rerunning
failed jobs can reuse a successful selector's old output.

Allocations expire at the end of their UTC month and never renew automatically.
Keep a ledger before replacing a variable. Reserve all earlier windows in full,
including unused slots, and include their sum in `priorReservedCredits`. Include
comparison work if it shares this envelope. Replacement windows must start
strictly after every earlier window ends. Never overlap windows, move them
backwards or recycle reservations. The stateless parser checks arithmetic in
one record; it cannot validate truthful history. Reconfirm allowance and usage
at each month change, including carried-over work. Removing the variable
disables new selections; it does not stop already selected or running jobs.

The selectors are steps of the `Detect changes` job, so `Check`, `Lint and
contracts` and `Miri` wait for no extra runner. A separate selector job added
its own queue wait, about 2 minutes at the median and up to 8 minutes at p90
on pull requests. The steps check out `scripts/` from `main` into
`.ci-selectors` and run the helper from there, with read-only access. An old helper rejects extended allocations and uses GitHub until
the new helper is merged. A failed selector step discards even partial output,
so Miri uses GitHub. Direct local tests do not establish live extended selection.
An unavailable Blacksmith runner after selection does not migrate an existing
job: cancel or rerun that job on GitHub. Miri's checks, toolchain, cache, and timeout
remain the same; a selector failure does not suppress them. Miri restores caches
on pull requests and saves them only on `main`. Type-aware benchmarks cancel
superseded runs of the same pull request. Push and manual runs use distinct
concurrency groups and are never cancelled by that policy.

This is a bounded trial allocation, not a live billing integration or an
organization-wide spending limit. Provider billing rules, cleanup overhead, and
other workflows can affect total usage. Verify those separately before enabling
the trial. No Blacksmith API token, paid storage, or other add-on is required.

### Optional bounded Blacksmith heavy jobs

`Check` and Release Validation's `drift-full` use GitHub by default. Set the
repository variable `BLACKSMITH_HEAVY_ALLOCATION` only after confirming the
current monthly allowance, usage and organization headroom:

```json
{"month":"2026-09","budgetCredits":2280,"priorReservedCredits":2100,"allocations":[{"workflow":"ci.yml","job":"check","firstRunNumber":12345,"slots":1}]}
```

This example activates nothing. Each 4-vCPU Ubuntu 24.04 Check slot reserves
180 credits, because one slot gives the runner to two parallel 30-minute jobs:
`Check` and `Lint and contracts`. Each full drift slot reserves 210. Each job
costs its timeout at 2 credits per minute plus 30 credits for its own runner
start, checkout and cache overhead. A Check slot is 2 x (30 x 2 + 30) = 180,
and a drift slot is 90 x 2 + 30 = 210.

When a change raises a slot cost, update `BLACKSMITH_HEAVY_ALLOCATION` in the
same step. Otherwise the selector rejects the complete record, and every heavy
job falls back to GitHub-hosted runners, including the release `drift-full`
jobs.

For the change from 90 to 180 credits per Check slot, halve the `slots` of each
`ci.yml/check` window. The reservation in credits stays the same. Put the
`drift-full` windows first in `allocations`, because the selector reserves
credits in array order. The selector before this change also accepts the
migrated record, so set the variable before the merge.

The only supported pairs are `ci.yml/check`,
`release-validation.yml/drift-full` and `release.yml/drift-full`. Reusable
validation uses its caller's workflow identity and run number, so direct and
called release windows are separate. Every window must fit the shared budget,
including unmatched windows. Duplicate pairs, malformed fields, unsafe numbers,
expired months and budgets above 8,000 fail closed. Confirm larger allowances before use.

The heavy prior must cover the live same-month Miri variable's full cumulative
reservation. Invalid Miri accounting rejects heavy admission. Keep a manual
ledger of all earlier heavy and Miri windows when replacing either variable.
Reserve skipped and cancelled slots in full. Start replacement windows after
previous endpoints; never recycle slots or renew automatically. Earlier-month
Miri windows expire, but already-running work still needs manual accounting.
Avoid month-end admission when queue delay could cross billing months.

Selectors use trusted `main` code with read-only permissions. CI publishes
each selection as an output of `Detect changes`; Release Validation keeps its
own `heavy-runner` job. Missing variables
skip startup. Failed selection, unknown callers, forks, Dependabot and reruns
use GitHub while substantive validation still runs. Only CI main pushes or
internal PRs, direct main release dispatches or schedules, and Release main
dispatches are eligible. Reusable validation still checks the caller's SHA.
Verify actual runner placement and results before claiming an enabled trial or
a speed improvement. Reservations estimate cost; they do not enforce billing.

## Rust conventions

- Prefer early returns and guard clauses.
- Use `FxHashMap` and `FxHashSet`.
- Treat `unwrap` and `expect` on user-controlled paths as defects unless
  strongly justified.
- Give every lint suppression a reason.
- Preserve size assertions when touching hot-path types.
- Normalize path separators in tests.
- In a timing assertion, measure only the operation under test. Read the
  elapsed time before you wait for a killed descendant to exit. The init
  process must reap an orphan, some container init processes reap zombies
  only after seconds, and `kill -0` reports a zombie as live.
- Redact versions, durations, temporary roots, and other volatile data in
  snapshots.

## Cargo feature unification

`npm run check:cargo-features` compares the crate features of
`cargo build -p fallow-cli` with those of `cargo build --workspace`. Cargo
resolves features per build, so a feature that only another member enables
makes cargo compile two variants of that crate and its dependents. After each
edit, a switch between the two commands then compiles the fallow crates again.
Fix a failure with the feature in `[workspace.dependencies]`, or with a
feature-only dependency in `crates/cli/Cargo.toml`. The check runs in
`verify:fast` and in the CI Clippy job.

## Hook parity

Codex does not execute `.claude/settings.json` hooks. Mirror the repository
hooks manually when they did not run.

Claude runs `node scripts/check-comment-quality.mjs --claude-pre-tool` before
each `Edit`, `Write`, and `MultiEdit`. The hook blocks an edit that adds a
narration comment or a comment about the edit history. Codex does not run
this hook, so run `node scripts/check-comment-quality.mjs --working-tree`
before you commit.

Pre-commit parity:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
typos
python3 scripts/scan-hidden-unicode.py --mode committed --staged
sh scripts/check-conflict-markers.sh --staged
node scripts/check-comment-quality.mjs --staged
node scripts/check-miri-cfg.mjs
cargo fallow size --staged --fail-on-growth --quiet
sh scripts/hook-file-length.sh HEAD
sh scripts/hook-changelog-fragments.sh
npm run lint:js
npm run fmt:js:check
```

Write changelog entries to `changelog.d/<branch>.md`, never to `CHANGELOG.md`.

The JavaScript checks run only when staged files touch a lintable JavaScript or
TypeScript scope. `typos`, Python, and Node checks run only when the matching
tool is installed, exactly as in the `hk.pkl` pre-commit hook. When `typos` or Node
is missing, the hook prints a hint with the version in `mise.toml`. The Miri cfg check
runs only when staged files include a Rust file. The conflict-marker check fails
when a staged line starts with `<<<<<<<`, `|||||||` or `>>>>>>>`; build such
lines at runtime in tests. `cargo fmt` and Clippy run
only when staged files include a Rust file or a Cargo, toolchain, rustfmt or
Clippy config file. The file-size check runs only when staged files include a
`.rs` file. The file-length check runs only when staged files include a `.mjs`
or `.ts` file, `.fallowrc.json` or `scripts/fallow-file-length.json`. See
[File size](#file-size).

The Miri cfg check reads the crates that the CI `miri` job tests. It fails when
code that Miri compiles names a module declared under `not(miri)`, for example
a `#[cfg(test)]` module that calls `crate::tests::parse_ts` while `mod tests`
is `#[cfg(all(test, not(miri)))]`. The Miri job is path-filtered and slow, so
this check runs in `verify:fast`, in the pre-commit hook, and in the CI script
tests on every pull request. Fix a failure with `#[cfg(all(test, not(miri)))]`
on the calling module.

Pre-push parity:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --document-private-items
cargo fallow check --baseline cargo-fallow-baseline.json --fail-on-issues --quiet
sh scripts/hook-file-length.sh origin/main upstream/main
```

The `cargo doc` step is the same command as the required `Documentation` CI
job. A broken intra-doc link passes fmt and clippy, so the hook catches it
before the push. With no change the step takes under 1 s.

The `cargo fallow check` step is the same command as the required
`Rust Structure` CI job. See [Rust structure](#rust-structure). When
`cargo-fallow` is not installed, the hook skips the step and prints a hint
with the version in `mise.toml`.

Recommended full local verification before review:

```bash
cargo test --workspace --lib --bins --tests --examples
cargo check --workspace --benches
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --document-private-items
```

## File size

A source file over 2000 lines must not grow. Two checks hold this limit:

- `cargo fallow size` checks `.rs` files.
- `fallow audit` checks `.mjs` and `.ts` files with `health.maxFileLines`.

### Rust files

The `file-size` pre-commit step runs `cargo fallow size --staged
--fail-on-growth` on each staged `.rs` file. The limit is 2000 lines
(`max-file-lines` in `[workspace.metadata.fallow.size]` of the root
`Cargo.toml`). The step fails in
these cases:

- A staged file over the limit has more lines than its `HEAD` version.
- A new staged file is over the limit.

A file over the limit can keep its size or shrink. The step reads the staged
blob, not the working tree, so unstaged edits do not change the result. A
renamed file is compared with its old path. The rule is no-growth only. It
does not ask a change to shrink a file.

The step skips these files:

- Generated files. One of the first 10 lines of a generated file is a
  comment with one of the exact headers that the repository generators
  write: `AUTOGENERATED FILE. DO NOT EDIT.`, or `Generated from <source>. Do
  not edit.` (`Generated by` is also valid). cargo-fallow also accepts an
  `@generated` comment. An author can still add a header by hand, so review a
  new generator header like any other change.
- Files in a `snapshots` or `__snapshots__` directory. Snapshot files
  (`.snap`) are not checked file types.
- Test files. These are files in a `tests` or `test` directory, and files
  named `tests.rs`, `*_tests.rs` or `*_test.rs`. A test file grows with each
  new test case.

To fix a failure, move the new code to a new module, or remove lines from
the file in the same change. To skip only this step for one commit, run
`HK_SKIP_STEPS=file-size git commit`. `HK=0 git commit` skips all hooks.

The `Large file growth` step of the CI `Rust Structure` job runs the same
rule on pull requests. It compares `HEAD` with the merge base of the pull
request base branch:

```bash
cargo fallow size --base origin/main --fail-on-growth
```

The CI step has no skip route. A pull request that must grow a large file
needs a maintainer decision. The step runs after a failed step. When the step
cannot read the base ref, for example because the ref is not fetched, it exits
with 2, not 1.

## Rust structure

[cargo-fallow](https://github.com/fallow-rs/cargo-fallow) checks the Rust
workspace. The configuration is in `[workspace.metadata.fallow]` of the root
`Cargo.toml`. The required `Rust Structure` CI job, the `cargo-fallow`
pre-push step and `npm run verify:fast` run the same command:

```bash
cargo fallow check --baseline cargo-fallow-baseline.json --fail-on-issues
```

Each gate fails the run:

- `deps`: a dependency that no code uses, or that only tests use. A
  feature-only dependency goes in `ignored` of
  `[package.metadata.fallow.deps]`, with the reason.
- `files`: a `.rs` file that no target compiles.
- `visibility`: a `pub`, `pub(crate)` or `pub(super)` item that can be less
  visible. `production` lists the release binaries. cargo-fallow treats each
  crate that a registry can publish as an external API, so it reports no dead
  `pub` item in the published crates. Only `pub(crate)` and `pub(super)`
  items there get a finding
  ([cargo-fallow#70](https://github.com/fallow-rs/cargo-fallow/issues/70)). Run
  `cargo fallow fix --dry-run`, then `cargo fallow fix --yes`, then the
  Clippy commands. Clippy can reject an edit that `fix` accepts.
- `boundaries`: the crate rules and the source rules in
  `[workspace.metadata.fallow.boundaries]`. The crate rules keep the
  dependency direction of
  [Architecture invariants](../architecture-invariants.md). The source rules
  keep the semantic client in `fallow-api` and the process-tree code in
  `fallow-process`. A source rule matches tokens, so a pattern in a comment
  or a string does not match.
- `complexity`: a function over the cognitive limit of 25 or the cyclomatic
  limit of 20. `cargo-fallow-baseline.json` holds the functions that were over
  a limit when the gate started. A new function over a limit fails. When you
  simplify a function in the baseline, save the baseline again with
  `cargo fallow check --only complexity --save-baseline cargo-fallow-baseline.json`.
- `dupes`: the duplication percentage over `threshold` in
  `[workspace.metadata.fallow.dupes]`. The threshold is a ratchet just above
  the current percentage, so a change that adds a clone can fail the gate.
  Remove the clone, or remove another clone in the same change. The single
  clone groups are warnings. Lower `threshold` when the percentage goes down.
- `parse`: a target root file that does not parse.

The cargo-fallow complexity model is not the Clippy model, so a function can
pass `clippy::cognitive_complexity` and fail the cargo-fallow limit.

To hide one finding, put `// fallow-ignore-next-line <rule> -- <reason>`
above the line. Link the cargo-fallow issue when the finding is a cargo-fallow
false positive. `cargo fallow explain <rule>` explains each rule.

`scripts/check-cargo-features.mjs` stays, because cargo-fallow does not
compare the feature sets of a member build and the workspace build yet
([cargo-fallow#71](https://github.com/fallow-rs/cargo-fallow/issues/71)).

### JavaScript and TypeScript files

The first `health.thresholdOverrides` entry in
`scripts/fallow-file-length.json` sets
`maxFileLines: 2000` for `**/*.mjs` and `**/*.ts`. Two later entries set
`maxFileLines: 0` for test files and for generated files, so the check skips
them. A later entry wins. Add a new generated `.mjs` or `.ts` file to the last
entry. The check also skips each path in `ignorePatterns` and in
`health.ignore`.

`scripts/fallow-file-length.json` extends `.fallowrc.json` for the gate. The
overrides are not in `.fallowrc.json`, because there they change each
`fallow health` run in the repository. The gate config also
turns off the cyclomatic, cognitive and CRAP rules, and it sets
`audit.worsened` to `error`. A change then fails only on file length. The
audit fails in these cases:

- A new file is over the limit, or a file crosses the limit (`introduced`).
- A file over the limit grew by 5 lines or more (`worsened`).

A file over the limit that grew by 1 to 4 lines stays `inherited` and passes.
This is the line-count tolerance of `fallow audit`. A comment line and a blank
line count as lines.

`scripts/hook-file-length.sh` runs the audit. It uses
`target/debug/fallow` when it exists, else a `fallow` on the `PATH`. When it
finds no binary or no base ref, it prints a note and passes, because CI still
runs the check:

```bash
fallow audit --config scripts/fallow-file-length.json --base <merge-base> --no-css --quiet
```

The script gives the merge base of `HEAD` and the ref to `--base`, not the
ref. `fallow audit` reads the base files at the commit that `--base` names.
When the base branch removed lines from a long file after the branch started,
a compare with the ref shows growth that the branch did not add.

- The `file-length` pre-commit step compares the working tree with `HEAD`.
  It reads the working tree, not the staged blob.
- The `file-length` pre-push step compares the working tree with the merge
  base of `origin/main`, or of `upstream/main` in a fork clone.
- `npm run verify:fast` runs the script against `origin/main`.
- The `File length` step of the CI `Fallow Self Analysis` job runs the audit
  with the binary of the pull request against `HEAD^1`. HEAD is the merge
  commit of the pull request, so `HEAD^1` is the tip of the base branch. The
  job runs when a pull request changes a `.mjs` or `.ts` file or one of the
  two config files.

To fix a failure, move the new code to a new module, or remove lines from the
file in the same change. To skip only the hook step for one commit, run
`HK_SKIP_STEPS=file-length git commit`. The CI step has no skip route.

`verify:fast` runs the two checks as its `Large file growth` and `File length`
steps, so a local run finds the failure before CI does. The steps compare with
the local `origin/main` ref, so run `git fetch origin main` first to compare
with the current base. When `origin/main` does not exist, for example in a
fork clone with another remote name, `verify:fast` skips the steps and prints
the reason.

During a merge commit, the pre-commit step compares each file with the larger
of its `HEAD` and `MERGE_HEAD` versions. Thus growth that the merged branch
brought in does not fail the merge. On `git commit --amend`, the step compares
with the commit that the amend replaces, so it does not see growth that is
already in that commit. The CI step is the authoritative check for amends.

## Version bumps

The `Version bumps against the base` step of the CI `JS Lint and Format` job
checks each `*CACHE_VERSION` and `*SCHEMA_VERSION` constant in the Rust
sources. The step reads the net change that the non-merge commits of the
pull request make to each constant. It fails when the pull request head has
the same value as the current base branch for such a constant. A merge of
the base into the branch moves the merge base, so the step does not use the
merge base. A removal and a bump that the branch reverts again do not count.

This case occurs when two pull requests make the same bump, for example
`CACHE_VERSION` from 372 to 373. Git merges two equal edits with no conflict,
so the second merge keeps 373. A warm cache of the old format then matches
the new version. To fix a failure, set the constant above the value on the
base branch.

```bash
node scripts/check-version-bumps.mjs --base origin/main
```

The step compares the pull request head, not the merge commit of the
checkout. The required checks do not require an up-to-date branch, so a
green run can become stale when the base branch moves. A rebase that is not
`scripts/ship-rebase.mjs` can drop an equal bump from the commit, and then
the step cannot see the bump. `scripts/ship-rebase.mjs` compares the values
before and after each rebase of the serial merge flow. With `--if-needed`,
it also compares the values of a squash merge without a rebase, and it
rebases when that merge loses a bump. Both scripts use
`scripts/version-constants.mjs`.

## Evidence standard

For a bug fix, prove the reproduction fails without the change, passes with the
change, and works on a public real project or representative public fixture.
Then run the relevant broad suite.

Each claim in a pull request or review states how far its proof goes:

1. stated: the claim has no evidence yet;
2. pointed at: a source line supports it;
3. shown impossible: the types or the control flow exclude the other case;
4. ran: a command produced the expected output;
5. reproduced: the behavior shows on a real project or public fixture.

A safety claim needs level 4 or 5. When a check fails or passes too easily,
suspect the check first: confirm that the binary under test contains the change
and that the cache (`.fallow/`) does not serve an older result.

### Test evidence

- Write the expected value by hand. A test that computes its expected value
  with the code under test proves nothing.
- Give every "no finding" assertion a positive control: a second input on
  which the same rule does report. Without it, a detector that reports nothing
  also passes.
- Read every snapshot diff before you accept it.
- Fix the pattern, not the instance. Search for the other places where the
  same defect shape occurs and cover them in the same change.
- When a test waits for a child process, wait for a readiness signal. Stop
  early when the worker thread or process ends. Use a wall-clock bound only to
  prevent a hang.
- Put a timing assertion far below the duration of the failure case, not just
  above the normal duration. For example, use 20 seconds when a regression
  blocks for 30 seconds.
- To reproduce a timing flake, run the test binary at `nice -n 19` while busy
  loops such as `yes > /dev/null` fill every core.
- A test that removes write permission from a directory to force an error does
  not fail as expected when it runs as root, because root ignores the mode. Do
  not treat these failures as regressions. Run the tests as a normal user.

### Behavior comparison on public projects

When runtime behavior changes, run `npm run conformance:public-smoke` twice:
once with `--fallow-bin` set to the branch build and once with a release
build of the merge base (`git merge-base origin/main HEAD`), each with its
own `--out-dir`. Do not use the latest released binary as the baseline when
`main` has unreleased runtime changes: the diff then mixes those changes
with the changes of the branch. Pass `--clone` with a shared
`--cache-dir`: without a local project path, every project is skipped and the
script exits 2. Explain every difference in the pull request. An unexplained
difference is a finding.

### No-change evidence for extraction and analysis changes

Use `scripts/compare-binaries.mjs` to prove that a change to extraction or
analysis does not change the output on real projects. Build a release binary
of the merge base and a release binary of the branch. Then run:

```bash
node scripts/compare-binaries.mjs --base /path/to/base/fallow --head target/release/fallow
```

The script runs both binaries with `--format json --quiet --no-cache` on each
project and command. It removes volatile keys, such as timings and run IDs,
and compares the results. Each pair gives one line: `IDENTICAL`, `DIFF` or
`ERROR`, the two exit codes, and the user CPU time of each run. The script
writes the normalized output and a unified diff of each `DIFF` to
`target/compare-binaries`. It exits 1 on a `DIFF`, an `ERROR` or an exit-code
mismatch.

The default projects are `fastify`, `vue-core`, `svelte` and `next.js` under
`benchmarks/fixtures/real-world/`. Run `node benchmarks/download-fixtures.mjs`
to get them. In a worktree, the script uses the fixtures of the main checkout.

The command set must cover every analysis that reads the changed facts. Bare
`fallow` does not run opt-in analyses such as `security`. The default set is
bare `fallow`, `security`, `health` and `dupes`. When a change touches facts
that other commands read, add each command with `--command`, for example
`--command flags` or `--command "dupes --mode semantic"`.

For documentation and agent discovery, validate a clean Git-visible tree,
classified root and maintainer documents, local links, repository source paths,
portable references, adapter drift, cross-repository contracts, the docs index,
and the Trigger Tree static gate.
