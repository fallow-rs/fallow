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
