//! Complexity of SFC scripts in the coordinates of the full source file.

use std::path::Path;

use fallow_types::discover::FileId;
use fallow_types::extract::ModuleInfo;

use crate::parse_source_to_module;

fn parse_sfc_with_complexity(source: &str, filename: &str) -> ModuleInfo {
    parse_source_to_module(FileId(0), Path::new(filename), source, 0, true)
}

#[test]
fn vue_script_setup_complexity_maps_to_sfc_lines() {
    let info = parse_sfc_with_complexity(
        r#"<template />
<script setup lang="ts">
const helper = () => {
  if (flag) return 1;
  return 0;
};
</script>
"#,
        "Complexity.vue",
    );

    assert_eq!(info.complexity.len(), 1);
    let function = &info.complexity[0];
    assert_eq!(function.name, "helper");
    assert_eq!(function.line, 3);
    assert_eq!(function.col, 15);
    assert_eq!(function.cyclomatic, 2);
}

#[test]
fn vue_inline_script_complexity_maps_columns_to_sfc_source() {
    let info = parse_sfc_with_complexity(
        r"<script>const helper = () => { if (flag) return 1; return 0; };</script>",
        "InlineComplexity.vue",
    );

    assert_eq!(info.complexity.len(), 1);
    let function = &info.complexity[0];
    assert_eq!(function.name, "helper");
    assert_eq!(function.line, 1);
    assert_eq!(function.col, 23);
}

#[test]
fn vue_script_complexity_contributions_map_to_sfc_lines() {
    let info = parse_sfc_with_complexity(
        r#"<template />
<script setup lang="ts">
const helper = () => {
  if (flag) return 1;
  return 0;
};
</script>
"#,
        "Contributions.vue",
    );

    let function = &info.complexity[0];
    assert!(!function.contributions.is_empty(), "{function:?}");
    for contribution in &function.contributions {
        assert_eq!(contribution.line, 4, "{contribution:?}");
        assert_eq!(contribution.col, 2, "{contribution:?}");
    }
}

#[test]
fn inline_script_complexity_contributions_map_columns_to_sfc_source() {
    let info = parse_sfc_with_complexity(
        r"<script>const helper = () => { if (flag) return 1; return 0; };</script>",
        "InlineContributions.vue",
    );

    let function = &info.complexity[0];
    assert!(!function.contributions.is_empty(), "{function:?}");
    for contribution in &function.contributions {
        assert_eq!(contribution.line, 1, "{contribution:?}");
        assert_eq!(contribution.col, 31, "{contribution:?}");
    }
}
