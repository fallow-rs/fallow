//! Mutant site discovery and mutation-switching instrumentation.
//!
//! The crate is pure: it reads one source text and returns the mutants and
//! the instrumented text. It does no IO, and it depends on no other Fallow
//! workspace crate. The engine joins the mutants to graph, diff and
//! complexity facts.
//!
//! The instrumented text is a splice over the original text. The crate never
//! reprints the AST, so the output keeps the language of the input (TypeScript
//! stays TypeScript, JSX stays JSX) and runner transforms see the original
//! syntax.
//!
//! Subsumption is a static rule: negate-condition gives no mutant for a test
//! that is a comparison or a logical expression, because the relational and
//! logical operator mutants of that test cover it.

mod collect;
mod emit;
mod id;
mod operator;

use serde::Serialize;

pub use operator::Operator;

/// The language of a source text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SourceKind {
    /// JavaScript. JSX syntax is accepted, and module or script mode is
    /// detected from the content.
    Js,
    /// JavaScript with JSX.
    Jsx,
    /// TypeScript without JSX.
    Ts,
    /// TypeScript with JSX.
    Tsx,
}

impl SourceKind {
    /// Selects the kind from the file extension of `path`.
    pub fn from_path(path: &str) -> Option<Self> {
        let extension = path.rsplit_once('.')?.1;
        match extension {
            "js" | "mjs" | "cjs" => Some(Self::Js),
            "jsx" => Some(Self::Jsx),
            "ts" | "mts" | "cts" => Some(Self::Ts),
            "tsx" => Some(Self::Tsx),
            _ => None,
        }
    }

    /// The Oxc source type for this kind.
    pub const fn source_type(self) -> oxc_span::SourceType {
        match self {
            Self::Js => oxc_span::SourceType::unambiguous().with_jsx(true),
            Self::Jsx => oxc_span::SourceType::jsx(),
            Self::Ts => oxc_span::SourceType::ts(),
            Self::Tsx => oxc_span::SourceType::tsx(),
        }
    }
}

/// Options for [`instrument`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MutationOptions {
    /// The operators that may produce mutants.
    pub operators: Vec<Operator>,
    /// Byte ranges in the original text. When set, a mutant is kept only when
    /// its span overlaps one of the ranges. `None` mutates the whole file.
    pub ranges: Option<Vec<(u32, u32)>>,
}

impl Default for MutationOptions {
    fn default() -> Self {
        Self {
            operators: Operator::ALL.to_vec(),
            ranges: None,
        }
    }
}

/// The stable identity of a mutant: 16 lowercase hex digits.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct MutantId(pub String);

impl MutantId {
    /// The id as text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for MutantId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// The function that contains a mutant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FunctionRef {
    /// The function name, when the syntax gives one.
    pub name: Option<String>,
    /// The original byte span of the function.
    pub span: (u32, u32),
}

/// One mutant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Mutant {
    /// The stable id.
    pub id: MutantId,
    /// The operator that produced the mutant.
    pub operator: Operator,
    /// The original byte span that the mutant replaces.
    pub span: (u32, u32),
    /// The 1-based line of the span start.
    pub line: u32,
    /// The 0-based byte column of the span start.
    pub column: u32,
    /// The original text of the span.
    pub original: String,
    /// The text that replaces the span. The original source with this
    /// replacement applied is the mutated program. At the start of an
    /// expression statement, the text can start with `void`, so that the
    /// statement does not join the line before it.
    pub replacement: String,
    /// The innermost function that contains the mutant.
    pub enclosing_function: Option<FunctionRef>,
}

/// One copied run of original text in the instrumented output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct SpliceSegment {
    /// The byte offset in the instrumented output.
    pub generated_start: u32,
    /// The byte offset in the original text.
    pub original_start: u32,
    /// The length in bytes.
    pub len: u32,
}

/// Maps instrumented byte offsets back to original byte offsets.
///
/// Each segment is a run of original text that the output copies unchanged.
/// Text that the instrumentation adds has no segment. The segments are in
/// increasing `generated_start` order.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct SpliceMap {
    /// The copied runs, in output order.
    pub segments: Vec<SpliceSegment>,
}

impl SpliceMap {
    /// The original offset of a generated offset, or `None` when the
    /// instrumentation added the byte.
    pub fn original_offset(&self, generated: u32) -> Option<u32> {
        let index = self
            .segments
            .partition_point(|segment| segment.generated_start <= generated);
        let segment = self.segments.get(index.checked_sub(1)?)?;
        let delta = generated - segment.generated_start;
        (delta < segment.len).then_some(segment.original_start + delta)
    }
}

/// The result of [`instrument`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Instrumented {
    /// The mutants, in source order.
    pub mutants: Vec<Mutant>,
    /// The instrumented source text.
    pub code: String,
    /// The map from output offsets to original offsets.
    pub map: SpliceMap,
}

/// An error from [`instrument`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MutationError {
    /// The source text does not parse.
    Parse {
        /// The parser messages.
        messages: Vec<String>,
    },
    /// The source text or the instrumented text is larger than `u32::MAX`
    /// bytes.
    TooLarge,
}

impl std::fmt::Display for MutationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Parse { messages } => {
                write!(f, "the source does not parse: {}", messages.join("; "))
            }
            Self::TooLarge => {
                f.write_str("the source or the instrumented output is larger than 4 GiB")
            }
        }
    }
}

impl std::error::Error for MutationError {}

/// True for a byte that can continue an identifier. Non-ASCII bytes count,
/// so that an edit never joins a Unicode identifier with a keyword.
const fn is_identifier_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'$') || byte >= 0x80
}

/// The prelude function that asks the runtime whether a mutant is active.
///
/// The instrumented output calls `__fallow_m_a(id)` before each mutated
/// branch. The function returns `globalThis.__fallowMutation.a(id)`, or
/// `false` when the runtime object is not set.
pub const ACTIVE_FUNCTION: &str = "__fallow_m_a";

/// The prelude function that reports a hit on the original branch.
///
/// The instrumented output calls `__fallow_m_c(id)` for each mutant of a site
/// before the original branch runs. The function calls
/// `globalThis.__fallowMutation.c(id)` when the runtime object is set.
pub const HIT_FUNCTION: &str = "__fallow_m_c";

/// Finds the mutants in `source` and returns the instrumented text.
///
/// `path` is part of each mutant id. Use the project-relative path, so that
/// ids are the same on each machine.
pub fn instrument(
    source: &str,
    path: &str,
    source_type: SourceKind,
    options: &MutationOptions,
) -> Result<Instrumented, MutationError> {
    if u32::try_from(source.len()).is_err() {
        return Err(MutationError::TooLarge);
    }
    let allocator = oxc_allocator::Allocator::default();
    let parsed = oxc_parser::Parser::new(&allocator, source, source_type.source_type()).parse();
    if parsed.fatal_error || parsed.diagnostics.has_errors() {
        let messages = parsed.diagnostics.iter().map(ToString::to_string).collect();
        return Err(MutationError::Parse { messages });
    }
    let collected = collect::collect(&parsed.program, source);
    let insertions = header_insertions(&parsed.program, source);
    let line_starts = line_starts(source);

    let mut mutants = Vec::new();
    let mut emit_sites = Vec::new();
    let mut seen_ids = std::collections::BTreeSet::new();
    for site in collected.sites {
        let kept = select_candidates(source, &site, &collected.statement_starts, options);
        if kept.is_empty() {
            continue;
        }
        let mut emit_mutants = Vec::with_capacity(kept.len());
        for candidate in kept {
            let id = unique_id(path, source, &site, &candidate, &mut seen_ids);
            let (line, column) = line_column(&line_starts, candidate.region.0);
            emit_mutants.push(emit::EmitMutant {
                id: id.0.clone(),
                region: candidate.region,
                replacement: candidate.replacement.clone(),
            });
            mutants.push(Mutant {
                id,
                operator: candidate.operator,
                span: candidate.region,
                line,
                column,
                original: slice(source, candidate.region).to_owned(),
                replacement: candidate.replacement,
                enclosing_function: site.function.clone(),
            });
        }
        emit_sites.push(emit::EmitSite {
            span: site.span,
            mutants: emit_mutants,
        });
    }
    mutants.sort_by_key(|mutant| mutant.span);
    let (code, map) = emit::emit(
        source,
        &emit_sites,
        &collected.statement_starts,
        &insertions,
    );
    // The splice map holds `u32` offsets. When the output fits, no offset
    // was truncated.
    if u32::try_from(code.len()).is_err() {
        return Err(MutationError::TooLarge);
    }
    Ok(Instrumented { mutants, code, map })
}

/// The prelude: two function declarations that call the runtime.
///
/// The prelude uses hoisted function declarations, not a `const`. In a module
/// cycle, another module can call a hoisted function of this module before
/// the prelude line runs. A `const` binding is then in its temporal dead
/// zone and throws a `ReferenceError`. A function declaration exists from
/// the start of the module, and each call reads the runtime object again.
/// Two classic scripts can also declare the same functions without a
/// redeclaration error.
const PRELUDE: &str = "function __fallow_m_a(id) { const m = globalThis.__fallowMutation; return m ? m.a(id) : false; } function __fallow_m_c(id) { const m = globalThis.__fallowMutation; if (m) m.c(id); }";

const TS_NOCHECK: &str = "// @ts-nocheck";

/// The `// @ts-nocheck` comment goes after the hashbang and the leading
/// comments, so that pragmas such as `@jsxImportSource` stay before the first
/// token. The prelude goes after the directive prologue, so that
/// `"use strict"` and `"use client"` stay the first statements.
///
/// A directive can share a line with the last leading comment. The line
/// comment then ends with a newline, or the directive and the prelude become
/// comment text.
fn header_insertions(program: &oxc_ast::ast::Program<'_>, source: &str) -> Vec<emit::Insertion> {
    let offset = leading_end(program, source);
    let line_break_follows = source
        .get(offset as usize..)
        .is_some_and(|rest| rest.starts_with(['\n', '\r', '\u{2028}', '\u{2029}']));
    let nocheck = if offset == 0 {
        format!("{TS_NOCHECK}\n")
    } else if program.directives.is_empty() || line_break_follows {
        format!("\n{TS_NOCHECK}")
    } else {
        format!("\n{TS_NOCHECK}\n")
    };
    let Some(last) = program.directives.last() else {
        let text = if offset == 0 {
            format!("{nocheck}{PRELUDE}\n")
        } else {
            format!("{nocheck}\n{PRELUDE}")
        };
        return vec![emit::Insertion { offset, text }];
    };
    let end = last.span.end;
    let terminated = source
        .get(..end as usize)
        .is_some_and(|text| text.ends_with(';'));
    let separator = if terminated { "" } else { ";" };
    vec![
        emit::Insertion {
            offset,
            text: nocheck,
        },
        emit::Insertion {
            offset: end,
            text: format!("{separator}{PRELUDE}"),
        },
    ]
}

/// The end of the hashbang and of the comments before the first token.
fn leading_end(program: &oxc_ast::ast::Program<'_>, source: &str) -> u32 {
    use oxc_span::GetSpan;
    let first_token = program
        .directives
        .first()
        .map(|directive| directive.span.start)
        .or_else(|| program.body.first().map(|statement| statement.span().start))
        .unwrap_or(source.len() as u32);
    let hashbang_end = program
        .hashbang
        .as_ref()
        .map_or(0, |hashbang| hashbang.span.end);
    program
        .comments
        .iter()
        .take_while(|comment| comment.span.end <= first_token)
        .map(|comment| comment.span.end)
        .last()
        .map_or(hashbang_end, |end| end.max(hashbang_end))
}

fn slice(source: &str, span: (u32, u32)) -> &str {
    source
        .get(span.0 as usize..span.1 as usize)
        .unwrap_or_default()
}

/// Applies the operator and range options, then drops candidates that give
/// the original text or the same text as an earlier candidate of the site.
fn select_candidates(
    source: &str,
    site: &collect::Site,
    statement_starts: &std::collections::BTreeSet<u32>,
    options: &MutationOptions,
) -> Vec<collect::Candidate> {
    let original = slice(source, site.span);
    let mut texts = std::collections::BTreeSet::new();
    let mut kept = Vec::new();
    for candidate in &site.candidates {
        if !options.operators.contains(&candidate.operator) {
            continue;
        }
        let (start, end) = candidate.region;
        let in_range = options
            .ranges
            .as_ref()
            .is_none_or(|ranges| ranges.iter().any(|&(from, to)| start < to && from < end));
        if !in_range {
            continue;
        }
        let replacement = guard_statement_start(source, statement_starts, candidate);
        let mutated = format!(
            "{}{}{}",
            slice(source, (site.span.0, start)),
            replacement,
            slice(source, (end, site.span.1)),
        );
        if mutated == original || !texts.insert(mutated) {
            continue;
        }
        kept.push(collect::Candidate {
            operator: candidate.operator,
            region: candidate.region,
            replacement,
        });
    }
    kept
}

/// Characters that continue the previous line when a statement starts with
/// them and that line has no semicolon.
const ASI_HAZARDS: [char; 6] = ['(', '[', '`', '+', '-', '/'];

/// True when a statement that starts with `text` can join the line before it.
/// A prefix `++` or `--` cannot, because the postfix forms forbid a line
/// break before the operator.
fn starts_with_asi_hazard(text: &str) -> bool {
    let hazard = text.starts_with(ASI_HAZARDS);
    hazard && !text.starts_with("++") && !text.starts_with("--")
}

/// Prefixes `void` to a replacement that starts an expression statement with
/// an ASI hazard, so that the original source with the replacement applied
/// keeps the statement boundary. When the original also starts with a hazard,
/// the line before it already ends the statement, and no prefix is needed.
fn guard_statement_start(
    source: &str,
    statement_starts: &std::collections::BTreeSet<u32>,
    candidate: &collect::Candidate,
) -> String {
    let (start, end) = candidate.region;
    let guarded = statement_starts.contains(&start)
        && starts_with_asi_hazard(&candidate.replacement)
        && !starts_with_asi_hazard(slice(source, (start, end)));
    if !guarded {
        return candidate.replacement.clone();
    }
    // `do!f()` has no space after the keyword; `dovoid` would be a name.
    let joins = source
        .as_bytes()
        .get((start as usize).wrapping_sub(1))
        .is_some_and(|&byte| is_identifier_byte(byte));
    let prefix = if joins { " void " } else { "void " };
    format!("{prefix}{}", candidate.replacement)
}

fn unique_id(
    path: &str,
    source: &str,
    site: &collect::Site,
    candidate: &collect::Candidate,
    seen: &mut std::collections::BTreeSet<MutantId>,
) -> MutantId {
    let anchor_start = site.anchor.0;
    let input = id::IdInput {
        path,
        anchor_text: slice(source, site.anchor),
        relative: (
            candidate.region.0.saturating_sub(anchor_start),
            candidate.region.1.saturating_sub(anchor_start),
        ),
        operator: candidate.operator,
        replacement: &candidate.replacement,
    };
    let mut occurrence = 0;
    loop {
        let id = id::mutant_id(&input, occurrence);
        if seen.insert(id.clone()) {
            return id;
        }
        occurrence += 1;
    }
}

fn line_starts(source: &str) -> Vec<u32> {
    std::iter::once(0)
        .chain(
            source
                .match_indices('\n')
                .map(|(index, _)| index as u32 + 1),
        )
        .collect()
}

fn line_column(line_starts: &[u32], offset: u32) -> (u32, u32) {
    let index = line_starts
        .partition_point(|&start| start <= offset)
        .saturating_sub(1);
    let start = line_starts.get(index).copied().unwrap_or(0);
    (index as u32 + 1, offset - start)
}
