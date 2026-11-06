### Changed

- **The `Rust Structure` CI job uses cargo-fallow 0.3.0.** The rule `parse-through-parse-bounded` now fails when a listed file that wraps its own parser calls no longer calls `parse_bounded`. It also fails when a `max` count is higher than the calls of its file. The fuzz targets count as users of workspace items, so `fallow_core::parse_script` needs no suppression. This change affects contributors only.
