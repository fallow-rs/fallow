//! `fallow-engine` integration tests, built as one test binary.
//!
//! A separate test binary per file links the engine once per file. One
//! binary links it once.
//! Add a new integration test as a module here, not as a new file in
//! `tests/`.

mod cache_model;
mod churn_cache_transparency;
mod dupes_integration;
mod dupes_profile;
mod dupes_span;
mod dupes_stress_test;
