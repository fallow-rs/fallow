### Fixed

- **`fallow audit` no longer reports an old unused export behind a SvelteKit path alias as introduced.** The base snapshot of `fallow audit` did not have the gitignored `.svelte-kit/tsconfig.json`, which holds the path aliases of a SvelteKit app. On the base, a file that the app imports only through such an alias was an unused file, so its unused exports looked introduced in the head. The base snapshot now gets a copy of `.svelte-kit/tsconfig.json` for the root and for each workspace package. It also links the `node_modules` of each workspace package that the base has, because pnpm installs the dependencies of a workspace package there and not in the root `node_modules`.
  (Closes [#4790](https://github.com/fallow-rs/fallow/issues/4790))
