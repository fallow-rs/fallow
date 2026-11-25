### Fixed

- **An import of a SvelteKit virtual module now uses `@sveltejs/kit`, also without `svelte.config.js`.** An import of `$app/*`, `$env/*`, `$service-worker` or a generated `./$types` module credits `@sveltejs/kit` in the manifest of the importing file, in the package where the SvelteKit plugin is active. Before, a project without SvelteKit config evidence got an `unused-dev-dependency` finding for `@sveltejs/kit`, and `fallow fix` removed the package. `--trace-dependency @sveltejs/kit` reports the credit with the new `virtual-module-import` reason, which names the first importing file and the specifier.
  (Closes [#5019](https://github.com/fallow-rs/fallow/issues/5019))
