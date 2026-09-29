//! `fallow-engine` integration tests, built as one test binary.
//!
//! Each module used to be a test binary of its own. Every binary linked the
//! engine, so the build linked it once per file. One binary links it once.
//! Add a new integration test as a module here, not as a new file in
//! `tests/`.

mod churn_cache_transparency;
mod dupes_integration;
mod dupes_profile;
mod dupes_stress_test;
