//! Re-exports from `fallow-extract`.
//!
//! All parsing/extraction logic has been moved to the `fallow-extract` crate.
//! This module provides backwards-compatible re-exports so that
//! `fallow_core::extract::*` paths continue to resolve.

// Re-export all types
pub use fallow_extract::{
    ANGULAR_TPL_SENTINEL, DynamicImportInfo, DynamicImportPattern, ExportInfo, ExportName,
    INSTANCE_EXPORT_SENTINEL, ImportInfo, ImportedName, MemberAccess, MemberInfo, MemberKind,
    ModuleInfo, ParseResult, ReExportInfo, RequireCallInfo, VisibilityTag,
};

// Re-export extraction functions
pub use fallow_extract::{
    extract_astro_frontmatter, extract_css_module_exports, extract_mdx_statements,
    extract_sfc_scripts, is_sfc_file, parse_all_files, parse_from_content, parse_single_file,
};

// Re-export sub-modules for code that imports from them directly
pub use fallow_extract::astro;
pub use fallow_extract::css;
pub use fallow_extract::flags;
pub use fallow_extract::go;
pub use fallow_extract::inventory;
pub use fallow_extract::mdx;
pub use fallow_extract::sfc;
pub use fallow_extract::visitor;
