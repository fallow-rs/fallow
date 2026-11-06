### Fixed

- **`fallow health` now scores the functions in Astro `<script>` blocks.** Before, health scored only the frontmatter and the template markup of an `.astro` file, so a complex client script passed the gate. Health now scores each inline `<script>` block, including `is:inline` and `define:vars` scripts, as it scores a Vue or Svelte `<script>` block. Each finding has its line and column in the full `.astro` file. A block with a `src` attribute or a data `type` (for example `application/ld+json`) gets no score. Existing Astro projects can get new complexity and CRAP findings after the upgrade. The extraction cache version changes, so the first run after the upgrade parses each file again.
  (Closes [#4261](https://github.com/fallow-rs/fallow/issues/4261))
