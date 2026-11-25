//! The accessor, the reasons and the message text of the
//! `override-files-unmatched` kind (issue #5227).

use super::WorkspaceDiagnosticKind;

/// The glob has glob characters and matched no project file.
pub const OVERRIDE_GLOB_REASON_GLOB_UNMATCHED: &str = "glob-unmatched";
/// The glob is the path of a folder. A glob matches only files.
pub const OVERRIDE_GLOB_REASON_DIRECTORY: &str = "directory";
/// The glob is a literal path, and no project file has that path.
pub const OVERRIDE_GLOB_REASON_PATH_NOT_FOUND: &str = "path-not-found";
/// The glob looks like a package or binary name, not like a file path.
pub const OVERRIDE_GLOB_REASON_NOT_A_PATH: &str = "not-a-path";

/// The rule whose finding is on the importing file, not on the import target.
const UNRESOLVED_IMPORTS: &str = "unresolved-imports";

impl WorkspaceDiagnosticKind {
    /// The glob, the `files` globs, the per-file rules and the reason of an
    /// `override-files-unmatched` entry. `None` for every other kind.
    #[must_use]
    pub fn unmatched_override_glob(&self) -> Option<(&str, &[String], &[String], &str)> {
        match self {
            Self::OverrideFilesUnmatched {
                pattern,
                files,
                rules,
                reason,
            } => Some((
                pattern.as_str(),
                files.as_slice(),
                rules.as_slice(),
                reason.as_str(),
            )),
            _ => None,
        }
    }

    /// The next step of an `override-files-unmatched` entry, as one clause
    /// for a one-line note. `None` for every other kind.
    ///
    /// The clause holds no config value, so a note can group the globs with
    /// the same clause, and a Markdown line needs no escape for it.
    #[must_use]
    pub fn unmatched_override_glob_hint(&self) -> Option<&'static str> {
        let (_, _, rules, reason) = self.unmatched_override_glob()?;
        Some(match reason {
            OVERRIDE_GLOB_REASON_DIRECTORY => {
                "a glob matches only files; add /** to the folder path"
            }
            OVERRIDE_GLOB_REASON_GLOB_UNMATCHED => {
                "globs are project-root-relative; check for typos, or remove the glob"
            }
            _ if has_rule(rules, UNRESOLVED_IMPORTS) => {
                "the override applies to the importing file; add the import specifier to \
                 ignoreUnresolvedImports"
            }
            OVERRIDE_GLOB_REASON_NOT_A_PATH => {
                "overrides[].files takes file globs; use ignoreDependencies for a package, \
                 ignoreBinaries for a binary"
            }
            _ => {
                "no file has this path; check for typos, and use the path of the file that has \
                 the finding"
            }
        })
    }
}

fn has_rule(rules: &[String], rule: &str) -> bool {
    rules.iter().any(|name| name == rule)
}

/// The message of an `override-files-unmatched` entry.
pub(super) fn override_glob_message(
    pattern: &str,
    files: &[String],
    rules: &[String],
    reason: &str,
) -> String {
    let mut sentences = vec![format!(
        "overrides[].files glob '{pattern}' matched no project file in this run, so the \
         override has no effect for it."
    )];
    if files.len() > 1 {
        sentences.push(format!(
            "The override has the globs [{}].",
            files.join(", ")
        ));
    }
    match reason {
        OVERRIDE_GLOB_REASON_DIRECTORY => {
            sentences.push(format!(
                "'{pattern}' is a folder, and a glob matches only files. Use '{}/**' to match \
                 the files in the folder.",
                pattern.trim_end_matches('/')
            ));
            return sentences.join(" ");
        }
        OVERRIDE_GLOB_REASON_GLOB_UNMATCHED => {
            sentences.push(
                "A glob is relative to the project root. Fix the glob, or remove it from the \
                 config. If the files exist only after a build, ignore this note."
                    .to_owned(),
            );
            return sentences.join(" ");
        }
        OVERRIDE_GLOB_REASON_NOT_A_PATH => sentences.push(
            "`files` takes globs of project files, not package or binary names. To hide a \
             package, add it to `ignoreDependencies`. To hide a binary, add it to \
             `ignoreBinaries`."
                .to_owned(),
        ),
        _ => sentences.push(
            "No project file has this path relative to the project root. Check the path for \
             typos. If the file exists only after a build, ignore this note."
                .to_owned(),
        ),
    }
    sentences.push(if has_rule(rules, UNRESOLVED_IMPORTS) {
        "An override applies to the file that has the finding. For unresolved-imports, that \
         is the importing file, not the import target. To hide one import, add its specifier \
         to `ignoreUnresolvedImports`."
            .to_owned()
    } else {
        "An override applies to the file that has the finding. Use the path of that file, or \
         remove the glob from the config."
            .to_owned()
    });
    sentences.join(" ")
}
