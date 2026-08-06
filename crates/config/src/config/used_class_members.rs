use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// A `usedClassMembers` entry from config or an external plugin.
///
/// Supports either a plain member name or glob pattern (`"agInit"`,
/// `"enter*"`) or a scoped rule that only applies when a class matches
/// specific `extends` / `implements` heritage clauses.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(untagged)]
pub enum UsedClassMemberRule {
    /// Globally suppress this class member name or glob pattern for all classes.
    Name(String),
    /// Suppress these class member names only for matching classes.
    Scoped(ScopedUsedClassMemberRule),
}

impl From<&str> for UsedClassMemberRule {
    fn from(value: &str) -> Self {
        Self::Name(value.to_string())
    }
}

impl From<String> for UsedClassMemberRule {
    fn from(value: String) -> Self {
        Self::Name(value)
    }
}

/// A heritage-constrained `usedClassMembers` rule.
#[derive(Debug, Clone, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ScopedUsedClassMemberRule {
    /// Only apply when the class extends this parent class name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extends: Option<String>,
    /// Only apply when the class implements this interface name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub implements: Option<String>,
    /// Member names or glob patterns that should be treated as framework-used.
    pub members: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ScopedUsedClassMemberRuleDef {
    #[serde(default)]
    extends: Option<String>,
    #[serde(default)]
    implements: Option<String>,
    members: Vec<String>,
}

impl TryFrom<ScopedUsedClassMemberRuleDef> for ScopedUsedClassMemberRule {
    type Error = &'static str;

    fn try_from(value: ScopedUsedClassMemberRuleDef) -> Result<Self, Self::Error> {
        if value.extends.is_none() && value.implements.is_none() {
            return Err("scoped usedClassMembers rules require `extends` or `implements`");
        }

        Ok(Self {
            extends: value.extends,
            implements: value.implements,
            members: value.members,
        })
    }
}

impl<'de> Deserialize<'de> for ScopedUsedClassMemberRule {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        ScopedUsedClassMemberRuleDef::deserialize(deserializer)?
            .try_into()
            .map_err(serde::de::Error::custom)
    }
}

impl ScopedUsedClassMemberRule {
    /// Whether a class with the given heritage clause is in this rule's scope.
    ///
    /// An unset `extends` / `implements` filter matches any class; a set
    /// filter requires exact name equality (`implements` matches when any one
    /// implemented interface equals it). Both set filters must match.
    #[must_use]
    pub fn matches_heritage(
        &self,
        super_class: Option<&str>,
        implemented_interfaces: &[String],
    ) -> bool {
        let extends_matches = self
            .extends
            .as_deref()
            .is_none_or(|expected| super_class == Some(expected));
        let implements_matches = self
            .implements
            .as_deref()
            .is_none_or(|expected| implemented_interfaces.iter().any(|iface| iface == expected));

        extends_matches && implements_matches
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserialize_plain_member_name() {
        let rule: UsedClassMemberRule = serde_json::from_str(r#""agInit""#).unwrap();
        assert_eq!(rule, UsedClassMemberRule::Name("agInit".to_string()));
    }

    #[test]
    fn deserialize_scoped_rule() {
        let rule: UsedClassMemberRule = serde_json::from_str(
            r#"{"implements":"ICellRendererAngularComp","members":["refresh"]}"#,
        )
        .unwrap();
        assert_eq!(
            rule,
            UsedClassMemberRule::Scoped(ScopedUsedClassMemberRule {
                extends: None,
                implements: Some("ICellRendererAngularComp".to_string()),
                members: vec!["refresh".to_string()],
            })
        );
    }

    #[test]
    fn scoped_rule_matches_extends_and_implements() {
        let rule = ScopedUsedClassMemberRule {
            extends: Some("BaseCommand".to_string()),
            implements: Some("Runnable".to_string()),
            members: vec!["execute".to_string()],
        };

        assert!(rule.matches_heritage(
            Some("BaseCommand"),
            &["Runnable".to_string(), "Disposable".to_string()]
        ));
        assert!(!rule.matches_heritage(Some("OtherBase"), &["Runnable".to_string()]));
        assert!(!rule.matches_heritage(Some("BaseCommand"), &["Other".to_string()]));
    }

    #[test]
    fn deserialize_scoped_rule_requires_constraint() {
        let error = serde_json::from_str::<ScopedUsedClassMemberRule>(r#"{"members":["refresh"]}"#)
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("require `extends` or `implements`"),
            "unexpected error: {error}"
        );
    }
}
