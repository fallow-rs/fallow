//! Shared graph types: module nodes, re-export edges, export symbols, and references.

use std::ops::Range;
use std::path::PathBuf;

use fallow_types::discover::FileId;
use fallow_types::extract::{ExportName, VisibilityTag};

/// A single module in the graph.
///
/// Boolean flags are packed into a `u8` to keep the struct at 96 bytes
/// (down from 104 with 5 separate `bool` fields), improving cache line
/// utilization in hot graph traversal loops.
#[derive(Debug)]
pub struct ModuleNode {
    /// Unique identifier for this module.
    pub file_id: FileId,
    /// Absolute path to the module file.
    pub path: PathBuf,
    /// Range into the flat `edges` array.
    pub edge_range: Range<usize>,
    /// Exports declared by this module.
    pub exports: Vec<ExportSymbol>,
    /// Re-exports from this module (export { x } from './y', export * from './z').
    pub re_exports: Vec<ReExportEdge>,
    /// Packed boolean flags (entry point, reachability, CJS).
    pub(crate) flags: u8,
}

// Bit positions for packed boolean flags in `ModuleNode::flags`.
const FLAG_ENTRY_POINT: u8 = 1 << 0;
const FLAG_REACHABLE: u8 = 1 << 1;
const FLAG_RUNTIME_REACHABLE: u8 = 1 << 2;
const FLAG_TEST_REACHABLE: u8 = 1 << 3;
const FLAG_CJS_EXPORTS: u8 = 1 << 4;

impl ModuleNode {
    /// Whether this module is an entry point.
    #[inline]
    pub const fn is_entry_point(&self) -> bool {
        self.flags & FLAG_ENTRY_POINT != 0
    }

    /// Whether this module is reachable from any entry point.
    #[inline]
    pub const fn is_reachable(&self) -> bool {
        self.flags & FLAG_REACHABLE != 0
    }

    /// Whether this module is reachable from a runtime/application root.
    #[inline]
    pub const fn is_runtime_reachable(&self) -> bool {
        self.flags & FLAG_RUNTIME_REACHABLE != 0
    }

    /// Whether this module is reachable from a test root.
    #[inline]
    pub const fn is_test_reachable(&self) -> bool {
        self.flags & FLAG_TEST_REACHABLE != 0
    }

    /// Whether this module has CJS exports (module.exports / exports.*).
    #[inline]
    pub const fn has_cjs_exports(&self) -> bool {
        self.flags & FLAG_CJS_EXPORTS != 0
    }

    /// Set whether this module is an entry point.
    #[inline]
    pub fn set_entry_point(&mut self, v: bool) {
        if v {
            self.flags |= FLAG_ENTRY_POINT;
        } else {
            self.flags &= !FLAG_ENTRY_POINT;
        }
    }

    /// Set whether this module is reachable from any entry point.
    #[inline]
    pub fn set_reachable(&mut self, v: bool) {
        if v {
            self.flags |= FLAG_REACHABLE;
        } else {
            self.flags &= !FLAG_REACHABLE;
        }
    }

    /// Set whether this module is reachable from a runtime/application root.
    #[inline]
    pub fn set_runtime_reachable(&mut self, v: bool) {
        if v {
            self.flags |= FLAG_RUNTIME_REACHABLE;
        } else {
            self.flags &= !FLAG_RUNTIME_REACHABLE;
        }
    }

    /// Set whether this module is reachable from a test root.
    #[inline]
    pub fn set_test_reachable(&mut self, v: bool) {
        if v {
            self.flags |= FLAG_TEST_REACHABLE;
        } else {
            self.flags &= !FLAG_TEST_REACHABLE;
        }
    }

    /// Set whether this module has CJS exports.
    #[inline]
    pub fn set_cjs_exports(&mut self, v: bool) {
        if v {
            self.flags |= FLAG_CJS_EXPORTS;
        } else {
            self.flags &= !FLAG_CJS_EXPORTS;
        }
    }

    /// Build flags byte from individual booleans (used by graph construction).
    #[inline]
    pub(crate) fn flags_from(
        is_entry_point: bool,
        is_runtime_reachable: bool,
        has_cjs_exports: bool,
    ) -> u8 {
        let mut f = 0u8;
        if is_entry_point {
            f |= FLAG_ENTRY_POINT;
        }
        if is_runtime_reachable {
            f |= FLAG_RUNTIME_REACHABLE;
        }
        if has_cjs_exports {
            f |= FLAG_CJS_EXPORTS;
        }
        f
    }
}

/// A re-export edge, tracking which exports are forwarded from which module.
#[derive(Debug)]
pub struct ReExportEdge {
    /// The module being re-exported from.
    pub source_file: FileId,
    /// The name imported from the source (or "*" for star re-exports).
    pub imported_name: String,
    /// The name exported from this module.
    pub exported_name: String,
    /// Whether this is a type-only re-export.
    pub is_type_only: bool,
    /// Source span of the re-export declaration on this module, used for
    /// line-number reporting. `(0, 0)` for re-exports synthesized inside the
    /// graph layer (e.g., `export *` chain propagation, namespace narrowing).
    pub span: oxc_span::Span,
}

/// An export with reference tracking.
#[derive(Debug)]
pub struct ExportSymbol {
    /// The exported name (named or default).
    pub name: ExportName,
    /// Whether this is a type-only export.
    pub is_type_only: bool,
    /// Whether this export is registered through a runtime side effect at module
    /// load time (e.g. a Lit `@customElement('tag')` decorator or a
    /// `customElements.define('tag', ClassRef)` call). The unused-export
    /// detector treats this as an effective reference.
    pub is_side_effect_used: bool,
    /// Visibility tag from JSDoc/TSDoc comment (`@public`, `@internal`, `@alpha`, `@beta`).
    /// Exports with any visibility tag are never reported as unused.
    pub visibility: VisibilityTag,
    /// Source span of the export declaration.
    pub span: oxc_span::Span,
    /// Which files reference this export.
    pub references: Vec<SymbolReference>,
    /// Members of this export (enum members, class members).
    pub members: Vec<fallow_types::extract::MemberInfo>,
}

/// A reference to an export from another file.
#[derive(Debug, Clone, Copy)]
pub struct SymbolReference {
    /// The file that references this export.
    pub from_file: FileId,
    /// How the export is referenced.
    pub kind: ReferenceKind,
    /// Byte span of the import statement in the referencing file.
    /// Used by the LSP to locate references for Code Lens navigation.
    pub import_span: oxc_span::Span,
}

/// How an export is referenced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReferenceKind {
    /// A named import (`import { foo }`).
    NamedImport,
    /// A default import (`import Foo`).
    DefaultImport,
    /// A namespace import (`import * as ns`).
    NamespaceImport,
    /// A re-export (`export { foo } from './bar'`).
    ReExport,
    /// A dynamic import (`import('./foo')`).
    DynamicImport,
    /// A side-effect import (`import './styles'`).
    SideEffectImport,
}

// Size assertions for types defined in this module.
// `ExportSymbol` and `SymbolReference` are stored in Vecs per module node.
// `ReExportEdge` is stored in a Vec per module for re-export chain resolution.
#[cfg(target_pointer_width = "64")]
const _: () = assert!(std::mem::size_of::<ExportSymbol>() == 88);
#[cfg(target_pointer_width = "64")]
const _: () = assert!(std::mem::size_of::<SymbolReference>() == 16);
#[cfg(target_pointer_width = "64")]
const _: () = assert!(std::mem::size_of::<ReExportEdge>() == 64);
// `ModuleNode` is stored in a Vec — one per discovered file.
// PathBuf has different sizes on Unix vs Windows, so restrict to Unix.
#[cfg(all(target_pointer_width = "64", unix))]
const _: () = assert!(std::mem::size_of::<ModuleNode>() == 96);

#[cfg(test)]
mod tests {
    use super::*;

    // ── ReferenceKind ───────────────────────────────────────────

    #[test]
    fn reference_kind_equality() {
        assert_eq!(ReferenceKind::NamedImport, ReferenceKind::NamedImport);
        assert_ne!(ReferenceKind::NamedImport, ReferenceKind::DefaultImport);
    }

    #[test]
    fn reference_kind_all_variants_are_distinct() {
        let all = [
            ReferenceKind::NamedImport,
            ReferenceKind::DefaultImport,
            ReferenceKind::NamespaceImport,
            ReferenceKind::ReExport,
            ReferenceKind::DynamicImport,
            ReferenceKind::SideEffectImport,
        ];
        for (i, a) in all.iter().enumerate() {
            for (j, b) in all.iter().enumerate() {
                if i == j {
                    assert_eq!(a, b);
                } else {
                    assert_ne!(a, b);
                }
            }
        }
    }

    #[test]
    fn reference_kind_copy() {
        let original = ReferenceKind::NamespaceImport;
        let copied = original;
        assert_eq!(original, copied);
    }

    #[test]
    fn reference_kind_debug_format() {
        let kind = ReferenceKind::DynamicImport;
        let debug_str = format!("{kind:?}");
        assert_eq!(debug_str, "DynamicImport");
    }

    // ── SymbolReference ─────────────────────────────────────────

    #[test]
    fn symbol_reference_construction() {
        let reference = SymbolReference {
            from_file: FileId(42),
            kind: ReferenceKind::NamedImport,
            import_span: oxc_span::Span::new(10, 30),
        };
        assert_eq!(reference.from_file, FileId(42));
        assert_eq!(reference.kind, ReferenceKind::NamedImport);
        assert_eq!(reference.import_span.start, 10);
        assert_eq!(reference.import_span.end, 30);
    }

    #[test]
    fn symbol_reference_copy_preserves_all_fields() {
        let reference = SymbolReference {
            from_file: FileId(7),
            kind: ReferenceKind::ReExport,
            import_span: oxc_span::Span::new(5, 25),
        };
        let copied = reference;
        // Verify the copy matches the original
        assert_eq!(copied.from_file, reference.from_file);
        assert_eq!(copied.kind, reference.kind);
        assert_eq!(copied.import_span.start, reference.import_span.start);
        assert_eq!(copied.import_span.end, reference.import_span.end);
    }

    // ── ReExportEdge ────────────────────────────────────────────

    #[test]
    fn re_export_edge_construction() {
        let edge = ReExportEdge {
            source_file: FileId(3),
            imported_name: "*".to_string(),
            exported_name: "*".to_string(),
            is_type_only: false,
            span: oxc_span::Span::default(),
        };
        assert_eq!(edge.source_file, FileId(3));
        assert_eq!(edge.imported_name, "*");
        assert_eq!(edge.exported_name, "*");
        assert!(!edge.is_type_only);
    }

    #[test]
    fn re_export_edge_type_only() {
        let edge = ReExportEdge {
            source_file: FileId(1),
            imported_name: "MyType".to_string(),
            exported_name: "MyType".to_string(),
            is_type_only: true,
            span: oxc_span::Span::default(),
        };
        assert!(edge.is_type_only);
    }

    #[test]
    fn re_export_edge_renamed() {
        let edge = ReExportEdge {
            source_file: FileId(2),
            imported_name: "internal".to_string(),
            exported_name: "public".to_string(),
            is_type_only: false,
            span: oxc_span::Span::default(),
        };
        assert_ne!(edge.imported_name, edge.exported_name);
        assert_eq!(edge.imported_name, "internal");
        assert_eq!(edge.exported_name, "public");
    }

    // ── ExportSymbol ────────────────────────────────────────────

    #[test]
    fn export_symbol_named() {
        let sym = ExportSymbol {
            name: ExportName::Named("myFunction".to_string()),
            is_type_only: false,
            is_side_effect_used: false,
            visibility: VisibilityTag::None,
            span: oxc_span::Span::new(0, 50),
            references: vec![],
            members: vec![],
        };
        assert!(matches!(sym.name, ExportName::Named(ref n) if n == "myFunction"));
        assert!(!sym.is_type_only);
        assert_eq!(sym.visibility, VisibilityTag::None);
    }

    #[test]
    fn export_symbol_default() {
        let sym = ExportSymbol {
            name: ExportName::Default,
            is_type_only: false,
            is_side_effect_used: false,
            visibility: VisibilityTag::None,
            span: oxc_span::Span::new(0, 20),
            references: vec![],
            members: vec![],
        };
        assert!(matches!(sym.name, ExportName::Default));
    }

    #[test]
    fn export_symbol_public_tag() {
        let sym = ExportSymbol {
            name: ExportName::Named("api".to_string()),
            is_type_only: false,
            is_side_effect_used: false,
            visibility: VisibilityTag::Public,
            span: oxc_span::Span::new(0, 10),
            references: vec![],
            members: vec![],
        };
        assert_eq!(sym.visibility, VisibilityTag::Public);
    }

    #[test]
    fn export_symbol_type_only() {
        let sym = ExportSymbol {
            name: ExportName::Named("MyInterface".to_string()),
            is_type_only: true,
            is_side_effect_used: false,
            visibility: VisibilityTag::None,
            span: oxc_span::Span::new(0, 30),
            references: vec![],
            members: vec![],
        };
        assert!(sym.is_type_only);
    }

    #[test]
    fn export_symbol_with_references() {
        let sym = ExportSymbol {
            name: ExportName::Named("helper".to_string()),
            is_type_only: false,
            is_side_effect_used: false,
            visibility: VisibilityTag::None,
            span: oxc_span::Span::new(0, 20),
            references: vec![
                SymbolReference {
                    from_file: FileId(1),
                    kind: ReferenceKind::NamedImport,
                    import_span: oxc_span::Span::new(0, 10),
                },
                SymbolReference {
                    from_file: FileId(2),
                    kind: ReferenceKind::ReExport,
                    import_span: oxc_span::Span::new(5, 15),
                },
            ],
            members: vec![],
        };
        assert_eq!(sym.references.len(), 2);
        assert_eq!(sym.references[0].from_file, FileId(1));
        assert_eq!(sym.references[1].kind, ReferenceKind::ReExport);
    }

    // ── ModuleNode ──────────────────────────────────────────────

    #[test]
    fn module_node_construction() {
        let mut node = ModuleNode {
            file_id: FileId(0),
            path: PathBuf::from("/project/src/index.ts"),
            edge_range: 0..5,
            exports: vec![],
            re_exports: vec![],
            flags: ModuleNode::flags_from(true, true, false),
        };
        node.set_reachable(true);
        assert_eq!(node.file_id, FileId(0));
        assert!(node.is_entry_point());
        assert!(node.is_reachable());
        assert!(node.is_runtime_reachable());
        assert!(!node.is_test_reachable());
        assert!(!node.has_cjs_exports());
        assert_eq!(node.edge_range, 0..5);
    }

    #[test]
    fn module_node_non_entry_unreachable() {
        let node = ModuleNode {
            file_id: FileId(5),
            path: PathBuf::from("/project/src/orphan.ts"),
            edge_range: 0..0,
            exports: vec![],
            re_exports: vec![],
            flags: ModuleNode::flags_from(false, false, false),
        };
        assert!(!node.is_entry_point());
        assert!(!node.is_reachable());
        assert!(!node.is_runtime_reachable());
        assert!(!node.is_test_reachable());
        assert!(node.edge_range.is_empty());
    }

    #[test]
    fn module_node_cjs_exports() {
        let mut node = ModuleNode {
            file_id: FileId(2),
            path: PathBuf::from("/project/lib/legacy.js"),
            edge_range: 3..7,
            exports: vec![],
            re_exports: vec![],
            flags: ModuleNode::flags_from(false, true, true),
        };
        node.set_reachable(true);
        assert!(node.has_cjs_exports());
        assert!(node.is_runtime_reachable());
        assert_eq!(node.edge_range.len(), 4);
    }

    #[test]
    fn module_node_with_exports_and_re_exports() {
        let node = ModuleNode {
            file_id: FileId(1),
            path: PathBuf::from("/project/src/barrel.ts"),
            edge_range: 0..3,
            exports: vec![ExportSymbol {
                name: ExportName::Named("localFn".to_string()),
                is_type_only: false,
                is_side_effect_used: false,
                visibility: VisibilityTag::None,
                span: oxc_span::Span::new(0, 20),
                references: vec![],
                members: vec![],
            }],
            re_exports: vec![ReExportEdge {
                source_file: FileId(2),
                imported_name: "*".to_string(),
                exported_name: "*".to_string(),
                is_type_only: false,
                span: oxc_span::Span::default(),
            }],
            flags: ModuleNode::flags_from(false, true, false),
        };
        assert_eq!(node.exports.len(), 1);
        assert_eq!(node.re_exports.len(), 1);
        assert_eq!(node.re_exports[0].source_file, FileId(2));
    }
}
