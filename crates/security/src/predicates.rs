//! Literal, object and call-option gates of a catalogue row.

use fallow_types::extract::{SinkCallOptions, SinkLiteralValue, SinkObjectProperty};

use crate::Matcher;

impl Matcher {
    /// Whether captured literal metadata satisfies this row's literal gates.
    #[must_use]
    pub fn literal_value_satisfied(&self, literal: Option<&SinkLiteralValue>) -> bool {
        if self.literal_values.is_empty()
            && self.literal_contains.is_empty()
            && self.literal_integers.is_empty()
        {
            return true;
        }
        let string_satisfied = (self.literal_values.is_empty() && self.literal_contains.is_empty())
            || match literal {
                Some(SinkLiteralValue::String(value)) => {
                    let lower = value.to_ascii_lowercase();
                    (self.literal_values.is_empty()
                        || self
                            .literal_values
                            .iter()
                            .any(|expected| lower == expected.to_ascii_lowercase()))
                        && (self.literal_contains.is_empty()
                            || self
                                .literal_contains
                                .iter()
                                .any(|needle| lower.contains(&needle.to_ascii_lowercase())))
                }
                _ => false,
            };
        let integer_satisfied = self.literal_integers.is_empty()
            || match literal {
                Some(SinkLiteralValue::Integer(value)) => self.literal_integers.contains(value),
                _ => false,
            };
        string_satisfied && integer_satisfied
    }

    /// Whether captured object-literal metadata satisfies this row's object
    /// property gates.
    #[must_use]
    pub fn object_properties_satisfied(&self, properties: &[SinkObjectProperty]) -> bool {
        if self.object_properties.is_empty() && self.object_missing_or_false.is_empty() {
            return true;
        }
        for predicate in &self.object_properties {
            let Some(property) = properties.iter().find(|p| p.key == predicate.key) else {
                return false;
            };
            if !predicate.value.matches(&property.value) {
                return false;
            }
        }
        if self.object_missing_or_false.is_empty() {
            return true;
        }
        self.object_missing_or_false.iter().any(|key| {
            properties
                .iter()
                .find(|p| p.key == *key)
                .is_none_or(|property| matches!(property.value, SinkLiteralValue::Boolean(false)))
        })
    }

    /// Whether missing-key predicates are satisfied by complete static object
    /// key metadata.
    #[must_use]
    pub fn object_missing_satisfied(&self, keys: &[String], keys_complete: bool) -> bool {
        if self.object_missing.is_empty() {
            return true;
        }
        keys_complete && self.object_missing.iter().any(|key| !keys.contains(key))
    }

    /// Whether the call options of a sink can enable one of this row's
    /// `options_may_enable` options. Unknown options (`None`, an options
    /// variable, a spread) can enable them. Known options cannot when each
    /// listed option is absent or the literal `false`.
    #[must_use]
    pub fn options_may_enable_satisfied(&self, options: Option<&SinkCallOptions>) -> bool {
        if self.options_may_enable.is_empty() {
            return true;
        }
        let Some(options) = options else {
            return true;
        };
        if !options.keys_complete {
            return true;
        }
        self.options_may_enable.iter().any(|key| {
            options.keys.contains(key)
                && !options.properties.iter().any(|property| {
                    property.key == *key
                        && matches!(property.value, SinkLiteralValue::Boolean(false))
                })
        })
    }

    /// Whether captured context names satisfy this row's context keyword gate.
    #[must_use]
    pub fn context_satisfied(&self, context_names: &[String]) -> bool {
        if self.context_keywords.is_empty() {
            return true;
        }
        context_names.iter().any(|name| {
            let lower = name.to_ascii_lowercase();
            self.context_keywords
                .iter()
                .any(|keyword| lower.contains(&keyword.to_ascii_lowercase()))
        })
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "the test asserts a catalogue invariant directly"
)]
mod tests {
    use super::*;
    use crate::catalogue;

    #[test]
    fn options_may_enable_needs_proof_that_the_option_is_off() {
        let shell_spawn = catalogue()
            .matchers()
            .iter()
            .find(|m| m.options_may_enable == ["shell"])
            .expect("spawn shell row present");
        let options =
            |keys: &[&str], properties: Vec<SinkObjectProperty>, complete: bool| SinkCallOptions {
                properties,
                keys: keys.iter().map(ToString::to_string).collect(),
                keys_complete: complete,
            };
        let shell = |value: SinkLiteralValue| SinkObjectProperty {
            key: "shell".to_string(),
            value,
        };

        assert!(
            !shell_spawn.is_literal_aware(),
            "options_may_enable keeps the non-literal command gate"
        );
        assert!(shell_spawn.options_may_enable_satisfied(None));
        assert!(shell_spawn.options_may_enable_satisfied(Some(&options(&[], vec![], false))));
        assert!(shell_spawn.options_may_enable_satisfied(Some(&options(
            &["shell"],
            vec![shell(SinkLiteralValue::Boolean(true))],
            true,
        ))));
        assert!(shell_spawn.options_may_enable_satisfied(Some(&options(
            &["shell"],
            vec![shell(SinkLiteralValue::String("/bin/bash".to_string()))],
            true,
        ))));
        assert!(
            shell_spawn.options_may_enable_satisfied(Some(&options(&["shell"], vec![], true))),
            "a non-literal shell value can enable the shell"
        );
        assert!(!shell_spawn.options_may_enable_satisfied(Some(&options(&[], vec![], true))));
        assert!(!shell_spawn.options_may_enable_satisfied(Some(&options(
            &["stdio"],
            vec![],
            true,
        ))));
        assert!(!shell_spawn.options_may_enable_satisfied(Some(&options(
            &["shell"],
            vec![shell(SinkLiteralValue::Boolean(false))],
            true,
        ))));
    }
}
