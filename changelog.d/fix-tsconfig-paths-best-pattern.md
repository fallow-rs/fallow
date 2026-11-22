### Fixed

- **A tsconfig `paths` alias resolves with the one pattern that TypeScript selects.** fallow now uses an exact pattern, else the matching wildcard pattern with the longest prefix, and tries only the targets of that pattern. Before, a pattern with a shorter prefix but a longer suffix could win, and a miss tried the targets of every other matching pattern. The dependency credit of an alias uses the same pattern.

- **`baseUrl` resolves an import that `paths` does not resolve.** A tsconfig with both `baseUrl` and `paths` now tries `baseUrl` when no `paths` pattern resolves the specifier, as TypeScript does. Before, any `paths` map stopped the lookup, so for example `@use "styles/vars"` in a Sass file stayed unresolved.
