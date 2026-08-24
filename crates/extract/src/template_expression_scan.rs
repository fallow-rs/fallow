//! Text-side usage scan and completeness guard for markup the JSX visitor
//! cannot fully account for: MDX prose lines (never parsed) and Astro `{ ... }`
//! expression regions (parsed, but a bare identifier passed whole is not a
//! whole-object use the visitor records).
//!
//! Namespace-import narrowing trusts the member-access stream it is given: once
//! one access is recorded for a binding in a non-entry consumer, every sibling
//! export missing from the stream is reported as unused. A text scanner of a
//! template cannot promise that stream is complete shape by shape (a directive
//! on a masked `<style>` tag, a template literal that looks like a code span),
//! so completeness is enforced structurally instead, over the whole file:
//! every structured template pass reports the byte spans it classified, and
//! [`record_unexplained_mentions`] turns every mention of an import binding
//! outside those spans into a whole-object use; the script side the visitor
//! parsed (Astro frontmatter, MDX statement lines) goes through
//! [`record_unexplained_script_mentions`], which explains a mention only by a
//! static dotted access the visitor recorded or a JSX tag root, because the
//! visitor records a bare identifier as a whole-object use for an allow-list
//! of positions only (`const N = NS`, `pick(NS)`, `[NS]` are not on it). Both
//! keep the graph on its mark-all path. Narrowing therefore applies only when
//! every mention of the binding was structurally understood; for the
//! dead-code crediting consumers (namespace, CSS module, enum, and class
//! member crediting) an unmodelled shape degrades to over-credit.
//!
//! Member accesses also feed consumers that create findings from them (the
//! security secret-source index reads `process.env.X` / `import.meta.env.X`
//! pairs), so the MDX prose scan records a dotted chain only when its root is
//! an import local of the file: a documentation sentence mentioning
//! `process.env.API_KEY` is not a secret read. See issue #2355.

use rustc_hash::FxHashSet;

use crate::visitor::push_member_tag_accesses;
use crate::{ImportInfo, MemberAccess};

/// The import locals the completeness guards protect: every import binding
/// with a local name (namespace, default, named, CSS module, type-only).
/// Enum and class member crediting and CSS-module narrowing read the same
/// whole-object uses as namespace narrowing, so the guards cover them too.
pub fn guarded_import_locals(imports: &[ImportInfo]) -> FxHashSet<&str> {
    imports
        .iter()
        .map(|import| import.local_name.as_str())
        .filter(|local| !local.is_empty())
        .collect()
}

/// The import locals whose exports the graph narrows by member access: a
/// namespace import (`import * as NS`) and a default import of a CSS module
/// (`import styles from './card.module.css'`, mirroring the graph's CSS-module
/// path test). The script-side guard covers exactly these, so a class or enum
/// named in a frontmatter type annotation or `new` expression keeps the
/// visitor's own member crediting instead of falling back to every member.
pub fn narrowable_import_locals(imports: &[ImportInfo]) -> FxHashSet<&str> {
    imports
        .iter()
        .filter(|import| match &import.imported_name {
            crate::ImportedName::Namespace => true,
            crate::ImportedName::Default => is_css_module_source(&import.source),
            crate::ImportedName::Named(name) => {
                name == "default" && is_css_module_source(&import.source)
            }
            crate::ImportedName::SideEffect => false,
        })
        .map(|import| import.local_name.as_str())
        .filter(|local| !local.is_empty())
        .collect()
}

pub fn is_css_module_source(source: &str) -> bool {
    let file_name = source.rsplit(['/', '\\']).next().unwrap_or(source);
    let Some((stem, extension)) = file_name.rsplit_once('.') else {
        return false;
    };
    stem.ends_with(".module") && matches!(extension, "css" | "scss" | "sass" | "less")
}

/// The `(object, member)` pairs recorded for the guarded bindings, which
/// [`record_unexplained_script_mentions`] explains dotted mentions against.
pub fn recorded_member_pairs<'a>(
    accesses: &'a [MemberAccess],
    guarded: &FxHashSet<&str>,
) -> FxHashSet<(&'a str, &'a str)> {
    accesses
        .iter()
        .filter(|access| guarded.contains(access.object.as_str()))
        .map(|access| (access.object.as_str(), access.member.as_str()))
        .collect()
}

/// The import declaration spans in file coordinates, merged for
/// [`pos_in_ranges`]: the local name inside its own declaration is not a
/// mention.
pub fn import_declaration_ranges(imports: &[ImportInfo]) -> Vec<(usize, usize)> {
    merge_ranges(
        imports
            .iter()
            .map(|import| (import.span.start as usize, import.span.end as usize))
            .collect(),
    )
}

/// Which markup fragment [`scan_template_usage`] is reading.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TemplateScanMode {
    /// An MDX prose line: the only tag and expression scan the body gets, so
    /// opening dotted tags and dotted chains whose root is an import local
    /// record member accesses (every crediting consumer is keyed by import
    /// locals; a foreign root such as `process.env` in prose is documentation,
    /// not a read). Inline code spans are documentation too: nothing inside
    /// them records an access, and a binding mentioned there falls back to a
    /// whole-object use.
    MdxProse,
    /// An Astro `{ ... }` region the visitor parsed cleanly: the visitor owns
    /// the dotted chains (and the tag scan the dotted tags), so only the bare
    /// whole-object passes (`all={NS}`) the visitor does not record are added.
    /// Backticks are template literals, never code spans.
    AstroParsedRegion,
}

/// Where an identifier chain sits relative to a JSX tag delimiter.
#[derive(Clone, Copy, PartialEq, Eq)]
enum TagPosition {
    /// Immediately after `</`: the opening tag already recorded.
    Closing,
    /// Immediately after `<`: a component tag name.
    Opening,
    /// Plain expression or prose position.
    None,
}

/// Scan one text fragment and append what it implies for usage crediting.
///
/// Identifier chains (`Ident`, `Ident.Ident`, ...) are classified by the byte in
/// front of them: after `</` nothing records; after `<` a dotted name records
/// one access per nesting level in MDX mode while a bare name never does
/// (rendering a component is not a whole-object pass); anywhere else a dotted
/// chain records one access per level in MDX mode, and a bare name that is an
/// import binding records a deduplicated whole-object use. In MDX mode a
/// dotted chain records only when its root is an import binding, so prose
/// never creates member accesses on foreign roots (`process.env.API_KEY`,
/// `items.map`). Only the byte directly in front counts as a tag delimiter,
/// so `{a < NS}` keeps `NS` a whole-object use instead of mistaking it for a
/// tag.
///
/// Every identifier in the fragment is classified by one of those arms, so the
/// whole fragment counts as structurally understood, except the inline code
/// spans MDX mode skips: those are handed to [`record_unexplained_mentions`]
/// so a binding mentioned inside one (a real code span, or a template literal
/// the line scanner cannot tell apart from one) keeps its mark-all crediting.
///
/// The scan is byte-based, but every boundary it slices on (`<`, `/`, `.`,
/// backtick, ASCII identifier bytes) is ASCII, so slices always land on char
/// boundaries.
pub fn scan_template_usage(
    text: &str,
    import_locals: &FxHashSet<&str>,
    mode: TemplateScanMode,
    accesses: &mut Vec<MemberAccess>,
    whole_object_uses: &mut Vec<String>,
) {
    let record_dotted = mode == TemplateScanMode::MdxProse;
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let byte = bytes[i];
        if byte == b'`' && mode == TemplateScanMode::MdxProse {
            let end = skip_inline_code_span(bytes, i);
            if let Some(span) = text.get(i..end) {
                record_unexplained_mentions(span, import_locals, &[], whole_object_uses);
            }
            i = end;
            continue;
        }
        if !is_identifier_start(byte) {
            i += 1;
            continue;
        }
        let end = dotted_chain_end(bytes, i);
        let Some(chain) = text.get(i..end) else {
            return;
        };
        let dotted = chain.contains('.');
        match tag_position(bytes, i) {
            TagPosition::Opening | TagPosition::None if dotted => {
                if record_dotted && import_locals.contains(chain_root(chain)) {
                    push_member_tag_accesses(accesses, chain);
                }
            }
            TagPosition::None => push_whole_object_use(whole_object_uses, import_locals, chain),
            TagPosition::Closing | TagPosition::Opening => {}
        }
        i = end;
    }
}

/// Record a whole-object use for every guarded binding mentioned in `text`
/// outside `explained`, the disjoint ascending byte ranges a structured pass
/// classified (see [`merge_ranges`]). A mention is the binding name on
/// identifier boundaries that is not the property of a member access (so
/// `NS.Moon` and the spread `...NS` mention `NS`, while `other.NS` and
/// `other?.NS` do not). Whole-object uses are deduplicated.
///
/// This is the completeness guard of issue #2355: a binding whose every
/// mention lies inside an explained span narrows on the accesses the
/// structured passes recorded; one unexplained mention keeps it on the
/// mark-all path.
pub fn record_unexplained_mentions(
    text: &str,
    guarded: &FxHashSet<&str>,
    explained: &[(usize, usize)],
    whole_object_uses: &mut Vec<String>,
) {
    if guarded.is_empty() {
        return;
    }
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if !is_identifier_start(bytes[i]) {
            i += 1;
            continue;
        }
        let end = identifier_end(bytes, i);
        if !is_member_property(bytes, i)
            && !pos_in_ranges(explained, i)
            && let Some(name) = text.get(i..end)
        {
            push_whole_object_use(whole_object_uses, guarded, name);
        }
        i = end;
    }
}

/// Completeness guard for script text the visitor parsed: the Astro
/// frontmatter and the MDX `import` / `export` statement lines (issue #2355).
///
/// The visitor records a bare identifier as a whole-object use only for an
/// allow-list of positions (spread, `Object.keys(NS)`, `for ... in`, computed
/// non-string access, rest destructuring), so an alias (`const N = NS`), a
/// cast (`NS as T`), a call argument (`pick(NS)`), `Object.assign({}, NS)`,
/// an array literal (`[NS]`), a property value (`{ all: NS }`), or a JSX
/// attribute on a statement line (`<Callout all={NS} />`) leaves no trace and
/// would let the binding narrow to its dotted accesses. This guard walks the
/// raw script text starting at `text_offset` in the file and records a
/// whole-object use for every guarded binding (see
/// [`narrowable_import_locals`]) mentioned on identifier boundaries outside
/// the `excluded` ranges (the import declarations, in file coordinates, merged
/// by [`merge_ranges`]) unless the mention is structurally understood: a
/// static dotted access `NS.member` whose `(NS, member)` pair the visitor
/// `recorded`, or a JSX tag root (`<NS.Card`, `</Callout`). Set membership
/// rather than a count keeps the guard independent of how many times the
/// visitor records one access and of the deduplication applied to
/// template-sourced accesses. Mentions the visitor does understand more
/// precisely (`NS["Moon"]`, `NS?.Moon`) are unexplained here and keep the
/// binding on mark-all, which only over-credits. The member property of an
/// access (`other.NS`) is not a mention; the spread `...NS` is.
pub fn record_unexplained_script_mentions(
    text: &str,
    text_offset: usize,
    guarded: &FxHashSet<&str>,
    excluded: &[(usize, usize)],
    recorded: &FxHashSet<(&str, &str)>,
    whole_object_uses: &mut Vec<String>,
) {
    if guarded.is_empty() {
        return;
    }
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if !is_identifier_start(bytes[i]) {
            i += 1;
            continue;
        }
        let end = identifier_end(bytes, i);
        let Some(name) = text.get(i..end) else {
            return;
        };
        if guarded.contains(name)
            && !is_member_property(bytes, i)
            && !pos_in_ranges(excluded, text_offset + i)
            && !is_explained_script_mention(bytes, text, i, end, recorded)
        {
            push_whole_object_use(whole_object_uses, guarded, name);
        }
        i = end;
    }
}

/// Whether the guarded identifier at `start..end` is a static dotted access the
/// visitor `recorded` or a JSX tag root.
fn is_explained_script_mention(
    bytes: &[u8],
    text: &str,
    start: usize,
    end: usize,
    recorded: &FxHashSet<(&str, &str)>,
) -> bool {
    if tag_position(bytes, start) != TagPosition::None {
        return true;
    }
    if bytes.get(end) != Some(&b'.') {
        return false;
    }
    let member_end = identifier_end(bytes, end + 1);
    if member_end == end + 1 {
        return false;
    }
    match (text.get(start..end), text.get(end + 1..member_end)) {
        (Some(object), Some(member)) => recorded.contains(&(object, member)),
        _ => false,
    }
}

/// The root identifier of a dotted chain (`NS` for `NS.Card.Body`).
fn chain_root(chain: &str) -> &str {
    chain.split('.').next().unwrap_or(chain)
}

/// Whether the identifier at `start` is the property of a member access: it is
/// preceded by a `.` that is not the tail of a spread operator (`...`).
fn is_member_property(bytes: &[u8], start: usize) -> bool {
    match bytes[..start] {
        [.., b'.', b'.', b'.'] => false,
        [.., b'.'] => true,
        _ => false,
    }
}

fn push_whole_object_use(
    whole_object_uses: &mut Vec<String>,
    guarded: &FxHashSet<&str>,
    name: &str,
) {
    if guarded.contains(name) && !whole_object_uses.iter().any(|existing| existing == name) {
        whole_object_uses.push(name.to_string());
    }
}

/// Sort byte ranges by start and merge overlapping or touching ones into the
/// disjoint ascending list [`pos_in_ranges`] relies on. The union covers
/// exactly the same byte positions as the input.
pub fn merge_ranges(mut ranges: Vec<(usize, usize)>) -> Vec<(usize, usize)> {
    ranges.sort_unstable_by_key(|&(start, _)| start);
    let mut merged: Vec<(usize, usize)> = Vec::with_capacity(ranges.len());
    for (start, end) in ranges {
        if let Some(last) = merged.last_mut()
            && start <= last.1
        {
            last.1 = last.1.max(end);
        } else {
            merged.push((start, end));
        }
    }
    merged
}

/// Test whether `pos` falls inside any range of the disjoint ascending list
/// produced by [`merge_ranges`]. Membership is a binary search
/// (`partition_point`): because the list is disjoint and ascending, only the
/// last range with `start <= pos` can contain it, so a single bounds check on
/// that candidate decides membership.
pub fn pos_in_ranges(ranges: &[(usize, usize)], pos: usize) -> bool {
    let idx = ranges.partition_point(|&(start, _)| start <= pos);
    idx > 0 && pos < ranges[idx - 1].1
}

fn tag_position(bytes: &[u8], start: usize) -> TagPosition {
    match bytes[..start] {
        [.., b'<', b'/'] => TagPosition::Closing,
        [.., b'<'] => TagPosition::Opening,
        _ => TagPosition::None,
    }
}

/// Given `open` = the index of a backtick, return the index just past the
/// matching code-span closer: a backtick run of exactly the same length, as
/// CommonMark defines code spans. An unmatched run is literal text, so the
/// scan resumes right after it instead of hiding the rest of the line.
fn skip_inline_code_span(bytes: &[u8], open: usize) -> usize {
    let run = backtick_run_len(bytes, open);
    let mut j = open + run;
    while j < bytes.len() {
        if bytes[j] != b'`' {
            j += 1;
            continue;
        }
        let closer = backtick_run_len(bytes, j);
        if closer == run {
            return j + closer;
        }
        j += closer;
    }
    open + run
}

fn backtick_run_len(bytes: &[u8], start: usize) -> usize {
    bytes[start..].iter().take_while(|&&b| b == b'`').count()
}

/// Return the end index of the dotted identifier chain starting at `start`
/// (`NS.Card` in `<NS.Card />`). A dot that is not followed by another
/// identifier is left unconsumed.
fn dotted_chain_end(bytes: &[u8], start: usize) -> usize {
    let mut end = identifier_end(bytes, start);
    while end < bytes.len() && bytes[end] == b'.' {
        let segment_end = identifier_end(bytes, end + 1);
        if segment_end == end + 1 {
            break;
        }
        end = segment_end;
    }
    end
}

fn is_identifier_start(byte: u8) -> bool {
    byte.is_ascii_alphabetic() || byte == b'_' || byte == b'$'
}

fn identifier_end(bytes: &[u8], start: usize) -> usize {
    if !bytes.get(start).is_some_and(|&b| is_identifier_start(b)) {
        return start;
    }
    let mut end = start + 1;
    while bytes
        .get(end)
        .is_some_and(|&b| b.is_ascii_alphanumeric() || b == b'_' || b == b'$')
    {
        end += 1;
    }
    end
}

#[cfg(test)]
mod tests {
    use super::*;

    const MDX: TemplateScanMode = TemplateScanMode::MdxProse;
    const ASTRO: TemplateScanMode = TemplateScanMode::AstroParsedRegion;

    fn scan(text: &str, locals: &[&str], mode: TemplateScanMode) -> (Vec<String>, Vec<String>) {
        let import_locals: FxHashSet<&str> = locals.iter().copied().collect();
        let mut accesses = Vec::new();
        let mut whole = Vec::new();
        scan_template_usage(text, &import_locals, mode, &mut accesses, &mut whole);
        let accesses = accesses
            .into_iter()
            .map(|access| format!("{}.{}", access.object, access.member))
            .collect();
        (accesses, whole)
    }

    fn unexplained(text: &str, guarded: &[&str], explained: &[(usize, usize)]) -> Vec<String> {
        let guarded: FxHashSet<&str> = guarded.iter().copied().collect();
        let mut whole = Vec::new();
        record_unexplained_mentions(text, &guarded, explained, &mut whole);
        whole
    }

    #[test]
    fn dotted_chain_records_each_level_and_bare_import_local_records_whole_use() {
        let (accesses, whole) = scan("icon={NS.Moon} all={NS} {A.B.C}", &["NS", "A"], MDX);
        assert_eq!(accesses, vec!["NS.Moon", "A.B.C", "A.B"]);
        assert_eq!(whole, vec!["NS"]);
    }

    #[test]
    fn bare_identifier_that_is_not_an_import_records_nothing() {
        let (accesses, whole) = scan("{items.map((item) => item)}", &["NS"], MDX);
        assert!(
            accesses.is_empty(),
            "a chain rooted outside the import locals is not a use; got {accesses:?}"
        );
        assert!(whole.is_empty());
    }

    /// Issue #2355: prose chains record only for import-local roots, so a
    /// documentation sentence naming `process.env.API_KEY` never becomes a
    /// secret-source member access, while `{NS.helper()}` still records.
    #[test]
    fn mdx_prose_dotted_chain_with_foreign_root_records_nothing() {
        let (accesses, whole) = scan(
            "Set process.env.API_KEY and import.meta.env.SECRET, then {NS.helper()} <NS.Card />",
            &["NS"],
            MDX,
        );
        assert_eq!(accesses, vec!["NS.helper", "NS.Card"]);
        assert!(whole.is_empty());

        let (accesses, _) = scan("Set process.env.API_KEY first", &["process"], MDX);
        assert_eq!(
            accesses,
            vec!["process.env.API_KEY", "process.env"],
            "an imported `process` binding is a real root"
        );
    }

    fn script(
        text: &str,
        offset: usize,
        guarded: &[&str],
        excluded: &[(usize, usize)],
        recorded: &[(&str, &str)],
    ) -> Vec<String> {
        let guarded: FxHashSet<&str> = guarded.iter().copied().collect();
        let recorded: FxHashSet<(&str, &str)> = recorded.iter().copied().collect();
        let mut whole = Vec::new();
        record_unexplained_script_mentions(text, offset, &guarded, excluded, &recorded, &mut whole);
        whole
    }

    /// Issue #2355 script guard: a mention is explained only by a static
    /// dotted access the visitor recorded or a JSX tag root; an alias, a cast,
    /// a call argument, a literal element, a property value, and a dotted
    /// access the visitor did not record all keep the binding on mark-all.
    #[test]
    fn script_mentions_are_explained_only_by_recorded_dotted_access_or_tag_root() {
        let recorded = [("NS", "Moon"), ("NS", "Star"), ("CSS", "root")];
        for shape in [
            "const N = NS;",
            "const rec = NS as Record<string, unknown>;",
            "const picked = pick(NS);",
            "const merged = Object.assign({}, NS);",
            "const list = [NS];",
            "const props = { all: NS };",
            "export const all = NS;",
            "export const Demo = () => <Callout all={NS} />;",
            "const sun = NS.Sun;",
            "const moon = NS?.Moon;",
            "const moon = NS[\"Moon\"];",
            "const copy = { ...NS };",
        ] {
            assert_eq!(
                script(shape, 0, &["NS"], &[], &recorded),
                vec!["NS"],
                "{shape}"
            );
        }
        for shape in [
            "const moon = NS.Moon; const star = NS.Star.Deep;",
            "export const Demo = () => <NS.Moon><NS.Star /></NS.Moon>;",
            "const cls = CSS.root; const other = outer.NS; const n = NSX + fooNS;",
            "<Callout all={other} />",
        ] {
            assert!(
                script(shape, 0, &["NS", "CSS"], &[], &recorded).is_empty(),
                "{shape}"
            );
        }
    }

    /// Issue #2355 script guard scope: namespace imports and CSS-module
    /// default imports are narrowable; other default, named, and side-effect
    /// imports are not.
    #[test]
    fn narrowable_import_locals_cover_namespace_and_css_module_bindings() {
        let import = |source: &str, imported_name: crate::ImportedName, local: &str| ImportInfo {
            source: source.to_string(),
            imported_name,
            local_name: local.to_string(),
            is_type_only: false,
            is_type_only_star: false,
            from_style: false,
            span: oxc_span::Span::default(),
            source_span: oxc_span::Span::default(),
        };
        let imports = [
            import("./ns", crate::ImportedName::Namespace, "NS"),
            import("./card.module.css", crate::ImportedName::Default, "styles"),
            import("../a/b.module.scss", crate::ImportedName::Default, "scss"),
            import("./theme.module.less", crate::ImportedName::Default, "less"),
            import(
                "./theme.module.sass",
                crate::ImportedName::Named("default".into()),
                "sass",
            ),
            import("./plain.css", crate::ImportedName::Default, "plain"),
            import("./module.ts", crate::ImportedName::Default, "Layout"),
            import("./util", crate::ImportedName::Named("Util".into()), "Util"),
            import("./side", crate::ImportedName::SideEffect, ""),
        ];
        let mut locals: Vec<&str> = narrowable_import_locals(&imports).into_iter().collect();
        locals.sort_unstable();
        assert_eq!(locals, vec!["NS", "less", "sass", "scss", "styles"]);
        assert_eq!(guarded_import_locals(&imports).len(), 8);
    }

    /// Issue #2355 script guard: the import declaration's own span is not a
    /// mention, positions are compared in file coordinates, and only guarded
    /// bindings record.
    #[test]
    fn script_mentions_skip_excluded_import_spans_and_honour_offsets() {
        let text = "import * as NS from './ns';\nconst N = NS;\n";
        let import_span = merge_ranges(vec![(100, 127)]);
        assert_eq!(script(text, 100, &["NS"], &import_span, &[]), vec!["NS"]);

        let text = "import * as NS from './ns';\nconst moon = NS.Moon;\n";
        assert!(script(text, 100, &["NS"], &import_span, &[("NS", "Moon")]).is_empty());
        assert_eq!(
            script(text, 0, &["NS"], &import_span, &[("NS", "Moon")]),
            vec!["NS"],
            "an offset mismatch leaves the declaration unexcluded"
        );
        assert!(script(text, 100, &[], &import_span, &[]).is_empty());
    }

    #[test]
    fn opening_tag_records_member_tag_only_in_mdx_mode() {
        let (accesses, whole) = scan("<NS.Card /> <NS.Card>x</NS.Card> <NS />", &["NS"], MDX);
        assert_eq!(accesses, vec!["NS.Card", "NS.Card"]);
        assert!(
            whole.is_empty(),
            "a rendered tag is not a whole-object pass"
        );

        let (accesses, whole) = scan("<NS.Card /> <NS.Card>x</NS.Card>", &["NS"], ASTRO);
        assert!(
            accesses.is_empty(),
            "Astro leaves tags to its template scan"
        );
        assert!(whole.is_empty());
    }

    #[test]
    fn parsed_astro_region_records_only_bare_whole_object_passes() {
        let (accesses, whole) = scan("fn(NS.Moon, NS) + other.prop", &["NS", "other"], ASTRO);
        assert!(
            accesses.is_empty(),
            "chains belong to the visitor; got {accesses:?}"
        );
        assert_eq!(whole, vec!["NS"]);
    }

    #[test]
    fn comparison_operator_is_not_a_tag_delimiter() {
        let (accesses, whole) = scan("{a < NS} {b <NS.max}", &["NS"], MDX);
        assert_eq!(accesses, vec!["NS.max"]);
        assert_eq!(whole, vec!["NS"]);
    }

    #[test]
    fn whole_object_uses_are_deduplicated() {
        let (_, whole) = scan("{NS} {NS} {fn(NS)}", &["NS"], MDX);
        assert_eq!(whole, vec!["NS"]);
    }

    #[test]
    fn inline_code_span_records_no_access_but_keeps_mentioned_binding_on_mark_all() {
        let (accesses, whole) = scan("Use `NS.Hidden` and `Other` but {NS.Shown}", &["NS"], MDX);
        assert_eq!(accesses, vec!["NS.Shown"]);
        assert_eq!(
            whole,
            vec!["NS"],
            "a binding mentioned inside a code span is unexplained"
        );

        let (accesses, whole) = scan("Use `Other.Thing` but {NS.Shown}", &["NS"], MDX);
        assert_eq!(accesses, vec!["NS.Shown"]);
        assert!(
            whole.is_empty(),
            "only guarded bindings record; got {whole:?}"
        );

        let (accesses, whole) = scan("{`${NS.Tpl}`} {`${NS}`}", &["NS"], ASTRO);
        assert!(accesses.is_empty());
        assert_eq!(whole, vec!["NS"], "Astro backticks are template literals");
    }

    #[test]
    fn template_literal_mistaken_for_code_span_still_keeps_binding_on_mark_all() {
        let (accesses, whole) = scan(
            "<NS.Star /> <Callout title={`Moon: ${NS.Moon}`}>",
            &["NS"],
            MDX,
        );
        assert_eq!(accesses, vec!["NS.Star"]);
        assert_eq!(whole, vec!["NS"]);
    }

    #[test]
    fn multibyte_text_around_chains_does_not_panic() {
        let (accesses, whole) = scan("Café {NS.Uni} 日本語 {NS} ✓", &["NS"], MDX);
        assert_eq!(accesses, vec!["NS.Uni"]);
        assert_eq!(whole, vec!["NS"]);
    }

    #[test]
    fn unexplained_mentions_honour_identifier_boundaries_and_dots() {
        let whole = unexplained("NSX fooNS other.NS a?.NS", &["NS"], &[]);
        assert!(whole.is_empty(), "got {whole:?}");

        let whole = unexplained("x NS.Moon y", &["NS"], &[]);
        assert_eq!(whole, vec!["NS"]);

        let whole = unexplained("<Callout {...NS} />", &["NS"], &[]);
        assert_eq!(whole, vec!["NS"], "a spread is a mention, not a property");

        let whole = unexplained(
            "(NS) {NS} <NS /> $NS _NS NS$",
            &["NS", "$NS", "_NS", "NS$"],
            &[],
        );
        assert_eq!(whole, vec!["NS", "$NS", "_NS", "NS$"]);
    }

    #[test]
    fn unexplained_mentions_skip_explained_ranges() {
        let text = "{NS.Moon} <NS.Star /> define:vars={{ NS }}";
        let explained = merge_ranges(vec![(1, 8), (11, 13)]);
        assert_eq!(unexplained(text, &["NS"], &explained), vec!["NS"]);

        let explained = merge_ranges(vec![(1, 8), (11, 13), (36, 40)]);
        assert!(unexplained(text, &["NS"], &explained).is_empty());
    }

    #[test]
    fn unexplained_mentions_on_multibyte_text_do_not_panic() {
        let whole = unexplained("Café NS 日本語 NS ✓ éNS", &["NS"], &[]);
        assert_eq!(whole, vec!["NS"]);
    }

    #[test]
    fn merge_ranges_sorts_and_merges_overlaps() {
        assert_eq!(
            merge_ranges(vec![(10, 20), (0, 5), (15, 25), (5, 7), (30, 31)]),
            vec![(0, 7), (10, 25), (30, 31)]
        );
        let merged = merge_ranges(vec![(10, 20), (0, 5), (15, 25)]);
        for (pos, inside) in [
            (0, true),
            (4, true),
            (5, false),
            (9, false),
            (10, true),
            (24, true),
            (25, false),
        ] {
            assert_eq!(pos_in_ranges(&merged, pos), inside, "pos {pos}");
        }
    }
}
