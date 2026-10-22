//! Content digest of a function's source text (`source_hash`).
//!
//! A function hashes its whole span, so a chain of N nested functions hashes
//! O(N^2) bytes. A function with more than [`MAX_HASHED_FUNCTION_HEIGHT`]
//! levels of nested functions below it gets no hash. Each byte then lies in
//! at most `MAX_HASHED_FUNCTION_HEIGHT + 1` hashed spans, so the work is
//! linear in the source length. The complexity and the inventory visitors
//! apply the same rule, so the hashes that join them stay the same, and every
//! hash that a function keeps has the same value as before.

use oxc_span::Span;

/// Levels of nested functions below a function, above which the function
/// gets no `source_hash`. Real code stays far below it: the deepest function
/// in about 65000 files of next.js, TypeScript, svelte, vue-core, astro and
/// vite has 10 levels below it.
pub const MAX_HASHED_FUNCTION_HEIGHT: u32 = 64;

#[cfg(test)]
thread_local! {
    /// Source bytes hashed on this thread, so tests can pin a linear cost.
    static HASHED_BYTES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Return the bytes hashed on this thread since the last call.
#[cfg(test)]
pub fn take_hashed_bytes() -> usize {
    HASHED_BYTES.with(|hashed| hashed.replace(0))
}

/// The `source_hash` of the function at `span` with `height` levels of nested
/// functions below it, or `None` when the height is above the cap or the span
/// is not a valid slice of `source`.
pub fn function_source_hash(source: &str, span: Span, height: u32) -> Option<String> {
    if height > MAX_HASHED_FUNCTION_HEIGHT {
        return None;
    }
    let slice = source.get(span.start as usize..span.end as usize)?;
    #[cfg(test)]
    HASHED_BYTES.with(|hashed| hashed.set(hashed.get() + slice.len()));
    Some(fallow_cov_protocol::source_hash_for(slice.as_bytes()))
}

#[cfg(all(test, not(miri)))]
mod tests {
    use super::*;

    /// The non-module units of a chain of `levels` nested arrows, innermost
    /// first.
    fn nested_arrows(levels: usize) -> (String, Vec<fallow_types::extract::FunctionComplexity>) {
        let source = format!("export const f = {}1;\n", "()=>".repeat(levels));
        let allocator = oxc_allocator::Allocator::default();
        let parsed =
            oxc_parser::Parser::new(&allocator, &source, oxc_span::SourceType::mjs()).parse();
        let line_offsets = fallow_types::extract::compute_line_offsets(&source);
        let units = crate::complexity::compute_complexity(&parsed.program, &source, &line_offsets)
            .into_iter()
            .filter(|unit| unit.name != fallow_types::extract::MODULE_UNIT_NAME)
            .collect();
        (source, units)
    }

    #[test]
    fn a_function_with_too_many_nested_functions_has_no_source_hash() {
        let levels = MAX_HASHED_FUNCTION_HEIGHT as usize + 3;
        let (_, arrows) = nested_arrows(levels);
        assert_eq!(arrows.len(), levels);
        for (height, arrow) in arrows.iter().enumerate() {
            let kept = height <= MAX_HASHED_FUNCTION_HEIGHT as usize;
            assert_eq!(arrow.source_hash.is_some(), kept, "height {height}");
        }
    }

    #[test]
    fn hashed_bytes_stay_linear_on_nested_arrows() {
        take_hashed_bytes();
        let (source, _) = nested_arrows(4_000);
        let hashed = take_hashed_bytes();
        let bound = source.len() * (MAX_HASHED_FUNCTION_HEIGHT as usize + 1);
        assert!(
            hashed <= bound,
            "hashed {hashed} bytes for a {} byte source",
            source.len()
        );
    }
}
