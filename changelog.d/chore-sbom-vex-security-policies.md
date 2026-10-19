### Added

- **Each release now publishes a CycloneDX SBOM for every target.** The files
  are named `fallow-<target>.cdx.json`. `.vex/openvex.json` states why fallow
  is not affected by each advisory that the repository ignores. `SECURITY.md`
  now lists the supported versions and the code scanning policy.
