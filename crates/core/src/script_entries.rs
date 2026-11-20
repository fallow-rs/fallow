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
pub fn push_script_entry_patterns(
    entry_patterns: &mut Vec<(crate::plugins::PathRule, String)>,
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
            entry_patterns.push((crate::plugins::PathRule::new(pat), source.to_string()));
        }
    }
}
