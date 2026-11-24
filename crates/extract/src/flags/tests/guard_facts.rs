//! Tests for the facts of a flag guard: identical branches and empty
//! branches.

use super::*;

fn only_flag(source: &str) -> FlagUse {
    let mut flags = extract_from_source(source);
    assert_eq!(flags.len(), 1, "one flag read in {source}");
    flags.remove(0)
}

#[test]
fn identical_if_branches_ignore_whitespace_and_comments() {
    let flag =
        only_flag("if (process.env.FEATURE_X) {\n  run(1);\n} else {\n  run( 1 ) ; // same\n}");
    assert!(flag.facts.identical_branches());
    assert!(!flag.facts.empty_branch());
}

#[test]
fn a_one_token_difference_is_not_identical() {
    let flag = only_flag("if (process.env.FEATURE_X) { run(1); } else { run(2); }");
    assert!(!flag.facts.identical_branches());
}

#[test]
fn identical_ternary_arms_are_identical_branches() {
    let flag = only_flag("const v = process.env.FEATURE_X ? pick('a') : pick('a');");
    assert!(flag.facts.identical_branches());
}

#[test]
fn a_guard_without_code_in_any_branch_is_an_empty_branch() {
    for source in [
        "if (process.env.FEATURE_X) {}",
        "if (process.env.FEATURE_X) ;",
        "if (!process.env.FEATURE_X) {}",
        "if (process.env.FEATURE_X) {} else {}",
        "if (process.env.FEATURE_X === false) ; else {}",
    ] {
        assert!(only_flag(source).facts.empty_branch(), "{source}");
    }
}

#[test]
fn a_guard_with_code_in_one_branch_is_not_an_empty_branch() {
    // Turning the flag on or off runs or skips that code, so the flag
    // does something, also when the code runs only while the flag is off.
    for source in [
        "if (process.env.FEATURE_X) { run(); }",
        "if (process.env.FEATURE_X) { run(); } else {}",
        "if (process.env.FEATURE_X) {} else { run(); }",
        "if (!process.env.FEATURE_X) { run(); } else {}",
        "if (process.env.FEATURE_X === false) { run(); } else {}",
        "if (!process.env.FEATURE_X) return;",
    ] {
        assert!(!only_flag(source).facts.empty_branch(), "{source}");
    }
}

#[test]
fn ternary_and_jsx_guards_are_empty_only_when_no_arm_renders() {
    for source in [
        "const v = useFlag('beta') ? null : undefined;",
        "const v = useFlag('beta') ? <></> : null;",
        "const v = useFlag('beta') ? void 0 : null;",
        "const v = useFlag('beta') ? void undefined : void 'x';",
        "const v = useFlag('beta') ? false : <></>;",
        "const v = <div>{useFlag('beta') && <></>}</div>;",
    ] {
        assert!(only_flag(source).facts.empty_branch(), "{source}");
    }
    for source in [
        "const v = useFlag('beta') ? null : <Old />;",
        "const v = useFlag('beta') ? undefined : <Old />;",
        "const v = useFlag('beta') ? false : <Old />;",
        "const v = useFlag('beta') ? <></> : <Old />;",
        "const v = !useFlag('beta') ? <Beta /> : null;",
        "const v = useFlag('beta') ? <Beta /> : null;",
        "const v = useFlag('beta') ? <Beta /> : false;",
        "const v = useFlag('beta') ? false : 1;",
        "const v = <div>{useFlag('beta') && <Beta />}</div>;",
        "const v = useFlag('beta') ? void track() : null;",
        "const v = useFlag('beta') ? null : void (count += 1);",
    ] {
        assert!(!only_flag(source).facts.empty_branch(), "{source}");
    }
}

#[test]
fn a_bound_read_takes_the_facts_of_its_guard() {
    let flag = only_flag("const on = useFlag('beta');\nif (on) { run(); } else { run(); }");
    assert!(flag.facts.identical_branches());
}

#[test]
fn a_constant_read_takes_the_facts_of_its_guard() {
    let found = constants("const FEATURE_X = true;\nconst v = FEATURE_X ? null : <></>;");
    assert!(found[0].reads[0].facts.empty_branch());
}

#[test]
fn first_guard_keeps_the_vercel_flag_definition() {
    let source = "import { flag } from 'flags/next';\n\
                  const showBanner = flag({ key: 'banner', decide: () => false });\n\
                  if (await showBanner()) {}\n\
                  async function other(showBanner) { if (await showBanner()) {} }";
    let flags = extract_from_source(source);
    let [definition, read] = flags.as_slice() else {
        panic!("expected the declaration and its guard read: {flags:?}");
    };
    assert!(definition.facts.definition());
    assert_eq!(definition.line, 2);
    assert!(definition.guard_span_start.is_none());
    assert_eq!(
        (read.flag_name.as_str(), read.line, read.col),
        ("banner", 3, 10)
    );
    assert!(!read.facts.definition());
    assert!(read.facts.empty_branch());
    assert_eq!(guard_text(source, read), "if (await showBanner()) {}");
}

#[test]
fn a_read_without_a_guard_has_no_facts() {
    let flag = only_flag("track(useFlag('beta'));");
    assert_eq!(flag.facts, FlagSiteFacts::default());
}

#[test]
fn a_bound_vercel_flag_call_is_a_definition() {
    let source = "import { flag, evaluate } from 'flags/next';\n\
                  export const showBanner = flag({\n\
                    key: 'show-banner',\n\
                    decide: () => Boolean(process.env.FEATURE_BANNER),\n\
                  });\n\
                  const value = await evaluate('show-banner');\n";
    let facts = extract_facts(source);
    let definition = facts
        .flag_uses
        .iter()
        .find(|flag| flag.flag_name == "show-banner" && flag.line == 2)
        .expect("definition site");
    assert!(definition.facts.definition());
    let evaluate = facts
        .flag_uses
        .iter()
        .find(|flag| flag.line == 6)
        .expect("evaluate site");
    assert!(!evaluate.facts.definition(), "evaluate reads the flag");
    let definitions = facts
        .registry_facts
        .map(|registry| registry.definitions)
        .unwrap_or_default();
    assert_eq!(
        definitions,
        vec![FlagDefinition {
            binding: "showBanner".to_string(),
            line: definition.line,
            col: definition.col,
        }]
    );
}

#[test]
fn an_unbound_vercel_flag_call_is_not_a_definition() {
    let facts = extract_facts(
        "import { flag } from 'flags/next';\nregister(flag({ key: 'loose', decide: () => false }));",
    );
    assert!(!facts.flag_uses[0].facts.definition());
    assert!(facts.registry_facts.is_none());
}
