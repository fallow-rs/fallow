# Quality gates

Use this before large changes, reviews, commits, and pushes.

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
