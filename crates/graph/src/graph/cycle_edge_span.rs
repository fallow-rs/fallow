//! The source position of one edge of an import cycle.

use fallow_types::discover::FileId;

use super::{ModuleGraph, ModuleNode};

impl ModuleNode {
    pub(super) fn re_export_span_start(&self, target: FileId, type_only: bool) -> Option<u32> {
        self.re_exports
            .iter()
            .find(|re| {
                re.source_file == target && re.is_type_only == type_only && !re.span.is_empty()
            })
            .map(|re| re.span.start)
    }
}

impl ModuleGraph {
    /// Find the byte offset of the import statement from `source` to `target`.
    ///
    /// Mixed imports to the same target are stored as one edge. Prefer the
    /// first eager value import, then the first value-carrying import, so
    /// runtime-cycle diagnostics and line suppressions anchor on the import
    /// that actually participates in the cycle. With lazy edges skipped, a
    /// lazy `import()` on a mixed edge is not part of the cycle. An edge that
    /// only a re-export makes anchors on the `export ... from` declaration.
    /// Returns `None` if no edge exists or the edge has no symbols.
    #[must_use]
    pub fn find_import_span_start(&self, source: FileId, target: FileId) -> Option<u32> {
        let idx = source.0 as usize;
        if idx >= self.modules.len() {
            return None;
        }
        let module = &self.modules[idx];
        for edge in &self.edges[module.edge_range.clone()] {
            if edge.target == target {
                let symbol = edge
                    .symbols
                    .iter()
                    .find(|s| s.is_eager_value())
                    .or_else(|| edge.symbols.iter().find(|s| !s.is_type_only))
                    .or_else(|| edge.symbols.first())?;
                if symbol.import_span.is_empty()
                    && let Some(start) = module.re_export_span_start(target, symbol.is_type_only)
                {
                    return Some(start);
                }
                return Some(symbol.import_span.start);
            }
        }
        None
    }
}
