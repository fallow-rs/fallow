use super::*;

/// Source-file extensions scanned for Tailwind utility-class-shaped tokens when
/// crediting `@theme` token usage. Mirrors the font-family source scan (markup,
/// JS/TS className strings / `clsx` args / CSS-in-JS, preprocessor stylesheets)
/// but deliberately EXCLUDES plain `.css`, which would re-read the `@theme`
/// DEFINITION and self-credit every token.
pub(super) const THEME_USAGE_SOURCE_EXTS: &[&str] = &[
    "scss", "sass", "less", "js", "jsx", "ts", "tsx", "mjs", "cjs", "vue", "svelte", "astro",
    "html", "mdx",
];

/// Collect every Tailwind-utility-shaped token from `source` into `out`: a
/// maximal run of `[a-z0-9-]` that, with leading/trailing `-` trimmed, still
/// contains a `-` and starts with a lowercase letter. Captures `bg-brand`,
/// `rounded-card`, `text-2xl`, and the `color-brand` core of a
/// `var(--color-brand)` / `[--color-brand]` reference. Deliberately captures the
/// dashed SHAPE, never a bare word, so a dictionary-word theme name
/// (`brand`/`card`/`muted`) is credited only by a real `-<name>` utility suffix,
/// not by the word appearing anywhere in source.
fn collect_class_shaped_tokens(source: &str, out: &mut rustc_hash::FxHashSet<String>) {
    let bytes = source.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' {
            let start = i;
            while i < bytes.len() {
                let c = bytes[i];
                if c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-' {
                    i += 1;
                } else {
                    break;
                }
            }
            let tok = source[start..i].trim_matches('-');
            if tok.contains('-') && tok.as_bytes().first().is_some_and(u8::is_ascii_lowercase) {
                out.insert(tok.to_owned());
            }
        } else {
            i += 1;
        }
    }
}

/// Location-aware sibling of [`collect_class_shaped_tokens`]: appends every
/// Tailwind-utility-shaped token in `source` to `out` as `(token, rel, line)`.
///
/// The scan visits each byte once and counts newlines as it passes them, so
/// the cost is linear in the source length. All tokens of the file share one
/// path allocation.
pub(super) fn collect_class_shaped_tokens_located(
    source: &str,
    rel: &str,
    out: &mut Vec<(String, std::sync::Arc<str>, u32)>,
) {
    let rel: std::sync::Arc<str> = std::sync::Arc::from(rel);
    let bytes = source.as_bytes();
    let mut line = 1u32;
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' {
            let start = i;
            while i < bytes.len() {
                let c = bytes[i];
                if c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-' {
                    i += 1;
                } else {
                    break;
                }
            }
            let tok = source[start..i].trim_matches('-');
            if tok.contains('-') && tok.as_bytes().first().is_some_and(u8::is_ascii_lowercase) {
                out.push((tok.to_owned(), std::sync::Arc::clone(&rel), line));
            }
        } else {
            #[cfg(test)]
            tests::note_line_byte();
            if b == b'\n' {
                line = line.saturating_add(1);
            }
            i += 1;
        }
    }
}

/// Tailwind v4 `@theme` design tokens (`--color-brand`, `--radius-card`) defined
/// in a stylesheet but used by no generated utility, `var()` read, `@apply`, or
/// arbitrary value anywhere in the project: dead design tokens (the
/// `unused-export` of the token era). Heavily gated to stay near-zero-false-
/// positive (panel BLOCKs):
///
/// - **Partial scope** (`changed_files` / `ws_roots`): abstain. A partial view
///   cannot prove a token dead.
/// - **v4 gate**: emit only when the project declares a `tailwindcss` dependency
///   AND at least one `@theme` token was found.
/// - **Tailwind plugin** (`@plugin` / config `plugins[]`): abstain. A plugin can
///   consume tokens invisibly to the scan (the DI blind spot).
/// - **Published library**: a token defined in a stylesheet that is a published
///   package surface is a public design-token API consumed downstream; skip it.
/// - **Variant namespaces** (`--breakpoint-*` / `--container-*`): excluded from
///   candidacy in this version. Crediting their `<name>:` / `@<name>:` variant
///   usage robustly needs a dedicated variant parser; a follow-up can add it.
///   (Acceptance criterion 7: excluded when the variant scan is not built.)
///
/// The usage test is false-negative-leaning by design: every check CREDITS usage,
/// so a genuinely-dead token is missed before a live one is flagged.
pub(super) struct UnusedThemeTokenScanInput<'a> {
    pub(super) tokens: &'a CssTokenSets,
    pub(super) files: &'a [fallow_types::discover::DiscoveredFile],
    pub(super) config: &'a ResolvedConfig,
    pub(super) ignore_set: &'a globset::GlobSet,
    pub(super) changed_files: Option<&'a rustc_hash::FxHashSet<std::path::PathBuf>>,
    pub(super) output_changed_files: Option<&'a rustc_hash::FxHashSet<std::path::PathBuf>>,
    pub(super) ws_roots: Option<&'a [std::path::PathBuf]>,
    pub(super) summary: &'a mut fallow_output::CssAnalyticsSummary,
}

/// A classified `@theme` token candidate (namespace + name + definition site)
/// surviving the variant / published-library / unknown-namespace filters.
pub(super) struct ThemeTokenCandidate {
    pub(super) token: String,
    pub(super) namespace: String,
    pub(super) name: String,
    pub(super) value: String,
    pub(super) path: String,
    pub(super) line: u32,
}

/// Classify the project's `@theme` token definers, dropping variant namespaces,
/// published-library stylesheets, and anything outside a known namespace.
pub(super) fn classify_theme_token_candidates(
    input: &UnusedThemeTokenScanInput<'_>,
) -> Vec<ThemeTokenCandidate> {
    classify_theme_token_candidates_from_tokens(input.tokens, input.config)
}

pub(super) fn classify_theme_token_candidates_from_tokens(
    tokens: &CssTokenSets,
    config: &ResolvedConfig,
) -> Vec<ThemeTokenCandidate> {
    let published = published_css_paths(config);
    let mut candidates: Vec<ThemeTokenCandidate> = Vec::new();
    for (raw, definition) in &tokens.theme_token_definers {
        if published.contains(&definition.path) {
            continue;
        }
        let Some(classified) = tailwind_theme::classify(raw) else {
            continue;
        };
        if classified.is_variant {
            continue;
        }
        candidates.push(ThemeTokenCandidate {
            token: format!("--{raw}"),
            namespace: classified.namespace,
            name: classified.name,
            value: definition.value.clone(),
            path: definition.path.clone(),
            line: definition.line,
        });
    }
    candidates
}

/// Build the utility-shaped usage surface: every class-shaped token from `@apply`
/// bodies plus non-CSS source (markup class attributes, `clsx` args, CSS-in-JS).
fn collect_theme_usage_tokens(
    input: &UnusedThemeTokenScanInput<'_>,
) -> rustc_hash::FxHashSet<String> {
    let mut utility_tokens: rustc_hash::FxHashSet<String> = rustc_hash::FxHashSet::default();
    for apply in &input.tokens.apply_tokens {
        collect_class_shaped_tokens(apply, &mut utility_tokens);
    }
    for file in input.files {
        let path = &file.path;
        let extension = path.extension().and_then(|ext| ext.to_str());
        if !extension.is_some_and(|ext| THEME_USAGE_SOURCE_EXTS.contains(&ext)) {
            continue;
        }
        let relative = path.strip_prefix(&input.config.root).unwrap_or(path);
        if input.ignore_set.is_match(relative) {
            continue;
        }
        if let Ok(source) = std::fs::read_to_string(path) {
            collect_class_shaped_tokens(&source, &mut utility_tokens);
        }
    }
    utility_tokens
}

/// The `var()` read surface: CSS-side `@theme` reads plus referenced custom
/// properties (leading dashes trimmed to the property key form).
fn collect_theme_var_reads(tokens: &CssTokenSets) -> rustc_hash::FxHashSet<String> {
    let mut var_reads: rustc_hash::FxHashSet<String> = tokens.theme_var_reads.clone();
    for referenced in &tokens.referenced_custom_props {
        var_reads.insert(referenced.trim_start_matches('-').to_owned());
    }
    var_reads
}

pub(super) fn scan_unused_theme_tokens(
    input: &mut UnusedThemeTokenScanInput<'_>,
) -> Vec<fallow_output::UnusedThemeToken> {
    use fallow_output::{CssCandidateAction, UnusedThemeToken};

    // Partial scope cannot prove a token dead.
    if input.changed_files.is_some() || input.ws_roots.is_some() {
        return Vec::new();
    }
    // v4 gate: a Tailwind dependency AND at least one @theme token present.
    if input.tokens.theme_token_definers.is_empty() || !project_uses_tailwind(&input.config.root) {
        return Vec::new();
    }
    // Tailwind-plugin abstain (DI blind spot).
    if project_uses_tailwind_plugin(input.tokens.any_plugin_directive, &input.config.root) {
        return Vec::new();
    }

    let candidates = classify_theme_token_candidates(input);
    if candidates.is_empty() {
        input.summary.unused_theme_tokens = 0;
        return Vec::new();
    }

    let utility_tokens = collect_theme_usage_tokens(input);
    let var_reads = collect_theme_var_reads(input.tokens);

    let mut out: Vec<UnusedThemeToken> = Vec::new();
    for candidate in candidates {
        let dash_name = format!("-{}", candidate.name);
        // The token's own custom-property key, used by the var() read test.
        let raw = candidate.token.trim_start_matches('-');
        let used = var_reads.contains(raw)
            || utility_tokens
                .iter()
                .any(|t| t.len() > dash_name.len() && t.ends_with(&dash_name));
        if used {
            continue;
        }
        out.push(UnusedThemeToken {
            actions: vec![CssCandidateAction::verify_unused_theme_token(
                &candidate.token,
                &candidate.namespace,
                &candidate.name,
            )],
            token: candidate.token,
            namespace: candidate.namespace,
            path: candidate.path,
            line: candidate.line,
        });
    }
    out.sort_by(|a, b| {
        a.path
            .cmp(&b.path)
            .then_with(|| a.line.cmp(&b.line))
            .then_with(|| a.token.cmp(&b.token))
    });
    input.summary.unused_theme_tokens = saturate_len(out.len());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    thread_local! {
        /// Bytes that the line count of the class token scan read on this
        /// thread, so a test can pin a linear cost.
        static LINE_BYTES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    }

    pub(super) fn note_line_byte() {
        LINE_BYTES.with(|read| read.set(read.get() + 1));
    }

    fn line_bytes_for(source: &str) -> (usize, usize) {
        LINE_BYTES.with(|read| read.set(0));
        let mut out = Vec::new();
        collect_class_shaped_tokens_located(source, "src/a.html", &mut out);
        (out.len(), LINE_BYTES.with(std::cell::Cell::get))
    }

    #[test]
    fn located_class_token_lines_read_each_byte_once() {
        use std::fmt::Write as _;
        let mut read = Vec::new();
        for count in [2000, 4000] {
            let mut source = String::from("<div>\n");
            for i in 0..count {
                let _ = write!(source, " text-{i}");
            }
            source.push_str("\n</div>\n");
            let (tokens, bytes) = line_bytes_for(&source);
            assert_eq!(tokens, count);
            // The line count reads each byte between tokens once. A prefix
            // rescan for each token reads about count * len / 2 bytes.
            let between_tokens = source
                .bytes()
                .filter(|b| !(b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'-'))
                .count();
            assert_eq!(bytes, between_tokens, "of {} bytes", source.len());
            read.push(bytes);
        }
        // Twice the tokens on the same line read about twice the bytes.
        assert!(read[1] <= 2 * read[0] + 16, "read {read:?}");
    }

    #[test]
    fn located_class_tokens_match_a_prefix_rescan() {
        use std::fmt::Write as _;
        let mut source = String::from("<div class=\"bg-brand\">\r\n");
        for i in 0..200 {
            let _ = write!(source, " text-{i} p-x");
        }
        source.push_str("\n\n-rounded-card- word\nlast-one é mt-2\n");

        let mut got = Vec::new();
        collect_class_shaped_tokens_located(&source, "src/a.html", &mut got);

        let mut want = Vec::new();
        let bytes = source.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            let start = i;
            while i < bytes.len()
                && (bytes[i].is_ascii_lowercase() || bytes[i].is_ascii_digit() || bytes[i] == b'-')
            {
                i += 1;
            }
            if i == start {
                i += 1;
                continue;
            }
            let tok = source[start..i].trim_matches('-');
            if tok.contains('-') && tok.as_bytes().first().is_some_and(u8::is_ascii_lowercase) {
                let line = 1 + source[..start].bytes().filter(|&b| b == b'\n').count();
                want.push((tok.to_owned(), u32::try_from(line).unwrap_or(u32::MAX)));
            }
        }

        let got_pairs: Vec<(String, u32)> = got
            .iter()
            .map(|(tok, _, line)| (tok.clone(), *line))
            .collect();
        assert_eq!(got_pairs, want);
        assert!(got.iter().all(|(_, rel, _)| &**rel == "src/a.html"));
        assert_eq!(got_pairs.first(), Some(&("bg-brand".to_owned(), 1)));
        assert_eq!(got_pairs.last(), Some(&("mt-2".to_owned(), 5)));
    }
}
