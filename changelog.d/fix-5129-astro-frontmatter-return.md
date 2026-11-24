### Fixed

- **Top-level `return` statements in `.astro` frontmatter no longer trigger parse errors or mark the file as degraded.** Astro compiles frontmatter scripts inside the component function body, where `return Astro.rewrite(...)` and `return Astro.redirect(...)` are valid. Fallow now enables `allow_return_outside_function` for Astro frontmatter parsing so imports and complexity from those pages count cleanly (`CACHE_VERSION` bumped to invalidate stale parse caches).
  (Closes [#5129](https://github.com/fallow-rs/fallow/issues/5129))
