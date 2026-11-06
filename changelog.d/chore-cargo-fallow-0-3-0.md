### Changed

- **The Rust structure checks use cargo-fallow 0.3.0.** The parser call site rule now fails when a listed file has fewer calls than its `max`, and the files that wrap their own calls must call `parse_bounded`. The visibility check counts the uses in the `fuzz/` workspace, so the suppression for `fallow_core::parse_script` is gone. This change affects contributors only.
