//! Work bounds for the static value resolution of the StyleX consumer scan.

use std::fmt::Write as _;
use std::path::Path;
use std::sync::atomic::Ordering;

use rustc_hash::FxHashSet;

use super::RESOLUTION_STEPS;
use crate::css_in_js::{ConsumerQuery, css_in_js_consumer_scan};

/// Constants where each one reads the previous constant twice.
fn doubling_chain(length: usize, first: &str, call: &str) -> String {
    let mut source = String::from(
        "import * as stylex from '@stylexjs/stylex';\n\
         export const vars = stylex.defineVars({ color: 'red' });\n",
    );
    writeln!(source, "const a0 = {first};").unwrap();
    for index in 1..=length {
        let previous = index - 1;
        writeln!(source, "const a{index} = a{previous} + a{previous};").unwrap();
    }
    writeln!(source, "export const label = {call}(a{length});").unwrap();
    source
}

fn resolution_steps(source: &str) -> usize {
    let leaf_paths: FxHashSet<String> = std::iter::once("color".to_string()).collect();
    let query = ConsumerQuery::MemberBinding {
        alias: "vars",
        leaf_paths: &leaf_paths,
    };
    let before = RESOLUTION_STEPS.load(Ordering::Relaxed);
    let _ = css_in_js_consumer_scan(source, Path::new("tokens.ts"), &[query]);
    RESOLUTION_STEPS.load(Ordering::Relaxed) - before
}

// Each constant reads the previous one twice, so a resolution without a memo
// or a budget visits 2 to the power of the chain length constants.

#[test]
fn primitive_check_resolves_a_doubling_chain_in_bounded_steps() {
    let steps = resolution_steps(&doubling_chain(16, "1", "String"));
    assert!(steps <= 4_096, "resolution steps: {steps}");
}

#[test]
fn bigint_check_resolves_a_doubling_chain_in_bounded_steps() {
    let steps = resolution_steps(&doubling_chain(16, "1", "Math.abs"));
    assert!(steps <= 4_096, "resolution steps: {steps}");
}

#[test]
fn a_long_doubling_chain_finishes_inside_the_step_budget() {
    for call in ["String", "Math.abs"] {
        let steps = resolution_steps(&doubling_chain(40, "1", call));
        assert!(steps <= 4_096, "resolution steps for {call}: {steps}");
    }
}
