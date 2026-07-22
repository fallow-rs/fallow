use super::*;

const NEAR_DUPLICATE_COLOR_DISTANCE: f64 = 2.0;
const NEAR_DUPLICATE_LENGTH_DISTANCE_PX: f64 = 0.5;
const NEAR_DUPLICATE_DURATION_DISTANCE_MS: f64 = 10.0;
const NEAR_DUPLICATE_SHADOW_DISTANCE_PX: f64 = 1.0;

#[derive(Clone, Debug)]
pub(super) struct ComparableThemeTokenCandidate {
    pub(super) token: String,
    pub(super) namespace: String,
    pub(super) name: String,
    pub(super) value: String,
    pub(super) path: String,
    pub(super) line: u32,
    pub(super) metric: ThemeTokenMetric,
    pub(super) origin: ComparableTokenOrigin,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ComparableTokenOrigin {
    Explicit,
    ProjectVocabulary,
}

impl ComparableTokenOrigin {
    fn priority(self) -> u8 {
        match self {
            Self::Explicit => 0,
            Self::ProjectVocabulary => 1,
        }
    }
}

#[derive(Clone, Debug)]
pub(super) enum ThemeTokenMetric {
    Color(OklabColor),
    LengthPx(f64),
    DurationMs(f64),
    ShadowPx(Vec<f64>),
}

impl ThemeTokenMetric {
    pub(super) fn distance(&self, other: &Self) -> Option<f64> {
        match (self, other) {
            (Self::Color(left), Self::Color(right)) => Some(oklab_distance(*left, *right)),
            (Self::LengthPx(left), Self::LengthPx(right))
            | (Self::DurationMs(left), Self::DurationMs(right)) => Some((left - right).abs()),
            (Self::ShadowPx(left), Self::ShadowPx(right)) if left.len() == right.len() => Some(
                left.iter()
                    .zip(right)
                    .map(|(l, r)| {
                        let delta = l - r;
                        delta * delta
                    })
                    .sum::<f64>()
                    .sqrt(),
            ),
            _ => None,
        }
    }

    pub(super) fn threshold(&self) -> f64 {
        match self {
            Self::Color(_) => NEAR_DUPLICATE_COLOR_DISTANCE,
            Self::LengthPx(_) => NEAR_DUPLICATE_LENGTH_DISTANCE_PX,
            Self::DurationMs(_) => NEAR_DUPLICATE_DURATION_DISTANCE_MS,
            Self::ShadowPx(_) => NEAR_DUPLICATE_SHADOW_DISTANCE_PX,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) struct OklabColor {
    l: f64,
    a: f64,
    b: f64,
}

pub(super) fn scan_near_duplicate_theme_tokens(
    input: &mut UnusedThemeTokenScanInput<'_>,
) -> Vec<fallow_output::NearDuplicateThemeToken> {
    use fallow_output::{CssCandidateAction, NearDuplicateThemeToken, NearestStylingToken};

    if input.changed_files.is_some() || input.ws_roots.is_some() {
        return Vec::new();
    }
    if input.tokens.theme_token_definers.is_empty() || !project_uses_tailwind(&input.config.root) {
        return Vec::new();
    }
    if project_uses_tailwind_plugin(input.tokens.any_plugin_directive, &input.config.root) {
        return Vec::new();
    }

    let mut candidates = comparable_theme_token_candidates(input.tokens, input.config);
    candidates.sort_by(|a, b| theme_token_sort_key(a).cmp(&theme_token_sort_key(b)));
    if candidates.len() < 2 {
        return Vec::new();
    }

    let mut out = Vec::new();
    let changed = input.output_changed_files;
    for candidate in &candidates {
        if let Some(changed) = changed
            && !css_output_path_in_changed_scope(&candidate.path, input.config, changed)
        {
            continue;
        }
        let nearest = find_nearest_duplicate_theme_token(candidate, &candidates, changed.is_some());

        let Some((nearest, distance)) = nearest else {
            continue;
        };
        let distance = round_distance(distance);
        let nearest_token = NearestStylingToken {
            name: nearest.token.clone(),
            value: nearest.value.clone(),
            path: nearest.path.clone(),
            line: nearest.line,
            distance,
        };
        out.push(NearDuplicateThemeToken {
            token: candidate.token.clone(),
            value: candidate.value.clone(),
            path: candidate.path.clone(),
            line: candidate.line,
            actions: vec![CssCandidateAction::replace_near_duplicate_token(
                &candidate.token,
                &nearest.token,
            )],
            nearest_token,
        });
    }
    out.sort_by(|a, b| {
        a.path
            .cmp(&b.path)
            .then_with(|| a.line.cmp(&b.line))
            .then_with(|| a.token.cmp(&b.token))
    });
    input.summary.near_duplicate_theme_tokens = saturate_len(out.len());
    out
}

pub(super) struct NearDuplicateCssInJsTokenScanInput<'a> {
    pub(super) config: &'a ResolvedConfig,
    pub(super) changed_files: Option<&'a rustc_hash::FxHashSet<std::path::PathBuf>>,
    pub(super) output_changed_files: Option<&'a rustc_hash::FxHashSet<std::path::PathBuf>>,
    pub(super) ws_roots: Option<&'a [std::path::PathBuf]>,
    pub(super) summary: &'a mut fallow_output::CssAnalyticsSummary,
    pub(super) css_in_js_definers: Option<&'a CssInJsDefiners>,
}

pub(super) fn scan_near_duplicate_css_in_js_tokens(
    input: &mut NearDuplicateCssInJsTokenScanInput<'_>,
) -> Vec<fallow_output::NearDuplicateThemeToken> {
    use fallow_output::{CssCandidateAction, NearDuplicateThemeToken, NearestStylingToken};

    if input.changed_files.is_some() || input.ws_roots.is_some() {
        return Vec::new();
    }

    let mut candidates = comparable_css_in_js_token_candidates(input.css_in_js_definers);
    candidates.sort_by(|a, b| theme_token_sort_key(a).cmp(&theme_token_sort_key(b)));
    if candidates.len() < 2 {
        return Vec::new();
    }

    let mut out = Vec::new();
    for candidate in &candidates {
        if let Some(changed) = input.output_changed_files
            && !css_output_path_in_changed_scope(&candidate.path, input.config, changed)
        {
            continue;
        }
        let Some((nearest, distance)) = find_nearest_duplicate_theme_token(
            candidate,
            &candidates,
            input.output_changed_files.is_some(),
        ) else {
            continue;
        };
        let nearest_token = NearestStylingToken {
            name: nearest.token.clone(),
            value: nearest.value.clone(),
            path: nearest.path.clone(),
            line: nearest.line,
            distance: round_distance(distance),
        };
        out.push(NearDuplicateThemeToken {
            token: candidate.token.clone(),
            value: candidate.value.clone(),
            path: candidate.path.clone(),
            line: candidate.line,
            actions: vec![CssCandidateAction::replace_near_duplicate_token(
                &candidate.token,
                &nearest.token,
            )],
            nearest_token,
        });
    }
    out.sort_by(|a, b| {
        a.path
            .cmp(&b.path)
            .then_with(|| a.line.cmp(&b.line))
            .then_with(|| a.token.cmp(&b.token))
    });
    input.summary.near_duplicate_css_in_js_tokens = saturate_len(out.len());
    out
}

pub(super) fn annotate_raw_style_value_nearest_tokens(
    raw_style_values: &mut [fallow_output::RawStyleValue],
    candidates: &[ComparableThemeTokenCandidate],
) {
    if raw_style_values.is_empty() || candidates.is_empty() {
        return;
    }
    let raw_value_counts = raw_style_value_counts(raw_style_values);
    for raw in raw_style_values {
        let Some(namespace) = raw_style_token_namespace(&raw.axis) else {
            continue;
        };
        let Some(metric) = parse_theme_token_metric(namespace, &raw.value) else {
            continue;
        };
        let raw_value = normalize_theme_token_value(&raw.value);
        if namespace == "color" && color_value_has_alpha(&raw_value) {
            continue;
        }
        let raw_key = (namespace.to_string(), raw_value.clone());
        let raw_value_is_repeated = raw_value_counts.get(&raw_key).copied().unwrap_or(0) > 1;
        let nearest = candidates
            .iter()
            .filter(|candidate| candidate.namespace == namespace)
            .filter_map(|candidate| {
                if candidate.origin == ComparableTokenOrigin::ProjectVocabulary
                    && (raw_value == candidate.value || raw_value_is_repeated)
                {
                    return None;
                }
                let distance = metric.distance(&candidate.metric)?;
                (distance <= metric.threshold()).then_some((candidate, round_distance(distance)))
            })
            .min_by(|(left, left_distance), (right, right_distance)| {
                left_distance
                    .total_cmp(right_distance)
                    .then_with(|| left.origin.priority().cmp(&right.origin.priority()))
                    .then_with(|| theme_token_sort_key(left).cmp(&theme_token_sort_key(right)))
            });
        if let Some((nearest, distance)) = nearest {
            raw.nearest_token = Some(fallow_output::NearestStylingToken {
                name: nearest.token.clone(),
                value: nearest.value.clone(),
                path: nearest.path.clone(),
                line: nearest.line,
                distance,
            });
        }
    }
}

fn raw_style_value_counts(
    raw_values: &[fallow_output::RawStyleValue],
) -> rustc_hash::FxHashMap<(String, String), u32> {
    let mut counts = rustc_hash::FxHashMap::default();
    for raw in raw_values {
        let Some(namespace) = raw_style_token_namespace(&raw.axis) else {
            continue;
        };
        *counts
            .entry((
                namespace.to_string(),
                normalize_theme_token_value(&raw.value),
            ))
            .or_insert(0) += 1;
    }
    counts
}

pub(super) fn comparable_css_in_js_token_candidates(
    definers: Option<&CssInJsDefiners>,
) -> Vec<ComparableThemeTokenCandidate> {
    let Some(definers) = definers else {
        return Vec::new();
    };
    let mut candidates = Vec::new();
    for definer in &definers.entries {
        for leaf in &definer.leaves {
            let Some(value) = leaf.value.as_deref() else {
                continue;
            };
            let Some(namespace) = css_in_js_token_namespace(definer.origin, &leaf.path) else {
                continue;
            };
            let Some(metric) = parse_theme_token_metric(namespace, value) else {
                continue;
            };
            candidates.push(ComparableThemeTokenCandidate {
                token: format!("{}.{}", definer.binding, leaf.path),
                namespace: namespace.to_string(),
                name: leaf.path.clone(),
                value: normalize_theme_token_value(value),
                path: definer.rel_path.clone(),
                line: leaf.def_line,
                metric,
                origin: ComparableTokenOrigin::Explicit,
            });
        }
    }
    candidates
}

fn css_in_js_token_namespace(
    origin: fallow_extract::CssInJsTokenOrigin,
    path: &str,
) -> Option<&'static str> {
    let first = path.split('.').next().unwrap_or(path);
    let normalized = first.to_ascii_lowercase();
    match origin {
        fallow_extract::CssInJsTokenOrigin::Panda => match normalized.as_str() {
            "colors" | "color" => Some("color"),
            "fontsizes" | "font-sizes" | "text" => Some("text"),
            "radii" | "radius" | "radiitokens" | "border-radii" => Some("radius"),
            "shadows" | "shadow" => Some("shadow"),
            _ => None,
        },
        _ => match normalized.as_str() {
            "color" | "colors" | "palette" => Some("color"),
            "fontsize" | "fontsizes" | "font-size" | "text" => Some("text"),
            "radius" | "radii" | "borderradius" | "border-radius" => Some("radius"),
            "shadow" | "shadows" | "boxshadow" | "box-shadow" => Some("shadow"),
            _ => None,
        },
    }
}

fn raw_style_token_namespace(axis: &str) -> Option<&'static str> {
    match axis {
        "color" => Some("color"),
        "font-size" => Some("text"),
        "radius" => Some("radius"),
        "shadow" => Some("shadow"),
        _ => None,
    }
}

pub(super) fn comparable_custom_property_token_candidates(
    tokens: &CssTokenSets,
) -> Vec<ComparableThemeTokenCandidate> {
    tokens
        .custom_property_definers
        .iter()
        .filter_map(|(token, definition)| {
            let namespace = custom_property_token_namespace(token)?;
            let metric = parse_theme_token_metric(namespace, &definition.value)?;
            Some(ComparableThemeTokenCandidate {
                token: token.clone(),
                namespace: namespace.to_string(),
                name: token.trim_start_matches('-').to_owned(),
                value: normalize_theme_token_value(&definition.value),
                path: definition.path.clone(),
                line: definition.line,
                metric,
                origin: ComparableTokenOrigin::Explicit,
            })
        })
        .collect()
}

pub(super) fn comparable_project_vocabulary_candidates(
    tokens: &CssTokenSets,
) -> Vec<ComparableThemeTokenCandidate> {
    let mut groups: rustc_hash::FxHashMap<(String, String), ProjectVocabularyValue> =
        rustc_hash::FxHashMap::default();
    for raw in &tokens.raw_style_values {
        let Some(namespace) = raw_style_token_namespace(&raw.axis) else {
            continue;
        };
        let value = normalize_theme_token_value(&raw.value);
        if namespace == "color" && color_value_has_alpha(&value) {
            continue;
        }
        let Some(metric) = parse_theme_token_metric(namespace, &value) else {
            continue;
        };
        let key = (namespace.to_string(), value.clone());
        let entry = groups.entry(key).or_insert_with(|| ProjectVocabularyValue {
            namespace: namespace.to_string(),
            value,
            path: raw.path.clone(),
            line: raw.line,
            count: 0,
            metric,
        });
        entry.count += 1;
        if (raw.path.as_str(), raw.line) < (entry.path.as_str(), entry.line) {
            entry.path.clone_from(&raw.path);
            entry.line = raw.line;
        }
    }

    let mut candidates: Vec<ComparableThemeTokenCandidate> = groups
        .into_values()
        .filter(|value| value.count >= 2)
        .map(|value| ComparableThemeTokenCandidate {
            token: project_vocabulary_token_name(&value.namespace, &value.value),
            namespace: value.namespace.clone(),
            name: value.value.clone(),
            value: value.value,
            path: value.path,
            line: value.line,
            metric: value.metric,
            origin: ComparableTokenOrigin::ProjectVocabulary,
        })
        .collect();
    candidates.sort_by(|a, b| theme_token_sort_key(a).cmp(&theme_token_sort_key(b)));
    candidates
}

#[derive(Clone, Debug)]
pub(super) struct ProjectVocabularyValue {
    namespace: String,
    value: String,
    path: String,
    line: u32,
    count: u32,
    metric: ThemeTokenMetric,
}

fn project_vocabulary_token_name(namespace: &str, value: &str) -> String {
    let stable_value = value.split_whitespace().collect::<Vec<_>>().join("_");
    format!("project-vocabulary.{namespace}.{stable_value}")
}

fn color_value_has_alpha(value: &str) -> bool {
    let trimmed = value.trim();
    let Some(hex) = trimmed.strip_prefix('#') else {
        return false;
    };
    matches!(hex.len(), 4 | 8)
}

fn custom_property_token_namespace(token: &str) -> Option<&'static str> {
    let key = token.trim_start_matches('-');
    if key.starts_with("color-") {
        Some("color")
    } else if key.starts_with("text-") || key.starts_with("font-size-") {
        Some("text")
    } else if key.starts_with("radius-") || key.starts_with("border-radius-") {
        Some("radius")
    } else if key.starts_with("shadow-") || key.starts_with("box-shadow-") {
        Some("shadow")
    } else {
        None
    }
}

pub(super) fn comparable_theme_token_candidates(
    tokens: &CssTokenSets,
    config: &ResolvedConfig,
) -> Vec<ComparableThemeTokenCandidate> {
    classify_theme_token_candidates_from_tokens(tokens, config)
        .into_iter()
        .filter_map(|candidate| {
            let metric = parse_theme_token_metric(&candidate.namespace, &candidate.value)?;
            Some(ComparableThemeTokenCandidate {
                token: candidate.token,
                namespace: candidate.namespace,
                name: candidate.name,
                value: normalize_theme_token_value(&candidate.value),
                path: candidate.path,
                line: candidate.line,
                metric,
                origin: ComparableTokenOrigin::Explicit,
            })
        })
        .collect()
}

fn find_nearest_duplicate_theme_token<'a>(
    candidate: &'a ComparableThemeTokenCandidate,
    candidates: &'a [ComparableThemeTokenCandidate],
    include_later_tokens: bool,
) -> Option<(&'a ComparableThemeTokenCandidate, f64)> {
    candidates
        .iter()
        .filter(|other| other.token != candidate.token)
        .filter(|other| other.namespace == candidate.namespace)
        .filter(|other| {
            include_later_tokens || theme_token_sort_key(other) < theme_token_sort_key(candidate)
        })
        .filter(|other| {
            !theme_token_names_are_deliberate_pair(
                &candidate.namespace,
                &candidate.name,
                &other.name,
            )
        })
        .filter_map(|other| {
            let distance = candidate.metric.distance(&other.metric)?;
            if distance > 0.0 && distance <= candidate.metric.threshold() {
                Some((other, distance))
            } else {
                None
            }
        })
        .min_by(
            |(left_candidate, left_distance), (right_candidate, right_distance)| {
                left_distance
                    .partial_cmp(right_distance)
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then_with(|| {
                        theme_token_sort_key(left_candidate)
                            .cmp(&theme_token_sort_key(right_candidate))
                    })
            },
        )
}

pub(super) fn theme_token_sort_key(candidate: &ComparableThemeTokenCandidate) -> (&str, u32, &str) {
    (&candidate.path, candidate.line, &candidate.token)
}

fn normalize_theme_token_value(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub(super) fn parse_theme_token_metric(namespace: &str, value: &str) -> Option<ThemeTokenMetric> {
    match namespace {
        "color" => fallow_extract::parse_css_color_rgb(value)
            .map(rgb_to_oklab)
            .map(ThemeTokenMetric::Color),
        "spacing" | "radius" | "text" => parse_length_px(value).map(ThemeTokenMetric::LengthPx),
        "duration" => parse_duration_ms(value).map(ThemeTokenMetric::DurationMs),
        "shadow" => parse_shadow_lengths_px(value).map(ThemeTokenMetric::ShadowPx),
        _ => None,
    }
}

fn parse_length_px(value: &str) -> Option<f64> {
    let (number, unit) = parse_number_with_unit(value.trim())?;
    match unit {
        "" if number == 0.0 => Some(0.0),
        "px" => Some(number),
        "rem" | "em" => Some(number * 16.0),
        _ => None,
    }
}

fn parse_duration_ms(value: &str) -> Option<f64> {
    let (number, unit) = parse_number_with_unit(value.trim())?;
    match unit {
        "ms" => Some(number),
        "s" => Some(number * 1000.0),
        _ => None,
    }
}

fn parse_shadow_lengths_px(value: &str) -> Option<Vec<f64>> {
    if value.contains(',') {
        return None;
    }
    let mut lengths = Vec::new();
    for part in value.split_whitespace() {
        let Some(length) = parse_length_px(part) else {
            break;
        };
        lengths.push(length);
    }
    if (2..=4).contains(&lengths.len()) {
        Some(lengths)
    } else {
        None
    }
}

fn parse_number_with_unit(value: &str) -> Option<(f64, &str)> {
    let split = value
        .char_indices()
        .find(|(idx, c)| *idx > 0 && !matches!(c, '0'..='9' | '.' | '+' | '-'))
        .map_or(value.len(), |(idx, _)| idx);
    let number = value[..split].parse::<f64>().ok()?;
    let unit = &value[split..];
    if number.is_finite() {
        Some((number, unit))
    } else {
        None
    }
}

#[expect(
    clippy::suboptimal_flops,
    reason = "OKLab conversion mirrors the reference matrix; mul_add obscures the coefficients."
)]
fn rgb_to_oklab((red, green, blue): (f64, f64, f64)) -> OklabColor {
    let linear_red = srgb_to_linear(red / 255.0);
    let linear_green = srgb_to_linear(green / 255.0);
    let linear_blue = srgb_to_linear(blue / 255.0);
    let long_cone = 0.412_221_470_8 * linear_red
        + 0.536_332_536_3 * linear_green
        + 0.051_445_992_9 * linear_blue;
    let medium_cone = 0.211_903_498_2 * linear_red
        + 0.680_699_545_1 * linear_green
        + 0.107_396_956_6 * linear_blue;
    let short_cone = 0.088_302_461_9 * linear_red
        + 0.281_718_837_6 * linear_green
        + 0.629_978_700_5 * linear_blue;
    let long_cone = long_cone.cbrt();
    let medium_cone = medium_cone.cbrt();
    let short_cone = short_cone.cbrt();
    OklabColor {
        l: 0.210_454_255_3 * long_cone + 0.793_617_785_0 * medium_cone
            - 0.004_072_046_8 * short_cone,
        a: 1.977_998_495_1 * long_cone - 2.428_592_205_0 * medium_cone
            + 0.450_593_709_9 * short_cone,
        b: 0.025_904_037_1 * long_cone + 0.782_771_766_2 * medium_cone
            - 0.808_675_766_0 * short_cone,
    }
}

fn srgb_to_linear(channel: f64) -> f64 {
    if channel <= 0.04045 {
        channel / 12.92
    } else {
        ((channel + 0.055) / 1.055).powf(2.4)
    }
}

#[expect(
    clippy::suboptimal_flops,
    reason = "Distance formula is clearer in expanded Euclidean form."
)]
fn oklab_distance(left: OklabColor, right: OklabColor) -> f64 {
    let l = left.l - right.l;
    let a = left.a - right.a;
    let b = left.b - right.b;
    ((l * l + a * a + b * b).sqrt()) * 100.0
}

pub(super) fn round_distance(distance: f64) -> f64 {
    (distance * 100.0).round() / 100.0
}

fn theme_token_names_are_deliberate_pair(namespace: &str, left: &str, right: &str) -> bool {
    if namespace == "color" && color_token_name_is_semantic_ui_role(left, right) {
        return true;
    }
    if let (Some((left_base, _)), Some((right_base, _))) =
        (split_numeric_suffix(left), split_numeric_suffix(right))
        && left_base == right_base
    {
        return true;
    }
    let state_suffixes = [
        "-hover",
        "-active",
        "-focus",
        "-disabled",
        "-pressed",
        "-selected",
    ];
    state_suffixes.iter().any(|suffix| {
        left.strip_suffix(suffix) == Some(right) || right.strip_suffix(suffix) == Some(left)
    })
}

fn color_token_name_is_semantic_ui_role(left: &str, right: &str) -> bool {
    const ROLES: &[&str] = &[
        "accent",
        "accent-foreground",
        "background",
        "border",
        "card",
        "card-foreground",
        "destructive",
        "destructive-foreground",
        "foreground",
        "input",
        "muted",
        "muted-foreground",
        "popover",
        "popover-foreground",
        "primary",
        "primary-foreground",
        "ring",
        "secondary",
        "secondary-foreground",
    ];
    ROLES.contains(&left) || ROLES.contains(&right)
}

fn split_numeric_suffix(name: &str) -> Option<(&str, &str)> {
    let split = name
        .char_indices()
        .rev()
        .find(|(_, c)| !c.is_ascii_digit())
        .map(|(idx, c)| idx + c.len_utf8())?;
    if split == name.len() {
        return None;
    }
    Some((&name[..split], &name[split..]))
}
