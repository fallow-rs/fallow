### Changed

- **The `fallow` agent skill runs `fallow doctor` as a preflight.** Agents now run the readiness check before the first analysis of a project and when a result looks wrong. They run a remediation without approval only when its `mutating` field is `false`.

### Fixed

- **`fallow doctor` names the project package manager in its install remediations.** A pnpm, Yarn or Bun project now gets `pnpm install`, `yarn install` or `bun install` instead of `npm install`. The type-aware companion install follows the same rule. The `packageManager` field of `package.json` wins, then the lockfile. A workspace package uses the signal of its monorepo root. npm stays the default when the repository has neither.
