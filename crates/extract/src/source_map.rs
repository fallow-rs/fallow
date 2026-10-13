//! Source-position mapping for code extracted from container formats.

use oxc_span::Span;

#[derive(Debug, Clone)]
struct FragmentMap {
    generated_start: u32,
    original_start: u32,
    len: u32,
}

/// Extracted source plus byte-offset mappings back to the original file.
#[derive(Debug, Clone, Default)]
pub struct ExtractionResult {
    /// Source text passed to the JavaScript parser.
    pub body: String,
    fragments: Vec<FragmentMap>,
}

impl ExtractionResult {
    /// Build a mapped result for one contiguous slice.
    #[must_use]
    pub fn contiguous(body: &str, original_start: usize) -> Self {
        let mut result = Self::default();
        result.push_mapped(body, original_start);
        result
    }

    /// Append original source text to the extracted body.
    pub fn push_mapped(&mut self, text: &str, original_start: usize) {
        if text.is_empty() {
            return;
        }
        let generated_start = self.body.len();
        self.body.push_str(text);
        self.fragments.push(FragmentMap {
            generated_start: generated_start as u32,
            original_start: original_start as u32,
            len: text.len() as u32,
        });
    }

    /// Map an extracted-buffer byte offset back to the original source, taking
    /// the fragment that begins at `offset` when two fragments meet there.
    fn original_offset_start_biased(&self, offset: u32) -> Option<u32> {
        let idx = self
            .fragments
            .partition_point(|fragment| fragment.generated_start <= offset)
            .checked_sub(1)?;
        let fragment = &self.fragments[idx];
        let delta = offset.checked_sub(fragment.generated_start)?;
        if delta <= fragment.len {
            Some(fragment.original_start + delta)
        } else {
            None
        }
    }

    /// Map an extracted-buffer byte offset back to the original source, taking
    /// the fragment that ends at `offset` when two fragments meet there.
    fn original_offset_end_biased(&self, offset: u32) -> Option<u32> {
        let idx = self
            .fragments
            .partition_point(|fragment| fragment.generated_start < offset)
            .checked_sub(1)?;
        let fragment = &self.fragments[idx];
        let delta = offset.checked_sub(fragment.generated_start)?;
        if delta <= fragment.len {
            Some(fragment.original_start + delta)
        } else {
            None
        }
    }

    /// Remap a span from extracted-buffer offsets to original-source offsets.
    #[must_use]
    pub fn remap_span(&self, span: Span) -> Span {
        if span.start == 0 && span.end == 0 {
            return span;
        }
        let Some(start) = self.original_offset_start_biased(span.start) else {
            return span;
        };
        let Some(end) = self.original_offset_end_biased(span.end) else {
            return span;
        };
        Span::new(start, end)
    }
}

/// One script section of a container file: the extracted script with its
/// mapping back to the file, and the source type to parse it with.
#[derive(Debug, Clone)]
pub struct ScriptSection {
    /// The extracted script and its byte mapping.
    pub extraction: ExtractionResult,
    /// The source type of the extracted script.
    pub source_type: oxc_span::SourceType,
}

/// The inline script sections of a container file: the `<script>` blocks of a
/// Vue or Svelte SFC, the frontmatter of an Astro component, or the
/// `import` and `export` statements of an MDX document.
///
/// Returns `None` when `path` is not a container format. Then the caller
/// parses the whole file.
#[must_use]
pub fn embedded_script_sections(
    path: &std::path::Path,
    source: &str,
) -> Option<Vec<ScriptSection>> {
    if crate::sfc::is_sfc_file(path) {
        return Some(
            crate::sfc::extract_sfc_scripts(source)
                .iter()
                .filter(|script| script.src.is_none())
                .map(|script| ScriptSection {
                    extraction: ExtractionResult::contiguous(&script.body, script.byte_offset),
                    source_type: crate::sfc::source_type_for_script(script),
                })
                .collect(),
        );
    }
    if crate::astro::is_astro_file(path) {
        return Some(
            crate::astro::extract_astro_frontmatter(source)
                .iter()
                .map(|script| ScriptSection {
                    extraction: ExtractionResult::contiguous(&script.body, script.byte_offset),
                    source_type: crate::sfc::source_type_for_script(script),
                })
                .collect(),
        );
    }
    if crate::mdx::is_mdx_file(path) {
        return Some(vec![ScriptSection {
            extraction: crate::mdx::extract_mdx_statements_mapped(source),
            source_type: oxc_span::SourceType::tsx(),
        }]);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two adjacent fragments: `"ab"` from original offset 10, `"cd"` from 100.
    /// Generated offset 2 is the seam between them.
    fn two_fragments() -> ExtractionResult {
        let mut result = ExtractionResult::default();
        result.push_mapped("ab", 10);
        result.push_mapped("cd", 100);
        result
    }

    #[test]
    fn start_biased_lookup_takes_the_fragment_beginning_at_the_offset() {
        let result = two_fragments();
        assert_eq!(result.original_offset_start_biased(0), Some(10));
        assert_eq!(result.original_offset_start_biased(1), Some(11));
        // The seam belongs to the second fragment for a span start.
        assert_eq!(result.original_offset_start_biased(2), Some(100));
    }

    #[test]
    fn end_biased_lookup_takes_the_fragment_ending_at_the_offset() {
        let result = two_fragments();
        // The same seam stays in the first fragment for a span end.
        assert_eq!(result.original_offset_end_biased(2), Some(12));
        assert_eq!(result.original_offset_end_biased(4), Some(102));
        // Nothing precedes offset 0, so an end-biased lookup finds no fragment.
        assert_eq!(result.original_offset_end_biased(0), None);
    }

    #[test]
    fn a_fragment_end_maps_but_an_offset_past_it_does_not() {
        let result = ExtractionResult::contiguous("abcde", 100);
        assert_eq!(result.original_offset_start_biased(5), Some(105));
        assert_eq!(result.original_offset_end_biased(5), Some(105));
        assert_eq!(result.original_offset_start_biased(6), None);
        assert_eq!(result.original_offset_end_biased(6), None);
    }

    #[test]
    fn an_empty_result_maps_nothing() {
        let result = ExtractionResult::default();
        assert_eq!(result.original_offset_start_biased(0), None);
        assert_eq!(result.original_offset_end_biased(0), None);
    }

    #[test]
    fn remap_span_pairs_the_start_biased_and_end_biased_lookups() {
        let result = two_fragments();
        assert_eq!(result.remap_span(Span::new(1, 4)), Span::new(11, 102));
        // A zero span is returned untouched.
        assert_eq!(result.remap_span(Span::new(0, 0)), Span::new(0, 0));
        // An unmappable endpoint leaves the whole span untouched.
        assert_eq!(result.remap_span(Span::new(1, 9)), Span::new(1, 9));
    }
}
