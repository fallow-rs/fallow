//! Security sink substitution between two revisions of one source file.
//!
//! A lint rule names the sink that it flags. A change that replaces the
//! flagged sink with a different sink of the same class silences the rule and
//! keeps the risk: `new Function(code)` becomes `vm.runInNewContext(code)`,
//! `el.innerHTML = html` becomes `frame.srcdoc = html`. A change can also move
//! the sink out of the static view of a syntactic rule: into an object key
//! under a JSX spread, into a computed member, or into an alias.
//!
//! This module compares the non-literal sink sites of the base and the head
//! revision of one file. It works on the whole file, not on one function, so
//! a sink that moves to another function of the same file gives no signal.
//! The result is reviewer evidence only. It never proves a vulnerability.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::OnceLock;

use fallow_security::CalleePattern;
use fallow_types::discover::FileId;
use fallow_types::extract::SinkShape;
use rustc_hash::FxHashSet;

/// A family of sinks where one sink can stand in for another.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum SinkClass {
    CodeExecution,
    Html,
}

/// One sink that takes part in substitution detection.
struct SubstituteSink {
    class: SinkClass,
    shape: SinkShape,
    pattern: &'static str,
    /// The name that the evidence uses.
    label: &'static str,
    /// The identifier that names the sink in source text.
    token: &'static str,
}

const fn sink(
    class: SinkClass,
    shape: SinkShape,
    pattern: &'static str,
    label: &'static str,
    token: &'static str,
) -> SubstituteSink {
    SubstituteSink {
        class,
        shape,
        pattern,
        label,
        token,
    }
}

/// The sinks of each class. The table holds catalogue sinks and the related
/// sinks that a rule for a catalogue sink does not name, such as `srcdoc`.
const SINKS: &[SubstituteSink] = &[
    sink(
        SinkClass::CodeExecution,
        SinkShape::Call,
        "eval",
        "eval",
        "eval",
    ),
    sink(
        SinkClass::CodeExecution,
        SinkShape::NewExpression,
        "Function",
        "new Function",
        "Function",
    ),
    sink(
        SinkClass::CodeExecution,
        SinkShape::Call,
        "Function",
        "Function",
        "Function",
    ),
    sink(
        SinkClass::CodeExecution,
        SinkShape::MemberCall,
        "vm.runInNewContext",
        "vm.runInNewContext",
        "runInNewContext",
    ),
    sink(
        SinkClass::CodeExecution,
        SinkShape::MemberCall,
        "vm.runInThisContext",
        "vm.runInThisContext",
        "runInThisContext",
    ),
    sink(
        SinkClass::CodeExecution,
        SinkShape::MemberCall,
        "vm.runInContext",
        "vm.runInContext",
        "runInContext",
    ),
    sink(
        SinkClass::CodeExecution,
        SinkShape::MemberCall,
        "vm.compileFunction",
        "vm.compileFunction",
        "compileFunction",
    ),
    sink(
        SinkClass::CodeExecution,
        SinkShape::NewExpression,
        "vm.Script",
        "new vm.Script",
        "Script",
    ),
    sink(
        SinkClass::CodeExecution,
        SinkShape::Call,
        "runInNewContext",
        "runInNewContext",
        "runInNewContext",
    ),
    sink(
        SinkClass::CodeExecution,
        SinkShape::Call,
        "runInThisContext",
        "runInThisContext",
        "runInThisContext",
    ),
    sink(
        SinkClass::CodeExecution,
        SinkShape::Call,
        "runInContext",
        "runInContext",
        "runInContext",
    ),
    sink(
        SinkClass::CodeExecution,
        SinkShape::Call,
        "compileFunction",
        "compileFunction",
        "compileFunction",
    ),
    // The extractor resolves a named, default or namespace import of `vm`
    // and a `require("vm")` binding to the `node:vm.<member>` path.
    sink(
        SinkClass::CodeExecution,
        SinkShape::MemberCall,
        "node:vm.runInNewContext",
        "runInNewContext",
        "runInNewContext",
    ),
    sink(
        SinkClass::CodeExecution,
        SinkShape::MemberCall,
        "node:vm.runInThisContext",
        "runInThisContext",
        "runInThisContext",
    ),
    sink(
        SinkClass::CodeExecution,
        SinkShape::MemberCall,
        "node:vm.runInContext",
        "runInContext",
        "runInContext",
    ),
    sink(
        SinkClass::CodeExecution,
        SinkShape::MemberCall,
        "node:vm.compileFunction",
        "compileFunction",
        "compileFunction",
    ),
    sink(
        SinkClass::CodeExecution,
        SinkShape::NewExpression,
        "node:vm.Script",
        "new Script",
        "Script",
    ),
    sink(
        SinkClass::Html,
        SinkShape::MemberAssign,
        "*.innerHTML",
        "innerHTML",
        "innerHTML",
    ),
    sink(
        SinkClass::Html,
        SinkShape::MemberAssign,
        "*.outerHTML",
        "outerHTML",
        "outerHTML",
    ),
    sink(
        SinkClass::Html,
        SinkShape::MemberAssign,
        "*.srcdoc",
        "srcdoc",
        "srcdoc",
    ),
    sink(
        SinkClass::Html,
        SinkShape::MemberCall,
        "*.insertAdjacentHTML",
        "insertAdjacentHTML",
        "insertAdjacentHTML",
    ),
    sink(
        SinkClass::Html,
        SinkShape::MemberCall,
        "*.setHTMLUnsafe",
        "setHTMLUnsafe",
        "setHTMLUnsafe",
    ),
    sink(
        SinkClass::Html,
        SinkShape::MemberCall,
        "*.createContextualFragment",
        "createContextualFragment",
        "createContextualFragment",
    ),
    sink(
        SinkClass::Html,
        SinkShape::MemberCall,
        "document.write",
        "document.write",
        "write",
    ),
    sink(
        SinkClass::Html,
        SinkShape::MemberCall,
        "document.writeln",
        "document.writeln",
        "writeln",
    ),
    sink(
        SinkClass::Html,
        SinkShape::JsxAttr,
        "dangerouslySetInnerHTML",
        "dangerouslySetInnerHTML",
        "dangerouslySetInnerHTML",
    ),
    sink(
        SinkClass::Html,
        SinkShape::JsxAttr,
        "srcDoc",
        "srcDoc",
        "srcDoc",
    ),
    sink(
        SinkClass::Html,
        SinkShape::JsxAttr,
        "srcdoc",
        "srcdoc",
        "srcdoc",
    ),
];

/// Extensions of the files where the extractor captures sink sites.
const SOURCE_EXTENSIONS: &[&str] = &[
    "js", "jsx", "ts", "tsx", "mjs", "cjs", "mts", "cts", "vue", "svelte", "astro",
];

/// The parsed callee pattern of each [`SINKS`] row, in row order.
fn patterns() -> &'static [Option<CalleePattern>] {
    static PATTERNS: OnceLock<Vec<Option<CalleePattern>>> = OnceLock::new();
    PATTERNS.get_or_init(|| {
        SINKS
            .iter()
            .map(|row| CalleePattern::parse(row.pattern))
            .collect()
    })
}

/// Whether the file at `rel_path` is a source file where the extractor
/// captures sink sites.
#[must_use]
pub fn is_sink_source_path(rel_path: &str) -> bool {
    let name = rel_path.rsplit('/').next().unwrap_or(rel_path);
    name.rsplit_once('.')
        .is_some_and(|(_, ext)| SOURCE_EXTENSIONS.contains(&ext))
}

/// Number of non-literal sink sites per table row, keyed by class and label.
type SinkCounts = BTreeMap<(SinkClass, &'static str), usize>;

fn sink_counts(path: &Path, source: &str) -> SinkCounts {
    let module = fallow_extract::parse_source_to_module(FileId(0), path, source, 0, false);
    let patterns = patterns();
    let mut seen: FxHashSet<(usize, u32)> = FxHashSet::default();
    let mut counts = SinkCounts::new();
    for site in module
        .security_sinks
        .iter()
        .filter(|site| site.arg_is_non_literal)
    {
        let Some(row) = SINKS.iter().zip(patterns).position(|(row, pattern)| {
            row.shape == site.sink_shape
                && pattern
                    .as_ref()
                    .is_some_and(|pattern| pattern.matches(&site.callee_path))
        }) else {
            continue;
        };
        // A call with two non-literal arguments is one sink site.
        if seen.insert((row, site.span_start)) {
            *counts
                .entry((SINKS[row].class, SINKS[row].label))
                .or_default() += 1;
        }
    }
    counts
}

/// How a sink can leave the static view of a syntactic rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HiddenForm {
    ComputedMember,
    ObjectKey,
    Alias,
}

impl HiddenForm {
    const ALL: [Self; 3] = [Self::ObjectKey, Self::ComputedMember, Self::Alias];

    /// The index of the form in [`Self::ALL`].
    const fn slot(self) -> usize {
        match self {
            Self::ObjectKey => 0,
            Self::ComputedMember => 1,
            Self::Alias => 2,
        }
    }

    const fn phrase(self) -> &'static str {
        match self {
            Self::ComputedMember => "into a computed member",
            Self::ObjectKey => "into an object key",
            Self::Alias => "into an alias",
        }
    }
}

const fn is_ident_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'$'
}

fn prev_non_space(bytes: &[u8], end: usize) -> Option<(usize, u8)> {
    bytes[..end]
        .iter()
        .enumerate()
        .rev()
        .find(|(_, byte)| !byte.is_ascii_whitespace())
        .map(|(index, byte)| (index, *byte))
}

fn next_non_space(bytes: &[u8], start: usize) -> Option<u8> {
    next_non_space_at(bytes, start).map(|(_, byte)| byte)
}

fn next_non_space_at(bytes: &[u8], start: usize) -> Option<(usize, u8)> {
    bytes
        .get(start..)?
        .iter()
        .enumerate()
        .find(|(_, byte)| !byte.is_ascii_whitespace())
        .map(|(offset, byte)| (start + offset, *byte))
}

/// Whether the object key that ends at `end` is followed by `:` and a value
/// that is not a literal. A literal value such as `{ innerHTML: 1 }` is data
/// and never a sink payload.
fn keys_non_literal_value(bytes: &[u8], end: usize) -> bool {
    let Some((colon, b':')) = next_non_space_at(bytes, end) else {
        return false;
    };
    let Some((value_start, first)) = next_non_space_at(bytes, colon + 1) else {
        return false;
    };
    if first.is_ascii_digit() || matches!(first, b'\'' | b'"' | b'-' | b'+' | b'.') {
        return false;
    }
    let word_end = bytes[value_start..]
        .iter()
        .position(|byte| !is_ident_byte(*byte))
        .map_or(bytes.len(), |offset| value_start + offset);
    !matches!(
        &bytes[value_start..word_end],
        b"true" | b"false" | b"null" | b"undefined"
    )
}

/// The rest of the line after `start` ends the expression: an alias such as
/// `const run = eval;` has nothing after the name.
fn ends_expression(bytes: &[u8], start: usize) -> bool {
    bytes[start..]
        .iter()
        .find(|byte| **byte != b' ' && **byte != b'\t')
        .is_none_or(|byte| matches!(byte, b';' | b',' | b')' | b'\n' | b'\r'))
}

/// Classify one occurrence of a sink token. `property_sink` allows the object
/// key form: only a property sink (`innerHTML`, `srcDoc`) reaches the sink
/// through an object key, for example under `Object.assign` or a JSX spread.
fn classify_occurrence(
    bytes: &[u8],
    start: usize,
    end: usize,
    property_sink: bool,
) -> Option<HiddenForm> {
    let quote = start.checked_sub(1).map(|index| bytes[index]);
    if let Some(quote) = quote.filter(|byte| matches!(byte, b'\'' | b'"' | b'`')) {
        if bytes.get(end) != Some(&quote) {
            return None;
        }
        if prev_non_space(bytes, start - 1).is_some_and(|(_, byte)| byte == b'[') {
            return Some(HiddenForm::ComputedMember);
        }
        let opens_key =
            prev_non_space(bytes, start - 1).is_some_and(|(_, byte)| matches!(byte, b'{' | b','));
        return (property_sink && opens_key && keys_non_literal_value(bytes, end + 1))
            .then_some(HiddenForm::ObjectKey);
    }
    let (prev_index, prev) = prev_non_space(bytes, start)?;
    let next = next_non_space(bytes, end);
    if matches!(prev, b'{' | b',') && next == Some(b':') {
        return (property_sink && keys_non_literal_value(bytes, end))
            .then_some(HiddenForm::ObjectKey);
    }
    let plain_assign = prev == b'='
        && prev_index
            .checked_sub(1)
            .is_none_or(|index| !matches!(bytes[index], b'=' | b'!' | b'<' | b'>'));
    let indirect_call = prev == b',' && next == Some(b')');
    ((plain_assign && ends_expression(bytes, end)) || indirect_call).then_some(HiddenForm::Alias)
}

/// Count the occurrences of `token` in each hidden form.
fn hidden_form_counts(source: &str, token: &str, property_sink: bool) -> [usize; 3] {
    let bytes = source.as_bytes();
    let mut counts = [0; 3];
    for (start, _) in source.match_indices(token) {
        let end = start + token.len();
        let bounded = start
            .checked_sub(1)
            .is_none_or(|i| !is_ident_byte(bytes[i]))
            && bytes.get(end).is_none_or(|byte| !is_ident_byte(*byte));
        if !bounded {
            continue;
        }
        if let Some(form) = classify_occurrence(bytes, start, end, property_sink) {
            counts[form.slot()] += 1;
        }
    }
    counts
}

/// The source token of the sink with `label`, and whether the sink is a
/// property (an assignment target or a JSX attribute).
fn token_of(label: &str) -> Option<(&'static str, bool)> {
    SINKS.iter().find(|row| row.label == label).map(|row| {
        let property = matches!(row.shape, SinkShape::MemberAssign | SinkShape::JsxAttr);
        (row.token, property)
    })
}

fn plural(labels: &[&str]) -> &'static str {
    if labels.len() == 1 { "sink" } else { "sinks" }
}

/// Labels whose count is higher in `from` than in `to`, in label order.
fn grown(class: SinkClass, from: &SinkCounts, to: &SinkCounts) -> Vec<&'static str> {
    from.iter()
        .filter(|((row_class, label), count)| {
            *row_class == class && **count > to.get(&(class, *label)).copied().unwrap_or(0)
        })
        .map(|((_, label), _)| *label)
        .collect()
}

/// Detect sink substitutions between the base and the head revision of one
/// source file, and return one evidence string per signal.
///
/// - A class whose sinks lose one name and gain another gives
///   `<removed> sink replaced by <added>`.
/// - A base sink whose count does not grow, and whose name shows up in a new
///   object key, computed member or alias, gives `<sink> sink moved into <form>`.
///
/// A sink that only moves within the file keeps its name and gives nothing.
#[must_use]
pub fn detect_sink_substitutions(path: &Path, base: &str, head: &str) -> Vec<String> {
    // Every signal needs a sink name in the base (the removed sink) and a
    // sink name in the head (the added sink or the hidden form).
    let names_a_sink = |text: &str| SINKS.iter().any(|row| text.contains(row.token));
    if base == head || !names_a_sink(base) || !names_a_sink(head) {
        return Vec::new();
    }
    let base_counts = sink_counts(path, base);
    let head_counts = sink_counts(path, head);
    let mut evidence = Vec::new();
    for class in [SinkClass::CodeExecution, SinkClass::Html] {
        let removed = grown(class, &base_counts, &head_counts);
        let added = grown(class, &head_counts, &base_counts);
        if !removed.is_empty() && !added.is_empty() {
            evidence.push(format!(
                "{} {} replaced by {}",
                removed.join(", "),
                plural(&removed),
                added.join(", ")
            ));
            continue;
        }
        // The extractor resolves some hidden forms, for example a JSX spread
        // of an inline object or `(0, eval)(x)`. Such a move keeps the count
        // of the sink, so each base sink whose count does not grow is a
        // candidate, not only a removed sink.
        let kept = base_counts
            .iter()
            .filter(|((row_class, label), count)| {
                *row_class == class
                    && head_counts.get(&(class, *label)).copied().unwrap_or(0) <= **count
            })
            .map(|((_, label), _)| *label);
        for label in kept {
            let Some((token, property_sink)) = token_of(label) else {
                continue;
            };
            let before = hidden_form_counts(base, token, property_sink);
            let after = hidden_form_counts(head, token, property_sink);
            if let Some(slot) = (0..HiddenForm::ALL.len()).find(|&i| after[i] > before[i]) {
                evidence.push(format!(
                    "{label} sink moved {}",
                    HiddenForm::ALL[slot].phrase()
                ));
            }
        }
    }
    evidence
}

#[cfg(test)]
mod tests {
    use super::*;

    fn detect(file: &str, base: &str, head: &str) -> Vec<String> {
        detect_sink_substitutions(Path::new(file), base, head)
    }

    #[test]
    fn new_function_replaced_by_vm_run_in_new_context() {
        let base = "export const run = (raw: string) => new Function(`return ${raw}`)();\n";
        let head = "import vm from 'node:vm';\nexport const run = (raw: string) => vm.runInNewContext(`(${raw})`);\n";
        assert_eq!(
            detect("src/run.ts", base, head),
            vec!["new Function sink replaced by runInNewContext"]
        );
    }

    #[test]
    fn eval_replaced_by_imported_run_in_new_context() {
        let base = "export const run = (code: string) => eval(code);\n";
        let head = "import { runInNewContext } from 'node:vm';\nexport const run = (code: string) => runInNewContext(code);\n";
        assert_eq!(
            detect("src/run.ts", base, head),
            vec!["eval sink replaced by runInNewContext"]
        );
    }

    #[test]
    fn eval_replaced_by_required_run_in_this_context() {
        let base = "export const run = (code: string) => eval(code);\n";
        let head = "const { runInThisContext } = require('vm');\nexport const run = (code: string) => runInThisContext(code);\n";
        assert_eq!(
            detect("src/run.ts", base, head),
            vec!["eval sink replaced by runInThisContext"]
        );
    }

    #[test]
    fn eval_replaced_by_imported_script() {
        let base = "export const run = (code: string) => eval(code);\n";
        let head = "import { Script } from 'node:vm';\nexport const run = (code: string) => new Script(code).runInThisContext();\n";
        assert_eq!(
            detect("src/run.ts", base, head),
            vec!["eval sink replaced by new Script"]
        );
    }

    #[test]
    fn inner_html_replaced_by_insert_adjacent_html() {
        let base = "export const set = (el: Element, html: string) => { el.innerHTML = html; };\n";
        let head = "export const set = (el: Element, html: string) => { el.insertAdjacentHTML('beforeend', html); };\n";
        assert_eq!(
            detect("src/set.ts", base, head),
            vec!["innerHTML sink replaced by insertAdjacentHTML"]
        );
    }

    #[test]
    fn jsx_html_sink_replaced_by_src_doc() {
        let base = "export const A = ({ h }: { h: string }) => <div dangerouslySetInnerHTML={{ __html: h }} />;\n";
        let head = "export const A = ({ h }: { h: string }) => <iframe srcDoc={h} />;\n";
        assert_eq!(
            detect("src/a.tsx", base, head),
            vec!["dangerouslySetInnerHTML sink replaced by srcDoc"]
        );
    }

    #[test]
    fn jsx_html_sink_moved_under_a_spread() {
        let base = "export const A = ({ h }: { h: string }) => <div dangerouslySetInnerHTML={{ __html: h }} />;\n";
        let head = "export const A = ({ h }: { h: string }) => <div {...{ dangerouslySetInnerHTML: { __html: h } }} />;\n";
        assert_eq!(
            detect("src/a.tsx", base, head),
            vec!["dangerouslySetInnerHTML sink moved into an object key"]
        );
    }

    #[test]
    fn eval_moved_into_a_computed_member_or_an_alias() {
        let base = "export const run = (code: string) => eval(code);\n";
        let computed = "export const run = (code: string) => globalThis['eval'](code);\n";
        let alias = "const run0 = eval;\nexport const run = (code: string) => run0(code);\n";
        let indirect = "export const run = (code: string) => (0, eval)(code);\n";
        assert_eq!(
            detect("src/run.ts", base, computed),
            vec!["eval sink moved into a computed member"]
        );
        assert_eq!(
            detect("src/run.ts", base, alias),
            vec!["eval sink moved into an alias"]
        );
        assert_eq!(
            detect("src/run.ts", base, indirect),
            vec!["eval sink moved into an alias"]
        );
    }

    #[test]
    fn a_sink_that_moves_or_goes_away_gives_no_signal() {
        let base = "export const set = (el: Element, html: string) => { el.innerHTML = html; };\n";
        let moved = "export const set = (el: Element, html: string) => {\n  const target = el;\n  target.innerHTML = html;\n};\n";
        let removed =
            "export const set = (el: Element, html: string) => { el.textContent = html; };\n";
        let literal = "export const set = (el: Element) => { el.innerHTML = ''; };\n";
        assert!(detect("src/set.ts", base, moved).is_empty());
        assert!(detect("src/set.ts", base, removed).is_empty());
        assert!(detect("src/set.ts", base, literal).is_empty());
    }

    #[test]
    fn an_unrelated_object_key_with_a_sink_name_gives_no_signal() {
        let base = "export const f = (el: Element, h: string, code: string) => { el.innerHTML = h; eval(code); };\n";
        let head = "export const f = (el: Element, h: string, code: string) => { el.textContent = h; return code; };\nexport const o = { innerHTML: 1, eval: 2 };\n";
        assert!(detect("src/a.ts", base, head).is_empty());
        let doc = "export const f = (h: string) => { document.write(h); };\n";
        let logger = "export const f = (h: string) => h;\nexport const log = { write: console.log, writeln: console.info };\n";
        assert!(detect("src/a.ts", doc, logger).is_empty());
    }

    #[test]
    fn a_property_sink_moved_into_an_assigned_object_key() {
        let base = "export const set = (el: Element, h: string) => { el.innerHTML = h; };\n";
        let head = "export const set = (el: Element, h: string) => { Object.assign(el, { innerHTML: h }); };\n";
        assert_eq!(
            detect("src/set.ts", base, head),
            vec!["innerHTML sink moved into an object key"]
        );
    }

    #[test]
    fn an_alias_added_beside_a_kept_sink_gives_no_signal() {
        let base = "export const run = (code: string) => eval(code);\n";
        let head = "const run0 = eval;\nexport const run = (code: string) => eval(code);\nexport const run1 = (code: string) => run0(code);\n";
        assert!(detect("src/run.ts", base, head).is_empty());
    }

    #[test]
    fn a_new_sink_without_a_removed_sink_gives_no_signal() {
        let base = "export const run = (code: string) => code;\n";
        let head = "export const run = (code: string) => eval(code);\n";
        assert!(detect("src/run.ts", base, head).is_empty());
    }

    #[test]
    fn sinks_of_different_classes_do_not_pair() {
        let base = "export const run = (code: string) => eval(code);\n";
        let head = "export const run = (el: Element, code: string) => { el.innerHTML = code; };\n";
        assert!(detect("src/run.ts", base, head).is_empty());
    }

    #[test]
    fn only_source_files_take_part() {
        assert!(is_sink_source_path("src/a.tsx"));
        assert!(is_sink_source_path("src/App.vue"));
        assert!(!is_sink_source_path("README.md"));
        assert!(!is_sink_source_path("Makefile"));
    }
}
