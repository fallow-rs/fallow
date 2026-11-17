### Fixed

- **`fallow dead-code` no longer reports a Svelte prop or import as unused when only a declaration tag reads it.** Svelte 5 supports the declaration tag `{const x = ...}` next to `{@const x = ...}`. Fallow now reads the identifiers of both tags. An expression such as `{constant}` stays an expression. The extraction cache version changes, so the first run after the update parses all files again.
  (Closes [#4714](https://github.com/fallow-rs/fallow/issues/4714))
- **`fallow dead-code` no longer reports an export as unused when only the expression of a Svelte `{#await}` shorthand uses it.** Fallow now reads `expr` in `{#await expr then}`, `{#await expr then value}` and `{#await expr catch error}`, as it does for the long form. The binding shadows names in the branch. A `then` or `catch` in a string, a call or an object stays part of the expression. The template complexity of `{#await expr then}` now counts its branch as the long form does. The extraction cache version changes, so the first run after the update parses all files again.
  (Closes [#4715](https://github.com/fallow-rs/fallow/issues/4715))
