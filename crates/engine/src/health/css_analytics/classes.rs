use super::*;

/// Shortest authored CSS class that can be a credible typo target. Below this a
/// one-edit near miss is too likely to be a coincidental collision between two
/// short real words (`catch` vs `match`, `list` vs `last`) rather than a typo.
/// Real component-class typos are compound / hyphenated and comfortably longer.
/// (Real-world smoke on Svelte: `catch` vs `match` in test fixtures.)
const MIN_DEFINED_CLASS_LEN: usize = 6;
/// Shortest markup token worth typo-checking, for the same reason. One below the
/// defined floor, since a one-edit pair differs in length by at most one.
const MIN_TOKEN_LEN: usize = 5;

/// Find the best one-edit typo suggestion for a markup token among the defined
/// classes, using a length-bucketed index so only classes of length `len-1`,
/// `len`, `len+1` are compared. Returns the lexicographically smallest defined
/// class at edit distance one (deterministic), or `None`.
fn best_class_suggestion<'a>(
    token: &str,
    by_len: &'a rustc_hash::FxHashMap<usize, Vec<&'a str>>,
) -> Option<&'a str> {
    let len = token.len();
    let mut best: Option<&str> = None;
    for candidate_len in [len.wrapping_sub(1), len, len + 1] {
        let Some(bucket) = by_len.get(&candidate_len) else {
            continue;
        };
        for &defined in bucket {
            if defined.len() < MIN_DEFINED_CLASS_LEN {
                continue;
            }
            if crate::css::is_typo_edit(token, defined)
                && best.is_none_or(|current| defined < current)
            {
                best = Some(defined);
            }
        }
    }
    best
}

/// True when a markup class token is Tailwind-flavored (a variant prefix `:`,
/// an opacity `/`, or an arbitrary-value bracket), so it is not an authored CSS
/// class and never a typo candidate.
fn is_tailwind_shaped(token: &str) -> bool {
    token.contains([':', '/', '[', ']'])
}

/// Length-bucketed index over the typo-target classes for O(1)-ish near-miss.
/// Drops names ending in `-` / `_`: those are SCSS interpolation artifacts
/// (`.display-#{$i}` parsed by lightningcss as a partial `display-`), never a
/// real typo target.
fn build_typo_target_index(
    defined: &rustc_hash::FxHashSet<String>,
) -> rustc_hash::FxHashMap<usize, Vec<&str>> {
    let mut by_len: rustc_hash::FxHashMap<usize, Vec<&str>> = rustc_hash::FxHashMap::default();
    for class in defined {
        if class.len() >= MIN_DEFINED_CLASS_LEN && !class.ends_with('-') && !class.ends_with('_') {
            by_len.entry(class.len()).or_default().push(class.as_str());
        }
    }
    by_len
}

/// Collect the likely-typo class references in one markup source into `out`,
/// deduping by `(rel, line, value)` via `seen`.
fn collect_unresolved_class_refs_in_file<'a>(
    source: &str,
    rel: &str,
    defined: &rustc_hash::FxHashSet<String>,
    by_len: &'a rustc_hash::FxHashMap<usize, Vec<&'a str>>,
    seen: &mut rustc_hash::FxHashSet<(String, u32, String)>,
    out: &mut Vec<fallow_output::UnresolvedClassReference>,
) {
    use fallow_output::{CssCandidateAction, UnresolvedClassReference};
    for token in crate::css::scan_markup_class_tokens(source).static_tokens {
        if token.value.len() < MIN_TOKEN_LEN
            || is_tailwind_shaped(&token.value)
            || defined.contains(&token.value)
        {
            continue;
        }
        let Some(suggestion) = best_class_suggestion(&token.value, by_len) else {
            continue;
        };
        let key = (rel.to_owned(), token.line, token.value.clone());
        if !seen.insert(key) {
            continue;
        }
        out.push(UnresolvedClassReference {
            actions: vec![CssCandidateAction::verify_unresolved_class(
                &token.value,
                suggestion,
            )],
            class: token.value,
            suggestion: suggestion.to_owned(),
            path: rel.to_owned(),
            line: token.line,
        });
    }
}

/// Scan markup for static `class` / `className` tokens that match no defined CSS
/// class but are one edit from a defined class (a likely typo / stale rename).
/// The defined set is the full project; markup honors the ignore / changed /
/// workspace filters (a typo is local). Near-zero false-positive by the near-miss
/// restriction: Tailwind utilities and third-party classes are not one edit from
/// an authored class. Candidates, never gated.
pub(super) fn scan_unresolved_class_references(
    files: &[fallow_types::discover::DiscoveredFile],
    ctx: HealthScanCtx<'_>,
    summary: &mut fallow_output::CssAnalyticsSummary,
    class_inventory: Option<&CssClassInventory>,
) -> Vec<fallow_output::UnresolvedClassReference> {
    let HealthScanCtx {
        config, ignore_set, ..
    } = ctx;

    use fallow_output::UnresolvedClassReference;

    // Abstain on preprocessor-dominant projects. lightningcss parses `.scss` /
    // `.sass` / `.less` source textually but cannot expand loops / mixins, so a
    // generated class (`.bg-#{$color}`, `.col-#{$i}`) is invisible to the defined
    // set. On a SCSS framework like Bootstrap that makes a real, used class
    // (`bg-white`) look unresolved and false-positive as a typo of a parsed
    // sibling. When preprocessor stylesheets outnumber plain CSS, the defined set
    // is too incomplete to trust, so emit nothing (real-world smoke: Bootstrap).
    let fallback_class_inventory;
    let class_inventory = if let Some(inventory) = class_inventory {
        inventory
    } else {
        fallback_class_inventory = css_class_inventory(files, config, ignore_set);
        &fallback_class_inventory
    };
    let css_files = class_inventory.css_files;
    let preprocessor_files = class_inventory.preprocessor_files;
    summary.preprocessor_stylesheets = saturate_len(preprocessor_files);
    if preprocessor_files > css_files {
        summary.preprocessor_reachability_abstained = true;
        return Vec::new();
    }

    let defined = &class_inventory.typo_target_classes;
    if defined.is_empty() {
        return Vec::new();
    }
    let by_len = build_typo_target_index(defined);

    let mut out: Vec<UnresolvedClassReference> = Vec::new();
    let mut seen: rustc_hash::FxHashSet<(String, u32, String)> = rustc_hash::FxHashSet::default();
    for file in files {
        let Some((rel, source)) = read_markup_scan_source(file, ctx) else {
            continue;
        };
        collect_unresolved_class_refs_in_file(&source, &rel, defined, &by_len, &mut seen, &mut out);
    }

    out.sort_by(|a, b| {
        a.path
            .cmp(&b.path)
            .then_with(|| a.line.cmp(&b.line))
            .then_with(|| a.class.cmp(&b.class))
    });
    summary.unresolved_class_references = saturate_len(out.len());
    out
}

/// Blank every `@font-face { ... }` block in a (lowercased) source so a declared
/// family's own `font-family:` inside its definition does not self-credit when
/// the source is scanned for OTHER references to that family. The `@font-face`,
/// `{`, and `}` boundaries are ASCII, so replacing the whole block range with
/// spaces preserves UTF-8 validity (any multi-byte family name inside the block
/// is fully within the replaced range).
fn mask_font_face_blocks(lower_source: &str) -> String {
    if !lower_source.contains("@font-face") {
        return lower_source.to_owned();
    }
    let mut bytes = lower_source.as_bytes().to_vec();
    let sb = lower_source.as_bytes();
    let mut search = 0;
    while let Some(rel) = lower_source[search..].find("@font-face") {
        let start = search + rel;
        let Some(brace_rel) = lower_source[start..].find('{') else {
            break;
        };
        let mut depth = 0usize;
        let mut j = start + brace_rel;
        while j < sb.len() {
            match sb[j] {
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
                _ => {}
            }
            j += 1;
        }
        let end = (j + 1).min(bytes.len());
        for b in &mut bytes[start..end] {
            *b = b' ';
        }
        search = end;
    }
    String::from_utf8(bytes).unwrap_or_else(|_| lower_source.to_owned())
}

/// Of the candidate unused `@font-face` families, the subset whose name appears
/// as a substring in some other source file (`.css`/`.scss`/`.sass`/`.less`,
/// JS/TS, or markup), OUTSIDE its own `@font-face` block. Such a family is
/// applied somewhere the structural `font-family` reference set cannot see (a
/// Tailwind v4 `--font-*` theme token in a `@theme` block lightningcss skips, a
/// `.scss` theme, a canvas/JS `fontFamily` assignment, an inline style), so it
/// is NOT dead.
pub(super) fn font_families_referenced_in_source(
    candidates: &[fallow_output::UnusedFontFace],
    files: &[fallow_types::discover::DiscoveredFile],
    config: &ResolvedConfig,
    ignore_set: &globset::GlobSet,
) -> rustc_hash::FxHashSet<String> {
    // `(original-case family, lowercase family)`; the lowercase form drives the
    // substring test because CSS font-family names are case-insensitive, while the
    // original case is what gets returned for the caller's retain.
    let mut pending: Vec<(String, String)> = candidates
        .iter()
        .map(|c| (c.family.clone(), c.family.to_ascii_lowercase()))
        .collect();
    let mut found: rustc_hash::FxHashSet<String> = rustc_hash::FxHashSet::default();
    for file in files {
        if pending.is_empty() {
            break;
        }
        let path = &file.path;
        let extension = path.extension().and_then(|ext| ext.to_str());
        if !matches!(
            extension,
            Some(
                "css"
                    | "scss"
                    | "sass"
                    | "less"
                    | "js"
                    | "jsx"
                    | "ts"
                    | "tsx"
                    | "mjs"
                    | "cjs"
                    | "vue"
                    | "svelte"
                    | "astro"
                    | "html"
                    | "mdx"
            )
        ) {
            continue;
        }
        let relative = path.strip_prefix(&config.root).unwrap_or(path);
        if ignore_set.is_match(relative) {
            continue;
        }
        let Ok(source) = std::fs::read_to_string(path) else {
            continue;
        };
        // `.css` is scanned too: a family can be referenced via a custom-property
        // value (a Tailwind v4 `--font-*` theme token, which lives inside a
        // `@theme` block that lightningcss skips, so the structural reference set
        // never sees it). The family's OWN `@font-face` definition is masked so it
        // does not self-credit (every declared family appears in its own block).
        let source_lower = mask_font_face_blocks(&source.to_ascii_lowercase());
        pending.retain(|(family, family_lower)| {
            if source_lower.contains(family_lower.as_str()) {
                found.insert(family.clone());
                false
            } else {
                true
            }
        });
    }
    found
}

/// Shortest global class worth reporting as unreferenced. Shorter names are
/// substring-prone (their literal appears inside many longer strings, so the
/// substring reference check already keeps them safe) and low-signal.
const MIN_UNREF_CLASS_LEN: usize = 5;

/// Extract class-shaped tokens from quoted string literals (`'...'` / `"..."` /
/// `` `...` ``) in a source string and add them to `out`, crediting a name
/// applied outside a `class=` / `className=` attribute (a config-object
/// `className: 'leveret-toast'`, a helper `return "x-y"`, a JS inline-style
/// `animation: 'progress-indeterminate 1s'`).
///
/// `require_dash` controls strictness. For CLASS crediting it is `true`: only
/// compound (dash-bearing) tokens are taken, so a generic single word never
/// coincidentally credits a class and breaks the whole-sheet abstain that
/// protects classes used in a surface fallow cannot read (Phoenix `.heex`). For
/// KEYFRAME crediting it is `false` (the caller filters to actually-defined
/// keyframes, so over-extraction is inert), letting a single-word keyframe name
/// (`spin`, `jsanim`) be credited from a JS `animation:` string too.
pub(super) fn collect_quoted_class_tokens(
    source: &str,
    out: &mut rustc_hash::FxHashSet<String>,
    require_dash: bool,
) {
    let bytes = source.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let quote = bytes[i];
        if quote == b'"' || quote == b'\'' || quote == b'`' {
            let start = i + 1;
            let mut j = start;
            while j < bytes.len() && bytes[j] != quote {
                j += 1;
            }
            if let Some(content) = source.get(start..j) {
                for token in content
                    .split(|c: char| !(c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'))
                {
                    let shaped = token.as_bytes().first().is_some_and(u8::is_ascii_lowercase)
                        && !token.ends_with('-')
                        && (if require_dash {
                            token.contains('-')
                        } else {
                            token.len() >= 3
                        });
                    if shaped {
                        out.insert(token.to_owned());
                    }
                }
            }
            i = j + 1;
        } else {
            i += 1;
        }
    }
}

/// Class names wrapped in a CSS Modules `:global(...)` selector. Such a class is
/// applied by code OUTSIDE this stylesheet, most often a third-party library's
/// runtime DOM that the module styles via an escape hatch (an antd
/// `.validatiemeldingenModal :global(.ant-modal-header)` override). The project's
/// own markup never writes that class, so it can never be credited and would
/// always surface as a (false) unreferenced-class candidate. `:global` is the
/// author's explicit "not locally scoped, applied elsewhere" marker, so excluding
/// these from the candidate set is semantically correct, not a heuristic guess.
fn collect_global_scoped_classes(source: &str, out: &mut rustc_hash::FxHashSet<String>) {
    let bytes = source.as_bytes();
    let mut i = 0;
    while let Some(rel) = source[i..].find(":global(") {
        let open = i + rel + ":global(".len();
        // Balance parentheses so a `:global(:is(.a, .b))` still closes correctly.
        let mut depth = 1usize;
        let mut j = open;
        while j < bytes.len() && depth > 0 {
            match bytes[j] {
                b'(' => depth += 1,
                b')' => depth -= 1,
                _ => {}
            }
            j += 1;
        }
        let inner_end = j.saturating_sub(1).max(open);
        if let Some(inner) = source.get(open..inner_end) {
            extract_dotted_class_names(inner, out);
        }
        i = j.max(open + 1);
    }
}

/// Push every `.class` token in a CSS selector fragment (the bare name, no dot)
/// into `out`. A class name is a dot followed by `[A-Za-z_-]` then any run of
/// `[A-Za-z0-9_-]`.
fn extract_dotted_class_names(selector: &str, out: &mut rustc_hash::FxHashSet<String>) {
    let bytes = selector.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'.' {
            let start = i + 1;
            if start < bytes.len()
                && (bytes[start].is_ascii_alphabetic() || matches!(bytes[start], b'_' | b'-'))
            {
                let mut j = start;
                while j < bytes.len()
                    && (bytes[j].is_ascii_alphanumeric() || matches!(bytes[j], b'_' | b'-'))
                {
                    j += 1;
                }
                if let Some(name) = selector.get(start..j) {
                    out.insert(name.to_owned());
                }
                i = j;
                continue;
            }
        }
        i += 1;
    }
}

/// Whole-project CSS class surfaces shared by the markup candidate passes.
#[derive(Clone, Debug, Default)]
pub(super) struct CssClassInventory {
    css_files: usize,
    preprocessor_files: usize,
    typo_target_classes: rustc_hash::FxHashSet<String>,
    defined_classes: Vec<(String, Vec<(String, u32)>)>,
}

/// Collect both class surfaces in one file pass. The typo-target set includes
/// every authored class from standalone stylesheets and Astro/Vue/Svelte style
/// blocks. The located standalone set powers the unreferenced-global-class
/// check and omits `:global(...)` selectors.
///
/// The typo-target surface is not narrowed by `changed_files` or workspace
/// roots. A definition in an unchanged file must still suppress an unresolved
/// reference. Only the health ignore filter applies.
pub(super) fn css_class_inventory(
    files: &[fallow_types::discover::DiscoveredFile],
    config: &ResolvedConfig,
    ignore_set: &globset::GlobSet,
) -> CssClassInventory {
    use fallow_types::extract::ExportName;
    let mut inventory = CssClassInventory::default();
    for file in files {
        let path = &file.path;
        let extension = path.extension().and_then(|ext| ext.to_str());
        let is_preprocessor = matches!(extension, Some("scss" | "sass" | "less"));
        let is_css = extension == Some("css") || is_preprocessor;
        let has_style_blocks = matches!(extension, Some("astro" | "vue" | "svelte"));
        if !is_css && !has_style_blocks {
            continue;
        }
        let relative = path.strip_prefix(&config.root).unwrap_or(path);
        if ignore_set.is_match(relative) {
            continue;
        }
        if extension == Some("css") {
            inventory.css_files += 1;
        } else if is_preprocessor {
            inventory.preprocessor_files += 1;
        }
        let Ok(source) = std::fs::read_to_string(path) else {
            continue;
        };
        if has_style_blocks {
            for style in crate::css::extract_sfc_styles(&source) {
                let is_style_scss = style
                    .lang
                    .as_deref()
                    .is_some_and(|lang| matches!(lang, "scss" | "sass"));
                for export in crate::css::extract_css_module_exports(&style.body, is_style_scss) {
                    if let ExportName::Named(name) = export.name {
                        inventory.typo_target_classes.insert(name);
                    }
                }
            }
            continue;
        }
        let mut global_scoped: rustc_hash::FxHashSet<String> = rustc_hash::FxHashSet::default();
        collect_global_scoped_classes(&source, &mut global_scoped);
        let mut seen: rustc_hash::FxHashSet<String> = rustc_hash::FxHashSet::default();
        let mut classes: Vec<(String, u32)> = Vec::new();
        for export in crate::css::extract_css_module_exports(&source, is_preprocessor) {
            let ExportName::Named(name) = export.name else {
                continue;
            };
            inventory.typo_target_classes.insert(name.clone());
            // A `:global(.foo)` override targets DOM applied outside this module
            // (a third-party library's runtime markup), so it is never authored in
            // project markup and must not be an unreferenced-class candidate.
            if global_scoped.contains(&name) {
                continue;
            }
            if !seen.insert(name.clone()) {
                continue;
            }
            let start = export.span.start as usize;
            let line = 1 + source
                .get(..start)
                .map_or(0, |s| s.bytes().filter(|&b| b == b'\n').count());
            classes.push((name, u32::try_from(line).unwrap_or(u32::MAX)));
        }
        if !classes.is_empty() {
            inventory
                .defined_classes
                .push((relative.to_string_lossy().replace('\\', "/"), classes));
        }
    }
    inventory
}

/// Scan for global CSS classes referenced by NO in-project markup (the CSS
/// analogue of an unused export). Heavily gated to stay near-zero-false-positive:
///
/// - **Partial scope** (`changed_files` / `ws_roots`): abstain. A partial markup
///   view cannot prove a global class dead.
/// - **Preprocessor-dominant** (`.scss`/`.sass`/`.less` outnumber plain `.css`):
///   abstain. The parser cannot expand loops/mixins, so the markup-vs-CSS join
///   is unreliable.
/// - **Published surface**: a stylesheet reachable from `package.json` entries,
///   or whose classes are referenced by zero in-project markup (a design system
///   consumed elsewhere), abstains entirely.
/// - **Reference test** (panel gate 1): a class is referenced if it is a whole
///   static markup token OR a substring of any dynamic-class source, so a class
///   assembled from a `${...}` / `clsx(...)` fragment is never flagged.
pub(super) fn scan_unreferenced_css_classes(
    files: &[fallow_types::discover::DiscoveredFile],
    ctx: HealthScanCtx<'_>,
    summary: &mut fallow_output::CssAnalyticsSummary,
    reference_surface: Option<&CssReferenceSurface>,
    class_inventory: Option<&CssClassInventory>,
) -> Vec<fallow_output::UnreferencedCssClass> {
    let HealthScanCtx {
        config,
        ignore_set,
        changed_files,
        output_changed_files: _,
        ws_roots,
    } = ctx;

    use fallow_output::UnreferencedCssClass;

    // Partial scope cannot prove a global class dead.
    if changed_files.is_some() || ws_roots.is_some() {
        return Vec::new();
    }
    // Preprocessor-dominant projects have an unreliable defined/used join.
    let fallback_class_inventory;
    let class_inventory = if let Some(inventory) = class_inventory {
        inventory
    } else {
        fallback_class_inventory = css_class_inventory(files, config, ignore_set);
        &fallback_class_inventory
    };
    let css_files = class_inventory.css_files;
    let preprocessor_files = class_inventory.preprocessor_files;
    if preprocessor_files > css_files {
        return Vec::new();
    }

    let fallback_reference_surface;
    let reference_surface = if let Some(surface) = reference_surface {
        surface
    } else {
        fallback_reference_surface = css_reference_surface(files, config, ignore_set);
        &fallback_reference_surface
    };

    let published = published_css_paths(config);
    let dependency_prefixes = dependency_class_prefixes(config);

    let mut out: Vec<UnreferencedCssClass> = Vec::new();
    for (rel, classes) in &class_inventory.defined_classes {
        push_unreferenced_css_class_candidates(
            &mut out,
            rel,
            classes.clone(),
            &published,
            &dependency_prefixes,
            reference_surface,
        );
    }

    out.sort_by(|a, b| {
        a.path
            .cmp(&b.path)
            .then_with(|| a.line.cmp(&b.line))
            .then_with(|| a.class.cmp(&b.class))
    });
    summary.unreferenced_css_classes = saturate_len(out.len());
    out
}

#[derive(Clone, Debug)]
pub(super) struct CssReferenceSurface {
    static_tokens: rustc_hash::FxHashSet<String>,
    dynamic_class_names: rustc_hash::FxHashSet<String>,
    dynamic_corpus: String,
    css_module_dot_properties: SortedPrefixLookup,
    css_module_bracket_properties: rustc_hash::FxHashSet<String>,
    dynamic_prefixes_reversed: SortedPrefixLookup,
    dynamic_literals: rustc_hash::FxHashSet<String>,
}

#[derive(Clone, Debug, Default)]
struct SortedPrefixLookup {
    values: Vec<String>,
}

impl SortedPrefixLookup {
    fn new(mut values: Vec<String>) -> Self {
        values.sort_unstable();
        values.dedup();
        Self { values }
    }

    fn contains_prefix(&self, prefix: &str) -> bool {
        let index = self.values.partition_point(|value| value.as_str() < prefix);
        self.values
            .get(index)
            .is_some_and(|value| value.starts_with(prefix))
    }
}

#[derive(Debug, Default)]
struct CssReferenceSurfaceBuilder {
    static_tokens: rustc_hash::FxHashSet<String>,
    dynamic_corpus: String,
    source_corpus: String,
    dynamic_interpolants: rustc_hash::FxHashSet<String>,
}

impl CssReferenceSurfaceBuilder {
    fn finish(self) -> CssReferenceSurface {
        let dynamic_class_names = collect_class_name_tokens(&self.dynamic_corpus);
        let mut css_module_dot_properties = Vec::new();
        let mut css_module_bracket_properties = rustc_hash::FxHashSet::default();
        collect_css_module_properties(
            &self.source_corpus,
            &mut css_module_dot_properties,
            &mut css_module_bracket_properties,
        );
        let dynamic_prefixes_reversed = collect_dynamic_prefixes(&self.dynamic_corpus);
        let dynamic_literals =
            collect_dynamic_literals(&self.source_corpus, &self.dynamic_interpolants);

        CssReferenceSurface {
            static_tokens: self.static_tokens,
            dynamic_class_names,
            dynamic_corpus: self.dynamic_corpus,
            css_module_dot_properties: SortedPrefixLookup::new(css_module_dot_properties),
            css_module_bracket_properties,
            dynamic_prefixes_reversed: SortedPrefixLookup::new(dynamic_prefixes_reversed),
            dynamic_literals,
        }
    }
}

impl CssReferenceSurface {
    fn references(&self, class: &str) -> bool {
        self.static_tokens.contains(class)
            || self.dynamic_class_referenced(class)
            || self.css_module_property_referenced(class)
            || self.dynamic_prefix_referenced(class)
            || self.dynamic_literal_referenced(class)
    }

    fn dynamic_class_referenced(&self, class: &str) -> bool {
        if class.bytes().all(is_class_name_byte) {
            return self.dynamic_class_names.contains(class);
        }
        class_name_occurrences(&self.dynamic_corpus, class)
            .next()
            .is_some()
    }

    fn css_module_property_referenced(&self, class: &str) -> bool {
        let Some(alias) = css_module_property_alias(class) else {
            return false;
        };
        self.css_module_dot_properties.contains_prefix(&alias)
            || self.css_module_bracket_properties.contains(&alias)
    }

    fn dynamic_prefix_referenced(&self, class: &str) -> bool {
        let Some(dash) = class.rfind('-') else {
            return false;
        };
        let head = &class[..=dash];
        if head.bytes().all(is_class_name_byte) {
            let reversed: String = head.bytes().rev().map(char::from).collect();
            return self.dynamic_prefixes_reversed.contains_prefix(&reversed);
        }
        INTERP_MARKERS
            .iter()
            .any(|marker| self.dynamic_corpus.contains(&format!("{head}{marker}")))
    }

    fn dynamic_literal_referenced(&self, class: &str) -> bool {
        is_plain_dynamic_class_value(class) && self.dynamic_literals.contains(class)
    }
}

const INTERP_MARKERS: [&str; 6] = ["${", "' +", "'+", "\" +", "\"+", "` +"];

fn css_module_property_alias(class: &str) -> Option<String> {
    if !class.contains('-') {
        return None;
    }
    let mut alias = String::with_capacity(class.len());
    let mut uppercase_next = false;
    for c in class.chars() {
        if c == '-' {
            uppercase_next = true;
            continue;
        }
        if uppercase_next {
            alias.extend(c.to_uppercase());
            uppercase_next = false;
        } else {
            alias.push(c);
        }
    }
    (alias != class && is_valid_js_property_ident(&alias)).then_some(alias)
}

fn is_valid_js_property_ident(value: &str) -> bool {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first == '_' || first == '$' || first.is_ascii_alphabetic())
        && chars.all(|c| c == '_' || c == '$' || c.is_ascii_alphanumeric())
}

fn is_plain_dynamic_class_value(class: &str) -> bool {
    class.len() >= MIN_UNREF_CLASS_LEN
        && class
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

fn class_name_occurrences<'a>(source: &'a str, class: &'a str) -> impl Iterator<Item = usize> + 'a {
    source.match_indices(class).filter_map(move |(offset, _)| {
        let before = source.as_bytes().get(offset.wrapping_sub(1)).copied();
        let after = source.as_bytes().get(offset + class.len()).copied();
        if before.is_some_and(is_class_name_byte) || after.is_some_and(is_class_name_byte) {
            None
        } else {
            Some(offset)
        }
    })
}

fn is_class_name_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_'
}

fn collect_class_name_tokens(source: &str) -> rustc_hash::FxHashSet<String> {
    let bytes = source.as_bytes();
    let mut out = rustc_hash::FxHashSet::default();
    let mut start = 0usize;
    while start < bytes.len() {
        if !is_class_name_byte(bytes[start]) {
            start += 1;
            continue;
        }
        let mut end = start + 1;
        while bytes.get(end).is_some_and(|byte| is_class_name_byte(*byte)) {
            end += 1;
        }
        out.insert(source[start..end].to_owned());
        start = end;
    }
    out
}

fn collect_css_module_properties(
    source: &str,
    dot_properties: &mut Vec<String>,
    bracket_properties: &mut rustc_hash::FxHashSet<String>,
) {
    let bytes = source.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'.'
            && bytes
                .get(i + 1)
                .is_some_and(|byte| is_js_identifier_start(*byte))
        {
            let start = i + 1;
            let mut end = start + 1;
            while bytes
                .get(end)
                .is_some_and(|byte| is_js_identifier_continue(*byte))
            {
                end += 1;
            }
            dot_properties.push(source[start..end].to_owned());
            i = end;
            continue;
        }
        if bytes[i] == b'[' && matches!(bytes.get(i + 1), Some(b'\'' | b'"')) {
            let quote = bytes[i + 1];
            let start = i + 2;
            let mut end = start;
            while bytes.get(end).is_some_and(|byte| *byte != quote) {
                end += 1;
            }
            if bytes.get(end) == Some(&quote)
                && bytes.get(end + 1) == Some(&b']')
                && source
                    .get(start..end)
                    .is_some_and(is_valid_js_property_ident)
            {
                bracket_properties.insert(source[start..end].to_owned());
            }
        }
        i += 1;
    }
}

fn collect_dynamic_prefixes(source: &str) -> Vec<String> {
    let bytes = source.as_bytes();
    let mut out = Vec::new();
    for marker in INTERP_MARKERS {
        for (end, _) in source.match_indices(marker) {
            let mut start = end;
            while start > 0 && is_class_name_byte(bytes[start - 1]) {
                start -= 1;
            }
            if start < end {
                out.push(source[start..end].bytes().rev().map(char::from).collect());
            }
        }
    }
    out
}

fn collect_dynamic_literals(
    source: &str,
    interpolants: &rustc_hash::FxHashSet<String>,
) -> rustc_hash::FxHashSet<String> {
    if interpolants.is_empty() {
        return rustc_hash::FxHashSet::default();
    }
    let bytes = source.as_bytes();
    let mut out = rustc_hash::FxHashSet::default();
    for (quote_offset, quote) in bytes.iter().copied().enumerate() {
        if !matches!(quote, b'\'' | b'"' | b'`') {
            continue;
        }
        let offset = quote_offset + 1;
        let mut end = offset;
        while bytes.get(end).is_some_and(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'_')
        }) {
            end += 1;
        }
        let Some(class) = source.get(offset..end) else {
            continue;
        };
        if !is_plain_dynamic_class_value(class)
            || !matches!(
                bytes.get(end),
                Some(b'\'' | b'"' | b'`' | b',' | b';' | b')' | b']' | b'}')
            )
        {
            continue;
        }
        let window_start = offset.saturating_sub(120);
        let window_end = source.len().min(end + 120);
        let Some(window) = source.get(window_start..window_end) else {
            continue;
        };
        if interpolants
            .iter()
            .any(|name| contains_ignore_ascii_case(window, name))
        {
            out.insert(class.to_owned());
        }
    }
    out
}

fn contains_ignore_ascii_case(haystack: &str, needle: &str) -> bool {
    haystack
        .as_bytes()
        .windows(needle.len())
        .any(|window| window.eq_ignore_ascii_case(needle.as_bytes()))
}

fn collect_dynamic_class_interpolants(source: &str, out: &mut rustc_hash::FxHashSet<String>) {
    let bytes = source.as_bytes();
    let mut i = 0usize;
    while let Some(rel) = source.get(i..).and_then(|tail| tail.find("${")) {
        let start = i + rel + 2;
        let mut name_start = start;
        while bytes
            .get(name_start)
            .is_some_and(|b| b.is_ascii_whitespace())
        {
            name_start += 1;
        }
        let Some(first) = bytes.get(name_start).copied() else {
            break;
        };
        if !is_js_identifier_start(first) {
            i = start;
            continue;
        }
        let mut name_end = name_start + 1;
        while bytes
            .get(name_end)
            .is_some_and(|b| is_js_identifier_continue(*b))
        {
            name_end += 1;
        }
        let mut cursor = name_end;
        while bytes.get(cursor).is_some_and(|b| b.is_ascii_whitespace()) {
            cursor += 1;
        }
        if bytes.get(cursor) == Some(&b'}') {
            out.insert(source[name_start..name_end].to_owned());
        }
        i = cursor.saturating_add(1);
    }
}

fn is_js_identifier_start(byte: u8) -> bool {
    byte.is_ascii_alphabetic() || byte == b'_' || byte == b'$'
}

fn is_js_identifier_continue(byte: u8) -> bool {
    is_js_identifier_start(byte) || byte.is_ascii_digit()
}

pub(super) fn css_reference_surface(
    files: &[fallow_types::discover::DiscoveredFile],
    config: &ResolvedConfig,
    ignore_set: &globset::GlobSet,
) -> CssReferenceSurface {
    let mut surface = CssReferenceSurfaceBuilder::default();
    for file in files {
        collect_css_reference_surface_file(&mut surface, file, config, ignore_set);
    }
    collect_markdown_reference_surface_files(&mut surface, config, ignore_set);
    surface.finish()
}

fn collect_css_reference_surface_file(
    surface: &mut CssReferenceSurfaceBuilder,
    file: &fallow_types::discover::DiscoveredFile,
    config: &ResolvedConfig,
    ignore_set: &globset::GlobSet,
) {
    let path = &file.path;
    let extension = path.extension().and_then(|ext| ext.to_str());
    if !matches!(extension, Some("js" | "ts" | "mjs" | "cjs"))
        && !extension.is_some_and(is_markup_source_extension)
    {
        return;
    }
    let relative = path.strip_prefix(&config.root).unwrap_or(path);
    if ignore_set.is_match(relative) {
        return;
    }
    let Ok(source) = std::fs::read_to_string(path) else {
        return;
    };
    surface.source_corpus.push_str(&source);
    surface.source_corpus.push('\n');
    let is_markup_surface = extension.is_some_and(is_markup_source_extension);
    if !is_markup_surface {
        return;
    }
    let scan = crate::css::scan_markup_class_tokens(&source);
    for token in scan.static_tokens {
        surface.static_tokens.insert(token.value);
    }
    collect_quoted_class_tokens(&source, &mut surface.static_tokens, true);
    if scan.has_dynamic {
        collect_dynamic_class_interpolants(&source, &mut surface.dynamic_interpolants);
        surface.dynamic_corpus.push_str(&source);
        surface.dynamic_corpus.push('\n');
    }
}

fn collect_markdown_reference_surface_files(
    surface: &mut CssReferenceSurfaceBuilder,
    config: &ResolvedConfig,
    ignore_set: &globset::GlobSet,
) {
    collect_markdown_reference_surface_dir(surface, &config.root, config, ignore_set);
}

fn collect_markdown_reference_surface_dir(
    surface: &mut CssReferenceSurfaceBuilder,
    dir: &std::path::Path,
    config: &ResolvedConfig,
    ignore_set: &globset::GlobSet,
) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let relative = path.strip_prefix(&config.root).unwrap_or(&path);
        if ignore_set.is_match(relative) || is_skipped_markdown_reference_path(relative) {
            continue;
        }
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_dir() {
            collect_markdown_reference_surface_dir(surface, &path, config, ignore_set);
            continue;
        }
        let extension = path.extension().and_then(|ext| ext.to_str());
        if !matches!(extension, Some("md" | "mdx")) {
            continue;
        }
        let Ok(source) = std::fs::read_to_string(&path) else {
            continue;
        };
        surface.source_corpus.push_str(&source);
        surface.source_corpus.push('\n');
        let scan = crate::css::scan_markup_class_tokens(&source);
        for token in scan.static_tokens {
            surface.static_tokens.insert(token.value);
        }
        collect_quoted_class_tokens(&source, &mut surface.static_tokens, true);
        if scan.has_dynamic {
            collect_dynamic_class_interpolants(&source, &mut surface.dynamic_interpolants);
            surface.dynamic_corpus.push_str(&source);
            surface.dynamic_corpus.push('\n');
        }
    }
}

fn is_skipped_markdown_reference_path(relative: &std::path::Path) -> bool {
    relative.components().any(|component| {
        let std::path::Component::Normal(name) = component else {
            return false;
        };
        matches!(
            name.to_str(),
            Some(
                "node_modules"
                    | ".git"
                    | ".next"
                    | ".nuxt"
                    | ".svelte-kit"
                    | "dist"
                    | "build"
                    | "target"
                    | "coverage"
                    | ".turbo"
                    | ".cache"
            )
        )
    })
}

pub(super) fn is_markup_source_extension(extension: &str) -> bool {
    matches!(
        extension,
        "jsx" | "tsx" | "html" | "astro" | "vue" | "svelte" | "md" | "mdx"
    )
}

fn push_unreferenced_css_class_candidates(
    out: &mut Vec<fallow_output::UnreferencedCssClass>,
    rel: &str,
    classes: Vec<(String, u32)>,
    published: &rustc_hash::FxHashSet<String>,
    dependency_prefixes: &rustc_hash::FxHashSet<String>,
    reference_surface: &CssReferenceSurface,
) {
    use fallow_output::{CssCandidateAction, UnreferencedCssClass};

    if published.contains(rel)
        || !classes
            .iter()
            .any(|(class, _)| reference_surface.references(class))
    {
        return;
    }
    for (class, line) in classes {
        if class.len() >= MIN_UNREF_CLASS_LEN
            && !reference_surface.references(&class)
            && !class_matches_dependency_prefix(&class, dependency_prefixes)
        {
            out.push(UnreferencedCssClass {
                actions: vec![CssCandidateAction::verify_unreferenced_class(&class)],
                class,
                path: rel.to_string(),
                line,
            });
        }
    }
}

#[cfg(test)]
#[expect(
    clippy::unwrap_used,
    reason = "tests use unwrap to keep reference-surface fixtures concise"
)]
mod tests {
    use super::*;
    use fallow_config::{FallowConfig, OutputFormat};
    use fallow_types::discover::{DiscoveredFile, FileId};

    fn reference_surface(sources: &[(&str, &str)]) -> CssReferenceSurface {
        let dir = tempfile::tempdir().unwrap();
        let files: Vec<DiscoveredFile> = sources
            .iter()
            .enumerate()
            .map(|(index, (relative, source))| {
                let path = dir.path().join(relative);
                if let Some(parent) = path.parent() {
                    std::fs::create_dir_all(parent).unwrap();
                }
                std::fs::write(&path, source).unwrap();
                DiscoveredFile {
                    id: FileId(u32::try_from(index).unwrap()),
                    path,
                    size_bytes: u64::try_from(source.len()).unwrap(),
                }
            })
            .collect();
        let config = FallowConfig::default().resolve(
            dir.path().to_path_buf(),
            OutputFormat::Human,
            1,
            true,
            true,
            None,
        );
        css_reference_surface(&files, &config, &globset::GlobSet::empty())
    }

    #[test]
    fn dynamic_reference_index_preserves_tokens_prefixes_and_boundaries() {
        let surface = reference_surface(&[
            ("src/state.ts", "export const state = 'selected';"),
            (
                "src/Card.tsx",
                r"
                    const className = ready ? 'reactive' : fallback;
                    const tone = 'danger';
                    export const Card = () => (
                        <div className={`${className} notice-${tone}`} />
                    );
                ",
            ),
        ]);

        assert!(surface.references("reactive"));
        assert!(surface.references("notice-danger"));
        assert!(surface.references("selected"));
        assert!(!surface.references("active"));
        assert!(!surface.references("other-danger"));
        assert!(!surface.references("selecting"));
    }

    #[test]
    fn css_module_property_index_preserves_conservative_prefix_matching() {
        let surface = reference_surface(&[(
            "src/styles.ts",
            r"
                export const primary = styles.buttonPrimaryExtra;
                export const navigation = styles['navigationItem'];
            ",
        )]);

        assert!(surface.references("button-primary"));
        assert!(surface.references("navigation-item"));
        assert!(!surface.references("button-secondary"));
        assert!(!surface.references("navigation-link"));
    }
}
