//! Permutation oracle: the order of discovery must not change the output.
//!
//! The parallel source walk collects files in thread order, and workspace
//! discovery collects packages in the order of the declared globs. The test
//! build of the binary has the `permutation-hook` feature of `fallow-config`,
//! so `FALLOW_PERMUTATION_SEED` permutes these lists before the code that
//! must make them deterministic. The oracle runs each command and each output
//! format on a generated project without a seed and with a seed, and the
//! output bytes must be the same.
//!
//! Pull request CI runs one fixed seed pair per template. The daily
//! `Oracle Seeds` workflow runs `seed_sweep_keeps_output_identical` over
//! `FALLOW_PERMUTATION_SEEDS` project seeds from
//! `FALLOW_PERMUTATION_SEED_START`. A failure names the template, the project
//! seed and the permutation seed, and the environment that runs the same
//! project seed again in the sweep.

mod oracle;
mod project;
mod templates;
