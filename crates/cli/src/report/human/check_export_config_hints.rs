//! The hints at the end of the "Unused exports" section.
//!
//! The engine adds an `add-to-config` action to each unused export in a
//! generated file or in a shadcn/ui folder. A group of at least
//! [`MIN_FINDINGS`] such findings gets one hint line with the config entry
//! that hides the group.

use std::collections::BTreeSet;
use std::path::Path;

use colored::Colorize;
use fallow_types::output::{
    AddToConfigValue, GENERATED_FILE_EXPORTS_HINT, IssueAction, UI_FOLDER_EXPORTS_HINT,
};
use fallow_types::output_dead_code::UnusedExportFinding;

/// The number of findings of one group below which a hint is noise.
const MIN_FINDINGS: usize = 3;

/// Close the "Unused exports" section: the source and test split, then the
/// config hints. Each line goes before the blank line that ends the section.
pub(in crate::report::human) fn finish_section(
    lines: &mut Vec<String>,
    exports: &[UnusedExportFinding],
    root: &Path,
) {
    super::super::insert_test_src_split(lines, exports, root, |e| &e.export.path);
    for hint in config_hints(exports) {
        let line = format!("  {}", hint.dimmed());
        if lines.last().is_some_and(String::is_empty) {
            let position = lines.len() - 1;
            lines.insert(position, line);
        } else {
            lines.push(line);
        }
    }
}

/// The hint of each group with enough findings: generated files first, then
/// each shadcn/ui folder in path order.
fn config_hints(exports: &[UnusedExportFinding]) -> Vec<String> {
    let mut generated_files: BTreeSet<&str> = BTreeSet::new();
    let mut generated_findings = 0_usize;
    let mut ui_folders: Vec<(&str, usize)> = Vec::new();
    for finding in exports {
        let Some((description, file)) = ignore_exports_rule(finding) else {
            continue;
        };
        if description == GENERATED_FILE_EXPORTS_HINT {
            generated_files.insert(file);
            generated_findings += 1;
        } else if description == UI_FOLDER_EXPORTS_HINT {
            match ui_folders.iter_mut().find(|(pattern, _)| *pattern == file) {
                Some((_, count)) => *count += 1,
                None => ui_folders.push((file, 1)),
            }
        }
    }

    let mut hints = Vec::new();
    if generated_findings >= MIN_FINDINGS {
        hints.push(generated_hint(generated_findings, &generated_files));
    }
    ui_folders.sort_unstable();
    for (pattern, count) in ui_folders {
        if count >= MIN_FINDINGS {
            hints.push(format!(
                "{count} of these exports are in the shadcn/ui folder `{folder}`. To keep each part of a copied component, add to the fallow config: {entry}",
                folder = pattern.trim_end_matches("/**"),
                entry = ignore_exports_entry(pattern),
            ));
        }
    }
    hints
}

/// The description and the file pattern of the `ignoreExports` action.
fn ignore_exports_rule(finding: &UnusedExportFinding) -> Option<(&str, &str)> {
    finding.actions.iter().find_map(|action| {
        let IssueAction::AddToConfig(add) = action else {
            return None;
        };
        let AddToConfigValue::ExportsRules(rules) = &add.value else {
            return None;
        };
        Some((add.description.as_str(), rules.first()?.file.as_str()))
    })
}

fn generated_hint(findings: usize, files: &BTreeSet<&str>) -> String {
    let count = files.len();
    let noun = if count == 1 { "file" } else { "files" };
    let lead = format!(
        "{findings} of these exports are in {count} generated {noun}. A generator writes generated code again, so `fallow fix` keeps it."
    );
    match shared_pattern(files) {
        Some(pattern) => format!(
            "{lead} To hide these findings, add to the fallow config: {entry}. Check that the pattern matches only generated files.",
            entry = ignore_exports_entry(&pattern),
        ),
        None => format!(
            "{lead} To hide these findings, add an `ignoreExports` entry with `\"exports\": [\"*\"]` for each file."
        ),
    }
}

/// The first parts of a compound extension that people give to files that
/// they write. A pattern from such an extension matches too many files.
const WRITTEN_FILE_PARTS: &[&str] = &[
    "test",
    "spec",
    "stories",
    "story",
    "d",
    "config",
    "module",
    "component",
    "service",
    "min",
];

/// One pattern for every file: the file itself when there is one, or
/// `**/*<suffix>` when every file name ends in the same compound extension,
/// such as `.graphql.ts`. A path with a glob character gives no pattern.
fn shared_pattern(files: &BTreeSet<&str>) -> Option<String> {
    let mut names = files.iter();
    let first = names.next()?;
    if files.len() == 1 {
        return (!first.contains(['[', ']', '{', '}', '*', '?'])).then(|| (*first).to_string());
    }
    let suffix = compound_extension(first)?;
    names
        .all(|file| compound_extension(file) == Some(suffix))
        .then(|| format!("**/*{suffix}"))
}

/// The part of the file name from its first dot, when it has two dots or
/// more and does not start with a part of [`WRITTEN_FILE_PARTS`]:
/// `.graphql.ts` of `src/messages.graphql.ts`.
fn compound_extension(file: &str) -> Option<&str> {
    let name = file.rsplit('/').next()?;
    let suffix = &name[name.find('.')?..];
    let first_part = suffix[1..].split('.').next()?;
    (suffix.matches('.').count() >= 2 && !WRITTEN_FILE_PARTS.contains(&first_part))
        .then_some(suffix)
}

fn ignore_exports_entry(pattern: &str) -> String {
    format!("\"ignoreExports\": [{{ \"file\": \"{pattern}\", \"exports\": [\"*\"] }}]")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn files<'a>(paths: &[&'a str]) -> BTreeSet<&'a str> {
        paths.iter().copied().collect()
    }

    #[test]
    fn files_with_one_compound_extension_share_a_pattern() {
        assert_eq!(
            shared_pattern(&files(&[
                "apps/server/src/a.graphql.ts",
                "packages/x/b.graphql.ts"
            ])),
            Some("**/*.graphql.ts".to_string())
        );
    }

    #[test]
    fn one_file_is_its_own_pattern() {
        assert_eq!(
            shared_pattern(&files(&["src/routeTree.gen.ts"])),
            Some("src/routeTree.gen.ts".to_string())
        );
    }

    #[test]
    fn files_with_other_names_share_no_pattern() {
        assert_eq!(
            shared_pattern(&files(&["src/a.graphql.ts", "src/b.ts"])),
            None
        );
        assert_eq!(shared_pattern(&files(&["src/a.ts", "src/b.ts"])), None);
    }

    #[test]
    fn an_extension_of_written_files_gives_no_pattern() {
        assert_eq!(
            shared_pattern(&files(&[
                "src/generated/a.test.ts",
                "src/generated/b.test.ts"
            ])),
            None,
            "`**/*.test.ts` matches the tests that people write"
        );
        assert_eq!(shared_pattern(&files(&["src/app/[id]/x.gen.ts"])), None);
    }

    #[test]
    fn the_generated_hint_gives_the_config_entry() {
        let hint = generated_hint(67, &files(&["src/a.graphql.ts", "src/b.graphql.ts"]));
        assert!(hint.starts_with("67 of these exports are in 2 generated files."));
        assert!(
            hint.contains(r#""ignoreExports": [{ "file": "**/*.graphql.ts", "exports": ["*"] }]"#)
        );
    }
}
