//! The entry probes of a plan (see `fallow_mutation::instrument_with_probes`).
//!
//! A run that selects its dry-run test files by reach needs the probed code of
//! each file that a dry-run test file reaches, not only of the files with a
//! mutant. A file with a mutant gets the probes in its instrumented code. Each
//! other script file of the reach gets a copy with only the probes. A file
//! that the plan cannot probe (a component file, a file that does not parse)
//! has no entry, and a reach check compares its content instead.

use std::collections::BTreeMap;
use std::path::Path;

use fallow_mutation::{MutationOptions, Outline, SourceKind};
use rayon::prelude::*;
use rustc_hash::FxHashSet;

use super::layout::Layout;

/// The probed code of one file and its outline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbedFile {
    /// The instrumented text with the entry probes.
    pub code: String,
    /// The outline of the source text.
    pub outline: Outline,
}

/// The probed files of a plan, by root-relative path.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PlanProbes {
    /// The probed files, by root-relative path.
    pub files: BTreeMap<String, ProbedFile>,
}

/// The kind of a file that gets probes: a script file. A component file and
/// the module part of a Svelte file keep their plain copy.
fn script_kind(path: &str) -> Option<SourceKind> {
    match Layout::from_path(path)? {
        Layout::Script(kind) => Some(kind),
        _ => None,
    }
}

/// Probes `source` with `options`. `None` for a file that gets no probes.
pub(super) fn probe_source(
    source: &str,
    path: &str,
    options: &MutationOptions,
) -> Option<ProbedFile> {
    let kind = script_kind(path)?;
    let (instrumented, outline) = fallow_extract::parse_bounded(source, || {
        fallow_mutation::instrument_with_probes(source, path, kind, options)
    })?
    .ok()?;
    Some(ProbedFile {
        code: instrumented.code,
        outline,
    })
}

/// Probes each script file of `reach_files` that is not in `planned`.
pub(super) fn probe_reach_files(
    root: &Path,
    reach_files: &[String],
    planned: &FxHashSet<&str>,
) -> BTreeMap<String, ProbedFile> {
    let options = MutationOptions {
        operators: Vec::new(),
        ..MutationOptions::default()
    };
    reach_files
        .par_iter()
        .filter(|path| !planned.contains(path.as_str()))
        .filter_map(|path| {
            let source = std::fs::read_to_string(root.join(path)).ok()?;
            let probed = probe_source(&source, path, &options)?;
            Some((path.clone(), probed))
        })
        .collect()
}
