//! Large-function collection for health reports.

/// Collect functions exceeding 60 LOC when the unit size risk profile warrants it.
///
/// Only populated when `very_high_risk >= 3%` in the unit size profile. Sorted
/// by line count descending.
pub(super) struct LargeFunctionInput<'a> {
    pub(super) vital_signs: &'a fallow_output::VitalSigns,
    pub(super) modules: &'a [crate::source::ModuleInfo],
    pub(super) file_paths:
        &'a rustc_hash::FxHashMap<crate::discover::FileId, &'a std::path::PathBuf>,
    pub(super) config_root: &'a std::path::Path,
    pub(super) ignore_set: &'a globset::GlobSet,
    pub(super) changed_files: Option<&'a rustc_hash::FxHashSet<std::path::PathBuf>>,
    pub(super) ws_roots: Option<&'a [std::path::PathBuf]>,
}

pub(super) fn collect_large_functions(
    input: &LargeFunctionInput<'_>,
) -> Vec<fallow_output::LargeFunctionEntry> {
    let dominated = input
        .vital_signs
        .unit_size_profile
        .as_ref()
        .is_some_and(|p| p.very_high_risk >= 3.0);
    if !dominated {
        return Vec::new();
    }

    let mut entries = Vec::new();
    for module in input.modules {
        let Some(&path) = input.file_paths.get(&module.file_id) else {
            continue;
        };
        let relative = path.strip_prefix(input.config_root).unwrap_or(path);
        if input.ignore_set.is_match(relative) {
            continue;
        }
        if let Some(changed) = input.changed_files
            && !changed.contains(path.as_path())
        {
            continue;
        }
        if let Some(ws) = input.ws_roots
            && !ws.iter().any(|r| path.starts_with(r))
        {
            continue;
        }
        for func in &module.complexity {
            if func.line_count > 60 {
                entries.push(fallow_output::LargeFunctionEntry {
                    path: path.clone(),
                    name: func.name.clone(),
                    line: func.line,
                    line_count: func.line_count,
                });
            }
        }
    }
    entries.sort_by_key(|e| std::cmp::Reverse(e.line_count));
    entries
}
