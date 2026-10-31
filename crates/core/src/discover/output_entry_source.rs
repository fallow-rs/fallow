//! Map a package entry in build output back to its source file.

use std::path::{Path, PathBuf};

use fallow_config::TsconfigOutputMap;
use fallow_types::discover::EntryPointSource;

use super::entry_points::{
    EntryTargets, OutputDirEntry, entry_has_parent_dir, resolve_entry_via_output_dir,
};
use crate::plugins::config_parser::lexical_normalize;

/// Map a package entry that points at build output to its source file.
///
/// This runs only the output-directory step of package entry resolution,
/// so `tsconfig` `rootDir` and `outDir` apply. `base` is the package
/// directory and is also the boundary for the result. Returns `None` when
/// the entry is not build output or no source file exists.
pub fn output_entry_source_file(base: &Path, entry: &str) -> Option<PathBuf> {
    if entry.contains('*') || entry_has_parent_dir(entry) {
        return None;
    }
    let canonical_base = dunce::canonicalize(base).ok()?;
    let output_map = TsconfigOutputMap::from_project(base);
    let targets = EntryTargets {
        output_map: &output_map,
        discovered: None,
    };
    match resolve_entry_via_output_dir(
        base,
        entry,
        &canonical_base,
        EntryPointSource::PackageJsonMain,
        None,
        targets,
    ) {
        OutputDirEntry::ShortCircuit(entry_point) => {
            entry_point.map(|entry_point| lexical_normalize(&entry_point.path))
        }
        OutputDirEntry::Continue => None,
    }
}
