//! Model-based oracles for the persisted caches: the read-set oracle of the
//! graph cache key, and warm equals cold over generated edit sequences.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests use unwrap and expect to keep fixture setup concise"
)]

mod project;
mod read_set;
mod sequences;
mod templates;
