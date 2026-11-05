### Changed

- **More repository checks run in cargo-fallow.** The `Rust Structure` CI job now also checks the bounded parser call sites, the Miri test build, the feature sets of `fallow-cli`, and glob re-exports of the internal type modules. It also enforces the growth of large `.rs` files in the same `cargo fallow check` run. cargo-fallow replaces `scripts/check-parse-sites.mjs`, `scripts/check-miri-cfg.mjs` and `scripts/check-cargo-features.mjs`. The duplication gate now counts production code only. This change affects contributors only.

### Removed

- **Unused internal Rust items are gone.** Only `fallow-api`, `fallow-types`, `fallow-output`, `fallow-config`, `fallow-report-protocol`, `fallow-license` and `fallow-v8-coverage` keep their exported items as entry points of the visibility check. The other workspace crates lost the `pub` items that no other crate uses, including the deprecated `fallow_core::analyze::feature_flags` functions. Use `fallow_api::run_feature_flags`. The workspace Rust crates are not a semver-stable API (see `docs/backwards-compatibility.md`). The CLI, JSON, npm and protocol surfaces do not change.
