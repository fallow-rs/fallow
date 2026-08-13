# Quality gates

Use this before large changes, reviews, commits, and pushes.

## One-time local setup

The type-aware CLI tests launch the real sidecar from
`tools/type-aware-sidecar/`, which needs its own dependencies:

```bash
npm --prefix tools/type-aware-sidecar install
```

Without it, the sidecar falls back to whatever `typescript` ancestor
`node_modules` directories provide. The root install pins the same
`typescript` version as the sidecar (kept in lockstep through the root
`package.json` `overrides` entry), so the type-aware CLI tests still pass
after a root-only `npm ci`. When the resolvable `typescript` is missing or too
old, the sidecar exits with code 2 and a stderr message naming the conflict
and the install command above; that failure is a missing install, not a code
defect. CI installs the sidecar, so these tests pass there either way.

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
pnpm --dir editors/vscode install
```

`scripts/assert-local-resolution.mjs` enforces the invariant for the
entrypoints that load third-party modules. It runs before the JavaScript lint
and format scripts, and before contract generation reaches the extension
codegen. When it fires it names the foreign path it resolved and the install
command that fixes it. Entrypoints that import only `node:` builtins cannot
escape and need no guard. The type-aware sidecar keeps its own preflight in
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
