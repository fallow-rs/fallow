### Fixed

- React Native: a re-export from a platform family now credits the re-exported names on each member of the family, as an import does. `export { default } from "./x"` no longer reports `default` in `x.tsx` as unused when `x.ios.tsx` exists. This applies to named re-exports and to `export *`.
