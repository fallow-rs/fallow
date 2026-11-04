### Changed

- **The crate dependency rules of the repository are in one place.** The `architecture_boundaries` Rust tests no longer check crate edges or the process-tree and glob re-export patterns. The `cargo fallow` boundary rules in the root `Cargo.toml` check them, with dev and build dependencies included. This change affects contributors only. The released packages do not change.
