//! Entry patterns for the file arguments of package.json scripts and CI
//! commands.

/// The entry-pattern source of the files that a built-in test runner runs
/// from a script or CI command (`node --test`, `bun test`, `deno test`).
/// Its entries have the test role.
pub const SCRIPT_TEST_ENTRY_SOURCE: &str = "script-tests";

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
/// like a package entry: `node scripts/build` runs `scripts/build.js`.
pub fn push_script_entry_patterns(
    entry_patterns: &mut Vec<(crate::plugins::PathRule, String)>,
    root: &std::path::Path,
    prefix: &str,
    entry_files: &[String],
    test_entry_files: &[String],
    ci_sources: Option<&rustc_hash::FxHashMap<String, String>>,
) {
    let source_of = |entry: &String| {
        ci_sources
            .and_then(|sources| sources.get(entry))
            .map_or("scripts", String::as_str)
    };
    let sources = entry_files
        .iter()
        .map(|entry| (entry, source_of(entry)))
        .chain(
            test_entry_files
                .iter()
                .map(|entry| (entry, SCRIPT_TEST_ENTRY_SOURCE)),
        );
    for (entry, source) in sources {
        if let Some(pat) = crate::scripts::normalize_script_entry_pattern(prefix, entry) {
            let pat = resolve_literal_entry(root, pat);
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
        &ci_analysis.entry_files,
        &ci_analysis.test_entry_files,
        Some(&ci_analysis.entry_file_sources),
    );
}

/// Replace a literal entry pattern with the project-relative path of the
/// source file that it names. A glob, or a reference that names no file,
/// stays as it is.
fn resolve_literal_entry(root: &std::path::Path, pattern: String) -> String {
    if crate::scripts::is_glob(&pattern) {
        return pattern;
    }
    crate::discover::probe_runnable_file(&root.join(&pattern))
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
        push_script_entry_patterns(&mut out, root, prefix, &entries, &[], None);
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
