//! One visitor that narrows an `AnalysisResults` value to a result scope.
//!
//! The changed-file scope, the diff scope and the workspace scope each
//! implement `ResultScope`. `retain_in_scope` names every finding array
//! one time, so a new array reaches every scope.

#[cfg(test)]
#[path = "result_scope_tests.rs"]
mod tests;
