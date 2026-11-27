//! The entry points that the scripts of a package reference. The program of
//! each command decides how a file argument resolves (issue #5370).

use std::path::Path;

use fallow_config::TsconfigOutputMap;
use fallow_types::discover::{EntryPoint, EntryPointSource};
use rustc_hash::{FxHashMap, FxHashSet};

use super::{
    EntryProbe, EntryTargets, PackageEntryBuckets, ScriptWorkspaces, entry_has_parent_dir,
    resolve_entry_path_with_output_map, runtime_package_script_names_with_seeds,
};
use crate::scripts::{IgnoredCommandEntries, ScriptCatalog};

/// The package whose scripts add entry points.
pub(super) struct ScriptPackage<'a> {
    pub(super) root: &'a Path,
    pub(super) canonical_root: &'a Path,
    pub(super) output_map: &'a TsconfigOutputMap,
    pub(super) workspaces: ScriptWorkspaces<'a>,
}

impl ScriptPackage<'_> {
    /// The directory of the package relative to the project root, with `/`
    /// separators. `None` for a package outside the project root.
    fn dir(&self) -> Option<String> {
        self.root
            .strip_prefix(self.workspaces.project_root)
            .ok()
            .map(|dir| dir.to_string_lossy().replace('\\', "/"))
    }

    /// Resolve a file reference of the script `script` of the package. A
    /// reference outside the package, such as `../web/scripts/a.ts` from
    /// `yarn workspace web node scripts/a.ts`, resolves from the project
    /// root.
    ///
    /// `probe` decides whether a directory names its index file: only for a
    /// module that a script runtime runs. A config file reference is a
    /// package-style reference.
    fn resolve(
        &self,
        file_ref: &str,
        script: &str,
        dir: Option<&str>,
        probe: EntryProbe,
        skipped_entries: &mut FxHashMap<String, usize>,
    ) -> Option<EntryPoint> {
        if entry_has_parent_dir(file_ref)
            && let Some(project_ref) =
                dir.and_then(|dir| crate::scripts::normalize_script_entry_pattern(dir, file_ref))
        {
            let project_root = self.workspaces.project_root;
            let canonical_project_root =
                dunce::canonicalize(project_root).unwrap_or_else(|_| project_root.to_path_buf());
            return resolve_entry_path_with_output_map(
                project_root,
                &project_ref,
                &canonical_project_root,
                EntryPointSource::package_json_script(script),
                Some(skipped_entries),
                EntryTargets {
                    output_map: &TsconfigOutputMap::from_project(project_root),
                    discovered: self.workspaces.discovered,
                    probe,
                },
            );
        }
        resolve_entry_path_with_output_map(
            self.root,
            file_ref,
            self.canonical_root,
            EntryPointSource::package_json_script(script),
            Some(skipped_entries),
            EntryTargets {
                output_map: self.output_map,
                discovered: self.workspaces.discovered,
                probe,
            },
        )
    }
}

/// Push the entries that the scripts of `package` reference. A file that a
/// runtime script runs is a runtime entry: the `start` lifecycle, and the
/// scripts that a runtime script of another package calls. Other script
/// files and every config file are support entries.
#[expect(
    clippy::disallowed_types,
    reason = "API matches serde-deserialized package.json scripts map"
)]
pub(super) fn push_script_entries(
    package: &ScriptPackage<'_>,
    scripts: &std::collections::HashMap<String, String>,
    ignored: IgnoredCommandEntries<'_>,
    entries: &mut PackageEntryBuckets<'_>,
    skipped_entries: &mut FxHashMap<String, usize>,
) {
    let dir_buf = package.dir();
    let dir = dir_buf.as_deref();
    let no_seeds = FxHashSet::default();
    let seeds = dir
        .and_then(|dir| package.workspaces.runtime_seeds.get(dir))
        .unwrap_or(&no_seeds);
    let runtime_scripts = runtime_package_script_names_with_seeds(scripts, seeds);
    let catalog = ScriptCatalog::from_scripts(scripts).with_workspaces(
        std::sync::Arc::clone(package.workspaces.packages),
        dir.unwrap_or_default(),
    );
    // Key order: the entry dedup keeps the first script that runs a file.
    let sorted: std::collections::BTreeMap<_, _> = scripts.iter().collect();
    for (script_name, script_value) in sorted {
        let refs = package_script_refs(script_value, &catalog, ignored);
        for file_ref in refs.inheritable {
            let probe = if refs.modules.binary_search(&file_ref).is_ok() {
                EntryProbe::ScriptModule
            } else {
                EntryProbe::ScriptPath
            };
            if let Some(ep) = package.resolve(&file_ref, script_name, dir, probe, skipped_entries) {
                if runtime_scripts.contains(script_name) {
                    entries.runtime.push(ep);
                } else {
                    entries.support.push(ep);
                }
            }
        }
        for config_ref in refs.support {
            if let Some(ep) = package.resolve(
                &config_ref,
                script_name,
                dir,
                EntryProbe::Package,
                skipped_entries,
            ) {
                entries.support.push(ep);
            }
        }
    }
}

struct PackageScriptRefs {
    inheritable: Vec<String>,
    support: Vec<String>,
    /// The sorted subset of `inheritable` that a script runtime runs as
    /// modules.
    modules: Vec<String>,
}

/// Collect the file references of one script. `catalog` holds the scripts of
/// the same package, so a call such as `npm run lint -- src/a.ts` resolves to
/// the command that the package manager runs.
fn package_script_refs(
    script: &str,
    catalog: &ScriptCatalog,
    ignored: IgnoredCommandEntries<'_>,
) -> PackageScriptRefs {
    let mut inheritable = Vec::new();
    let mut support = Vec::new();
    let mut modules = Vec::new();
    for command in crate::scripts::parse_script_with_catalog(script, catalog) {
        if command.runs_modules {
            modules.extend_from_slice(command.entry_files(ignored));
        }
        inheritable.extend_from_slice(command.entry_files(ignored));
        support.extend(command.config_args);
    }
    let refs = crate::discover::parse_scripts::extract_script_file_refs_with_modules(
        script,
        crate::discover::parse_scripts::CommandRefContext {
            ignored,
            scripts: catalog,
        },
    );
    inheritable.extend(refs.paths);
    modules.extend(refs.modules);
    modules.sort_unstable();
    modules.dedup();
    support.sort_unstable();
    support.dedup();
    inheritable.retain(|path| support.binary_search(path).is_err());
    inheritable.sort_unstable();
    inheritable.dedup();
    PackageScriptRefs {
        inheritable,
        support,
        modules,
    }
}
