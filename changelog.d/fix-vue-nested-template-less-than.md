### Fixed

- **A `<` inside a nested Vue `<template>` no longer hides the references of
  the template.** Before this fix, a `<` in an interpolation of a nested
  template, such as `<template v-if="W">{{ 1 < 2 }}</template>`, made fallow
  read the `<` as the start of a tag. The search for the closing
  `</template>` then failed, and each name that the template used was lost.
  A named import that only the template used was reported as an unused
  export. Fallow now skips `{{ }}` interpolations when it looks for the
  closing `</template>`. A `<` that is not followed by a letter, `/`, `!` or
  `?` is now read as text, as in HTML. The cache versions change, so a warm
  cache does not keep the old result.
  ([#3543](https://github.com/fallow-rs/fallow/issues/3543))
