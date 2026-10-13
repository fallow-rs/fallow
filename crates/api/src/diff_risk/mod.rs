//! Diff-risk rules of `fallow audit`.
//!
//! The rules compare the base and head text of each changed file. They report
//! a fact about the change, so every finding is introduced. The pass reads the
//! base text through one `git cat-file --batch` process at the merge base of
//! the base ref and `HEAD`.
//!
//! - `weakened-tests`: a new skip or focus marker, fewer test cases across the
//!   changed test files, fewer assertions in a test file, or a less strict
//!   coverage threshold.
//! - `added-suppression`: a new lint, type-check, coverage or Fallow
//!   suppression comment.
//! - `test-only-branch`: a new test-environment branch in a non-test source
//!   file.
//! - `diverged-clone-sibling`: one copy of a base clone group was edited out
//!   of the group, and no sibling changed. Needs the base snapshot.
//! - `added-dependency`: a new `package.json` dependency. Off by default.

pub mod clones;
pub mod text;

use std::path::{Path, PathBuf};
use std::process::Command;

use fallow_config::{ResolvedConfig, RulesConfig, Severity};
use fallow_engine::changed_files::RenamedFile;
use fallow_engine::module_graph::relative_key_path;
use fallow_output::{
    DiffRiskAction, DiffRiskActionType, DiffRiskFinding, DiffRiskLocation, DiffRiskReport,
    DiffRiskRule, DiffRiskSeverity, WeakenedTestsKind,
};
use fallow_types::duplicates::CloneGroup;
use fallow_types::suppress::{IssueKind, Suppression, SuppressionTarget};
use rustc_hash::{FxHashMap, FxHashSet};

use crate::audit_run::{BaseFileReader, BaseRead};
use clones::{BaseCloneGroup, CloneSpan, CloneTexts};

/// Extensions of files that hold JavaScript-style comments, where an added
/// suppression or a test-only branch is meaningful.
const SOURCE_EXTENSIONS: &[&str] = &[
    "js", "jsx", "ts", "tsx", "mjs", "cjs", "mts", "cts", "vue", "svelte", "astro",
];
/// Extensions where a suppression comment is meaningful. Adds styles and
/// markup to [`SOURCE_EXTENSIONS`].
const STYLE_EXTENSIONS: &[&str] = &["css", "scss", "sass", "less", "html"];

/// Where the rule severities come from.
#[derive(Clone, Copy)]
pub enum DiffRiskRules<'a> {
    /// The resolved config, with per-path `overrides`.
    Config(&'a ResolvedConfig),
    /// One rules table for every path.
    Rules(&'a RulesConfig),
}

impl DiffRiskRules<'_> {
    fn for_path(self, abs: &Path) -> RulesConfig {
        match self {
            Self::Config(config) => config.resolve_rules_for_path(abs),
            Self::Rules(rules) => rules.clone(),
        }
    }
}

/// Inputs of one diff-risk pass.
pub struct DiffRiskInput<'a> {
    /// Head analysis root. Finding paths are relative to it.
    pub root: &'a Path,
    /// Resolved base ref of the audit.
    pub base_ref: &'a str,
    /// Absolute paths of the changed files.
    pub changed_files: &'a FxHashSet<PathBuf>,
    /// Renames between base and head.
    pub renames: &'a [RenamedFile],
    /// Rule severities.
    pub rules: DiffRiskRules<'a>,
    /// Clone groups of the base snapshot. `None` turns off
    /// `diverged-clone-sibling`: without a base pass no copy can be compared.
    pub base_clone_groups: Option<&'a [BaseCloneGroup]>,
    /// Instance locations of every head clone group.
    pub head_clone_groups: Vec<Vec<CloneSpan>>,
}

/// The instance locations of a clone group, relative to `root`.
#[must_use]
pub fn clone_spans(group: &CloneGroup, root: &Path) -> Vec<CloneSpan> {
    group
        .instances
        .iter()
        .map(|instance| CloneSpan {
            path: relative_key_path(&instance.file, root),
            start_line: u32::try_from(instance.start_line).unwrap_or(u32::MAX),
            end_line: u32::try_from(instance.end_line).unwrap_or(u32::MAX),
        })
        .collect()
}

/// The base and head text of one changed file.
struct FileTexts {
    /// Head path relative to the analysis root.
    rel: String,
    /// Absolute head path.
    abs: PathBuf,
    /// Base text; `None` when the file is new.
    base: Option<String>,
    /// Head text; `None` when the change deleted the file.
    head: Option<String>,
}

/// A finding before the severity and suppression pass.
struct RawFinding {
    rule: DiffRiskRule,
    kind: Option<WeakenedTestsKind>,
    path: String,
    line: Option<u32>,
    message: String,
    evidence: String,
    related: Vec<DiffRiskLocation>,
}

/// Run every diff-risk rule over the changed files.
///
/// Best-effort: a file that cannot be read is skipped, and a base-reader
/// error stops the scan of the remaining files, so a broken pipe never reads
/// as an empty base file.
#[must_use]
pub fn detect(input: &DiffRiskInput<'_>) -> DiffRiskReport {
    let Ok(git_root) = fallow_engine::changed_files::resolve_git_toplevel(input.root) else {
        return DiffRiskReport::default();
    };
    let Some(reader) = BaseFileReader::spawn(input.root) else {
        return DiffRiskReport::default();
    };
    let mut base = BaseSource {
        git_root: dunce::canonicalize(&git_root).unwrap_or(git_root),
        reader,
        commit: merge_base(input.root, input.base_ref),
    };
    let files = read_files(input, &mut base);

    let mut raw = Vec::new();
    for file in &files {
        let rules = input.rules.for_path(&file.abs);
        detect_file(input.root, file, &rules, &mut raw);
    }
    detect_removed_tests(input.root, &files, &mut raw);
    if let Some(base_groups) = input.base_clone_groups {
        detect_diverged_clones(input, &mut base, base_groups, &files, &mut raw);
    }

    let findings = finalize(input, &files, raw);
    DiffRiskReport { findings }
}

/// Base-side text access: one `git cat-file --batch` reader at the merge
/// base.
struct BaseSource {
    git_root: PathBuf,
    reader: BaseFileReader,
    commit: String,
}

impl BaseSource {
    /// Read the base text of an absolute path. A path outside the repository
    /// reads as an error.
    fn read(&mut self, abs: &Path) -> BaseRead {
        match abs.strip_prefix(&self.git_root) {
            Ok(relative) => self.reader.read(&self.commit, relative),
            Err(_) => BaseRead::Error,
        }
    }
}

/// The merge base of `base_ref` and `HEAD`, or `base_ref` when git cannot
/// resolve one. The changed-file list uses the merge base too, so a base
/// branch that moved on does not read as a change of this branch.
fn merge_base(root: &Path, base_ref: &str) -> String {
    let mut command = Command::new("git");
    command
        .args(["merge-base", "--end-of-options", base_ref, "HEAD"])
        .current_dir(root)
        .stderr(std::process::Stdio::null());
    fallow_engine::changed_files::clear_ambient_git_env(&mut command);
    command
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
        .filter(|sha| !sha.is_empty())
        .unwrap_or_else(|| base_ref.to_owned())
}

/// Whether a diff-risk rule can read this file. The rules read discovered
/// source files (every clone group member is one), test files, test-runner
/// and coverage configs, and package manifests. Other changed files, such as
/// a lockfile or an image, are not read.
fn is_rule_relevant(root: &Path, rel: &str) -> bool {
    fallow_engine::discover::SOURCE_EXTENSIONS.contains(&extension(rel))
        || takes_suppression_comments(rel)
        || is_js_test_code(root, rel)
        || text::is_coverage_config_path(rel)
        || crate::dependency_deltas::is_manifest_path(rel)
}

fn read_files(input: &DiffRiskInput<'_>, source: &mut BaseSource) -> Vec<FileTexts> {
    let rename_sources: FxHashSet<&Path> = input
        .renames
        .iter()
        .map(|rename| rename.from.as_path())
        .collect();
    let rename_targets: FxHashMap<&Path, &Path> = input
        .renames
        .iter()
        .map(|rename| (rename.to.as_path(), rename.from.as_path()))
        .collect();
    let mut paths: Vec<&PathBuf> = input.changed_files.iter().collect();
    paths.sort();

    let mut files = Vec::new();
    for abs in paths {
        // The old path of a rename is not a deletion: its text lives on at
        // the new path, which reads its base text from the old path.
        if rename_sources.contains(abs.as_path()) {
            continue;
        }
        if !abs.starts_with(input.root) {
            continue;
        }
        let base_abs = rename_targets
            .get(abs.as_path())
            .copied()
            .unwrap_or(abs.as_path());
        if !base_abs.starts_with(&source.git_root) {
            continue;
        }
        let rel = relative_key_path(abs, input.root);
        if !is_rule_relevant(input.root, &rel) {
            continue;
        }
        let head = match std::fs::read(abs) {
            Ok(bytes) => Some(String::from_utf8_lossy(&bytes).into_owned()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(_) => continue,
        };
        let base = match source.read(base_abs) {
            BaseRead::Content(base) => Some(base),
            BaseRead::Missing => None,
            BaseRead::Error => break,
        };
        if base.is_none() && head.is_none() {
            continue;
        }
        files.push(FileTexts {
            rel,
            abs: abs.clone(),
            base,
            head,
        });
    }
    files
}

fn extension(rel: &str) -> &str {
    let name = rel.rsplit('/').next().unwrap_or(rel);
    name.rsplit_once('.').map_or("", |(_, ext)| ext)
}

fn is_source_file(rel: &str) -> bool {
    SOURCE_EXTENSIONS.contains(&extension(rel))
}

fn takes_suppression_comments(rel: &str) -> bool {
    is_source_file(rel) || STYLE_EXTENSIONS.contains(&extension(rel))
}

/// A JavaScript-family test file, where the test rules apply. A test file in
/// another language, such as a Rust integration test, can hold JavaScript
/// test text in a string, so it is not read.
fn is_js_test_code(root: &Path, rel: &str) -> bool {
    is_source_file(rel) && fallow_engine::test_paths::is_test_code_path_str(root, rel)
}

/// A test file, or a type test of the `tsd` and Vitest conventions
/// (`test-d/`, `*.test-d.ts`).
fn is_type_or_test_file(root: &Path, rel: &str) -> bool {
    fallow_engine::test_paths::is_test_path_str(root, rel)
        || rel.split('/').any(|segment| segment == "test-d")
        || rel.contains(".test-d.")
}

/// Directory names of test fixtures and generated code. A tool writes these
/// files, or a test reads them as data, so their suppression comments are
/// not a choice of the change author.
const FIXTURE_OR_GENERATED_DIRS: &[&str] = &[
    "fixture",
    "fixtures",
    "__fixtures__",
    "__snapshots__",
    "generated",
    "__generated__",
];

fn is_fixture_or_generated(rel: &str) -> bool {
    let mut dirs = rel.split('/').rev().skip(1);
    dirs.any(|dir| FIXTURE_OR_GENERATED_DIRS.contains(&dir))
}

/// File-name markers of story files (`button.stories.tsx`).
const STORY_FILE_MARKERS: &[&str] = &[".stories.", ".story."];

/// Test or story code. Each copy there shows one case or one state, so the
/// copies differ on purpose.
fn is_example_copy(root: &Path, rel: &str) -> bool {
    if fallow_engine::test_paths::is_test_path_str(root, rel) {
        return true;
    }
    let name = rel.rsplit('/').next().unwrap_or(rel).to_ascii_lowercase();
    STORY_FILE_MARKERS
        .iter()
        .any(|marker| name.contains(marker))
}

/// A tool config such as `vite.config.ts`. A test-environment check is
/// normal there.
fn is_tool_config(rel: &str) -> bool {
    let name = rel.rsplit('/').next().unwrap_or(rel);
    // An rc file is a dot file whose first name part ends in `rc`, such as
    // `.eslintrc.js`. `src.ts` and `arc.ts` are not rc files.
    let dot_rc = name.strip_prefix('.').is_some_and(|rest| {
        rest.split_once('.')
            .is_some_and(|(stem, _ext)| stem.ends_with("rc"))
    });
    name.contains(".config.") || name.contains(".conf.") || dot_rc
}

fn detect_file(root: &Path, file: &FileTexts, rules: &RulesConfig, raw: &mut Vec<RawFinding>) {
    let base = file.base.as_deref().unwrap_or("");
    let Some(head) = file.head.as_deref() else {
        return;
    };
    let rel = file.rel.as_str();
    let is_test = is_js_test_code(root, rel);

    if rules.weakened_tests != Severity::Off {
        if is_test {
            for hit in text::skip_only_hits(base, head) {
                raw.push(RawFinding {
                    rule: DiffRiskRule::WeakenedTests,
                    kind: Some(WeakenedTestsKind::SkipOnly),
                    path: rel.to_owned(),
                    line: Some(hit.line),
                    message: format!(
                        "The change adds `{}`, so other tests or this test do not run",
                        hit.evidence
                    ),
                    evidence: hit.evidence,
                    related: Vec::new(),
                });
            }
        }
        if text::is_coverage_config_path(rel) {
            for hit in text::weakened_threshold_hits(base, head) {
                raw.push(RawFinding {
                    rule: DiffRiskRule::WeakenedTests,
                    kind: Some(WeakenedTestsKind::CoverageThreshold),
                    path: rel.to_owned(),
                    line: Some(hit.line),
                    message: format!(
                        "The change makes a threshold less strict ({})",
                        hit.evidence
                    ),
                    evidence: hit.evidence,
                    related: Vec::new(),
                });
            }
        }
    }

    if rules.added_suppression != Severity::Off
        && takes_suppression_comments(rel)
        && !is_fixture_or_generated(rel)
    {
        let type_test = is_type_or_test_file(root, rel);
        for hit in text::added_suppression_hits(base, head) {
            // In a test, `@ts-expect-error` asserts that a type error exists.
            if type_test && hit.evidence == "@ts-expect-error" {
                continue;
            }
            raw.push(RawFinding {
                rule: DiffRiskRule::AddedSuppression,
                kind: None,
                path: rel.to_owned(),
                line: Some(hit.line),
                message: format!("The change adds a `{}` suppression comment", hit.evidence),
                evidence: hit.evidence,
                related: Vec::new(),
            });
        }
    }

    if rules.test_only_branch != Severity::Off
        && is_source_file(rel)
        && !fallow_engine::test_paths::is_test_path_str(root, rel)
        && !is_tool_config(rel)
    {
        for hit in text::test_only_branch_hits(base, head) {
            raw.push(RawFinding {
                rule: DiffRiskRule::TestOnlyBranch,
                kind: None,
                path: rel.to_owned(),
                line: Some(hit.line),
                message: format!(
                    "The change adds a `{}` check to source code, so tests and production can take different paths",
                    hit.evidence
                ),
                evidence: hit.evidence,
                related: Vec::new(),
            });
        }
    }

    if rules.added_dependency != Severity::Off && crate::dependency_deltas::is_manifest_path(rel) {
        let deltas = crate::dependency_deltas::manifest_dependency_deltas(base, head);
        for added in deltas.added {
            let evidence = format!("{}@{} ({})", added.name, added.to, added.section);
            raw.push(RawFinding {
                rule: DiffRiskRule::AddedDependency,
                kind: None,
                path: rel.to_owned(),
                line: (added.line > 0).then_some(added.line),
                message: format!("The change adds the dependency `{}`", added.name),
                evidence,
                related: Vec::new(),
            });
        }
    }
}

/// The names that the change removed from the project.
///
/// - Identifiers that a changed non-test source file held in base and no
///   longer holds in head, such as a deleted function or a dropped call.
///   Identifiers of a deleted file count only when no changed non-test source
///   file still holds them in head, and names that the deleted file imports
///   never count.
/// - The file stem of each deleted file without test cases, and each of its
///   directories that is gone at head, such as a deleted fixture directory.
///
/// Reserved words and common globals never count.
struct RemovedNames {
    identifiers: FxHashSet<String>,
    segments: FxHashSet<String>,
}

/// The shortest name that can explain a test removal. Shorter names, such as
/// `id` or `get`, are too common to tie a test to the removed code.
const MIN_REMOVED_NAME_LEN: usize = 4;

/// JS and TS reserved words and common globals. Each source file holds them,
/// so they do not tie a test to the removed code.
const NOT_REMOVED_NAMES: &[&str] = &[
    "async",
    "await",
    "break",
    "case",
    "catch",
    "class",
    "console",
    "const",
    "continue",
    "default",
    "delete",
    "else",
    "enum",
    "export",
    "exports",
    "extends",
    "false",
    "finally",
    "from",
    "function",
    "import",
    "instanceof",
    "interface",
    "module",
    "null",
    "require",
    "return",
    "static",
    "super",
    "switch",
    "this",
    "throw",
    "true",
    "type",
    "typeof",
    "undefined",
    "void",
    "while",
    "with",
    "yield",
];

/// Whether a token can name removed code.
fn is_removed_name_candidate(name: &str) -> bool {
    name.len() >= MIN_REMOVED_NAME_LEN && !NOT_REMOVED_NAMES.contains(&name)
}

/// The names that the import statements of a file bind. An imported name is
/// declared in another file, so its use in a deleted file does not show that
/// the name left the project.
fn imported_names(src: &str) -> FxHashSet<String> {
    let mut statements = String::new();
    let mut in_import = false;
    for line in src.lines() {
        let trimmed = line.trim_start();
        if !in_import && trimmed.starts_with("import ") {
            in_import = true;
        }
        if !in_import {
            continue;
        }
        statements.push_str(line);
        statements.push('\n');
        if trimmed.ends_with(';') || trimmed.contains("from ") || trimmed.ends_with("from") {
            in_import = false;
        }
    }
    text::code_identifiers(&statements)
}

impl RemovedNames {
    fn collect(root: &Path, files: &[FileTexts]) -> Self {
        let mut identifiers = FxHashSet::default();
        let mut deleted_identifiers = FxHashSet::default();
        let mut segments = FxHashSet::default();
        let mut head_union = FxHashSet::default();
        for file in files {
            // A deleted file with test cases does not explain its own removal.
            if file.head.is_none()
                && file
                    .base
                    .as_deref()
                    .is_some_and(|base| text::test_case_count(base) == 0)
            {
                segments.extend(deleted_path_segments(root, &file.rel));
            }
            if !is_source_file(&file.rel)
                || fallow_engine::test_paths::is_test_path_str(root, &file.rel)
            {
                continue;
            }
            if let Some(head) = file.head.as_deref() {
                head_union.extend(text::code_identifiers(head));
            }
            let Some(base) = file.base.as_deref() else {
                continue;
            };
            match file.head.as_deref() {
                Some(head) => {
                    let head = text::code_identifiers(head);
                    identifiers.extend(
                        text::code_identifiers(base)
                            .into_iter()
                            .filter(|name| is_removed_name_candidate(name) && !head.contains(name)),
                    );
                }
                None => {
                    let imported = imported_names(base);
                    deleted_identifiers.extend(text::code_identifiers(base).into_iter().filter(
                        |name| is_removed_name_candidate(name) && !imported.contains(name),
                    ));
                }
            }
        }
        // A name of a deleted file that a changed source file still holds did
        // not leave the project, for example a function that moved from the
        // deleted file.
        identifiers.extend(
            deleted_identifiers
                .into_iter()
                .filter(|name| !head_union.contains(name)),
        );
        Self {
            identifiers,
            segments,
        }
    }

    /// Whether the lines that a test file lost name removed code that the
    /// head test no longer names. Then the tests left together with the code
    /// they covered. A path segment matches only a whole token of a line, so
    /// a deleted `core` directory does not match `score`.
    fn explain(&self, base: &str, head: &str) -> bool {
        let removed: Vec<&str> = text::added_lines(head, base)
            .into_iter()
            .map(|(_, line)| line)
            .collect();
        if removed.is_empty() {
            return false;
        }
        let head_identifiers = text::code_identifiers(head);
        let names_removed_identifier = removed.iter().any(|line| {
            text::code_identifiers(line)
                .iter()
                .any(|name| self.identifiers.contains(name) && !head_identifiers.contains(name))
        });
        names_removed_identifier
            || self.segments.iter().any(|segment| {
                !head.contains(segment.as_str())
                    && removed
                        .iter()
                        .any(|line| path_tokens(line).any(|token| token == segment))
            })
    }
}

/// The path segments that a deleted file takes out of the project: its file
/// stem, and each directory that is gone at head. A directory that still
/// exists, such as `src/components`, did not leave the project, so a test that
/// imports another file from it is not explained.
fn deleted_path_segments(root: &Path, rel: &str) -> Vec<String> {
    let stem_path = rel.rsplit_once('.').map_or(rel, |(stem, _)| stem);
    let mut found = Vec::new();
    let mut prefix_len = 0;
    let parts: Vec<&str> = stem_path.split('/').collect();
    let last = parts.len().saturating_sub(1);
    for (index, part) in parts.iter().enumerate() {
        prefix_len += part.len();
        let is_stem = index == last;
        let gone = is_stem || !root.join(&rel[..prefix_len]).exists();
        if gone && part.len() >= MIN_REMOVED_NAME_LEN {
            found.push((*part).to_owned());
        }
        prefix_len += 1;
    }
    found
}

/// The tokens of a line split on path and string delimiters.
fn path_tokens(line: &str) -> impl Iterator<Item = &str> {
    line.split(['/', '.', '\'', '"', '`'])
}

/// `removed-tests` and `assertion-drop`. Both compare per file, and both
/// report only when the total over every changed test file also dropped, so
/// tests or assertions that moved to another changed file do not report. A
/// drop does not report when the lost test lines name code that the change
/// removed: the tests went together with that code. An assertion drop reports
/// only when assertions are at least half of the code lines that the file
/// lost. Otherwise the change rewrote the test, for example it moved setup
/// checks into a shared helper.
fn detect_removed_tests(root: &Path, files: &[FileTexts], raw: &mut Vec<RawFinding>) {
    struct Counts<'a> {
        rel: &'a str,
        base_cases: usize,
        head_cases: usize,
        base_asserts: usize,
        head_asserts: usize,
        explained: bool,
        assertions_led: bool,
    }
    let removed_names = RemovedNames::collect(root, files);
    let counts: Vec<Counts<'_>> = files
        .iter()
        .filter(|file| is_js_test_code(root, &file.rel))
        .map(|file| {
            let base = file.base.as_deref().unwrap_or("");
            let head = file.head.as_deref().unwrap_or("");
            Counts {
                rel: &file.rel,
                base_cases: text::test_case_count(base),
                head_cases: text::test_case_count(head),
                base_asserts: text::assertion_count(base),
                head_asserts: text::assertion_count(head),
                explained: removed_names.explain(base, head),
                assertions_led: {
                    let (asserts, code) = text::removed_assertion_lines(base, head);
                    asserts * 2 >= code
                },
            }
        })
        .collect();
    let total = |pick: fn(&Counts<'_>) -> usize| counts.iter().map(pick).sum::<usize>();
    let cases_dropped = total(|c| c.head_cases) < total(|c| c.base_cases);
    let asserts_dropped = total(|c| c.head_asserts) < total(|c| c.base_asserts);
    for count in counts.iter().filter(|count| !count.explained) {
        if cases_dropped && count.head_cases < count.base_cases {
            let evidence = format!("test cases: {} -> {}", count.base_cases, count.head_cases);
            raw.push(RawFinding {
                rule: DiffRiskRule::WeakenedTests,
                kind: Some(WeakenedTestsKind::RemovedTests),
                path: count.rel.to_owned(),
                line: None,
                message: format!("The change removes test cases ({evidence})"),
                evidence,
                related: Vec::new(),
            });
        } else if asserts_dropped
            && count.assertions_led
            && count.head_cases >= count.base_cases
            && count.head_asserts < count.base_asserts
        {
            let evidence = format!(
                "assertions: {} -> {}",
                count.base_asserts, count.head_asserts
            );
            raw.push(RawFinding {
                rule: DiffRiskRule::WeakenedTests,
                kind: Some(WeakenedTestsKind::AssertionDrop),
                path: count.rel.to_owned(),
                line: None,
                message: format!("The change removes assertions ({evidence})"),
                evidence,
                related: Vec::new(),
            });
        }
    }
}

fn detect_diverged_clones(
    input: &DiffRiskInput<'_>,
    source: &mut BaseSource,
    base_groups: &[BaseCloneGroup],
    files: &[FileTexts],
    raw: &mut Vec<RawFinding>,
) {
    let changed: FxHashSet<String> = files.iter().map(|file| file.rel.clone()).collect();
    let renames: FxHashMap<String, String> = input
        .renames
        .iter()
        .map(|rename| {
            (
                relative_key_path(&rename.from, input.root),
                relative_key_path(&rename.to, input.root),
            )
        })
        .collect();
    let head_by_rel: FxHashMap<&str, Option<&str>> = files
        .iter()
        .map(|file| (file.rel.as_str(), file.head.as_deref()))
        .collect();
    let root = input.root;
    let mut base_text = |rel: &str| match source.read(&root.join(rel)) {
        BaseRead::Content(text) => Some(text),
        BaseRead::Missing | BaseRead::Error => None,
    };
    let mut head_text = |rel: &str| match head_by_rel.get(rel) {
        Some(head) => head.map(str::to_owned),
        None => std::fs::read_to_string(root.join(rel)).ok(),
    };
    let found = clones::diverged_clone_siblings(
        base_groups,
        &input.head_clone_groups,
        &changed,
        &renames,
        &mut CloneTexts {
            base: &mut base_text,
            head: &mut head_text,
        },
    );
    for diverged in found {
        // Test and story copies differ case by case on purpose, so an edit to
        // one of them is not a missed sibling.
        if is_example_copy(root, &diverged.path) {
            continue;
        }
        let rules = input.rules.for_path(&root.join(&diverged.path));
        if rules.diverged_clone_sibling == Severity::Off {
            continue;
        }
        let related: Vec<DiffRiskLocation> = diverged
            .siblings
            .iter()
            .map(|sibling| DiffRiskLocation {
                path: sibling.path.clone(),
                line: sibling.start_line,
            })
            .collect();
        let evidence = related
            .iter()
            .map(|location| format!("{}:{}", location.path, location.line))
            .collect::<Vec<_>>()
            .join(", ");
        raw.push(RawFinding {
            rule: DiffRiskRule::DivergedCloneSibling,
            kind: None,
            path: diverged.path,
            line: diverged.line,
            message: format!(
                "The change edits one copy of duplicated code, and its {} unchanged copies no longer match",
                related.len()
            ),
            evidence: format!("unchanged copies: {evidence}"),
            related,
        });
    }
}

const fn issue_kind(rule: DiffRiskRule) -> IssueKind {
    match rule {
        DiffRiskRule::WeakenedTests => IssueKind::WeakenedTests,
        DiffRiskRule::AddedSuppression => IssueKind::AddedSuppression,
        DiffRiskRule::TestOnlyBranch => IssueKind::TestOnlyBranch,
        DiffRiskRule::DivergedCloneSibling => IssueKind::DivergedCloneSibling,
        DiffRiskRule::AddedDependency => IssueKind::AddedDependency,
    }
}

const fn rule_severity(rules: &RulesConfig, rule: DiffRiskRule) -> Severity {
    rules.severity_for_kind(issue_kind(rule))
}

/// Whether a head suppression covers the finding. An `added-suppression`
/// finding honors only a suppression that names its rule: a new blanket
/// `fallow-ignore` comment must not hide itself.
fn is_suppressed(suppressions: &[Suppression], rule: DiffRiskRule, line: Option<u32>) -> bool {
    let kind = issue_kind(rule);
    suppressions.iter().any(|suppression| {
        let names_rule = matches!(
            &suppression.target,
            Some(SuppressionTarget::Issue(target)) if *target == kind
        );
        if rule == DiffRiskRule::AddedSuppression && !names_rule {
            return false;
        }
        let line = line.unwrap_or(0);
        if line == 0 {
            suppression.line == 0 && suppression.matches_issue_kind(0, kind)
        } else {
            suppression.matches_issue_kind(line, kind)
        }
    })
}

fn finalize(
    input: &DiffRiskInput<'_>,
    files: &[FileTexts],
    raw: Vec<RawFinding>,
) -> Vec<DiffRiskFinding> {
    let mut suppressions: FxHashMap<&str, Vec<Suppression>> = FxHashMap::default();
    let head_exists: FxHashMap<&str, bool> = files
        .iter()
        .map(|file| (file.rel.as_str(), file.head.is_some()))
        .collect();
    for file in files {
        if let Some(head) = file.head.as_deref() {
            suppressions.insert(
                file.rel.as_str(),
                fallow_engine::source::suppressions_from_text(head),
            );
        }
    }
    let mut findings: Vec<DiffRiskFinding> = raw
        .into_iter()
        .filter_map(|finding| {
            let rules = input.rules.for_path(&input.root.join(&finding.path));
            let severity = match rule_severity(&rules, finding.rule) {
                Severity::Off => return None,
                Severity::Warn => DiffRiskSeverity::Warn,
                Severity::Error => DiffRiskSeverity::Error,
            };
            if suppressions
                .get(finding.path.as_str())
                .is_some_and(|list| is_suppressed(list, finding.rule, finding.line))
            {
                return None;
            }
            let exists = head_exists
                .get(finding.path.as_str())
                .copied()
                .unwrap_or(true);
            let actions = suppression_actions(finding.rule, &finding.path, finding.line, exists);
            Some(DiffRiskFinding {
                rule: finding.rule,
                kind: finding.kind,
                path: finding.path,
                line: finding.line,
                message: finding.message,
                evidence: finding.evidence,
                severity,
                introduced: true,
                related: finding.related,
                actions,
            })
        })
        .collect();
    findings.sort_by(|a, b| {
        a.path
            .cmp(&b.path)
            .then(a.line.cmp(&b.line))
            .then(a.rule.cmp(&b.rule))
            .then(a.kind.cmp(&b.kind))
            .then(a.evidence.cmp(&b.evidence))
    });
    findings
}

/// The comment markers of a file type, `None` when the file cannot hold a
/// suppression comment.
fn comment_markers(rel: &str) -> Option<(&'static str, &'static str)> {
    match extension(rel) {
        "css" | "scss" | "sass" | "less" => Some(("/* ", " */")),
        "html" => Some(("<!-- ", " -->")),
        _ if is_source_file(rel) => Some(("// ", "")),
        _ => None,
    }
}

fn suppression_actions(
    rule: DiffRiskRule,
    path: &str,
    line: Option<u32>,
    head_exists: bool,
) -> Vec<DiffRiskAction> {
    let code = rule.code();
    let markers = comment_markers(path).filter(|_| head_exists);
    let Some((open, close)) = markers else {
        return vec![DiffRiskAction {
            kind: DiffRiskActionType::SuppressInConfig,
            auto_fixable: false,
            description: format!(
                "Set `{code}` to `off` for `{path}` in an `overrides` entry of the Fallow config"
            ),
            comment: None,
        }];
    };
    let mut actions = Vec::new();
    if line.is_some() {
        actions.push(DiffRiskAction {
            kind: DiffRiskActionType::SuppressLine,
            auto_fixable: false,
            description: format!("Suppress this `{code}` finding with a comment above the line"),
            comment: Some(format!("{open}fallow-ignore-next-line {code}{close}")),
        });
    }
    actions.push(DiffRiskAction {
        kind: DiffRiskActionType::SuppressFile,
        auto_fixable: false,
        description: format!("Suppress `{code}` for the whole file"),
        comment: Some(format!("{open}fallow-ignore-file {code}{close}")),
    });
    actions
}

#[cfg(test)]
mod tests {
    use super::*;

    fn suppressions(src: &str) -> Vec<Suppression> {
        fallow_engine::source::suppressions_from_text(src)
    }

    fn file(rel: &str, base: &str, head: &str) -> FileTexts {
        FileTexts {
            rel: rel.to_owned(),
            abs: PathBuf::from("/project").join(rel),
            base: Some(base.to_owned()),
            head: Some(head.to_owned()),
        }
    }

    fn rules_of(raw: &[RawFinding]) -> Vec<(DiffRiskRule, &str)> {
        raw.iter()
            .map(|finding| (finding.rule, finding.evidence.as_str()))
            .collect()
    }

    #[test]
    fn only_files_a_rule_reads_are_relevant() {
        let root = Path::new("/project");
        for rel in [
            "src/a.ts",
            "src/a.test.ts",
            "src/page.mdx",
            "src/query.graphql",
            "styles/app.css",
            "package.json",
            "packages/ui/package.json",
            "vitest.config.ts",
            ".nycrc",
            ".fallowrc.json",
        ] {
            assert!(is_rule_relevant(root, rel), "{rel} must be read");
        }
        for rel in [
            "pnpm-lock.yaml",
            "package-lock.json",
            "yarn.lock",
            "assets/logo.png",
            "fonts/inter.woff2",
            "README.md",
        ] {
            assert!(!is_rule_relevant(root, rel), "{rel} must not be read");
        }
    }

    fn removed_test_findings(files: &[FileTexts]) -> Vec<(String, String)> {
        removed_test_findings_in(Path::new("/project"), files)
    }

    fn removed_test_findings_in(root: &Path, files: &[FileTexts]) -> Vec<(String, String)> {
        let mut raw = Vec::new();
        detect_removed_tests(root, files, &mut raw);
        let mut found: Vec<(String, String)> = raw
            .into_iter()
            .map(|finding| (finding.path, finding.evidence))
            .collect();
        found.sort_unstable();
        found
    }

    fn pairs(list: &[(&str, &str)]) -> Vec<(String, String)> {
        list.iter()
            .map(|(a, b)| ((*a).to_owned(), (*b).to_owned()))
            .collect()
    }

    const ERRORS_BASE: &str = "export const ERR_A = 'a';\nexport const ERR_GONE = 'gone';\n";
    const ERRORS_HEAD: &str = "export const ERR_A = 'a';\n";
    const ERRORS_TEST_BASE: &str = "test('ERR_A', () => {\n  expect(ERR_A).toBe('a');\n});\ntest('ERR_GONE', () => {\n  expect(ERR_GONE).toBe('gone');\n});\n";
    const ERRORS_TEST_HEAD: &str = "test('ERR_A', () => {\n  expect(ERR_A).toBe('a');\n});\n";

    #[test]
    fn tests_removed_with_the_source_name_they_cover_are_not_reported() {
        let files = [
            file("src/errors.ts", ERRORS_BASE, ERRORS_HEAD),
            file("src/errors.test.ts", ERRORS_TEST_BASE, ERRORS_TEST_HEAD),
        ];
        assert!(removed_test_findings(&files).is_empty());
    }

    #[test]
    fn tests_removed_while_their_source_name_stays_are_reported() {
        let files = [
            file("src/errors.ts", ERRORS_BASE, ERRORS_BASE),
            file("src/errors.test.ts", ERRORS_TEST_BASE, ERRORS_TEST_HEAD),
        ];
        assert_eq!(
            removed_test_findings(&files),
            pairs(&[("src/errors.test.ts", "test cases: 2 -> 1")])
        );
    }

    #[test]
    fn assertions_removed_with_the_source_name_they_cover_are_not_reported() {
        let base = "test('errors', () => {\n  expect(ERR_A).toBe('a');\n  expect(ERR_GONE).toBe('gone');\n});\n";
        let head = "test('errors', () => {\n  expect(ERR_A).toBe('a');\n});\n";
        let files = [
            file("src/errors.ts", ERRORS_BASE, ERRORS_HEAD),
            file("src/errors.test.ts", base, head),
        ];
        assert!(removed_test_findings(&files).is_empty());
        let files = [
            file("src/errors.ts", ERRORS_BASE, ERRORS_BASE),
            file("src/errors.test.ts", base, head),
        ];
        assert_eq!(
            removed_test_findings(&files),
            pairs(&[("src/errors.test.ts", "assertions: 2 -> 1")])
        );
    }

    #[test]
    fn tests_removed_with_a_deleted_fixture_directory_are_not_reported() {
        let base = "test('a', () => run('basic'));\ntest('legacy', () => run('legacy-types'));\n";
        let head = "test('a', () => run('basic'));\n";
        let files = [
            file("test/package.spec.js", base, head),
            FileTexts {
                rel: "test/fixtures/legacy-types/src/index.ts".to_owned(),
                abs: PathBuf::from("/project/test/fixtures/legacy-types/src/index.ts"),
                base: Some("export const a = 1;\n".to_owned()),
                head: None,
            },
        ];
        assert!(removed_test_findings(&files).is_empty());
    }

    #[test]
    fn assertions_that_leave_with_rewritten_setup_code_are_not_reported() {
        let base = "test('creates', async () => {\n  const response = await client.post('/api').send({ query: CREATE });\n  expect(response.body.errors).toBeUndefined();\n  const id = response.body.data.create.id;\n  expect(await read(id)).toEqual({ id });\n});\n";
        let head = "test('creates', async () => {\n  const { id } = await createRecord();\n  expect(await read(id)).toEqual({ id });\n});\n";
        let files = [file("test/create.spec.ts", base, head)];
        assert!(removed_test_findings(&files).is_empty());
        let head = "test('creates', async () => {\n  const response = await client.post('/api').send({ query: CREATE });\n  const id = response.body.data.create.id;\n});\n";
        let files = [file("test/create.spec.ts", base, head)];
        assert_eq!(
            removed_test_findings(&files),
            pairs(&[("test/create.spec.ts", "assertions: 2 -> 0")])
        );
    }

    #[test]
    fn tests_of_a_call_that_one_source_file_dropped_are_not_reported() {
        let hook_base = "export const useCommands = () => {\n  const enabled = useIsFeatureEnabled('CORE');\n  return enabled ? commands : [];\n};\n";
        let hook_head = "export const useCommands = () => commands;\n";
        let other = "export const usePage = () => useIsFeatureEnabled('PAGE');\n";
        let test_base = "jest.mock('./flags', () => ({ useIsFeatureEnabled: () => flag() }));\nit('shows commands', () => {\n  expect(useCommands()).toEqual(commands);\n});\nit('hides commands with the flag off', () => {\n  flag.mockReturnValue(false);\n  expect(useCommands()).toEqual([]);\n});\n";
        let test_head =
            "it('shows commands', () => {\n  expect(useCommands()).toEqual(commands);\n});\n";
        let files = [
            file("src/useCommands.ts", hook_base, hook_head),
            file("src/usePage.ts", other, other),
            file("src/useCommands.test.ts", test_base, test_head),
        ];
        assert!(removed_test_findings(&files).is_empty());
    }

    #[test]
    fn words_in_comments_and_test_titles_do_not_explain_a_removal() {
        let source_base =
            "// Hide the items when the page is disabled.\nexport const items = (all) => all;\n";
        let source_head = "export const items = (all) => all;\n";
        let test_base = "it('keeps items', () => {\n  expect(items([1])).toEqual([1]);\n});\nit('keeps items when the page is disabled', () => {\n  expect(items([])).toEqual([]);\n});\n";
        let test_head = "it('keeps items', () => {\n  expect(items([1])).toEqual([1]);\n});\n";
        let files = [
            file("src/items.ts", source_base, source_head),
            file("src/items.test.ts", test_base, test_head),
        ];
        assert_eq!(
            removed_test_findings(&files),
            pairs(&[("src/items.test.ts", "test cases: 2 -> 1")])
        );
    }

    #[test]
    fn a_deleted_test_file_does_not_explain_its_own_removal() {
        let files = [FileTexts {
            rel: "src/zero.test.ts".to_owned(),
            abs: PathBuf::from("/project/src/zero.test.ts"),
            base: Some("import { price } from './zero';\nit('keeps zero', () => {\n  expect(price(0)).toBe(0);\n});\n".to_owned()),
            head: None,
        }];
        assert_eq!(
            removed_test_findings(&files),
            pairs(&[("src/zero.test.ts", "test cases: 1 -> 0")])
        );
    }

    fn deleted(rel: &str, base: &str) -> FileTexts {
        FileTexts {
            rel: rel.to_owned(),
            abs: PathBuf::from("/project").join(rel),
            base: Some(base.to_owned()),
            head: None,
        }
    }

    #[test]
    fn an_unrelated_deleted_source_file_does_not_explain_a_deleted_test_file() {
        let files = [
            deleted(
                "src/legacy.ts",
                "import { a } from './a';\nimport { price } from './price';\nexport const legacy = price(a);\n",
            ),
            deleted(
                "src/price.test.ts",
                "import { price } from './price';\nit('keeps zero', () => {\n  expect(price(0)).toBe(0);\n});\n",
            ),
        ];
        assert_eq!(
            removed_test_findings(&files),
            pairs(&[("src/price.test.ts", "test cases: 1 -> 0")])
        );
    }

    #[test]
    fn a_name_that_a_changed_file_still_holds_does_not_explain_a_removal() {
        let files = [
            deleted("src/legacy.ts", "export const formatPrice = (n) => n;\n"),
            file(
                "src/format.ts",
                "export const format = (n) => n;\n",
                "export const formatPrice = (n) => n;\n",
            ),
            file(
                "src/format.test.ts",
                "it('formats', () => {\n  expect(formatPrice(1)).toBe(1);\n});\n",
                "",
            ),
        ];
        assert_eq!(
            removed_test_findings(&files),
            pairs(&[("src/format.test.ts", "test cases: 1 -> 0")])
        );
    }

    #[test]
    fn a_deleted_directory_name_matches_whole_path_segments_only() {
        let root = tempfile::tempdir().expect("tempdir");
        let base =
            "it('a', () => run());\nit('scores', () => {\n  expect(score(1)).toBe(1);\n});\n";
        let head = "it('a', () => run());\n";
        let files = [
            file("src/score.test.ts", base, head),
            deleted("src/core/x.ts", "export const x = 1;\n"),
        ];
        assert_eq!(
            removed_test_findings_in(root.path(), &files),
            pairs(&[("src/score.test.ts", "test cases: 2 -> 1")])
        );
        let base =
            "import { a } from '../core/a';\nit('a', () => run(a));\nit('b', () => run(b));\n";
        let head = "it('a', () => run(a));\n";
        let files = [
            file("src/run.test.ts", base, head),
            deleted("src/core/x.ts", "export const x = 1;\n"),
        ];
        assert!(removed_test_findings_in(root.path(), &files).is_empty());
    }

    #[test]
    fn a_directory_that_still_exists_does_not_explain_a_removal() {
        let root = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir_all(root.path().join("src/components")).expect("components dir");
        let files = [
            deleted(
                "src/components/Legacy.tsx",
                "export const Legacy = () => null;\n",
            ),
            deleted(
                "src/__tests__/button.test.tsx",
                "import { Button } from '../components/Button';\nit('renders', () => {\n  expect(Button()).toBeTruthy();\n});\n",
            ),
        ];
        assert_eq!(
            removed_test_findings_in(root.path(), &files),
            pairs(&[("src/__tests__/button.test.tsx", "test cases: 1 -> 0")])
        );
    }

    #[test]
    fn edited_story_copy_is_not_a_diverged_clone() {
        for rel in [
            "apps/storybook/stories/select.stories.tsx",
            "src/button.story.tsx",
        ] {
            assert!(is_example_copy(Path::new("/project"), rel), "{rel}");
        }
        assert!(!is_example_copy(Path::new("/project"), "src/store.ts"));
    }

    #[test]
    fn tool_config_names_match_config_and_dot_rc_files_only() {
        for name in [
            "vite.config.ts",
            "pkg/karma.conf.js",
            ".eslintrc.js",
            "pkg/.mocharc.cjs",
        ] {
            assert!(is_tool_config(name), "{name}");
        }
        for name in ["src.ts", "src/arc.ts", "lib/orc.js", "src/index.ts"] {
            assert!(!is_tool_config(name), "{name}");
        }
    }

    #[test]
    fn suppressions_in_fixture_and_generated_files_are_not_reported() {
        let head = "// @ts-nocheck\nexport const a = 1;\n";
        let mut raw = Vec::new();
        let root = Path::new("/project");
        let rules = RulesConfig::default();
        for rel in [
            "src/__tests__/upstream/fixture/generated/schema.ts",
            "test/fixtures/app/src/index.ts",
            "src/__generated__/graphql.ts",
            "src/generated/client.ts",
        ] {
            detect_file(root, &file(rel, "", head), &rules, &mut raw);
        }
        assert!(raw.is_empty(), "{:?}", rules_of(&raw));
        detect_file(root, &file("src/schema.ts", "", head), &rules, &mut raw);
        assert_eq!(
            rules_of(&raw),
            vec![(DiffRiskRule::AddedSuppression, "@ts-nocheck")]
        );
    }

    #[test]
    fn ts_expect_error_in_type_tests_is_an_assertion_not_a_suppression() {
        let head = "// @ts-expect-error\nconst x: string = 1;\n";
        let mut raw = Vec::new();
        let root = Path::new("/project");
        let rules = RulesConfig::default();
        detect_file(root, &file("test-d/hooks.ts", "", head), &rules, &mut raw);
        detect_file(root, &file("src/a.test-d.ts", "", head), &rules, &mut raw);
        detect_file(root, &file("src/a.test.ts", "", head), &rules, &mut raw);
        assert!(raw.is_empty(), "{:?}", rules_of(&raw));
        detect_file(root, &file("src/a.ts", "", head), &rules, &mut raw);
        assert_eq!(
            rules_of(&raw),
            vec![(DiffRiskRule::AddedSuppression, "@ts-expect-error")]
        );
    }

    /// A test file in another language (a Rust integration test under
    /// `tests/`) can hold JavaScript test text in a string. The test rules
    /// read JavaScript-family test files only.
    #[test]
    fn test_rules_read_javascript_test_files_only() {
        let root = Path::new("/project");
        let rules = RulesConfig::default();
        let base = "it('a', f);\nit('b', f);\n";
        let head = "it.skip('a', f);\n";
        let mut raw = Vec::new();
        let rust = file("crates/cli/tests/integration/a_tests.rs", base, head);
        detect_file(root, &rust, &rules, &mut raw);
        detect_removed_tests(root, std::slice::from_ref(&rust), &mut raw);
        assert!(raw.is_empty(), "{:?}", rules_of(&raw));

        let js = file("src/a.test.ts", base, head);
        detect_file(root, &js, &rules, &mut raw);
        detect_removed_tests(root, std::slice::from_ref(&js), &mut raw);
        assert_eq!(
            rules_of(&raw),
            vec![
                (DiffRiskRule::WeakenedTests, "it.skip"),
                (DiffRiskRule::WeakenedTests, "test cases: 2 -> 1"),
            ]
        );
    }

    #[test]
    fn named_line_suppression_hides_the_finding() {
        let list = suppressions("// fallow-ignore-next-line weakened-tests\nit.skip('a', f);\n");
        assert!(is_suppressed(&list, DiffRiskRule::WeakenedTests, Some(2)));
        assert!(!is_suppressed(&list, DiffRiskRule::TestOnlyBranch, Some(2)));
    }

    #[test]
    fn file_suppression_hides_a_file_level_finding() {
        let list = suppressions("// fallow-ignore-file weakened-tests\n");
        assert!(is_suppressed(&list, DiffRiskRule::WeakenedTests, None));
    }

    #[test]
    fn blanket_suppression_does_not_hide_an_added_suppression() {
        let list = suppressions("// fallow-ignore-file\nconst a = 1;\n");
        assert!(!is_suppressed(
            &list,
            DiffRiskRule::AddedSuppression,
            Some(1)
        ));
        assert!(is_suppressed(&list, DiffRiskRule::TestOnlyBranch, Some(2)));
    }

    #[test]
    fn json_and_deleted_files_get_a_config_action() {
        let actions =
            suppression_actions(DiffRiskRule::AddedDependency, "package.json", Some(3), true);
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].kind, DiffRiskActionType::SuppressInConfig);
        let actions = suppression_actions(DiffRiskRule::WeakenedTests, "a.test.ts", None, false);
        assert_eq!(actions[0].kind, DiffRiskActionType::SuppressInConfig);
    }

    #[test]
    fn source_files_get_line_and_file_comments() {
        let actions = suppression_actions(DiffRiskRule::TestOnlyBranch, "src/a.ts", Some(3), true);
        let comments: Vec<_> = actions
            .iter()
            .filter_map(|a| a.comment.as_deref())
            .collect();
        assert_eq!(
            comments,
            vec![
                "// fallow-ignore-next-line test-only-branch",
                "// fallow-ignore-file test-only-branch"
            ]
        );
        let actions =
            suppression_actions(DiffRiskRule::AddedSuppression, "src/a.css", Some(3), true);
        assert_eq!(
            actions[0].comment.as_deref(),
            Some("/* fallow-ignore-next-line added-suppression */")
        );
    }
}
