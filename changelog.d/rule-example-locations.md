### Fixed

- **Compact output gives the line of a re-export cycle.** A `re-export-cycle:` line now has the line of the re-export in the first file after the path, as a `circular-dependency:` line does: `re-export-cycle:src/a.ts:4:src/a.ts <-> src/b.ts`. The line is `0` when it is not known. A script that splits a compact `re-export-cycle:` line on `:` must read one more field.

- **A `css-dead-surface` finding for unused scoped classes points at the class.** The `scoped-unused-class` finding of a Vue or Svelte component reported line 1 of the file. It now reports the line of the first definition of an unused class in the `<style scoped>` block, in all formats. A `// fallow-ignore-next-line css-dead-surface` comment must now be above that line. `css_analytics.scoped_unused[]` in `fallow health --css --format json` adds the field `line`. The SARIF and CodeClimate location of the finding changes, so a CI system that keys a finding on its line sees one finding go and a new one come.
