//! Source-position mapping for code extracted from container formats.

use oxc_span::Span;

/// Extractor contract for container formats that feed extracted source to Oxc.
pub trait SfcExtractor {
    /// Return one or more source-mapped extracted fragments from `source`.
    fn extract(&self, source: &str) -> Vec<ExtractionResult>;
}

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

    /// Map an extracted-buffer byte offset back to the original source.
    #[must_use]
    pub fn original_offset(&self, offset: u32) -> Option<u32> {
        self.original_offset_start_biased(offset)
    }

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
        let Some(start) = self.original_offset(span.start) else {
            return span;
        };
        let Some(end) = self.original_offset_end_biased(span.end) else {
            return span;
        };
        Span::new(start, end)
    }
}
