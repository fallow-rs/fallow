//! Model-based oracles for the mutation result cache: the read-set oracle of
//! the result cache key, and warm equals cold over edit sequences.
//!
//! The read set of a mutation result is the read set of the test runner
//! process, so these tests run the real runner. The Vitest runner needs
//! `node` on `PATH` and `npm ci` in `tools/mutation-runner`. The `bun test`
//! runner needs `bun` on `PATH`. A test skips a runner that is not
//! available, except when `FALLOW_MUTATION_E2E_REQUIRED=1` is set.
//!
//! The tests use only `run_mutation` and the cache counts of its report, so
//! the oracle proof can run them on older commits.

mod project;
mod read_set;
mod sequences;
mod template;
mod trace;
