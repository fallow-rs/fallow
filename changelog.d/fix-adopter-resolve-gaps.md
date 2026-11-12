### Fixed

- **`fallow dead-code` credits a hand-written declaration file through its JS sibling.** An import of `./lib.mjs` now also counts as a use of the matching export of `lib.d.mts`. The same applies to `.js` with `.d.ts` and to `.cjs` with `.d.cts`. Under `includeEntryExports`, fallow reported each export of such a declaration file as unused. An export that no importer uses still reports.
- **`fallow dead-code` no longer reports an alias import of gitignored generated code as unresolved.** A tsconfig `paths` alias or a framework alias that points at a missing, gitignored path (for example `@/.next/types/routes`) is now silent, the same as the relative form of the import. An alias target that no ignore rule covers still reports.
- **The Nuxt plugin resolves `#layers/<name>/` for the `$meta.name` of the project root.** Nuxt points this alias at the project root. Fallow did not register it, so files that only these imports reach reported as unused.
