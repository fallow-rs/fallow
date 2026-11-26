# Inline Suppression Comments

| Comment | Effect |
|---------|--------|
| `// fallow-ignore-next-line` | Suppress any issue on the next line |
| `// fallow-ignore-next-line unused-export` | Suppress specific issue type |
| `// fallow-ignore-file` | Suppress all issues in a file |
| `// fallow-ignore-file unused-export` | Suppress specific issue type file-wide |
| `// fallow-ignore-start code-duplication` ... `// fallow-ignore-end code-duplication` | Suppress clones only inside the range; a clone that reaches into it is cut at the border |

The `/* */` and JSDoc `/** */` block forms work the same way. In a multi-line JSDoc comment, the marker can start any ` * ` line. A next-line marker in a block comment applies to the line after the closing `*/`. In a block comment, a line that starts with the marker word is a marker only when each target before an optional `-- reason` is an issue kind or looks like one (lowercase words joined by `-`). A wrapped prose line such as ` * fallow-ignore-next-line is how you hide a finding.` is not a marker. A block comment must start its line, and a single-line block comment must end its line. For `fallow dupes` and for Vue, Svelte, Astro, MDX, HTML and CSS files, a multi-line block comment must open with `/*` or `/**` alone on its line, or with the marker on that line. Markup files also accept `<!-- fallow-ignore-next-line -->` on one line.

A range marker (`fallow-ignore-start`, `fallow-ignore-end`) must be on its own line and supports only `code-duplication`. The reason goes on the start marker. A range marker in a string or template literal does not count. `fallow dupes` reports a range marker with a problem (an end with no start, a nested start, a start with no end, no kind, another kind, or a marker that shares its line with code) in `range_marker_problems`, and `fallow suppressions` lists ranges with `level: "range"` and `end_line`.

## Valid Issue Type Tokens

`unused-file`, `unused-export`, `unused-type`, `unused-dependency`, `unused-dev-dependency`, `unused-enum-member`, `unused-class-member`, `unused-store-member`, `unresolved-import`, `unlisted-dependency`, `duplicate-export`, `circular-dependency`, `re-export-cycle`, `boundary-violation`, `policy-violation`, `unused-optional-dependency`, `type-only-dependency`, `test-only-dependency`, `code-duplication`
