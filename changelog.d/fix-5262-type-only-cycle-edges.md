### Fixed

- **`circular-dependency` no longer reports a cycle that closes only through imports of types.** TypeScript removes a plain `import { X }` when `X` is only a type in the target (an interface, a type alias, or a re-export of one). Such an import does not load the target at runtime, so it no longer closes a cycle. The import still counts when the tsconfig of the importer sets `verbatimModuleSyntax`, `preserveValueImports`, or `importsNotUsedAsValues` to `preserve` or `error`, because TypeScript then keeps it. A namespace import, a side-effect import and a re-export still count. When a dense group of files loses such an edge, the cycle list of that group can show other cycles that the per-group limit hid before.
- **A local `export { X }` of an interface or a type alias, and `export default interface`, are now type-only exports.** An unused one is now reported as `unused-type` instead of `unused-export`.
  (Closes [#5262](https://github.com/fallow-rs/fallow/issues/5262))
