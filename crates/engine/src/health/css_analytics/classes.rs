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

/// Count plain-CSS vs preprocessor (`.scss`/`.sass`/`.less`) stylesheet files in
/// the project (ignore-filtered). Used to abstain from class-typo detection when
/// preprocessors dominate, because the parser cannot expand their loops/mixins,
/// so the defined-class set is unreliable.
fn count_stylesheet_kinds(
    files: &[fallow_types::discover::DiscoveredFile],
    config: &ResolvedConfig,
    ignore_set: &globset::GlobSet,
) -> (usize, usize) {
    let mut css = 0usize;
    let mut preprocessor = 0usize;
    for file in files {
        let path = &file.path;
        let kind = match path.extension().and_then(|ext| ext.to_str()) {
            Some("css") => &mut css,
            Some("scss" | "sass" | "less") => &mut preprocessor,
            _ => continue,
        };
        let relative = path.strip_prefix(&config.root).unwrap_or(path);
        if ignore_set.is_match(relative) {
            continue;
        }
        *kind += 1;
    }
    (css, preprocessor)
}

/// Collect every authored CSS class name defined anywhere in the project (plain
/// and module `.css`/`.scss`, plus Astro/SFC `<style>` blocks of any scoping). The set
/// is the typo-suggestion target for [`scan_unresolved_class_references`], so it
/// is NOT narrowed by `changed_files` / `ws_roots`: a class defined in an
/// unchanged file must still count as defined, or a markup token referencing it
/// would false-positive as unresolved. Only the ignore filter applies.
fn collect_defined_css_classes(
    files: &[fallow_types::discover::DiscoveredFile],
    config: &ResolvedConfig,
    ignore_set: &globset::GlobSet,
) -> rustc_hash::FxHashSet<String> {
    use fallow_types::extract::ExportName;
    let mut defined: rustc_hash::FxHashSet<String> = rustc_hash::FxHashSet::default();
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
                        defined.insert(name);
                    }
                }
            }
            continue;
        }
        for export in crate::css::extract_css_module_exports(&source, is_preprocessor) {
            if let ExportName::Named(name) = export.name {
                defined.insert(name);
            }
        }
    }
    defined
}

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
    let (css_files, preprocessor_files) = count_stylesheet_kinds(files, config, ignore_set);
    summary.preprocessor_stylesheets = saturate_len(preprocessor_files);
    if preprocessor_files > css_files {
        summary.preprocessor_reachability_abstained = true;
        return Vec::new();
    }

    let defined = collect_defined_css_classes(files, config, ignore_set);
    if defined.is_empty() {
        return Vec::new();
    }
    let by_len = build_typo_target_index(&defined);

    let mut out: Vec<UnresolvedClassReference> = Vec::new();
    let mut seen: rustc_hash::FxHashSet<(String, u32, String)> = rustc_hash::FxHashSet::default();
    for file in files {
        let Some((rel, source)) = read_markup_scan_source(file, ctx) else {
            continue;
        };
        collect_unresolved_class_refs_in_file(
            &source, &rel, &defined, &by_len, &mut seen, &mut out,
        );
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

/// Per-stylesheet located class definitions from STANDALONE `.css`/`.scss`/
/// `.sass`/`.less` files (not SFC `<style>` blocks, which are component-scoped
/// and covered by the scoped-unused check). Returns `(rel_path, [(class, 1-based
/// line)])`, each class deduped to its first definition. The defined surface for
/// the unreferenced-global-class candidate. Classes wrapped in `:global(...)`
/// are dropped: they target externally-applied DOM and are never authored in
/// markup.
fn collect_defined_css_classes_located(
    files: &[fallow_types::discover::DiscoveredFile],
    config: &ResolvedConfig,
    ignore_set: &globset::GlobSet,
) -> Vec<(String, Vec<(String, u32)>)> {
    use fallow_types::extract::ExportName;
    let mut out: Vec<(String, Vec<(String, u32)>)> = Vec::new();
    for file in files {
        let path = &file.path;
        let extension = path.extension().and_then(|ext| ext.to_str());
        let is_preprocessor = matches!(extension, Some("scss" | "sass" | "less"));
        if extension != Some("css") && !is_preprocessor {
            continue;
        }
        let relative = path.strip_prefix(&config.root).unwrap_or(path);
        if ignore_set.is_match(relative) {
            continue;
        }
        let Ok(source) = std::fs::read_to_string(path) else {
            continue;
        };
        let mut global_scoped: rustc_hash::FxHashSet<String> = rustc_hash::FxHashSet::default();
        collect_global_scoped_classes(&source, &mut global_scoped);
        let mut seen: rustc_hash::FxHashSet<String> = rustc_hash::FxHashSet::default();
        let mut classes: Vec<(String, u32)> = Vec::new();
        for export in crate::css::extract_css_module_exports(&source, is_preprocessor) {
            let ExportName::Named(name) = export.name else {
                continue;
            };
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
            out.push((relative.to_string_lossy().replace('\\', "/"), classes));
        }
    }
    out
}

#[derive(Clone, Debug)]
pub(super) struct CssClassInventory {
    css_files: usize,
    preprocessor_files: usize,
    defined_classes: Vec<(String, Vec<(String, u32)>)>,
}

pub(super) fn css_class_inventory(
    files: &[fallow_types::discover::DiscoveredFile],
    config: &ResolvedConfig,
    ignore_set: &globset::GlobSet,
) -> CssClassInventory {
    let (css_files, preprocessor_files) = count_stylesheet_kinds(files, config, ignore_set);
    CssClassInventory {
        css_files,
        preprocessor_files,
        defined_classes: collect_defined_css_classes_located(files, config, ignore_set),
    }
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
    dynamic_corpus: String,
    source_corpus: String,
    dynamic_interpolants: rustc_hash::FxHashSet<String>,
}

impl CssReferenceSurface {
    fn references(&self, class: &str) -> bool {
        self.static_tokens.contains(class)
            || class_name_occurrences(&self.dynamic_corpus, class)
                .next()
                .is_some()
            || self.css_module_property_referenced(class)
            || self.dynamic_prefix_referenced(class)
            || self.dynamic_literal_referenced(class)
    }

    fn css_module_property_referenced(&self, class: &str) -> bool {
        let Some(alias) = css_module_property_alias(class) else {
            return false;
        };
        self.source_corpus.contains(&format!(".{alias}"))
            || self.source_corpus.contains(&format!("['{alias}']"))
            || self.source_corpus.contains(&format!("[\"{alias}\"]"))
    }

    fn dynamic_prefix_referenced(&self, class: &str) -> bool {
        let Some(dash) = class.rfind('-') else {
            return false;
        };
        let head = &class[..=dash];
        const INTERP_MARKERS: [&str; 6] = ["${", "' +", "'+", "\" +", "\"+", "` +"];
        INTERP_MARKERS
            .iter()
            .any(|marker| self.dynamic_corpus.contains(&format!("{head}{marker}")))
    }

    fn dynamic_literal_referenced(&self, class: &str) -> bool {
        if !is_plain_dynamic_class_value(class) || self.dynamic_interpolants.is_empty() {
            return false;
        }
        class_literal_occurrences(&self.source_corpus, class).any(|offset| {
            let start = offset.saturating_sub(120);
            let end = self.source_corpus.len().min(offset + class.len() + 120);
            let Some(window) = self.source_corpus.get(start..end) else {
                return false;
            };
            let window = window.to_ascii_lowercase();
            self.dynamic_interpolants
                .iter()
                .any(|name| window.contains(&name.to_ascii_lowercase()))
        })
    }
}

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

fn class_literal_occurrences<'a>(
    source: &'a str,
    class: &'a str,
) -> impl Iterator<Item = usize> + 'a {
    source.match_indices(class).filter_map(move |(offset, _)| {
        let before = source.as_bytes().get(offset.wrapping_sub(1)).copied();
        let after = source.as_bytes().get(offset + class.len()).copied();
        match (before, after) {
            (Some(b'\''), Some(b'\'' | b',' | b';' | b')' | b']' | b'}'))
            | (Some(b'"'), Some(b'"' | b',' | b';' | b')' | b']' | b'}'))
            | (Some(b'`'), Some(b'`' | b',' | b';' | b')' | b']' | b'}')) => Some(offset),
            _ => None,
        }
    })
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
    let mut surface = CssReferenceSurface {
        static_tokens: rustc_hash::FxHashSet::default(),
        dynamic_corpus: String::new(),
        source_corpus: String::new(),
        dynamic_interpolants: rustc_hash::FxHashSet::default(),
    };
    for file in files {
        collect_css_reference_surface_file(&mut surface, file, config, ignore_set);
    }
    collect_markdown_reference_surface_files(&mut surface, config, ignore_set);
    surface
}

fn collect_css_reference_surface_file(
    surface: &mut CssReferenceSurface,
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
    surface: &mut CssReferenceSurface,
    config: &ResolvedConfig,
    ignore_set: &globset::GlobSet,
) {
    collect_markdown_reference_surface_dir(surface, &config.root, config, ignore_set);
}

fn collect_markdown_reference_surface_dir(
    surface: &mut CssReferenceSurface,
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
