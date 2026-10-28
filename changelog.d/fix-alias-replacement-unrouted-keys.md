### Fixed

- **`fallow dead-code` no longer reports the exports of an alias replacement file for two more alias shapes.** Fallow applies a config alias only when the normal resolution of an import fails, so these replacement files had no consumer. With `includeEntryExports`, fallow reported their exports as unused. Fallow now credits every export of the replacement file in these cases:
  - A Vitest or Vite alias with a bare key for an installed package, such as `{ "@scope/pkg": "./__mocks__/@scope/pkg.ts" }` with `node_modules/@scope/pkg` present. The import resolves to the real package.
  - A top-level `resolve.alias` with a relative key in a Vite mode config, such as `vite.config.prod.ts`. The import resolves to the original file.

  An alias key for a package that is not installed, and a path-alias key such as `@lib`, keep precise export tracking, because the import resolves to the replacement file.
