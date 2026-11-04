//! Typed views of the Playwright fixture facts.

use super::*;

/// Iterate typed Playwright fixture-use facts.
pub(super) fn playwright_fixture_use_facts(
    semantic_facts: &[SemanticFact],
) -> impl Iterator<Item = &PlaywrightFixtureUseFact> {
    semantic_facts.iter().filter_map(|fact| {
        if let SemanticFact::PlaywrightFixtureUse(access) = fact {
            Some(access)
        } else {
            None
        }
    })
}

/// Iterate typed Playwright fixture-definition facts.
pub(super) fn playwright_fixture_definition_facts(
    semantic_facts: &[SemanticFact],
) -> impl Iterator<Item = &PlaywrightFixtureDefinitionFact> {
    semantic_facts.iter().filter_map(|fact| {
        if let SemanticFact::PlaywrightFixtureDefinition(access) = fact {
            Some(access)
        } else {
            None
        }
    })
}

/// Iterate typed Playwright fixture-alias facts.
pub(super) fn playwright_fixture_alias_facts(
    semantic_facts: &[SemanticFact],
) -> impl Iterator<Item = &PlaywrightFixtureAliasFact> {
    semantic_facts.iter().filter_map(|fact| {
        if let SemanticFact::PlaywrightFixtureAlias(access) = fact {
            Some(access)
        } else {
            None
        }
    })
}

/// Iterate typed Playwright fixture-type facts.
pub(super) fn playwright_fixture_type_facts(
    semantic_facts: &[SemanticFact],
) -> impl Iterator<Item = &PlaywrightFixtureTypeFact> {
    semantic_facts.iter().filter_map(|fact| {
        if let SemanticFact::PlaywrightFixtureType(access) = fact {
            Some(access)
        } else {
            None
        }
    })
}
