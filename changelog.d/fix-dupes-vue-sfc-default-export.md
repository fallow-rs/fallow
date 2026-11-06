### Fixed

- **`fallow dupes` reports each section of identical Vue, Svelte and Astro files.** When two or more component files were identical across more than one section, only the last section was reported. The `<template>` block and the `<script>` block (for example an `export default {}` body) got no clone group. The detector now ends each clone at a section border, so each section of a copied component forms its own clone group with the correct line range in the component file.
  (Closes [#4251](https://github.com/fallow-rs/fallow/issues/4251))
