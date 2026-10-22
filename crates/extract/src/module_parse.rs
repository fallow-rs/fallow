//! Public entry points that parse one source into a [`ModuleInfo`] on a stack
//! that is large enough for it.

use std::path::Path;

use fallow_types::discover::FileId;
use fallow_types::extract::ModuleInfo;

use crate::parse::parse_source_to_module_unbounded;
use fallow_types::extract::FlagPatterns;

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
pub fn parse_source_to_module_with_flags(
    file_id: FileId,
    path: &Path,
    source: &str,
    content_hash: u64,
    need_complexity: bool,
    flag_patterns: &FlagPatterns,
) -> ModuleInfo {
    let bounded = crate::nesting::parse_bounded(crate::strip_bom(source), || {
        let module = parse_source_to_module_unbounded(
            file_id,
            path,
            source,
            content_hash,
            need_complexity,
            flag_patterns,
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
