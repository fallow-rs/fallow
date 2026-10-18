### Fixed

- **A `<` in template text no longer hides a later component invocation in
  the component contract scan.** Before this fix, the caller scan for Vue,
  Svelte, Angular and Ember templates read each `<` as the start of a tag.
  In `<p>{{ a < b }}<Button size="lg" /></p>`, the scan read
  `< b }}<Button size="lg" />` as one tag, and the `Button` invocation was
  lost. The scan now reads a `<` as a tag start only when a letter, `/`, `!`
  or `?` follows it. It also skips text expressions as a unit: `{{ }}` in
  Vue, Angular and Ember, and `{ }` in Svelte. A Handlebars comment, such as
  `{{!-- don't --}}`, is skipped as plain text, so an apostrophe in it cannot
  hide a later tag. The cache versions change, so a warm cache does not keep
  the old result.
