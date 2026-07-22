//! Shared infrastructure for the GitHub-native text formats:
//! `--format github-annotations` (workflow commands on stdout) and
//! `--format github-summary` (step-summary markdown).
//!
//! The escaping contract is byte-compatible with the strict typed fallback in
//! `action/scripts/annotate.sh`, NOT the legacy jq `san` helper: message
//! bodies escape `%` to `%25`, CR to `%0D`, and LF to `%0A`; property values
//! (`file=`, `title=`) additionally escape `,` to `%2C` and `:` to `%3A`.
//! Non-ASCII text passes through as UTF-8. Line numbers below 1 clamp to 1,
//! and fallow's 0-based columns convert to GitHub's 1-based workflow-command
//! columns at this boundary.

use std::path::Path;
use std::sync::OnceLock;

use serde_json::Value;

/// Escape a workflow-command message body: `%` to `%25`, CR to `%0D`, LF to
/// `%0A`. The `%` escape runs first so already-escaped input escapes again
/// (matching the jq `esc` helper's `gsub` order).
#[must_use]
fn escape_message(text: &str) -> String {
    text.replace('%', "%25")
        .replace('\r', "%0D")
        .replace('\n', "%0A")
}

/// Escape a workflow-command property value (`file=`, `title=`): message
/// escaping plus `,` to `%2C` and `:` to `%3A`, because commas separate
/// properties and colons terminate the command prefix.
#[must_use]
fn escape_property(text: &str) -> String {
    escape_message(text).replace(',', "%2C").replace(':', "%3A")
}

/// Clamp a line number below 1 to 1: GitHub rejects `line=0`, and the typed
/// annotation fallback in `annotate.sh` applies the same floor.
#[must_use]
const fn clamp_line(line: u64) -> u64 {
    if line < 1 { 1 } else { line }
}

/// Convert fallow's 0-based column to GitHub's 1-based workflow-command
/// column (the jq layer's `col + 1` convention).
#[must_use]
pub(crate) const fn one_based_col(col: u64) -> u64 {
    col + 1
}

/// Annotation severity, ordered most-severe-first so a plain sort puts
/// errors ahead of warnings ahead of notices (GitHub shows at most 10
/// annotations per type per step; the worst findings must sort into view).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum AnnotationLevel {
    Error,
    Warning,
    Notice,
}

impl AnnotationLevel {
    /// The workflow-command name (`::error`, `::warning`, `::notice`).
    #[must_use]
    const fn command(self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Warning => "warning",
            Self::Notice => "notice",
        }
    }
}

/// One GitHub workflow-command annotation, pre-escaping. `message` holds real
/// newlines; `render_annotation` applies the escaping contract.
#[derive(Debug)]
pub struct Annotation {
    pub(crate) level: AnnotationLevel,
    /// Repo-root-relative once [`PathRebase::apply`] ran.
    pub(crate) path: String,
    /// Omitted from the command when `None`; clamped to 1 when below 1.
    pub(crate) line: Option<u64>,
    pub(crate) end_line: Option<u64>,
    /// Already converted to GitHub's 1-based columns.
    pub(crate) col: Option<u64>,
    pub(crate) title: String,
    pub(crate) message: String,
}

/// Render one annotation as a workflow-command line. Property order matches
/// the jq layer: `file`, `line`, `endLine`, `col`, `title`.
#[must_use]
pub(crate) fn render_annotation(annotation: &Annotation) -> String {
    use std::fmt::Write as _;
    let mut props = format!("file={}", escape_property(&annotation.path));
    if let Some(line) = annotation.line {
        let _ = write!(props, ",line={}", clamp_line(line));
    }
    if let Some(end_line) = annotation.end_line {
        let _ = write!(props, ",endLine={end_line}");
    }
    if let Some(col) = annotation.col {
        let _ = write!(props, ",col={col}");
    }
    format!(
        "::{} {props},title={}::{}",
        annotation.level.command(),
        escape_property(&annotation.title),
        escape_message(&annotation.message)
    )
}

/// Sort annotations most-severe-first, then by path, then by line. Default
/// result ordering is path-sorted (ADR-004), so without this sort GitHub's
/// visible 10 per type would be the lexicographically first paths, not the
/// worst findings.
pub(crate) fn sort_annotations(annotations: &mut [Annotation]) {
    annotations.sort_by(|a, b| {
        a.level
            .cmp(&b.level)
            .then_with(|| a.path.cmp(&b.path))
            .then_with(|| a.line.unwrap_or(0).cmp(&b.line.unwrap_or(0)))
    });
}

/// The trailing total notice: always emitted after the annotation stream when
/// at least one annotation exists, so consumers know when GitHub's 10-per-type
/// display cap truncated the visible set. There is deliberately no
/// producer-side cap (the bundled action keeps its own `MAX_ANNOTATIONS`
/// during migration).
#[must_use]
pub(crate) fn budget_notice(total: usize) -> Option<String> {
    (total > 0).then(|| {
        let noun = if total == 1 {
            "annotation"
        } else {
            "annotations"
        };
        format!(
            "::notice::fallow emitted {total} {noun}; GitHub shows at most 10 per type per step"
        )
    })
}

/// Package manager for fix-command hints, mirroring the jq layer's
/// `PKG_MANAGER` parameterization plus native lockfile sniffing (bun is new
/// relative to the jq layer).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum PackageManager {
    #[default]
    Npm,
    Pnpm,
    Yarn,
    Bun,
}

impl PackageManager {
    /// The `remove <pkg>` command for this package manager.
    #[must_use]
    pub(crate) fn remove_command(self, package: &str) -> String {
        match self {
            Self::Npm => format!("npm uninstall {package}"),
            Self::Pnpm => format!("pnpm remove {package}"),
            Self::Yarn => format!("yarn remove {package}"),
            Self::Bun => format!("bun remove {package}"),
        }
    }

    /// The `add <pkg>` command for this package manager.
    #[must_use]
    pub(crate) fn add_command(self, package: &str) -> String {
        match self {
            Self::Npm => format!("npm install {package}"),
            Self::Pnpm => format!("pnpm add {package}"),
            Self::Yarn => format!("yarn add {package}"),
            Self::Bun => format!("bun add {package}"),
        }
    }
}

/// Resolve the package manager: an explicit `PKG_MANAGER` env value wins
/// (action parity; unrecognized values fall back to npm exactly like the jq
/// `else` branch), otherwise lockfile sniffing at the analysis root.
#[must_use]
fn resolve_package_manager(env_value: Option<&str>, root: &Path) -> PackageManager {
    if let Some(value) = env_value {
        return match value {
            "pnpm" => PackageManager::Pnpm,
            "yarn" => PackageManager::Yarn,
            "bun" => PackageManager::Bun,
            _ => PackageManager::Npm,
        };
    }
    if root.join("pnpm-lock.yaml").is_file() {
        PackageManager::Pnpm
    } else if root.join("yarn.lock").is_file() {
        PackageManager::Yarn
    } else if root.join("bun.lock").is_file() || root.join("bun.lockb").is_file() {
        PackageManager::Bun
    } else {
        PackageManager::Npm
    }
}

/// How report paths are rebased onto the git repository root. CI platforms
/// address files by repo-root-relative path (GitHub annotations, GitLab's
/// Code Quality widget, the review-discussion APIs), while fallow emits
/// analysis-root-relative paths; when the analysis root is a subdirectory
/// (e.g. `packages/app/`), every path needs the offset prefixed.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub enum PathRebase {
    /// Paths pass through unchanged (analysis root == repo root, or no git).
    #[default]
    None,
    /// Prefix (no trailing slash) prepended to every emitted path.
    Prefix(String),
}

impl PathRebase {
    /// Apply the rebase to one analysis-root-relative path.
    #[must_use]
    pub(crate) fn apply(&self, path: &str) -> String {
        match self {
            Self::None => path.to_owned(),
            Self::Prefix(prefix) => format!("{prefix}/{path}"),
        }
    }

    /// The prefix this rebase prepends, empty when it prepends nothing.
    ///
    /// Consumers that resolve ownership from an already-rebased path strip
    /// this back off to recover the analysis-root-relative path CODEOWNERS
    /// patterns are written against.
    #[must_use]
    fn prefix(&self) -> &str {
        match self {
            Self::None => "",
            Self::Prefix(prefix) => prefix,
        }
    }

    /// Resolve the rebase: an explicit `--report-path-prefix` wins over
    /// git-toplevel detection; no git and no flag means paths pass through.
    /// An explicit empty prefix disables rebasing.
    #[must_use]
    fn resolve(root: &Path, explicit: Option<&str>) -> Self {
        if let Some(prefix) = explicit {
            return Self::from_explicit_prefix(prefix);
        }
        let Some(toplevel) = crate::base_worktree::git_toplevel(root) else {
            return Self::None;
        };
        let canonical_root = dunce::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
        Self::from_root_offset(&canonical_root, &toplevel)
    }

    fn from_explicit_prefix(prefix: &str) -> Self {
        let normalized = prefix.replace('\\', "/");
        let trimmed = normalized.trim_matches('/');
        if trimmed.is_empty() {
            Self::None
        } else {
            Self::Prefix(trimmed.to_owned())
        }
    }

    /// Compute the analysis root's offset below the repo toplevel. Pure so
    /// tests can pin the rebasing behavior without a live git repo.
    #[must_use]
    fn from_root_offset(root: &Path, toplevel: &Path) -> Self {
        match root.strip_prefix(toplevel) {
            Ok(offset) if offset.as_os_str().is_empty() => Self::None,
            Ok(offset) => Self::Prefix(offset.display().to_string().replace('\\', "/")),
            Err(_) => Self::None,
        }
    }
}

/// Process-wide `--report-path-prefix` override, set once by `main`
/// after parse (same ambient pattern as the report sink and the
/// max-file-size override).
static REPORT_PATH_PREFIX: OnceLock<Option<String>> = OnceLock::new();

/// Record the `--report-path-prefix` flag value. Call at most once.
pub(crate) fn set_report_path_prefix(prefix: Option<String>) {
    let _ = REPORT_PATH_PREFIX.set(prefix);
}

fn report_path_prefix() -> Option<&'static str> {
    REPORT_PATH_PREFIX
        .get()
        .and_then(|prefix| prefix.as_deref())
}

/// The presentation prefix for this run, resolved once.
///
/// `report_rebase` shells out to `git rev-parse --show-toplevel`; the emitters
/// need the answer on every issue, and the review renderer has no `root` to
/// re-derive it from.
static RESOLVED_REPORT_PREFIX: OnceLock<String> = OnceLock::new();

/// Resolve the presentation prefix once, after `--report-path-prefix` is
/// recorded. Call at most once.
pub(crate) fn init_report_prefix(root: &Path) {
    let _ = RESOLVED_REPORT_PREFIX.set(report_rebase(root).prefix().to_owned());
}

/// The prefix prepended to every CI-facing path emitted this run. Empty when
/// the analysis root is the repository root, or when no CLI run resolved it.
#[must_use]
pub(crate) fn report_prefix() -> &'static str {
    RESOLVED_REPORT_PREFIX.get().map_or("", String::as_str)
}

/// Rebase every CI-facing report path emitted for this run onto the repo root.
///
/// Shared by the GitHub renderers and the CodeClimate emitters (which the
/// review and sticky-summary formats derive their paths from), so all CI
/// surfaces address files the same way the platform does.
#[must_use]
fn report_rebase(root: &Path) -> PathRebase {
    PathRebase::resolve(root, report_path_prefix())
}

/// Ambient options for the GitHub renderers, resolved once per render at the
/// CLI print boundary so the pure render functions stay deterministic.
#[derive(Debug, Default)]
pub struct RenderOptions {
    pub rebase: PathRebase,
    pub pm: PackageManager,
}

/// Resolve render options from the process environment and the analysis root.
#[must_use]
pub(crate) fn resolve_render_options(root: &Path) -> RenderOptions {
    let env_pm = std::env::var("PKG_MANAGER").ok();
    RenderOptions {
        rebase: report_rebase(root),
        pm: resolve_package_manager(env_pm.as_deref(), root),
    }
}

/// Iterate an optional array field (`.[key][]?` in jq terms).
pub(crate) fn arr<'a>(value: &'a Value, key: &str) -> impl Iterator<Item = &'a Value> {
    value
        .get(key)
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
        .iter()
}

/// String field, or `""` when missing or not a string.
#[must_use]
pub(crate) fn s<'a>(value: &'a Value, key: &str) -> &'a str {
    value.get(key).and_then(Value::as_str).unwrap_or_default()
}

/// Unsigned number field, or 0 when missing.
#[must_use]
pub(crate) fn u(value: &Value, key: &str) -> u64 {
    value.get(key).and_then(Value::as_u64).unwrap_or_default()
}

/// Boolean field, or `false` when missing.
#[must_use]
pub(crate) fn b(value: &Value, key: &str) -> bool {
    value.get(key).and_then(Value::as_bool).unwrap_or_default()
}

/// Format a JSON number the way jq interpolates it: integers without a
/// decimal point, floats in shortest round-trip form.
#[must_use]
pub(crate) fn fmt_num(value: &Value) -> String {
    if let Some(int) = value.as_i64() {
        return int.to_string();
    }
    value.as_f64().map_or_else(String::new, |float| {
        if float.fract() == 0.0 && float.abs() < 1e15 {
            format!("{}", float as i64)
        } else {
            format!("{float}")
        }
    })
}

/// Format a numeric field, or `"0"` when missing.
#[must_use]
pub(crate) fn num(value: &Value, key: &str) -> String {
    value.get(key).map_or_else(|| "0".to_owned(), fmt_num)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escape_property_path_with_comma() {
        assert_eq!(escape_property("src/a,b.ts"), "src/a%2Cb.ts");
    }

    #[test]
    fn escape_property_title_with_colon() {
        assert_eq!(escape_property("High: complexity"), "High%3A complexity");
    }

    #[test]
    fn escape_message_percent() {
        assert_eq!(escape_message("100% sure"), "100%25 sure");
    }

    #[test]
    fn escape_message_crlf_reason() {
        assert_eq!(escape_message("reason\r\nnext"), "reason%0D%0Anext");
    }

    #[test]
    fn escape_message_non_ascii_passes_through_as_utf8() {
        assert_eq!(escape_message("München.ts"), "München.ts");
        assert_eq!(escape_property("München.ts"), "München.ts");
    }

    #[test]
    fn escape_message_empty_string() {
        assert_eq!(escape_message(""), "");
        assert_eq!(escape_property(""), "");
    }

    #[test]
    fn escape_message_already_escaped_input_escapes_again() {
        // Matches jq: `%` escaping runs first, so `%25` becomes `%2525`.
        assert_eq!(escape_message("50%25 done"), "50%2525 done");
    }

    #[test]
    fn escape_message_leaves_comma_and_colon_untouched() {
        assert_eq!(escape_message("a,b: c"), "a,b: c");
    }

    #[test]
    fn escape_property_escapes_all_reserved_characters_together() {
        assert_eq!(escape_property("a,b:c%d\r\ne"), "a%2Cb%3Ac%25d%0D%0Ae");
    }

    #[test]
    fn clamp_line_floors_zero_to_one() {
        assert_eq!(clamp_line(0), 1);
    }

    #[test]
    fn clamp_line_keeps_positive_lines() {
        assert_eq!(clamp_line(1), 1);
        assert_eq!(clamp_line(42), 42);
    }

    #[test]
    fn one_based_col_converts_zero_based() {
        assert_eq!(one_based_col(0), 1);
        assert_eq!(one_based_col(7), 8);
    }

    fn annotation(level: AnnotationLevel, path: &str, line: Option<u64>) -> Annotation {
        Annotation {
            level,
            path: path.to_owned(),
            line,
            end_line: None,
            col: None,
            title: "T".to_owned(),
            message: "m".to_owned(),
        }
    }

    #[test]
    fn render_annotation_property_order_matches_jq_layer() {
        let rendered = render_annotation(&Annotation {
            level: AnnotationLevel::Warning,
            path: "src/a.ts".to_owned(),
            line: Some(3),
            end_line: Some(9),
            col: Some(5),
            title: "Code duplication".to_owned(),
            message: "first\nsecond".to_owned(),
        });
        assert_eq!(
            rendered,
            "::warning file=src/a.ts,line=3,endLine=9,col=5,title=Code duplication::first%0Asecond"
        );
    }

    #[test]
    fn render_annotation_omits_absent_properties_and_clamps_line() {
        let rendered =
            render_annotation(&annotation(AnnotationLevel::Error, "pkg/a,b.ts", Some(0)));
        assert_eq!(rendered, "::error file=pkg/a%2Cb.ts,line=1,title=T::m");
        let no_line = render_annotation(&annotation(AnnotationLevel::Notice, "a.ts", None));
        assert_eq!(no_line, "::notice file=a.ts,title=T::m");
    }

    #[test]
    fn sort_annotations_orders_severity_then_path_then_line() {
        let mut annotations = vec![
            annotation(AnnotationLevel::Notice, "a/a.ts", Some(1)),
            annotation(AnnotationLevel::Warning, "z/z.ts", Some(9)),
            annotation(AnnotationLevel::Error, "z/late.ts", Some(2)),
            annotation(AnnotationLevel::Warning, "a/a.ts", Some(5)),
            annotation(AnnotationLevel::Warning, "a/a.ts", Some(2)),
        ];
        sort_annotations(&mut annotations);
        let order: Vec<(AnnotationLevel, &str, Option<u64>)> = annotations
            .iter()
            .map(|a| (a.level, a.path.as_str(), a.line))
            .collect();
        assert_eq!(
            order,
            vec![
                (AnnotationLevel::Error, "z/late.ts", Some(2)),
                (AnnotationLevel::Warning, "a/a.ts", Some(2)),
                (AnnotationLevel::Warning, "a/a.ts", Some(5)),
                (AnnotationLevel::Warning, "z/z.ts", Some(9)),
                (AnnotationLevel::Notice, "a/a.ts", Some(1)),
            ]
        );
    }

    #[test]
    fn budget_notice_appears_only_when_annotations_exist() {
        assert_eq!(budget_notice(0), None);
        assert_eq!(
            budget_notice(12).as_deref(),
            Some(
                "::notice::fallow emitted 12 annotations; GitHub shows at most 10 per type per step"
            )
        );
    }

    #[test]
    fn budget_notice_uses_singular_noun_for_one_annotation() {
        assert_eq!(
            budget_notice(1).as_deref(),
            Some(
                "::notice::fallow emitted 1 annotation; GitHub shows at most 10 per type per step"
            )
        );
    }

    #[test]
    fn package_manager_env_wins_and_unrecognized_falls_back_to_npm() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("pnpm-lock.yaml"), "").expect("write");
        assert_eq!(
            resolve_package_manager(Some("yarn"), dir.path()),
            PackageManager::Yarn
        );
        assert_eq!(
            resolve_package_manager(Some("something-else"), dir.path()),
            PackageManager::Npm
        );
    }

    #[test]
    fn package_manager_lockfile_sniffing_covers_bun() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert_eq!(
            resolve_package_manager(None, dir.path()),
            PackageManager::Npm
        );
        std::fs::write(dir.path().join("bun.lockb"), "").expect("write");
        assert_eq!(
            resolve_package_manager(None, dir.path()),
            PackageManager::Bun
        );
        std::fs::write(dir.path().join("bun.lock"), "").expect("write");
        assert_eq!(
            resolve_package_manager(None, dir.path()),
            PackageManager::Bun
        );
        std::fs::write(dir.path().join("yarn.lock"), "").expect("write");
        assert_eq!(
            resolve_package_manager(None, dir.path()),
            PackageManager::Yarn
        );
        std::fs::write(dir.path().join("pnpm-lock.yaml"), "").expect("write");
        assert_eq!(
            resolve_package_manager(None, dir.path()),
            PackageManager::Pnpm
        );
    }

    #[test]
    fn package_manager_commands() {
        assert_eq!(PackageManager::Npm.remove_command("x"), "npm uninstall x");
        assert_eq!(PackageManager::Pnpm.remove_command("x"), "pnpm remove x");
        assert_eq!(PackageManager::Yarn.add_command("x"), "yarn add x");
        assert_eq!(PackageManager::Bun.add_command("x"), "bun add x");
        assert_eq!(PackageManager::Bun.remove_command("x"), "bun remove x");
    }

    #[test]
    fn path_rebase_from_root_offset_prefixes_subdirectory_roots() {
        use std::path::Path;
        let rebase =
            PathRebase::from_root_offset(Path::new("/repo/packages/app"), Path::new("/repo"));
        assert_eq!(rebase, PathRebase::Prefix("packages/app".to_owned()));
        assert_eq!(rebase.apply("src/a.ts"), "packages/app/src/a.ts");
    }

    #[test]
    fn path_rebase_is_none_at_repo_root_or_outside_toplevel() {
        use std::path::Path;
        assert_eq!(
            PathRebase::from_root_offset(Path::new("/repo"), Path::new("/repo")),
            PathRebase::None
        );
        assert_eq!(
            PathRebase::from_root_offset(Path::new("/elsewhere"), Path::new("/repo")),
            PathRebase::None
        );
        assert_eq!(PathRebase::None.apply("src/a.ts"), "src/a.ts");
    }

    #[test]
    fn path_rebase_explicit_prefix_wins_and_is_trimmed() {
        use std::path::Path;
        let rebase = PathRebase::resolve(Path::new("/nonexistent-fallow-root"), Some("/pkg/app/"));
        assert_eq!(rebase, PathRebase::Prefix("pkg/app".to_owned()));
        let empty = PathRebase::resolve(Path::new("/nonexistent-fallow-root"), Some("//"));
        assert_eq!(empty, PathRebase::None);
    }

    #[test]
    fn fmt_num_matches_jq_interpolation() {
        assert_eq!(fmt_num(&serde_json::json!(42)), "42");
        assert_eq!(fmt_num(&serde_json::json!(24.0)), "24");
        assert_eq!(fmt_num(&serde_json::json!(30.5)), "30.5");
        assert_eq!(fmt_num(&serde_json::json!(-3)), "-3");
    }

    #[test]
    fn value_helpers_default_missing_fields() {
        let value = serde_json::json!({"name": "x", "line": 4, "flag": true, "items": [1]});
        assert_eq!(s(&value, "name"), "x");
        assert_eq!(s(&value, "missing"), "");
        assert_eq!(u(&value, "line"), 4);
        assert_eq!(u(&value, "missing"), 0);
        assert!(b(&value, "flag"));
        assert!(!b(&value, "missing"));
        assert_eq!(arr(&value, "items").count(), 1);
        assert_eq!(arr(&value, "missing").count(), 0);
        assert_eq!(num(&value, "line"), "4");
        assert_eq!(num(&value, "missing"), "0");
    }
}
