### Fixed

- **Release SBOMs list only the crates the binary compiles.** Each per-target `fallow-<target>.cdx.json` came from `cargo cyclonedx`, which reads workspace-unified features, so the v3.45.0 Linux SBOM declared 35 crates (`rand`, `time`, `jiff`, `cookie` and others) that `fallow` never builds. The release now generates it with `sbom-attest generate` from `cargo tree --target` and fails the SBOM job when `sbom-attest audit` finds a crate that is missing, phantom, yanked, unhashed or at the wrong version.
