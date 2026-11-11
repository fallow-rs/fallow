### Changed

- **The install hint for a missing companion uses the package manager of the project.** When `fallow-type-aware` or `fallow-similar-code` is missing, the warning of `--type-aware`, `fallow type-aware status` and `fallow similar-code status` now show `pnpm add -D`, `yarn add -D` or `bun add -d` for a project that uses pnpm, Yarn or Bun, the same as `fallow doctor`. The `packageManager` field of `package.json` decides first, then the lockfile. Without either, the hint stays `npm install --save-dev`.
