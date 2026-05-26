use super::common::{create_config, fixture_path};

#[test]
fn new_expression_receivers_credit_class_members() {
    // Issue #605: methods reached through a freshly-constructed instance must
    // credit the class. Covers the two public adoption-PR shapes:
    //   - direct constructor receiver: `new TracesRepository(client).search(data)`
    //   - fluent chain rooted at a constructor:
    //     `new OptionBuilder().addDefault(...).addFromCli(...).build()`
    let root = fixture_path("issue-605-new-class-member");
    let mut config = create_config(root);
    config.rules.unused_class_members = fallow_config::Severity::Error;
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused: Vec<String> = results
        .unused_class_members
        .iter()
        .map(|m| format!("{}.{}", m.member.parent_name, m.member.member_name))
        .collect();

    // Direct constructor-receiver credits (everr#144 shape).
    for credited in ["TracesRepository.search", "TracesRepository.getTrace"] {
        assert!(
            !unused.contains(&credited.to_string()),
            "{credited} is called via `new TracesRepository(client).<method>()` and must be \
             credited (issue #605), found: {unused:?}"
        );
    }

    // Fluent-chain-rooted-at-constructor credits (graphql-markdown#2949 shape).
    // `addDefault` is the first method directly off the constructor; the rest
    // are downstream chain members validated as self-returning.
    for credited in [
        "OptionBuilder.addDefault",
        "OptionBuilder.addFromConfig",
        "OptionBuilder.addFromCli",
        "OptionBuilder.build",
    ] {
        assert!(
            !unused.contains(&credited.to_string()),
            "{credited} is reached through a fluent chain off `new OptionBuilder()` and must be \
             credited (issue #605), found: {unused:?}"
        );
    }

    // `peek()` is the first method off the constructor, so it is credited.
    assert!(
        !unused.contains(&"OptionBuilder.peek".to_string()),
        "OptionBuilder.peek is called directly off `new OptionBuilder()` and must be credited, \
         found: {unused:?}"
    );

    // A user class whose name collides with a global builtin must still be
    // credited: extraction records the bare identifier and the analyze layer
    // resolves `URL` to the user export. Guarding on `is_builtin_constructor`
    // at extraction time would silently re-introduce the #605 false positive
    // here (caught by Codex's parallel review).
    assert!(
        !unused.contains(&"URL.parse".to_string()),
        "URL.parse is called via `new URL().parse()` on a USER class named like a builtin and \
         must be credited (issue #605), found: {unused:?}"
    );

    // Regression guards: genuinely-unused members must STILL be reported.
    // `unusedRepoMethod` / `addUnused` are never called; `afterPeek` is reached
    // only as a downstream member after the non-self-returning `peek()`, so the
    // #387 safety check (each chain step must be self-returning) rejects it.
    for flagged in [
        "TracesRepository.unusedRepoMethod",
        "OptionBuilder.addUnused",
        "OptionBuilder.afterPeek",
        "URL.unusedOnUrl",
    ] {
        assert!(
            unused.contains(&flagged.to_string()),
            "{flagged} has no crediting call site and must remain flagged unused (no blanket \
             over-credit), found: {unused:?}"
        );
    }
}
