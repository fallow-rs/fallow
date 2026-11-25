//! The `overrides[].files` globs that matched no project file, as
//! `override-files-unmatched` entries (issue #5227).

use fallow_config::{
    PartialRulesConfig, ResolvedConfig, WorkspaceDiagnosticKind, project_level_override_rules,
};
use fallow_types::workspace::{
    OVERRIDE_GLOB_REASON_DIRECTORY, OVERRIDE_GLOB_REASON_GLOB_UNMATCHED,
    OVERRIDE_GLOB_REASON_NOT_A_PATH, OVERRIDE_GLOB_REASON_PATH_NOT_FOUND,
};

/// The characters that make a pattern a glob instead of a literal path.
const GLOB_CHARACTERS: [char; 4] = ['*', '?', '[', '{'];

/// One entry per `overrides[].files` glob that matched no project file in
/// the latest dead-code pass, in config order.
///
/// An override whose every rule is a project-level rule gets no entry: the
/// `override-rule-ineffective` entry of each rule already says that the glob
/// does not matter, and it names the key that works.
pub(super) fn unmatched_override_globs(config: &ResolvedConfig) -> Vec<WorkspaceDiagnosticKind> {
    let globs = config.override_globs();
    let rules: Vec<Vec<String>> = config
        .overrides
        .iter()
        .map(|entry| per_file_rules(&entry.rules))
        .collect();
    config
        .ignore_list_effect
        .override_files
        .ineffective(&globs)
        .into_iter()
        .filter_map(|glob| {
            let entry = config.overrides.get(glob.entry)?;
            let rules = rules.get(glob.entry)?;
            let only_project_level =
                rules.is_empty() && !project_level_override_rules(&entry.rules).is_empty();
            if only_project_level {
                return None;
            }
            Some(WorkspaceDiagnosticKind::OverrideFilesUnmatched {
                pattern: glob.pattern.to_owned(),
                files: entry.files.clone(),
                rules: rules.clone(),
                reason: reason(config, glob.pattern).to_owned(),
            })
        })
        .collect()
}

/// The canonical names of the rules that `rules` sets and that apply per
/// file, in the order of the rules config.
fn per_file_rules(rules: &PartialRulesConfig) -> Vec<String> {
    let project_level = project_level_override_rules(rules);
    let Ok(serde_json::Value::Object(map)) = serde_json::to_value(rules) else {
        return Vec::new();
    };
    map.into_iter()
        .map(|(name, _)| name)
        .filter(|name| !project_level.contains(&name.as_str()))
        .collect()
}

/// Why `pattern` matched no file. A folder comes first, because a folder
/// name without a `/` also looks like a package name.
fn reason(config: &ResolvedConfig, pattern: &str) -> &'static str {
    let path = pattern.strip_prefix("./").unwrap_or(pattern);
    if !path.is_empty() && config.root.join(path).is_dir() {
        return OVERRIDE_GLOB_REASON_DIRECTORY;
    }
    if path.contains(GLOB_CHARACTERS) {
        return OVERRIDE_GLOB_REASON_GLOB_UNMATCHED;
    }
    if looks_like_a_name(path) {
        return OVERRIDE_GLOB_REASON_NOT_A_PATH;
    }
    OVERRIDE_GLOB_REASON_PATH_NOT_FOUND
}

/// Whether `path` looks like a package or binary name: `@scope/name`, or one
/// segment without a file extension. A package name has no capital letters,
/// so a root file such as `Makefile` or `LICENSE` stays a path.
fn looks_like_a_name(path: &str) -> bool {
    if path.is_empty() || path.chars().any(char::is_uppercase) {
        return false;
    }
    if let Some(scoped) = path.strip_prefix('@') {
        return scoped.split('/').count() == 2;
    }
    !path.contains('/') && !path.contains('.')
}

#[cfg(test)]
mod tests {
    use super::looks_like_a_name;

    #[test]
    fn names_and_paths() {
        assert!(looks_like_a_name("typedoc"));
        assert!(looks_like_a_name("@golar/vue"));
        assert!(!looks_like_a_name("dot.exe"));
        assert!(!looks_like_a_name("vuetify/lib/styles/layers.css"));
        assert!(!looks_like_a_name("@scope/pkg/file.ts"));
        assert!(!looks_like_a_name("Makefile"));
        assert!(!looks_like_a_name("LICENSE"));
        assert!(!looks_like_a_name(""));
    }
}
