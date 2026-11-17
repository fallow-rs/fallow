//! Parser options for the JavaScript and TypeScript parse of a source file.

use oxc_parser::ParseOptions;
use oxc_span::SourceType;

/// A file without parse errors at or below this size skips the JSX retry.
pub const JSX_RETRY_SKIP_CLEAN_MAX_LEN: usize = 100;

/// Parser options for a source of `source_type`.
///
/// A `.js`, `.jsx`, `.ts` or `.tsx` file can be a CommonJS module. Node.js
/// wraps a CommonJS module in a function, so a top-level `return` is valid
/// there. oxc allows it only for `.cjs` and `.cts`, and reports an error for
/// these extensions. The error marks a valid file as degraded, so allow the
/// statement for every source whose module kind the extension does not fix.
///
/// The JSX retry parses a `.js` file as `SourceType::jsx()`, which is an ES
/// module. It passes the original source type here, so a CommonJS file keeps
/// its top-level `return`.
pub fn parse_options_for(source_type: SourceType) -> ParseOptions {
    ParseOptions {
        allow_return_outside_function: source_type.is_unambiguous(),
        ..ParseOptions::default()
    }
}
