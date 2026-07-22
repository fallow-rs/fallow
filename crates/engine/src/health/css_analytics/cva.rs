use super::*;

pub(super) fn scan_cva_duplicate_variant_blocks(
    files: &[fallow_types::discover::DiscoveredFile],
    ctx: HealthScanCtx<'_>,
) -> Vec<fallow_output::CvaDuplicateVariantBlock> {
    let mut blocks: rustc_hash::FxHashMap<String, Vec<fallow_output::CssBlockOccurrence>> =
        rustc_hash::FxHashMap::default();
    for file in files {
        let Some((rel, source)) = read_js_style_scan_source(file, ctx) else {
            continue;
        };
        if !source_contains_cva_variants(&source) {
            continue;
        }
        for (value, line) in collect_cva_class_blocks(&source) {
            blocks
                .entry(value)
                .or_default()
                .push(fallow_output::CssBlockOccurrence {
                    path: rel.clone(),
                    line,
                });
        }
    }
    let mut out: Vec<_> = blocks
        .into_iter()
        .filter_map(|(value, mut occurrences)| {
            if occurrences.len() < 2 {
                return None;
            }
            occurrences.sort_by(|a, b| a.path.cmp(&b.path).then_with(|| a.line.cmp(&b.line)));
            let occurrence_count = saturate_len(occurrences.len());
            Some(fallow_output::CvaDuplicateVariantBlock {
                value,
                occurrence_count,
                occurrences,
                actions: vec![fallow_output::CssCandidateAction::consolidate_block(
                    occurrence_count,
                )],
            })
        })
        .collect();
    out.sort_by(|a, b| {
        b.occurrence_count
            .cmp(&a.occurrence_count)
            .then_with(|| {
                let a_key = a
                    .occurrences
                    .first()
                    .map_or(("", 0), |occ| (occ.path.as_str(), occ.line));
                let b_key = b
                    .occurrences
                    .first()
                    .map_or(("", 0), |occ| (occ.path.as_str(), occ.line));
                a_key.cmp(&b_key)
            })
            .then_with(|| a.value.cmp(&b.value))
    });
    out
}

pub(super) fn scan_cva_variant_token_drifts(
    files: &[fallow_types::discover::DiscoveredFile],
    ctx: HealthScanCtx<'_>,
    token_candidates: &[ComparableThemeTokenCandidate],
) -> Vec<fallow_output::CvaVariantTokenDrift> {
    if token_candidates.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut seen: rustc_hash::FxHashSet<(String, u32, String, String)> =
        rustc_hash::FxHashSet::default();
    for file in files {
        let Some((rel, source)) = read_js_style_scan_source(file, ctx) else {
            continue;
        };
        if !source_contains_cva_variants(&source) {
            continue;
        }
        collect_cva_file_token_drifts(&mut out, &mut seen, &rel, &source, token_candidates);
    }
    out.sort_by(|a, b| {
        a.path
            .cmp(&b.path)
            .then_with(|| a.line.cmp(&b.line))
            .then_with(|| a.class_token.cmp(&b.class_token))
            .then_with(|| a.nearest_token.name.cmp(&b.nearest_token.name))
    });
    out
}

fn collect_cva_file_token_drifts(
    out: &mut Vec<fallow_output::CvaVariantTokenDrift>,
    seen: &mut rustc_hash::FxHashSet<(String, u32, String, String)>,
    rel: &str,
    source: &str,
    token_candidates: &[ComparableThemeTokenCandidate],
) {
    for (variant_classes, line) in collect_cva_class_blocks(source) {
        for arbitrary in crate::css::scan_tailwind_arbitrary_values(&variant_classes) {
            let Some((namespace, value, metric)) = cva_arbitrary_value_metric(&arbitrary.value)
            else {
                continue;
            };
            let Some((nearest, distance)) =
                nearest_styling_token(namespace, &metric, token_candidates)
            else {
                continue;
            };
            let key = (
                rel.to_owned(),
                line,
                arbitrary.value.clone(),
                nearest.token.clone(),
            );
            if !seen.insert(key) {
                continue;
            }
            out.push(fallow_output::CvaVariantTokenDrift {
                class_token: arbitrary.value.clone(),
                value: value.clone(),
                variant_classes: variant_classes.clone(),
                path: rel.to_owned(),
                line,
                nearest_token: fallow_output::NearestStylingToken {
                    name: nearest.token.clone(),
                    value: nearest.value.clone(),
                    path: nearest.path.clone(),
                    line: nearest.line,
                    distance: round_distance(distance),
                },
                actions: vec![
                    fallow_output::CssCandidateAction::replace_cva_variant_arbitrary_value(
                        &arbitrary.value,
                        &nearest.token,
                    ),
                ],
            });
        }
    }
}

fn cva_arbitrary_value_metric(
    class_token: &str,
) -> Option<(&'static str, String, ThemeTokenMetric)> {
    let marker = "-[";
    let start = class_token.find(marker)?;
    let value_start = start + marker.len();
    let raw = class_token.get(value_start..class_token.len().checked_sub(1)?)?;
    let value = raw.replace('_', " ");
    let prefix = class_token.get(..start)?;
    let namespace = match prefix {
        "bg" | "border" | "fill" | "stroke" | "ring" | "outline" | "decoration" | "accent"
        | "caret" | "from" | "via" | "to" => "color",
        "text" if parse_theme_token_metric("color", &value).is_some() => "color",
        "text" => "text",
        "rounded" => "radius",
        "shadow" => "shadow",
        _ if prefix.starts_with("rounded-") => "radius",
        _ if prefix.starts_with("shadow-") => "shadow",
        _ => return None,
    };
    let metric = parse_theme_token_metric(namespace, &value)?;
    Some((namespace, value, metric))
}

fn nearest_styling_token<'a>(
    namespace: &str,
    metric: &ThemeTokenMetric,
    candidates: &'a [ComparableThemeTokenCandidate],
) -> Option<(&'a ComparableThemeTokenCandidate, f64)> {
    candidates
        .iter()
        .filter(|candidate| candidate.namespace == namespace)
        .filter_map(|candidate| {
            let distance = metric.distance(&candidate.metric)?;
            (distance <= metric.threshold()).then_some((candidate, distance))
        })
        .min_by(|(left, left_distance), (right, right_distance)| {
            left_distance
                .total_cmp(right_distance)
                .then_with(|| theme_token_sort_key(left).cmp(&theme_token_sort_key(right)))
        })
}

fn read_js_style_scan_source(
    file: &fallow_types::discover::DiscoveredFile,
    ctx: HealthScanCtx<'_>,
) -> Option<(String, String)> {
    let HealthScanCtx {
        config,
        ignore_set,
        changed_files,
        output_changed_files: _,
        ws_roots,
    } = ctx;
    let path = &file.path;
    let extension = path.extension().and_then(|ext| ext.to_str());
    if !matches!(extension, Some("js" | "jsx" | "ts" | "tsx" | "mjs" | "cjs")) {
        return None;
    }
    if path
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.ends_with(".d.ts"))
    {
        return None;
    }
    let path_text = path.to_string_lossy();
    if path_text.contains("__tests__")
        || path_text.contains("/test/")
        || path_text.contains("/tests/")
        || path_text.contains(".test.")
        || path_text.contains(".spec.")
    {
        return None;
    }
    let relative = path.strip_prefix(&config.root).unwrap_or(path);
    if ignore_set.is_match(relative) {
        return None;
    }
    if let Some(changed) = changed_files
        && !changed.contains(path)
    {
        return None;
    }
    if let Some(roots) = ws_roots
        && !roots.iter().any(|root| path.starts_with(root))
    {
        return None;
    }
    let source = std::fs::read_to_string(path).ok()?;
    let rel = relative.to_string_lossy().replace('\\', "/");
    Some((rel, source))
}

fn source_contains_cva_variants(source: &str) -> bool {
    source.contains("cva(")
        && source.contains("variants")
        && (source.contains("class-variance-authority") || source.contains("styled-system"))
}

fn collect_cva_class_blocks(source: &str) -> Vec<(String, u32)> {
    let mut out = Vec::new();
    let mut search = 0usize;
    while let Some(rel) = source[search..].find("cva(") {
        let start = search + rel;
        search = start + 4;
        if start > 0 && is_identifier_byte(source.as_bytes()[start - 1]) {
            continue;
        }
        let Some(end) = scan_call_end(source, start + 3) else {
            continue;
        };
        let base_line = source[..start].bytes().filter(|b| *b == b'\n').count() as u32 + 1;
        collect_quoted_cva_class_blocks(&source[start..end], base_line, &mut out);
    }
    out
}

fn is_identifier_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'$'
}

fn scan_call_end(source: &str, open_paren: usize) -> Option<usize> {
    let bytes = source.as_bytes();
    let mut i = open_paren;
    let mut depth = 0usize;
    let mut quote: Option<u8> = None;
    let mut escaped = false;
    while i < bytes.len() {
        let b = bytes[i];
        if let Some(q) = quote {
            if escaped {
                escaped = false;
            } else if b == b'\\' {
                escaped = true;
            } else if b == q {
                quote = None;
            }
            i += 1;
            continue;
        }
        if matches!(b, b'\'' | b'"' | b'`') {
            quote = Some(b);
            i += 1;
            continue;
        }
        if b == b'(' {
            depth += 1;
        } else if b == b')' {
            depth = depth.checked_sub(1)?;
            if depth == 0 {
                return Some(i + 1);
            }
        }
        i += 1;
    }
    None
}

fn collect_quoted_cva_class_blocks(source: &str, base_line: u32, out: &mut Vec<(String, u32)>) {
    let bytes = source.as_bytes();
    let mut i = 0;
    let mut line = base_line;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'\n' {
            line = line.saturating_add(1);
            i += 1;
            continue;
        }
        if !matches!(b, b'\'' | b'"' | b'`') {
            i += 1;
            continue;
        }
        let quote = b;
        let start_line = line;
        i += 1;
        let start = i;
        let mut escaped = false;
        while i < bytes.len() {
            let c = bytes[i];
            if c == b'\n' {
                line = line.saturating_add(1);
            }
            if escaped {
                escaped = false;
                i += 1;
                continue;
            }
            if c == b'\\' {
                escaped = true;
                i += 1;
                continue;
            }
            if c == quote {
                if let Some(block) = normalize_cva_class_block(&source[start..i]) {
                    out.push((block, start_line));
                }
                i += 1;
                break;
            }
            i += 1;
        }
    }
}

fn normalize_cva_class_block(value: &str) -> Option<String> {
    let tokens: Vec<_> = value.split_whitespace().collect();
    if tokens.len() < 3 {
        return None;
    }
    let class_like = tokens
        .iter()
        .filter(|token| {
            token.contains('-')
                || token.contains(':')
                || token.contains('[')
                || token.contains('/')
                || matches!(
                    **token,
                    "flex" | "grid" | "block" | "inline-flex" | "hidden"
                )
        })
        .count();
    (class_like >= 2).then(|| tokens.join(" "))
}

/// True for a byte that can appear inside a Tailwind class token (used to anchor
/// the `animate-` prefix at a token boundary so `xanimate-` does not match).
fn is_tailwind_class_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'-' || b == b'_'
}

/// Extract `@keyframes` names applied via Tailwind from one source string: the
/// custom-ident after `animate-[<name>_...]` (arbitrary value, up to the first
/// `_`/`]`) and after a bare `animate-<name>` utility. The `animate-` prefix must
/// sit at a token boundary. Names are collected raw; the caller filters them to
/// actually-defined keyframes.
fn collect_animate_keyframe_names(source: &str, out: &mut rustc_hash::FxHashSet<String>) {
    let bytes = source.as_bytes();
    const PREFIX: &str = "animate-";
    let mut search = 0;
    while let Some(rel) = source[search..].find(PREFIX) {
        let start = search + rel;
        search = start + PREFIX.len();
        // The prefix must start at a token boundary (`hover:animate-x` is fine,
        // `myanimate-x` is not).
        if start > 0 && is_tailwind_class_byte(bytes[start - 1]) {
            continue;
        }
        let after = start + PREFIX.len();
        if after >= bytes.len() {
            continue;
        }
        if bytes[after] == b'[' {
            // Arbitrary value: `animate-[badge-pop_0.5s_...]` -> `badge-pop`.
            let name_start = after + 1;
            let mut j = name_start;
            while j < bytes.len() {
                let c = bytes[j];
                if c == b'-' || c.is_ascii_alphanumeric() {
                    j += 1;
                } else {
                    break;
                }
            }
            if j > name_start {
                out.insert(source[name_start..j].to_owned());
            }
        } else {
            // Named utility: `animate-bar-fill` -> `bar-fill`.
            let mut j = after;
            while j < bytes.len() {
                let c = bytes[j];
                if c == b'-' || c.is_ascii_lowercase() || c.is_ascii_digit() {
                    j += 1;
                } else {
                    break;
                }
            }
            let name = source[after..j].trim_end_matches('-');
            if !name.is_empty() {
                out.insert(name.to_owned());
            }
        }
    }
}

/// Collect `@keyframes` names applied via Tailwind markup utilities
/// (`animate-[name_...]` / `animate-name`) across the project's markup and JS,
/// so a keyframe used only that way (never via a CSS `animation:` declaration)
/// is not wrongly flagged `unreferenced`. Not gated on the Tailwind dependency:
/// the `animate-[...]` / `animate-<name>` shapes are distinctive, the caller
/// filters the result to actually-defined keyframes, and a project can apply
/// Tailwind utilities without declaring the npm dep at the scanned root
/// (CDN / PostCSS / monorepo subpackage).
pub(super) fn collect_markup_keyframe_references(
    files: &[fallow_types::discover::DiscoveredFile],
    config: &ResolvedConfig,
    ignore_set: &globset::GlobSet,
) -> rustc_hash::FxHashSet<String> {
    let mut out: rustc_hash::FxHashSet<String> = rustc_hash::FxHashSet::default();
    for file in files {
        let path = &file.path;
        let extension = path.extension().and_then(|ext| ext.to_str());
        if !matches!(
            extension,
            Some("jsx" | "tsx" | "html" | "astro" | "vue" | "svelte" | "js" | "ts" | "mjs" | "cjs")
        ) {
            continue;
        }
        let relative = path.strip_prefix(&config.root).unwrap_or(path);
        if ignore_set.is_match(relative) {
            continue;
        }
        if let Ok(source) = std::fs::read_to_string(path) {
            collect_animate_keyframe_names(&source, &mut out);
            // Also a keyframe named in a JS inline-style `animation:` /
            // `animationName:` string (`animation: 'progress-indeterminate 1.5s'`)
            // appears as a dashed token in a quoted string; the caller filters
            // these to actually-defined keyframes, so an unrelated dashed token
            // can never manufacture a reference. `require_dash: false` so a
            // single-word keyframe name (`spin`, `jsanim`) is credited too.
            collect_quoted_class_tokens(&source, &mut out, false);
        }
    }
    out
}
