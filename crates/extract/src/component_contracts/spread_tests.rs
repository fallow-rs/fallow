//! Work bounds for the spread key resolution of module-scope `const` objects.

use std::fmt::Write as _;

use oxc_allocator::Allocator;
use oxc_parser::Parser;
use oxc_semantic::SemanticBuilder;
use oxc_span::SourceType;

use super::OBJECT_KEY_CALLS;

/// Stack size of a normal spawned thread.
const NORMAL_STACK: usize = 2 * 1024 * 1024;

fn collect_counting(source: &str) -> (fallow_types::extract::ComponentContractFacts, usize) {
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, source, SourceType::mjs()).parse();
    assert!(parsed.diagnostics.is_empty(), "test source must parse");
    let semantic = SemanticBuilder::new()
        .with_build_nodes(true)
        .build(&parsed.program)
        .semantic;
    OBJECT_KEY_CALLS.with(|calls| calls.set(0));
    let facts = super::collect(&semantic, &[], &[]);
    (facts, OBJECT_KEY_CALLS.with(std::cell::Cell::get))
}

fn keys_of<'f>(
    facts: &'f fallow_types::extract::ComponentContractFacts,
    local: &str,
) -> Option<&'f [String]> {
    facts
        .spread_bindings
        .iter()
        .find(|binding| binding.local == local)
        .map(|binding| binding.keys.as_slice())
}

#[test]
fn repeated_spreads_of_one_object_resolve_each_object_once() {
    let levels = 9;
    let mut source = String::from("const a0 = { id: 'x' };\n");
    for level in 1..=levels {
        let previous = level - 1;
        writeln!(
            source,
            "const a{level} = {{ ...a{previous}, ...a{previous}, ...a{previous}, ...a{previous} }};"
        )
        .unwrap();
    }
    writeln!(source, "const sink = {{ ...a{levels} }};").unwrap();

    let (facts, calls) = collect_counting(&source);

    assert_eq!(keys_of(&facts, "sink"), Some(&["id".to_string()][..]));
    // One resolution per object, plus the object literals themselves. Without
    // a memo the count grows as 4 to the power of the chain length.
    assert!(calls <= 4 * (levels + 1), "object key resolutions: {calls}");
}

#[test]
fn long_spread_chain_stays_inside_a_normal_stack() {
    let objects = 3_000;
    let mut source = String::new();
    for index in 0..objects {
        let next = index + 1;
        writeln!(source, "const a{index} = {{ ...a{next} }};").unwrap();
    }
    writeln!(source, "const a{objects} = {{ id: 'x' }};").unwrap();

    let calls = std::thread::Builder::new()
        .stack_size(NORMAL_STACK)
        .spawn(move || collect_counting(&source).1)
        .expect("spawn test thread")
        .join()
        .expect("spread resolution must not overflow the stack");

    assert!(calls <= 200 * objects, "object key resolutions: {calls}");
}

#[test]
fn spread_object_with_too_many_keys_is_not_recorded() {
    let mut source = String::from("const base = {");
    for key in 0..=super::MAX_SPREAD_KEYS {
        write!(source, " k{key}: 1,").unwrap();
    }
    source.push_str(" };\nconst wide = { ...base };\n");

    let (facts, _) = collect_counting(&source);

    assert!(keys_of(&facts, "base").is_none());
    assert!(keys_of(&facts, "wide").is_none());
}
