//! `fallow trace-error`: resolve a runtime stack trace's frames against the
//! project graph.
//!
//! Two independent halves, kept apart so each is testable on its own:
//!
//! 1. [`parse_stack_trace`] turns text into frames. Pure, no graph, no I/O.
//! 2. [`resolve_stack_trace`] asks the module graph which definitions each
//!    frame's identifier names.
//!
//! The graph is asked ONLY about frames whose file resolved to project source.
//! Everything else keeps its place in the reported array with its origin
//! recorded, so the reported frame numbering matches the trace as pasted and
//! the counts close.
//!
//! No source-map layer. A frame pointing into a build artifact reports that
//! fact rather than being rebound through a map that may be stale, because a
//! confidently wrong line is worse than a named refusal.

use std::path::{Path, PathBuf};

use fallow_types::discover::FileId;
use fallow_types::extract::MemberKind;
use fallow_types::trace_error::{
    ErrorTrace, ErrorTraceCandidate, ErrorTraceCounts, ErrorTraceFrame, ErrorTraceSchemaVersion,
    FrameOrigin, FrameResolution,
};
use rustc_hash::FxHashMap;

use crate::graph::ModuleGraph;
use crate::module_graph::RetainedModuleGraph;
use crate::trace::trace_impl::{matching_module_indexes, relativize};

/// Largest stack trace this verb will read, from a file or from stdin.
///
/// A stack trace is a handful of kilobytes. The cap exists so a redirected log
/// file cannot turn a bounded question into an unbounded one.
pub const MAX_STACK_TRACE_BYTES: u64 = 1024 * 1024;

/// Largest number of frames reported. Runtimes cap their own traces well below
/// this (V8's default is 10), so reaching it means the input is a log rather
/// than a trace. Frames past the cap are counted in `frames_omitted`.
const MAX_REPORTED_FRAMES: usize = 256;

/// Largest number of definitions listed for one ambiguous frame. The true match
/// count stays visible through `candidates_omitted`, so capping the list never
/// makes a frame look less ambiguous than it is.
const MAX_FRAME_CANDIDATES: usize = 10;

/// Path segments whose presence means the frame points at generated bundle
/// output rather than at source. Resolving those needs a source map, which this
/// verb deliberately does not do.
const BUILD_OUTPUT_SEGMENTS: &[&str] = &["dist", "build", "out", ".next"];

/// The installed dependency tree's directory name.
const DEPENDENCY_SEGMENT: &str = "node_modules";

/// One frame as read from the input, before the graph is consulted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawFrame {
    /// The input line, trimmed and otherwise verbatim.
    pub raw: String,
    /// The function identifier the runtime printed, with the `async` and `new`
    /// markers removed.
    pub function: Option<String>,
    /// Whether the runtime marked the frame as a constructor call.
    pub is_constructor: bool,
    /// Whether the runtime marked the frame as an async call.
    pub is_async: bool,
    /// The frame's file, unwrapped from any URL scheme and forward-slashed.
    pub file: Option<String>,
    /// 1-based line, when the runtime supplied one.
    pub line: Option<u32>,
    /// 1-based column, when the runtime supplied one.
    pub column: Option<u32>,
}

/// A stack trace after parsing and before resolution.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ParsedStackTrace {
    /// The first non-blank line preceding any frame, verbatim.
    pub header: Option<String>,
    /// Recognised frames, in input order.
    pub frames: Vec<RawFrame>,
    /// Non-blank lines that were neither a frame nor the header.
    pub unparsed_lines: usize,
}

/// Parse a runtime stack trace into frames.
///
/// Recognises the V8 / Node form (`    at name (file:line:col)`) and the
/// SpiderMonkey / JavaScriptCore form (`name@file:line:col`). A line matching
/// neither is counted, never silently dropped.
#[must_use]
pub fn parse_stack_trace(input: &str) -> ParsedStackTrace {
    let mut parsed = ParsedStackTrace::default();
    for line in input.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Some(frame) = parse_frame(trimmed) {
            parsed.frames.push(frame);
            continue;
        }
        if parsed.header.is_none() && parsed.frames.is_empty() {
            parsed.header = Some(trimmed.to_string());
            continue;
        }
        parsed.unparsed_lines += 1;
    }
    parsed
}

/// Recognise one line as a frame in either supported runtime form.
fn parse_frame(trimmed: &str) -> Option<RawFrame> {
    parse_v8_frame(trimmed).or_else(|| parse_at_sign_frame(trimmed))
}

/// V8 / Node: `at name (file:line:col)`, `at file:line:col`, `at name (native)`.
fn parse_v8_frame(trimmed: &str) -> Option<RawFrame> {
    let rest = trimmed.strip_prefix("at ")?.trim_start();
    // The location is parenthesised whenever the runtime printed a name, and a
    // function name may itself contain a parenthesis only in pathological
    // cases, so the LAST opening parenthesis of a parenthesised tail is the
    // separator.
    let (name_part, location_part) = match rest.strip_suffix(')') {
        Some(head) => match head.rfind(" (") {
            Some(index) => (&head[..index], &head[index + 2..]),
            None => ("", rest),
        },
        None => ("", rest),
    };

    let mut name = name_part.trim();
    let mut is_async = false;
    let mut is_constructor = false;
    if let Some(stripped) = name.strip_prefix("async ") {
        is_async = true;
        name = stripped.trim_start();
    }
    if let Some(stripped) = name.strip_prefix("new ") {
        is_constructor = true;
        name = stripped.trim_start();
    }

    let (file, line, column) = parse_location(location_part);
    Some(RawFrame {
        raw: trimmed.to_string(),
        function: named_function(name),
        is_constructor,
        is_async,
        file,
        line,
        column,
    })
}

/// SpiderMonkey / JavaScriptCore: `name@file:line:col`, `@file:line:col`.
///
/// A location without a line number is rejected rather than accepted, so an
/// ordinary line that happens to contain an `@` does not become a frame.
fn parse_at_sign_frame(trimmed: &str) -> Option<RawFrame> {
    let (name_part, location_part) = trimmed.rsplit_once('@')?;
    let (file, line, column) = parse_location(location_part);
    let file = file?;
    line?;
    Some(RawFrame {
        raw: trimmed.to_string(),
        function: named_function(name_part.trim()),
        is_constructor: false,
        is_async: false,
        file: Some(file),
        line,
        column,
    })
}

/// A frame's printed name, or `None` when the runtime printed a placeholder.
fn named_function(name: &str) -> Option<String> {
    if name.is_empty() || name == "<anonymous>" {
        return None;
    }
    Some(name.to_string())
}

/// Split a `file:line:col` location, peeling the numeric tail from the right so
/// a Windows drive letter and a URL scheme keep their own colons.
fn parse_location(location: &str) -> (Option<String>, Option<u32>, Option<u32>) {
    let location = location.trim();
    if location.is_empty() || location == "native" || location == "<anonymous>" {
        return (None, None, None);
    }
    let (head, column) = peel_number(location);
    let (head, line) = if column.is_some() {
        peel_number(head)
    } else {
        (head, None)
    };
    // Only a `file:line:col` tail yields both numbers. A `file:line` tail
    // leaves the single number in `column`, where it belongs on `line`.
    let (line, column) = match (line, column) {
        (Some(line), column) => (Some(line), column),
        (None, Some(single)) => (Some(single), None),
        (None, None) => (None, None),
    };
    (normalize_frame_path(head), line, column)
}

/// Split a trailing `:<digits>` off a location, when there is one.
fn peel_number(text: &str) -> (&str, Option<u32>) {
    let Some((head, tail)) = text.rsplit_once(':') else {
        return (text, None);
    };
    if tail.is_empty() || !tail.bytes().all(|byte| byte.is_ascii_digit()) {
        return (text, None);
    }
    match tail.parse::<u32>() {
        Ok(value) => (head, Some(value)),
        Err(_) => (text, None),
    }
}

/// Unwrap a frame path from its URL scheme and forward-slash it.
///
/// `file://` and `http(s)://` are unwrapped because the remainder is a real
/// path that can match a module. Every other scheme (`node:`, `webpack://`,
/// extension schemes) is left intact, because rewriting it would invent a path
/// the runtime never named.
fn normalize_frame_path(path: &str) -> Option<String> {
    let path = path.trim().replace('\\', "/");
    if path.is_empty() {
        return None;
    }
    if let Some(rest) = path.strip_prefix("file://") {
        // `file:///C:/src/a.ts` carries a leading slash before the drive
        // letter that is not part of the filesystem path.
        let rest = rest
            .strip_prefix('/')
            .filter(|tail| is_windows_drive_prefixed(tail))
            .unwrap_or(rest);
        return (!rest.is_empty()).then(|| rest.to_string());
    }
    for scheme in ["https://", "http://"] {
        if let Some(rest) = path.strip_prefix(scheme) {
            // Drop the authority; the path component is the only part that can
            // correspond to a file in the project.
            let authority_end = rest.find('/')?;
            let remainder = &rest[authority_end..];
            return (remainder.len() > 1).then(|| remainder.to_string());
        }
    }
    Some(path)
}

/// Whether a path begins with a `C:/`-style Windows drive prefix.
fn is_windows_drive_prefixed(path: &str) -> bool {
    let mut bytes = path.bytes();
    matches!(
        (bytes.next(), bytes.next(), bytes.next()),
        (Some(drive), Some(b':'), Some(b'/')) if drive.is_ascii_alphabetic()
    )
}

/// Whether a normalized path contains `segment` as a whole path component.
fn has_path_segment(path: &str, segment: &str) -> bool {
    path.split('/').any(|component| component == segment)
}

/// Whether a frame path names a runtime internal rather than a project file.
fn is_runtime_internal(path: &str) -> bool {
    path.starts_with("node:") || path.starts_with("internal/")
}

/// A frame's printed name, split into the parts a graph lookup addresses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Identifier<'a> {
    /// The name exactly as the runtime printed it, for diagnostics.
    printed: &'a str,
    /// The segment before the last dot, when there is one. V8 prints a
    /// receiver here (`Task` in `Task.run`, or the synthetic `Object`).
    owner: Option<&'a str>,
    /// The last segment: the function or member the frame is executing.
    name: &'a str,
}

/// Split a printed frame name into an optional owner and the member or function
/// name it addresses.
///
/// Returns `None` for a name that is not an addressable identifier, such as
/// `Object.<anonymous>` or a SpiderMonkey `outer/<` closure marker. Those are
/// reported as frames and are simply never looked up.
fn identifier_parts(function: &str) -> Option<Identifier<'_>> {
    if function.is_empty()
        || function
            .chars()
            .any(|c| c.is_whitespace() || c == '<' || c == '>' || c == '/' || c == '\\')
    {
        return None;
    }
    let mut segments = function.split('.');
    let mut owner = None;
    let mut name = segments.next()?;
    if name.is_empty() {
        return None;
    }
    for segment in segments {
        if segment.is_empty() {
            return None;
        }
        owner = Some(name);
        name = segment;
    }
    Some(Identifier {
        printed: function,
        owner,
        name,
    })
}

/// Stable wire token for a member kind, matching `--trace FILE:MEMBER`.
const fn member_kind_label(kind: MemberKind) -> &'static str {
    match kind {
        MemberKind::ClassMethod => "class-method",
        MemberKind::ClassProperty => "class-property",
        MemberKind::EnumMember => "enum-member",
        MemberKind::StoreMember => "store-member",
        MemberKind::NamespaceMember => "namespace-member",
    }
}

/// A candidate before its declaration line has been resolved.
struct LocatedCandidate {
    file_id: FileId,
    file: String,
    symbol: String,
    member: Option<String>,
    kind: &'static str,
    span_start: u32,
}

/// Per-run state every frame's resolution shares.
///
/// The two caches exist because a stack trace repeats itself: a recursive or
/// deeply nested trace names the same file in frame after frame, and resolving
/// or reading that file once per frame would turn a bounded question into a
/// syscall per frame.
struct TraceContext<'a> {
    /// The project root as the caller spelled it.
    root: &'a Path,
    /// The project root with symlinks resolved, when it could be read.
    canonical_root: Option<PathBuf>,
    /// An absolute frame path mapped to its project-root-relative spelling,
    /// or to `None` when it resolves to nothing inside the project.
    resolved_paths: FxHashMap<String, Option<String>>,
    /// A module's line offsets, or `None` when the file could not be read.
    line_offsets: FxHashMap<FileId, Option<Vec<u32>>>,
}

impl<'a> TraceContext<'a> {
    fn new(root: &'a Path) -> Self {
        Self {
            root,
            canonical_root: dunce::canonicalize(root).ok(),
            resolved_paths: FxHashMap::default(),
            line_offsets: FxHashMap::default(),
        }
    }

    /// The project-root-relative spelling of a frame's file, or `None` when
    /// the frame path is relative, unreadable, or outside the project root.
    ///
    /// This verb is the one command whose input is written by a machine rather
    /// than typed by a human: the runtime prints the absolute path ITS process
    /// saw. A project reached through a symlink (macOS `/tmp`, a checkout
    /// linked into place) therefore prints a prefix no module path carries,
    /// and comparing that spelling against the canonicalized paths discovery
    /// stored reports a real project file as out of corpus. Resolving both
    /// sides once turns it back into the ordinary root-relative comparison.
    ///
    /// A path that does not exist is NOT rewritten: it resolves to `None` and
    /// the frame is compared as the runtime spelled it, so a missing file can
    /// never be turned into a match on something else.
    fn root_relative(&mut self, path: &str) -> Option<String> {
        if !Path::new(path).is_absolute() {
            return None;
        }
        if let Some(cached) = self.resolved_paths.get(path) {
            return cached.clone();
        }
        let resolved = dunce::canonicalize(path).ok().and_then(|canonical| {
            let root = self.canonical_root.as_deref().unwrap_or(self.root);
            let relative = canonical.strip_prefix(root).ok()?;
            Some(relative.to_string_lossy().replace('\\', "/"))
        });
        self.resolved_paths
            .insert(path.to_string(), resolved.clone());
        resolved
    }
}

/// Resolve a parsed stack trace against the module graph.
#[must_use]
pub fn resolve_stack_trace(
    graph: &RetainedModuleGraph,
    root: &Path,
    parsed: ParsedStackTrace,
    source: String,
) -> ErrorTrace {
    resolve_with_graph(graph.as_graph(), root, parsed, source)
}

fn resolve_with_graph(
    graph: &ModuleGraph,
    root: &Path,
    parsed: ParsedStackTrace,
    source: String,
) -> ErrorTrace {
    let ParsedStackTrace {
        header,
        frames,
        unparsed_lines,
    } = parsed;

    let frames_omitted = frames.len().saturating_sub(MAX_REPORTED_FRAMES);
    let mut context = TraceContext::new(root);
    let mut counts = ErrorTraceCounts {
        frames_omitted,
        unparsed_lines,
        ..ErrorTraceCounts::default()
    };

    let resolved_frames: Vec<ErrorTraceFrame> = frames
        .into_iter()
        .take(MAX_REPORTED_FRAMES)
        .enumerate()
        .map(|(index, frame)| resolve_frame(graph, &mut context, index, frame))
        .collect();

    counts.frames = resolved_frames.len();
    for frame in &resolved_frames {
        match frame.origin {
            FrameOrigin::InProject => counts.in_project += 1,
            FrameOrigin::NodeModules => counts.node_modules += 1,
            FrameOrigin::OutOfCorpus => counts.out_of_corpus += 1,
        }
        match frame.resolution {
            FrameResolution::Resolved => counts.resolved += 1,
            FrameResolution::Ambiguous => counts.ambiguous += 1,
            FrameResolution::NotFound => counts.not_found += 1,
            FrameResolution::NotAttempted => counts.not_attempted += 1,
        }
    }

    let reason = summary_reason(&counts, header.is_some());

    ErrorTrace {
        schema_version: ErrorTraceSchemaVersion::V1,
        source,
        header,
        reason,
        frames: resolved_frames,
        counts,
    }
}

/// One sentence stating what the run answered and what it did not.
///
/// `has_header` is the count `counts` cannot carry: the first non-blank line of
/// a frameless input is taken as the error header and reported under `header`,
/// so it is deliberately absent from `unparsed_lines`. A sentence that claims to
/// describe the INPUT has to count it back in, or its number contradicts the
/// lines the caller pasted. The frames branch instead says `further`, because
/// there the header and the frames are both already reported above it.
fn summary_reason(counts: &ErrorTraceCounts, has_header: bool) -> String {
    use std::fmt::Write as _;

    if counts.frames == 0 {
        let input_lines = counts.unparsed_lines + usize::from(has_header);
        return if input_lines == 0 {
            "no stack frames in the input".to_string()
        } else {
            format!(
                "no stack frames recognised in {input_lines} non-blank input {}",
                plural(input_lines, "line", "lines")
            )
        };
    }
    let mut reason = format!(
        "{} {}: {} resolved, {} ambiguous, {} not found, {} not attempted",
        counts.frames,
        plural(counts.frames, "frame", "frames"),
        counts.resolved,
        counts.ambiguous,
        counts.not_found,
        counts.not_attempted
    );
    if counts.frames_omitted > 0 {
        let _ = write!(
            reason,
            " ({} further frames omitted)",
            counts.frames_omitted
        );
    }
    if counts.unparsed_lines > 0 {
        let _ = write!(
            reason,
            "; {} further input {} not recognised as a frame",
            counts.unparsed_lines,
            plural(counts.unparsed_lines, "line", "lines")
        );
    }
    reason
}

fn plural(count: usize, one: &'static str, many: &'static str) -> &'static str {
    if count == 1 { one } else { many }
}

/// What asking (or declining to ask) the graph produced for one frame.
struct FrameOutcome {
    resolution: FrameResolution,
    candidates: Vec<ErrorTraceCandidate>,
    candidates_omitted: usize,
    line_mismatch: bool,
    reason: String,
}

impl FrameOutcome {
    /// The graph was not consulted, and `reason` says why.
    fn not_attempted(reason: String) -> Self {
        Self {
            resolution: FrameResolution::NotAttempted,
            candidates: Vec::new(),
            candidates_omitted: 0,
            line_mismatch: false,
            reason,
        }
    }
}

/// Classify one frame and, when it points at project source with an
/// addressable identifier, ask the graph what that identifier names.
fn resolve_frame(
    graph: &ModuleGraph,
    context: &mut TraceContext<'_>,
    index: usize,
    frame: RawFrame,
) -> ErrorTraceFrame {
    let (origin, outcome) = classify_and_look_up(graph, context, &frame);
    let RawFrame {
        raw,
        function,
        is_constructor,
        is_async,
        file,
        line,
        column,
    } = frame;
    ErrorTraceFrame {
        index,
        raw,
        function,
        is_constructor,
        is_async,
        file,
        line,
        column,
        origin,
        resolution: outcome.resolution,
        candidates: outcome.candidates,
        candidates_omitted: outcome.candidates_omitted,
        line_mismatch: outcome.line_mismatch,
        reason: outcome.reason,
    }
}

/// The four gates a frame passes before the graph is asked anything, in order:
/// it must carry a location, that location must not be a dependency, it must
/// match a module, and it must carry an addressable identifier. Failing any one
/// of them is `not_attempted` with the gate named, never `not_found`.
fn classify_and_look_up(
    graph: &ModuleGraph,
    context: &mut TraceContext<'_>,
    frame: &RawFrame,
) -> (FrameOrigin, FrameOutcome) {
    let Some(path) = frame.file.as_deref() else {
        return (
            FrameOrigin::OutOfCorpus,
            FrameOutcome::not_attempted("frame carries no source location".to_string()),
        );
    };

    if has_path_segment(path, DEPENDENCY_SEGMENT) {
        return (
            FrameOrigin::NodeModules,
            FrameOutcome::not_attempted(
                "frame is in an installed dependency, not in project source".to_string(),
            ),
        );
    }

    // An absolute frame path is resolved to its root-relative spelling FIRST,
    // and only once, so a symlinked project root does not make every frame
    // look out of corpus and so the resolution costs one stat rather than one
    // per module.
    let resolved = context.root_relative(path);
    let match_path = resolved.as_deref().unwrap_or(path);
    let root = context.root;
    let module_indexes = matching_module_indexes(graph, root, match_path);
    if module_indexes.is_empty() {
        return (
            FrameOrigin::OutOfCorpus,
            FrameOutcome::not_attempted(out_of_corpus_reason(path)),
        );
    }

    let Some(name) = frame.function.as_deref() else {
        return (
            FrameOrigin::InProject,
            FrameOutcome::not_attempted(
                "frame carries no function identifier to look up".to_string(),
            ),
        );
    };

    let Some(identifier) = identifier_parts(name) else {
        return (
            FrameOrigin::InProject,
            FrameOutcome::not_attempted(format!("'{name}' is not an addressable identifier")),
        );
    };

    (
        FrameOrigin::InProject,
        look_up(graph, context, &module_indexes, identifier, frame.line),
    )
}

/// Why a frame that matched no module is out of corpus. The generated-output
/// case is named specifically, because "not in the project" would read as a
/// missing file when the real answer is "this is the compiled form of a file
/// that IS in the project, and reading it back needs a source map".
fn out_of_corpus_reason(path: &str) -> String {
    if is_runtime_internal(path) {
        return format!("'{path}' is a runtime internal, not project source");
    }
    if BUILD_OUTPUT_SEGMENTS
        .iter()
        .any(|segment| has_path_segment(path, segment))
    {
        return format!(
            "'{path}' is generated build output; resolving it back to source needs a source map, which this command does not read"
        );
    }
    format!("'{path}' is not a module in the analysed project")
}

/// Ask the graph which definitions an identifier names, and turn the answer
/// into a frame outcome without preferring any one match.
fn look_up(
    graph: &ModuleGraph,
    context: &mut TraceContext<'_>,
    module_indexes: &[usize],
    identifier: Identifier<'_>,
    frame_line: Option<u32>,
) -> FrameOutcome {
    let name = identifier.printed;
    let located = collect_candidates(
        graph,
        context.root,
        module_indexes,
        identifier.owner,
        identifier.name,
    );
    let total = located.len();
    let candidates_omitted = total.saturating_sub(MAX_FRAME_CANDIDATES);
    let single_file_id = located.first().map(|candidate| candidate.file_id);
    let candidates: Vec<ErrorTraceCandidate> = located
        .into_iter()
        .take(MAX_FRAME_CANDIDATES)
        .map(|candidate| resolve_candidate_line(graph, candidate, &mut context.line_offsets))
        .collect();

    let mut line_mismatch = false;
    let (resolution, mut reason) = match total {
        0 => (
            FrameResolution::NotFound,
            format!(
                "no definition named '{name}' is exported from the module this frame points at; a module-local function is not in the graph's definition set"
            ),
        ),
        1 => {
            let hit = &candidates[0];
            let target = hit.member.as_ref().map_or_else(
                || format!("{}:{}", hit.file, hit.symbol),
                |member| format!("{}:{}.{member}", hit.file, hit.symbol),
            );
            (
                FrameResolution::Resolved,
                format!("'{name}' names {target} ({})", hit.kind),
            )
        }
        _ => (
            FrameResolution::Ambiguous,
            format!("'{name}' names {total} definitions; none is preferred"),
        ),
    };

    // The look-up matches on the identifier alone, so a single match is
    // reported as `resolved` however far the frame's line sits from it. The
    // frame's line is checked against the definitions of the file it points
    // at, and a disagreement is stated rather than silently carried.
    if resolution == FrameResolution::Resolved
        && let Some(file_id) = single_file_id
        && let Some(note) = line_mismatch_note(graph, context, file_id, &candidates[0], frame_line)
    {
        line_mismatch = true;
        reason.push_str(&note);
    }

    FrameOutcome {
        resolution,
        candidates,
        candidates_omitted,
        line_mismatch,
        reason,
    }
}

/// Why the frame's own line disagrees with the definition its identifier
/// matched, or `None` when the two agree or the comparison cannot be made.
///
/// The check is the cheapest one that answers the question a reader would ask
/// next: which definition in this file is declared closest above the line the
/// runtime reported? When that is the matched definition, the frame's line and
/// the match tell the same story. When it is a DIFFERENT definition, the
/// runtime was executing past a declaration the match does not cover, so the
/// match is more likely a same-named definition elsewhere in the file. The
/// frame stays `resolved`, because the graph's answer to the question asked is
/// still correct; only the caller can decide what to do with the disagreement.
fn line_mismatch_note(
    graph: &ModuleGraph,
    context: &mut TraceContext<'_>,
    file_id: FileId,
    candidate: &ErrorTraceCandidate,
    frame_line: Option<u32>,
) -> Option<String> {
    let frame_line = frame_line?;
    let candidate_line = candidate.line?;
    let nearest = nearest_declaration_at_or_above(graph, context, file_id, frame_line);
    if nearest
        .as_ref()
        .is_some_and(|(name, line)| *line == candidate_line && name == &candidate_label(candidate))
    {
        return None;
    }
    let matched = candidate_label(candidate);
    Some(match nearest {
        Some((name, line)) => format!(
            "; the frame's line {frame_line} sits after the declaration of '{name}' at line {line}, not after '{matched}' at line {candidate_line}, so verify this is the definition that ran"
        ),
        None => format!(
            "; the frame's line {frame_line} is above every definition this file declares, including '{matched}' at line {candidate_line}, so verify this is the definition that ran"
        ),
    })
}

/// The printed name of a candidate, matching how the frame's identifier reads.
fn candidate_label(candidate: &ErrorTraceCandidate) -> String {
    candidate.member.as_ref().map_or_else(
        || candidate.symbol.clone(),
        |member| format!("{}.{member}", candidate.symbol),
    )
}

/// The definition declared closest at or above `line` in a module, as its
/// printed name and 1-based declaration line.
///
/// Exports and their members are the graph's whole definition set, so this
/// answers from what the look-up already consulted, using the line offsets the
/// candidate resolution cached.
fn nearest_declaration_at_or_above(
    graph: &ModuleGraph,
    context: &mut TraceContext<'_>,
    file_id: FileId,
    line: u32,
) -> Option<(String, u32)> {
    let module = graph.modules.get(file_id.0 as usize)?;
    let offsets = read_line_offsets(graph, file_id, &mut context.line_offsets)?;
    let declaration_line =
        |start: u32| fallow_types::extract::byte_offset_to_line_col(offsets, start).0;
    // The winner is tracked by position so a losing declaration never pays for
    // a formatted name.
    let mut nearest: Option<(usize, Option<usize>, u32)> = None;
    let mut consider = |export_index: usize, member_index: Option<usize>, declared: u32| {
        if declared <= line && nearest.is_none_or(|(_, _, best)| declared > best) {
            nearest = Some((export_index, member_index, declared));
        }
    };
    for (export_index, export) in module.exports.iter().enumerate() {
        consider(export_index, None, declaration_line(export.span.start));
        for (member_index, member) in export.members.iter().enumerate() {
            consider(
                export_index,
                Some(member_index),
                declaration_line(member.span.start),
            );
        }
    }

    let (export_index, member_index, declared) = nearest?;
    let export = module.exports.get(export_index)?;
    let name = match member_index.and_then(|index| export.members.get(index)) {
        Some(member) => format!("{}.{}", export.name, member.name),
        None => export.name.to_string(),
    };
    Some((name, declared))
}

/// A module's line offsets, computed at most once per trace.
fn read_line_offsets<'a>(
    graph: &ModuleGraph,
    file_id: FileId,
    line_offsets: &'a mut FxHashMap<FileId, Option<Vec<u32>>>,
) -> Option<&'a Vec<u32>> {
    line_offsets
        .entry(file_id)
        .or_insert_with(|| {
            graph
                .modules
                .get(file_id.0 as usize)
                .and_then(|module| std::fs::read_to_string(&module.path).ok())
                .map(|source| fallow_types::extract::compute_line_offsets(&source))
        })
        .as_ref()
}

/// Every definition the frame's identifier could name, deterministically
/// ordered and deduplicated.
///
/// Two rules run, and both are reported when both hit. An export whose name
/// equals the frame's last segment is a candidate because V8 prints a synthetic
/// receiver (`Object.run`) for a plain function; a member of an export named by
/// the second-to-last segment is a candidate because that is what `Task.run`
/// literally reads as. Choosing between them is the caller's judgment, not this
/// command's.
fn collect_candidates(
    graph: &ModuleGraph,
    root: &Path,
    module_indexes: &[usize],
    owner: Option<&str>,
    name: &str,
) -> Vec<LocatedCandidate> {
    let mut candidates: Vec<LocatedCandidate> = Vec::new();
    for &index in module_indexes {
        let Some(module) = graph.modules.get(index) else {
            continue;
        };
        let file = relativize(&module.path, root);
        for export in &module.exports {
            if export.name.matches_str(name) {
                candidates.push(LocatedCandidate {
                    file_id: module.file_id,
                    file: file.clone(),
                    symbol: export.name.to_string(),
                    member: None,
                    kind: "export",
                    span_start: export.span.start,
                });
            }
            if owner.is_some_and(|owner| export.name.matches_str(owner)) {
                for member in &export.members {
                    if member.name == name {
                        candidates.push(LocatedCandidate {
                            file_id: module.file_id,
                            file: file.clone(),
                            symbol: export.name.to_string(),
                            member: Some(member.name.clone()),
                            kind: member_kind_label(member.kind),
                            span_start: member.span.start,
                        });
                    }
                }
            }
        }
    }
    candidates.sort_by(|left, right| {
        left.file
            .cmp(&right.file)
            .then_with(|| left.symbol.cmp(&right.symbol))
            .then_with(|| left.member.cmp(&right.member))
    });
    candidates.dedup_by(|left, right| {
        left.file == right.file && left.symbol == right.symbol && left.member == right.member
    });
    candidates
}

/// Turn a candidate's declaration span into a 1-based line, reading each source
/// file at most once per trace. A file that cannot be read yields no line
/// rather than a guessed one.
fn resolve_candidate_line(
    graph: &ModuleGraph,
    candidate: LocatedCandidate,
    line_offsets: &mut FxHashMap<FileId, Option<Vec<u32>>>,
) -> ErrorTraceCandidate {
    let line = read_line_offsets(graph, candidate.file_id, line_offsets).map(|offsets| {
        fallow_types::extract::byte_offset_to_line_col(offsets, candidate.span_start).0
    });
    ErrorTraceCandidate {
        file: candidate.file,
        symbol: candidate.symbol,
        member: candidate.member,
        kind: candidate.kind.to_string(),
        line,
    }
}

/// Resolve a stack trace through an existing analysis session.
///
/// # Errors
///
/// Returns an error if parsing or graph construction fails.
pub fn trace_error_with_session(
    session: &crate::session::AnalysisSession,
    input: &str,
    source: String,
) -> crate::EngineResult<ErrorTrace> {
    let output = session.analyze_dead_code_with_shared_artifacts(false, true)?;
    let graph = output
        .graph
        .as_ref()
        .ok_or_else(|| crate::EngineError::new("trace-error requires a retained module graph"))?;
    Ok(resolve_stack_trace(
        graph,
        session.root(),
        parse_stack_trace(input),
        source,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frames(input: &str) -> Vec<RawFrame> {
        parse_stack_trace(input).frames
    }

    #[test]
    fn reads_the_v8_frame_forms() {
        let parsed = parse_stack_trace(
            "TypeError: user.load is not a function\n\
             \x20   at loadUser (/proj/src/services/user.ts:42:11)\n\
             \x20   at async UserService.load (/proj/src/user.ts:10:3)\n\
             \x20   at new Widget (/proj/src/widget.tsx:5:9)\n\
             \x20   at /proj/src/bare.ts:7:2\n",
        );

        assert_eq!(
            parsed.header.as_deref(),
            Some("TypeError: user.load is not a function")
        );
        assert_eq!(parsed.unparsed_lines, 0);
        assert_eq!(parsed.frames.len(), 4);

        assert_eq!(parsed.frames[0].function.as_deref(), Some("loadUser"));
        assert_eq!(
            parsed.frames[0].file.as_deref(),
            Some("/proj/src/services/user.ts")
        );
        assert_eq!(parsed.frames[0].line, Some(42));
        assert_eq!(parsed.frames[0].column, Some(11));

        assert_eq!(
            parsed.frames[1].function.as_deref(),
            Some("UserService.load")
        );
        assert!(parsed.frames[1].is_async);
        assert!(!parsed.frames[1].is_constructor);

        assert_eq!(parsed.frames[2].function.as_deref(), Some("Widget"));
        assert!(parsed.frames[2].is_constructor);

        assert_eq!(parsed.frames[3].function, None);
        assert_eq!(parsed.frames[3].file.as_deref(), Some("/proj/src/bare.ts"));
        assert_eq!(parsed.frames[3].line, Some(7));
    }

    #[test]
    fn reads_the_at_sign_frame_form() {
        let parsed = parse_stack_trace(
            "loadUser@/proj/src/user.ts:42:11\n@/proj/src/index.ts:3:1\nboot@https://app.test/assets/main.js:1:20\n",
        );

        assert_eq!(parsed.frames.len(), 3);
        assert_eq!(parsed.frames[0].function.as_deref(), Some("loadUser"));
        assert_eq!(parsed.frames[0].file.as_deref(), Some("/proj/src/user.ts"));
        assert_eq!(parsed.frames[0].line, Some(42));
        assert_eq!(parsed.frames[1].function, None);
        assert_eq!(
            parsed.frames[2].file.as_deref(),
            Some("/assets/main.js"),
            "an http location keeps its path component and drops the authority"
        );
    }

    #[test]
    fn a_line_with_a_stray_at_sign_is_not_a_frame() {
        let parsed = parse_stack_trace("reported by dev@example.com\n");

        assert_eq!(parsed.frames, [] as [crate::trace_error::RawFrame; 0]);
        assert_eq!(
            parsed.header.as_deref(),
            Some("reported by dev@example.com")
        );
        assert_eq!(parsed.unparsed_lines, 0);
    }

    #[test]
    fn unrecognised_lines_are_counted_never_dropped() {
        let parsed = parse_stack_trace("Error: boom\nnot a frame\nalso not a frame\n");

        assert_eq!(parsed.frames, [] as [crate::trace_error::RawFrame; 0]);
        assert_eq!(parsed.header.as_deref(), Some("Error: boom"));
        assert_eq!(parsed.unparsed_lines, 2);
    }

    #[test]
    fn an_empty_input_parses_to_nothing() {
        let parsed = parse_stack_trace("");

        assert_eq!(parsed, ParsedStackTrace::default());
    }

    #[test]
    fn the_frameless_reason_counts_the_header_line_it_describes() {
        // Three non-blank lines in, none of them a frame: the first is taken as
        // the header and reported separately, so `unparsed_lines` is 2. A
        // sentence about the INPUT must still say three, or its number
        // contradicts what the caller pasted.
        let parsed = parse_stack_trace("something went wrong\nsee the logs\nand the dashboard\n");
        assert_eq!(parsed.unparsed_lines, 2);
        let counts = ErrorTraceCounts {
            unparsed_lines: parsed.unparsed_lines,
            ..ErrorTraceCounts::default()
        };

        assert_eq!(
            summary_reason(&counts, parsed.header.is_some()),
            "no stack frames recognised in 3 non-blank input lines"
        );
    }

    #[test]
    fn a_single_unrecognised_line_is_reported_as_one_input_line() {
        let parsed = parse_stack_trace("something went wrong\n");
        assert_eq!(parsed.unparsed_lines, 0);
        let counts = ErrorTraceCounts {
            unparsed_lines: parsed.unparsed_lines,
            ..ErrorTraceCounts::default()
        };

        assert_eq!(
            summary_reason(&counts, parsed.header.is_some()),
            "no stack frames recognised in 1 non-blank input line"
        );
    }

    #[test]
    fn an_input_with_no_lines_at_all_still_reads_as_empty() {
        let counts = ErrorTraceCounts::default();

        assert_eq!(
            summary_reason(&counts, false),
            "no stack frames in the input"
        );
    }

    #[test]
    fn the_frame_reason_marks_unparsed_lines_as_further_than_what_it_listed() {
        // With frames present the header and the frames are already reported
        // above the sentence, so the trailing count is explicitly the remainder
        // rather than a second claim about the whole input.
        let counts = ErrorTraceCounts {
            frames: 1,
            resolved: 1,
            unparsed_lines: 2,
            ..ErrorTraceCounts::default()
        };

        assert_eq!(
            summary_reason(&counts, true),
            "1 frame: 1 resolved, 0 ambiguous, 0 not found, 0 not attempted; \
             2 further input lines not recognised as a frame"
        );
    }

    #[test]
    fn a_native_or_anonymous_location_carries_no_path() {
        let parsed = frames("    at doThing (native)\n    at Array.forEach (<anonymous>)\n");

        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].file, None);
        assert_eq!(parsed[0].line, None);
        assert_eq!(parsed[1].file, None);
    }

    #[test]
    fn a_file_url_keeps_a_windows_drive_letter() {
        let parsed = frames(
            "    at run (file:///C:/proj/src/a.ts:3:4)\n    at run (file:///proj/src/b.ts:5:6)\n",
        );

        assert_eq!(parsed[0].file.as_deref(), Some("C:/proj/src/a.ts"));
        assert_eq!(parsed[0].line, Some(3));
        assert_eq!(parsed[1].file.as_deref(), Some("/proj/src/b.ts"));
    }

    #[test]
    fn a_backslash_windows_path_is_forward_slashed_and_keeps_its_numbers() {
        let parsed = frames("    at run (C:\\proj\\src\\a.ts:12:4)\n");

        assert_eq!(parsed[0].file.as_deref(), Some("C:/proj/src/a.ts"));
        assert_eq!(parsed[0].line, Some(12));
        assert_eq!(parsed[0].column, Some(4));
    }

    #[test]
    fn a_node_internal_frame_keeps_its_scheme() {
        let parsed = frames(
            "    at process.processTicksAndRejections (node:internal/process/task_queues:95:5)\n",
        );

        assert_eq!(
            parsed[0].file.as_deref(),
            Some("node:internal/process/task_queues")
        );
        assert!(is_runtime_internal("node:internal/process/task_queues"));
    }

    #[test]
    fn a_location_without_a_column_puts_its_number_on_the_line() {
        let parsed = frames("    at run (/proj/src/a.ts:12)\n");

        assert_eq!(parsed[0].file.as_deref(), Some("/proj/src/a.ts"));
        assert_eq!(parsed[0].line, Some(12));
        assert_eq!(parsed[0].column, None);
    }

    #[test]
    fn identifier_parts_split_on_the_last_dot() {
        let parts = |name| identifier_parts(name).map(|id| (id.owner, id.name));
        assert_eq!(parts("run"), Some((None, "run")));
        assert_eq!(parts("Task.run"), Some((Some("Task"), "run")));
        assert_eq!(parts("ns.Task.run"), Some((Some("Task"), "run")));
    }

    #[test]
    fn identifier_parts_reject_non_addressable_names() {
        for name in [
            "Object.<anonymous>",
            "outer/<",
            "global code",
            "Task..run",
            "",
        ] {
            assert_eq!(identifier_parts(name), None, "name was {name:?}");
        }
    }

    #[test]
    fn an_absolute_frame_path_resolves_through_a_symlinked_root() {
        let dir = tempfile::tempdir().expect("tempdir");
        let real = dir.path().join("real/src");
        std::fs::create_dir_all(&real).expect("project dirs");
        std::fs::write(real.join("index.ts"), "export const run = () => 0;\n").expect("source");
        let linked = dir.path().join("linked");
        #[cfg(unix)]
        std::os::unix::fs::symlink(dir.path().join("real"), &linked).expect("symlink");
        #[cfg(windows)]
        std::os::windows::fs::symlink_dir(dir.path().join("real"), &linked).expect("symlink");

        let root = dir.path().join("real");
        let mut context = TraceContext::new(&root);
        let frame = linked
            .join("src/index.ts")
            .to_string_lossy()
            .replace('\\', "/");

        assert_eq!(
            context.root_relative(&frame).as_deref(),
            Some("src/index.ts"),
            "a frame spelled through a symlink must resolve to the module's own \
             root-relative path"
        );
    }

    #[test]
    fn a_relative_or_missing_frame_path_is_never_rewritten() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut context = TraceContext::new(dir.path());

        assert_eq!(context.root_relative("src/index.ts"), None);
        assert_eq!(
            context
                .root_relative(&dir.path().join("gone.ts").to_string_lossy())
                .as_deref(),
            None,
            "a path that does not exist resolves to nothing rather than to \
             something else"
        );
    }

    #[test]
    fn path_segment_matching_is_component_wise() {
        assert!(has_path_segment(
            "/proj/node_modules/x/index.js",
            "node_modules"
        ));
        assert!(!has_path_segment(
            "/proj/my_node_modules_shim/a.js",
            "node_modules"
        ));
        assert!(has_path_segment("/proj/dist/main.js", "dist"));
        assert!(!has_path_segment("/proj/src/distance.ts", "dist"));
    }
}
