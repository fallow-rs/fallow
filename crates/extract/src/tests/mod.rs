mod astro;
mod css;
mod graphql;
mod js_ts;
mod mdx;
mod regex_compile;
mod sfc;

use std::path::Path;

use fallow_types::discover::FileId;
use fallow_types::extract::ModuleInfo;

use crate::parse::parse_source_to_module;

/// Shared test helper: parse TypeScript source and return `ModuleInfo`.
pub fn parse_ts(source: &str) -> ModuleInfo {
    parse_source_to_module(FileId(0), Path::new("test.ts"), source, 0, false)
}

/// Shared test helper: parse TypeScript source with complexity metrics.
pub fn parse_ts_with_complexity(source: &str) -> ModuleInfo {
    parse_source_to_module(FileId(0), Path::new("test.ts"), source, 0, true)
}

/// Shared test helper: parse TSX source and return `ModuleInfo`.
pub fn parse_tsx(source: &str) -> ModuleInfo {
    parse_source_to_module(FileId(0), Path::new("test.tsx"), source, 0, false)
}

#[test]
fn parses_glimmer_typescript_as_typescript() {
    let info = parse_source_to_module(
        FileId(0),
        Path::new("component.gts"),
        "import type Service from './service';\nexport type ServiceRef = Service;\n",
        0,
        false,
    );

    assert_eq!(info.imports.len(), 1);
    assert_eq!(info.imports[0].source, "./service");
    assert!(info.imports[0].is_type_only);
    assert!(
        info.exports
            .iter()
            .any(|export| export.name.matches_str("ServiceRef"))
    );
}

/// Regression test for issue #375: a `.gts` file containing both a
/// module-level template expression (assigned to const) and a class-body
/// template must still parse all imports and the default export.
///
/// Before the context-aware stripping fix, the module-level template was
/// blanked to spaces, leaving `const Wrapper: TOC<...> = ;` which is a
/// TypeScript syntax error. oxc bailed and returned zero imports, causing
/// every referenced component to be reported as unused.
#[test]
fn parses_gts_with_multi_template_blocks() {
    let source = "import type {TOC} from '@ember/component/template-only';\n\
                  import Component from '@glimmer/component';\n\
                  import BillingInfo from 'my-app/components/billing-info';\n\
                  \n\
                  const Wrapper: TOC<{ Blocks: { default: [] } }> = <template>\n  <div class=\"wrapper\">{{yield}}</div>\n</template>;\n\
                  \n\
                  export default class InvoiceDetails extends Component {\n  <template>\n    <Wrapper>\n      <BillingInfo />\n    </Wrapper>\n  </template>\n}\n";

    let info = parse_source_to_module(
        FileId(0),
        Path::new("invoice-details.gts"),
        source,
        0,
        false,
    );

    assert_eq!(
        info.imports.len(),
        3,
        "all three import statements should be extracted; got {:?}",
        info.imports.iter().map(|i| &i.source).collect::<Vec<_>>()
    );
    assert!(
        info.imports
            .iter()
            .any(|i| i.source == "@ember/component/template-only"),
    );
    assert!(
        info.imports
            .iter()
            .any(|i| i.source == "@glimmer/component")
    );
    assert!(
        info.imports
            .iter()
            .any(|i| i.source == "my-app/components/billing-info"),
    );
    assert!(
        info.exports
            .iter()
            .any(|e| matches!(e.name, fallow_types::extract::ExportName::Default)),
        "default export should be extracted",
    );
}

/// Regression test for issue #379: a `.gts` file that uses the canonical
/// template-only-component shape (`export default <template>...</template>`
/// with no `const` wrapper) must still parse the import statement and the
/// default export.
///
/// Before the keyword-aware `is_expression_position` fix, the previous
/// non-whitespace byte before `<template>` was `t` (end of `default`),
/// which fell through to blank-out and left `export default ;`, a
/// TypeScript syntax error that made oxc bail and drop every import.
#[test]
fn parses_gts_with_standalone_default_template() {
    let source = "import Icon from 'my-app/components/icon';\n\
                  \n\
                  export default <template>\n  <span class=\"badge\"><Icon /> badge</span>\n</template>\n";

    let info = parse_source_to_module(FileId(0), Path::new("badge.gts"), source, 0, false);

    assert_eq!(
        info.imports.len(),
        1,
        "import statement should be extracted; got {:?}",
        info.imports.iter().map(|i| &i.source).collect::<Vec<_>>()
    );
    assert_eq!(info.imports[0].source, "my-app/components/icon");
    assert!(
        info.exports
            .iter()
            .any(|e| matches!(e.name, fallow_types::extract::ExportName::Default)),
        "default export should be extracted",
    );
}

/// Issue #475: the same source bytes with and without a leading UTF-8 BOM
/// must produce identical extraction results. `parse_source_to_module` strips
/// the BOM as a defense-in-depth step before any line-offset computation or
/// oxc parse, so the byte spans on every export entry must match.
#[test]
fn parse_source_to_module_strips_bom_defense_in_depth() {
    let body = "import { foo } from './foo';\nexport const bar = 1;\n";
    let with_bom = format!("\u{FEFF}{body}");
    let info_plain = parse_ts(body);
    let info_bom = parse_ts(&with_bom);

    assert_eq!(info_plain.imports.len(), info_bom.imports.len());
    assert_eq!(info_plain.exports.len(), info_bom.exports.len());
    // Compare byte spans on every export entry: identical post-strip source
    // must produce identical spans.
    let plain_spans: Vec<(u32, u32)> = info_plain
        .exports
        .iter()
        .map(|e| (e.span.start, e.span.end))
        .collect();
    let bom_spans: Vec<(u32, u32)> = info_bom
        .exports
        .iter()
        .map(|e| (e.span.start, e.span.end))
        .collect();
    assert_eq!(
        plain_spans, bom_spans,
        "BOM-bearing source must produce identical export byte spans (no shift by the BOM codepoint)",
    );
}

/// Issue #475: confirm `strip_bom` + hash invariant: the post-strip bytes
/// of a BOM-bearing source equal the post-strip bytes of the same source
/// without BOM, so the cache `content_hash` (xxh3 over post-strip bytes)
/// matches and the cache hits on both shapes.
#[test]
fn bom_stripped_before_hash_so_with_and_without_bom_yield_same_hash() {
    let body = "export const x = 1;\n";
    let plain = body;
    let bom = format!("\u{FEFF}{body}");

    let plain_hash = xxhash_rust::xxh3::xxh3_64(crate::strip_bom(plain).as_bytes());
    let bom_hash = xxhash_rust::xxh3::xxh3_64(crate::strip_bom(&bom).as_bytes());
    assert_eq!(
        plain_hash, bom_hash,
        "post-strip hashes must match so the extraction cache hits regardless of BOM presence",
    );

    // The `ModuleInfo.content_hash` field carried by `parse_source_to_module`
    // is set by the caller; confirm the caller's invariant by hashing the
    // same post-strip bytes we'd pass to the parser.
    let plain_info = parse_ts(plain);
    let bom_info = parse_ts(&bom);
    // Both calls go through `parse_source_to_module`, which now strips BOM
    // defense-in-depth. The exported byte spans + member layout must match.
    assert_eq!(
        plain_info.exports.len(),
        bom_info.exports.len(),
        "BOM-bearing and BOM-free source must yield the same number of exports",
    );
}

/// Issue #475: `compute_line_offsets` runs against the post-BOM source, so
/// line numbers for symbols on line 1 are not shifted by the BOM codepoint.
/// This is the user-visible fix: the first reported export of a BOM-bearing
/// file lands on line 1 col 0 (not line 1 col 3).
#[test]
fn bom_stripped_before_line_offsets_so_line_numbers_align() {
    use fallow_types::extract::{byte_offset_to_line_col, compute_line_offsets};

    let body = "export const first = 1;\nexport const second = 2;\n";
    let with_bom = format!("\u{FEFF}{body}");
    let info_plain = parse_ts(body);
    let info_bom = parse_ts(&with_bom);

    let plain_first = info_plain
        .exports
        .iter()
        .find(|e| e.name.matches_str("first"))
        .expect("plain source exports `first`");
    let bom_first = info_bom
        .exports
        .iter()
        .find(|e| e.name.matches_str("first"))
        .expect("BOM-bearing source exports `first`");

    // The byte spans must be identical (the BOM is stripped before parse),
    // and the line/col mapping on the post-strip source produces (1, 0).
    let plain_offsets = compute_line_offsets(body);
    let bom_offsets = compute_line_offsets(crate::strip_bom(&with_bom));
    let plain_pos = byte_offset_to_line_col(&plain_offsets, plain_first.span.start);
    let bom_pos = byte_offset_to_line_col(&bom_offsets, bom_first.span.start);
    assert_eq!(
        plain_pos, bom_pos,
        "line/col must align across BOM presence"
    );
    assert_eq!(
        plain_pos.0, 1,
        "the first export sits on line 1 in both views",
    );
}
