//! The output-directory step of package entry resolution: map an entry in
//! build output back to its source file.

use super::*;

/// Outcome of the output-directory mapping step.
///
/// `ShortCircuit` means an output-dir branch applied and carries the resolved
/// entry (which may itself be `None` when validation rejected the candidate);
/// `Continue` signals that filesystem probing should proceed.
pub(in crate::discover) enum OutputDirEntry {
    ShortCircuit(Option<EntryPoint>),
    Continue,
}

/// Map an output-directory entry back to a source file, short-circuiting resolution.
pub(in crate::discover) fn resolve_entry_via_output_dir(
    base: &Path,
    entry: &str,
    canonical_root: &Path,
    source: EntryPointSource,
    mut skipped_entries: Option<&mut FxHashMap<String, usize>>,
    targets: EntryTargets<'_>,
) -> OutputDirEntry {
    if !targets.probe.may_map_to_source(entry) {
        return OutputDirEntry::Continue;
    }
    let resolution = targets
        .output_map
        .resolve_source_for_entry(entry, SOURCE_EXTENSIONS);
    match &resolution {
        TsconfigOutputResolution::Resolved(source_path) => {
            return OutputDirEntry::ShortCircuit(validated_entry_point(
                source_path,
                canonical_root,
                entry,
                source,
                skipped_entries.as_deref_mut(),
            ));
        }
        TsconfigOutputResolution::ConfiguredButUnresolved => return OutputDirEntry::Continue,
        TsconfigOutputResolution::ConfiguredWithoutSource
        | TsconfigOutputResolution::Unconfigured => {}
    }

    let is_discovered = |path: &Path| {
        targets
            .discovered
            .is_none_or(|is_discovered| is_discovered(path))
    };
    // A source candidate that resolves out of the project counts as missing.
    let is_file = |path: &Path| project_read::is_probed_project_file(canonical_root, path);
    let mapped =
        output_entry_to_source_path_where(base, entry, SOURCE_EXTENSIONS, is_file, is_discovered);
    if matches!(
        resolution,
        TsconfigOutputResolution::ConfiguredWithoutSource
    ) {
        // The tsconfig claims the output but names no file, so a bundler can
        // own it. Only an analyzed source file under `src/` counts.
        return match mapped.filter(|source_path| is_discovered(source_path)) {
            Some(source_path) => OutputDirEntry::ShortCircuit(validated_entry_point(
                &source_path,
                canonical_root,
                entry,
                source,
                skipped_entries,
            )),
            None => OutputDirEntry::Continue,
        };
    }
    if let Some(source_path) = mapped {
        return OutputDirEntry::ShortCircuit(validated_entry_point(
            &source_path,
            canonical_root,
            entry,
            source,
            skipped_entries.as_deref_mut(),
        ));
    }

    if is_entry_in_output_dir(entry)
        && let Some(source_path) = try_source_index_fallback(base, canonical_root)
    {
        tracing::info!(
            entry = %entry,
            fallback = %source_path.display(),
            "package.json entry resolves to an ignored output directory; falling back to source index"
        );
        return OutputDirEntry::ShortCircuit(validated_entry_point(
            &source_path,
            canonical_root,
            entry,
            source,
            skipped_entries,
        ));
    }

    OutputDirEntry::Continue
}
