### Fixed

- **`css-broken-reference` no longer suggests a "did you mean" fix for a defined class.** Three cases caused false findings. In a Tailwind project, a utility such as `w-full` was reported as a typo of an authored class such as `wl-full`. Fallow now skips tokens with the shape of a Tailwind utility when a stylesheet loads Tailwind or the root `package.json` declares it. A class that a component defines only through `:global(.x)` in an Astro, Vue or Svelte `<style>` block, or in a CSS module, now counts as defined. A `<style>` block in an `.html` file now contributes its classes. Thanks to @zaphir for the report.
  (Closes [#3588](https://github.com/fallow-rs/fallow/issues/3588))
