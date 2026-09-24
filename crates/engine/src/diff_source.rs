//! Read a unified diff and place it under the directory its paths are relative
//! to, or say why the run cannot.
//!
//! The CLI (`--diff-file`, `--diff-stdin`, `$FALLOW_DIFF_FILE`) and the
//! programmatic API (a diff inherited from `FALLOW_DIFF_FILE`) both call these
//! functions. A diff that one surface stands down on therefore stands down on
//! the other with the same reason token and the same sentence, which is what
//! `request_outcomes["diff-filter"]` publishes on both.

use std::io::Read as _;
use std::path::{Path, PathBuf};

use fallow_output::{DiffIndex, MAX_DIFF_BYTES};

/// Why a supplied diff could not be applied, plus the sentence that says so.
///
/// The reason token is what `request_outcomes["diff-filter"].reason` publishes
/// and the message is what both the stderr line and that entry's `message`
/// render, so the log a human read and the envelope a script read cannot state
/// different things. Every stand-down returns one of these instead of printing
/// where it happens, so the caller decides whether to print and always records
/// (issue #2688).
#[derive(Debug)]
pub struct DiffStandDown {
    reason: &'static str,
    message: String,
}

impl DiffStandDown {
    fn new(reason: &'static str, message: String) -> Self {
        Self { reason, message }
    }

    /// The kebab-case reason token.
    #[must_use]
    pub const fn reason(&self) -> &'static str {
        self.reason
    }

    /// The sentence that names the problem and the next step.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }

    /// The reason token and the sentence, by value.
    #[must_use]
    pub fn into_parts(self) -> (&'static str, String) {
        (self.reason, self.message)
    }

    fn oversize(label: &str, bytes: u64, cap: u64) -> Self {
        Self::new(
            "oversize",
            format!(
                "{label} is {bytes} bytes (cap {cap}); line-level filtering disabled, \
                 reporting all findings. Narrow the diff; the cap is fixed."
            ),
        )
    }

    fn unreadable(label: &str, err: &std::io::Error) -> Self {
        Self::new(
            "unreadable",
            format!(
                "could not read {label}: {err} (line-level filtering disabled, \
                 reporting all findings). Check the path exists and is readable."
            ),
        )
    }

    fn not_utf8(label: &str, err: &std::string::FromUtf8Error) -> Self {
        Self::new(
            "not-utf8",
            format!(
                "could not read {label} as UTF-8: {err} (line-level filtering disabled, \
                 reporting all findings). Regenerate the diff as UTF-8 text."
            ),
        )
    }

    /// The diff's paths resolve equally well under two different directories,
    /// so existence alone cannot place its base. Rather than filter against a
    /// guess (whose wrong half drops every source-anchored finding), the run
    /// discards the diff and reports at full scope, so the message names the
    /// ambiguity and says so rather than letting silence imply the report was
    /// scoped.
    fn ambiguous_base(candidate_bases: &[PathBuf], root: &Path, label: &str) -> Self {
        let bases = join_bases(candidate_bases, root, " and ");
        Self::new(
            "ambiguous-base",
            format!(
                "the paths in {label} name existing files under {bases}, so their base is \
                 ambiguous and fallow cannot tell which one the diff is relative to. It will \
                 not filter against a guess: every finding is reported (full scope, not scoped \
                 to the diff). Generate the diff from the repository root (plain `git diff`, \
                 not `git diff --relative`) to scope the report."
            ),
        )
    }

    /// A diff whose paths name no file under any candidate base was almost
    /// certainly generated relative to some other directory. fallow cannot
    /// place it, so it discards the diff and reports at full scope. Say so,
    /// once, rather than let the unscoped report imply the diff was applied.
    fn foreign_namespace(
        index: &DiffIndex,
        candidate_bases: &[PathBuf],
        root: &Path,
        label: &str,
    ) -> Self {
        let total = index.touched_files().count();
        let bases = join_bases(candidate_bases, root, ", ");
        Self::new(
            "foreign-namespace",
            format!(
                "none of the {total} file(s) named by {label} exist under {bases}; the diff's \
                 paths look relative to a different directory. fallow cannot place the diff, so \
                 every finding is reported (full scope, not scoped to the diff). Regenerate the \
                 diff from one of those directories to scope the report."
            ),
        )
    }
}

fn join_bases(candidate_bases: &[PathBuf], root: &Path, separator: &str) -> String {
    candidate_bases
        .iter()
        .map(|base| base_label(base, root))
        .collect::<Vec<_>>()
        .join(separator)
}

/// Name a candidate base without putting the machine's checkout path in it.
///
/// This sentence is the `message` of a wire member, and every other
/// path-bearing member of a fallow envelope is project-root-relative, so an
/// absolute base here would make one input's output differ between checkouts.
/// The two candidates a run offers are the analysis root and the git toplevel
/// above it ([`diff_base_candidates`]), and naming them by their relation to
/// the root tells the user which directory to regenerate the diff from at least
/// as well as the absolute path did: what they need is the path prefix their
/// diff is missing, which is exactly the offset reported here.
fn base_label(base: &Path, root: &Path) -> String {
    if base == root {
        return "the project root".to_owned();
    }
    if let Ok(offset) = root.strip_prefix(base) {
        let offset = offset.display().to_string().replace('\\', "/");
        return format!("the repository root (the project root is {offset} below it)");
    }
    if let Ok(inside) = base.strip_prefix(root) {
        return inside.display().to_string().replace('\\', "/");
    }
    "a directory outside the project root".to_owned()
}

/// Read a diff from `reader`, up to `limit` bytes, as UTF-8 text.
///
/// # Errors
///
/// Returns the `unreadable`, `oversize` or `not-utf8` stand-down.
pub fn read_diff_text(
    reader: impl std::io::Read,
    label: &str,
    limit: u64,
) -> Result<String, DiffStandDown> {
    let mut bytes = Vec::new();
    if let Err(err) = reader.take(limit + 1).read_to_end(&mut bytes) {
        return Err(DiffStandDown::unreadable(label, &err));
    }
    if bytes.len() as u64 > limit {
        return Err(DiffStandDown::oversize(label, bytes.len() as u64, limit));
    }
    String::from_utf8(bytes).map_err(|err| DiffStandDown::not_utf8(label, &err))
}

/// Read a diff file as UTF-8 text, within [`MAX_DIFF_BYTES`].
///
/// # Errors
///
/// Returns the `unreadable`, `oversize` or `not-utf8` stand-down.
pub fn read_diff_file(path: &Path, label: &str) -> Result<String, DiffStandDown> {
    if let Ok(meta) = std::fs::metadata(path)
        && meta.len() > MAX_DIFF_BYTES
    {
        return Err(DiffStandDown::oversize(label, meta.len(), MAX_DIFF_BYTES));
    }
    match std::fs::File::open(path) {
        Ok(file) => read_diff_text(file, label, MAX_DIFF_BYTES),
        Err(err) => Err(DiffStandDown::unreadable(label, &err)),
    }
}

/// Place a parsed diff under the directory its paths are relative to.
///
/// A diff that parsed but names no analyzable head-side file (empty,
/// deletion-only, or binary-only) changed nothing a finding can be attributed
/// to. That is a real, EMPTY scope, not an unplaceable base: the empty index is
/// kept so every source-anchored finding filters out. Only a diff that cannot
/// be placed (foreign or ambiguous base) stands down, and the run then reports
/// at full scope.
///
/// # Errors
///
/// Returns the `foreign-namespace` or `ambiguous-base` stand-down.
pub fn place_diff(
    index: DiffIndex,
    root: &Path,
    candidate_bases: &[PathBuf],
    label: &str,
) -> Result<DiffIndex, DiffStandDown> {
    if index.touched_files().next().is_none() {
        return Ok(index);
    }
    // The diff names files, but none under any candidate base (foreign), or
    // equally under two at once (ambiguous). Either way the findings cannot be
    // expressed in its namespace. An unfilterable path is RETAINED, never
    // silently dropped, so drop the diff instead of the findings and report at
    // full scope.
    match choose_diff_base(&index, candidate_bases) {
        None => Err(DiffStandDown::foreign_namespace(
            &index,
            candidate_bases,
            root,
            label,
        )),
        Some(chosen) if chosen.ambiguous => {
            Err(DiffStandDown::ambiguous_base(candidate_bases, root, label))
        }
        Some(chosen) => {
            let offset = root_offset_below(&chosen.base, root);
            Ok(index.with_base(chosen.base).with_root_offset(offset))
        }
    }
}

/// Where the analysis root sits below `base`, forward-slashed, empty when they
/// are the same directory.
fn root_offset_below(base: &Path, root: &Path) -> String {
    root.strip_prefix(base)
        .map(|offset| offset.display().to_string().replace('\\', "/"))
        .unwrap_or_default()
}

/// The base a diff's paths were written relative to, plus whether the evidence
/// actually distinguished it from the runner-up.
struct ChosenBase {
    base: PathBuf,
    ambiguous: bool,
}

/// Decide which directory the diff's paths are relative to.
///
/// A unified diff carries no statement of its own base. `git diff` writes paths
/// relative to the repository toplevel, but `git diff --relative` writes them
/// relative to the invoking directory, and both reach fallow. Assuming either
/// one silently drops every source-anchored finding for users of the other.
///
/// The paths themselves settle it: they name files that exist on disk. Score
/// each candidate by how many of the diff's paths resolve under it and take the
/// best. `candidate_bases` is ordered most-preferred first, so an exact tie
/// keeps the caller's precedence.
///
/// A tie is not a decision. A repo with both `<toplevel>/src/a.ts` and
/// `<root>/src/a.ts` resolves the diff path `src/a.ts` under either candidate,
/// and existence alone cannot say which the diff meant. Picking the preferred
/// one and staying silent would reproduce the empty-report-looks-clean failure
/// this whole mechanism exists to prevent, so the tie is reported.
/// `None` means the diff names nothing under any candidate.
fn choose_diff_base(index: &DiffIndex, candidate_bases: &[PathBuf]) -> Option<ChosenBase> {
    let mut scored: Vec<(usize, &PathBuf)> = candidate_bases
        .iter()
        .map(|base| {
            let resolved = index
                .touched_files()
                .filter(|path| base.join(path).exists())
                .count();
            (resolved, base)
        })
        .filter(|(resolved, _)| *resolved > 0)
        .collect();

    // Stable sort by score, descending: equal scores keep caller precedence.
    scored.sort_by(|(a, _), (b, _)| b.cmp(a));
    let (best_score, best_base) = *scored.first()?;
    let ambiguous = scored
        .get(1)
        .is_some_and(|(runner_up, _)| *runner_up == best_score);

    Some(ChosenBase {
        base: best_base.clone(),
        ambiguous,
    })
}

/// Directories a supplied unified diff's paths might be relative to, most
/// preferred first: the git toplevel above `root`, then `root` itself. Only
/// `root` outside a git repository or when `root` is the toplevel.
///
/// `git diff` writes paths relative to the repository toplevel, while
/// `git diff --relative` writes them relative to the invoking directory. A
/// unified diff does not say which one it is, so the caller offers both and the
/// paths decide (see [`place_diff`]). The two coincide for a single-package
/// repo, and differ when the root addresses a package inside a monorepo.
///
/// The toplevel is only used to measure how far `root` sits below it; the
/// returned base is that many components popped off `root` itself, so it keeps
/// `root`'s spelling. Finding paths are built from `root`, and a canonicalized
/// base would fail to prefix them wherever the two disagree (`/tmp` vs
/// `/private/tmp` on macOS).
#[must_use]
pub fn diff_base_candidates(root: &Path) -> Vec<PathBuf> {
    let Some(toplevel) = git_toplevel_base(root) else {
        return vec![root.to_path_buf()];
    };
    if toplevel == root {
        return vec![root.to_path_buf()];
    }
    vec![toplevel, root.to_path_buf()]
}

/// `root` with its offset below the git toplevel popped off, preserving
/// `root`'s spelling. `None` outside a git repo.
fn git_toplevel_base(root: &Path) -> Option<PathBuf> {
    let toplevel = crate::changed_files::resolve_git_toplevel(root).ok()?;
    let canonical_root = dunce::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    let offset = canonical_root.strip_prefix(&toplevel).ok()?;
    let mut base = root.to_path_buf();
    for _ in offset.components() {
        if !base.pop() {
            return None;
        }
    }
    Some(base)
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    #[test]
    fn the_reader_accepts_the_exact_limit_and_rejects_one_byte_more() {
        assert_eq!(
            read_diff_text(Cursor::new(b"12345678"), "test diff", 8).unwrap(),
            "12345678"
        );
        let stand_down = read_diff_text(Cursor::new(b"123456789"), "test diff", 8).unwrap_err();
        assert_eq!(stand_down.reason(), "oversize");
    }

    #[test]
    fn the_reader_rejects_invalid_utf8() {
        let stand_down = read_diff_text(Cursor::new([0xff, 0xfe]), "test diff", 8).unwrap_err();
        assert_eq!(stand_down.reason(), "not-utf8");
    }

    #[test]
    fn a_missing_diff_file_stands_down_as_unreadable() {
        let dir = tempfile::tempdir().expect("tempdir");
        let stand_down = read_diff_file(&dir.path().join("absent.diff"), "label").unwrap_err();
        assert_eq!(stand_down.reason(), "unreadable");
        assert!(stand_down.message().starts_with("could not read label: "));
    }

    /// The stand-down message is a wire field, so it names the candidate bases
    /// by their relation to the project root rather than by absolute path.
    #[test]
    fn a_stand_down_names_its_bases_without_the_checkout_path() {
        let root = Path::new("/checkout/packages/app");
        let toplevel = Path::new("/checkout");
        let index = DiffIndex::from_unified_diff(
            "diff --git a/src/a.ts b/src/a.ts\n\
             --- a/src/a.ts\n\
             +++ b/src/a.ts\n\
             @@ -0,0 +1,1 @@\n\
             +export const a = 1;\n",
        );
        let bases = vec![toplevel.to_path_buf(), root.to_path_buf()];

        let foreign = DiffStandDown::foreign_namespace(&index, &bases, root, "--diff-file pr.diff");
        assert_eq!(foreign.reason(), "foreign-namespace");
        let ambiguous = DiffStandDown::ambiguous_base(&bases, root, "--diff-file pr.diff");
        assert_eq!(ambiguous.reason(), "ambiguous-base");

        for message in [foreign.message(), ambiguous.message()] {
            assert!(
                !message.contains("/checkout"),
                "no absolute base reaches the wire: {message}"
            );
            assert!(
                message.contains("the project root"),
                "the analysis root is named: {message}"
            );
            assert!(
                message.contains("the repository root (the project root is packages/app below it)"),
                "the toplevel is named with the offset the diff is missing: {message}"
            );
        }
    }

    /// A single-candidate run (analysis root at the repository toplevel) names
    /// the one base it had, and still names no path.
    #[test]
    fn a_single_candidate_base_is_named_as_the_project_root() {
        let root = Path::new("/checkout");
        let stand_down = DiffStandDown::ambiguous_base(&[root.to_path_buf()], root, "--diff-stdin");
        assert!(
            stand_down
                .message()
                .contains("under the project root, so their base is ambiguous"),
            "{}",
            stand_down.message()
        );
        assert!(!stand_down.message().contains("/checkout"));
    }

    /// The cap has no override, so the remedy cannot suggest raising it.
    #[test]
    fn the_oversize_remedy_asks_only_for_something_the_user_can_do() {
        let stand_down =
            DiffStandDown::oversize("--diff-file pr.diff", MAX_DIFF_BYTES + 1, MAX_DIFF_BYTES);
        assert!(
            !stand_down.message().contains("raise the cap"),
            "{}",
            stand_down.message()
        );
        assert!(stand_down.message().contains("Narrow the diff"));
    }
}
