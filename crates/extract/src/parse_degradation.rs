//! Parse degradation reported for a source file.

/// How much of the file the parser actually understood.
///
/// oxc reports recoverable errors for valid-but-newer syntax as well as for
/// genuinely broken sources, so this is reporting data only: extraction keeps
/// every symbol it found, and no finding is ever withheld because of it. What
/// it buys is honesty about the case where a file failed to parse and its
/// imports therefore never credited anything, which otherwise surfaces as a
/// confident `unused-file` finding on the file that was in fact imported.
#[derive(Debug, Clone, Copy, Default)]
pub struct ParseDegradation {
    pub error_count: u32,
    pub panicked: bool,
}

impl ParseDegradation {
    pub fn merge(&mut self, other: Self) {
        self.error_count = self.error_count.saturating_add(other.error_count);
        self.panicked |= other.panicked;
    }

    pub fn apply_to(self, module: &mut fallow_types::extract::ModuleInfo) {
        module.parse_error_count = module.parse_error_count.saturating_add(self.error_count);
        module.parse_panicked |= self.panicked;
    }

    pub fn from_parser(parser_return: &oxc_parser::ParserReturn<'_>) -> Self {
        Self {
            error_count: u32::try_from(parser_return.diagnostics.len()).unwrap_or(u32::MAX),
            panicked: parser_return.fatal_error,
        }
    }
}
