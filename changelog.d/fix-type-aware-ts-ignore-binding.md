### Fixed

- **`--type-aware` no longer gives up on a project because of a binding error that `@ts-ignore` suppresses.** The semantic pass treated each binding diagnostic as blocking, also one under a `// @ts-ignore` or `// @ts-expect-error` comment, which the compiler drops. For example, Svelte declares `export const arguments: never;` under `// @ts-ignore` in an ambient file, so the whole program got no type evidence. The semantic pass now applies these comments to binding diagnostics, as the compiler does. Syntax, program and configuration errors stay blocking.
