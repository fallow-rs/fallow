//! Re-exports from `fallow-extract`.
//!
//! All parsing/extraction logic has been moved to the `fallow-extract` crate.
//! This module provides backwards-compatible re-exports so that
//! `fallow_core::extract::*` paths continue to resolve.

// Re-export all types
pub use fallow_extract::{
    ANGULAR_TPL_SENTINEL, DynamicImportInfo, DynamicImportPattern, ExportInfo, ExportName,
    FACTORY_CALL_SENTINEL, FLUENT_CHAIN_NEW_SENTINEL, FLUENT_CHAIN_SENTINEL,
    INSTANCE_EXPORT_SENTINEL, ImportInfo, ImportedName, MemberAccess, MemberInfo, MemberKind,
    ModuleInfo, PLAYWRIGHT_FIXTURE_DEF_SENTINEL, PLAYWRIGHT_FIXTURE_USE_SENTINEL, ParseResult,
    ReExportInfo, RequireCallInfo, VisibilityTag,
};

// Re-export extraction functions
pub use fallow_extract::{
    extract_astro_frontmatter, extract_css_module_exports, extract_mdx_statements,
    extract_sfc_scripts, is_glimmer_file, is_sfc_file, parse_all_files, parse_from_content,
    parse_single_file, strip_glimmer_templates,
};

// Re-export sub-modules for code that imports from them directly
pub use fallow_extract::astro;
pub use fallow_extract::css;
pub use fallow_extract::flags;
pub use fallow_extract::inventory;
pub use fallow_extract::mdx;
pub use fallow_extract::sfc;
pub use fallow_extract::visitor;
