# @fallow-cli/fallow-node

Native Node.js bindings for fallow's main analyses.

## Install

```bash
npm install @fallow-cli/fallow-node   # or: pnpm/yarn/bun add @fallow-cli/fallow-node
```

## API

- `detectDeadCode(options?)`
- `detectCircularDependencies(options?)`
- `detectBoundaryViolations(options?)`
- `detectDuplication(options?)`
- `detectSimilarCode(options?)`
- `detectFeatureFlags(options?)`
- `computeComplexity(options?)`
- `computeHealth(options?)`

All functions are async and return the same JSON-shaped report contracts that the CLI emits for `--format json`.

`detectSimilarCode` is opt-in and advisory. It resolves and verifies the
exact-version `fallow-similar-code` companion before project source is read,
then returns unverified semantic candidates from the pinned local model. It
never downloads the model. Run `fallow similar-code setup --local` separately
after explicit user confirmation.

Enum-like option values use lowercase CLI-style strings such as `"mild"`, `"cyclomatic"`, `"handle"`, and `"low"`.

Shared options mirror analysis-affecting CLI globals, including `root`, `configPath`, `noCache`, `threads`, `diffFile`, `production`, `changedSince`, `workspace`, `changedWorkspaces`, and `explain`. Object-shaped JSON roots have a top-level `kind` discriminator. Branch on `kind` when reading a report. `diffFile` accepts a path to a unified diff file. Reading diffs from stdin is CLI-only.

The public declarations live in `types/index.d.ts`. The package-level
`index.d.ts` is a generated entry point that re-exports that canonical source.
This keeps the package root stable when NAPI-RS regenerates its temporary
declarations during a native build.

Rejected promises throw a `FallowNodeError` with:

- `message`
- `exitCode`
- optional `code`
- optional `help`
- optional `context`
