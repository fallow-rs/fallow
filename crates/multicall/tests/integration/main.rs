//! `fallow-multicall` integration tests, built as one test binary.
//!
//! A separate test binary per file links the crate once per file. One
//! binary links it once. Add a new integration test as a module here, not as a new
//! file in `tests/`.

mod parity;
mod release_features;
mod server_dispatch;
