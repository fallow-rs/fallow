use std::path::Path;

use fallow_types::results::AnalysisResults;

use super::{
    GroupedSectionInput, MAX_FLAT_ITEMS, build_human_grouped_section, format_duplicate_prop_shape,
    format_prop_drilling_chain, format_thin_wrapper, prop_drilling_anchor,
};

pub(super) fn push_react_component_health_sections(
    lines: &mut Vec<String>,
    results: &AnalysisResults,
    root: &Path,
) {
    build_human_grouped_section(GroupedSectionInput {
        lines,
        items: &results.prop_drilling_chains,
        title: "Prop drilling",
        level: if results.enforced_prop_drilling_count() > 0 {
            crate::report::Level::Error
        } else {
            crate::report::Level::Warn
        },
        root,
        max_files: MAX_FLAT_ITEMS,
        get_path: prop_drilling_anchor,
        format_detail: &format_prop_drilling_chain,
    });

    build_human_grouped_section(GroupedSectionInput {
        lines,
        items: &results.thin_wrappers,
        title: "Thin wrappers",
        level: if results.enforced_thin_wrapper_count() > 0 {
            crate::report::Level::Error
        } else {
            crate::report::Level::Warn
        },
        root,
        max_files: MAX_FLAT_ITEMS,
        get_path: |w: &fallow_types::output_dead_code::ThinWrapperFinding| w.wrapper.file.as_path(),
        format_detail: &format_thin_wrapper,
    });

    build_human_grouped_section(GroupedSectionInput {
        lines,
        items: &results.duplicate_prop_shapes,
        title: "Duplicate prop shapes",
        level: if results.enforced_duplicate_prop_shape_count() > 0 {
            crate::report::Level::Error
        } else {
            crate::report::Level::Warn
        },
        root,
        max_files: MAX_FLAT_ITEMS,
        get_path: |d: &fallow_types::output_dead_code::DuplicatePropShapeFinding| {
            d.shape.file.as_path()
        },
        format_detail: &format_duplicate_prop_shape,
    });
}
