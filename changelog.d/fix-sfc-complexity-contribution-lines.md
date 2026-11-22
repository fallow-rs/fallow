### Fixed

- **`fallow health --complexity-breakdown` gives Vue, Svelte and Astro contributions the lines of the file.** Each contribution of a function in a `<script>` block or in Astro frontmatter now has the line and column of the full file, as the function itself does. Before, the contributions kept the lines of the script body. The extraction cache version moves, so the first run after the upgrade parses every file again.
