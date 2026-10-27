//! Nested defaults in an object pattern must not cost exponential work.
//!
//! Each default re-walks its left pattern against the fallback, so without a
//! bound `const { k: { k: { m } = s } = s } = s` doubles its work at every
//! level of nesting.

use std::path::Path;
use std::sync::mpsc;
use std::time::Duration;

use fallow_types::discover::FileId;

use crate::parse_source_to_module;

/// Nesting depth. Exponential work at this depth does not finish.
const DEPTH: usize = 40;

/// Time limit for the extraction. Bounded work takes milliseconds.
const LIMIT: Duration = Duration::from_secs(20);

fn nested_default_pattern(depth: usize) -> String {
    let mut pattern = String::from("{ m }");
    for _ in 0..depth {
        pattern = format!("{{ k: {pattern} = s }}");
    }
    format!("const s = {{}};\nexport const {pattern} = s;\n")
}

#[test]
fn nested_object_pattern_defaults_finish() {
    let source = nested_default_pattern(DEPTH);
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let info = parse_source_to_module(FileId(0), Path::new("index.ts"), &source, 0, false);
        let _ = sender.send(info.exports.len());
    });
    let exports = receiver
        .recv_timeout(LIMIT)
        .expect("extraction of nested object pattern defaults must finish");
    assert_eq!(exports, 1, "the binding `m` stays an export");
}

fn credits_used_on_service(source: String) -> bool {
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let info = parse_source_to_module(FileId(0), Path::new("index.ts"), &source, 0, false);
        let credited = info
            .member_accesses
            .iter()
            .any(|access| access.object == "Service" && access.member == "used");
        let _ = sender.send(credited);
    });
    receiver
        .recv_timeout(LIMIT)
        .expect("extraction of the pattern must finish")
}

/// The direct object binding member cap of the visitor ends credits near 4000
/// properties. 2000 properties stay below it and use a fraction of the budget.
#[test]
fn wide_pattern_with_defaults_keeps_the_last_credit() {
    let properties: Vec<String> = (0..2_000)
        .map(|index| format!("p{index} = new Fallback()"))
        .collect();
    let source = format!(
        "import {{ Fallback, Service }} from './services';\n\
         const holder = {{ p1999: new Service() }};\n\
         const {{ {} }} = holder;\n\
         p1999.used();\n",
        properties.join(", ")
    );
    assert!(
        credits_used_on_service(source),
        "the last property of a wide pattern keeps its credit"
    );
}

#[test]
fn nested_object_literal_fallbacks_finish_and_keep_credits() {
    let mut pattern = String::from("{ m }");
    let mut fallback = String::from("{ m: new Service() }");
    for _ in 0..DEPTH {
        pattern = format!("{{ k: {pattern} = {fallback} }}");
        fallback = format!("{{ k: {fallback} }}");
    }
    let source = format!(
        "import {{ Service }} from './services';\n\
         const s = {{}};\n\
         const {pattern} = s;\n\
         m.used();\n"
    );
    assert!(
        credits_used_on_service(source),
        "an object literal fallback credits its class"
    );
}
