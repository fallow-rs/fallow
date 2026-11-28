### Fixed

- **Fallow checks a path that a project file names against the project before it tests whether the file exists.** The result is now the same whether a file outside the project exists or not:
  - A `package.json` entry with an absolute path outside the project, or behind a link out of the project, is skipped before any probe. The warning names the entry only. A file that the extension or index probe reaches through a link out of the project counts as missing.
  - `fallow coverage setup` skips such an entry when it selects the server entry file.
  - A plugin setup file counts only when it resolves inside the project. A local Oxlint `jsPlugins` path out of the project is not probed.
  - A Nuxt layer counts only when its config resolves inside the project. A layer in the same repository or workspace still counts.
  - A `webpack://` or `vite://` source of a V8 source map resolves only to a file inside the project root.
  - A lockfile that is a link out of the project no longer selects the package manager.
  - A workspace glob match that resolves outside the project gives no `glob-matched-no-package-json` diagnostic and no recovered nested package.
  - `fallow rule-pack init` refuses a pack path behind a link out of the project before it checks whether the file exists.
