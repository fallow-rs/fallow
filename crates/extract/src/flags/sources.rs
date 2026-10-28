//! Classify the import sources of a file for feature flag detection: a flag
//! SDK or flag module, or a module of the project.

/// Lowercase start of a source word that names a flag vendor, such as
/// `unleash` in `@unleash/proxy-client-react` or `launchdarkly` in
/// `./launchDarklyClient`.
const FLAG_VENDOR_WORDS: &[&str] = &[
    "launchdarkly",
    "statsig",
    "unleash",
    "growthbook",
    "splitio",
    "splitsoftware",
    "posthog",
    "configcat",
    "optimizely",
    "eppo",
    "flagsmith",
    "openfeature",
];

/// Lowercase source tokens that name flags, such as `flags` in `@/lib/flags`.
const FLAG_TOKENS: &[&str] = &[
    "flag",
    "flags",
    "toggle",
    "toggles",
    "featureflag",
    "featureflags",
    "featuretoggle",
    "featuretoggles",
];

/// Lowercase source tokens that can stand next to a flag token in a flag
/// module name, such as `sdk` in `@flags-sdk/launchdarkly` or `use` in
/// `./useFlags`. A token such as `icons` in `country-flag-icons` is not one.
const FLAG_CONTEXT_TOKENS: &[&str] = &[
    "use", "sdk", "client", "provider", "context", "service", "store", "config", "js", "react",
    "node",
];

/// Lowercase source tokens that name flags in a module of the project. A
/// local module such as `./flags.server` or `./flagUtils` is a flag module.
/// `toggle` is not one, because `./ThemeToggle` is a UI component.
const LOCAL_FLAG_TOKENS: &[&str] = &[
    "flag",
    "flags",
    "featureflag",
    "featureflags",
    "featuretoggle",
    "featuretoggles",
];

/// Lowercase source tokens that can name a flag module of the project, such
/// as `feature` in `./featureGate`. A generic SDK name from such a module is
/// not a certain flag site, but it is also not a plain project function.
const LOCAL_MAYBE_FLAG_TOKENS: &[&str] = &[
    "feature",
    "features",
    "gate",
    "gates",
    "experiment",
    "experiments",
    "toggle",
    "toggles",
];

/// File extensions that can end a path segment of an import source.
const SOURCE_EXTENSIONS: &[&str] = &[
    "ts", "tsx", "mts", "cts", "js", "jsx", "mjs", "cjs", "json", "vue", "svelte",
];

/// Whether an import source names a flag SDK or a flag module. One path
/// segment must name a flag vendor or flags, so `@/lib/flags` and
/// `@unleash/proxy-client-react` match, but `country-flag-icons` and
/// `@/features/checkout/api` do not. For a module of the project, a
/// segment such as `flags.server` or `flagUtils` that does not start with an
/// uppercase letter also matches.
pub(super) fn is_flag_source(source: &str) -> bool {
    source.split('/').any(is_flag_segment)
        || (is_local_source(source) && source.split('/').any(is_local_flag_segment))
}

/// Whether an import source of the project can be a flag module: a segment
/// has a flag word or a word such as `feature` or `gate`. A generic SDK name
/// from such a module stays a flag site with `medium` confidence.
pub(super) fn may_be_local_flag_source(source: &str) -> bool {
    source.split('/').any(|segment| {
        segment_tokens(segment).iter().any(|token| {
            FLAG_TOKENS.contains(&token.as_str())
                || LOCAL_MAYBE_FLAG_TOKENS.contains(&token.as_str())
        })
    })
}

/// Whether one path segment of a project import source names flags. A
/// segment that starts with an uppercase letter, such as `CountryFlag`, is
/// usually a UI component, so it does not match.
fn is_local_flag_segment(segment: &str) -> bool {
    !segment.starts_with(|c: char| c.is_ascii_uppercase())
        && segment_tokens(segment)
            .iter()
            .any(|token| LOCAL_FLAG_TOKENS.contains(&token.as_str()))
}

/// Whether one path segment of an import source names a flag vendor or flags.
fn is_flag_segment(segment: &str) -> bool {
    let lower = segment.to_ascii_lowercase();
    let names_vendor = lower
        .split(|c: char| !c.is_ascii_alphanumeric())
        .any(|word| {
            FLAG_VENDOR_WORDS
                .iter()
                .any(|vendor| word.starts_with(vendor))
        });
    if names_vendor {
        return true;
    }
    let mut tokens = segment_tokens(segment);
    if tokens.len() > 1
        && tokens
            .last()
            .is_some_and(|last| SOURCE_EXTENSIONS.contains(&last.as_str()))
    {
        tokens.pop();
    }
    let is_flag = |token: &String| FLAG_TOKENS.contains(&token.as_str());
    if !tokens.iter().any(is_flag) {
        return false;
    }
    let feature_flag_pair = tokens
        .windows(2)
        .any(|pair| pair[0] == "feature" && is_flag(&pair[1]));
    feature_flag_pair
        || tokens
            .iter()
            .all(|token| is_flag(token) || FLAG_CONTEXT_TOKENS.contains(&token.as_str()))
}

/// The lowercase words of a path segment, split at each non-alphanumeric
/// character and at each change from a lowercase letter or digit to an
/// uppercase letter, so `useFeatureFlags.ts` gives `use`, `feature`,
/// `flags` and `ts`.
fn segment_tokens(segment: &str) -> Vec<String> {
    let mut tokens: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut previous_lower = false;
    for c in segment.chars() {
        if !c.is_ascii_alphanumeric() {
            if !current.is_empty() {
                tokens.push(std::mem::take(&mut current));
            }
            previous_lower = false;
            continue;
        }
        if c.is_ascii_uppercase() && previous_lower && !current.is_empty() {
            tokens.push(std::mem::take(&mut current));
        }
        previous_lower = c.is_ascii_lowercase() || c.is_ascii_digit();
        current.push(c.to_ascii_lowercase());
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

/// Whether an import source is a module of the project: a relative path, an
/// absolute path, or a `@/`, `~/` or `#` alias.
pub(super) fn is_local_source(source: &str) -> bool {
    source.starts_with('.')
        || source.starts_with('/')
        || source.starts_with("@/")
        || source.starts_with("~/")
        || source.starts_with('#')
}
