### Fixed

- **Type-aware symbol impact follows compiler-resolved imports and re-exports.** Explicit `.js` imports that resolve to TypeScript sources now retain transitive affected files and targeted tests, including their shortest provenance paths.
