use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use fallow_config::{ResolvedConfig, WorkspaceDiagnostic, WorkspaceDiagnosticKind};
use fallow_types::discover::{DiscoveredFile, FileId};
use ignore::WalkBuilder;
use rustc_hash::FxHashSet;

use super::{ALLOWED_HIDDEN_DIRS, SCRIPT_SCOPE_DENYLIST};

/// Process-wide dedupe of the size-skip / largest-files stderr notes, keyed by a
/// content-derived string, so combined-mode (`fallow` runs check + dupes +
/// health, each of which can trigger a source walk) emits each note at most once
/// per distinct content. Mirrors the workspace-diagnostics `should_emit`
/// pattern (issue #1086).
fn should_emit_note_once(key: String) -> bool {
    static EMITTED: OnceLock<Mutex<FxHashSet<String>>> = OnceLock::new();
    EMITTED
        .get_or_init(|| Mutex::new(FxHashSet::default()))
        .lock()
        .map_or(true, |mut set| set.insert(key))
}

/// A discovered file path paired with its on-disk size in bytes, as collected
/// by the parallel walker before [`DiscoveredFile`] ids are assigned.
type SizedFile = (PathBuf, u64);

/// Dot-prefixed directories the walk dropped, collected inside the parallel
/// walker's `filter_entry` predicate.
///
/// `Arc<Mutex<..>>` and not a borrow: `WalkBuilder::filter_entry` requires
/// `Fn(&DirEntry) -> bool + Send + Sync + 'static`, so the closure can neither
/// borrow a local nor mutate captured state directly, and the predicate runs
/// on every walker thread.
type SkippedDotdirSink = Arc<Mutex<Vec<PathBuf>>>;

/// Number of example file paths named in the aggregated skipped-large-file and
/// largest-files stderr notes before the tail collapses to "and N more". Keeps
/// the notes to one bounded line on a monorepo that skips many files.
const NOTE_EXAMPLE_CAP: usize = 5;

/// Directory levels below a skipped dotdir the bounded scan descends. The
/// dotdir itself is level 0. Two levels reach the conventional
/// `<dotdir>/<group>/<file>` layout (`.claude/hooks/probe.mjs`) with one level
/// of headroom, and stop well above a vendored toolchain tree.
const DOTDIR_SCAN_MAX_DEPTH: usize = 2;

/// Directory entries the bounded scan reads across all levels of ONE skipped
/// dotdir. The ceiling this buys is a SYSCALL count, not a wall-clock figure:
/// a directory-heavy dotdir spends budget on subdirectories that each cost an
/// opendir of their own, so 256 entries can still mean 257 directory reads and
/// several milliseconds. State the bound in syscalls, never in milliseconds.
const DOTDIR_SCAN_MAX_ENTRIES: usize = 256;

/// Directory entries the bounded scan reads across ALL skipped dotdirs in one
/// walk. [`DOTDIR_SCAN_MAX_ENTRIES`] bounds a single directory and nothing
/// bounded the sum, so the added cost was linear in the candidate count: a
/// synthetic tree of 1000 directory-heavy dotdirs turned a 191 ms run into
/// 6.6 s. Candidates are scanned in sorted order and share this budget, so
/// exhausting it drops the advisory for the remaining candidates
/// deterministically instead of paying an unbounded cost. With this ceiling
/// and [`DOTDIR_SCAN_MAX_CANDIDATES`] the same synthetic trees measure about
/// 30 ms of added work whether they hold 300 or 1000 candidates, and a real
/// repository stays inside run-to-run noise.
const DOTDIR_SCAN_TOTAL_ENTRIES: usize = 1024;

/// Skipped dotdirs the bounded scan OPENS in one walk. The entry budget does
/// not bound the per-candidate setup cost, since each scan builds its own
/// gitignore matcher chain (about 0.2 ms) before it reads a single entry, so
/// the candidate count needs a ceiling of its own. Counted after the name
/// checks, so a monorepo full of `.turbo` and `.next` directories cannot spend
/// the ceiling on directories that were never going to be scanned.
const DOTDIR_SCAN_MAX_CANDIDATES: usize = 64;

/// File extensions that put a file in the module graph the advisory talks
/// about. Narrower than [`SOURCE_EXTENSIONS`] on purpose: the message states
/// that the directory's imports and exports are not analyzed, and that is only
/// true of code. A dotdir holding nothing but a generated Lighthouse
/// `report.html`, a Sanity runtime page, a `schema.graphql`, or a stylesheet
/// has no imports or exports to lose, so it does not earn the advisory.
const DOTDIR_MODULE_EXTENSIONS: &[&str] = &[
    "ts", "tsx", "mts", "cts", "gts", "js", "jsx", "mjs", "cjs", "gjs", "vue", "svelte", "astro",
    "mdx",
];

/// Discovered-file-count threshold above which the pre-parse largest-files note
/// fires, so an out-of-memory hang at the parse stage has a visible suspect
/// list (issue #1086).
const LARGE_SET_THRESHOLD: usize = 20_000;

/// Single-file byte threshold above which the pre-parse largest-files note
/// fires even on a small project. Set just under the default 5 MB skip so the
/// note fires for kept files that are approaching the skip limit (the genuine
/// out-of-memory suspects), not for ordinary large-but-benign files.
const LARGE_FILE_NOTE_BYTES: u64 = 4 * 1024 * 1024;

/// Minimum size for a file to appear in the largest-files note. Filters out the
/// `0.0 MB` entries that would otherwise pad the list once it fires, keeping the
/// named files to plausible memory contributors.
const NOTE_FILE_FLOOR_BYTES: u64 = 256 * 1024;

/// Minimum size for content-shape based minified-bundle skipping. Smaller
/// one-line files can be hand-written utilities, while multi-MB one-line JS is
/// generated output in practice.
const MINIFIED_FILE_SKIP_BYTES: u64 = 1024 * 1024;

/// Number of bytes inspected when deciding whether a large JS file is minified.
const MINIFIED_SAMPLE_BYTES: usize = 256 * 1024;

/// A single line this long in a multi-MB JS file is treated as generated
/// minified output. This avoids parsing assets that can expand to huge ASTs.
const MINIFIED_LONG_LINE_BYTES: usize = 128 * 1024;

/// Whether a path is a TypeScript declaration file (`.d.ts`/`.d.mts`/`.d.cts`).
/// Declaration files are exempt from the per-file size skip because they are
/// reachability roots for global types: skipping a large `auto-imports.d.ts`
/// would false-flag the files whose types it provides.
fn is_declaration_file(path: &Path) -> bool {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    name.ends_with(".d.ts") || name.ends_with(".d.mts") || name.ends_with(".d.cts")
}

fn is_plain_js_file(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|ext| ext.to_str()),
        Some("js" | "mjs" | "cjs")
    )
}

fn has_minified_line_shape(path: &Path) -> bool {
    use std::io::Read;

    let Ok(mut file) = std::fs::File::open(path) else {
        return false;
    };
    let mut sample = vec![0; MINIFIED_SAMPLE_BYTES];
    let Ok(len) = file.read(&mut sample) else {
        return false;
    };
    sample.truncate(len);
    if sample.is_empty() {
        return false;
    }

    let mut current_line = 0usize;
    for byte in sample {
        if byte == b'\n' || byte == b'\r' {
            current_line = 0;
            continue;
        }
        current_line += 1;
        if current_line >= MINIFIED_LONG_LINE_BYTES {
            return true;
        }
    }
    false
}

fn is_probably_minified_generated_js(path: &Path, size_bytes: u64) -> bool {
    size_bytes >= MINIFIED_FILE_SKIP_BYTES
        && is_plain_js_file(path)
        && !is_declaration_file(path)
        && has_minified_line_shape(path)
}

/// Render a byte count as a megabyte figure with one decimal place.
fn format_size_mb(bytes: u64) -> String {
    #[expect(
        clippy::cast_precision_loss,
        reason = "display-only size figure; precision loss past 2^53 bytes is irrelevant"
    )]
    let mb = bytes as f64 / (1024.0 * 1024.0);
    format!("{mb:.1} MB")
}

/// Join up to [`NOTE_EXAMPLE_CAP`] `path (size)` examples (already ordered) into
/// one comma-separated string, collapsing the tail to "and N more".
fn summarize_examples(root: &Path, examples: &[SizedFile]) -> String {
    let shown: Vec<String> = examples
        .iter()
        .take(NOTE_EXAMPLE_CAP)
        .map(|(path, size)| {
            let display = display_relative_path(root, path);
            format!("{display} ({})", format_size_mb(*size))
        })
        .collect();
    let remaining = examples.len().saturating_sub(NOTE_EXAMPLE_CAP);
    if remaining > 0 {
        format!("{}, and {remaining} more", shown.join(", "))
    } else {
        shown.join(", ")
    }
}

/// Split discovered `(path, size)` pairs into the kept set and the set skipped
/// for exceeding `max_file_size_bytes`. Declaration files are never skipped.
fn partition_by_size(
    raw: Vec<SizedFile>,
    max_file_size_bytes: Option<u64>,
) -> (Vec<SizedFile>, Vec<SizedFile>) {
    let Some(limit) = max_file_size_bytes else {
        return (raw, Vec::new());
    };
    raw.into_iter()
        .partition(|(path, size)| *size <= limit || is_declaration_file(path))
}

/// Split discovered `(path, size)` pairs into files kept for parsing and files
/// skipped because they look like generated minified JavaScript.
fn partition_minified_generated_js(
    raw: Vec<SizedFile>,
    max_file_size_bytes: Option<u64>,
) -> (Vec<SizedFile>, Vec<SizedFile>) {
    if max_file_size_bytes.is_none() {
        return (raw, Vec::new());
    }
    raw.into_iter()
        .partition(|(path, size)| !is_probably_minified_generated_js(path, *size))
}

/// Build the typed diagnostics for the over-limit files this walk dropped and
/// emit one aggregated `tracing::warn!` so a human running `fallow` sees what
/// was dropped. Mirrors the JSON-plus-gated-warn pattern used for undeclared
/// workspaces. The caller writes the returned list to the registry.
fn report_skipped_large_files(
    config: &ResolvedConfig,
    skipped: &[SizedFile],
) -> Vec<WorkspaceDiagnostic> {
    if skipped.is_empty() {
        return Vec::new();
    }
    let diagnostics: Vec<WorkspaceDiagnostic> = skipped
        .iter()
        .map(|(path, size_bytes)| {
            WorkspaceDiagnostic::new(
                &config.root,
                path.clone(),
                WorkspaceDiagnosticKind::SkippedLargeFile {
                    size_bytes: *size_bytes,
                },
            )
        })
        .collect();

    let mut sorted: Vec<SizedFile> = skipped.to_vec();
    sorted.sort_unstable_by_key(|f| std::cmp::Reverse(f.1));
    let count = skipped.len();
    if !config.quiet
        && should_emit_note_once(format!(
            "skip::{}::{count}::{}",
            config.root.display(),
            sorted.first().map_or(0, |f| f.1)
        ))
    {
        let examples = summarize_examples(&config.root, &sorted);
        let noun = if count == 1 { "file" } else { "files" };
        tracing::warn!(
            "fallow: skipped {count} {noun} over the max file size limit ({examples}). \
             Raise the limit with --max-file-size <MB> (or FALLOW_MAX_FILE_SIZE), or add them to ignorePatterns."
        );
    }
    diagnostics
}

/// Build the typed diagnostics for generated minified JS files skipped before
/// parsing. The caller writes the returned list to the registry.
fn report_skipped_minified_files(
    config: &ResolvedConfig,
    skipped: &[SizedFile],
) -> Vec<WorkspaceDiagnostic> {
    if skipped.is_empty() {
        return Vec::new();
    }
    let diagnostics: Vec<WorkspaceDiagnostic> = skipped
        .iter()
        .map(|(path, size_bytes)| {
            WorkspaceDiagnostic::new(
                &config.root,
                path.clone(),
                WorkspaceDiagnosticKind::SkippedMinifiedFile {
                    size_bytes: *size_bytes,
                },
            )
        })
        .collect();

    let mut sorted: Vec<SizedFile> = skipped.to_vec();
    sorted.sort_unstable_by_key(|f| std::cmp::Reverse(f.1));
    let count = skipped.len();
    if !config.quiet
        && should_emit_note_once(format!(
            "minified::{}::{count}::{}",
            config.root.display(),
            sorted.first().map_or(0, |f| f.1)
        ))
    {
        let examples = summarize_examples(&config.root, &sorted);
        let noun = if count == 1 { "file" } else { "files" };
        let pronoun = if count == 1 { "it" } else { "them" };
        tracing::warn!(
            "fallow: skipped {count} minified generated JS {noun} ({examples}). \
             Add {pronoun} to ignorePatterns, rename {pronoun} with a .min.js suffix, or use --max-file-size 0 to analyze {pronoun}."
        );
    }
    diagnostics
}

/// Join up to [`NOTE_EXAMPLE_CAP`] root-relative paths (already ordered) into
/// one comma-separated string, collapsing the tail to "and N more". The
/// size-bearing sibling is [`summarize_examples`].
fn summarize_paths(root: &Path, examples: &[&PathBuf]) -> String {
    let shown: Vec<String> = examples
        .iter()
        .take(NOTE_EXAMPLE_CAP)
        .map(|path| display_relative_path(root, path))
        .collect();
    let remaining = examples.len().saturating_sub(NOTE_EXAMPLE_CAP);
    if remaining > 0 {
        format!("{}, and {remaining} more", shown.join(", "))
    } else {
        shown.join(", ")
    }
}

/// Like [`summarize_paths`], but for a list already known to be incomplete: the
/// tail reads "and more" rather than naming a count the run cannot vouch for.
fn summarize_paths_open_ended(root: &Path, examples: &[&PathBuf]) -> String {
    let shown: Vec<String> = examples
        .iter()
        .take(NOTE_EXAMPLE_CAP)
        .map(|path| display_relative_path(root, path))
        .collect();
    if examples.len() > NOTE_EXAMPLE_CAP {
        format!("{}, and more", shown.join(", "))
    } else {
        shown.join(", ")
    }
}

/// Render `path` relative to `root` with forward slashes. Cross-platform
/// output stability depends on the slash normalisation.
fn display_relative_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .display()
        .to_string()
        .replace('\\', "/")
}

/// Whether a candidate file inside a skipped dotdir is one this run had
/// already excluded from analysis, matching what [`FileVisitor`] applies to
/// every discovered file: the compiled `ignorePatterns` set (user entries plus
/// the built-in defaults) against the ROOT-RELATIVE path, plus the production
/// excludes when the run is a `--production` run.
///
/// The root-relative path is what matters. Matching the directory path instead
/// would miss the pattern a user actually writes, because `.claude/**` does not
/// match `.claude`.
///
/// Production excludes ARE applied, even though the skip the diagnostic reports
/// is a traversal decision that `--production` does not change. A `--production`
/// run that named a dotdir holding only `thing.test.ts` would print a remedy
/// (`fallow --root .qa --production`) that returns nothing, so the advisory has
/// to agree with the file set the run would actually analyze. Combined mode's
/// two walks can therefore disagree, which the documented union semantics of
/// the combined root already cover.
fn is_excluded_from_analysis(
    config: &ResolvedConfig,
    production_excludes: Option<&globset::GlobSet>,
    path: &Path,
) -> bool {
    let relative = path.strip_prefix(&config.root).unwrap_or(path);
    config.ignore_patterns.is_match(relative)
        || production_excludes.is_some_and(|excludes| excludes.is_match(relative))
}

/// True when `path` carries an extension that puts it in the module graph the
/// advisory describes. See [`DOTDIR_MODULE_EXTENSIONS`] for why this is
/// narrower than [`has_source_extension`].
fn has_module_extension(path: &Path) -> bool {
    path.extension()
        .and_then(OsStr::to_str)
        .is_some_and(|ext| DOTDIR_MODULE_EXTENSIONS.contains(&ext))
}

/// Depth- and entry-capped search for one reportable source file, stopping at
/// the first hit. Never a recursive walk of an unbounded tree: the ceiling is
/// `1 + DOTDIR_SCAN_MAX_ENTRIES` directory reads for one dotdir, and
/// [`DOTDIR_SCAN_TOTAL_ENTRIES`] across the whole walk.
///
/// Runs on `ignore::WalkBuilder` with the same git settings as the source walk
/// rather than a bare `read_dir`, because "the project has not excluded it" has
/// to mean what git means. Only the DIRECTORY form of a gitignore rule
/// (`.build-tools/`) prunes a dotdir before the walk's own filter sees it: the
/// `dir/**`, `dir/*`, `**/dir/**` and file-level (`*.ts`) forms all leave the
/// directory reaching this scan with every file inside it ignored, and a cache
/// directory that ignores itself through its own nested `.gitignore` does the
/// same. Reporting those would advertise two remedies that both do nothing,
/// since a re-rooted `fallow --root <dir>` still reads the parent repository's
/// gitignore and would find no files either.
///
/// Accepted tradeoff: for a dotdir with more than [`DOTDIR_SCAN_MAX_ENTRIES`]
/// entries whose only source file sits past the budget, the verdict is
/// readdir-order dependent, so the advisory can flap between runs. The
/// alternative is unbounded I/O, and the consequence of a flap is a missing
/// advisory line, never a changed analysis. No test may depend on that
/// boundary.
fn scan_for_reportable_source(
    config: &ResolvedConfig,
    production_excludes: Option<&globset::GlobSet>,
    dir: &Path,
    budget: &mut usize,
) -> bool {
    let mut builder = WalkBuilder::new(dir);
    builder
        .hidden(false)
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true)
        .follow_links(false)
        .max_depth(Some(DOTDIR_SCAN_MAX_DEPTH + 1))
        .threads(1);
    builder.filter_entry(|entry| {
        if entry.depth() == 0 || !entry.file_type().is_some_and(|ft| ft.is_dir()) {
            return true;
        }
        entry
            .file_name()
            .to_str()
            .is_none_or(|name| !SCRIPT_SCOPE_DENYLIST.contains(&name) && name != "node_modules")
    });

    let mut per_dotdir = DOTDIR_SCAN_MAX_ENTRIES;
    for entry in builder.build() {
        if per_dotdir == 0 || *budget == 0 {
            return false;
        }
        per_dotdir -= 1;
        *budget -= 1;
        let Ok(entry) = entry else {
            continue;
        };
        // Regular files only. A symlink is never followed out of the scan, and
        // a fifo or a socket named `pipe.ts` is not source either.
        #[expect(
            clippy::filetype_is_file,
            reason = "regular files only is the point: !is_dir() would readmit fifos and sockets"
        )]
        let is_regular_file = entry
            .file_type()
            .is_some_and(|file_type| file_type.is_file());
        if !is_regular_file {
            continue;
        }
        if has_module_extension(entry.path())
            && !is_excluded_from_analysis(config, production_excludes, entry.path())
        {
            return true;
        }
    }
    false
}

/// Path components whose subtrees never earn the skipped-source-dotdir
/// advisory. A hidden directory under one of these is test scaffolding rather
/// than first-party source the project meant to analyze.
const DOTDIR_NOISE_PATH_COMPONENTS: &[&str] = &[
    "__fixtures__",
    "__mocks__",
    "__tests__",
    "e2e",
    "fixture",
    "fixtures",
    "playground",
    "playgrounds",
    "spec",
    "test",
    "tests",
];

/// Whether a dropped dotdir is worth opening at all. Decided from the PATH
/// alone, so it costs no I/O and runs before the scan budget is touched:
/// `.git` in a large repository, a `.jj` object store, and `node_modules/.pnpm`
/// are never opened. `ALLOWED_HIDDEN_DIRS` and every plugin- or
/// script-contributed scope are already excluded by construction, since a
/// directory they admit is never dropped and so never reaches this list.
fn dotdir_is_scan_candidate(config: &ResolvedConfig, dir: &Path) -> bool {
    let Some(name) = dir.file_name().and_then(OsStr::to_str) else {
        return false;
    };
    if SCRIPT_SCOPE_DENYLIST.contains(&name) {
        return false;
    }
    let relative = dir.strip_prefix(&config.root).unwrap_or(dir);
    // Pure cost saving, not a further condition: the built-in `**/node_modules/**`
    // ignore default makes every file under a `node_modules` component ignored,
    // so the scan could only ever return false.
    if relative
        .components()
        .any(|component| component.as_os_str() == OsStr::new("node_modules"))
    {
        return false;
    }
    // Precision, and the one place this check is deliberately less complete
    // than it could be. A hidden directory under a test, fixture, or playground
    // tree is usually there BECAUSE it is hidden: some of these exist purely to
    // exercise hidden-directory handling, so an advisory about them is wrong
    // about the project every time it fires. Measured on a ten-repository
    // corpus, this component filter removes every false positive one framework
    // contributed and half of another's while keeping the true positives, which
    // sit at a repository root rather than under a test tree.
    !relative.components().any(|component| {
        DOTDIR_NOISE_PATH_COMPONENTS.contains(&component.as_os_str().to_string_lossy().as_ref())
    })
}

/// Build the typed diagnostics for the dot-prefixed directories this walk
/// dropped that hold source files the project has not excluded, and emit one
/// aggregated `tracing::warn!` so the otherwise silent skip is visible on
/// stderr too (issue #461). The caller writes the returned list to the
/// registry.
fn report_skipped_source_dotdirs(
    config: &ResolvedConfig,
    production_excludes: Option<&globset::GlobSet>,
    candidates: &[PathBuf],
) -> Vec<WorkspaceDiagnostic> {
    if candidates.is_empty() {
        return Vec::new();
    }
    // The caller sorted and deduped, so both caps truncate deterministically:
    // the same tree reports the same prefix on every run.
    let mut budget = DOTDIR_SCAN_TOTAL_ENTRIES;
    let scannable: Vec<&PathBuf> = candidates
        .iter()
        .filter(|dir| dotdir_is_scan_candidate(config, dir))
        .collect();
    let reportable: Vec<&PathBuf> = scannable
        .iter()
        .copied()
        .take(DOTDIR_SCAN_MAX_CANDIDATES)
        .filter(|dir| scan_for_reportable_source(config, production_excludes, dir, &mut budget))
        .collect();
    // Either ceiling can stop the scan with candidates left unexamined, so the
    // count is a floor rather than a total whenever one of them binds.
    let truncated = scannable.len() > DOTDIR_SCAN_MAX_CANDIDATES || budget == 0;
    if reportable.is_empty() {
        return Vec::new();
    }

    let diagnostics: Vec<WorkspaceDiagnostic> = reportable
        .iter()
        .map(|dir| {
            WorkspaceDiagnostic::new(
                &config.root,
                (*dir).clone(),
                WorkspaceDiagnosticKind::SkippedSourceDotdir,
            )
        })
        .collect();

    let count = reportable.len();
    if !config.quiet
        && should_emit_note_once(format!(
            "dotdir::{}::{count}::{}",
            config.root.display(),
            reportable
                .first()
                .map_or_else(String::new, |dir| display_relative_path(&config.root, dir))
        ))
    {
        tracing::warn!(
            "{}",
            build_skipped_dotdirs_note(&config.root, &reportable, truncated)
        );
    }
    diagnostics
}

/// Build the skipped-source-dotdir note. Pure so the singular and plural forms,
/// the truncated prefix, and the single-directory remedy substitution are
/// unit-testable without a tracing subscriber, mirroring
/// [`build_largest_files_note`].
///
/// With exactly one directory the remedy names it instead of printing a `<dir>`
/// placeholder: the path is already known and was printed a few words earlier,
/// so a placeholder would make the one case a user can act on directly the one
/// case they have to retype.
fn build_skipped_dotdirs_note(root: &Path, reportable: &[&PathBuf], truncated: bool) -> String {
    let count = reportable.len();
    // An exact remainder inside an explicitly inexact total reads as a
    // contradiction ("at least 64 ... and 59 more"), so a truncated run drops
    // the tail count.
    let examples = if truncated {
        summarize_paths_open_ended(root, reportable)
    } else {
        summarize_paths(root, reportable)
    };
    let noun = if count == 1 {
        "directory"
    } else {
        "directories"
    };
    let verb = if count == 1 { "contains" } else { "contain" };
    let at_least = if truncated { "at least " } else { "" };
    let (target, pronoun) = match reportable {
        [only] => (display_relative_path(root, only), "it"),
        _ => ("<dir>".to_owned(), "one"),
    };
    format!(
        "fallow: skipped {at_least}{count} hidden {noun} that {verb} source files ({examples}). \
         Hidden directories are not traversed and no config field adds one: analyze {pronoun} \
         with fallow --root {target} if it holds first-party source, or add '{target}/**' to \
         ignorePatterns to silence this."
    )
}

/// Build the pre-parse largest-files note, or `None` when the discovered set is
/// neither unusually large nor contains an unusually large file. Pure so the
/// pluralization, floor filtering, and count-only fallback are unit-testable
/// without a tracing subscriber. See issue #1086.
fn build_largest_files_note(root: &Path, files: &[DiscoveredFile]) -> Option<String> {
    if files.is_empty() {
        return None;
    }
    let largest = files.iter().map(|f| f.size_bytes).max().unwrap_or(0);
    if files.len() <= LARGE_SET_THRESHOLD && largest < LARGE_FILE_NOTE_BYTES {
        return None;
    }
    let count = files.len();
    let noun = if count == 1 { "file" } else { "files" };
    let mut by_size: Vec<SizedFile> = files
        .iter()
        .filter(|f| f.size_bytes >= NOTE_FILE_FLOOR_BYTES)
        .map(|f| (f.path.clone(), f.size_bytes))
        .collect();
    by_size.sort_unstable_by_key(|f| std::cmp::Reverse(f.1));
    if by_size.is_empty() {
        // Large file SET with no individually large file: report the count only,
        // omitting a "largest:" list that would otherwise be all sub-floor noise.
        return Some(format!(
            "fallow: discovered {count} {noun}. If analysis stalls or runs out of memory, \
             exclude large generated files via ignorePatterns or --max-file-size."
        ));
    }
    let examples = summarize_examples(root, &by_size);
    Some(format!(
        "fallow: discovered {count} {noun}; largest: {examples}. If analysis stalls or runs out of memory, \
         exclude large generated files via ignorePatterns or --max-file-size."
    ))
}

/// Emit a pre-parse note listing the largest kept files when the discovered set
/// is unusually large or contains an unusually large file, so an out-of-memory
/// hang at the parse stage is diagnosable (issue #1086). Visible before the
/// expensive parse begins, so it survives a subsequent crash.
fn note_largest_files(config: &ResolvedConfig, files: &[DiscoveredFile]) {
    if config.quiet {
        return;
    }
    if let Some(message) = build_largest_files_note(&config.root, files)
        && should_emit_note_once(format!("note::{}::{}", config.root.display(), files.len()))
    {
        tracing::warn!("{message}");
    }
}

/// How a [`HiddenDirScope`] matches a hidden directory during the walk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HiddenDirMatch {
    /// Match by directory NAME at any depth beneath the scope root.
    ///
    /// Framework plugins declare bundle-boundary conventions like `.client`
    /// and `.server` that a project may place under any route directory, so
    /// the name is the whole rule and the depth is not knowable in advance.
    AnyDepth,
    /// Match the exact root-relative directory PATH.
    ///
    /// A `package.json` script naming `.a/.b/build.mjs` states where the file
    /// it needs actually lives, so the scope admits `<root>/.a` and
    /// `<root>/.a/.b` and nothing else. An unrelated `packages/x/.b` stays
    /// untraversed (issue #461).
    ExactPath,
}

/// Package-scoped hidden directories that source discovery should traverse.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HiddenDirScope {
    root: PathBuf,
    dirs: Vec<String>,
    match_mode: HiddenDirMatch,
}

impl HiddenDirScope {
    /// Build a scope rooted at a package directory that admits the given
    /// hidden directory names at any depth beneath it.
    ///
    /// This is the plugin-contributed shape. For a scope inferred from a
    /// concrete path, use [`HiddenDirScope::new_exact_paths`], which does not
    /// admit the same name elsewhere in the tree.
    #[must_use]
    pub fn new(root: PathBuf, dirs: Vec<String>) -> Self {
        Self {
            root,
            dirs,
            match_mode: HiddenDirMatch::AnyDepth,
        }
    }

    /// Build a scope rooted at a package directory that admits exactly the
    /// given root-relative directory paths.
    #[must_use]
    pub fn new_exact_paths(root: PathBuf, dirs: Vec<String>) -> Self {
        Self {
            root,
            dirs,
            match_mode: HiddenDirMatch::ExactPath,
        }
    }

    /// Rebuild a scope with an explicit match mode.
    ///
    /// Used when a scope crosses a crate boundary and must arrive with the
    /// same semantics it left with.
    #[must_use]
    pub fn with_match_mode(root: PathBuf, dirs: Vec<String>, match_mode: HiddenDirMatch) -> Self {
        Self {
            root,
            dirs,
            match_mode,
        }
    }

    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    #[must_use]
    pub fn dirs(&self) -> &[String] {
        &self.dirs
    }

    #[must_use]
    pub fn match_mode(&self) -> HiddenDirMatch {
        self.match_mode
    }

    fn allows(&self, path: &Path, name: &OsStr) -> bool {
        match self.match_mode {
            HiddenDirMatch::AnyDepth => {
                path.starts_with(&self.root) && self.dirs.iter().any(|dir| OsStr::new(dir) == name)
            }
            HiddenDirMatch::ExactPath => {
                // `Path` compares component-wise, so a `/`-separated entry
                // from a script string matches on every platform.
                let Ok(relative) = path.strip_prefix(&self.root) else {
                    return false;
                };
                self.dirs.iter().any(|dir| Path::new(dir) == relative)
            }
        }
    }
}

/// Per-thread file collector for the parallel walker.
///
/// Source files (by extension) flow to `shared`; when `config_shared` is set,
/// non-source files admitted by the config-candidate type group flow to it
/// instead. The two channels are disjoint and the source channel is byte-for-byte
/// identical to the config-capture-disabled walk.
struct FileVisitor<'a> {
    root: &'a Path,
    canonical_root: Option<&'a Path>,
    ignore_patterns: &'a globset::GlobSet,
    production_excludes: &'a Option<globset::GlobSet>,
    shared: &'a Mutex<Vec<(std::path::PathBuf, u64)>>,
    config_shared: Option<&'a Mutex<Vec<std::path::PathBuf>>>,
    local: Vec<(std::path::PathBuf, u64)>,
    config_local: Vec<std::path::PathBuf>,
}

impl ignore::ParallelVisitor for FileVisitor<'_> {
    fn visit(&mut self, result: Result<ignore::DirEntry, ignore::Error>) -> ignore::WalkState {
        let Ok(entry) = result else {
            return ignore::WalkState::Continue;
        };
        if entry.file_type().is_some_and(|ft| ft.is_dir()) {
            return ignore::WalkState::Continue;
        }
        let relative = entry
            .path()
            .strip_prefix(self.root)
            .unwrap_or_else(|_| entry.path());
        if self.ignore_patterns.is_match(relative) {
            return ignore::WalkState::Continue;
        }
        if self
            .production_excludes
            .as_ref()
            .is_some_and(|excludes| excludes.is_match(relative))
        {
            return ignore::WalkState::Continue;
        }
        let symlink_size = if entry.file_type().is_some_and(|ft| ft.is_symlink()) {
            let Some(size) = contained_symlink_file_size(entry.path(), self.canonical_root) else {
                tracing::debug!(
                    path = %entry.path().display(),
                    "skipping source symlink with a broken, non-file, or outside-root target"
                );
                return ignore::WalkState::Continue;
            };
            Some(size)
        } else {
            None
        };
        if has_source_extension(entry.path()) {
            let size_bytes =
                symlink_size.unwrap_or_else(|| entry.metadata().map_or(0, |m| m.len()));
            self.local.push((entry.into_path(), size_bytes));
        } else if self.config_shared.is_some() {
            // A non-source file admitted by the config-candidate type group. No
            // size metadata is needed; these are pattern-matched, never parsed.
            self.config_local.push(entry.into_path());
        }
        ignore::WalkState::Continue
    }
}

fn contained_symlink_file_size(path: &Path, canonical_root: Option<&Path>) -> Option<u64> {
    let root = canonical_root?;
    let target = path.canonicalize().ok()?;
    if !target.starts_with(root) {
        return None;
    }
    let metadata = target.metadata().ok()?;
    metadata.is_file().then_some(metadata.len())
}

impl Drop for FileVisitor<'_> {
    #[expect(
        clippy::expect_used,
        reason = "poisoned walk collector lock means worker state is unrecoverable"
    )]
    fn drop(&mut self) {
        if !self.local.is_empty() {
            self.shared
                .lock()
                .expect("walk collector lock poisoned")
                .append(&mut self.local);
        }
        if let Some(config_shared) = self.config_shared
            && !self.config_local.is_empty()
        {
            config_shared
                .lock()
                .expect("walk config collector lock poisoned")
                .append(&mut self.config_local);
        }
    }
}

/// Builder that creates per-thread `FileVisitor` instances for the parallel walker.
struct FileVisitorBuilder<'a> {
    root: &'a Path,
    canonical_root: Option<&'a Path>,
    ignore_patterns: &'a globset::GlobSet,
    production_excludes: &'a Option<globset::GlobSet>,
    shared: &'a Mutex<Vec<(std::path::PathBuf, u64)>>,
    config_shared: Option<&'a Mutex<Vec<std::path::PathBuf>>>,
}

impl<'s> ignore::ParallelVisitorBuilder<'s> for FileVisitorBuilder<'s> {
    fn build(&mut self) -> Box<dyn ignore::ParallelVisitor + 's> {
        Box::new(FileVisitor {
            root: self.root,
            canonical_root: self.canonical_root,
            ignore_patterns: self.ignore_patterns,
            production_excludes: self.production_excludes,
            shared: self.shared,
            config_shared: self.config_shared,
            local: Vec::new(),
            config_local: Vec::new(),
        })
    }
}

/// File extensions discovery treats as analyzable source files.
pub const SOURCE_EXTENSIONS: &[&str] = &[
    "ts", "tsx", "mts", "cts", "gts", "js", "jsx", "mjs", "cjs", "gjs", "vue", "svelte", "astro",
    "mdx", "css", "scss", "sass", "less", "html", "graphql", "gql",
];

/// Glob patterns for test/dev/story files excluded in production mode.
pub const PRODUCTION_EXCLUDE_PATTERNS: &[&str] = &[
    "**/*.test.*",
    "**/*.spec.*",
    "**/*.e2e.*",
    "**/*.e2e-spec.*",
    "**/*.bench.*",
    "**/*.fixture.*",
    "**/*.stories.*",
    "**/*.story.*",
    "**/__tests__/**",
    "**/__mocks__/**",
    "**/__snapshots__/**",
    "**/__fixtures__/**",
    "**/test/**",
    "**/tests/**",
    "*.config.*",
    "**/.*.js",
    "**/.*.ts",
    "**/.*.mjs",
    "**/.*.cjs",
];

/// Check if a hidden directory name is on the allowlist.
pub fn is_allowed_hidden_dir(name: &OsStr) -> bool {
    ALLOWED_HIDDEN_DIRS.iter().any(|&d| OsStr::new(d) == name)
}

fn is_allowed_scoped_hidden_dir(
    name: &OsStr,
    path: &Path,
    additional_hidden_dir_scopes: &[HiddenDirScope],
) -> bool {
    additional_hidden_dir_scopes
        .iter()
        .any(|scope| scope.allows(path, name))
}

/// Files Yarn Plug'n'Play writes at the workspace root. They carry source
/// extensions (`.pnp.cjs` is the multi-megabyte generated loader with the
/// install state inlined, `.pnp.loader.mjs` its ESM shim) but are install
/// artifacts, not project source, so the walker drops them by name.
const YARN_PNP_GENERATED_FILES: &[&str] = &[".pnp.cjs", ".pnp.loader.mjs"];

fn is_yarn_pnp_generated_file(name: &OsStr) -> bool {
    YARN_PNP_GENERATED_FILES
        .iter()
        .any(|&f| OsStr::new(f) == name)
}

/// Check if a hidden directory entry should be allowed through the filter.
///
/// Returns `true` if the entry is not hidden or is on the allowlist.
/// Hidden files (not directories) are allowed through since the type filter
/// handles them, except for the generated Yarn PnP files.
fn is_allowed_hidden_with_scopes(
    entry: &ignore::DirEntry,
    additional_hidden_dir_scopes: &[HiddenDirScope],
) -> bool {
    let name = entry.file_name();
    let name_str = name.to_string_lossy();

    if !name_str.starts_with('.') {
        return true;
    }

    if entry.file_type().is_some_and(|ft| !ft.is_dir()) {
        return !is_yarn_pnp_generated_file(name);
    }

    is_allowed_hidden_dir(name)
        || is_allowed_scoped_hidden_dir(name, entry.path(), additional_hidden_dir_scopes)
}

/// Discover all source files in the project.
///
/// # Panics
///
/// Panics if the file type glob or progress template is invalid (compile-time constants).
pub fn discover_files(config: &ResolvedConfig) -> Vec<DiscoveredFile> {
    discover_files_with_additional_hidden_dirs(config, &[])
}

/// The set of config-file basenames (last path component of every built-in
/// plugin `config_patterns()` entry, brace forms preserved) that the walk should
/// additionally admit so non-source configs (`tsconfig.json`, `bunfig.toml`,
/// `.eslintrc.json`, ...) can be captured in one traversal instead of being
/// re-discovered by a filesystem re-walk in `discover_config_files`.
///
/// Derived live from the built-in plugin list, so it can never drift behind a
/// new plugin's config patterns. Source-extension config basenames
/// (`vite.config.{ts,js}`) are admitted too, but the walk visitor routes them
/// back to the source channel by extension, so the config channel only ever
/// collects genuinely non-source files.
fn config_candidate_basename_globs() -> &'static [String] {
    static GLOBS: OnceLock<Vec<String>> = OnceLock::new();
    GLOBS.get_or_init(|| {
        let mut set: FxHashSet<String> = FxHashSet::default();
        for plugin in crate::plugins::registry::builtin::create_builtin_plugins() {
            for pattern in plugin.config_patterns() {
                let basename = pattern.rsplit('/').next().unwrap_or(pattern);
                set.insert(basename.to_string());
            }
        }
        let mut globs: Vec<String> = set.into_iter().collect();
        globs.sort_unstable();
        globs
    })
}

/// True when `path`'s extension is one of the known source extensions, i.e. the
/// file belongs in the source channel rather than the config-candidate channel.
fn has_source_extension(path: &Path) -> bool {
    path.extension()
        .and_then(OsStr::to_str)
        .is_some_and(|ext| SOURCE_EXTENSIONS.contains(&ext))
}

/// Build the file-type filter. Always selects known source extensions; when
/// `capture_config` is set, also selects config-candidate basenames so the
/// walker yields them for the second collection channel.
#[expect(
    clippy::expect_used,
    reason = "source file globs are hard-coded compile-time constants"
)]
fn build_walk_types(capture_config: bool) -> ignore::types::Types {
    static SOURCE_TYPES: OnceLock<ignore::types::Types> = OnceLock::new();
    static SOURCE_AND_CONFIG_TYPES: OnceLock<ignore::types::Types> = OnceLock::new();

    let cache = if capture_config {
        &SOURCE_AND_CONFIG_TYPES
    } else {
        &SOURCE_TYPES
    };
    cache
        .get_or_init(|| {
            let mut types_builder = ignore::types::TypesBuilder::new();
            let source_glob = format!("*.{{{}}}", SOURCE_EXTENSIONS.join(","));
            types_builder
                .add("source", &source_glob)
                .expect("valid glob");
            types_builder.select("source");
            if capture_config {
                for glob in config_candidate_basename_globs() {
                    // Ignore individually-invalid plugin patterns rather than panicking;
                    // a malformed pattern simply fails to admit its config file (the
                    // pre-existing filesystem fallback still covers production mode).
                    let _ = types_builder.add("config", glob);
                }
                types_builder.select("config");
            }
            types_builder.build().expect("valid types")
        })
        .clone()
}

/// Construct the parallel walker, applying the appropriate hidden-dir filter.
/// When `capture_config` is set the walk also yields config-candidate files for
/// the secondary collection channel.
fn build_source_walk_builder(
    config: &ResolvedConfig,
    additional_hidden_dir_scopes: &[HiddenDirScope],
    capture_config: bool,
    skipped_dotdirs: &SkippedDotdirSink,
) -> WalkBuilder {
    let mut walk_builder = WalkBuilder::new(&config.root);
    walk_builder
        .hidden(false)
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true)
        .types(build_walk_types(capture_config))
        .threads(config.threads);
    // One filter, not two: `filter_entry` replaces rather than chains, and the
    // dropped-dotdir record has to happen on the same false path that decides
    // the skip so the allowlist and every plugin- or script-contributed scope
    // are excluded by construction (issue #461).
    let scopes = additional_hidden_dir_scopes.to_vec();
    let sink = Arc::clone(skipped_dotdirs);
    walk_builder.filter_entry(move |entry| {
        if is_allowed_hidden_with_scopes(entry, &scopes) {
            return true;
        }
        if entry.file_type().is_some_and(|ft| ft.is_dir())
            && let Ok(mut collected) = sink.lock()
        {
            collected.push(entry.path().to_path_buf());
        }
        false
    });
    walk_builder
}

/// Compile the production-mode exclude glob set, or `None` outside production mode.
fn build_production_excludes(config: &ResolvedConfig) -> Option<globset::GlobSet> {
    if !config.production {
        return None;
    }
    let mut builder = globset::GlobSetBuilder::new();
    for pattern in PRODUCTION_EXCLUDE_PATTERNS {
        if let Ok(glob) = globset::GlobBuilder::new(pattern)
            .literal_separator(true)
            .build()
        {
            builder.add(glob);
        }
    }
    builder.build().ok()
}

/// Discover all source files in the project, with package-scoped hidden dirs.
///
/// # Panics
///
/// Panics if the file type glob or progress template is invalid (compile-time constants).
pub fn discover_files_with_additional_hidden_dirs(
    config: &ResolvedConfig,
    additional_hidden_dir_scopes: &[HiddenDirScope],
) -> Vec<DiscoveredFile> {
    discover_files_and_config_candidates(config, additional_hidden_dir_scopes).0
}

/// Discover source files AND, in one traversal, the non-source config-candidate
/// files (`tsconfig.json`, `bunfig.toml`, `.eslintrc.json`, ...) used by
/// `discover_config_files` to resolve plugin config patterns in-memory instead of
/// re-walking the filesystem.
///
/// The returned `Vec<DiscoveredFile>` is byte-for-byte identical to the
/// config-capture-disabled walk: config candidates are routed to the second
/// return value by extension and never enter the source channel. Config capture
/// is skipped in production mode (where the walk applies `PRODUCTION_EXCLUDE_PATTERNS`
/// and `discover_config_files` keeps its filesystem path), so the second vector is
/// empty there.
///
/// # Panics
///
/// Panics if the file type glob or progress template is invalid (compile-time constants).
pub fn discover_files_and_config_candidates(
    config: &ResolvedConfig,
    additional_hidden_dir_scopes: &[HiddenDirScope],
) -> (Vec<DiscoveredFile>, Vec<PathBuf>) {
    let discovered =
        discover_files_config_candidates_and_diagnostics(config, additional_hidden_dir_scopes);
    (discovered.files, discovered.config_candidates)
}

/// Source files, config candidates, and the source-discovery diagnostics one
/// walk produced.
///
/// `diagnostics` is the walk's OWN skip list, not a read of the process-wide
/// registry: combined mode can run two walks on the same root concurrently, and
/// each walk replaces the registry's source-discovery entries, so only the
/// by-value list is a stable answer to "what did THIS analysis skip" (issue
/// #2366).
pub struct DiscoveredSources {
    /// Source files with stable path-sorted [`FileId`]s.
    pub files: Vec<DiscoveredFile>,
    /// Non-source config-candidate paths captured in the same traversal.
    pub config_candidates: Vec<PathBuf>,
    /// Skipped-large-file, skipped-minified-file, and skipped-source-dotdir
    /// diagnostics from this walk.
    pub diagnostics: Vec<WorkspaceDiagnostic>,
}

/// [`discover_files_and_config_candidates`] plus the source-discovery
/// diagnostics this walk recorded, for callers that must carry a per-analysis
/// snapshot instead of reading the shared registry back (issue #2366).
///
/// # Panics
///
/// Panics if the file type glob or progress template is invalid (compile-time constants).
#[expect(
    clippy::cast_possible_truncation,
    reason = "file count is bounded by project size, well under u32::MAX"
)]
#[expect(clippy::expect_used, reason = "the collector lock must remain usable")]
pub fn discover_files_config_candidates_and_diagnostics(
    config: &ResolvedConfig,
    additional_hidden_dir_scopes: &[HiddenDirScope],
) -> DiscoveredSources {
    let _span = tracing::info_span!("discover_files").entered();

    let capture_config = !config.production;
    let skipped_dotdirs: SkippedDotdirSink = Arc::new(Mutex::new(Vec::new()));
    let walk_builder = build_source_walk_builder(
        config,
        additional_hidden_dir_scopes,
        capture_config,
        &skipped_dotdirs,
    );
    let production_excludes = build_production_excludes(config);
    let canonical_root = config.root.canonicalize().ok();

    let collected: Mutex<Vec<(std::path::PathBuf, u64)>> = Mutex::new(Vec::new());
    let config_collected: Mutex<Vec<std::path::PathBuf>> = Mutex::new(Vec::new());
    let mut visitor_builder = FileVisitorBuilder {
        root: &config.root,
        canonical_root: canonical_root.as_deref(),
        ignore_patterns: &config.ignore_patterns,
        production_excludes: &production_excludes,
        shared: &collected,
        config_shared: capture_config.then_some(&config_collected),
    };
    walk_builder.build_parallel().visit(&mut visitor_builder);

    let mut raw = collected
        .into_inner()
        .expect("walk collector lock poisoned");
    // ADR-004 (path-sorted FileIds): the parallel walk visits files in
    // nondeterministic order, so we sort by absolute path BEFORE the
    // `.enumerate()` FileId assignment below. This is the stable-cross-run
    // identity invariant the persisted graph cache depends on: an identical
    // file set yields identical FileIds, so a cache hit (same paths +
    // fingerprints) can trust graph data persisted by FileId. Do not replace
    // this with insertion-order assignment.
    raw.sort_unstable_by(|a, b| a.0.cmp(&b.0));

    let mut config_candidates = config_collected
        .into_inner()
        .expect("walk config collector lock poisoned");
    config_candidates.sort_unstable();

    // The parallel walk records dotdirs in nondeterministic thread order, and
    // the diagnostic array order is part of the JSON contract, so sort and
    // dedupe before the predicate runs (issue #2366).
    let mut dotdir_candidates = skipped_dotdirs
        .lock()
        .map_or_else(|_| Vec::new(), |mut guard| std::mem::take(&mut *guard));
    dotdir_candidates.sort_unstable();
    dotdir_candidates.dedup();

    let (kept, skipped) = partition_by_size(raw, config.max_file_size_bytes);
    let (kept, skipped_minified) =
        partition_minified_generated_js(kept, config.max_file_size_bytes);
    // One registry write replaces this root's whole source-discovery set, so a
    // stale entry from a previous pass drops out (issue #1086) without a window
    // in which a concurrent walk on the same root can observe or clobber a
    // half-written set (issue #2366).
    let diagnostics = fallow_config::replace_source_discovery_diagnostics(
        &config.root,
        report_skipped_large_files(config, &skipped)
            .into_iter()
            .chain(report_skipped_minified_files(config, &skipped_minified))
            .chain(report_skipped_source_dotdirs(
                config,
                production_excludes.as_ref(),
                &dotdir_candidates,
            ))
            .collect(),
    );

    let files: Vec<DiscoveredFile> = kept
        .into_iter()
        .enumerate()
        .map(|(idx, (path, size_bytes))| DiscoveredFile {
            id: FileId(idx as u32),
            path,
            size_bytes,
        })
        .collect();

    note_largest_files(config, &files);

    DiscoveredSources {
        files,
        config_candidates,
        diagnostics,
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;
    use std::path::MAIN_SEPARATOR;

    use super::*;

    #[test]
    fn skipped_dotdirs_note_names_the_directory_when_there_is_one() {
        let root = Path::new("/repo");
        let only = PathBuf::from("/repo/.tooling");
        let note = build_skipped_dotdirs_note(root, &[&only], false);
        assert!(note.contains("skipped 1 hidden directory that contains source files"));
        assert!(note.contains("analyze it with fallow --root .tooling"));
        assert!(note.contains("add '.tooling/**' to"));
        assert!(
            !note.contains("<dir>"),
            "the single-directory remedy must be copy-pasteable: {note}"
        );
    }

    #[test]
    fn skipped_dotdirs_note_pluralizes_and_keeps_the_placeholder() {
        let root = Path::new("/repo");
        let a = PathBuf::from("/repo/.a");
        let b = PathBuf::from("/repo/.b");
        let note = build_skipped_dotdirs_note(root, &[&a, &b], false);
        assert!(note.contains("skipped 2 hidden directories that contain source files"));
        assert!(note.contains("analyze one with fallow --root <dir>"));
    }

    #[test]
    fn skipped_dotdirs_note_drops_the_tail_count_when_truncated() {
        let root = Path::new("/repo");
        let owned: Vec<PathBuf> = (0..8)
            .map(|i| PathBuf::from(format!("/repo/.d{i}")))
            .collect();
        let reportable: Vec<&PathBuf> = owned.iter().collect();

        let bounded = build_skipped_dotdirs_note(root, &reportable, true);
        assert!(bounded.contains("skipped at least 8 hidden directories"));
        assert!(
            bounded.contains("and more") && !bounded.contains("and 3 more"),
            "an inexact total must not carry an exact remainder: {bounded}"
        );

        let complete = build_skipped_dotdirs_note(root, &reportable, false);
        assert!(!complete.contains("at least"));
        assert!(complete.contains("and 3 more"));
    }

    #[test]
    fn dotdir_noise_path_components_stay_sorted_and_lowercase() {
        let mut sorted = DOTDIR_NOISE_PATH_COMPONENTS.to_vec();
        sorted.sort_unstable();
        assert_eq!(sorted, DOTDIR_NOISE_PATH_COMPONENTS);
        for component in DOTDIR_NOISE_PATH_COMPONENTS {
            assert!(!component.starts_with('.'), "'{component}' is not hidden");
            assert_eq!(
                *component,
                component.to_lowercase(),
                "'{component}' is matched verbatim against a path component"
            );
        }
    }

    #[test]
    fn script_scope_denylist_stays_disjoint_and_sorted() {
        let mut sorted = SCRIPT_SCOPE_DENYLIST.to_vec();
        sorted.sort_unstable();
        assert_eq!(
            sorted, SCRIPT_SCOPE_DENYLIST,
            "keep the list sorted so additions stay reviewable"
        );
        for dir in SCRIPT_SCOPE_DENYLIST {
            assert!(dir.starts_with('.'), "'{dir}' is not a hidden directory");
            assert!(
                !ALLOWED_HIDDEN_DIRS.contains(dir),
                "'{dir}' is traversed, so it can never be a skipped candidate"
            );
        }
    }

    #[test]
    fn dotdir_module_extensions_are_a_subset_of_source_extensions() {
        for ext in DOTDIR_MODULE_EXTENSIONS {
            assert!(
                SOURCE_EXTENSIONS.contains(ext),
                "'{ext}' is not discovered as source, so it cannot be a trigger"
            );
        }
        for ext in ["css", "scss", "sass", "less", "html", "graphql", "gql"] {
            assert!(
                !DOTDIR_MODULE_EXTENSIONS.contains(&ext),
                "'{ext}' carries no imports or exports for the message to be about"
            );
        }
    }

    /// Reproduce the FileId-assignment rule used by `walk_source_files`: sort by
    /// absolute path, then assign `FileId(idx)` in that order.
    fn assign_file_ids(mut raw: Vec<(std::path::PathBuf, u64)>) -> Vec<DiscoveredFile> {
        raw.sort_unstable_by(|a, b| a.0.cmp(&b.0));
        raw.into_iter()
            .enumerate()
            .map(|(idx, (path, size_bytes))| DiscoveredFile {
                id: FileId(idx as u32),
                path,
                size_bytes,
            })
            .collect()
    }

    /// ADR-004: an identical file set must yield identical FileIds regardless of
    /// the (nondeterministic, parallel) discovery order. The persisted graph
    /// cache keys persisted graph data by FileId, so a cache HIT (same paths +
    /// fingerprints) must reproduce the exact same FileId-to-path mapping the
    /// graph was built against. This guards the cache's soundness prerequisite.
    #[test]
    fn file_id_assignment_is_deterministic_for_identical_file_set() {
        let paths = [
            "/project/src/z.ts",
            "/project/src/a.ts",
            "/project/src/components/Button.tsx",
            "/project/src/components/Button.module.css",
            "/project/index.ts",
        ];

        // Two independent walks that observe the same paths in DIFFERENT orders.
        let walk_one: Vec<(std::path::PathBuf, u64)> = paths
            .iter()
            .map(|p| (std::path::PathBuf::from(p), 10))
            .collect();
        let mut walk_two = walk_one.clone();
        walk_two.reverse();

        let files_one = assign_file_ids(walk_one);
        let files_two = assign_file_ids(walk_two);

        // Identical (FileId -> path) mapping despite the different walk orders.
        assert_eq!(files_one.len(), files_two.len());
        for (a, b) in files_one.iter().zip(files_two.iter()) {
            assert_eq!(a.id, b.id);
            assert_eq!(a.path, b.path);
        }

        // The mapping is the path-sorted order, and each FileId equals its index
        // (the density invariant `project.rs` asserts and the graph relies on).
        for (idx, file) in files_one.iter().enumerate() {
            assert_eq!(file.id, FileId(idx as u32));
        }
        assert_eq!(
            files_one[0].path,
            std::path::PathBuf::from("/project/index.ts")
        );
    }

    #[test]
    fn file_id_assignment_recomputes_after_rename_or_delete() {
        let before = assign_file_ids(vec![
            (std::path::PathBuf::from("/project/src/a.ts"), 10),
            (std::path::PathBuf::from("/project/src/b.ts"), 10),
            (std::path::PathBuf::from("/project/src/c.ts"), 10),
        ]);
        let after_delete = assign_file_ids(vec![
            (std::path::PathBuf::from("/project/src/a.ts"), 10),
            (std::path::PathBuf::from("/project/src/c.ts"), 10),
        ]);
        let after_rename = assign_file_ids(vec![
            (std::path::PathBuf::from("/project/src/a.ts"), 10),
            (std::path::PathBuf::from("/project/src/c.ts"), 10),
            (std::path::PathBuf::from("/project/src/d.ts"), 10),
        ]);

        assert_eq!(before[0].id, FileId(0));
        assert_eq!(before[1].id, FileId(1));
        assert_eq!(before[2].id, FileId(2));
        assert_eq!(after_delete[0].id, FileId(0));
        assert_eq!(after_delete[1].id, FileId(1));
        assert_eq!(
            after_delete[1].path,
            std::path::PathBuf::from("/project/src/c.ts")
        );
        assert_eq!(after_rename[0].id, FileId(0));
        assert_eq!(after_rename[1].id, FileId(1));
        assert_eq!(
            after_rename[1].path,
            std::path::PathBuf::from("/project/src/c.ts")
        );
        assert_eq!(after_rename[2].id, FileId(2));
        assert_eq!(
            after_rename[2].path,
            std::path::PathBuf::from("/project/src/d.ts")
        );
    }

    #[test]
    fn allowed_hidden_dirs() {
        assert!(is_allowed_hidden_dir(OsStr::new(".storybook")));
        assert!(is_allowed_hidden_dir(OsStr::new(".vitepress")));
        assert!(is_allowed_hidden_dir(OsStr::new(".well-known")));
        assert!(is_allowed_hidden_dir(OsStr::new(".changeset")));
        assert!(is_allowed_hidden_dir(OsStr::new(".github")));
    }

    #[test]
    fn disallowed_hidden_dirs() {
        assert!(!is_allowed_hidden_dir(OsStr::new(".git")));
        assert!(!is_allowed_hidden_dir(OsStr::new(".cache")));
        assert!(!is_allowed_hidden_dir(OsStr::new(".vscode")));
        assert!(!is_allowed_hidden_dir(OsStr::new(".fallow")));
        assert!(!is_allowed_hidden_dir(OsStr::new(".next")));
    }

    #[test]
    fn non_hidden_dirs_not_in_allowlist() {
        assert!(!is_allowed_hidden_dir(OsStr::new("src")));
        assert!(!is_allowed_hidden_dir(OsStr::new("node_modules")));
    }

    #[test]
    fn walk_types_match_every_supported_source_extension() {
        for capture_config in [false, true] {
            let types = build_walk_types(capture_config);
            for extension in SOURCE_EXTENSIONS {
                let path = format!("packages/ui/src/nested/component.{extension}");
                assert!(
                    types.matched(&path, false).is_whitelist(),
                    "expected source match for {path} with capture_config={capture_config}"
                );
            }
        }
    }

    #[test]
    fn walk_types_match_typescript_declaration_files() {
        let types = build_walk_types(true);
        for path in [
            "src/env.d.ts",
            "packages/app/types/generated.d.mts",
            "packages/app/types/compat.d.cts",
        ] {
            assert!(
                types.matched(path, false).is_whitelist(),
                "expected declaration source match for {path}"
            );
        }
    }

    #[test]
    fn walk_types_reject_source_extension_near_misses() {
        for capture_config in [false, true] {
            let types = build_walk_types(capture_config);
            for path in [
                "src/component.tsx.bak",
                "src/component.tsxmap",
                "src/component.TS",
                "src/component.gqlx",
                "src/component.htm",
                "src/component",
                "assets/component.png",
            ] {
                assert!(
                    types.matched(path, false).is_ignore(),
                    "expected non-source rejection for {path} with capture_config={capture_config}"
                );
            }
        }
    }

    #[test]
    fn walk_types_keep_config_candidate_selection_separate() {
        assert!(
            build_walk_types(true)
                .matched("packages/app/tsconfig.json", false)
                .is_whitelist()
        );
        assert!(
            build_walk_types(false)
                .matched("packages/app/tsconfig.json", false)
                .is_ignore()
        );
    }

    #[test]
    fn source_extensions_include_typescript() {
        assert!(SOURCE_EXTENSIONS.contains(&"ts"));
        assert!(SOURCE_EXTENSIONS.contains(&"tsx"));
        assert!(SOURCE_EXTENSIONS.contains(&"mts"));
        assert!(SOURCE_EXTENSIONS.contains(&"cts"));
        assert!(SOURCE_EXTENSIONS.contains(&"gts"));
    }

    #[test]
    fn source_extensions_include_javascript() {
        assert!(SOURCE_EXTENSIONS.contains(&"js"));
        assert!(SOURCE_EXTENSIONS.contains(&"jsx"));
        assert!(SOURCE_EXTENSIONS.contains(&"mjs"));
        assert!(SOURCE_EXTENSIONS.contains(&"cjs"));
        assert!(SOURCE_EXTENSIONS.contains(&"gjs"));
    }

    #[test]
    fn source_extensions_include_sfc_formats() {
        assert!(SOURCE_EXTENSIONS.contains(&"vue"));
        assert!(SOURCE_EXTENSIONS.contains(&"svelte"));
        assert!(SOURCE_EXTENSIONS.contains(&"astro"));
    }

    #[test]
    fn source_extensions_include_styles() {
        assert!(SOURCE_EXTENSIONS.contains(&"css"));
        assert!(SOURCE_EXTENSIONS.contains(&"scss"));
        assert!(SOURCE_EXTENSIONS.contains(&"sass"));
        assert!(SOURCE_EXTENSIONS.contains(&"less"));
    }

    #[test]
    fn source_extensions_exclude_non_source() {
        assert!(!SOURCE_EXTENSIONS.contains(&"json"));
        assert!(!SOURCE_EXTENSIONS.contains(&"yaml"));
        assert!(!SOURCE_EXTENSIONS.contains(&"md"));
        assert!(!SOURCE_EXTENSIONS.contains(&"png"));
        assert!(!SOURCE_EXTENSIONS.contains(&"htm"));
    }

    #[test]
    fn source_extensions_include_html() {
        assert!(SOURCE_EXTENSIONS.contains(&"html"));
    }

    #[test]
    fn source_extensions_include_graphql_documents() {
        assert!(SOURCE_EXTENSIONS.contains(&"graphql"));
        assert!(SOURCE_EXTENSIONS.contains(&"gql"));
    }

    fn build_production_glob_set() -> globset::GlobSet {
        let mut builder = globset::GlobSetBuilder::new();
        for pattern in PRODUCTION_EXCLUDE_PATTERNS {
            builder.add(
                globset::GlobBuilder::new(pattern)
                    .literal_separator(true)
                    .build()
                    .expect("valid glob pattern"),
            );
        }
        builder.build().expect("valid glob set")
    }

    #[test]
    fn production_excludes_test_files() {
        let set = build_production_glob_set();
        assert!(set.is_match("src/Button.test.ts"));
        assert!(set.is_match("src/utils.spec.tsx"));
        assert!(set.is_match("src/__tests__/helper.ts"));
        assert!(!set.is_match("src/Button.ts"));
        assert!(!set.is_match("src/utils.tsx"));
    }

    #[test]
    fn production_excludes_story_files() {
        let set = build_production_glob_set();
        assert!(set.is_match("src/Button.stories.tsx"));
        assert!(set.is_match("src/Card.story.ts"));
        assert!(!set.is_match("src/Button.tsx"));
    }

    #[test]
    fn production_excludes_config_files_at_root_only() {
        let set = build_production_glob_set();
        assert!(set.is_match("vitest.config.ts"));
        assert!(set.is_match("jest.config.js"));
        assert!(!set.is_match("src/app/app.config.ts"));
        assert!(!set.is_match("src/app/app.config.server.ts"));
        assert!(!set.is_match("packages/foo/vitest.config.ts"));
        assert!(!set.is_match("src/config.ts"));
    }

    #[test]
    fn production_patterns_are_valid_globs() {
        let _ = build_production_glob_set();
    }

    #[test]
    fn disallowed_hidden_dirs_idea() {
        assert!(!is_allowed_hidden_dir(OsStr::new(".idea")));
    }

    #[test]
    fn source_extensions_include_mdx() {
        assert!(SOURCE_EXTENSIONS.contains(&"mdx"));
    }

    #[test]
    fn source_extensions_exclude_image_and_data_formats() {
        assert!(!SOURCE_EXTENSIONS.contains(&"png"));
        assert!(!SOURCE_EXTENSIONS.contains(&"jpg"));
        assert!(!SOURCE_EXTENSIONS.contains(&"svg"));
        assert!(!SOURCE_EXTENSIONS.contains(&"txt"));
        assert!(!SOURCE_EXTENSIONS.contains(&"csv"));
        assert!(!SOURCE_EXTENSIONS.contains(&"wasm"));
    }

    #[test]
    fn is_declaration_file_matches_dts_variants() {
        assert!(is_declaration_file(Path::new("env.d.ts")));
        assert!(is_declaration_file(Path::new("src/auto-imports.d.ts")));
        assert!(is_declaration_file(Path::new("mod.d.mts")));
        assert!(is_declaration_file(Path::new("compat.d.cts")));
        assert!(!is_declaration_file(Path::new("index.ts")));
        assert!(!is_declaration_file(Path::new("component.tsx")));
        assert!(!is_declaration_file(Path::new("notes.d.txt")));
    }

    #[test]
    fn format_size_mb_renders_one_decimal() {
        assert_eq!(format_size_mb(5 * 1024 * 1024), "5.0 MB");
        assert_eq!(format_size_mb(1024 * 1024 + 512 * 1024), "1.5 MB");
        assert_eq!(format_size_mb(0), "0.0 MB");
    }

    #[test]
    fn partition_by_size_no_limit_keeps_all() {
        let raw = vec![(PathBuf::from("a.ts"), 10), (PathBuf::from("b.ts"), 10_000)];
        let (kept, skipped) = partition_by_size(raw, None);
        assert_eq!(kept.len(), 2);
        assert!(skipped.is_empty());
    }

    #[test]
    fn partition_by_size_skips_strictly_over_limit() {
        let raw = vec![
            (PathBuf::from("under.ts"), 99),
            (PathBuf::from("exact.ts"), 100),
            (PathBuf::from("over.ts"), 101),
        ];
        let (kept, skipped) = partition_by_size(raw, Some(100));
        let kept_has = |name: &str| kept.iter().any(|(p, _)| p.as_path() == Path::new(name));
        assert!(kept_has("under.ts"));
        assert!(
            kept_has("exact.ts"),
            "a file exactly at the limit is kept (skip is strictly-greater)"
        );
        assert_eq!(skipped.len(), 1);
        assert_eq!(skipped[0].0, PathBuf::from("over.ts"));
    }

    #[test]
    fn partition_by_size_exempts_declaration_files() {
        let raw = vec![
            (PathBuf::from("huge.ts"), 10_000),
            (PathBuf::from("auto-imports.d.ts"), 10_000),
        ];
        let (kept, skipped) = partition_by_size(raw, Some(100));
        assert!(
            kept.iter()
                .any(|(p, _)| p.as_path() == Path::new("auto-imports.d.ts")),
            "declaration files are exempt from the size skip regardless of size"
        );
        assert_eq!(skipped.len(), 1);
        assert_eq!(skipped[0].0, PathBuf::from("huge.ts"));
    }

    fn disco(path: &str, size_bytes: u64) -> DiscoveredFile {
        DiscoveredFile {
            id: FileId(0),
            path: PathBuf::from(path),
            size_bytes,
        }
    }

    #[test]
    fn largest_files_note_below_threshold_is_none() {
        let files = [disco("a.ts", 100), disco("b.ts", 200)];
        assert!(build_largest_files_note(Path::new("/p"), &files).is_none());
    }

    #[test]
    fn largest_files_note_single_file_uses_singular() {
        let files = [disco("big.ts", 5 * 1024 * 1024)];
        let note = build_largest_files_note(Path::new("/p"), &files).expect("note fires");
        assert!(
            note.contains("discovered 1 file;"),
            "singular noun on the single-big-file path (issue #1086 regression): {note}"
        );
        assert!(!note.contains("discovered 1 files"));
        assert!(note.contains("big.ts (5.0 MB)"));
    }

    #[test]
    fn largest_files_note_filters_sub_floor_files() {
        let files = [disco("big.ts", 5 * 1024 * 1024), disco("tiny.ts", 10)];
        let note = build_largest_files_note(Path::new("/p"), &files).expect("note fires");
        assert!(note.contains("discovered 2 files;"));
        assert!(note.contains("big.ts (5.0 MB)"));
        assert!(
            !note.contains("tiny.ts"),
            "sub-floor files are not listed as `0.0 MB` chaff: {note}"
        );
    }

    #[test]
    fn largest_files_note_large_set_no_big_file_omits_list() {
        let files: Vec<DiscoveredFile> = (0..=LARGE_SET_THRESHOLD)
            .map(|i| disco(&format!("f{i}.ts"), 100))
            .collect();
        let note = build_largest_files_note(Path::new("/p"), &files).expect("large set fires");
        assert!(note.contains(&format!("discovered {} files", LARGE_SET_THRESHOLD + 1)));
        assert!(
            !note.contains("largest:"),
            "no sub-floor `largest:` list when no file clears the floor: {note}"
        );
    }

    mod discover_files_integration {
        use std::path::PathBuf;

        use fallow_config::{
            DuplicatesConfig, FallowConfig, FlagsConfig, HealthConfig, OutputFormat, ResolveConfig,
            RulesConfig,
        };

        use super::*;

        /// Create a minimal ResolvedConfig pointing at the given root directory.
        fn make_config(root: PathBuf, production: bool) -> ResolvedConfig {
            FallowConfig {
                production: production.into(),
                ..Default::default()
            }
            .resolve(root, OutputFormat::Human, 1, true, true, None)
        }

        /// Helper to collect discovered file names (relative to root) for assertions.
        /// Normalizes path separators to `/` for cross-platform test consistency.
        fn file_names(files: &[DiscoveredFile], root: &std::path::Path) -> Vec<String> {
            files
                .iter()
                .map(|f| {
                    f.path
                        .strip_prefix(root)
                        .unwrap_or(&f.path)
                        .to_string_lossy()
                        .replace('\\', "/")
                })
                .collect()
        }

        #[cfg(unix)]
        fn symlink_file(target: &Path, link: &Path) {
            std::os::unix::fs::symlink(target, link).expect("create file symlink");
        }

        #[cfg(windows)]
        fn symlink_file(target: &Path, link: &Path) {
            std::os::windows::fs::symlink_file(target, link).expect("create file symlink");
        }

        #[cfg(unix)]
        fn symlink_dir(target: &Path, link: &Path) {
            std::os::unix::fs::symlink(target, link).expect("create directory symlink");
        }

        #[cfg(windows)]
        fn symlink_dir(target: &Path, link: &Path) {
            std::os::windows::fs::symlink_dir(target, link).expect("create directory symlink");
        }

        #[test]
        fn source_symlinks_must_target_regular_files_inside_root() {
            let dir = tempfile::tempdir().expect("create project");
            let outside = tempfile::tempdir().expect("create outside dir");
            let src = dir.path().join("src");
            std::fs::create_dir_all(&src).unwrap();
            std::fs::write(src.join("regular.ts"), "export const regular = 1;").unwrap();
            std::fs::write(src.join("inside-target.ts"), "export const inside = 1;").unwrap();
            std::fs::write(
                outside.path().join("outside-target.ts"),
                "export const outside = 1;",
            )
            .unwrap();
            std::fs::create_dir_all(src.join("directory-target")).unwrap();

            symlink_file(&src.join("inside-target.ts"), &src.join("inside-link.ts"));
            symlink_file(
                &outside.path().join("outside-target.ts"),
                &src.join("outside-link.ts"),
            );
            symlink_file(&src.join("missing-target.ts"), &src.join("broken-link.ts"));
            symlink_dir(
                &src.join("directory-target"),
                &src.join("directory-link.ts"),
            );

            let config = make_config(dir.path().to_path_buf(), false);
            let names = file_names(&discover_files(&config), dir.path());

            assert!(names.contains(&"src/regular.ts".to_string()));
            assert!(names.contains(&"src/inside-target.ts".to_string()));
            assert!(names.contains(&"src/inside-link.ts".to_string()));
            assert!(!names.contains(&"src/outside-link.ts".to_string()));
            assert!(!names.contains(&"src/broken-link.ts".to_string()));
            assert!(!names.contains(&"src/directory-link.ts".to_string()));
        }

        /// Yarn PnP writes `.pnp.cjs` and `.pnp.loader.mjs` at the workspace
        /// root. They match the source extension filter but are generated
        /// install state, not code to analyze.
        #[test]
        fn skips_yarn_pnp_generated_files() {
            let dir = tempfile::tempdir().expect("create temp dir");
            std::fs::write(dir.path().join(".pnp.cjs"), "module.exports = {};").unwrap();
            std::fs::write(dir.path().join(".pnp.loader.mjs"), "export {};").unwrap();
            std::fs::write(dir.path().join("index.ts"), "export const a = 1;").unwrap();

            let config = make_config(dir.path().to_path_buf(), false);
            let names = file_names(&discover_files(&config), dir.path());

            assert_eq!(names, vec!["index.ts".to_string()]);
        }

        #[test]
        fn discovers_source_files_with_valid_extensions() {
            let dir = tempfile::tempdir().expect("create temp dir");
            let src = dir.path().join("src");
            std::fs::create_dir_all(&src).unwrap();

            std::fs::write(src.join("app.ts"), "export const a = 1;").unwrap();
            std::fs::write(src.join("component.tsx"), "export default () => {};").unwrap();
            std::fs::write(src.join("utils.js"), "module.exports = {};").unwrap();
            std::fs::write(src.join("helper.jsx"), "export const h = 1;").unwrap();
            std::fs::write(src.join("config.mjs"), "export default {};").unwrap();
            std::fs::write(src.join("legacy.cjs"), "module.exports = {};").unwrap();
            std::fs::write(src.join("types.mts"), "export type T = string;").unwrap();
            std::fs::write(src.join("compat.cts"), "module.exports = {};").unwrap();

            let config = make_config(dir.path().to_path_buf(), false);
            let files = discover_files(&config);
            let names = file_names(&files, dir.path());

            assert!(names.contains(&"src/app.ts".to_string()));
            assert!(names.contains(&"src/component.tsx".to_string()));
            assert!(names.contains(&"src/utils.js".to_string()));
            assert!(names.contains(&"src/helper.jsx".to_string()));
            assert!(names.contains(&"src/config.mjs".to_string()));
            assert!(names.contains(&"src/legacy.cjs".to_string()));
            assert!(names.contains(&"src/types.mts".to_string()));
            assert!(names.contains(&"src/compat.cts".to_string()));
        }

        #[test]
        fn compact_source_glob_preserves_discovered_file_inventory() {
            let dir = tempfile::tempdir().expect("create temp dir");
            let nested = dir.path().join("packages/ui/src/nested");
            std::fs::create_dir_all(&nested).unwrap();

            let mut expected = Vec::new();
            for (index, extension) in SOURCE_EXTENSIONS.iter().enumerate() {
                let relative = format!("packages/ui/src/nested/source-{index}.{extension}");
                std::fs::write(dir.path().join(&relative), "export const value = 1;").unwrap();
                expected.push(relative);
            }
            for relative in [
                "packages/ui/src/nested/env.d.ts",
                "packages/ui/src/nested/generated.d.mts",
                "packages/ui/src/nested/compat.d.cts",
            ] {
                std::fs::write(dir.path().join(relative), "export type Value = string;").unwrap();
                expected.push(relative.to_string());
            }
            let rejected = [
                "packages/ui/src/nested/component.tsx.bak",
                "packages/ui/src/nested/component.tsxmap",
                "packages/ui/src/nested/component.TS",
                "packages/ui/src/nested/component.gqlx",
                "packages/ui/src/nested/component.htm",
                "packages/ui/src/nested/component",
                "packages/ui/src/nested/component.png",
            ];
            for relative in rejected {
                std::fs::write(dir.path().join(relative), "not source").unwrap();
            }

            let config = make_config(dir.path().to_path_buf(), false);
            let names = file_names(&discover_files(&config), dir.path());

            for relative in expected {
                assert!(
                    names.contains(&relative),
                    "missing supported source {relative}"
                );
            }
            for relative in rejected {
                assert!(
                    !names.iter().any(|name| name == relative),
                    "unexpected near-miss source {relative}"
                );
            }
        }

        #[test]
        fn excludes_non_source_extensions() {
            let dir = tempfile::tempdir().expect("create temp dir");
            let src = dir.path().join("src");
            std::fs::create_dir_all(&src).unwrap();

            std::fs::write(src.join("app.ts"), "export const a = 1;").unwrap();

            std::fs::write(src.join("data.json"), "{}").unwrap();
            std::fs::write(src.join("readme.md"), "# Hello").unwrap();
            std::fs::write(src.join("notes.txt"), "notes").unwrap();
            std::fs::write(src.join("logo.png"), [0u8; 8]).unwrap();

            let config = make_config(dir.path().to_path_buf(), false);
            let files = discover_files(&config);
            let names = file_names(&files, dir.path());

            assert_eq!(names.len(), 1, "only the .ts file should be discovered");
            assert!(names.contains(&"src/app.ts".to_string()));
        }

        #[test]
        fn excludes_disallowed_hidden_directories() {
            let dir = tempfile::tempdir().expect("create temp dir");

            let git_dir = dir.path().join(".git");
            std::fs::create_dir_all(&git_dir).unwrap();
            std::fs::write(git_dir.join("hooks.ts"), "// git hook").unwrap();

            let idea_dir = dir.path().join(".idea");
            std::fs::create_dir_all(&idea_dir).unwrap();
            std::fs::write(idea_dir.join("workspace.ts"), "// idea").unwrap();

            let cache_dir = dir.path().join(".cache");
            std::fs::create_dir_all(&cache_dir).unwrap();
            std::fs::write(cache_dir.join("cached.js"), "// cached").unwrap();

            let src = dir.path().join("src");
            std::fs::create_dir_all(&src).unwrap();
            std::fs::write(src.join("app.ts"), "export const a = 1;").unwrap();

            let config = make_config(dir.path().to_path_buf(), false);
            let files = discover_files(&config);
            let names = file_names(&files, dir.path());

            assert_eq!(names.len(), 1, "only src/app.ts should be discovered");
            assert!(names.contains(&"src/app.ts".to_string()));
        }

        #[test]
        fn includes_allowed_hidden_directories() {
            let dir = tempfile::tempdir().expect("create temp dir");

            let storybook = dir.path().join(".storybook");
            std::fs::create_dir_all(&storybook).unwrap();
            std::fs::write(storybook.join("main.ts"), "export default {};").unwrap();

            let github = dir.path().join(".github");
            std::fs::create_dir_all(&github).unwrap();
            std::fs::write(github.join("actions.js"), "module.exports = {};").unwrap();

            let changeset = dir.path().join(".changeset");
            std::fs::create_dir_all(&changeset).unwrap();
            std::fs::write(changeset.join("config.js"), "module.exports = {};").unwrap();

            let config = make_config(dir.path().to_path_buf(), false);
            let files = discover_files(&config);
            let names = file_names(&files, dir.path());

            assert!(
                names.contains(&".storybook/main.ts".to_string()),
                "files in .storybook should be discovered"
            );
            assert!(
                names.contains(&".github/actions.js".to_string()),
                "files in .github should be discovered"
            );
            assert!(
                names.contains(&".changeset/config.js".to_string()),
                "files in .changeset should be discovered"
            );
        }

        #[test]
        fn default_discovery_excludes_client_and_server_hidden_directories() {
            let dir = tempfile::tempdir().expect("create temp dir");
            let app = dir.path().join("app");
            std::fs::create_dir_all(app.join(".client")).unwrap();
            std::fs::create_dir_all(app.join(".server")).unwrap();
            std::fs::write(app.join(".client/analytics.ts"), "export const a = 1;").unwrap();
            std::fs::write(app.join(".server/db.ts"), "export const db = {};").unwrap();
            std::fs::write(app.join("root.tsx"), "export default function Root() {}").unwrap();

            let config = make_config(dir.path().to_path_buf(), false);
            let files = discover_files(&config);
            let names = file_names(&files, dir.path());

            assert!(names.contains(&"app/root.tsx".to_string()));
            assert!(!names.contains(&"app/.client/analytics.ts".to_string()));
            assert!(!names.contains(&"app/.server/db.ts".to_string()));
        }

        #[test]
        fn scoped_hidden_dirs_include_client_and_server_under_package_root() {
            let dir = tempfile::tempdir().expect("create temp dir");
            let package = dir.path().join("packages/app");
            std::fs::create_dir_all(package.join("app/.client")).unwrap();
            std::fs::create_dir_all(package.join("app/.server")).unwrap();
            std::fs::write(
                package.join("app/.client/analytics.ts"),
                "export const track = () => {};",
            )
            .unwrap();
            std::fs::write(package.join("app/.server/db.ts"), "export const db = {};").unwrap();

            let config = make_config(dir.path().to_path_buf(), false);
            let scopes = [HiddenDirScope::new(
                package,
                vec![".client".to_string(), ".server".to_string()],
            )];
            let files = discover_files_with_additional_hidden_dirs(&config, &scopes);
            let names = file_names(&files, dir.path());

            assert!(names.contains(&"packages/app/app/.client/analytics.ts".to_string()));
            assert!(names.contains(&"packages/app/app/.server/db.ts".to_string()));
        }

        #[test]
        fn scoped_hidden_dirs_do_not_include_unscoped_packages() {
            let dir = tempfile::tempdir().expect("create temp dir");
            let active = dir.path().join("packages/active");
            let inactive = dir.path().join("packages/inactive");
            std::fs::create_dir_all(active.join("app/.server")).unwrap();
            std::fs::create_dir_all(inactive.join("app/.server")).unwrap();
            std::fs::write(active.join("app/.server/db.ts"), "export const db = {};").unwrap();
            std::fs::write(inactive.join("app/.server/db.ts"), "export const db = {};").unwrap();

            let config = make_config(dir.path().to_path_buf(), false);
            let scopes = [HiddenDirScope::new(active, vec![".server".to_string()])];
            let files = discover_files_with_additional_hidden_dirs(&config, &scopes);
            let names = file_names(&files, dir.path());

            assert!(names.contains(&"packages/active/app/.server/db.ts".to_string()));
            assert!(!names.contains(&"packages/inactive/app/.server/db.ts".to_string()));
        }

        #[test]
        fn exact_path_scope_does_not_admit_the_same_name_elsewhere() {
            // A script naming `.a/.b/deep.mjs` says where the file it needs
            // lives. Before issue #461 the scope stored the bare names, so an
            // unrelated `elsewhere/.b` and `unrelated/.a` were pulled in too.
            let dir = tempfile::tempdir().expect("create temp dir");
            std::fs::create_dir_all(dir.path().join(".a/.b")).unwrap();
            std::fs::create_dir_all(dir.path().join("elsewhere/.b")).unwrap();
            std::fs::create_dir_all(dir.path().join("unrelated/.a")).unwrap();
            std::fs::write(dir.path().join(".a/.b/deep.mjs"), "export const a = 1;").unwrap();
            std::fs::write(dir.path().join("elsewhere/.b/y.mjs"), "export const b = 1;").unwrap();
            std::fs::write(dir.path().join("unrelated/.a/u.mjs"), "export const c = 1;").unwrap();

            let config = make_config(dir.path().to_path_buf(), false);
            let scopes = [HiddenDirScope::new_exact_paths(
                dir.path().to_path_buf(),
                vec![".a".to_string(), format!(".a{MAIN_SEPARATOR}.b")],
            )];
            let files = discover_files_with_additional_hidden_dirs(&config, &scopes);
            let names = file_names(&files, dir.path());

            assert!(names.contains(&".a/.b/deep.mjs".to_string()));
            assert!(!names.contains(&"elsewhere/.b/y.mjs".to_string()));
            assert!(!names.contains(&"unrelated/.a/u.mjs".to_string()));
        }

        #[test]
        fn exact_path_scope_admits_a_hidden_dir_under_a_visible_parent() {
            let dir = tempfile::tempdir().expect("create temp dir");
            std::fs::create_dir_all(dir.path().join("tools/.config")).unwrap();
            std::fs::create_dir_all(dir.path().join("other/.config")).unwrap();
            std::fs::write(
                dir.path().join("tools/.config/eslint.config.js"),
                "export default [];",
            )
            .unwrap();
            std::fs::write(
                dir.path().join("other/.config/eslint.config.js"),
                "export default [];",
            )
            .unwrap();

            let config = make_config(dir.path().to_path_buf(), false);
            let scopes = [HiddenDirScope::new_exact_paths(
                dir.path().to_path_buf(),
                vec![format!("tools{MAIN_SEPARATOR}.config")],
            )];
            let files = discover_files_with_additional_hidden_dirs(&config, &scopes);
            let names = file_names(&files, dir.path());

            assert!(names.contains(&"tools/.config/eslint.config.js".to_string()));
            assert!(!names.contains(&"other/.config/eslint.config.js".to_string()));
        }

        #[test]
        fn any_depth_scope_keeps_matching_by_name_for_plugins() {
            // Framework plugins declare `.client` / `.server` conventions that
            // may sit under any route directory, so the plugin shape must keep
            // matching at any depth.
            let dir = tempfile::tempdir().expect("create temp dir");
            std::fs::create_dir_all(dir.path().join("app/routes/deep/.server")).unwrap();
            std::fs::write(
                dir.path().join("app/routes/deep/.server/db.ts"),
                "export const db = {};",
            )
            .unwrap();

            let config = make_config(dir.path().to_path_buf(), false);
            let scopes = [HiddenDirScope::new(
                dir.path().to_path_buf(),
                vec![".server".to_string()],
            )];
            let files = discover_files_with_additional_hidden_dirs(&config, &scopes);
            let names = file_names(&files, dir.path());

            assert!(names.contains(&"app/routes/deep/.server/db.ts".to_string()));
        }

        #[test]
        fn excludes_root_build_directory() {
            let dir = tempfile::tempdir().expect("create temp dir");

            std::fs::write(dir.path().join(".ignore"), "/build/\n").unwrap();

            let build_dir = dir.path().join("build");
            std::fs::create_dir_all(&build_dir).unwrap();
            std::fs::write(build_dir.join("output.js"), "// build output").unwrap();

            let src = dir.path().join("src");
            std::fs::create_dir_all(&src).unwrap();
            std::fs::write(src.join("app.ts"), "export const a = 1;").unwrap();

            let config = make_config(dir.path().to_path_buf(), false);
            let files = discover_files(&config);
            let names = file_names(&files, dir.path());

            assert_eq!(names.len(), 1, "root build/ should be excluded via .ignore");
            assert!(names.contains(&"src/app.ts".to_string()));
        }

        #[test]
        fn includes_nested_build_directory() {
            let dir = tempfile::tempdir().expect("create temp dir");

            let nested_build = dir.path().join("src").join("build");
            std::fs::create_dir_all(&nested_build).unwrap();
            std::fs::write(nested_build.join("helper.ts"), "export const h = 1;").unwrap();

            let config = make_config(dir.path().to_path_buf(), false);
            let files = discover_files(&config);
            let names = file_names(&files, dir.path());

            assert!(
                names.contains(&"src/build/helper.ts".to_string()),
                "nested build/ directories should be included"
            );
        }

        #[test]
        #[expect(
            clippy::cast_possible_truncation,
            reason = "test file counts are trivially small"
        )]
        fn file_ids_are_sequential_after_sorting() {
            let dir = tempfile::tempdir().expect("create temp dir");
            let src = dir.path().join("src");
            std::fs::create_dir_all(&src).unwrap();

            std::fs::write(src.join("z_last.ts"), "export const z = 1;").unwrap();
            std::fs::write(src.join("a_first.ts"), "export const a = 1;").unwrap();
            std::fs::write(src.join("m_middle.ts"), "export const m = 1;").unwrap();

            let config = make_config(dir.path().to_path_buf(), false);
            let files = discover_files(&config);

            for (idx, file) in files.iter().enumerate() {
                assert_eq!(file.id, FileId(idx as u32), "FileId should be sequential");
            }

            for pair in files.windows(2) {
                assert!(
                    pair[0].path < pair[1].path,
                    "files should be sorted by path"
                );
            }
        }

        #[test]
        fn production_mode_excludes_test_files() {
            let dir = tempfile::tempdir().expect("create temp dir");
            let src = dir.path().join("src");
            std::fs::create_dir_all(&src).unwrap();

            std::fs::write(src.join("app.ts"), "export const a = 1;").unwrap();
            std::fs::write(src.join("app.test.ts"), "test('a', () => {});").unwrap();
            std::fs::write(src.join("app.spec.ts"), "describe('a', () => {});").unwrap();
            std::fs::write(src.join("app.stories.tsx"), "export default {};").unwrap();

            let config = make_config(dir.path().to_path_buf(), true);
            let files = discover_files(&config);
            let names = file_names(&files, dir.path());

            assert!(
                names.contains(&"src/app.ts".to_string()),
                "source files should be included in production mode"
            );
            assert!(
                !names.contains(&"src/app.test.ts".to_string()),
                "test files should be excluded in production mode"
            );
            assert!(
                !names.contains(&"src/app.spec.ts".to_string()),
                "spec files should be excluded in production mode"
            );
            assert!(
                !names.contains(&"src/app.stories.tsx".to_string()),
                "story files should be excluded in production mode"
            );
        }

        #[test]
        fn non_production_mode_includes_test_files() {
            let dir = tempfile::tempdir().expect("create temp dir");
            let src = dir.path().join("src");
            std::fs::create_dir_all(&src).unwrap();

            std::fs::write(src.join("app.ts"), "export const a = 1;").unwrap();
            std::fs::write(src.join("app.test.ts"), "test('a', () => {});").unwrap();

            let config = make_config(dir.path().to_path_buf(), false);
            let files = discover_files(&config);
            let names = file_names(&files, dir.path());

            assert!(names.contains(&"src/app.ts".to_string()));
            assert!(
                names.contains(&"src/app.test.ts".to_string()),
                "test files should be included in non-production mode"
            );
        }

        #[test]
        fn empty_directory_returns_no_files() {
            let dir = tempfile::tempdir().expect("create temp dir");
            let config = make_config(dir.path().to_path_buf(), false);
            let files = discover_files(&config);
            assert!(files.is_empty(), "empty project should discover no files");
        }

        #[test]
        fn hidden_files_not_discovered_as_source() {
            let dir = tempfile::tempdir().expect("create temp dir");

            std::fs::write(dir.path().join(".env"), "SECRET=abc").unwrap();
            std::fs::write(dir.path().join(".gitignore"), "node_modules").unwrap();
            std::fs::write(dir.path().join(".eslintrc.js"), "module.exports = {};").unwrap();

            let src = dir.path().join("src");
            std::fs::create_dir_all(&src).unwrap();
            std::fs::write(src.join("app.ts"), "export const a = 1;").unwrap();

            let config = make_config(dir.path().to_path_buf(), false);
            let files = discover_files(&config);
            let names = file_names(&files, dir.path());

            assert!(
                !names.contains(&".env".to_string()),
                ".env should not be discovered"
            );
            assert!(
                !names.contains(&".gitignore".to_string()),
                ".gitignore should not be discovered"
            );
        }

        /// Create a config with custom ignore patterns.
        fn make_config_with_ignores(root: PathBuf, ignores: Vec<String>) -> ResolvedConfig {
            FallowConfig {
                type_aware: fallow_config::TypeAwareConfig::default(),
                schema: None,
                extends: vec![],
                entry: vec![],
                ignore_patterns: ignores,
                ignore_findings: vec![],
                framework: vec![],
                workspaces: None,
                ignore_dependencies: vec![],
                ignore_unresolved_imports: vec![],
                ignore_exports: vec![],
                ignore_catalog_references: vec![],
                ignore_dependency_overrides: vec![],
                ignore_exports_used_in_file: fallow_config::IgnoreExportsUsedInFileConfig::default(
                ),
                used_class_members: vec![],
                ignore_decorators: vec![],
                unused_component_props: fallow_config::UnusedComponentPropsConfig::default(),
                duplicates: DuplicatesConfig::default(),
                similar_code: fallow_config::SimilarCodeConfig::default(),
                health: HealthConfig::default(),
                rules: RulesConfig::default(),
                boundaries: fallow_config::BoundaryConfig::default(),
                production: false.into(),
                plugins: vec![],
                rule_packs: vec![],
                dynamically_loaded: vec![],
                overrides: vec![],
                regression: None,
                audit: fallow_config::AuditConfig::default(),
                codeowners: None,
                public_packages: vec![],
                flags: FlagsConfig::default(),
                security: fallow_config::SecurityConfig::default(),
                fix: fallow_config::FixConfig::default(),
                resolve: ResolveConfig::default(),
                sealed: false,
                include_entry_exports: false,
                auto_imports: false,
                cache: fallow_config::CacheConfig::default(),
            }
            .resolve(root, OutputFormat::Human, 1, true, true, None)
        }

        #[test]
        fn custom_ignore_patterns_exclude_matching_files() {
            let dir = tempfile::tempdir().expect("create temp dir");

            let generated = dir.path().join("src").join("api").join("generated");
            std::fs::create_dir_all(&generated).unwrap();
            std::fs::write(generated.join("client.ts"), "export const api = {};").unwrap();

            let client = dir.path().join("src").join("api").join("client");
            std::fs::create_dir_all(&client).unwrap();
            std::fs::write(client.join("fetch.ts"), "export const fetch = {};").unwrap();

            let src = dir.path().join("src");
            std::fs::write(src.join("index.ts"), "export const x = 1;").unwrap();

            let config = make_config_with_ignores(
                dir.path().to_path_buf(),
                vec![
                    "src/api/generated/**".to_string(),
                    "src/api/client/**".to_string(),
                ],
            );
            let files = discover_files(&config);
            let names = file_names(&files, dir.path());

            assert_eq!(names.len(), 1, "only non-ignored files: {names:?}");
            assert!(names.contains(&"src/index.ts".to_string()));
        }

        #[test]
        fn leading_dot_ignore_patterns_exclude_matching_files() {
            let dir = tempfile::tempdir().expect("create temp dir");

            let generated = dir.path().join("src").join("generated");
            std::fs::create_dir_all(&generated).unwrap();
            std::fs::write(generated.join("client.ts"), "export const api = {};").unwrap();

            let src = dir.path().join("src");
            std::fs::write(src.join("index.ts"), "export const x = 1;").unwrap();

            let config = make_config_with_ignores(
                dir.path().to_path_buf(),
                vec!["./src/generated/**".to_string()],
            );
            let files = discover_files(&config);
            let names = file_names(&files, dir.path());

            assert_eq!(names, vec!["src/index.ts"]);
        }

        #[test]
        fn default_ignore_patterns_exclude_node_modules_and_dist() {
            let dir = tempfile::tempdir().expect("create temp dir");

            let nm = dir.path().join("node_modules").join("lodash");
            std::fs::create_dir_all(&nm).unwrap();
            std::fs::write(nm.join("lodash.js"), "module.exports = {};").unwrap();

            let dist = dir.path().join("dist");
            std::fs::create_dir_all(&dist).unwrap();
            std::fs::write(dist.join("bundle.js"), "// bundled").unwrap();

            let src = dir.path().join("src");
            std::fs::create_dir_all(&src).unwrap();
            std::fs::write(src.join("index.ts"), "export const x = 1;").unwrap();

            let config = make_config(dir.path().to_path_buf(), false);
            let files = discover_files(&config);
            let names = file_names(&files, dir.path());

            assert_eq!(names.len(), 1);
            assert!(names.contains(&"src/index.ts".to_string()));
        }

        #[test]
        fn default_ignore_patterns_exclude_root_build() {
            let dir = tempfile::tempdir().expect("create temp dir");

            let build = dir.path().join("build");
            std::fs::create_dir_all(&build).unwrap();
            std::fs::write(build.join("output.js"), "// built").unwrap();

            let nested_build = dir.path().join("src").join("build");
            std::fs::create_dir_all(&nested_build).unwrap();
            std::fs::write(nested_build.join("helper.ts"), "export const h = 1;").unwrap();

            let src = dir.path().join("src");
            std::fs::write(src.join("index.ts"), "export const x = 1;").unwrap();

            let config = make_config(dir.path().to_path_buf(), false);
            let files = discover_files(&config);
            let names = file_names(&files, dir.path());

            assert_eq!(
                names.len(),
                2,
                "root build/ excluded, nested kept: {names:?}"
            );
            assert!(names.contains(&"src/index.ts".to_string()));
            assert!(names.contains(&"src/build/helper.ts".to_string()));
        }

        /// Resolve a config then override the per-file size limit in bytes.
        fn make_config_with_max_file_size(
            root: PathBuf,
            max_file_size_bytes: Option<u64>,
        ) -> ResolvedConfig {
            let mut config = make_config(root, false);
            config.max_file_size_bytes = max_file_size_bytes;
            config
        }

        #[test]
        fn skips_files_over_max_file_size() {
            let dir = tempfile::tempdir().expect("create temp dir");
            let src = dir.path().join("src");
            std::fs::create_dir_all(&src).unwrap();
            std::fs::write(src.join("small.ts"), "export const a = 1;").unwrap();
            std::fs::write(src.join("huge.ts"), "x".repeat(5_000)).unwrap();

            let config = make_config_with_max_file_size(dir.path().to_path_buf(), Some(1_000));
            let files = discover_files(&config);
            let names = file_names(&files, dir.path());

            assert!(names.contains(&"src/small.ts".to_string()));
            assert!(
                !names.contains(&"src/huge.ts".to_string()),
                "a file over the size limit must not be discovered"
            );
        }

        #[test]
        fn declaration_files_exempt_from_size_skip() {
            let dir = tempfile::tempdir().expect("create temp dir");
            let src = dir.path().join("src");
            std::fs::create_dir_all(&src).unwrap();
            std::fs::write(src.join("auto-imports.d.ts"), "x".repeat(5_000)).unwrap();
            std::fs::write(src.join("huge.ts"), "x".repeat(5_000)).unwrap();

            let config = make_config_with_max_file_size(dir.path().to_path_buf(), Some(1_000));
            let files = discover_files(&config);
            let names = file_names(&files, dir.path());

            assert!(
                names.contains(&"src/auto-imports.d.ts".to_string()),
                "a large .d.ts is exempt from the skip (reachability root for global types)"
            );
            assert!(!names.contains(&"src/huge.ts".to_string()));
        }

        #[test]
        fn unlimited_size_keeps_large_files() {
            let dir = tempfile::tempdir().expect("create temp dir");
            let src = dir.path().join("src");
            std::fs::create_dir_all(&src).unwrap();
            std::fs::write(src.join("huge.ts"), "x".repeat(5_000)).unwrap();

            let config = make_config_with_max_file_size(dir.path().to_path_buf(), None);
            let files = discover_files(&config);
            let names = file_names(&files, dir.path());

            assert!(
                names.contains(&"src/huge.ts".to_string()),
                "no limit keeps every file"
            );
        }

        #[test]
        fn skipped_file_recorded_in_workspace_diagnostics() {
            let dir = tempfile::tempdir().expect("create temp dir");
            let src = dir.path().join("src");
            std::fs::create_dir_all(&src).unwrap();
            std::fs::write(src.join("huge.ts"), "x".repeat(5_000)).unwrap();

            let config = make_config_with_max_file_size(dir.path().to_path_buf(), Some(1_000));
            let _ = discover_files(&config);

            let diagnostics = fallow_config::workspace_diagnostics_for(dir.path());
            let skipped: Vec<_> = diagnostics
                .iter()
                .filter(|d| {
                    matches!(
                        d.kind,
                        fallow_config::WorkspaceDiagnosticKind::SkippedLargeFile { .. }
                    )
                })
                .collect();
            assert_eq!(
                skipped.len(),
                1,
                "the skipped file is recorded in workspace diagnostics for JSON output"
            );
            assert!(skipped[0].path.ends_with("src/huge.ts"));
            assert!(
                matches!(
                    skipped[0].kind,
                    fallow_config::WorkspaceDiagnosticKind::SkippedLargeFile { size_bytes }
                        if size_bytes == 5_000
                ),
                "the recorded diagnostic carries the on-disk byte size"
            );
        }

        /// The skipped-source-dotdir entries the last walk on `root` recorded.
        fn dotdir_diagnostics(root: &Path) -> Vec<fallow_config::WorkspaceDiagnostic> {
            fallow_config::workspace_diagnostics_for(root)
                .into_iter()
                .filter(|d| {
                    matches!(
                        d.kind,
                        fallow_config::WorkspaceDiagnosticKind::SkippedSourceDotdir
                    )
                })
                .collect()
        }

        fn write_at(root: &Path, relative: &str, contents: &str) {
            let path = root.join(relative);
            std::fs::create_dir_all(path.parent().expect("has a parent")).unwrap();
            std::fs::write(path, contents).unwrap();
        }

        #[test]
        fn skipped_source_dotdir_recorded_in_workspace_diagnostics() {
            let dir = tempfile::tempdir().expect("create temp dir");
            write_at(
                dir.path(),
                ".claude/hooks/probe.mjs",
                "export const a = 1;\n",
            );
            write_at(dir.path(), "src/app.ts", "export const b = 2;\n");

            let config = make_config(dir.path().to_path_buf(), false);
            let files = discover_files(&config);
            let names = file_names(&files, dir.path());

            let reported = dotdir_diagnostics(dir.path());
            assert_eq!(reported.len(), 1, "one skipped dotdir holds source files");
            assert!(reported[0].path.ends_with(".claude"));
            assert_eq!(reported[0].kind.id(), "skipped-source-dotdir");
            assert!(
                reported[0].message.contains("--root"),
                "message names the real remedy: {}",
                reported[0].message
            );
            assert!(
                names.contains(&"src/app.ts".to_string()),
                "traversal is unchanged for ordinary directories"
            );
            assert!(
                !names.contains(&".claude/hooks/probe.mjs".to_string()),
                "the diagnostic reports the skip, it does not change traversal"
            );
        }

        #[test]
        fn allowlisted_dotdir_is_not_reported() {
            let dir = tempfile::tempdir().expect("create temp dir");
            write_at(dir.path(), ".storybook/main.ts", "export const a = 1;\n");

            let config = make_config(dir.path().to_path_buf(), false);
            let files = discover_files(&config);
            let names = file_names(&files, dir.path());

            assert!(dotdir_diagnostics(dir.path()).is_empty());
            assert!(
                names.contains(&".storybook/main.ts".to_string()),
                "an allowlisted dotdir is still traversed"
            );
        }

        #[test]
        fn denylisted_dotdir_is_not_reported() {
            let dir = tempfile::tempdir().expect("create temp dir");
            write_at(dir.path(), ".idea/workspace.ts", "export const a = 1;\n");
            write_at(dir.path(), ".husky/hook.js", "export const b = 2;\n");
            write_at(dir.path(), ".next/page.js", "export const c = 3;\n");
            write_at(dir.path(), ".pnpm/x.js", "export const d = 4;\n");

            let config = make_config(dir.path().to_path_buf(), false);
            let _ = discover_files(&config);

            assert!(
                dotdir_diagnostics(dir.path()).is_empty(),
                "build caches, VCS and package-manager state never advise"
            );
        }

        #[test]
        fn scoped_dotdir_is_traversed_and_not_reported() {
            let dir = tempfile::tempdir().expect("create temp dir");
            write_at(
                dir.path(),
                ".claude/hooks/probe.mjs",
                "export const a = 1;\n",
            );

            let config = make_config(dir.path().to_path_buf(), false);
            let scopes = [HiddenDirScope::new(
                dir.path().to_path_buf(),
                vec![".claude".to_owned()],
            )];
            let files = discover_files_with_additional_hidden_dirs(&config, &scopes);
            let names = file_names(&files, dir.path());

            assert!(
                dotdir_diagnostics(dir.path()).is_empty(),
                "a plugin- or script-contributed scope is admitted, so nothing was skipped"
            );
            assert!(names.contains(&".claude/hooks/probe.mjs".to_string()));
        }

        #[test]
        fn ignore_patterns_silence_the_skipped_source_dotdir() {
            let dir = tempfile::tempdir().expect("create temp dir");
            write_at(
                dir.path(),
                ".claude/hooks/probe.mjs",
                "export const a = 1;\n",
            );

            let config =
                make_config_with_ignores(dir.path().to_path_buf(), vec![".claude/**".to_owned()]);
            let _ = discover_files(&config);

            assert!(
                dotdir_diagnostics(dir.path()).is_empty(),
                "the documented silencing route works"
            );
        }

        #[test]
        fn dotdir_without_source_files_is_not_reported() {
            let dir = tempfile::tempdir().expect("create temp dir");
            write_at(dir.path(), ".claude/settings.json", "{}\n");
            write_at(dir.path(), ".claude/README.md", "# notes\n");

            let config = make_config(dir.path().to_path_buf(), false);
            let _ = discover_files(&config);

            assert!(dotdir_diagnostics(dir.path()).is_empty());
        }

        #[test]
        fn dotdir_source_at_scan_depth_limit_is_reported() {
            let dir = tempfile::tempdir().expect("create temp dir");
            write_at(dir.path(), ".claude/a/b/deep.ts", "export const a = 1;\n");

            let config = make_config(dir.path().to_path_buf(), false);
            let _ = discover_files(&config);

            assert_eq!(dotdir_diagnostics(dir.path()).len(), 1);
        }

        #[test]
        fn dotdir_source_below_scan_depth_limit_is_not_reported() {
            let dir = tempfile::tempdir().expect("create temp dir");
            write_at(
                dir.path(),
                ".claude/a/b/c/deeper.ts",
                "export const a = 1;\n",
            );

            let config = make_config(dir.path().to_path_buf(), false);
            let _ = discover_files(&config);

            assert!(
                dotdir_diagnostics(dir.path()).is_empty(),
                "the depth cap is real, so widening it stays a deliberate act"
            );
        }

        /// Mark `root` as a git worktree so the `ignore` crate applies the
        /// gitignore files below it. `require_git` is on by default, and it
        /// tests for the presence of `.git`, not for a valid object store.
        fn mark_as_git_repo(root: &Path) {
            std::fs::create_dir_all(root.join(".git")).expect("create .git marker");
        }

        #[test]
        fn gitignored_dotdir_contents_are_not_reported() {
            // The directory FORM (`.tooling/`) prunes the dotdir upstream of the
            // predicate, so these are the forms that reach it with every file
            // inside already ignored.
            for pattern in [".tooling/**", ".tooling/*", "**/.tooling/**", "*.ts"] {
                let dir = tempfile::tempdir().expect("create temp dir");
                mark_as_git_repo(dir.path());
                write_at(dir.path(), ".gitignore", &format!("{pattern}\n"));
                write_at(dir.path(), ".tooling/mod.ts", "export const a = 1;\n");

                let config = make_config(dir.path().to_path_buf(), false);
                let _ = discover_files(&config);

                assert!(
                    dotdir_diagnostics(dir.path()).is_empty(),
                    "gitignore pattern '{pattern}' excludes the contents, so neither \
                     advertised remedy would find anything there"
                );
            }
        }

        #[test]
        fn self_ignoring_dotdir_is_not_reported() {
            let dir = tempfile::tempdir().expect("create temp dir");
            mark_as_git_repo(dir.path());
            write_at(dir.path(), ".toolcache/.gitignore", "*\n");
            write_at(dir.path(), ".toolcache/mod.ts", "export const a = 1;\n");

            let config = make_config(dir.path().to_path_buf(), false);
            let _ = discover_files(&config);

            assert!(
                dotdir_diagnostics(dir.path()).is_empty(),
                "a cache directory that ignores itself has excluded its own contents"
            );
        }

        #[test]
        fn ungitignored_dotdir_in_a_git_repo_is_still_reported() {
            let dir = tempfile::tempdir().expect("create temp dir");
            mark_as_git_repo(dir.path());
            write_at(dir.path(), ".gitignore", "dist/\n");
            write_at(dir.path(), ".tooling/mod.ts", "export const a = 1;\n");

            let config = make_config(dir.path().to_path_buf(), false);
            let _ = discover_files(&config);

            assert_eq!(
                dotdir_diagnostics(dir.path()).len(),
                1,
                "the gitignore check must not swallow the case the diagnostic exists for"
            );
        }

        #[test]
        fn production_run_does_not_report_a_test_only_dotdir() {
            let dir = tempfile::tempdir().expect("create temp dir");
            write_at(dir.path(), ".qa/thing.test.ts", "export const a = 1;\n");
            write_at(dir.path(), ".qa/thing.stories.tsx", "export const b = 2;\n");

            let config = make_config(dir.path().to_path_buf(), true);
            let _ = discover_files(&config);

            assert!(
                dotdir_diagnostics(dir.path()).is_empty(),
                "a --production run would analyze none of those files, so the \
                 --root remedy would return nothing"
            );
        }

        #[test]
        fn production_run_still_reports_a_dotdir_with_production_source() {
            let dir = tempfile::tempdir().expect("create temp dir");
            write_at(dir.path(), ".qa/thing.test.ts", "export const a = 1;\n");
            write_at(dir.path(), ".qa/helper.ts", "export const b = 2;\n");

            let config = make_config(dir.path().to_path_buf(), true);
            let _ = discover_files(&config);

            assert_eq!(dotdir_diagnostics(dir.path()).len(), 1);
        }

        #[test]
        fn dotdir_with_only_generated_markup_is_not_reported() {
            let dir = tempfile::tempdir().expect("create temp dir");
            write_at(dir.path(), ".lighthouseci/lhr-1.html", "<html></html>\n");
            write_at(dir.path(), ".styles/theme.css", ":root { color: red; }\n");
            write_at(dir.path(), ".gql/schema.graphql", "type Query { a: Int }\n");

            let config = make_config(dir.path().to_path_buf(), false);
            let _ = discover_files(&config);

            assert!(
                dotdir_diagnostics(dir.path()).is_empty(),
                "the message claims imports and exports are lost, and these have none"
            );
        }

        #[test]
        fn generated_tool_and_foreign_vcs_dotdirs_are_not_reported() {
            let dir = tempfile::tempdir().expect("create temp dir");
            write_at(dir.path(), ".astro/types.d.ts", "export {};\n");
            write_at(dir.path(), ".wxt/types/imports.d.ts", "export {};\n");
            write_at(dir.path(), ".yalc/pkg/index.js", "export const a = 1;\n");
            write_at(dir.path(), ".jj/repo/config.js", "export const b = 2;\n");
            write_at(dir.path(), ".svn/pristine/y.js", "export const c = 3;\n");

            let config = make_config(dir.path().to_path_buf(), false);
            let _ = discover_files(&config);

            assert!(
                dotdir_diagnostics(dir.path()).is_empty(),
                "generated output and foreign VCS metadata are not first-party source"
            );
        }

        #[test]
        fn denylisted_dotdirs_do_not_consume_the_candidate_ceiling() {
            let dir = tempfile::tempdir().expect("create temp dir");
            // Sorted before the real candidate, and more of them than the
            // ceiling, so a cap applied before the name checks would hide it.
            for index in 0..(DOTDIR_SCAN_MAX_CANDIDATES + 8) {
                write_at(
                    dir.path(),
                    &format!("packages/pkg{index:03}/.turbo/blob.js"),
                    "export const a = 1;\n",
                );
            }
            write_at(dir.path(), "zz/.tooling/mod.ts", "export const b = 2;\n");

            let config = make_config(dir.path().to_path_buf(), false);
            let _ = discover_files(&config);

            let reported = dotdir_diagnostics(dir.path());
            assert_eq!(reported.len(), 1, "{reported:?}");
            assert!(reported[0].path.ends_with(".tooling"));
        }

        #[test]
        fn one_pathological_dotdir_cannot_starve_the_rest() {
            let dir = tempfile::tempdir().expect("create temp dir");
            // Wide and shallow, no source: exhausts this candidate's own budget.
            for index in 0..(DOTDIR_SCAN_MAX_ENTRIES * 2) {
                write_at(dir.path(), &format!(".aaa-noise/f{index}.bin"), "x");
            }
            write_at(dir.path(), ".zzz-real/mod.ts", "export const a = 1;\n");

            let config = make_config(dir.path().to_path_buf(), false);
            let _ = discover_files(&config);

            let reported = dotdir_diagnostics(dir.path());
            assert_eq!(reported.len(), 1, "{reported:?}");
            assert!(reported[0].path.ends_with(".zzz-real"));
        }

        #[test]
        fn repeat_walks_do_not_stack_skipped_source_dotdirs() {
            let dir = tempfile::tempdir().expect("create temp dir");
            write_at(
                dir.path(),
                ".claude/hooks/probe.mjs",
                "export const a = 1;\n",
            );

            let config = make_config(dir.path().to_path_buf(), false);
            let _ = discover_files(&config);
            let _ = discover_files(&config);

            assert_eq!(
                dotdir_diagnostics(dir.path()).len(),
                1,
                "each walk replaces its own root's source-discovery set"
            );
        }

        #[test]
        fn skips_large_one_line_js_as_minified_generated_output() {
            let dir = tempfile::tempdir().expect("create temp dir");
            let src = dir.path().join("src");
            std::fs::create_dir_all(&src).unwrap();
            let asset = src.join("index-abc123.js");
            std::fs::write(&asset, "x".repeat(MINIFIED_FILE_SKIP_BYTES as usize + 1)).unwrap();

            let config = make_config(dir.path().to_path_buf(), false);
            let files = discover_files(&config);
            let names = file_names(&files, dir.path());

            assert!(
                !names.contains(&"src/index-abc123.js".to_string()),
                "large one-line JS assets should be skipped before parsing"
            );

            let diagnostics = fallow_config::workspace_diagnostics_for(dir.path());
            assert!(
                diagnostics.iter().any(|diag| {
                    diag.path.ends_with("src/index-abc123.js")
                        && matches!(
                            diag.kind,
                            fallow_config::WorkspaceDiagnosticKind::SkippedMinifiedFile { .. }
                        )
                }),
                "the skipped minified asset is recorded for JSON output: {diagnostics:?}"
            );
        }

        #[test]
        fn unlimited_size_keeps_large_one_line_js() {
            let dir = tempfile::tempdir().expect("create temp dir");
            let src = dir.path().join("src");
            std::fs::create_dir_all(&src).unwrap();
            let asset = src.join("index-abc123.js");
            std::fs::write(&asset, "x".repeat(MINIFIED_FILE_SKIP_BYTES as usize + 1)).unwrap();

            let config = make_config_with_max_file_size(dir.path().to_path_buf(), None);
            let files = discover_files(&config);
            let names = file_names(&files, dir.path());

            assert!(
                names.contains(&"src/index-abc123.js".to_string()),
                "--max-file-size 0 should opt out of generated JS skipping"
            );
        }

        #[test]
        fn keeps_large_multiline_js() {
            let dir = tempfile::tempdir().expect("create temp dir");
            let src = dir.path().join("src");
            std::fs::create_dir_all(&src).unwrap();
            let asset = src.join("handwritten.js");
            let mut content = String::new();
            while content.len() <= MINIFIED_FILE_SKIP_BYTES as usize + 1 {
                content.push_str("export const value = 1;\n");
            }
            std::fs::write(&asset, content).unwrap();

            let config = make_config(dir.path().to_path_buf(), false);
            let files = discover_files(&config);
            let names = file_names(&files, dir.path());

            assert!(
                names.contains(&"src/handwritten.js".to_string()),
                "large multiline JS should not be treated as a generated minified asset"
            );
        }
    }
}
