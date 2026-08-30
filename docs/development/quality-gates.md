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
binary comes from that package's own devDependencies. Without the install the
step ends in `napi: command not found` and exit 127, after every earlier gate
has already passed.

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
- `npm run check:knowledge-architecture` for docs and routing changes.
- `npm run check:agent-adapters` for skill or adapter changes.
- `python3 scripts/check_telemetry_doc_sync.py` when telemetry agent-source
  guidance or a public companion contract changes.
- `node scripts/check-audit-schema-doc-sync.mjs` when audit or dead-code JSON
  envelope versions or the public audit example change.

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
npm run lint:js
npm run fmt:js:check
```

The JavaScript checks run only when staged files touch a lintable JavaScript or
TypeScript scope. `typos`, Python, and Node checks run only when the matching
tool is installed, exactly as in `.githooks/pre-commit`.

Pre-push parity:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
```

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

For documentation and agent discovery, validate a clean Git-visible tree,
classified root and maintainer documents, local links, repository source paths,
portable references, adapter drift, cross-repository contracts, the docs index,
and the Trigger Tree static gate.
