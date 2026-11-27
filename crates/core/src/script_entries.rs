//! Entry patterns for the file arguments of package.json scripts and CI
//! commands.

/// The entry-pattern source of the files that a built-in test runner runs
/// from a script or CI command (`node --test`, `bun test`, `deno test`).
/// Its entries have the test role.
pub const SCRIPT_TEST_ENTRY_SOURCE: &str = "script-tests";

/// The file arguments of script, task or CI commands that become entry
/// points.
#[derive(Clone, Copy)]
pub struct ScriptEntryFiles<'a> {
    /// Every file argument that becomes an entry point.
    pub entries: &'a [String],
    /// The files that a built-in test runner runs.
    pub tests: &'a [String],
    /// The subset of `entries` that a script runtime runs as modules
    /// (`node scripts/build`).
    pub modules: &'a [String],
}

impl<'a> ScriptEntryFiles<'a> {
    /// The entry files of the analysis of package.json scripts and tasks.
    #[must_use]
    pub fn of(analysis: &'a crate::scripts::ScriptAnalysis) -> Self {
        Self {
            entries: &analysis.entry_files,
            tests: &analysis.test_entry_files,
            modules: &analysis.module_entry_files,
        }
    }
}

/// Push the entry patterns of script file arguments. A file that a built-in
/// test runner runs is also a test entry, so the code that it imports counts
/// as reached by tests. The `scripts` rule comes first, so it stays the
/// reported source of the entry.
///
/// `ci_sources` maps an entry of a CI file to the path of that CI file. The
/// CI file path is then the source of the entry, so `list --entry-points`
/// names the file that runs it.
///
/// A file argument without a glob resolves against `root`, the project root,
/// like a package entry: `node scripts/build` runs `scripts/build.js`. Only a
/// module that a script runtime runs resolves a directory to its index file
/// (`node scripts/build` runs `scripts/build/index.js`). The directory of a
/// file runner such as `vitest run src/lib/` is a filter.
pub fn push_script_entry_patterns(
    entry_patterns: &mut Vec<(crate::plugins::PathRule, String)>,
    root: &std::path::Path,
    prefix: &str,
    files: ScriptEntryFiles<'_>,
    ci_sources: Option<&rustc_hash::FxHashMap<String, String>>,
) {
    let source_of = |entry: &String| {
        ci_sources
            .and_then(|sources| sources.get(entry))
            .map_or("scripts", String::as_str)
    };
    let sources = files
        .entries
        .iter()
        .map(|entry| (entry, source_of(entry)))
        .chain(
            files
                .tests
                .iter()
                .map(|entry| (entry, SCRIPT_TEST_ENTRY_SOURCE)),
        );
    for (entry, source) in sources {
        if let Some(pat) = crate::scripts::normalize_script_entry_pattern(prefix, entry) {
            let module = files.modules.contains(entry);
            let pat = resolve_literal_entry(root, pat, module);
            entry_patterns.push((crate::plugins::PathRule::new(pat), source.to_string()));
        }
    }
}

/// Analyze CI config files for binary invocations and merge the results.
pub fn analyze_ci_scripts(
    config: &fallow_config::ResolvedConfig,
    bin_map: &rustc_hash::FxHashMap<String, String>,
    all_dep_set: &rustc_hash::FxHashSet<String>,
    all_scripts: &crate::scripts::ScriptCatalog,
    plugin_result: &mut crate::plugins::AggregatedPluginResult,
) {
    let ci_analysis = crate::scripts::ci::analyze_ci_files(
        &config.root,
        bin_map,
        all_dep_set,
        all_scripts,
        crate::scripts::IgnoredCommandEntries::new(&config.ignore_command_entries),
    );
    plugin_result
        .script_used_packages
        .extend(ci_analysis.used_packages);
    push_script_entry_patterns(
        &mut plugin_result.entry_patterns,
        &config.root,
        "",
        ScriptEntryFiles {
            entries: &ci_analysis.entry_files,
            tests: &ci_analysis.test_entry_files,
            modules: &ci_analysis.module_entry_files,
        },
        Some(&ci_analysis.entry_file_sources),
    );
}

/// Replace a literal entry pattern with the project-relative path of the
/// source file that it names. A glob, or a reference that names no file,
/// stays as it is. A `module` pattern can also name a directory with an
/// index file.
fn resolve_literal_entry(root: &std::path::Path, pattern: String, module: bool) -> String {
    if crate::scripts::is_glob(&pattern) {
        return pattern;
    }
    let path = root.join(&pattern);
    crate::discover::probe_runnable_file(&path)
        .or_else(|| {
            module
                .then(|| crate::discover::probe_directory_index(&path))
                .flatten()
        })
        .and_then(|file| {
            file.strip_prefix(root)
                .ok()
                .map(|rel| rel.to_string_lossy().replace('\\', "/"))
        })
        .unwrap_or(pattern)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn patterns(root: &std::path::Path, prefix: &str, entries: &[&str]) -> Vec<String> {
        let entries: Vec<String> = entries.iter().map(|entry| (*entry).to_string()).collect();
        let mut out = Vec::new();
        let files = ScriptEntryFiles {
            entries: &entries,
            tests: &[],
            modules: &[],
        };
        push_script_entry_patterns(&mut out, root, prefix, files, None);
        out.into_iter().map(|(rule, _)| rule.pattern).collect()
    }

    #[test]
    fn extensionless_entries_resolve_to_source_files() {
        let dir = tempfile::tempdir().expect("temp project");
        let root = dir.path();
        std::fs::create_dir_all(root.join("apps/api/scripts")).expect("dirs");
        std::fs::write(root.join("apps/api/scripts/seed.ts"), "").expect("seed");
        std::fs::write(root.join("apps/api/scripts/run.mjs"), "").expect("run");
        std::fs::write(root.join("apps/api/scripts/build.js"), "").expect("build");
        std::fs::write(root.join("apps/api/scripts/build.prod.js"), "").expect("prod");
        std::fs::write(root.join("apps/api/scripts/gen.ts"), "").expect("gen");

        assert_eq!(
            patterns(
                root,
                "apps/api",
                &[
                    "scripts/seed",
                    "./scripts/run",
                    "scripts/build.prod",
                    "scripts/gen.js"
                ]
            ),
            [
                "apps/api/scripts/seed.ts",
                "apps/api/scripts/run.mjs",
                "apps/api/scripts/build.prod.js",
                "apps/api/scripts/gen.ts"
            ]
        );
    }

    #[test]
    fn non_runnable_files_with_the_same_stem_are_not_entries() {
        let dir = tempfile::tempdir().expect("temp project");
        std::fs::create_dir_all(dir.path().join("scripts")).expect("dirs");
        std::fs::write(dir.path().join("scripts/theme.css"), "").expect("css");
        std::fs::write(dir.path().join("scripts/page.html"), "").expect("html");
        assert_eq!(
            patterns(dir.path(), "", &["scripts/theme", "scripts/page"]),
            ["scripts/theme", "scripts/page"]
        );
    }

    #[test]
    fn a_non_source_extension_is_never_replaced() {
        let dir = tempfile::tempdir().expect("temp project");
        std::fs::create_dir_all(dir.path().join("scripts")).expect("dirs");
        std::fs::write(dir.path().join("scripts/deploy.ts"), "").expect("deploy");
        std::fs::write(dir.path().join("tsconfig.ts"), "").expect("tsconfig");
        assert_eq!(
            patterns(dir.path(), "", &["scripts/deploy.sh", "tsconfig.json"]),
            ["scripts/deploy.sh", "tsconfig.json"]
        );
    }

    #[test]
    fn only_a_runtime_module_directory_resolves_to_its_index_file() {
        let dir = tempfile::tempdir().expect("temp project");
        std::fs::create_dir_all(dir.path().join("scripts/gen")).expect("dirs");
        std::fs::write(dir.path().join("scripts/gen/index.js"), "").expect("index");
        let entries = ["scripts/gen".to_string(), "./scripts/gen/".to_string()];
        let mut out = Vec::new();
        let files = ScriptEntryFiles {
            entries: &entries,
            tests: &[],
            modules: &entries[..1],
        };
        push_script_entry_patterns(&mut out, dir.path(), "", files, None);
        let patterns: Vec<String> = out.into_iter().map(|(rule, _)| rule.pattern).collect();
        assert_eq!(patterns, ["scripts/gen/index.js", "scripts/gen"]);
    }

    #[test]
    fn globs_missing_files_and_directories_keep_their_pattern() {
        let dir = tempfile::tempdir().expect("temp project");
        std::fs::create_dir_all(dir.path().join("src/lib")).expect("dirs");
        std::fs::write(dir.path().join("src/lib/index.ts"), "").expect("index");
        assert_eq!(
            patterns(
                dir.path(),
                "",
                &["test/**/*.test", "scripts/missing", "src/lib/"]
            ),
            ["test/**/*.test", "scripts/missing", "src/lib"]
        );
    }
}
