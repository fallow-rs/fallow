### Fixed

- **The release SBOM of each target now lists only the crates of the `fallow-cli` build for that target.** Before, each SBOM also listed crates that only other workspace members use, such as `time`, `jiff` and `cookie`. An SBOM scan then reported advisories for crates that are not in the binary. The release job now removes those components, and it fails when a crate of the build has no component.
  (Closes [#5131](https://github.com/fallow-rs/fallow/issues/5131))
