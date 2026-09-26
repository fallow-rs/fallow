//! CSS and stylesheet extraction helpers owned by the engine boundary.

use fallow_extract::CssInJsObjectSheets;
use fallow_extract::css::StylesheetTokens;
use fallow_extract::css_classes::MarkupClassScan;
use fallow_extract::sfc::SfcStyle;
use fallow_extract::tailwind::TailwindArbitraryUse;
use fallow_types::extract::{CssAnalytics, ExportInfo};

/// Scan one stylesheet for Tailwind `@theme` tokens, `@apply` tokens and
/// `var()` reads.
#[must_use]
pub fn scan_stylesheet_tokens(source: &str) -> StylesheetTokens {
    fallow_extract::css::scan_stylesheet_tokens(source)
}

/// Extract CSS module exports from a stylesheet.
#[must_use]
pub fn extract_css_module_exports(source: &str, is_scss: bool) -> Vec<ExportInfo> {
    fallow_extract::css::extract_css_module_exports(source, is_scss)
}

/// Scan markup for static class tokens.
#[must_use]
pub fn scan_markup_class_tokens(source: &str) -> MarkupClassScan {
    fallow_extract::css_classes::scan_markup_class_tokens(source)
}

/// Return whether two class tokens differ by one edit.
#[must_use]
pub fn is_typo_edit(token: &str, defined: &str) -> bool {
    fallow_extract::css_classes::is_typo_edit(token, defined)
}

/// Compute structural CSS analytics for a standard CSS stylesheet.
#[must_use]
pub fn compute_css_analytics(source: &str) -> Option<CssAnalytics> {
    fallow_extract::css_metrics::compute_css_analytics(source)
}

/// Build a virtual stylesheet from CSS-in-JS tagged templates.
#[must_use]
pub fn css_in_js_virtual_stylesheet(source: &str) -> Option<String> {
    fallow_extract::css_in_js_virtual_stylesheet(source)
}

/// Build virtual stylesheets from CSS-in-JS object notation.
#[must_use]
pub fn css_in_js_object_sheets(source: &str, path: &std::path::Path) -> CssInJsObjectSheets {
    fallow_extract::css_in_js_object_sheets(source, path)
}

/// Extract SFC or Astro style blocks.
#[must_use]
pub fn extract_sfc_styles(source: &str) -> Vec<SfcStyle> {
    fallow_extract::sfc::extract_sfc_styles(source)
}

/// Return scoped classes that look unused within one SFC source.
#[must_use]
pub fn scoped_unused_classes(source: &str) -> Vec<String> {
    fallow_extract::sfc_css::scoped_unused_classes(source)
}

/// Build a virtual stylesheet from SFC style blocks.
#[must_use]
pub fn sfc_virtual_stylesheet(source: &str) -> Option<String> {
    fallow_extract::sfc_css::sfc_virtual_stylesheet(source)
}

/// Build a virtual stylesheet from SFC preprocessor style blocks.
#[must_use]
pub fn sfc_preprocessor_virtual_stylesheet(source: &str) -> Option<String> {
    fallow_extract::sfc_css::sfc_preprocessor_virtual_stylesheet(source)
}

/// Scan markup source for Tailwind arbitrary-value utilities.
#[must_use]
pub fn scan_tailwind_arbitrary_values(source: &str) -> Vec<TailwindArbitraryUse> {
    fallow_extract::tailwind::scan_tailwind_arbitrary_values(source)
}
