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
fn a_read_without_a_guard_has_no_facts() {
    let flag = only_flag("track(useFlag('beta'));");
    assert_eq!(flag.facts, FlagSiteFacts::default());
}
