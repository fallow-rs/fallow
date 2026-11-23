mod baseline;
mod counts;
mod entry_weight;
mod flags;
mod outcome;
mod recovery;
mod tolerance;

pub use baseline::{
    RegressionOpts, SaveRegressionTarget, compare_check_regression_with_identity,
    save_baseline_to_config_with_identity, save_flags_regression_baseline,
    save_regression_baseline_with_identity,
};
pub use counts::{CheckCounts, FlagsCounts};
pub use entry_weight::{
    EntryWeightCounts, EntryWeightGate, compare_entry_weight, load_entry_weight_baseline,
    save_entry_weight_baseline,
};
pub use flags::{compare_flags_regression, print_flags_regression};
pub use outcome::{RegressionOutcome, print_regression_outcome};
pub use tolerance::Tolerance;

/// The config file that a flag-only `--save-regression-baseline` rewrites:
/// the `--config` path, else the discovered config file, else a new
/// `.fallowrc.json` in the root. The save check and the write both use this
/// function, so they name the same file.
pub fn regression_config_target(
    config: Option<&std::path::Path>,
    root: &std::path::Path,
) -> std::path::PathBuf {
    config.map_or_else(
        || {
            fallow_engine::project_config::find_config_file(root)
                .unwrap_or_else(|| root.join(".fallowrc.json"))
        },
        std::path::Path::to_path_buf,
    )
}
