### Fixed

- **A framework path alias of a workspace package no longer resolves into another package.** In a monorepo with two SvelteKit apps, `$lib/foo` in one app could resolve to `foo` in the `src/lib` of the other app. Fallow then reported false `boundary-violation` findings and reported the real files of the importing app as unused. A plugin alias from a workspace package (SvelteKit `$lib`, Nuxt `~/` and the others) now applies only to the files of that package. An alias from the project root still applies everywhere. SvelteKit `$lib` also follows `kit.files.lib` in `svelte.config.js`.
  (Closes [#4246](https://github.com/fallow-rs/fallow/issues/4246))
