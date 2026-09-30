# Quality gates

Use this before large changes, reviews, commits, and pushes.

## One-time local setup

### Git hooks with hk

[hk](https://hk.jdx.dev) runs the git hooks. The root `hk.pkl` defines the
`pre-commit`, `commit-msg` and `pre-push` hooks. hk evaluates `hk.pkl` with the
`pkl` CLI, so install both, then run `hk install` once per clone:

```bash
mise install
hk install --mise
```

Without mise, install `hk` and `pkl` at the versions in `mise.toml` and run
`hk install`. `--mise` starts the hooks through `mise x`, so a shell or git
client without mise activation still finds hk. `HK=0 git commit` skips the
hooks for one command.

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
call: hk, pkl, Node.js, `typos`, `cargo-shear`, `cargo-deny`, `cargo-audit`,
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
does not check typos and cargo-deny, because their CI actions fix the tool
version in the action commit. A Dependabot action bump then does not fail.
Update those two versions by hand.

### Package installs

A root-only `npm ci` is enough for the type-aware CLI test targets. The
type-aware CLI tests launch the real sidecar from `tools/type-aware-sidecar/`;
without a sidecar-local install it resolves `typescript` from ancestor
`node_modules` directories, and the root install pins the same `typescript`
version as the sidecar (kept in lockstep through the root `package.json`
`overrides` entry). The sidecar's own `node --test` suite resolves
`typescript` the same way, so it also passes after a root-only `npm ci`.

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

The coverage producer corpus pins four JavaScript coverage producers in a
package of its own, so the root `npm ci` every contributor and every CI job
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
npm ci
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
- `npm run check:dupes-accuracy` for duplication changes: it re-runs the
  hand-written corpus in `tests/benchmark-corpus/` and exits non-zero below the
  committed floor in `results/accuracy-baseline.json`. Runs in `verify:full`.
  The floor is a regression tripwire, not a published accuracy claim.

Both companion parity checks resolve their companion checkout as a sibling of the
main checkout, which inside a linked git worktree is the clone the worktree
belongs to and not the worktree directory. `FALLOW_DOCS_DIR` and
`FALLOW_SKILLS_DIR` override that guess and keep failing closed, which is how
continuous integration runs them. A guessed companion checkout that is not
present at all stands down with a `skipped:` line naming where it looked, so a
checkout without the companion clones reports nothing to fix. A companion
checkout that exists and has lost an expected document is still reported.

## CI placement

The repository uses the GitHub free plan: 20 concurrent jobs, and 5 of them on
macOS. Every job in a pull request run takes a runner slot from the other open
pull requests, so a long queue slows every pull request. For this reason, a
check runs on pull requests only when it must run there. The other checks run
on push to `main`, on a schedule, or with the `ci:perf` label. A failure that
shows only on `main` is fixed in the next commit on `main`. The release gate
stops a release until `main` is green.

Concurrency follows the same goal. On a pull request, a new push cancels the
older run. On `main`, a new push also cancels the older push run of the same
workflow, because `main` gets many merges a day and only the newest commit
needs a result. The release commit, with a message that starts with
`chore: release v`, and manual or scheduled runs get a concurrency group of
their own. Nothing cancels them, so the release gate always gets a result for
the release commit.

On pull requests:

- `CI` (`ci.yml`). Its jobs include the required status checks on `main`.
- `Commitlint` (job `Commit messages`).
- `Ecosystem CI`, when Rust sources or `tests/ecosystem/**` change.
- `Type-aware Benchmarks`, when `tools/type-aware-sidecar/**` changes.
- `Protocol parity`, when `crates/cli/Cargo.toml` or `Cargo.lock` changes.
- `PGO Validate`, when `scripts/pgo-train.sh`,
  `.github/scripts/pgo-compare.mjs`, `.github/scripts/pgo-profile-match.mjs`,
  `benchmarks/download-fixtures.mjs`, `.github/actions/setup-rust/**`,
  `.cargo/config.toml`, `release.yml`, `pgo-validate.yml`, `Cargo.toml`,
  `Cargo.lock`, or `rust-toolchain.toml` changes. It compares a PGO build with
  a base build of the same pull request.
- `Review Electron` and `Test GitHub Action`, when their own paths change.

On push to `main` only (each one also has `workflow_dispatch`):

- `Coverage`, with the coverage floor.
- `Cross-Architecture`.
- `Module Coupling`.
- `Fuzz Smoke`, which also runs every week.
- `Scorecard`.

On push to `main`, and on a pull request only with the `ci:perf` label:

- `Benchmarks` (CodSpeed).
- `Binary Size`.
- `Allocation Tracking`.

Add the `ci:perf` label to a pull request that changes a hot path, the binary
size, or the allocation profile. The label event starts the run. A pull request
without the label starts these workflows, but every job skips and uses no
runner.

On a schedule only: `Conformance`, `Ecosystem (Full)`, `Real-World
Benchmarks`, `Hawk`, and `Release Validation`.

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

The release procedure tells you how to fix a missing or failed run.

### CI metrics

`node scripts/ci-metrics.mjs` reads recent completed runs with `gh api`. It
prints the queue time, run time, and wall time as p50 and p90 per workflow and
per job, and the number of jobs per pull request run. Use `--limit`,
`--workflow`, `--event`, and `--json` to select and export the data. Measure
before and after a CI change.

### Optional bounded Blacksmith Miri trial

Miri uses `ubuntu-latest` by default. A maintainer can opt in to a small trial
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

The selector runs on GitHub with read-only contents access and uses the helper
from `main`. An old helper rejects extended allocations and uses GitHub until
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

## Hook parity

Codex does not execute `.claude/settings.json` hooks. Mirror the repository
hooks manually when they did not run.

Pre-commit parity:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
typos
python3 scripts/scan-hidden-unicode.py --mode committed --staged
node scripts/check-comment-quality.mjs --staged
node scripts/check-miri-cfg.mjs
npm run lint:js
npm run fmt:js:check
```

The JavaScript checks run only when staged files touch a lintable JavaScript or
TypeScript scope. `typos`, Python, and Node checks run only when the matching
tool is installed, exactly as in the `hk.pkl` pre-commit hook. When `typos` or Node
is missing, the hook prints a hint with the version in `mise.toml`. The Miri cfg check
runs only when staged files include a Rust file. `cargo fmt` and Clippy run
only when staged files include a Rust file or a Cargo, toolchain, rustfmt or
Clippy config file.

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
cargo shear
```

The `cargo doc` step is the same command as the required `Documentation` CI
job. A broken intra-doc link passes fmt and clippy, so the hook catches it
before the push. With no change the step takes under 1 s.

The `cargo shear` step is the same command as the required
`Unused Dependencies` CI job. It catches a dependency whose last use a change
removes. When `cargo-shear` is not installed, the hook skips the step and
prints a hint with the version in `mise.toml`.

Recommended full local verification before review:

```bash
cargo test --workspace --lib --bins --tests --examples
cargo check --workspace --benches
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --document-private-items
```

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
once with `--fallow-bin` set to the branch build and once with the latest
released binary, each with its own `--out-dir`. Explain every difference in
the pull request. An unexplained difference is a finding.

For documentation and agent discovery, validate a clean Git-visible tree,
classified root and maintainer documents, local links, repository source paths,
portable references, adapter drift, cross-repository contracts, the docs index,
and the Trigger Tree static gate.
