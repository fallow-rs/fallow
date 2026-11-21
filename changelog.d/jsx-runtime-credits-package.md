### Fixed

- **`fallow dead-code` no longer reports `react` as unused when the automatic JSX runtime uses it.** The automatic runtime adds an import of `react/jsx-runtime` to each file with JSX, and no import statement shows it. Fallow now credits `react` when a file with JSX and no runtime pragma is below a tsconfig with `jsx` set to `react-jsx` or `react-jsxdev` and no `jsxImportSource`, or is in a package where Next.js is active. A project without JSX files still gets the finding, and the credit goes only to the workspace of the file with JSX.
  (Closes [#4943](https://github.com/fallow-rs/fallow/issues/4943))
