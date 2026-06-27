//! Tailwind CSS v4 `@theme` namespace catalogue.
//!
//! Classifies a `@theme` custom property (e.g. `color-brand`, `font-weight-bold`)
//! into its Tailwind v4 namespace plus the suffix `<name>` that becomes the
//! generated utility. The namespace list lives in
//! `crates/cli/data/tailwind-theme.toml` (versioned DATA, embedded via
//! `include_str!` and parsed once), so the `unused-theme-token` candidate stays
//! consistent with the data-driven framework-knowledge tables (`tooling.toml`)
//! rather than baking a catalogue into code. There is no regeneration step; to
//! track a new Tailwind namespace, edit one entry in the TOML.

/// Embedded namespace catalogue. Compile-time-embedded, so a green
/// `theme_namespaces_parse` test guarantees the released binary parses it.
const THEME_TOML: &str = include_str!("../../data/tailwind-theme.toml");

#[derive(serde::Deserialize)]
struct ThemeNamespacesToml {
    #[serde(default)]
    suffix: Vec<String>,
    #[serde(default)]
    variant: Vec<String>,
}

/// Parsed namespaces, ordered by length DESCENDING so the longest matching
/// prefix wins (`font-weight` before `font`, `inset-shadow` before `shadow`).
struct Namespaces {
    ordered: Vec<(String, bool)>,
}

/// The result of classifying a `@theme` token against the namespace catalogue.
pub struct ThemeClassification {
    /// The matched namespace (`color`, `font-weight`, `breakpoint`, ...).
    pub namespace: String,
    /// The suffix `<name>` that becomes the generated utility (`brand`, `bold`).
    pub name: String,
    /// `true` for breakpoint / container namespaces, whose tokens generate a
    /// `<name>:` / `@<name>:` variant rather than a utility suffix.
    pub is_variant: bool,
}

/// Parse and cache the embedded catalogue once. Panics with a clear message if
/// the embedded TOML is malformed; unreachable in a released binary because the
/// bytes are compile-time-embedded and gated by `theme_namespaces_parse`.
#[expect(
    clippy::expect_used,
    reason = "embedded crates/cli/data/tailwind-theme.toml is compile-time data pinned by theme_namespaces_parse"
)]
fn namespaces() -> &'static Namespaces {
    static NAMESPACES: std::sync::OnceLock<Namespaces> = std::sync::OnceLock::new();
    NAMESPACES.get_or_init(|| {
        let parsed: ThemeNamespacesToml = toml::from_str(THEME_TOML).expect(
            "embedded crates/cli/data/tailwind-theme.toml must parse; run \
             `cargo test -p fallow-cli theme_namespaces_parse` to see the error",
        );
        let mut ordered: Vec<(String, bool)> = parsed
            .suffix
            .into_iter()
            .map(|n| (n, false))
            .chain(parsed.variant.into_iter().map(|n| (n, true)))
            .collect();
        // Longest namespace first so `font-weight-bold` matches `font-weight`
        // (name `bold`) rather than `font` (name `weight-bold`).
        ordered.sort_by(|a, b| b.0.len().cmp(&a.0.len()).then_with(|| a.0.cmp(&b.0)));
        Namespaces { ordered }
    })
}

/// Classify a `@theme` custom-property name (the `--` already stripped, e.g.
/// `font-weight-bold`) into its namespace and the utility-suffix `<name>`.
///
/// Returns `None` when no known namespace is a prefix followed by a non-empty
/// `-<name>` segment, which is exactly how `--default-*` (no namespace match),
/// bare `--spacing` (no `-<name>`), and any unknown namespace are excluded from
/// candidacy. Three more forms are rejected: a `*` in the name (`--color-*`
/// reset), and a `--` in the name (`--font-sans--font-feature-settings`, the
/// Tailwind v4 token-PROPERTY modifier form, which configures an option ON a
/// token and generates no standalone utility, so flagging it would be a false
/// positive, caught by the real-world smoke on the Tailwind docs site).
#[must_use]
pub fn classify(raw: &str) -> Option<ThemeClassification> {
    for (namespace, is_variant) in &namespaces().ordered {
        if let Some(rest) = raw.strip_prefix(namespace.as_str())
            && let Some(name) = rest.strip_prefix('-')
            && !name.is_empty()
            && !name.contains('*')
            && !name.contains("--")
        {
            return Some(ThemeClassification {
                namespace: namespace.clone(),
                name: name.to_owned(),
                is_variant: *is_variant,
            });
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn theme_namespaces_parse() {
        let ns = namespaces();
        assert!(ns.ordered.iter().any(|(n, _)| n == "color"));
        assert!(ns.ordered.iter().any(|(n, v)| n == "breakpoint" && *v));
        // Ordered longest-first.
        for pair in ns.ordered.windows(2) {
            assert!(pair[0].0.len() >= pair[1].0.len());
        }
    }

    #[test]
    fn classifies_simple_color_token() {
        let c = classify("color-brand").unwrap();
        assert_eq!(c.namespace, "color");
        assert_eq!(c.name, "brand");
        assert!(!c.is_variant);
    }

    #[test]
    fn longest_prefix_wins_for_multi_word_namespace() {
        let c = classify("font-weight-heavy").unwrap();
        assert_eq!(c.namespace, "font-weight");
        assert_eq!(c.name, "heavy");
    }

    #[test]
    fn font_token_not_swallowed_by_font_weight() {
        let c = classify("font-poppins").unwrap();
        assert_eq!(c.namespace, "font");
        assert_eq!(c.name, "poppins");
    }

    #[test]
    fn inset_shadow_beats_shadow() {
        let c = classify("inset-shadow-glow").unwrap();
        assert_eq!(c.namespace, "inset-shadow");
        assert_eq!(c.name, "glow");
    }

    #[test]
    fn multi_segment_name_kept() {
        let c = classify("color-red-500").unwrap();
        assert_eq!(c.namespace, "color");
        assert_eq!(c.name, "red-500");
    }

    #[test]
    fn variant_namespace_flagged() {
        assert!(classify("breakpoint-tablet").unwrap().is_variant);
        assert!(classify("container-prose").unwrap().is_variant);
    }

    #[test]
    fn bare_namespace_excluded() {
        assert!(classify("spacing").is_none());
        assert!(classify("font").is_none());
        assert!(classify("radius").is_none());
    }

    #[test]
    fn unknown_namespace_excluded() {
        assert!(classify("default-transition-duration").is_none());
        assert!(classify("notanamespace-x").is_none());
    }

    #[test]
    fn empty_name_excluded() {
        // A trailing-dash ident from a malformed reset form.
        assert!(classify("color-").is_none());
    }

    #[test]
    fn token_property_modifier_excluded() {
        // `--font-sans--font-feature-settings` configures an option ON the
        // `font-sans` token; it is not a standalone utility token.
        assert!(classify("font-sans--font-feature-settings").is_none());
        assert!(classify("font-mono--font-feature-settings").is_none());
    }
}
