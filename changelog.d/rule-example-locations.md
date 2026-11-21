### Fixed

- **Compact output gives the line of a re-export cycle.** A `re-export-cycle:` line now has a line field after the path, as a `circular-dependency:` line has. An example is `re-export-cycle:src/a.ts:4:src/a.ts <-> src/b.ts`. The line is `0` when it is not known. A script that splits the line on `:` must read one more field.

- **A `css-dead-surface` finding for unused scoped classes points at the class.** Before, the `scoped-unused-class` finding of a Vue or Svelte component was on line 1. Now it is on the line of the first definition of an unused class in the `<style scoped>` block. Put a `fallow-ignore-next-line css-dead-surface` comment above that line to suppress the finding. `fallow health --css --format json` adds the field `line` to `css_analytics.scoped_unused[]`.

- **A `css-broken-reference` finding for an undefined `@keyframes` points at the rule.** Before, the `undefined-keyframes` finding was on line 1 of the stylesheet. Now it is on the line of the first rule that references the name. Put a `fallow-ignore-next-line css-broken-reference` comment above that rule to suppress the finding. `fallow health --css --format json` adds the field `line` to `css_analytics.undefined_keyframes[]`. In one stylesheet, the entries are now in line order, then in name order.

- **The CodeClimate fingerprint of the two CSS findings changes.** The fingerprint uses the line. A CI system that tracks findings by fingerprint shows each of these findings as new one time. Styling findings have no `finding_id`, so no `finding_id` changes.
