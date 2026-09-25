# Quality gates

Use this before large changes, reviews, commits, and pushes.

## One-time local setup

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

## Rust conventions

- Prefer early returns and guard clauses.
- Use `FxHashMap` and `FxHashSet`.
- Treat `unwrap` and `expect` on user-controlled paths as defects unless
  strongly justified.
- Give every lint suppression a reason.
- Preserve size assertions when touching hot-path types.
- Normalize path separators in tests.
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
tool is installed, exactly as in `.githooks/pre-commit`. The Miri cfg check
runs only when staged files include a Rust file.

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
```

The `cargo doc` step is the same command as the required `Documentation` CI
job. A broken intra-doc link passes fmt and clippy, so the hook catches it
before the push. With no change the step takes under 1 s.

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

### Behavior comparison on public projects

When runtime behavior changes, run `npm run conformance:public-smoke` twice:
once with `--fallow-bin` set to the branch build and once with the latest
released binary, each with its own `--out-dir`. Explain every difference in
the pull request. An unexplained difference is a finding.

For documentation and agent discovery, validate a clean Git-visible tree,
classified root and maintainer documents, local links, repository source paths,
portable references, adapter drift, cross-repository contracts, the docs index,
and the Trigger Tree static gate.
