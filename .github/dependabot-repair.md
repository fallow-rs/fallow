# Dependabot repair notes for fallow

The org-wide Dependabot workflow in `fallow-rs/.github` reads the `main` copy
of this file when it repairs a failed Dependabot pull request in this
repository. Its generic rules are in `dependabot/instructions.md` there. This
file adds what is specific to fallow.

## Fallow rules

1. Sign off the commit with `git commit -s`. CI checks the DCO sign-off.
2. The commit subject must pass commitlint: a conventional prefix and at most
   100 characters.
3. Prefer the regeneration scripts of this repository over a hand edit of a
   generated file.

## Install commands

| Directory | Command |
| --- | --- |
| Root | `npm ci --no-audit --no-fund --ignore-scripts` |
| `viz-frontend/` | `npm ci --ignore-scripts`, then `npm run build` |
| `editors/vscode/` | `corepack enable`, then `pnpm install --frozen-lockfile --ignore-scripts` |
| `crates/napi/` | `npm ci --omit=optional --ignore-scripts` |
| `tools/type-aware-sidecar/` | `npm ci --prefix tools/type-aware-sidecar --no-audit --no-fund --ignore-scripts` |

To change a lockfile, use `npx -y npm@11 install --package-lock-only
--ignore-scripts` in that directory, or `pnpm install --lockfile-only
--ignore-scripts` in `editors/vscode/`.

## Known causes and fixes

These causes repeat. Check them first.

| Failed job | Cause | Fix |
| --- | --- | --- |
| `JS Lint and Format`, step `Script tests` | `scripts/*.test.mjs` asserts the exact SHA or version of a pinned action or package. | `git grep` the old SHA or version. Replace it with the new value from the diff in the test file. Run `node --test scripts/<file>.test.mjs`. |
| `JS Lint and Format` or `npm Package` on `@tanstack/intent` | The root `package.json` and `npm/fallow/package.json` must pin the same version, and `scripts/workflow-policy.test.mjs` asserts it. | Bump both manifests. Update the root lockfile. Update the version literals in the test. |
| `Viz frontend` with `git diff --exit-code crates/cli/viz-assets` | A bundler update changes the committed viz bundle. | Run the `viz-frontend/` install and build. Commit `crates/cli/viz-assets/`. |
| `Check (n/2)` on the coverage producer corpus | `tests/coverage-producer-corpus/manifest.json` pins the producer dependency graph by digest. | Run `npm run refresh:coverage-producers`. Commit only version and digest changes. If `node_pin` changes, revert that line. |
| Runtime lock digest check in `tests/semantic-clone-corpus` | The corpus evidence pins the model runtime. New evidence changes similarity values, and a maintainer must review that shift. | Report "not fixed" with the failing case names. |
| `npm ci` fails with `Missing: <package> from lock file` | Dependabot pruned a nested lockfile entry. | Regenerate that lockfile. The diff must contain only version, resolved and integrity lines. |
| `Lint and contracts`, step `Clippy`, with a removed or renamed API | A semver-compatible update deprecated or changed an API. | Change the call sites in the affected crate. Run `cargo clippy -p <crate> --all-targets -- -D warnings`. |
| `NAPI package` or `Lint and contracts` with `cannot find function ... in napi::bindgen_prelude` | `napi` and `napi-derive` must move together. | Run `cargo update -p napi --precise <latest>` with the `napi-derive` version from the pull request. Run `cargo build -p fallow-node`. |
| `Analyze (*)` with "must use the same version" | `github/codeql-action` sub-actions differ. The fix is in `.github/`. | Report "not fixed". |
| `Cargo Deny` or `Security Audit` | A new advisory affects `main` too. | Report "not fixed" and name the advisory. |
| `cssparser` compile error with `lightningcss` | `lightningcss` pins its own `cssparser`. | Report "not fixed". |

## Local checks by job

- `JS Lint and Format`: the root install, then `npm run lint:js`,
  `npm run fmt:js:check` and `node --test scripts/*.test.mjs`.
- `npm Package`: the root install, then `node --test .github/scripts/*.test.mjs`.
- `Lint and contracts`: `cargo clippy --workspace --all-targets -- -D warnings`
  and `npm run generate:contracts:check`.
- `Check (n/2)`: run only the failed test with
  `cargo nextest run -p <crate> <test name>`, or `cargo test -p <crate> <test name>`.
- `Typos`: `typos` on the changed files.

A full workspace test run takes too long for this job. Run it only when no
narrower check covers the failure.
