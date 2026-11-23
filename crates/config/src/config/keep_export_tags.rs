//! The `keepExportTags` config key: the JSDoc visibility tags that keep an
//! export out of `unused-export`.

use std::fmt;

use fallow_types::extract::VisibilityTag;
use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize};

/// A JSDoc visibility tag that can keep an export out of `unused-export`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum KeepExportTag {
    /// `@public`, and also `@api public`.
    Public,
    /// `@internal`.
    Internal,
    /// `@alpha`.
    Alpha,
    /// `@beta`.
    Beta,
}

const VALID_NAMES: &str = "`public`, `internal`, `alpha` or `beta`";

impl KeepExportTag {
    fn parse(raw: &str) -> Result<Self, String> {
        let name = raw.strip_prefix('@').unwrap_or(raw);
        match name {
            "public" => Ok(Self::Public),
            "internal" => Ok(Self::Internal),
            "alpha" => Ok(Self::Alpha),
            "beta" => Ok(Self::Beta),
            "expected-unused" => Err(format!(
                "`{raw}` is not valid in `keepExportTags`: an `@expected-unused` export is \
                 always kept, and fallow reports it when it gets a consumer. Use {VALID_NAMES}."
            )),
            _ if name.starts_with(['+', '-']) => Err(format!(
                "`{raw}` is not valid in `keepExportTags`: list the tags that keep an export, \
                 without a `+` or `-` prefix, for example `[\"public\"]`. Use {VALID_NAMES}."
            )),
            _ => Err(format!(
                "`{raw}` is not a valid `keepExportTags` entry. Use {VALID_NAMES}."
            )),
        }
    }

    const fn visibility(self) -> VisibilityTag {
        match self {
            Self::Public => VisibilityTag::Public,
            Self::Internal => VisibilityTag::Internal,
            Self::Alpha => VisibilityTag::Alpha,
            Self::Beta => VisibilityTag::Beta,
        }
    }
}

impl<'de> Deserialize<'de> for KeepExportTag {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        Self::parse(&raw).map_err(serde::de::Error::custom)
    }
}

impl JsonSchema for KeepExportTag {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "KeepExportTag".into()
    }

    fn json_schema(_generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "description": "A JSDoc visibility tag name. A leading `@` is accepted. `public` also covers `@api public`.",
            "type": "string",
            "enum": ["public", "internal", "alpha", "beta", "@public", "@internal", "@alpha", "@beta"]
        })
    }
}

impl fmt::Display for KeepExportTag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Public => "public",
            Self::Internal => "internal",
            Self::Alpha => "alpha",
            Self::Beta => "beta",
        })
    }
}

/// Value of the `keepExportTags` config key.
///
/// The default lists all four tags, so a tagged export never gets an
/// `unused-export` finding. `["public"]` makes fallow check `@internal`,
/// `@alpha` and `@beta` exports like untagged exports.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Deserialize, Serialize, JsonSchema)]
#[serde(transparent)]
pub struct KeepExportTags(Vec<KeepExportTag>);

impl Default for KeepExportTags {
    fn default() -> Self {
        Self(vec![
            KeepExportTag::Public,
            KeepExportTag::Internal,
            KeepExportTag::Alpha,
            KeepExportTag::Beta,
        ])
    }
}

impl From<Vec<KeepExportTag>> for KeepExportTags {
    fn from(tags: Vec<KeepExportTag>) -> Self {
        Self(tags)
    }
}

impl KeepExportTags {
    /// Whether `tag` keeps its effect on an export. A tag that is not in the
    /// list has no effect: fallow checks the export like an untagged one.
    /// `None` and `ExpectedUnused` are not configurable and always keep
    /// their effect.
    #[must_use]
    pub fn retains(&self, tag: VisibilityTag) -> bool {
        match tag {
            VisibilityTag::None | VisibilityTag::ExpectedUnused => true,
            _ => self.0.iter().any(|kept| kept.visibility() == tag),
        }
    }

    /// Whether the list keeps all four tags, which is the default behavior.
    #[must_use]
    pub fn retains_all(&self) -> bool {
        [
            VisibilityTag::Public,
            VisibilityTag::Internal,
            VisibilityTag::Alpha,
            VisibilityTag::Beta,
        ]
        .into_iter()
        .all(|tag| self.retains(tag))
    }

    /// The configured tags sorted and without duplicates, so two lists
    /// with the same tags compare and hash equal.
    #[must_use]
    pub fn canonical(&self) -> Vec<KeepExportTag> {
        let mut tags = self.0.clone();
        tags.sort_by_key(|tag| tag.visibility() as u8);
        tags.dedup();
        tags
    }

    /// The configured tags in config order.
    #[must_use]
    pub fn tags(&self) -> &[KeepExportTag] {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(json: &str) -> Result<KeepExportTags, serde_json::Error> {
        serde_json::from_str(json)
    }

    #[test]
    fn default_retains_every_tag() {
        let tags = KeepExportTags::default();
        assert!(tags.retains_all());
        assert!(tags.retains(VisibilityTag::Internal));
    }

    #[test]
    fn public_only_drops_the_other_tags() {
        let tags = parse(r#"["public"]"#).unwrap();
        assert!(tags.retains(VisibilityTag::Public));
        assert!(!tags.retains(VisibilityTag::Internal));
        assert!(!tags.retains(VisibilityTag::Alpha));
        assert!(!tags.retains(VisibilityTag::Beta));
        assert!(tags.retains(VisibilityTag::ExpectedUnused));
        assert!(tags.retains(VisibilityTag::None));
        assert!(!tags.retains_all());
    }

    #[test]
    fn canonical_form_ignores_order_and_duplicates() {
        let ordered = parse(r#"["public", "internal", "alpha", "beta"]"#).unwrap();
        let shuffled = parse(r#"["beta", "@alpha", "internal", "public", "public"]"#).unwrap();
        assert_eq!(ordered.canonical(), shuffled.canonical());
        assert_ne!(
            ordered.canonical(),
            KeepExportTags::from(vec![KeepExportTag::Public]).canonical()
        );
    }

    #[test]
    fn empty_list_drops_public_too() {
        let tags = parse("[]").unwrap();
        assert!(!tags.retains(VisibilityTag::Public));
    }

    #[test]
    fn leading_at_sign_is_accepted() {
        let tags = parse(r#"["@internal"]"#).unwrap();
        assert_eq!(tags.tags(), &[KeepExportTag::Internal]);
    }

    #[test]
    fn invalid_entries_explain_the_valid_names() {
        let unknown = parse(r#"["private"]"#).unwrap_err().to_string();
        assert!(unknown.contains("`private` is not a valid `keepExportTags` entry"));
        let expected = parse(r#"["expected-unused"]"#).unwrap_err().to_string();
        assert!(expected.contains("always kept"));
        let prefixed = parse(r#"["-internal"]"#).unwrap_err().to_string();
        assert!(prefixed.contains("without a `+` or `-` prefix"));
    }
}
