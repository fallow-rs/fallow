//! Config-value dependency crediting.
//!
//! Some packages are loaded at runtime only because a config value names them,
//! with no import anywhere in the project. Crediting them keeps a declared
//! dependency out of the unused-dependency report.
//!
//! The rules are data, not code: the `(surface, value) -> credits` rows live in
//! `crates/core/data/config_value_credits.toml`, embedded via `include_str!`
//! and parsed once at startup. There is no regeneration step. Adding a rule for
//! an existing surface is a one-entry change. See `CONTRIBUTING.md`.

use rustc_hash::FxHashMap;

/// Embedded catalogue source. Because it is `include_str!`-embedded at compile
/// time, a green `catalogue_parses` test guarantees the released binary parses.
const CATALOGUE_TOML: &str = include_str!("../../data/config_value_credits.toml");

/// The config locations a credited value can be read from.
///
/// The set is closed on purpose: every surface has a call site that knows how
/// to normalize the value before the lookup, so an unknown surface in the TOML
/// is a bug rather than a new rule. Serde rejects unknown variants, which makes
/// the catalogue parse fail loudly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CreditSurface {
    /// Jest `testEnvironment` / Vitest `test.environment`, after the runner
    /// prefix is stripped. Credits are additive: the environment package itself
    /// is credited by the runner plugin.
    TestEnvironmentOptionalPeer,
    /// Vitest `test.environment` values that name a built-in environment rather
    /// than an installable package. Credits replace the name-derived
    /// dependencies entirely.
    VitestBuiltinEnvironment,
    /// Vite `css.transformer` / `build.cssMinify`, naming the CSS
    /// implementation package Vite loads but does not ship.
    ViteCssImplementation,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct CatalogueFile {
    #[serde(default)]
    credit: Vec<CreditEntry>,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct CreditEntry {
    surface: CreditSurface,
    /// The exact config value, after surface-specific normalization.
    value: String,
    /// Packages credited as referenced. Must be non-empty; an empty list would
    /// be a silently inert row.
    credits: Vec<String>,
    /// Optional human context; does not affect matching.
    #[expect(
        dead_code,
        reason = "documentation field, surfaced via the catalogue source"
    )]
    #[serde(default)]
    notes: Option<String>,
}

type Catalogue = FxHashMap<(CreditSurface, String), Vec<String>>;

/// Parse and validate catalogue source.
///
/// # Errors
///
/// Returns a human-readable message when the TOML is malformed, an entry names
/// an unknown surface, a value or credit is empty or whitespace, or two entries
/// claim the same `(surface, value)` key.
fn parse(source: &str) -> Result<Catalogue, String> {
    let parsed: CatalogueFile = toml::from_str(source).map_err(|e| e.to_string())?;
    let mut catalogue = Catalogue::default();

    for entry in parsed.credit {
        if entry.value.trim().is_empty() {
            return Err(format!(
                "credit entry for {:?} has an empty value",
                entry.surface
            ));
        }
        if entry.credits.is_empty() {
            return Err(format!(
                "credit entry {:?} / {:?} credits no packages",
                entry.surface, entry.value
            ));
        }
        if let Some(blank) = entry.credits.iter().find(|c| c.trim().is_empty()) {
            return Err(format!(
                "credit entry {:?} / {:?} has an empty credit {blank:?}",
                entry.surface, entry.value
            ));
        }
        let key = (entry.surface, entry.value);
        if catalogue.contains_key(&key) {
            return Err(format!("duplicate credit entry: {:?} / {:?}", key.0, key.1));
        }
        catalogue.insert(key, entry.credits);
    }

    Ok(catalogue)
}

/// Parse and cache the embedded catalogue once. Panics with a clear message if
/// the embedded TOML is invalid; this is unreachable in a released binary
/// because the bytes are compile-time-embedded and gated by `catalogue_parses`.
#[expect(
    clippy::expect_used,
    reason = "embedded credit catalogue is compile-time data pinned by catalogue_parses"
)]
fn catalogue() -> &'static Catalogue {
    static CATALOGUE: std::sync::OnceLock<Catalogue> = std::sync::OnceLock::new();
    CATALOGUE.get_or_init(|| {
        parse(CATALOGUE_TOML).expect(
            "embedded crates/core/data/config_value_credits.toml must be valid; run \
             `cargo test -p fallow-core config_value_credits` to see the error",
        )
    })
}

/// Packages credited because `value` was read from `surface`.
///
/// Returns `None` when no rule matches, which callers distinguish from a rule
/// crediting nothing: a row always credits at least one package.
#[must_use]
pub fn credited_packages(surface: CreditSurface, value: &str) -> Option<&'static [String]> {
    catalogue()
        .get(&(surface, value.to_string()))
        .map(Vec::as_slice)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalogue_parses() {
        let cat = catalogue();
        assert!(!cat.is_empty(), "catalogue must have entries");
    }

    #[test]
    fn jsdom_credits_canvas() {
        assert_eq!(
            credited_packages(CreditSurface::TestEnvironmentOptionalPeer, "jsdom"),
            Some(["canvas".to_string()].as_slice())
        );
    }

    #[test]
    fn edge_runtime_credits_vm_package() {
        assert_eq!(
            credited_packages(CreditSurface::VitestBuiltinEnvironment, "edge-runtime"),
            Some(["@edge-runtime/vm".to_string()].as_slice())
        );
    }

    #[test]
    fn lightningcss_credits_itself() {
        assert_eq!(
            credited_packages(CreditSurface::ViteCssImplementation, "lightningcss"),
            Some(["lightningcss".to_string()].as_slice())
        );
    }

    #[test]
    fn happy_dom_has_no_optional_peer_credit() {
        assert!(
            credited_packages(CreditSurface::TestEnvironmentOptionalPeer, "happy-dom").is_none()
        );
    }

    #[test]
    fn surfaces_do_not_leak_into_each_other() {
        assert!(credited_packages(CreditSurface::ViteCssImplementation, "jsdom").is_none());
        assert!(
            credited_packages(CreditSurface::TestEnvironmentOptionalPeer, "lightningcss").is_none()
        );
    }

    #[test]
    fn unknown_surface_is_rejected() {
        let err = parse(
            r#"
[[credit]]
surface = "vite-postcss-plugin"
value = "x"
credits = ["y"]
"#,
        )
        .expect_err("unknown surface must fail");
        assert!(err.contains("vite-postcss-plugin"), "got: {err}");
    }

    #[test]
    fn unknown_field_is_rejected() {
        let err = parse(
            r#"
[[credit]]
surface = "vite-css-implementation"
value = "x"
credits = ["y"]
ecosystem = "css"
"#,
        )
        .expect_err("unknown field must fail");
        assert!(err.contains("ecosystem"), "got: {err}");
    }

    #[test]
    fn empty_value_is_rejected() {
        parse(
            r#"
[[credit]]
surface = "vite-css-implementation"
value = "  "
credits = ["y"]
"#,
        )
        .expect_err("empty value must fail");
    }

    #[test]
    fn empty_credits_are_rejected() {
        parse(
            r#"
[[credit]]
surface = "vite-css-implementation"
value = "x"
credits = []
"#,
        )
        .expect_err("empty credits must fail");
    }

    #[test]
    fn blank_credit_is_rejected() {
        parse(
            r#"
[[credit]]
surface = "vite-css-implementation"
value = "x"
credits = [""]
"#,
        )
        .expect_err("blank credit must fail");
    }

    #[test]
    fn duplicate_entry_is_rejected() {
        let err = parse(
            r#"
[[credit]]
surface = "vite-css-implementation"
value = "x"
credits = ["y"]

[[credit]]
surface = "vite-css-implementation"
value = "x"
credits = ["z"]
"#,
        )
        .expect_err("duplicate entry must fail");
        assert!(err.contains("duplicate"), "got: {err}");
    }

    #[test]
    fn missing_required_field_is_rejected() {
        parse(
            r#"
[[credit]]
surface = "vite-css-implementation"
credits = ["y"]
"#,
        )
        .expect_err("missing value must fail");
    }
}
