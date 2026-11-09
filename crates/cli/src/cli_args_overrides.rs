//! Typed global option overrides shared by command dispatch.
use super::*;

impl Cli {
    /// Tri-state CLI override for type-aware analysis: `Some(true)` for
    /// `--type-aware`, `Some(false)` for `--no-type-aware`, `None` when
    /// neither flag was passed (environment and config decide).
    pub const fn type_aware_override(&self) -> Option<bool> {
        if self.no_type_aware {
            Some(false)
        } else if self.type_aware {
            Some(true)
        } else {
            None
        }
    }

    /// The global `--dupes-*` overrides of the `duplicates` config.
    pub const fn dupes_overrides(&self) -> DupesOverrides {
        DupesOverrides {
            mode: self.dupes_mode,
            near: self.dupes_near,
            threshold: self.dupes_threshold,
            min_tokens: self.dupes_min_tokens,
            min_lines: self.dupes_min_lines,
            min_occurrences: self.dupes_min_occurrences,
            skip_local: self.dupes_skip_local,
            ignore_symlinks: resolve_negatable_flag(
                self.dupes_ignore_symlinks,
                self.dupes_no_ignore_symlinks,
            ),
            cross_language: self.dupes_cross_language,
            ignore_imports: resolve_negatable_flag(
                self.dupes_ignore_imports,
                self.dupes_no_ignore_imports,
            ),
        }
    }
}
