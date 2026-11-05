### Fixed

- **`fallow health --css` counts `@keyframes` in SCSS and Less sources.** This includes a Vue or Svelte `<style lang="scss">` block. An `animation: spin 1s` next to its `@keyframes spin` no longer gives a false undefined-keyframes finding. When a Sass or Less animation value holds a variable or an interpolation that fallow cannot resolve, the unreferenced check skips the `@keyframes` blocks that Sass or Less sources define. The new summary field `keyframes_unchecked` gives their count. (Closes [#4214](https://github.com/fallow-rs/fallow/issues/4214))
