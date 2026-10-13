//! Mutant ids.
//!
//! An id hashes the path, the text of the enclosing function (or top-level
//! statement), the span relative to that node, the operator and the
//! replacement. An edit in another function, or a shift of the whole
//! function, keeps the id. An edit inside the function changes it.

use xxhash_rust::xxh3::xxh3_64;

use crate::{MutantId, Operator};

/// The inputs of one mutant id.
pub struct IdInput<'a> {
    pub path: &'a str,
    pub anchor_text: &'a str,
    pub relative: (u32, u32),
    pub operator: Operator,
    pub replacement: &'a str,
}

fn push_text(buffer: &mut Vec<u8>, text: &str) {
    buffer.extend_from_slice(&(text.len() as u64).to_le_bytes());
    buffer.extend_from_slice(text.as_bytes());
}

/// Computes the id. `occurrence` separates identical inputs, for example two
/// equal top-level statements; it is 0 for the first one.
pub fn mutant_id(input: &IdInput<'_>, occurrence: u32) -> MutantId {
    let mut buffer = Vec::with_capacity(64 + input.path.len() + input.replacement.len());
    push_text(&mut buffer, input.path);
    buffer.extend_from_slice(&xxh3_64(input.anchor_text.as_bytes()).to_le_bytes());
    buffer.extend_from_slice(&input.relative.0.to_le_bytes());
    buffer.extend_from_slice(&input.relative.1.to_le_bytes());
    push_text(&mut buffer, input.operator.as_str());
    push_text(&mut buffer, input.replacement);
    if occurrence > 0 {
        buffer.extend_from_slice(&occurrence.to_le_bytes());
    }
    MutantId(format!("{:016x}", xxh3_64(&buffer)))
}
