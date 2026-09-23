//! Levenshtein-distance helpers for typo detection across config surfaces.
//!
//! The implementation lives in `fallow_types::levenshtein`, so that the
//! suppression parser in `fallow-types` and the config consumers use one copy.

pub use fallow_types::levenshtein::{closest_match, levenshtein};
