//! The decisions of one run, each with the input that gave it.

mod pick;
pub mod steps;

pub use pick::{PerAnalysis, ProductionInputs, decide_production};
