//! Public entry points that parse one source into a [`ModuleInfo`] on a stack
//! that is large enough for it.

use std::path::Path;

use fallow_types::discover::FileId;
use fallow_types::extract::ModuleInfo;

use crate::parse::parse_source_to_module_unbounded;
use fallow_types::extract::FlagPatterns;

/// Extraction settings that must agree with the caller's cache identity.
pub struct ExtractionOptions<'a> {
    pub flag_patterns: &'a FlagPatterns,
    pub mode: fallow_types::shared_state::ExtractionMode,
}

/// Parse source text into a [`ModuleInfo`].
///
/// When `need_complexity` is false the per-function complexity visitor is
/// skipped, saving one full AST walk per file.  The dead-code analysis
/// pipeline never consumes complexity data, so callers that only need
/// imports/exports should pass `false`.
///
/// Flag detection uses the built-in patterns only; see
/// [`parse_source_to_module_with_flags`].
pub fn parse_source_to_module(
    file_id: FileId,
    path: &Path,
    source: &str,
    content_hash: u64,
    need_complexity: bool,
) -> ModuleInfo {
    parse_source_to_module_with_flags(
        file_id,
        path,
        source,
        content_hash,
        need_complexity,
        &FlagPatterns::default(),
    )
}

/// Parse source text into a [`ModuleInfo`], with the user flag patterns
/// applied on top of the built-in ones.
///
/// A leading byte order mark is removed, and the spans count from the end of
/// it.
pub fn parse_source_to_module_with_flags(
    file_id: FileId,
    path: &Path,
    source: &str,
    content_hash: u64,
    need_complexity: bool,
    flag_patterns: &FlagPatterns,
) -> ModuleInfo {
    parse_source_without_bom(
        file_id,
        path,
        crate::strip_bom(source),
        content_hash,
        need_complexity,
        &ExtractionOptions {
            flag_patterns,
            mode: fallow_types::shared_state::ExtractionMode::Standard,
        },
    )
}

/// [`parse_source_to_module_with_flags`] for a source that a caller already
/// read without its byte order mark.
///
/// Only the first U+FEFF of a file is the mark. Thus this function must not
/// remove a U+FEFF again: a second U+FEFF is source text, and the spans
/// count it.
pub fn parse_source_without_bom(
    file_id: FileId,
    path: &Path,
    source: &str,
    content_hash: u64,
    need_complexity: bool,
    options: &ExtractionOptions<'_>,
) -> ModuleInfo {
    let bounded = crate::nesting::parse_file_bounded(path, source, || {
        let module = parse_source_to_module_unbounded(
            file_id,
            path,
            source,
            content_hash,
            need_complexity,
            options,
        );
        // The CSS comment mask counts on the thread that parsed. Carry the
        // count back in case that was a sized thread.
        (module, crate::css_masked_bytes::take())
    });
    match bounded {
        Some((module, masked_bytes)) => {
            crate::css_masked_bytes::add(masked_bytes);
            module
        }
        None => crate::nesting::too_deep_module(file_id, source, content_hash),
    }
}

/// Parse source with explicit optional semantic facts.
#[cfg(test)]
pub fn parse_source_to_module_with_mode(
    file_id: FileId,
    path: &Path,
    source: &str,
    content_hash: u64,
    need_complexity: bool,
    mode: fallow_types::shared_state::ExtractionMode,
) -> ModuleInfo {
    parse_source_without_bom(
        file_id,
        path,
        crate::strip_bom(source),
        content_hash,
        need_complexity,
        &ExtractionOptions {
            flag_patterns: &FlagPatterns::default(),
            mode,
        },
    )
}
