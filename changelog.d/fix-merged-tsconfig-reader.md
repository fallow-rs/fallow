### Fixed

- **Settings that a tsconfig inherits through `extends` now count everywhere.** fallow reads tsconfig compiler options through one reader that follows a local, package, package subpath and array `extends` with the merge rules of TypeScript. A later entry of an array `extends` overrides an earlier one, and a relative path resolves against the config that sets it. These results change:
  - `jsx` and `jsxImportSource` from an extended config credit the JSX runtime package, so `react` is no longer an unused dependency when only a base config sets `"jsx": "react-jsx"`.
  - `types` and `paths` from an extended config credit their packages and register their aliases. `paths` targets resolve against `baseUrl` when a config of the chain sets it.
  - The `files`, `include`, `exclude` and `typeRoots` of a package config, such as one under `node_modules`, decide which module declaration files are unused. A parent that does not parse keeps every declaration file.
  - The resolver fallbacks for `paths`, `baseUrl` and `rootDirs` read the merged config. A `paths` mapping in a config that `extends` names through a package `exports` subpath now resolves.
  - Only a config that contains the importing file decides its `paths`, `baseUrl` and `rootDirs`, as in TypeScript. In an Nx-style app, the solution `tsconfig.json` (`files: []`) does not hide the `paths` of the `tsconfig.app.json` that owns the file. `files` and `include` add up, so a config that inherits `files: []` and sets its own `include` contains the files that `include` matches.
  - A `paths` target resolves against `baseUrl` (TypeScript 5) and then against the config that sets `paths` (TypeScript 7, which has no `baseUrl`), so an alias resolves on either version.
  - The boundary presets read an inherited `rootDir`.
  The graph cache version moves, so the first run after the upgrade resolves every import again.
  (Closes [#4980](https://github.com/fallow-rs/fallow/issues/4980))
