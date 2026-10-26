### Fixed

- **Component export queries no longer stop type-aware refinement for the whole run.** Vue, Astro, Svelte and MDX query files receive an `unsupported-syntax` abstention before the semantic backend opens them. Supported TypeScript and JavaScript queries continue to resolve in the same batch. (Closes [#3763](https://github.com/fallow-rs/fallow/issues/3763))
