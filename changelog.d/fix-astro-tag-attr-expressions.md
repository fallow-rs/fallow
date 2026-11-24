### Fixed

- **An expression in an attribute of an Astro `<style>` or `<script>` tag now counts as a use.** For example, `CSS` in `<style set:html={CSS}>` is no longer reported as an unused export. The body of the block stays out of the scan, as before. The extraction cache version changes, so a warm cache rebuilds.
  (Closes [#5098](https://github.com/fallow-rs/fallow/issues/5098))
