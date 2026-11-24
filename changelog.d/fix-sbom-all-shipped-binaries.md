### Fixed

- **The release SBOM of each target now covers every binary of the `fallow-<target>` archive.** Before, the SBOM covered only the `fallow` binary. Crates that only `fallow-lsp`, `fallow-mcp` or `fallow-similar-code` link, such as `tower-lsp-server`, `rmcp`, `rquickjs` and `candle-core`, were not in it. The SBOM also listed the `stub_sidecar` and `fallow-schema-emit` bins, which the release does not ship. The release job now merges the SBOMs of the four packages into one file with the same name, and it lists only the crates of their builds.
  (Closes [#5190](https://github.com/fallow-rs/fallow/issues/5190))
